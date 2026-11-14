use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::models::financial_operations::*;

#[derive(Debug)]
pub enum FinanceError {
    NotFound,
    Invalid(&'static str),
    Forbidden,
    Conflict(&'static str),
    Database(sqlx::Error),
}
impl From<sqlx::Error> for FinanceError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

async fn require_permission(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    actor: Uuid,
    key: &str,
) -> Result<(), FinanceError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE ur.user_id=$1 AND ur.company_id=$2 AND r.is_active AND (ur.expires_at IS NULL OR ur.expires_at>NOW()) AND p.permission_key=$3)")
        .bind(actor).bind(company).bind(key).fetch_one(&mut **tx).await?;
    if allowed {
        Ok(())
    } else {
        Err(FinanceError::Forbidden)
    }
}
async fn lock_idempotency(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    key: Uuid,
) -> Result<(), FinanceError> {
    let lock_key = format!("{company}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
fn validate_method(method: &str, session: Option<i64>) -> Result<(), FinanceError> {
    if !["cash", "card", "mobile_money", "other"].contains(&method)
        || ((method == "cash") != session.is_some())
    {
        return Err(FinanceError::Invalid(
            "Payment method and cash session are inconsistent",
        ));
    }
    Ok(())
}
fn clean_optional(value: &Option<String>, max: usize) -> Result<Option<String>, FinanceError> {
    value
        .as_ref()
        .map(|s| {
            let s = s.trim();
            if s.chars().count() > max {
                Err(FinanceError::Invalid("A text field is too long"))
            } else {
                Ok((!s.is_empty()).then(|| s.to_owned()))
            }
        })
        .transpose()
        .map(Option::flatten)
}
fn valid_numeric(value: Decimal) -> bool {
    value.scale() <= 4 && value.abs() < Decimal::new(1_000_000_000_000_000, 0)
}
fn payment_fingerprint(
    amount: Decimal,
    method: &str,
    session: Option<i64>,
    reference: &Option<String>,
    notes: &Option<String>,
    allocations: &[AllocationInput],
) -> Result<String, FinanceError> {
    let mut allocations = allocations
        .iter()
        .map(|a| (a.document_id, a.amount.to_string()))
        .collect::<Vec<_>>();
    allocations.sort_by_key(|a| a.0);
    serde_json::to_string(&json!({"amount":amount.to_string(),"method":method,"session":session,"reference":reference,"notes":notes,"allocations":allocations})).map_err(|_|FinanceError::Invalid("Invalid payment request"))
}
fn check_allocations(amount: Decimal, allocations: &[AllocationInput]) -> Result<(), FinanceError> {
    if amount <= Decimal::ZERO || !valid_numeric(amount) || allocations.len() > 500 {
        return Err(FinanceError::Invalid("Payment amount must be positive"));
    }
    let mut ids = allocations
        .iter()
        .map(|a| a.document_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != allocations.len()
        || allocations
            .iter()
            .any(|a| a.document_id <= 0 || a.amount <= Decimal::ZERO || !valid_numeric(a.amount))
    {
        return Err(FinanceError::Invalid(
            "Allocations must have unique documents and positive amounts",
        ));
    }
    if allocations.iter().map(|a| a.amount).sum::<Decimal>() > amount {
        return Err(FinanceError::Invalid(
            "Allocations exceed the payment amount",
        ));
    }
    Ok(())
}
async fn check_open_cash_session(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    session_id: Option<i64>,
) -> Result<(), FinanceError> {
    if let Some(id) = session_id {
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM cash_register_sessions WHERE id=$1 AND company_id=$2 FOR UPDATE",
        )
        .bind(id)
        .bind(company)
        .fetch_optional(&mut **tx)
        .await?;
        if status.as_deref() != Some("open") {
            return Err(FinanceError::Conflict(
                "Cash payment requires an open session in this shop",
            ));
        }
    }
    Ok(())
}
async fn company_currency(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
) -> Result<String, FinanceError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT default_currency::text FROM companies WHERE id=$1")
            .bind(company)
            .fetch_optional(&mut **tx)
            .await?;
    value
        .map(|s| s.trim().to_owned())
        .ok_or(FinanceError::NotFound)
}
async fn has_column(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    column: &str,
) -> Result<bool, FinanceError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema=current_schema() AND table_name=$1 AND column_name=$2)")
        .bind(table).bind(column).fetch_one(&mut **tx).await?)
}
async fn payment_replay(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    company: Uuid,
    key: Uuid,
    fingerprint: &str,
) -> Result<Option<PostingResult>, FinanceError> {
    let sql = format!(
        "SELECT id,payment_number,request_fingerprint FROM {table} WHERE company_id=$1 AND idempotency_key=$2"
    );
    if let Some(row) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(company)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?
    {
        let old: String = row.try_get("request_fingerprint")?;
        if old != fingerprint {
            return Err(FinanceError::Conflict(
                "Idempotency key was reused with different payment details",
            ));
        }
        return Ok(Some(PostingResult {
            id: row.try_get("id")?,
            document_number: Some(row.try_get("payment_number")?),
            replayed: true,
        }));
    }
    Ok(None)
}
async fn insert_allocations(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    company: Uuid,
    party_id: i64,
    payment_id: i64,
    document_ids: &[(i64, Decimal)],
    currency: &str,
) -> Result<(), FinanceError> {
    let (document_column, party_column) = if table == "customer_payment_allocations" {
        ("sale_id", "customer_id")
    } else {
        ("invoice_id", "supplier_id")
    };
    for (doc, amount) in document_ids {
        let sql = format!(
            "INSERT INTO {table}(company_id,{party_column},payment_id,{document_column},currency_code,amount) \
             VALUES($1,$2,$3,$4,$5,$6)"
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(company)
            .bind(party_id)
            .bind(payment_id)
            .bind(doc)
            .bind(currency)
            .bind(amount)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub async fn post_customer_payment(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    customer_id: i64,
    mut q: CustomerPaymentCreate,
) -> Result<PostingResult, FinanceError> {
    validate_method(&q.payment_method, q.cash_session_id)?;
    check_allocations(q.amount, &q.allocations)?;
    q.reference = clean_optional(&q.reference, 150)?;
    q.notes = clean_optional(&q.notes, 4000)?;
    let fingerprint = payment_fingerprint(
        q.amount,
        &q.payment_method,
        q.cash_session_id,
        &q.reference,
        &q.notes,
        &q.allocations,
    )?;
    let mut allocations = q
        .allocations
        .iter()
        .map(|a| (a.document_id, a.amount))
        .collect::<Vec<_>>();
    allocations.sort_by_key(|x| x.0);
    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, "customers.payment").await?;
    lock_idempotency(&mut tx, company, q.idempotency_key).await?;
    if let Some(r) = payment_replay(
        &mut tx,
        "customer_payments",
        company,
        q.idempotency_key,
        &fingerprint,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(r);
    }
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM customers WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(customer_id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Err(FinanceError::NotFound);
    }
    let currency = company_currency(&mut tx, company).await?;
    for (sale_id, _) in &allocations {
        let sale_has_currency = has_column(&mut tx, "sales", "currency_code").await?;
        let sale_sql = if sale_has_currency {
            "SELECT total_amount,customer_id,status,btrim(currency_code)::text AS currency FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE"
        } else {
            "SELECT total_amount,customer_id,status,NULL::text AS currency FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE"
        };
        let sale = sqlx::query(sale_sql)
            .bind(sale_id)
            .bind(company)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(FinanceError::NotFound)?;
        let owner: Option<i64> = sale.try_get("customer_id")?;
        let status: String = sale.try_get("status")?;
        if owner != Some(customer_id) || status != "completed" {
            return Err(FinanceError::Invalid(
                "Allocation target is not a completed sale for this customer",
            ));
        }
        if sale
            .try_get::<Option<String>, _>("currency")?
            .is_some_and(|sale_currency| sale_currency != currency)
        {
            return Err(FinanceError::Conflict(
                "Sale and customer receipt currencies do not match",
            ));
        }
        let total: Decimal = sale.try_get("total_amount")?;
        let applied:Decimal=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=$1 AND p.sale_id=$2),0)+COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.company_id=$1 AND a.sale_id=$2),0)+COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=$1 AND n.sale_id=$2),0)").bind(company).bind(sale_id).fetch_one(&mut *tx).await?;
        let requested = allocations
            .iter()
            .find(|(id, _)| id == sale_id)
            .map(|x| x.1)
            .unwrap_or_default();
        if requested > total - applied {
            return Err(FinanceError::Conflict(
                "Allocation exceeds outstanding sale amount",
            ));
        }
    }
    check_open_cash_session(&mut tx, company, q.cash_session_id).await?;
    sqlx::query("INSERT INTO customer_payment_sequences(company_id,next_number) VALUES($1,1) ON CONFLICT(company_id) DO NOTHING").bind(company).execute(&mut *tx).await?;
    let number:i64=sqlx::query_scalar("UPDATE customer_payment_sequences SET next_number=next_number+1 WHERE company_id=$1 RETURNING next_number-1").bind(company).fetch_one(&mut *tx).await?;
    let id: i64 = if has_column(&mut tx, "customer_payments", "currency_code").await? {
        sqlx::query_scalar("INSERT INTO customer_payments(company_id,customer_id,currency_code,payment_number,amount,payment_method,reference,notes,cash_session_id,idempotency_key,request_fingerprint,received_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id")
            .bind(company).bind(customer_id).bind(&currency).bind(number).bind(q.amount).bind(&q.payment_method).bind(&q.reference).bind(&q.notes).bind(q.cash_session_id).bind(q.idempotency_key).bind(&fingerprint).bind(actor).fetch_one(&mut *tx).await?
    } else {
        sqlx::query_scalar("INSERT INTO customer_payments(company_id,customer_id,payment_number,amount,payment_method,reference,notes,cash_session_id,idempotency_key,request_fingerprint,received_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id")
            .bind(company).bind(customer_id).bind(number).bind(q.amount).bind(&q.payment_method).bind(&q.reference).bind(&q.notes).bind(q.cash_session_id).bind(q.idempotency_key).bind(&fingerprint).bind(actor).fetch_one(&mut *tx).await?
    };
    insert_allocations(
        &mut tx,
        "customer_payment_allocations",
        company,
        customer_id,
        id,
        &allocations,
        &currency,
    )
    .await?;
    tx.commit().await?;
    Ok(PostingResult {
        id,
        document_number: Some(number),
        replayed: false,
    })
}

