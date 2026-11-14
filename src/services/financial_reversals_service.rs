use crate::models::financial_reversals::{
    CreditNoteReversalRequest, PaymentReversalRequest, ReversalOutcome,
};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ReversalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid reversal request")]
    Invalid,
    #[error("source financial event not found")]
    NotFound,
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict,
}

#[derive(Clone, Copy)]
enum Ledger {
    Customer,
    Supplier,
}

impl Ledger {
    fn party_table(self) -> &'static str {
        match self {
            Self::Customer => "customers",
            Self::Supplier => "suppliers",
        }
    }
    fn party_column(self) -> &'static str {
        match self {
            Self::Customer => "customer_id",
            Self::Supplier => "supplier_id",
        }
    }
    fn payment_table(self) -> &'static str {
        match self {
            Self::Customer => "customer_payments",
            Self::Supplier => "supplier_payments",
        }
    }
    fn allocation_table(self) -> &'static str {
        match self {
            Self::Customer => "customer_payment_allocations",
            Self::Supplier => "supplier_payment_allocations",
        }
    }
    fn allocation_reversal_table(self) -> &'static str {
        match self {
            Self::Customer => "customer_allocation_reversals",
            Self::Supplier => "supplier_allocation_reversals",
        }
    }
    fn payment_reversal_table(self) -> &'static str {
        match self {
            Self::Customer => "customer_payment_reversals",
            Self::Supplier => "supplier_payment_reversals",
        }
    }
    fn target_table(self) -> &'static str {
        match self {
            Self::Customer => "sales",
            Self::Supplier => "supplier_invoices",
        }
    }
    fn target_column(self) -> &'static str {
        match self {
            Self::Customer => "sale_id",
            Self::Supplier => "invoice_id",
        }
    }
    fn reversal_permission(self) -> &'static str {
        match self {
            Self::Customer => "customers.payment",
            Self::Supplier => "suppliers.payment",
        }
    }
}

async fn require_permission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company: Uuid,
    actor: Uuid,
    permission: &str,
) -> Result<(), ReversalError> {
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE ur.user_id=$1 AND ur.company_id=$2 AND r.is_active=TRUE AND (ur.expires_at IS NULL OR ur.expires_at>NOW()) AND p.permission_key=$3)")
        .bind(actor).bind(company).bind(permission).fetch_one(&mut **tx).await?;
    if allowed {
        Ok(())
    } else {
        Err(ReversalError::Invalid)
    }
}

fn payment_fingerprint(
    payment_id: i64,
    request: &PaymentReversalRequest,
) -> Result<String, ReversalError> {
    let mut allocations = request
        .allocations
        .iter()
        .map(|a| (a.allocation_id, a.amount, a.idempotency_key))
        .collect::<Vec<_>>();
    allocations.sort_by_key(|a| a.0);
    serde_json::to_string(&serde_json::json!({
        "payment_id": payment_id,
        "amount": request.amount.to_string(),
        "payment_method": request.payment_method,
        "cash_session_id": request.cash_session_id,
        "reason": request.reason.trim(),
        "allocations": allocations.iter().map(|(id, amount, key)| serde_json::json!({
            "allocation_id": id, "amount": amount.to_string(), "idempotency_key": key
        })).collect::<Vec<_>>()
    }))
    .map_err(|_| ReversalError::Invalid)
}

