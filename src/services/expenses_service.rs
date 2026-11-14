use chrono::Utc;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::expenses::*,
    services::cash_register_service::{CashRegisterError, current_cash_balance},
};

const SELECT_EXPENSE: &str = "SELECT id, expense_number, category, description, amount, \
    currency_code::text AS currency_code, expense_date, payment_method, reference, notes, \
    supplier_id, cash_session_id, status, voided_at, void_reason, created_by, created_at, version \
    FROM expenses";

fn clean(v: &Option<String>, max: usize) -> ApiResult<Option<String>> {
    match v.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) if s.chars().count() > max => Err(ApiError::bad_request(
            "INVALID_FIELD",
            format!("Text field exceeds {max} characters"),
        )),
        Some(s) => Ok(Some(s.to_string())),
        None => Ok(None),
    }
}

fn cash_err(e: CashRegisterError) -> ApiError {
    match e {
        CashRegisterError::Database(e) => e.into(),
        CashRegisterError::SessionNotOpen => ApiError::conflict(
            "SESSION_NOT_OPEN",
            "The cash session is not open",
        ),
        _ => ApiError::internal(),
    }
}

async fn fetch_one(db: impl sqlx::PgExecutor<'_>, company: Uuid, id: i64) -> ApiResult<Expense> {
    let sql = format!("{SELECT_EXPENSE} WHERE id=$1 AND company_id=$2");
    sqlx::query_as::<_, Expense>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(company)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| ApiError::not_found("EXPENSE_NOT_FOUND", "Expense not found"))
}

pub async fn get_expense(db: &PgPool, company: Uuid, id: i64) -> ApiResult<Expense> {
    fetch_one(db, company, id).await
}

pub async fn list_expenses(
    db: &PgPool,
    company: Uuid,
    q: ExpenseQuery,
) -> ApiResult<Vec<Expense>> {
    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(ApiError::bad_request(
            "INVALID_QUERY",
            "limit must be 1-200 and offset >= 0",
        ));
    }
    if let Some(s) = &q.status {
        if s != "posted" && s != "voided" {
            return Err(ApiError::bad_request("INVALID_QUERY", "status must be posted or voided"));
        }
    }
    let sql = format!(
        "{SELECT_EXPENSE} WHERE company_id=$1 \
         AND ($2::date IS NULL OR expense_date >= $2) \
         AND ($3::date IS NULL OR expense_date <= $3) \
         AND ($4::text IS NULL OR lower(category) = lower($4)) \
         AND ($5::text IS NULL OR status = $5) \
         ORDER BY expense_date DESC, id DESC LIMIT $6 OFFSET $7"
    );
    Ok(sqlx::query_as::<_, Expense>(sqlx::AssertSqlSafe(sql))
        .bind(company)
        .bind(q.from)
        .bind(q.to)
        .bind(q.category.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(q.status)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await?)
}

pub async fn summary(db: &PgPool, company: Uuid, q: ExpenseSummaryQuery) -> ApiResult<Value> {
    let today = Utc::now().date_naive();
    let from = q.from.unwrap_or(today.with_day0(0).unwrap_or(today));
    let to = q.to.unwrap_or(today);
    if to < from {
        return Err(ApiError::bad_request("INVALID_QUERY", "'to' must not be before 'from'"));
    }
    let v: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
           'from', $2::date::text, 'to', $3::date::text, \
           'currency_code', (SELECT default_currency::text FROM companies WHERE id=$1), \
           'total', COALESCE((SELECT SUM(amount)::text FROM expenses \
               WHERE company_id=$1 AND status='posted' AND expense_date BETWEEN $2 AND $3), '0'), \
           'count', (SELECT COUNT(*) FROM expenses \
               WHERE company_id=$1 AND status='posted' AND expense_date BETWEEN $2 AND $3), \
           'by_category', COALESCE((SELECT jsonb_agg(jsonb_build_object( \
               'category', category, 'total', total::text, 'count', cnt) ORDER BY total DESC) \
             FROM (SELECT category, SUM(amount) total, COUNT(*) cnt FROM expenses \
               WHERE company_id=$1 AND status='posted' AND expense_date BETWEEN $2 AND $3 \
               GROUP BY category) c), '[]'::jsonb))",
    )
    .bind(company)
    .bind(from)
    .bind(to)
    .fetch_one(db)
    .await?;
    Ok(v)
}

