//! Offline synchronisation.
//!
//! PULL: keyset-paginated delta feeds over `(updated_at, id)`. The final page returns a cursor
//! that is deliberately ~60 s in the past, so a row whose transaction committed late is never
//! skipped; clients upsert by id, so receiving a row twice is harmless.
//!
//! PUSH: offline devices upload operations identified by a client-generated UUID. Each operation
//! is journalled in `sync_operations`, executed through the normal business services (so every
//! rule still applies), and is idempotent: resending an operation returns the original outcome.
//! Operations that break a business rule (e.g. the sale exceeds server stock) are kept as
//! `rejected` for a manager to fix and retry; nothing is lost.
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::{
        cash_register::CashMovementRequest,
        expenses::ExpenseCreate,
        financial_operations::CustomerPaymentCreate,
        inventory::StockAdjustmentRequest,
        sales::{CheckoutRequest, CreateSaleRequest, SaleItemRequest, SalePaymentRequest},
        sync::*,
    },
    services::{
        cash_register_service, expenses_service, financial_operations_service, inventory_service,
        sales_service,
    },
};

const MAX_OPS_PER_PUSH: usize = 100;
const CURSOR_SAFETY_MARGIN_SECS: i64 = 60;
const RECLAIM_AFTER_SECS: i64 = 15;

// ----------------------------------------------------------------- permissions

pub async fn has_permission(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    key: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM user_roles ur
            JOIN roles r ON r.id = ur.role_id AND r.company_id = ur.company_id
            JOIN role_permissions rp ON rp.role_id = r.id
            JOIN permissions p ON p.id = rp.permission_id
            WHERE ur.user_id = $1 AND ur.company_id = $2 AND r.is_active = TRUE
              AND (ur.expires_at IS NULL OR ur.expires_at > NOW())
              AND p.permission_key = $3)"#,
    )
    .bind(user)
    .bind(company)
    .bind(key)
    .fetch_one(db)
    .await
}

async fn require(db: &PgPool, company: Uuid, user: Uuid, key: &str) -> ApiResult<()> {
    if has_permission(db, company, user, key).await? {
        Ok(())
    } else {
        Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            "You do not have permission to perform this operation",
        ))
    }
}

// ------------------------------------------------------------------------ pull

struct Entity {
    permission: &'static str,
    /// SELECT returning (data jsonb, ts timestamptz, k1 bigint, k2 bigint)
    sql: &'static str,
}

fn entity(name: &str) -> Option<Entity> {
    macro_rules! e {
        ($perm:expr, $cols:expr, $from:expr, $k1:expr, $k2:expr) => {
            Entity {
                permission: $perm,
                sql: concat!(
                    "SELECT jsonb_build_object(",
                    $cols,
                    ") AS data, updated_at AS ts, ",
                    $k1,
                    "::bigint AS k1, ",
                    $k2,
                    "::bigint AS k2 FROM ",
                    $from,
                    " WHERE company_id = $1 AND (updated_at, ",
                    $k1,
                    "::bigint, ",
                    $k2,
                    "::bigint) > ($2, $3, $4) ORDER BY updated_at, ",
                    $k1,
                    "::bigint, ",
                    $k2,
                    "::bigint LIMIT $5"
                ),
            }
        };
    }
    Some(match name {
        "categories" => e!(
            "products.view",
            "'id',id,'parent_id',parent_id,'name',name,'description',description,'is_active',is_active,'version',version,'updated_at',updated_at",
            "categories",
            "id",
            "0"
        ),
        "products" => e!(
            "products.view",
            "'id',id,'category_id',category_id,'unit_id',unit_id,'name',name,'sku',sku,'barcode',barcode,'description',description,'track_inventory',track_inventory,'is_active',is_active,'version',version,'updated_at',updated_at",
            "products",
            "id",
            "0"
        ),
        "variants" => e!(
            "products.view",
            "'id',id,'product_id',product_id,'name',name,'sku',sku,'barcode',barcode,'unit_id',unit_id,'selling_price',selling_price::text,'cost_price',cost_price::text,'is_default',is_default,'is_active',is_active,'version',version,'updated_at',updated_at",
            "product_variants",
            "id",
            "0"
        ),
        "customers" => e!(
            "customers.view",
            "'id',id,'customer_number',customer_number,'name',name,'email',email,'phone',phone,'address',address,'tax_number',tax_number,'credit_enabled',credit_enabled,'credit_limit',credit_limit::text,'payment_terms_days',payment_terms_days,'is_active',is_active,'currency_code',currency_code::text,'version',version,'updated_at',updated_at",
            "customers",
            "id",
            "0"
        ),
        "locations" => e!(
            "products.view",
            "'id',id,'name',name,'code',code,'location_type',location_type,'address',address,'is_default',is_default,'is_active',is_active,'version',version,'updated_at',updated_at",
            "inventory_locations",
            "id",
            "0"
        ),
        "registers" => e!(
            "cash_register.view",
            "'id',id,'location_id',location_id,'code',code,'name',name,'is_active',is_active,'updated_at',updated_at",
            "cash_registers",
            "id",
            "0"
        ),
        "stock" => e!(
            "inventory.view",
            "'location_id',location_id,'variant_id',variant_id,'quantity_on_hand',quantity_on_hand::text,'reserved_quantity',reserved_quantity::text,'version',version,'updated_at',updated_at",
            "stock_balances",
            "variant_id",
            "location_id"
        ),
        _ => return None,
    })
}