async fn find_replay(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    table: &str,
    company: Uuid,
    key: Uuid,
    fingerprint: &str,
) -> Result<Option<ReversalOutcome>, ReversalError> {
    let sql = format!(
        "SELECT id,request_fingerprint FROM {table} WHERE company_id=$1 AND idempotency_key=$2"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(company)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(row) = row {
        let stored: String = row.try_get("request_fingerprint")?;
        if stored != fingerprint {
            return Err(ReversalError::IdempotencyConflict);
        }
        return Ok(Some(ReversalOutcome {
            reversal_id: row.try_get("id")?,
            replayed: true,
        }));
    }
    Ok(None)
}

/// Reverse a payment and any allocations in one transaction. Lock order:
/// party -> payment -> target documents sorted by ID -> allocation rows sorted
/// by ID -> cash session (the database trigger locks the session on insert).
async fn reverse_payment(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    payment_id: i64,
    request: PaymentReversalRequest,
    ledger: Ledger,
) -> Result<ReversalOutcome, ReversalError> {
    if request.amount <= Decimal::ZERO
        || request.reason.trim().is_empty()
        || !["cash", "card", "mobile_money", "other"].contains(&request.payment_method.as_str())
        || ((request.payment_method == "cash") != request.cash_session_id.is_some())
    {
        return Err(ReversalError::Invalid);
    }
    let mut unique = request
        .allocations
        .iter()
        .map(|a| a.allocation_id)
        .collect::<Vec<_>>();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != request.allocations.len()
        || request
            .allocations
            .iter()
            .any(|a| a.amount <= Decimal::ZERO)
    {
        return Err(ReversalError::Invalid);
    }
    let fingerprint = payment_fingerprint(payment_id, &request)?;
    let payment_reversal_table = ledger.payment_reversal_table();
    let allocation_table = ledger.allocation_table();
    let allocation_reversal_table = ledger.allocation_reversal_table();
    let target_table = ledger.target_table();
    let target_column = ledger.target_column();
    let party_table = ledger.party_table();
    let party_column = ledger.party_column();

    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, ledger.reversal_permission()).await?;
    if let Some(replay) = find_replay(
        &mut tx,
        payment_reversal_table,
        company,
        request.idempotency_key,
        &fingerprint,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(replay);
    }

    let party_sql = format!(
        "SELECT {party_column} FROM {} WHERE id=$1 AND company_id=$2",
        ledger.payment_table()
    );
    let party_id: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(party_sql))
        .bind(payment_id)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ReversalError::NotFound)?;
    let lock_party =
        format!("SELECT id FROM {party_table} WHERE id=$1 AND company_id=$2 FOR UPDATE");
    if sqlx::query(sqlx::AssertSqlSafe(lock_party))
        .bind(party_id)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .is_none()
    {
        return Err(ReversalError::NotFound);
    }
    let payment_lock = format!(
        "SELECT amount,btrim(currency_code)::text AS currency_code FROM {} WHERE id=$1 AND company_id=$2 FOR UPDATE",
        ledger.payment_table()
    );
    let payment = sqlx::query(sqlx::AssertSqlSafe(payment_lock))
        .bind(payment_id)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ReversalError::NotFound)?;
    let currency: String = payment.try_get("currency_code")?;

    // Target documents are locked in ascending order before allocation rows.
    let targets_sql = format!(
        "SELECT {target_column} FROM {allocation_table} WHERE company_id=$1 AND payment_id=$2 AND id=ANY($3) ORDER BY {target_column}"
    );
    let target_ids: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(targets_sql))
        .bind(company)
        .bind(payment_id)
        .bind(&unique)
        .fetch_all(&mut *tx)
        .await?;
    if target_ids.len() != unique.len() {
        return Err(ReversalError::NotFound);
    }
    for target_id in target_ids {
        let lock =
            format!("SELECT id FROM {target_table} WHERE id=$1 AND company_id=$2 FOR UPDATE");
        if sqlx::query(sqlx::AssertSqlSafe(lock))
            .bind(target_id)
            .bind(company)
            .fetch_optional(&mut *tx)
            .await?
            .is_none()
        {
            return Err(ReversalError::NotFound);
        }
    }
    let allocations_lock = format!(
        "SELECT id FROM {allocation_table} WHERE company_id=$1 AND payment_id=$2 AND id=ANY($3) ORDER BY id FOR UPDATE"
    );
    let locked: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(allocations_lock))
        .bind(company)
        .bind(payment_id)
        .bind(&unique)
        .fetch_all(&mut *tx)
        .await?;
    if locked != unique {
        return Err(ReversalError::NotFound);
    }

    // Re-check after acquiring the source payment lock to serialize retries.
    if let Some(replay) = find_replay(
        &mut tx,
        payment_reversal_table,
        company,
        request.idempotency_key,
        &fingerprint,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(replay);
    }

    for allocation in &request.allocations {
        let allocation_fingerprint = serde_json::json!({
            "payment_reversal": request.idempotency_key,
            "payment_id": payment_id,
            "allocation_id": allocation.allocation_id,
            "amount": allocation.amount.to_string(),
            "reason": request.reason.trim(),
        })
        .to_string();
        let insert = format!(
            "INSERT INTO {allocation_reversal_table}(company_id,allocation_id,payment_id,{target_column},{party_column},currency_code,amount,reason,idempotency_key,request_fingerprint,reversed_by) SELECT company_id,id,payment_id,{target_column},{party_column},currency_code,$4,$5,$6,$7,$8 FROM {allocation_table} WHERE id=$1 AND company_id=$2 AND payment_id=$3 ON CONFLICT(company_id,idempotency_key) DO NOTHING"
        );
        let result = sqlx::query(sqlx::AssertSqlSafe(insert))
            .bind(allocation.allocation_id)
            .bind(company)
            .bind(payment_id)
            .bind(allocation.amount)
            .bind(request.reason.trim())
            .bind(allocation.idempotency_key)
            .bind(&allocation_fingerprint)
            .bind(actor)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            let check = format!(
                "SELECT request_fingerprint FROM {allocation_reversal_table} WHERE company_id=$1 AND idempotency_key=$2"
            );
            let stored: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(check))
                .bind(company)
                .bind(allocation.idempotency_key)
                .fetch_optional(&mut *tx)
                .await?;
            if stored.as_deref() != Some(allocation_fingerprint.as_str()) {
                return Err(ReversalError::IdempotencyConflict);
            }
        }
    }

    let payment_insert = format!(
        "INSERT INTO {payment_reversal_table}(company_id,payment_id,{party_column},currency_code,amount,payment_method,cash_session_id,reason,idempotency_key,request_fingerprint,reversed_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(company_id,idempotency_key) DO NOTHING RETURNING id"
    );
    let inserted: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(payment_insert))
        .bind(company)
        .bind(payment_id)
        .bind(party_id)
        .bind(currency)
        .bind(request.amount)
        .bind(request.payment_method)
        .bind(request.cash_session_id)
        .bind(request.reason.trim())
        .bind(request.idempotency_key)
        .bind(&fingerprint)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await?;
    let (reversal_id, replayed) = if let Some(id) = inserted {
        (id, false)
    } else {
        let replay = find_replay(
            &mut tx,
            payment_reversal_table,
            company,
            request.idempotency_key,
            &fingerprint,
        )
        .await?
        .ok_or(ReversalError::IdempotencyConflict)?;
        (replay.reversal_id, true)
    };
    tx.commit().await?;
    Ok(ReversalOutcome {
        reversal_id,
        replayed,
    })
}