use chrono::Datelike;

pub async fn create_expense(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    req: ExpenseCreate,
) -> ApiResult<ExpenseOutcome> {
    let category = req.category.trim().to_string();
    let description = req.description.trim().to_string();
    if category.is_empty() || category.chars().count() > 100 || description.is_empty() {
        return Err(ApiError::bad_request(
            "INVALID_EXPENSE",
            "Category (max 100 chars) and description are required",
        ));
    }
    if req.amount <= Decimal::ZERO || req.amount.scale() > 4 || req.amount >= Decimal::from(10u64.pow(14)) {
        return Err(ApiError::bad_request(
            "INVALID_AMOUNT",
            "Amount must be positive with at most 4 decimal places",
        ));
    }
    let method = req.payment_method.trim().to_string();
    if !["cash", "card", "mobile_money", "bank_transfer", "other"].contains(&method.as_str()) {
        return Err(ApiError::bad_request(
            "INVALID_PAYMENT_METHOD",
            "payment_method must be cash, card, mobile_money, bank_transfer or other",
        ));
    }
    if (method == "cash") != req.cash_session_id.is_some() {
        return Err(ApiError::bad_request(
            "CASH_SESSION_MISMATCH",
            "cash_session_id is required for cash expenses and must be omitted otherwise",
        ));
    }
    let reference = clean(&req.reference, 150)?;
    let notes = clean(&req.notes, 2000)?;
    let date = req.expense_date.unwrap_or_else(|| Utc::now().date_naive());

    let fingerprint = json!({
        "category": category.to_lowercase(), "description": description,
        "amount": req.amount.normalize().to_string(), "date": date.to_string(),
        "method": method, "session": req.cash_session_id, "supplier": req.supplier_id,
        "reference": reference, "notes": notes,
    })
    .to_string();

    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("expense:{company}:{}", req.idempotency_key))
        .execute(&mut *tx)
        .await?;

    let existing: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, request_fingerprint FROM expenses WHERE company_id=$1 AND idempotency_key=$2",
    )
    .bind(company)
    .bind(req.idempotency_key)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((id, stored)) = existing {
        if stored != fingerprint {
            return Err(ApiError::conflict(
                "IDEMPOTENCY_CONFLICT",
                "This idempotency key was already used with a different request",
            ));
        }
        let expense = fetch_one(&mut *tx, company, id).await?;
        tx.commit().await?;
        return Ok(ExpenseOutcome { expense, replayed: true });
    }

    let number: i64 = sqlx::query_scalar(
        "INSERT INTO expense_sequences (company_id, last_number) VALUES ($1, 1) \
         ON CONFLICT (company_id) DO UPDATE SET last_number = expense_sequences.last_number + 1 \
         RETURNING last_number",
    )
    .bind(company)
    .fetch_one(&mut *tx)
    .await?;

    let mut movement_id: Option<i64> = None;
    if let Some(session) = req.cash_session_id {
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM cash_register_sessions WHERE id=$1 AND company_id=$2 FOR UPDATE",
        )
        .bind(session)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?;
        match status.as_deref() {
            None => return Err(ApiError::not_found("SESSION_NOT_FOUND", "Cash session not found")),
            Some("open") => {}
            Some(_) => {
                return Err(ApiError::conflict("SESSION_NOT_OPEN", "The cash session is not open"));
            }
        }
        let available = current_cash_balance(&mut tx, company, session).await.map_err(cash_err)?;
        if available < req.amount {
            return Err(ApiError::conflict(
                "INSUFFICIENT_CASH",
                "The till does not hold enough cash for this expense",
            ));
        }
        movement_id = Some(
            sqlx::query_scalar(
                "INSERT INTO cash_movements (company_id, register_session_id, movement_type, \
                   amount, reference, notes, idempotency_key, request_fingerprint, created_by) \
                 VALUES ($1,$2,'paid_out',$3,$4,$5,$6,$7,$8) RETURNING id",
            )
            .bind(company)
            .bind(session)
            .bind(req.amount)
            .bind(format!("EXP-{number}"))
            .bind(format!("Expense: {category}"))
            .bind(Uuid::new_v4())
            .bind(format!("expense:{number}"))
            .bind(actor)
            .fetch_one(&mut *tx)
            .await?,
        );
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO expenses (company_id, expense_number, category, description, amount, \
           currency_code, expense_date, payment_method, reference, notes, supplier_id, \
           cash_session_id, cash_movement_id, idempotency_key, request_fingerprint, created_by) \
         VALUES ($1,$2,$3,$4,$5,(SELECT default_currency FROM companies WHERE id=$1), \
           $6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id",
    )
    .bind(company)
    .bind(number)
    .bind(&category)
    .bind(&description)
    .bind(req.amount)
    .bind(date)
    .bind(&method)
    .bind(&reference)
    .bind(&notes)
    .bind(req.supplier_id)
    .bind(req.cash_session_id)
    .bind(movement_id)
    .bind(req.idempotency_key)
    .bind(&fingerprint)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;

    let expense = fetch_one(&mut *tx, company, id).await?;
    tx.commit().await?;
    Ok(ExpenseOutcome { expense, replayed: false })
}