pub const ENTITIES: [&str; 7] = [
    "locations",
    "categories",
    "products",
    "variants",
    "stock",
    "customers",
    "registers",
];

type Cursor = (DateTime<Utc>, i64, i64);

fn parse_cursor(raw: Option<&str>) -> ApiResult<Cursor> {
    let epoch = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default();
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((epoch, 0, 0));
    };
    let bad = || ApiError::bad_request("INVALID_CURSOR", "The cursor is not valid; do a full pull");
    let mut parts = raw.splitn(3, '_');
    let ts = parts.next().ok_or_else(bad)?;
    let k1 = parts
        .next()
        .ok_or_else(bad)?
        .parse::<i64>()
        .map_err(|_| bad())?;
    let k2 = parts
        .next()
        .ok_or_else(bad)?
        .parse::<i64>()
        .map_err(|_| bad())?;
    let ts = DateTime::parse_from_rfc3339(ts)
        .map_err(|_| bad())?
        .with_timezone(&Utc);
    Ok((ts, k1, k2))
}

fn format_cursor(c: &Cursor) -> String {
    format!(
        "{}_{}_{}",
        c.0.to_rfc3339_opts(SecondsFormat::Micros, true),
        c.1,
        c.2
    )
}

pub async fn pull(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    name: &str,
    q: PullQuery,
) -> ApiResult<PullResponse> {
    let spec = entity(name).ok_or_else(|| {
        ApiError::not_found(
            "UNKNOWN_ENTITY",
            format!("Unknown entity. Available: {}", ENTITIES.join(", ")),
        )
    })?;
    require(db, company, user, spec.permission).await?;

    let limit = q.limit.unwrap_or(500);
    if !(1..=1000).contains(&limit) {
        return Err(ApiError::bad_request(
            "INVALID_QUERY",
            "limit must be 1-1000",
        ));
    }
    let input = parse_cursor(q.cursor.as_deref())?;

    let rows: Vec<(Value, DateTime<Utc>, i64, i64)> =
        sqlx::query_as(sqlx::AssertSqlSafe(spec.sql.to_string()))
            .bind(company)
            .bind(input.0)
            .bind(input.1)
            .bind(input.2)
            .bind(limit + 1)
            .fetch_all(db)
            .await?;

    let has_more = rows.len() as i64 > limit;
    let page: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let db_now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?;

    let next = if has_more {
        let last = page.last().expect("page is non-empty when has_more");
        (last.1, last.2, last.3)
    } else {
        // Caught up: stay ~60 s behind "now" so late-committing writes are re-delivered.
        let safe: Cursor = (db_now - Duration::seconds(CURSOR_SAFETY_MARGIN_SECS), 0, 0);
        input.max(safe)
    };

    Ok(PullResponse {
        entity: name.to_string(),
        items: page.into_iter().map(|r| r.0).collect(),
        next_cursor: format_cursor(&next),
        has_more,
        server_time: db_now,
    })
}

