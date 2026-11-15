use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    errors::api_error::ApiResult,
    models::{auth::AuthUser, session::*},
    services::session_service as svc,
    state::SharedState,
};

pub async fn refresh(
    State(s): State<SharedState>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<TokenResponse>> {
    Ok(Json(svc::refresh(&s.db, &s.jwt_secret, req).await?))
}

pub async fn logout(
    State(s): State<SharedState>,
    Json(req): Json<LogoutRequest>,
) -> ApiResult<StatusCode> {
    svc::logout(&s.db, req).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn logout_all(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> ApiResult<StatusCode> {
    svc::logout_all(&s.db, a.company_id, a.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn change_password(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> ApiResult<Json<TokenResponse>> {
    Ok(Json(
        svc::change_password(&s.db, &s.jwt_secret, a.company_id, a.user_id, req).await?,
    ))
}

pub async fn admin_reset_password(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<AdminResetPasswordRequest>,
) -> ApiResult<StatusCode> {
    svc::admin_reset_password(&s.db, a.company_id, a.user_id, id, req).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn forgot_password(
    State(s): State<SharedState>,
    Json(req): Json<ForgotPasswordRequest>,
) -> ApiResult<StatusCode> {
    svc::forgot_password(&s.db, req).await?;
    // Same answer whether or not the account exists.
    Ok(StatusCode::ACCEPTED)
}

pub async fn reset_password(
    State(s): State<SharedState>,
    Json(req): Json<ResetPasswordRequest>,
) -> ApiResult<StatusCode> {
    svc::reset_password(&s.db, req).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> ApiResult<Json<Value>> {
    Ok(Json(svc::me(&s.db, a.company_id, a.user_id).await?))
}
