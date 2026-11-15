use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::{
    errors::api_error::ApiResult,
    models::{auth::AuthUser, catalog::*},
    services::catalog_service as svc,
    state::SharedState,
};

pub async fn list_units(State(s): State<SharedState>) -> ApiResult<Json<Vec<Unit>>> {
    Ok(Json(svc::list_units(&s.db).await?))
}

pub async fn list_categories(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> ApiResult<Json<Vec<Category>>> {
    Ok(Json(svc::list_categories(&s.db, a.company_id).await?))
}

pub async fn create_category(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(req): Json<CategoryCreate>,
) -> ApiResult<(StatusCode, Json<Category>)> {
    Ok((
        StatusCode::CREATED,
        Json(svc::create_category(&s.db, a.company_id, req).await?),
    ))
}

pub async fn update_category(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<CategoryUpdate>,
) -> ApiResult<Json<Category>> {
    Ok(Json(
        svc::update_category(&s.db, a.company_id, id, req).await?,
    ))
}

pub async fn list_locations(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
) -> ApiResult<Json<Vec<Location>>> {
    Ok(Json(svc::list_locations(&s.db, a.company_id).await?))
}

pub async fn create_location(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(req): Json<LocationCreate>,
) -> ApiResult<(StatusCode, Json<Location>)> {
    Ok((
        StatusCode::CREATED,
        Json(svc::create_location(&s.db, a.company_id, req).await?),
    ))
}

pub async fn update_location(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<LocationUpdate>,
) -> ApiResult<Json<Location>> {
    Ok(Json(
        svc::update_location(&s.db, a.company_id, id, req).await?,
    ))
}