pub async fn bootstrap(db: &PgPool, company: Uuid, user: Uuid) -> ApiResult<Value> {
    let company_row: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('id', id, 'name', name, 'timezone', timezone, \
         'default_currency', default_currency::text) FROM companies WHERE id = $1",
    )
    .bind(company)
    .fetch_one(db)
    .await?;
    let units: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id, 'code', code, 'name', name, \
         'symbol', symbol, 'allows_decimal', allows_decimal) ORDER BY id), '[]'::jsonb) \
         FROM units_of_measure WHERE is_active",
    )
    .fetch_one(db)
    .await?;
    let permissions: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(jsonb_agg(DISTINCT p.permission_key), '[]'::jsonb)
           FROM user_roles ur
           JOIN roles r ON r.id = ur.role_id AND r.company_id = ur.company_id
           JOIN role_permissions rp ON rp.role_id = r.id
           JOIN permissions p ON p.id = rp.permission_id
           WHERE ur.user_id = $1 AND ur.company_id = $2 AND r.is_active = TRUE
             AND (ur.expires_at IS NULL OR ur.expires_at > NOW())"#,
    )
    .bind(user)
    .bind(company)
    .fetch_one(db)
    .await?;
    let server_time: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?;
    Ok(json!({
        "server_time": server_time,
        "company": company_row,
        "units": units,
        "permissions": permissions,
        "entities": ENTITIES,
        "operation_types": ["sale.checkout","expense.create","stock.adjust","customer.payment","cash.movement"],
        "max_operations_per_push": MAX_OPS_PER_PUSH,
    }))
}

// ------------------------------------------------------------------------ push

struct OpFailure {
    code: String,
    message: String,
    retryable: bool,
}

impl OpFailure {
    fn rule(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}

/// `InsufficientStock` -> `INSUFFICIENT_STOCK`. Database/internal errors are transient.
fn from_debug(dbg: &str, context: &str) -> OpFailure {
    let ident: String = dbg.chars().take_while(|c| c.is_alphanumeric()).collect();
    if ident.starts_with("Database") || ident.starts_with("Internal") || ident.is_empty() {
        tracing::error!("sync op {context}: transient failure: {dbg}");
        return OpFailure {
            code: "TEMPORARY_ERROR".into(),
            message: "A temporary server error occurred; resend the same operation".into(),
            retryable: true,
        };
    }
    let mut code = String::new();
    for (i, ch) in ident.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            code.push('_');
        }
        code.push(ch.to_ascii_uppercase());
    }
    OpFailure::rule(
        &code,
        format!(
            "The operation was rejected: {}",
            code.to_lowercase().replace('_', " ")
        ),
    )
}

fn from_api(e: ApiError) -> OpFailure {
    if e.status.is_server_error() {
        OpFailure {
            code: "TEMPORARY_ERROR".into(),
            message: e.message,
            retryable: true,
        }
    } else {
        OpFailure::rule(e.code, e.message)
    }
}

fn required_permissions(op_type: &str) -> Option<&'static [&'static str]> {
    Some(match op_type {
        "sale.checkout" => &["sales.create", "sales.checkout"],
        "expense.create" => &["expenses.manage"],
        "stock.adjust" => &["inventory.adjust"],
        "customer.payment" => &["customers.payment"],
        "cash.movement" => &["cash_register.movement"],
        _ => return None,
    })
}

fn bad_payload(e: impl std::fmt::Display) -> OpFailure {
    OpFailure::rule(
        "INVALID_PAYLOAD",
        format!("The operation payload is invalid: {e}"),
    )
}

#[derive(serde::Deserialize)]
struct SalePayload {
    location_id: i64,
    #[serde(default)]
    customer_id: Option<i64>,
    notes: Option<String>,
    items: Vec<SaleItemRequest>,
    register_id: Option<i64>,
    payments: Vec<SalePaymentRequest>,
}