pub async fn reverse_customer_payment(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    payment_id: i64,
    request: PaymentReversalRequest,
) -> Result<ReversalOutcome, ReversalError> {
    reverse_payment(db, company, actor, payment_id, request, Ledger::Customer).await
}
pub async fn reverse_supplier_payment(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    payment_id: i64,
    request: PaymentReversalRequest,
) -> Result<ReversalOutcome, ReversalError> {
    reverse_payment(db, company, actor, payment_id, request, Ledger::Supplier).await
}

/// Credit-note reversals restore the account balance but never touch cash.
pub async fn reverse_credit_note(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    credit_note_id: i64,
    request: CreditNoteReversalRequest,
    customer: bool,
) -> Result<ReversalOutcome, ReversalError> {
    if request.amount <= Decimal::ZERO || request.reason.trim().is_empty() {
        return Err(ReversalError::Invalid);
    }
    let fingerprint=serde_json::json!({"credit_note_id":credit_note_id,"amount":request.amount.to_string(),"reason":request.reason.trim()}).to_string();
    let (table, party, party_col, source, source_col) = if customer {
        (
            "customer_credit_note_reversals",
            "customers",
            "customer_id",
            "customer_credit_notes",
            "sale_id",
        )
    } else {
        (
            "supplier_credit_note_reversals",
            "suppliers",
            "supplier_id",
            "supplier_credit_notes",
            "invoice_id",
        )
    };
    let mut tx = db.begin().await?;
    require_permission(
        &mut tx,
        company,
        actor,
        if customer {
            "customers.credit"
        } else {
            "suppliers.invoice"
        },
    )
    .await?;
    if let Some(replay) = find_replay(
        &mut tx,
        table,
        company,
        request.idempotency_key,
        &fingerprint,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(replay);
    }
    let source_sql = format!(
        "SELECT {party_col},{source_col},btrim(currency_code)::text AS currency_code FROM {source} WHERE id=$1 AND company_id=$2"
    );
    let source_row = sqlx::query(sqlx::AssertSqlSafe(source_sql))
        .bind(credit_note_id)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ReversalError::NotFound)?;
    let party_id: i64 = source_row.try_get(party_col)?;
    let document_id: Option<i64> = source_row.try_get(source_col)?;
    let currency: String = source_row.try_get("currency_code")?;
    let party_lock = format!("SELECT id FROM {party} WHERE id=$1 AND company_id=$2 FOR UPDATE");
    sqlx::query(sqlx::AssertSqlSafe(party_lock))
        .bind(party_id)
        .bind(company)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(document_id) = document_id {
        let doc_table = if customer {
            "sales"
        } else {
            "supplier_invoices"
        };
        let doc_lock =
            format!("SELECT id FROM {doc_table} WHERE id=$1 AND company_id=$2 FOR UPDATE");
        sqlx::query(sqlx::AssertSqlSafe(doc_lock))
            .bind(document_id)
            .bind(company)
            .fetch_one(&mut *tx)
            .await?;
    }
    let note_lock = format!("SELECT amount FROM {source} WHERE id=$1 AND company_id=$2 FOR UPDATE");
    sqlx::query(sqlx::AssertSqlSafe(note_lock))
        .bind(credit_note_id)
        .bind(company)
        .fetch_one(&mut *tx)
        .await?;
    let insert = format!(
        "INSERT INTO {table}(company_id,credit_note_id,{party_col},currency_code,amount,reason,idempotency_key,request_fingerprint,reversed_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(company_id,idempotency_key) DO NOTHING RETURNING id"
    );
    let id: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(insert))
        .bind(company)
        .bind(credit_note_id)
        .bind(party_id)
        .bind(currency)
        .bind(request.amount)
        .bind(request.reason.trim())
        .bind(request.idempotency_key)
        .bind(&fingerprint)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await?;
    let outcome = if let Some(id) = id {
        ReversalOutcome {
            reversal_id: id,
            replayed: false,
        }
    } else {
        find_replay(
            &mut tx,
            table,
            company,
            request.idempotency_key,
            &fingerprint,
        )
        .await?
        .ok_or(ReversalError::IdempotencyConflict)?
    };
    tx.commit().await?;
    Ok(outcome)
}