pub async fn post_supplier_payment(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    supplier_id: i64,
    mut q: SupplierPaymentCreate,
) -> Result<PostingResult, FinanceError> {
    validate_method(&q.payment_method, q.cash_session_id)?;
    check_allocations(q.amount, &q.allocations)?;
    q.reference = clean_optional(&q.reference, 150)?;
    q.notes = clean_optional(&q.notes, 4000)?;
    let fingerprint = payment_fingerprint(
        q.amount,
        &q.payment_method,
        q.cash_session_id,
        &q.reference,
        &q.notes,
        &q.allocations,
    )?;
    let mut allocations = q
        .allocations
        .iter()
        .map(|a| (a.document_id, a.amount))
        .collect::<Vec<_>>();
    allocations.sort_by_key(|x| x.0);
    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, "suppliers.payment").await?;
    lock_idempotency(&mut tx, company, q.idempotency_key).await?;
    if let Some(r) = payment_replay(
        &mut tx,
        "supplier_payments",
        company,
        q.idempotency_key,
        &fingerprint,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(r);
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT id FROM suppliers WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(supplier_id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let currency = company_currency(&mut tx, company).await?;
    for (invoice_id, _) in &allocations {
        let invoice=sqlx::query("SELECT total_amount,supplier_id,status,btrim(currency_code)::text AS currency FROM supplier_invoices WHERE id=$1 AND company_id=$2 FOR UPDATE").bind(invoice_id).bind(company).fetch_optional(&mut *tx).await?.ok_or(FinanceError::NotFound)?;
        if invoice.try_get::<i64, _>("supplier_id")? != supplier_id
            || invoice.try_get::<String, _>("status")? != "posted"
        {
            return Err(FinanceError::Invalid(
                "Allocation target is not a posted invoice for this supplier",
            ));
        }
        if invoice.try_get::<String, _>("currency")? != currency {
            return Err(FinanceError::Conflict(
                "Supplier payments support only the company base currency in the applied schema",
            ));
        }
        let total: Decimal = invoice.try_get("total_amount")?;
        let applied:Decimal=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.company_id=$1 AND a.invoice_id=$2),0)+COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=$1 AND n.invoice_id=$2),0)").bind(company).bind(invoice_id).fetch_one(&mut *tx).await?;
        let requested = allocations
            .iter()
            .find(|(id, _)| id == invoice_id)
            .map(|x| x.1)
            .unwrap_or_default();
        if requested > total - applied {
            return Err(FinanceError::Conflict(
                "Allocation exceeds outstanding supplier invoice amount",
            ));
        }
    }
    check_open_cash_session(&mut tx, company, q.cash_session_id).await?;
    sqlx::query("INSERT INTO supplier_payment_sequences(company_id,next_number) VALUES($1,1) ON CONFLICT(company_id) DO NOTHING").bind(company).execute(&mut *tx).await?;
    let number:i64=sqlx::query_scalar("UPDATE supplier_payment_sequences SET next_number=next_number+1 WHERE company_id=$1 RETURNING next_number-1").bind(company).fetch_one(&mut *tx).await?;
    let id: i64 = if has_column(&mut tx, "supplier_payments", "currency_code").await? {
        sqlx::query_scalar("INSERT INTO supplier_payments(company_id,supplier_id,currency_code,payment_number,amount,payment_method,reference,notes,cash_session_id,idempotency_key,request_fingerprint,paid_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id")
            .bind(company).bind(supplier_id).bind(&currency).bind(number).bind(q.amount).bind(&q.payment_method).bind(&q.reference).bind(&q.notes).bind(q.cash_session_id).bind(q.idempotency_key).bind(&fingerprint).bind(actor).fetch_one(&mut *tx).await?
    } else {
        sqlx::query_scalar("INSERT INTO supplier_payments(company_id,supplier_id,payment_number,amount,payment_method,reference,notes,cash_session_id,idempotency_key,request_fingerprint,paid_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id")
            .bind(company).bind(supplier_id).bind(number).bind(q.amount).bind(&q.payment_method).bind(&q.reference).bind(&q.notes).bind(q.cash_session_id).bind(q.idempotency_key).bind(&fingerprint).bind(actor).fetch_one(&mut *tx).await?
    };
    insert_allocations(
        &mut tx,
        "supplier_payment_allocations",
        company,
        supplier_id,
        id,
        &allocations,
        &currency,
    )
    .await?;
    tx.commit().await?;
    Ok(PostingResult {
        id,
        document_number: Some(number),
        replayed: false,
    })
}