async fn execute(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    op_id: Uuid,
    op_type: &str,
    payload: &Value,
) -> Result<Value, OpFailure> {
    let mut payload = payload.clone();
    match op_type {
        "sale.checkout" => {
            let p: SalePayload = serde_json::from_value(payload).map_err(bad_payload)?;
            // Already completed by an earlier attempt (e.g. we crashed before journalling)?
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM sales WHERE company_id=$1 AND checkout_idempotency_key=$2 AND status='completed'",
            )
            .bind(company)
            .bind(op_id)
            .fetch_optional(db)
            .await
            .map_err(|e| from_debug(&format!("Database({e})"), "sale lookup"))?;
            if let Some(id) = existing {
                let sale = sales_service::get_sale(db, company, id)
                    .await
                    .map_err(|e| from_debug(&format!("{e:?}"), "sale"))?;
                return Ok(sale_summary(&sale));
            }
            let draft = sales_service::create_sale(
                db,
                company,
                actor,
                CreateSaleRequest {
                    location_id: p.location_id,
                    customer_id: p.customer_id,
                    notes: p.notes,
                    items: p.items,
                },
            )
            .await
            .map_err(|e| from_debug(&format!("{e:?}"), "create sale"))?;
            match sales_service::checkout_sale(
                db,
                company,
                actor,
                draft.id,
                CheckoutRequest {
                    expected_version: draft.version,
                    idempotency_key: op_id,
                    register_id: p.register_id,
                    payments: p.payments,
                },
            )
            .await
            {
                Ok(outcome) => Ok(sale_summary(&outcome.sale)),
                Err(e) => {
                    // Do not leave an orphan draft behind.
                    let _ = sales_service::cancel_sale(db, company, actor, draft.id).await;
                    Err(from_debug(&format!("{e:?}"), "checkout"))
                }
            }
        }
        "expense.create" => {
            set_key(&mut payload, op_id)?;
            let req: ExpenseCreate = serde_json::from_value(payload).map_err(bad_payload)?;
            let outcome = expenses_service::create_expense(db, company, actor, req)
                .await
                .map_err(from_api)?;
            Ok(
                json!({ "expense_id": outcome.expense.id, "expense_number": outcome.expense.expense_number }),
            )
        }
        "stock.adjust" => {
            set_key(&mut payload, op_id)?;
            let req: StockAdjustmentRequest =
                serde_json::from_value(payload).map_err(bad_payload)?;
            let r = inventory_service::adjust_stock(db, company, actor, req)
                .await
                .map_err(|e| from_debug(&format!("{e:?}"), "adjust"))?;
            serde_json::to_value(r).map_err(|_| OpFailure::rule("INTERNAL", "serialisation"))
        }
        "customer.payment" => {
            set_key(&mut payload, op_id)?;
            let customer_id = take_i64(&mut payload, "customer_id")?;
            let req: CustomerPaymentCreate =
                serde_json::from_value(payload).map_err(bad_payload)?;
            let r = financial_operations_service::post_customer_payment(
                db,
                company,
                actor,
                customer_id,
                req,
            )
            .await
            .map_err(|e| from_debug(&format!("{e:?}"), "customer payment"))?;
            serde_json::to_value(r).map_err(|_| OpFailure::rule("INTERNAL", "serialisation"))
        }
        "cash.movement" => {
            set_key(&mut payload, op_id)?;
            let session_id = take_i64(&mut payload, "session_id")?;
            let req: CashMovementRequest = serde_json::from_value(payload).map_err(bad_payload)?;
            let r = cash_register_service::create_movement(db, company, actor, session_id, req)
                .await
                .map_err(|e| from_debug(&format!("{e:?}"), "cash movement"))?;
            serde_json::to_value(r).map_err(|_| OpFailure::rule("INTERNAL", "serialisation"))
        }
        other => Err(OpFailure::rule(
            "UNKNOWN_OP_TYPE",
            format!("Unsupported operation type: {other}"),
        )),
    }
}

