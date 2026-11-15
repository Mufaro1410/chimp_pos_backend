use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde_json::Value;

use crate::{
    errors::api_error::ApiResult,
    models::{auth::AuthUser, sync::*},
    services::sync_service as svc,
    state::SharedState,
};

pub async fn bootstrap(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> ApiResult<Json<Value>> {
    Ok(Json(svc::bootstrap(&s.db, a.company_id, a.user_id).await?))
}

pub async fn pull(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(entity): Path<String>,
    Query(q): Query<PullQuery>,
) -> ApiResult<Json<PullResponse>> {
    Ok(Json(
        svc::pull(&s.db, a.company_id, a.user_id, &entity, q).await?,
    ))
}

pub async fn push(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(req): Json<PushRequest>,
) -> ApiResult<Json<PushResponse>> {
    Ok(Json(svc::push(&s.db, a.company_id, a.user_id, req).await?))
}

pub async fn list_operations(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(q): Query<OperationsQuery>,
) -> ApiResult<Json<Vec<SyncOperation>>> {
    Ok(Json(
        svc::list_operations(&s.db, a.company_id, a.user_id, q).await?,
    ))
}

pub async fn retry_operation(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    body: axum::body::Bytes,
) -> ApiResult<Json<PushResult>> {
    // The body is optional: an empty body means "retry as the original user".
    let req: RetryRequest = if body.iter().all(u8::is_ascii_whitespace) {
        RetryRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            crate::errors::api_error::ApiError::bad_request(
                "INVALID_BODY",
                format!("Invalid request body: {e}"),
            )
        })?
    };
    Ok(Json(
        svc::retry_operation(&s.db, a.company_id, id, a.user_id, req).await?,
    ))
}

pub async fn dismiss_operation(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<DismissRequest>,
) -> ApiResult<StatusCode> {
    svc::dismiss_operation(&s.db, a.company_id, id, req).await?;
    Ok(StatusCode::NO_CONTENT)
}
