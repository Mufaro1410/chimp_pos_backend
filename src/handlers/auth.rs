use axum::{Json, extract::State, http::StatusCode};

use crate::{
    errors::api_error::ApiResult,
    models::auth::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse},
    services::auth_service::{login, register_company},
    state::SharedState,
};

pub async fn register(
    State(state): State<SharedState>,
    Json(request): Json<RegisterRequest>,
) -> ApiResult<(StatusCode, Json<RegisterResponse>)> {
    let response = register_company(&state.db, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login_handler(
    State(state): State<SharedState>,
    Json(request): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    Ok(Json(login(&state, request).await?))
}