fn sale_summary(sale: &crate::models::sales::Sale) -> Value {
    json!({
        "sale_id": sale.id,
        "sale_number": sale.sale_number,
        "status": sale.status,
        "total_amount": sale.total_amount.to_string(),
        "currency_code": sale.currency_code,
    })
}

fn set_key(payload: &mut Value, op_id: Uuid) -> Result<(), OpFailure> {
    let obj = payload
        .as_object_mut()
        .ok_or_else(|| bad_payload("payload must be a JSON object"))?;
    obj.insert("idempotency_key".into(), json!(op_id));
    Ok(())
}

fn take_i64(payload: &mut Value, key: &str) -> Result<i64, OpFailure> {
    payload
        .as_object_mut()
        .and_then(|o| o.remove(key))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| bad_payload(format!("'{key}' (integer) is required")))
}

fn rejected(op: &PushOperation, code: &str, message: String) -> PushResult {
    PushResult {
        client_op_id: op.client_op_id,
        op_type: op.op_type.clone(),
        status: "rejected",
        result: None,
        error: Some(OpError {
            code: code.into(),
            message,
        }),
        retryable: false,
    }
}

/// Runs the operation and records the outcome in the journal row `journal_id`.
async fn run_and_record(
    db: &PgPool,
    company: Uuid,
    journal_id: i64,
    actor: Uuid,
    op_id: Uuid,
    op_type: &str,
    payload: &Value,
) -> PushResult {
    let base = |status: &'static str,
                result: Option<Value>,
                error: Option<OpError>,
                retryable: bool| PushResult {
        client_op_id: op_id,
        op_type: op_type.to_string(),
        status,
        result,
        error,
        retryable,
    };

    // Re-check permissions at execution time (retries run as the original user).
    if let Some(keys) = required_permissions(op_type) {
        for key in keys {
            match has_permission(db, company, actor, key).await {
                Ok(true) => {}
                Ok(false) => {
                    let msg = format!("The user lacks the '{key}' permission");
                    record_rejection(db, journal_id, "PERMISSION_DENIED", &msg).await;
                    return base(
                        "rejected",
                        None,
                        Some(OpError {
                            code: "PERMISSION_DENIED".into(),
                            message: msg,
                        }),
                        false,
                    );
                }
                Err(e) => {
                    tracing::error!("sync permission lookup failed: {e}");
                    return base(
                        "error",
                        None,
                        Some(OpError {
                            code: "TEMPORARY_ERROR".into(),
                            message: "Temporary server error".into(),
                        }),
                        true,
                    );
                }
            }
        }
    }

    match execute(db, company, actor, op_id, op_type, payload).await {
        Ok(result) => {
            let _ = sqlx::query(
                "UPDATE sync_operations SET status='applied', result=$2, error_code=NULL, \
                 error_message=NULL, processed_at=NOW() WHERE id=$1",
            )
            .bind(journal_id)
            .bind(&result)
            .execute(db)
            .await
            .map_err(|e| tracing::error!("sync journal update failed: {e}"));
            base("applied", Some(result), None, false)
        }
        Err(f) if f.retryable => base(
            "error",
            None,
            Some(OpError {
                code: f.code,
                message: f.message,
            }),
            true,
        ),
        Err(f) => {
            record_rejection(db, journal_id, &f.code, &f.message).await;
            base(
                "rejected",
                None,
                Some(OpError {
                    code: f.code,
                    message: f.message,
                }),
                false,
            )
        }
    }
}

async fn record_rejection(db: &PgPool, journal_id: i64, code: &str, message: &str) {
    let _ = sqlx::query(
        "UPDATE sync_operations SET status='rejected', error_code=$2, error_message=$3, \
         result=NULL, processed_at=NOW() WHERE id=$1",
    )
    .bind(journal_id)
    .bind(code)
    .bind(message)
    .execute(db)
    .await
    .map_err(|e| tracing::error!("sync journal update failed: {e}"));
}

