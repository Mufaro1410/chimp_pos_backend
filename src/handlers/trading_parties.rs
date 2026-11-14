// src/handlers/trading_parties.rs
use crate::{
    models::trading_parties::*,
    models::{
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
    },
    services::trading_parties_service::{self, TradingPartyError},
    state::SharedState,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
fn err(e: TradingPartyError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (s, c, m) = match e {
        TradingPartyError::NotFound => (
            StatusCode::NOT_FOUND,
            "RECORD_NOT_FOUND",
            "The requested record was not found",
        ),
        TradingPartyError::Invalid => (
            StatusCode::BAD_REQUEST,
            "INVALID_TRADING_PARTY",
            "Check the name, credit limit and payment terms",
        ),
        TradingPartyError::InvalidQuery => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUERY",
            "Invalid pagination or filter parameters (limit must be 1-100, offset >= 0)",
        ),
        TradingPartyError::VersionConflict => (
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            "The record changed; refresh before updating",
        ),
        TradingPartyError::Forbidden => (
            StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            "The customer credit permission is required",
        ),
        TradingPartyError::Database(e) => {
            eprintln!("Trading party database error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "An internal database error occurred",
            )
        }
    };
    (
        s,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: c.into(),
                message: m.into(),
            },
        }),
    )
}

pub async fn list_customers(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<TradingPartyListQuery>,
) -> Result<Json<Vec<Customer>>, (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::customers(&state.db, auth.company_id, query)
        .await
        .map(Json)
        .map_err(err)
}

pub async fn create_customer(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(q): Json<CustomerCreate>,
) -> Result<(StatusCode, Json<Customer>), (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::create_customer(&s.db, a.company_id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(err)
}
pub async fn update_customer(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<CustomerUpdate>,
) -> Result<Json<Customer>, (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::update_customer(&s.db, a.company_id, id, q)
        .await
        .map(Json)
        .map_err(err)
}
pub async fn update_customer_credit_policy(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<CustomerCreditPolicyUpdate>,
) -> Result<Json<Customer>, (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::update_customer_credit_policy(&s.db, a.company_id, a.user_id, id, q)
        .await
        .map(Json)
        .map_err(err)
}
pub async fn list_suppliers(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(query): Query<TradingPartyListQuery>,
) -> Result<Json<Vec<Supplier>>, (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::suppliers(&s.db, a.company_id, query)
        .await
        .map(Json)
        .map_err(err)
}

pub async fn create_supplier(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(q): Json<SupplierCreate>,
) -> Result<(StatusCode, Json<Supplier>), (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::create_supplier(&s.db, a.company_id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(err)
}
pub async fn update_supplier(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(q): Json<SupplierUpdate>,
) -> Result<Json<Supplier>, (StatusCode, Json<ApiErrorResponse>)> {
    trading_parties_service::update_supplier(&s.db, a.company_id, id, q)
        .await
        .map(Json)
        .map_err(err)
}