fn invoice_fingerprint(
    q: &SupplierInvoiceCreate,
    subtotal: Decimal,
    tax: Decimal,
    discount: Decimal,
    total: Decimal,
) -> Result<String, FinanceError> {
    let lines=q.lines.iter().map(|l|json!({"description":l.description.trim(),"quantity":l.quantity.to_string(),"unit_cost":l.unit_cost.to_string(),"tax":l.tax_amount.to_string(),"discount":l.discount_amount.to_string(),"purchase_order_item_id":l.purchase_order_item_id,"goods_receipt_item_id":l.goods_receipt_item_id})).collect::<Vec<_>>();
    serde_json::to_string(&json!({"invoice_number":q.invoice_number.trim(),"supplier_reference":q.supplier_reference,"purchase_order_id":q.purchase_order_id,"invoice_date":q.invoice_date,"due_date":q.due_date,"currency_code":q.currency_code.trim().to_ascii_uppercase(),"additional_charges":q.additional_charges.to_string(),"notes":q.notes,"subtotal":subtotal.to_string(),"tax":tax.to_string(),"discount":discount.to_string(),"total":total.to_string(),"lines":lines})).map_err(|_|FinanceError::Invalid("Invalid supplier invoice"))
}

async fn load_invoice(
    db: &PgPool,
    company: Uuid,
    id: i64,
) -> Result<SupplierInvoice, FinanceError> {
    let row=sqlx::query("SELECT id,supplier_id,invoice_number,supplier_reference,purchase_order_id,invoice_date,due_date,btrim(currency_code)::text AS currency_code,subtotal,tax_total,discount_total,additional_charges,total_amount,status,posted_at FROM supplier_invoices WHERE id=$1 AND company_id=$2")
        .bind(id).bind(company).fetch_optional(db).await?.ok_or(FinanceError::NotFound)?;
    let lines=sqlx::query_as::<_,SupplierInvoiceLine>("SELECT id,description,quantity,unit_cost,tax_amount,discount_amount,line_total,purchase_order_item_id,goods_receipt_item_id FROM supplier_invoice_items WHERE invoice_id=$1 AND company_id=$2 ORDER BY id").bind(id).bind(company).fetch_all(db).await?;
    Ok(SupplierInvoice {
        id: row.try_get("id")?,
        supplier_id: row.try_get("supplier_id")?,
        invoice_number: row.try_get("invoice_number")?,
        supplier_reference: row.try_get("supplier_reference")?,
        purchase_order_id: row.try_get("purchase_order_id")?,
        invoice_date: row.try_get("invoice_date")?,
        due_date: row.try_get("due_date")?,
        currency_code: row.try_get("currency_code")?,
        subtotal: row.try_get("subtotal")?,
        tax_total: row.try_get("tax_total")?,
        discount_total: row.try_get("discount_total")?,
        additional_charges: row.try_get("additional_charges")?,
        total_amount: row.try_get("total_amount")?,
        status: row.try_get("status")?,
        posted_at: row.try_get("posted_at")?,
        lines,
    })
}

