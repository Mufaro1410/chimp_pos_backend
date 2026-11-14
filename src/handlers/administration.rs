use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    models::{
        administration::*,
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
    },
    services::administration_service::{self, AdminError},
    state::SharedState,
};

fn error(e: AdminError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match e {
        AdminError::NotFound => (
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The requested resource was not found",
        ),
        AdminError::Invalid(m) => (StatusCode::BAD_REQUEST, "INVALID_REQUEST", m),
        AdminError::Conflict(m) => (StatusCode::CONFLICT, "CONFLICT", m),
        AdminError::Database(e) => {
            eprintln!("Administration database error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "An internal database error occurred",
            )
        }
        AdminError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "An internal error occurred",
        ),
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
#[derive(Deserialize)]
pub struct Pagination {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn get_shop(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> Result<Json<Shop>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::shop(&s.db, a.company_id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn update_shop(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(q): Json<ShopUpdate>,
) -> Result<Json<Shop>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::update_shop(&s.db, a.company_id, q)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn list_users(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(p): Query<Pagination>,
) -> Result<Json<Page<ManagedUser>>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::users(
        &s.db,
        a.company_id,
        p.limit.unwrap_or(50),
        p.offset.unwrap_or(0),
    )
    .await
    .map(Json)
    .map_err(error)
}
pub async fn get_user(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ManagedUser>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::user(&s.db, a.company_id, id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn create_user(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(q): Json<UserCreate>,
) -> Result<(StatusCode, Json<ManagedUser>), (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::create_user(&s.db, a.company_id, a.user_id, q)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(error)
}
pub async fn update_user(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(q): Json<UserUpdate>,
) -> Result<Json<ManagedUser>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::update_user(&s.db, a.company_id, id, q)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn assign_roles(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(q): Json<UserRoleUpdate>,
) -> Result<Json<ManagedUser>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::assign_roles(&s.db, a.company_id, id, a.user_id, q)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn list_roles(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> Result<Json<Vec<Role>>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::roles(&s.db, a.company_id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn get_role(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Role>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::role(&s.db, a.company_id, id)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn list_permissions(
    State(s): State<SharedState>,
    Extension(_a): Extension<AuthUser>,
) -> Result<Json<Vec<Permission>>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::permission_catalog(&s.db)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn update_role_permissions(
    State(s): State<SharedState>,
    Extension(_a): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(q): Json<RolePermissionUpdate>,
) -> Result<Json<Role>, (StatusCode, Json<ApiErrorResponse>)> {
    administration_service::update_role_permissions(&s.db, _a.company_id, id, q)
        .await
        .map(Json)
        .map_err(error)
}