pub async fn void_expense(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    id: i64,
    req: ExpenseVoid,
) -> ApiResult<Expense> {
    let reason = req.reason.trim().to_string();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(ApiError::bad_request("INVALID_REASON", "A reason (max 500 chars) is required"));
    }
    let mut tx = db.begin().await?;
    let row: Option<(String, i32, Option<i64>, Option<i64>, Decimal, i64)> = sqlx::query_as(
        "SELECT status, version, cash_session_id, cash_movement_id, amount, expense_number \
         FROM expenses WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, version, session, movement, amount, number) =
        row.ok_or_else(|| ApiError::not_found("EXPENSE_NOT_FOUND", "Expense not found"))?;
    if status == "voided" {
        return Err(ApiError::conflict("ALREADY_VOIDED", "This expense is already voided"));
    }
    if version != req.expected_version {
        return Err(ApiError::conflict(
            "VERSION_CONFLICT",
            "The record has been modified. Please refresh and try again",
        ));
    }
    if let (Some(session), Some(_)) = (session, movement) {
        let st: Option<String> = sqlx::query_scalar(
            "SELECT status FROM cash_register_sessions WHERE id=$1 AND company_id=$2 FOR UPDATE",
        )
        .bind(session)
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?;
        if st.as_deref() != Some("open") {
            return Err(ApiError::conflict(
                "SESSION_CLOSED",
                "A cash-paid expense can only be voided while its cash session is still open",
            ));
        }
        sqlx::query(
            "INSERT INTO cash_movements (company_id, register_session_id, movement_type, amount, \
               reference, notes, idempotency_key, request_fingerprint, created_by) \
             VALUES ($1,$2,'cash_in',$3,$4,$5,$6,$7,$8)",
        )
        .bind(company)
        .bind(session)
        .bind(amount)
        .bind(format!("VOID EXP-{number}"))
        .bind("Expense voided")
        .bind(Uuid::new_v4())
        .bind(format!("expense-void:{number}"))
        .bind(actor)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE expenses SET status='voided', voided_by=$3, voided_at=NOW(), void_reason=$4, \
         version=version+1, updated_at=NOW() WHERE id=$1 AND company_id=$2",
    )
    .bind(id)
    .bind(company)
    .bind(actor)
    .bind(&reason)
    .execute(&mut *tx)
    .await?;
    let expense = fetch_one(&mut *tx, company, id).await?;
    tx.commit().await?;
    Ok(expense)
}