pub async fn create_supplier_invoice(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    supplier_id: i64,
    mut q: SupplierInvoiceCreate,
) -> Result<SupplierInvoice, FinanceError> {
    q.invoice_number = q.invoice_number.trim().to_owned();
    q.currency_code = q.currency_code.trim().to_ascii_uppercase();
    q.supplier_reference = clean_optional(&q.supplier_reference, 150)?;
    q.notes = clean_optional(&q.notes, 4000)?;
    if q.invoice_number.is_empty()
        || q.invoice_number.chars().count() > 100
        || q.lines.is_empty()
        || q.lines.len() > 500
        || !valid_numeric(q.additional_charges)
        || q.additional_charges < Decimal::ZERO
        || q.due_date.is_some_and(|due| due < q.invoice_date)
        || q.lines.iter().any(|l| {
            l.description.trim().is_empty()
                || l.description.chars().count() > 255
                || l.quantity <= Decimal::ZERO
                || l.unit_cost < Decimal::ZERO
                || l.tax_amount < Decimal::ZERO
                || l.discount_amount < Decimal::ZERO
                || !valid_numeric(l.quantity)
                || !valid_numeric(l.unit_cost)
                || !valid_numeric(l.tax_amount)
                || !valid_numeric(l.discount_amount)
        })
    {
        return Err(FinanceError::Invalid(
            "Supplier invoice fields or lines are invalid",
        ));
    }
    let mut subtotal = Decimal::ZERO;
    let mut tax = Decimal::ZERO;
    let mut discount = Decimal::ZERO;
    let mut computed = Vec::with_capacity(q.lines.len());
    for l in &q.lines {
        let base = l
            .quantity
            .checked_mul(l.unit_cost)
            .ok_or(FinanceError::Invalid(
                "Invoice line amount exceeds supported precision",
            ))?
            .round_dp(4);
        if l.discount_amount > base + l.tax_amount {
            return Err(FinanceError::Invalid(
                "Invoice line discount exceeds its value",
            ));
        }
        let line_total = (base + l.tax_amount - l.discount_amount).round_dp(4);
        subtotal += base;
        tax += l.tax_amount;
        discount += l.discount_amount;
        if !valid_numeric(subtotal) || !valid_numeric(tax) || !valid_numeric(discount) {
            return Err(FinanceError::Invalid(
                "Invoice totals exceed supported precision",
            ));
        }
        computed.push(line_total);
    }
    subtotal = subtotal.round_dp(4);
    tax = tax.round_dp(4);
    discount = discount.round_dp(4);
    let total = (subtotal + tax - discount + q.additional_charges).round_dp(4);
    if total <= Decimal::ZERO || !valid_numeric(total) {
        return Err(FinanceError::Invalid(
            "Supplier invoice total must be positive",
        ));
    }
    let fingerprint = invoice_fingerprint(&q, subtotal, tax, discount, total)?;
    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, "suppliers.invoice").await?;
    lock_idempotency(&mut tx, company, q.idempotency_key).await?;
    if let Some(row)=sqlx::query("SELECT id,request_fingerprint FROM supplier_invoices WHERE company_id=$1 AND idempotency_key=$2").bind(company).bind(q.idempotency_key).fetch_optional(&mut *tx).await? {
        if row.try_get::<String,_>("request_fingerprint")?!=fingerprint{return Err(FinanceError::Conflict("Idempotency key was reused with different invoice details"));}
        let id:i64=row.try_get("id")?; tx.commit().await?; return load_invoice(db,company,id).await;
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT id FROM suppliers WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(supplier_id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let currency = company_currency(&mut tx, company).await?;
    if q.currency_code != currency {
        return Err(FinanceError::Conflict(
            "The applied account-payment schema supports supplier invoices only in the company base currency",
        ));
    }
    if let Some(po_id) = q.purchase_order_id {
        let po=sqlx::query("SELECT supplier_id,status FROM purchase_orders WHERE id=$1 AND company_id=$2 FOR UPDATE").bind(po_id).bind(company).fetch_optional(&mut *tx).await?.ok_or(FinanceError::NotFound)?;
        let po_supplier: i64 = po.try_get("supplier_id")?;
        let po_status: String = po.try_get("status")?;
        if po_supplier != supplier_id || po_status == "cancelled" || po_status == "draft" {
            return Err(FinanceError::Invalid(
                "Purchase order is not eligible for this supplier invoice",
            ));
        }
    }
    for line in &q.lines {
        if line.purchase_order_item_id.is_some() && q.purchase_order_id.is_none() {
            return Err(FinanceError::Invalid(
                "An invoice purchase-order line requires the matching purchase_order_id",
            ));
        }
        if line.goods_receipt_item_id.is_some() && q.purchase_order_id.is_none() {
            return Err(FinanceError::Invalid(
                "A receipt-linked invoice line requires the matching purchase_order_id",
            ));
        }
        if let Some(item_id) = line.purchase_order_item_id {
            let order:Option<i64>=sqlx::query_scalar("SELECT poi.purchase_order_id FROM purchase_order_items poi JOIN purchase_orders po ON po.id=poi.purchase_order_id AND po.company_id=poi.company_id WHERE poi.id=$1 AND poi.company_id=$2 AND po.supplier_id=$3").bind(item_id).bind(company).bind(supplier_id).fetch_optional(&mut *tx).await?;
            if order != q.purchase_order_id {
                return Err(FinanceError::Invalid(
                    "Purchase-order line does not belong to this invoice's order and supplier",
                ));
            }
        }
        if let Some(receipt_id) = line.goods_receipt_item_id {
            let order:Option<i64>=sqlx::query_scalar("SELECT gr.purchase_order_id FROM goods_receipt_items gri JOIN goods_receipts gr ON gr.id=gri.goods_receipt_id AND gr.company_id=gri.company_id JOIN purchase_orders po ON po.id=gr.purchase_order_id AND po.company_id=gr.company_id WHERE gri.id=$1 AND gri.company_id=$2 AND po.supplier_id=$3").bind(receipt_id).bind(company).bind(supplier_id).fetch_optional(&mut *tx).await?;
            if order != q.purchase_order_id {
                return Err(FinanceError::Invalid(
                    "Goods-receipt line does not belong to this invoice's order and supplier",
                ));
            }
            if let (Some(receipt), Some(poi)) =
                (line.goods_receipt_item_id, line.purchase_order_item_id)
            {
                let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM goods_receipt_items WHERE id=$1 AND company_id=$2 AND purchase_order_item_id=$3)").bind(receipt).bind(company).bind(poi).fetch_one(&mut *tx).await?;
                if !matches {
                    return Err(FinanceError::Invalid(
                        "Goods receipt item and purchase-order item do not match",
                    ));
                }
            }
        }
    }
    let invoice_id:i64=sqlx::query_scalar("INSERT INTO supplier_invoices(company_id,supplier_id,invoice_number,supplier_reference,purchase_order_id,invoice_date,due_date,currency_code,subtotal,tax_total,discount_total,additional_charges,total_amount,status,notes,idempotency_key,request_fingerprint,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'draft',$14,$15,$16,$17) RETURNING id")
        .bind(company).bind(supplier_id).bind(&q.invoice_number).bind(&q.supplier_reference).bind(q.purchase_order_id).bind(q.invoice_date).bind(q.due_date).bind(&q.currency_code).bind(subtotal).bind(tax).bind(discount).bind(q.additional_charges).bind(total).bind(&q.notes).bind(q.idempotency_key).bind(&fingerprint).bind(actor).fetch_one(&mut *tx).await.map_err(map_unique)?;
    for (line, line_total) in q.lines.iter().zip(computed) {
        sqlx::query("INSERT INTO supplier_invoice_items(company_id,invoice_id,purchase_order_item_id,goods_receipt_item_id,description,quantity,unit_cost,tax_amount,discount_amount,line_total) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(company).bind(invoice_id).bind(line.purchase_order_item_id).bind(line.goods_receipt_item_id).bind(line.description.trim()).bind(line.quantity).bind(line.unit_cost).bind(line.tax_amount).bind(line.discount_amount).bind(line_total).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE supplier_invoices SET status='posted',posted_by=$1,posted_at=NOW(),updated_at=NOW(),version=version+1 WHERE id=$2 AND company_id=$3 AND status='draft'").bind(actor).bind(invoice_id).bind(company).execute(&mut *tx).await?;
    tx.commit().await?;
    load_invoice(db, company, invoice_id).await
}
fn map_unique(error: sqlx::Error) -> FinanceError {
    if matches!(&error,sqlx::Error::Database(d) if d.code().as_deref()==Some("23505")) {
        FinanceError::Conflict("A financial idempotency key or document number already exists")
    } else {
        FinanceError::Database(error)
    }
}

pub async fn get_supplier_invoice(
    db: &PgPool,
    company: Uuid,
    id: i64,
) -> Result<SupplierInvoice, FinanceError> {
    load_invoice(db, company, id).await
}
pub async fn supplier_invoices(
    db: &PgPool,
    company: Uuid,
    supplier: i64,
    limit: i64,
    offset: i64,
) -> Result<Page<SupplierInvoice>, FinanceError> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    if sqlx::query_scalar::<_, i64>("SELECT id FROM suppliers WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(supplier)
        .fetch_optional(db)
        .await?
        .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let ids=sqlx::query_scalar::<_,i64>("SELECT id FROM supplier_invoices WHERE company_id=$1 AND supplier_id=$2 ORDER BY invoice_date DESC,id DESC LIMIT $3 OFFSET $4").bind(company).bind(supplier).bind(limit).bind(offset).fetch_all(db).await?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(load_invoice(db, company, id).await?);
    }
    Ok(Page {
        items,
        limit,
        offset,
    })
}

