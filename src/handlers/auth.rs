use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};

use crate::{
    models::auth::{
        AuthMeResponse, AuthUser, LoginRequest, LoginResponse, RegisterRequest, RegisterResponse,
    },
    services::auth_service::{login, register_company},
    state::SharedState,
};

pub async fn register(
    State(state): State<SharedState>,
    Json(request): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), (StatusCode, String)> {
    let response = register_company(&state.db, request).await.map_err(|err| {
        eprintln!("Registration error: {err:?}");

        (err.status_code(), err.public_message())
    })?;

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login_handler(
    State(state): State<SharedState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, String)> {
    let response = login(&state, request).await.map_err(|err| {
        eprintln!("Login error: {err:?}");
        (err.status_code(), err.public_message())
    })?;

    Ok(Json(response))
}

pub async fn me_handler(Extension(auth): Extension<AuthUser>) -> Json<AuthMeResponse> {
    Json(AuthMeResponse {
        user_id: auth.user_id,
        company_id: auth.company_id,
    })
}

/// Issues a fresh access token for a still-valid token. Clients should call this
/// before expiry (e.g. at 80% of `expires_in`).
pub async fn refresh_handler(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let token = crate::utils::jwt::generate_access_token(
        auth.user_id,
        auth.company_id,
        &state.jwt_secret,
    )
    .map_err(|err| (err.status_code(), err.public_message()))?;
    Ok(Json(serde_json::json!({
        "access_token": token,
        "token_type": "Bearer",
        "expires_in": crate::utils::jwt::token_ttl_seconds(),
    })))
}
