use axum::{
    Json,
    extract::{Extension, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::auth::AuthUser,
    state::SharedState,
};

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuditQuery {
    pub user_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<Uuid>,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub created_at: DateTime<Utc>,
}

pub async fn list_audit_logs(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuditEntry>>> {
    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(ApiError::bad_request(
            "INVALID_QUERY",
            "limit must be 1-200 and offset >= 0",
        ));
    }
    Ok(Json(
        sqlx::query_as::<_, AuditEntry>(
            "SELECT id, user_id, method, path, status_code, created_at FROM audit_logs \
             WHERE company_id=$1 AND ($2::uuid IS NULL OR user_id=$2) \
             ORDER BY id DESC LIMIT $3 OFFSET $4",
        )
        .bind(a.company_id)
        .bind(q.user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&s.db)
        .await?,
    ))
}