async fn credit_replay(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    company: Uuid,
    key: Uuid,
    party_column: &str,
    party_id: i64,
    document_column: &str,
    document_id: Option<i64>,
    amount: Decimal,
    reason: &str,
) -> Result<Option<PostingResult>, FinanceError> {
    let sql = format!(
        "SELECT id,credit_number,{party_column},{document_column},amount,reason FROM {table} WHERE company_id=$1 AND idempotency_key=$2"
    );
    if let Some(r) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(company)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?
    {
        let stored_amount: Decimal = r.try_get("amount")?;
        let same = r.try_get::<i64, _>(party_column)? == party_id
            && r.try_get::<Option<i64>, _>(document_column)? == document_id
            && stored_amount == amount
            && r.try_get::<String, _>("reason")? == reason;
        if !same {
            return Err(FinanceError::Conflict(
                "Idempotency key was reused with different credit-note details",
            ));
        }
        return Ok(Some(PostingResult {
            id: r.try_get("id")?,
            document_number: Some(r.try_get("credit_number")?),
            replayed: true,
        }));
    }
    Ok(None)
}
async fn next_credit_number(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    table: &str,
) -> Result<i64, FinanceError> {
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(company)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FinanceError::NotFound)?;
    let sql = format!("SELECT COALESCE(MAX(credit_number),0)+1 FROM {table} WHERE company_id=$1");
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(company)
        .fetch_one(&mut **tx)
        .await?)
}
pub async fn post_customer_credit_note(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    customer_id: i64,
    q: CreditNoteCreate,
) -> Result<PostingResult, FinanceError> {
    let reason = q.reason.trim().to_owned();
    if q.amount <= Decimal::ZERO
        || !valid_numeric(q.amount)
        || reason.is_empty()
        || reason.chars().count() > 1000
        || q.document_id.is_some_and(|id| id <= 0)
    {
        return Err(FinanceError::Invalid(
            "Credit note amount, reason or sale is invalid",
        ));
    }
    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, "customers.credit").await?;
    lock_idempotency(&mut tx, company, q.idempotency_key).await?;
    if let Some(r) = credit_replay(
        &mut tx,
        "customer_credit_notes",
        company,
        q.idempotency_key,
        "customer_id",
        customer_id,
        "sale_id",
        q.document_id,
        q.amount,
        &reason,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(r);
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT id FROM customers WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(customer_id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let currency = company_currency(&mut tx, company).await?;
    if let Some(sale_id) = q.document_id {
        let sale_has_currency = has_column(&mut tx, "sales", "currency_code").await?;
        let sale_sql = if sale_has_currency {
            "SELECT total_amount,customer_id,status,btrim(currency_code)::text AS currency FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE"
        } else {
            "SELECT total_amount,customer_id,status,NULL::text AS currency FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE"
        };
        let sale = sqlx::query(sale_sql)
            .bind(sale_id)
            .bind(company)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(FinanceError::NotFound)?;
        if sale.try_get::<Option<i64>, _>("customer_id")? != Some(customer_id)
            || sale.try_get::<String, _>("status")? != "completed"
        {
            return Err(FinanceError::Invalid(
                "Credit note must reference a completed sale for this customer",
            ));
        }
        if sale
            .try_get::<Option<String>, _>("currency")?
            .is_some_and(|sale_currency| sale_currency != currency)
        {
            return Err(FinanceError::Conflict(
                "Sale and customer credit note currencies do not match",
            ));
        }
        let total: Decimal = sale.try_get("total_amount")?;
        let applied:Decimal=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=$1 AND p.sale_id=$2),0)+COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.company_id=$1 AND a.sale_id=$2),0)+COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=$1 AND n.sale_id=$2),0)").bind(company).bind(sale_id).fetch_one(&mut *tx).await?;
        if q.amount > total - applied {
            return Err(FinanceError::Conflict(
                "Credit note exceeds the outstanding sale amount",
            ));
        }
    }
    let account_balance:Decimal=sqlx::query_scalar("SELECT COALESCE(balance,0) FROM customer_account_balances WHERE company_id=$1 AND customer_id=$2").bind(company).bind(customer_id).fetch_optional(&mut *tx).await?.ok_or(FinanceError::NotFound)?;
    if q.amount > account_balance {
        return Err(FinanceError::Conflict(
            "Credit note exceeds the customer's current receivable balance",
        ));
    }
    let number = next_credit_number(&mut tx, company, "customer_credit_notes").await?;
    let fingerprint = format!(
        "{customer_id}|{:?}|{}|{}",
        q.document_id,
        q.amount.normalize(),
        reason
    );
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO customer_credit_notes(company_id,customer_id,sale_id,currency_code,credit_number,amount,reason,idempotency_key,request_fingerprint,created_by) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
    )
    .bind(company)
    .bind(customer_id)
    .bind(q.document_id)
    .bind(&currency)
    .bind(number)
    .bind(q.amount)
    .bind(&reason)
    .bind(q.idempotency_key)
    .bind(&fingerprint)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PostingResult {
        id,
        document_number: Some(number),
        replayed: false,
    })
}
pub async fn post_supplier_credit_note(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    supplier_id: i64,
    q: CreditNoteCreate,
) -> Result<PostingResult, FinanceError> {
    let reason = q.reason.trim().to_owned();
    let invoice_id = q.document_id.ok_or(FinanceError::Invalid(
        "Supplier credit note requires an invoice",
    ))?;
    if q.amount <= Decimal::ZERO
        || !valid_numeric(q.amount)
        || reason.is_empty()
        || reason.chars().count() > 1000
        || invoice_id <= 0
    {
        return Err(FinanceError::Invalid(
            "Credit note amount, reason or invoice is invalid",
        ));
    }
    let mut tx = db.begin().await?;
    require_permission(&mut tx, company, actor, "suppliers.invoice").await?;
    lock_idempotency(&mut tx, company, q.idempotency_key).await?;
    if let Some(r) = credit_replay(
        &mut tx,
        "supplier_credit_notes",
        company,
        q.idempotency_key,
        "supplier_id",
        supplier_id,
        "invoice_id",
        Some(invoice_id),
        q.amount,
        &reason,
    )
    .await?
    {
        tx.commit().await?;
        return Ok(r);
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT id FROM suppliers WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(supplier_id)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let currency = company_currency(&mut tx, company).await?;
    let invoice=sqlx::query("SELECT total_amount,supplier_id,status,btrim(currency_code)::text AS currency FROM supplier_invoices WHERE id=$1 AND company_id=$2 FOR UPDATE").bind(invoice_id).bind(company).fetch_optional(&mut *tx).await?.ok_or(FinanceError::NotFound)?;
    if invoice.try_get::<i64, _>("supplier_id")? != supplier_id
        || invoice.try_get::<String, _>("status")? != "posted"
    {
        return Err(FinanceError::Invalid(
            "Credit note must reference a posted invoice for this supplier",
        ));
    }
    if invoice.try_get::<String, _>("currency")? != currency {
        return Err(FinanceError::Conflict(
            "Supplier credit notes support only the company base currency in the applied schema",
        ));
    }
    let total: Decimal = invoice.try_get("total_amount")?;
    let applied:Decimal=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.company_id=$1 AND a.invoice_id=$2),0)+COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=$1 AND n.invoice_id=$2),0)").bind(company).bind(invoice_id).fetch_one(&mut *tx).await?;
    if q.amount > total - applied {
        return Err(FinanceError::Conflict(
            "Credit note exceeds the outstanding invoice amount",
        ));
    }
    let account_balance:Decimal=sqlx::query_scalar("SELECT COALESCE(balance,0) FROM supplier_account_balances WHERE company_id=$1 AND supplier_id=$2").bind(company).bind(supplier_id).fetch_optional(&mut *tx).await?.ok_or(FinanceError::NotFound)?;
    if q.amount > account_balance {
        return Err(FinanceError::Conflict(
            "Credit note exceeds the supplier's current payable balance",
        ));
    }
    let number = next_credit_number(&mut tx, company, "supplier_credit_notes").await?;
    let fingerprint = format!(
        "{supplier_id}|{invoice_id}|{}|{}",
        q.amount.normalize(),
        reason
    );
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO supplier_credit_notes(company_id,supplier_id,invoice_id,currency_code,credit_number,amount,reason,idempotency_key,request_fingerprint,created_by) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
    )
    .bind(company)
    .bind(supplier_id)
    .bind(invoice_id)
    .bind(&currency)
    .bind(number)
    .bind(q.amount)
    .bind(&reason)
    .bind(q.idempotency_key)
    .bind(&fingerprint)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PostingResult {
        id,
        document_number: Some(number),
        replayed: false,
    })
}

