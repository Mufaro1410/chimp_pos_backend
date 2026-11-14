use crate::{
    models::{
        auth::AuthUser,
        financial_operations::*,
        product::{ApiErrorDetail, ApiErrorResponse},
    },
    services::financial_operations_service::{self, FinanceError},
    state::SharedState,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};

fn error(e: FinanceError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match e {
        FinanceError::NotFound => (
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The requested financial record was not found",
        ),
        FinanceError::Invalid(m) => (StatusCode::BAD_REQUEST, "INVALID_FINANCIAL_REQUEST", m),
        FinanceError::Forbidden => (
            StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            "The required financial permission is missing",
        ),
        FinanceError::Conflict(m) => (StatusCode::CONFLICT, "FINANCIAL_CONFLICT", m),
        FinanceError::Database(e) => {
            eprintln!("Financial operation database error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "An internal database error occurred",
            )
        }
    };
    (
        status,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }),
    )
}
fn page(q: StatementQuery) -> (i64, i64) {
    (
        q.limit.unwrap_or(50).clamp(1, 100),
        q.offset.unwrap_or(0).max(0),
    )
}

pub async fn customer_balance(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<AccountBalance>, (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::customer_balance(&s.db, a.company_id, id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn customer_statement(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(q): Query<StatementQuery>,
) -> Result<Json<Page<StatementLine>>, (StatusCode, Json<ApiErrorResponse>)> {
    let (l, o) = page(q);
    financial_operations_service::customer_statement(&s.db, a.company_id, id, l, o)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn customer_outstanding(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(q): Query<StatementQuery>,
) -> Result<Json<Page<OutstandingDocument>>, (StatusCode, Json<ApiErrorResponse>)> {
    let (l, o) = page(q);
    financial_operations_service::customer_outstanding(&s.db, a.company_id, id, l, o)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn post_customer_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<CustomerPaymentCreate>,
) -> Result<(StatusCode, Json<PostingResult>), (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::post_customer_payment(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}
pub async fn post_customer_credit_note(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<CreditNoteCreate>,
) -> Result<(StatusCode, Json<PostingResult>), (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::post_customer_credit_note(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}

pub async fn supplier_balance(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<AccountBalance>, (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::supplier_balance(&s.db, a.company_id, id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn supplier_statement(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(q): Query<StatementQuery>,
) -> Result<Json<Page<StatementLine>>, (StatusCode, Json<ApiErrorResponse>)> {
    let (l, o) = page(q);
    financial_operations_service::supplier_statement(&s.db, a.company_id, id, l, o)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn supplier_outstanding(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(q): Query<StatementQuery>,
) -> Result<Json<Page<OutstandingDocument>>, (StatusCode, Json<ApiErrorResponse>)> {
    let (l, o) = page(q);
    financial_operations_service::supplier_outstanding(&s.db, a.company_id, id, l, o)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn supplier_invoices(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(q): Query<StatementQuery>,
) -> Result<Json<Page<SupplierInvoice>>, (StatusCode, Json<ApiErrorResponse>)> {
    let (l, o) = page(q);
    financial_operations_service::supplier_invoices(&s.db, a.company_id, id, l, o)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn create_supplier_invoice(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<SupplierInvoiceCreate>,
) -> Result<(StatusCode, Json<SupplierInvoice>), (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::create_supplier_invoice(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}
pub async fn get_supplier_invoice(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<SupplierInvoice>, (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::get_supplier_invoice(&s.db, a.company_id, id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn post_supplier_payment(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<SupplierPaymentCreate>,
) -> Result<(StatusCode, Json<PostingResult>), (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::post_supplier_payment(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}
pub async fn post_supplier_credit_note(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<CreditNoteCreate>,
) -> Result<(StatusCode, Json<PostingResult>), (StatusCode, Json<ApiErrorResponse>)> {
    financial_operations_service::post_supplier_credit_note(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}