async fn process_one(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    device_id: Option<&str>,
    op: &PushOperation,
) -> Result<PushResult, sqlx::Error> {
    if required_permissions(&op.op_type).is_none() {
        return Ok(rejected(
            op,
            "UNKNOWN_OP_TYPE",
            format!("Unsupported operation type: {}", op.op_type),
        ));
    }
    if !op.payload.is_object() {
        return Ok(rejected(
            op,
            "INVALID_PAYLOAD",
            "payload must be a JSON object".into(),
        ));
    }

    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO sync_operations (company_id, client_op_id, device_id, user_id, op_type, payload, client_created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (company_id, client_op_id) DO NOTHING RETURNING id",
    )
    .bind(company)
    .bind(op.client_op_id)
    .bind(device_id)
    .bind(user)
    .bind(&op.op_type)
    .bind(&op.payload)
    .bind(op.client_created_at)
    .fetch_optional(db)
    .await?;

    if let Some(id) = inserted {
        return Ok(run_and_record(
            db,
            company,
            id,
            user,
            op.client_op_id,
            &op.op_type,
            &op.payload,
        )
        .await);
    }

    // Seen before: return the original outcome.
    let (id, status, payload, result, code, message, op_type, owner): (
        i64,
        String,
        Value,
        Option<Value>,
        Option<String>,
        Option<String>,
        String,
        Uuid,
    ) = sqlx::query_as(
        "SELECT id, status, payload, result, error_code, error_message, op_type, user_id \
         FROM sync_operations WHERE company_id=$1 AND client_op_id=$2",
    )
    .bind(company)
    .bind(op.client_op_id)
    .fetch_one(db)
    .await?;

    if payload != op.payload || op_type != op.op_type {
        return Ok(rejected(
            op,
            "IDEMPOTENCY_CONFLICT",
            "This client_op_id was already used with a different operation".into(),
        ));
    }

    Ok(match status.as_str() {
        "applied" => PushResult {
            client_op_id: op.client_op_id,
            op_type: op.op_type.clone(),
            status: "duplicate",
            result,
            error: None,
            retryable: false,
        },
        "rejected" | "dismissed" => PushResult {
            client_op_id: op.client_op_id,
            op_type: op.op_type.clone(),
            status: "rejected",
            result: None,
            retryable: false,
            error: Some(OpError {
                code: code.unwrap_or_else(|| "REJECTED".into()),
                message: message.unwrap_or_else(|| "The operation was rejected earlier".into()),
            }),
        },
        _ => {
            // Still marked processing: re-claim if the first attempt looks abandoned.
            let claimed: Option<i64> = sqlx::query_scalar(
                "UPDATE sync_operations SET attempts = attempts + 1, received_at = NOW() \
                 WHERE id=$1 AND status='processing' AND received_at < NOW() - make_interval(secs => $2) RETURNING id",
            )
            .bind(id)
            .bind(RECLAIM_AFTER_SECS as f64)
            .fetch_optional(db)
            .await?;
            match claimed {
                Some(_) => {
                    run_and_record(
                        db,
                        company,
                        id,
                        owner,
                        op.client_op_id,
                        &op.op_type,
                        &op.payload,
                    )
                    .await
                }
                None => PushResult {
                    client_op_id: op.client_op_id,
                    op_type: op.op_type.clone(),
                    status: "in_progress",
                    result: None,
                    error: None,
                    retryable: true,
                },
            }
        }
    })
}