pub async fn customer_balance(
    db: &PgPool,
    company: Uuid,
    customer: i64,
) -> Result<AccountBalance, FinanceError> {
    let row=sqlx::query("SELECT b.balance,c.default_currency::text AS currency FROM customer_account_balances b JOIN companies c ON c.id=b.company_id WHERE b.company_id=$1 AND b.customer_id=$2").bind(company).bind(customer).fetch_optional(db).await?.ok_or(FinanceError::NotFound)?;
    Ok(AccountBalance {
        party_id: customer,
        currency_code: row.try_get::<String, _>("currency")?.trim().to_owned(),
        balance: row.try_get("balance")?,
    })
}
pub async fn supplier_balance(
    db: &PgPool,
    company: Uuid,
    supplier: i64,
) -> Result<AccountBalance, FinanceError> {
    let currency=sqlx::query_scalar::<_,String>("SELECT default_currency::text FROM companies WHERE id=$1 AND EXISTS(SELECT 1 FROM suppliers WHERE id=$2 AND company_id=$1)").bind(company).bind(supplier).fetch_optional(db).await?.ok_or(FinanceError::NotFound)?;
    let mixed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supplier_invoices WHERE company_id=$1 AND supplier_id=$2 AND status='posted' AND btrim(currency_code)<>btrim($3)) OR EXISTS(SELECT 1 FROM supplier_payments p WHERE p.company_id=$1 AND p.supplier_id=$2 AND (to_jsonb(p)?'currency_code') AND btrim(to_jsonb(p)->>'currency_code')<>btrim($3)) OR EXISTS(SELECT 1 FROM supplier_credit_notes n WHERE n.company_id=$1 AND n.supplier_id=$2 AND (to_jsonb(n)?'currency_code') AND btrim(to_jsonb(n)->>'currency_code')<>btrim($3))").bind(company).bind(supplier).bind(&currency).fetch_one(db).await?;
    if mixed {
        return Err(FinanceError::Conflict(
            "Supplier account contains currencies the applied payment schema cannot safely reconcile",
        ));
    }
    let balance: Decimal = sqlx::query_scalar(
        "SELECT balance FROM supplier_account_balances WHERE company_id=$1 AND supplier_id=$2",
    )
    .bind(company)
    .bind(supplier)
    .fetch_one(db)
    .await?;
    Ok(AccountBalance {
        party_id: supplier,
        currency_code: currency.trim().to_owned(),
        balance,
    })
}

