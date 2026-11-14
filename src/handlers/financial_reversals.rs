use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::{
        auth::AuthUser,
        financial_reversals::{CreditNoteReversalRequest, PaymentReversalRequest, ReversalOutcome},
    },
    services::financial_reversals_service::{self as svc, ReversalError},
    state::SharedState,
};

fn map(e: ReversalError) -> ApiError {
    match e {
        ReversalError::Database(e) => e.into(),
        ReversalError::Invalid => ApiError::bad_request(
            "INVALID_REVERSAL",
            "The reversal request is invalid (check amount, reason, payment method / cash session and allocations)",
        ),
        ReversalError::NotFound => ApiError::not_found(
            "SOURCE_NOT_FOUND",
            "The original payment or credit note was not found",
        ),
        ReversalError::IdempotencyConflict => ApiError::conflict(
            "IDEMPOTENCY_CONFLICT",
            "This idempotency key was already used with a different request",
        ),
    }
}

fn respond(o: ReversalOutcome) -> (StatusCode, Json<ReversalOutcome>) {
    let status = if o.replayed { StatusCode::OK } else { StatusCode::CREATED };
    (status, Json(o))
}

pub async fn reverse_customer_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<PaymentReversalRequest>,
) -> ApiResult<(StatusCode, Json<ReversalOutcome>)> {
    let o = svc::reverse_customer_payment(&s.db, a.company_id, a.user_id, id, req)
        .await
        .map_err(map)?;
    Ok(respond(o))
}

pub async fn reverse_supplier_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<PaymentReversalRequest>,
) -> ApiResult<(StatusCode, Json<ReversalOutcome>)> {
    let o = svc::reverse_supplier_payment(&s.db, a.company_id, a.user_id, id, req)
        .await
        .map_err(map)?;
    Ok(respond(o))
}

pub async fn reverse_customer_credit_note(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<CreditNoteReversalRequest>,
) -> ApiResult<(StatusCode, Json<ReversalOutcome>)> {
    let o = svc::reverse_credit_note(&s.db, a.company_id, a.user_id, id, req, true)
        .await
        .map_err(map)?;
    Ok(respond(o))
}

pub async fn reverse_supplier_credit_note(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<CreditNoteReversalRequest>,
) -> ApiResult<(StatusCode, Json<ReversalOutcome>)> {
    let o = svc::reverse_credit_note(&s.db, a.company_id, a.user_id, id, req, false)
        .await
        .map_err(map)?;
    Ok(respond(o))
}

async fn payment_detail(
    db: &sqlx::PgPool,
    company: uuid::Uuid,
    id: i64,
    customer: bool,
) -> ApiResult<serde_json::Value> {
    let (payment, pay_rev, alloc, alloc_rev, doc_col) = if customer {
        ("customer_payments", "customer_payment_reversals", "customer_payment_allocations", "customer_allocation_reversals", "sale_id")
    } else {
        ("supplier_payments", "supplier_payment_reversals", "supplier_payment_allocations", "supplier_allocation_reversals", "invoice_id")
    };
    let sql = format!(
        "SELECT (to_jsonb(p) - 'request_fingerprint') || jsonb_build_object( \
           'reversed_amount', COALESCE((SELECT SUM(r.amount) FROM {pay_rev} r \
               WHERE r.payment_id = p.id AND r.company_id = p.company_id), 0)::text, \
           'allocations', COALESCE((SELECT jsonb_agg(jsonb_build_object( \
               'allocation_id', a.id, 'document_id', a.{doc_col}, 'amount', a.amount::text, \
               'reversed_amount', COALESCE((SELECT SUM(ar.amount) FROM {alloc_rev} ar \
                   WHERE ar.allocation_id = a.id AND ar.company_id = a.company_id), 0)::text \
             ) ORDER BY a.id) FROM {alloc} a \
             WHERE a.payment_id = p.id AND a.company_id = p.company_id), '[]'::jsonb)) \
         FROM {payment} p WHERE p.id = $1 AND p.company_id = $2"
    );
    sqlx::query_scalar::<_, serde_json::Value>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(company)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| ApiError::not_found("PAYMENT_NOT_FOUND", "Payment not found"))
}

/// Returns a payment with its allocations and how much of each has already been reversed
/// (the allocation ids are needed to build a reversal request).
pub async fn get_customer_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(payment_detail(&s.db, a.company_id, id, true).await?))
}

pub async fn get_supplier_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(payment_detail(&s.db, a.company_id, id, false).await?))
}