pub async fn push(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    req: PushRequest,
) -> ApiResult<PushResponse> {
    if req.operations.is_empty() || req.operations.len() > MAX_OPS_PER_PUSH {
        return Err(ApiError::bad_request(
            "INVALID_BATCH",
            format!("Send between 1 and {MAX_OPS_PER_PUSH} operations per request"),
        ));
    }
    let device = req
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if device.is_some_and(|d| d.chars().count() > 100) {
        return Err(ApiError::bad_request(
            "INVALID_DEVICE_ID",
            "device_id must be at most 100 characters",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    if !req.operations.iter().all(|o| seen.insert(o.client_op_id)) {
        return Err(ApiError::bad_request(
            "DUPLICATE_OP_ID",
            "client_op_id values must be unique within a request",
        ));
    }

    // Strictly in order: later operations (e.g. cash movements) may depend on earlier ones.
    let mut results = Vec::with_capacity(req.operations.len());
    for op in &req.operations {
        results.push(process_one(db, company, user, device, op).await?);
    }
    let server_time: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(db)
        .await?;
    Ok(PushResponse {
        results,
        server_time,
    })
}

// ------------------------------------------------------------ journal / review

pub async fn list_operations(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    q: OperationsQuery,
) -> ApiResult<Vec<SyncOperation>> {
    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(ApiError::bad_request(
            "INVALID_QUERY",
            "limit must be 1-200 and offset >= 0",
        ));
    }
    if let Some(s) = &q.status {
        if !["processing", "applied", "rejected", "dismissed"].contains(&s.as_str()) {
            return Err(ApiError::bad_request("INVALID_QUERY", "unknown status"));
        }
    }
    let manager = has_permission(db, company, user, "sync.manage").await?;
    Ok(sqlx::query_as::<_, SyncOperation>(
        "SELECT id, client_op_id, device_id, user_id, op_type, status, payload, result, error_code, \
         error_message, attempts, client_created_at, received_at, processed_at, resolution_note \
         FROM sync_operations WHERE company_id=$1 AND ($2 OR user_id=$3) \
         AND ($4::text IS NULL OR status=$4) ORDER BY id DESC LIMIT $5 OFFSET $6",
    )
    .bind(company)
    .bind(manager)
    .bind(user)
    .bind(q.status)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?)
}

/// Re-runs a rejected operation (after the cause, e.g. missing stock, has been fixed).
/// By default it executes as the original user so the sale stays attributed to the cashier.
/// With `run_as_reviewer` it executes under the reviewing manager's authority (their permissions
/// apply and the record is attributed to them); the journal records who approved it.
pub async fn retry_operation(
    db: &PgPool,
    company: Uuid,
    id: i64,
    reviewer: Uuid,
    req: RetryRequest,
) -> ApiResult<PushResult> {
    let row: Option<(i64, Uuid, Uuid, String, Value)> = sqlx::query_as(
        "UPDATE sync_operations SET status='processing', attempts=attempts+1, received_at=NOW() \
         WHERE id=$1 AND company_id=$2 AND status='rejected' \
         RETURNING id, client_op_id, user_id, op_type, payload",
    )
    .bind(id)
    .bind(company)
    .fetch_optional(db)
    .await?;
    let Some((id, op_id, owner, op_type, payload)) = row else {
        return Err(ApiError::conflict(
            "NOT_RETRYABLE",
            "Only rejected operations can be retried",
        ));
    };
    let actor = if req.run_as_reviewer { reviewer } else { owner };
    let result = run_and_record(db, company, id, actor, op_id, &op_type, &payload).await;
    if req.run_as_reviewer && result.status == "applied" {
        let _ = sqlx::query("UPDATE sync_operations SET resolution_note=$2 WHERE id=$1")
            .bind(id)
            .bind(format!(
                "Approved and executed by reviewer {reviewer} (originally submitted by {owner})"
            ))
            .execute(db)
            .await;
    }
    Ok(result)
}

pub async fn dismiss_operation(
    db: &PgPool,
    company: Uuid,
    id: i64,
    req: DismissRequest,
) -> ApiResult<()> {
    let note = req.note.trim();
    if note.is_empty() || note.chars().count() > 500 {
        return Err(ApiError::bad_request(
            "INVALID_NOTE",
            "A note (max 500 characters) is required",
        ));
    }
    let n = sqlx::query(
        "UPDATE sync_operations SET status='dismissed', resolution_note=$3, processed_at=NOW() \
         WHERE id=$1 AND company_id=$2 AND status='rejected'",
    )
    .bind(id)
    .bind(company)
    .bind(note)
    .execute(db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::conflict(
            "NOT_DISMISSIBLE",
            "Only rejected operations can be dismissed",
        ));
    }
    Ok(())
}
