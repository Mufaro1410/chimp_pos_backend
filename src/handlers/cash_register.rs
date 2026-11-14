use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    models::{
        auth::AuthUser,
        cash_register::*,
        product::{ApiErrorDetail, ApiErrorResponse},
    },
    services::cash_register_service::{self, CashRegisterError},
    state::SharedState,
};

#[derive(Deserialize)]
pub struct RegisterQuery {
    pub location_id: Option<i64>,
}

fn map_error(error: CashRegisterError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        CashRegisterError::NotFound => (
            StatusCode::NOT_FOUND,
            "CASH_REGISTER_NOT_FOUND",
            "The cash register or session was not found",
        ),
        CashRegisterError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "INVALID_CASH_REGISTER_REQUEST",
            "The supplied cash register request is invalid",
        ),
        CashRegisterError::InvalidAmount => (
            StatusCode::BAD_REQUEST,
            "INVALID_CASH_AMOUNT",
            "The cash amount must be non-negative, valid and have at most four decimal places",
        ),
        CashRegisterError::InsufficientCash => (
            StatusCode::CONFLICT,
            "INSUFFICIENT_REGISTER_CASH",
            "The register does not have enough available cash for this movement",
        ),
        CashRegisterError::RegisterInactive => (
            StatusCode::CONFLICT,
            "REGISTER_INACTIVE",
            "This cash register is inactive",
        ),
        CashRegisterError::RegisterAlreadyExists => (
            StatusCode::CONFLICT,
            "REGISTER_ALREADY_EXISTS",
            "A register with this code already exists at this location",
        ),
        CashRegisterError::SessionAlreadyOpen => (
            StatusCode::CONFLICT,
            "SESSION_ALREADY_OPEN",
            "This register already has an open session",
        ),
        CashRegisterError::SessionNotOpen => (
            StatusCode::CONFLICT,
            "SESSION_NOT_OPEN",
            "This register session is not open",
        ),
        CashRegisterError::SessionNotClosed => (
            StatusCode::CONFLICT,
            "SESSION_NOT_CLOSED",
            "This register session has not been closed",
        ),
        CashRegisterError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "IDEMPOTENCY_CONFLICT",
            "This idempotency key was already used with different details",
        ),
        CashRegisterError::Database(error) => {
            eprintln!("Cash register database error: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "CASH_REGISTER_DATABASE_ERROR",
                "An internal cash register error occurred",
            )
        }
    };

    (
        status,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: code.to_string(),
                message: message.to_string(),
            },
        }),
    )
}

pub async fn create_register(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Json(request): Json<CreateCashRegisterRequest>,
) -> Result<(StatusCode, Json<CashRegister>), (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::create_register(&state.db, user.company_id, user.user_id, request)
        .await
        .map(|register| (StatusCode::CREATED, Json(register)))
        .map_err(map_error)
}

pub async fn list_registers(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<RegisterQuery>,
) -> Result<Json<Vec<CashRegister>>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::list_registers(&state.db, user.company_id, query.location_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn open_session(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(register_id): Path<i64>,
    Json(request): Json<OpenCashSessionRequest>,
) -> Result<(StatusCode, Json<CashRegisterSession>), (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::open_session(
        &state.db,
        user.company_id,
        user.user_id,
        register_id,
        request,
    )
    .await
    .map(|session| (StatusCode::CREATED, Json(session)))
    .map_err(map_error)
}

pub async fn get_open_session(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(register_id): Path<i64>,
) -> Result<Json<CashRegisterSession>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::get_open_session(&state.db, user.company_id, register_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn get_session(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<i64>,
) -> Result<Json<CashRegisterSession>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::get_session(&state.db, user.company_id, session_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn get_session_summary(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<i64>,
) -> Result<Json<CashSessionSummary>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::get_session_summary(&state.db, user.company_id, session_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn create_movement(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<i64>,
    Json(request): Json<CashMovementRequest>,
) -> Result<(StatusCode, Json<CashMovement>), (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::create_movement(
        &state.db,
        user.company_id,
        user.user_id,
        session_id,
        request,
    )
    .await
    .map(|movement| (StatusCode::CREATED, Json(movement)))
    .map_err(map_error)
}

pub async fn list_movements(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<i64>,
) -> Result<Json<Vec<CashMovement>>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::list_movements(&state.db, user.company_id, session_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn close_session(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<i64>,
    Json(request): Json<CloseCashSessionRequest>,
) -> Result<Json<CashRegisterSession>, (StatusCode, Json<ApiErrorResponse>)> {
    cash_register_service::close_session(
        &state.db,
        user.company_id,
        user.user_id,
        session_id,
        request,
    )
    .await
    .map(Json)
    .map_err(map_error)
}