pub async fn customer_statement(
    db: &PgPool,
    company: Uuid,
    customer: i64,
    limit: i64,
    offset: i64,
) -> Result<Page<StatementLine>, FinanceError> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    if sqlx::query_scalar::<_, i64>("SELECT id FROM customers WHERE id=$1 AND company_id=$2")
        .bind(customer)
        .bind(company)
        .fetch_optional(db)
        .await?
        .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    let items=sqlx::query_as::<_,StatementLine>(r#"WITH events AS (
      SELECT 'sale'::text event_type,s.id document_id,s.created_at event_at,s.sale_number::text reference,s.total_amount balance_delta FROM sales s WHERE s.company_id=$1 AND s.customer_id=$2 AND s.status='completed'
      UNION ALL SELECT 'sale_payment',p.id,p.created_at,COALESCE(p.reference,'Sale payment'),-p.amount FROM sale_payments p JOIN sales s ON s.id=p.sale_id AND s.company_id=p.company_id WHERE s.company_id=$1 AND s.customer_id=$2 AND s.status='completed'
      UNION ALL SELECT 'account_receipt',p.id,p.received_at,COALESCE(p.reference,'Receipt '||p.payment_number::text),-p.amount FROM customer_payments p WHERE p.company_id=$1 AND p.customer_id=$2
      UNION ALL SELECT 'credit_note',n.id,n.created_at,'Credit note '||n.credit_number::text,-n.amount FROM customer_credit_notes n WHERE n.company_id=$1 AND n.customer_id=$2
    ), running AS (SELECT event_type,document_id,event_at,reference,balance_delta,SUM(balance_delta) OVER(ORDER BY event_at,event_type,document_id ROWS UNBOUNDED PRECEDING) balance_after FROM events)
    SELECT event_type,document_id,event_at,reference,balance_delta,balance_after FROM running ORDER BY event_at,event_type,document_id LIMIT $3 OFFSET $4"#).bind(company).bind(customer).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(Page {
        items,
        limit,
        offset,
    })
}
pub async fn supplier_statement(
    db: &PgPool,
    company: Uuid,
    supplier: i64,
    limit: i64,
    offset: i64,
) -> Result<Page<StatementLine>, FinanceError> {
    let _ = supplier_balance(db, company, supplier).await?;
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let items=sqlx::query_as::<_,StatementLine>(r#"WITH events AS (
      SELECT 'supplier_invoice'::text event_type,i.id document_id,(i.invoice_date::timestamp AT TIME ZONE 'UTC') event_at,i.invoice_number reference,i.total_amount balance_delta FROM supplier_invoices i WHERE i.company_id=$1 AND i.supplier_id=$2 AND i.status='posted'
      UNION ALL SELECT 'supplier_payment',p.id,p.paid_at,COALESCE(p.reference,'Payment '||p.payment_number::text),-p.amount FROM supplier_payments p WHERE p.company_id=$1 AND p.supplier_id=$2
      UNION ALL SELECT 'credit_note',n.id,n.created_at,'Credit note '||n.credit_number::text,-n.amount FROM supplier_credit_notes n WHERE n.company_id=$1 AND n.supplier_id=$2
    ), running AS (SELECT event_type,document_id,event_at,reference,balance_delta,SUM(balance_delta) OVER(ORDER BY event_at,event_type,document_id ROWS UNBOUNDED PRECEDING) balance_after FROM events)
    SELECT event_type,document_id,event_at,reference,balance_delta,balance_after FROM running ORDER BY event_at,event_type,document_id LIMIT $3 OFFSET $4"#).bind(company).bind(supplier).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(Page {
        items,
        limit,
        offset,
    })
}
pub async fn customer_outstanding(
    db: &PgPool,
    company: Uuid,
    customer: i64,
    limit: i64,
    offset: i64,
) -> Result<Page<OutstandingDocument>, FinanceError> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows=sqlx::query_as::<_,OutstandingDocument>(r#"SELECT s.id,s.sale_number::text document_number,s.created_at::date document_date,s.total_amount,
      COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=s.company_id AND p.sale_id=s.id),0)+COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.company_id=s.company_id AND a.sale_id=s.id),0) allocated_amount,
      COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=s.company_id AND n.sale_id=s.id),0) credited_amount,
      s.total_amount-COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=s.company_id AND p.sale_id=s.id),0)-COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.company_id=s.company_id AND a.sale_id=s.id),0)-COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=s.company_id AND n.sale_id=s.id),0) outstanding_amount
      FROM sales s WHERE s.company_id=$1 AND s.customer_id=$2 AND s.status='completed'
      AND s.total_amount-COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=s.company_id AND p.sale_id=s.id),0)-COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.company_id=s.company_id AND a.sale_id=s.id),0)-COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=s.company_id AND n.sale_id=s.id),0)>0
      ORDER BY s.created_at,s.id LIMIT $3 OFFSET $4"#).bind(company).bind(customer).bind(limit).bind(offset).fetch_all(db).await?;
    if sqlx::query_scalar::<_, i64>("SELECT id FROM customers WHERE id=$1 AND company_id=$2")
        .bind(customer)
        .bind(company)
        .fetch_optional(db)
        .await?
        .is_none()
    {
        return Err(FinanceError::NotFound);
    }
    Ok(Page {
        items: rows,
        limit,
        offset,
    })
}
pub async fn supplier_outstanding(
    db: &PgPool,
    company: Uuid,
    supplier: i64,
    limit: i64,
    offset: i64,
) -> Result<Page<OutstandingDocument>, FinanceError> {
    let _ = supplier_balance(db, company, supplier).await?;
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let rows=sqlx::query_as::<_,OutstandingDocument>(r#"SELECT i.id,i.invoice_number document_number,i.invoice_date document_date,i.total_amount,
      COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.company_id=i.company_id AND a.invoice_id=i.id),0) allocated_amount,
      COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=i.company_id AND n.invoice_id=i.id),0) credited_amount,
      i.total_amount-COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.company_id=i.company_id AND a.invoice_id=i.id),0)-COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=i.company_id AND n.invoice_id=i.id),0) outstanding_amount
      FROM supplier_invoices i WHERE i.company_id=$1 AND i.supplier_id=$2 AND i.status='posted'
      AND i.total_amount-COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.company_id=i.company_id AND a.invoice_id=i.id),0)-COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=i.company_id AND n.invoice_id=i.id),0)>0
      ORDER BY i.due_date NULLS LAST,i.invoice_date,i.id LIMIT $3 OFFSET $4"#).bind(company).bind(supplier).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(Page {
        items: rows,
        limit,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payments_allow_partial_allocation_but_reject_overallocation_and_duplicate_targets() {
        assert!(
            check_allocations(
                Decimal::new(100, 2),
                &[AllocationInput {
                    document_id: 1,
                    amount: Decimal::new(50, 2)
                }]
            )
            .is_ok()
        );
        assert!(
            check_allocations(
                Decimal::new(100, 2),
                &[AllocationInput {
                    document_id: 1,
                    amount: Decimal::new(101, 2)
                }]
            )
            .is_err()
        );
        assert!(
            check_allocations(
                Decimal::new(100, 2),
                &[
                    AllocationInput {
                        document_id: 1,
                        amount: Decimal::new(20, 2)
                    },
                    AllocationInput {
                        document_id: 1,
                        amount: Decimal::new(20, 2)
                    }
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn cash_requires_a_session_and_non_cash_forbids_one() {
        assert!(validate_method("cash", Some(1)).is_ok());
        assert!(validate_method("cash", None).is_err());
        assert!(validate_method("card", Some(1)).is_err());
    }
}
