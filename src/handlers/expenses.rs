use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde_json::Value;

use crate::{
    errors::api_error::ApiResult,
    models::{auth::AuthUser, expenses::*},
    services::expenses_service as svc,
    state::SharedState,
};

pub async fn list_expenses(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(q): Query<ExpenseQuery>,
) -> ApiResult<Json<Vec<Expense>>> {
    Ok(Json(svc::list_expenses(&s.db, a.company_id, q).await?))
}

pub async fn expense_summary(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Query(q): Query<ExpenseSummaryQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(svc::summary(&s.db, a.company_id, q).await?))
}

pub async fn get_expense(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Expense>> {
    Ok(Json(svc::get_expense(&s.db, a.company_id, id).await?))
}

pub async fn create_expense(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Json(req): Json<ExpenseCreate>,
) -> ApiResult<(StatusCode, Json<ExpenseOutcome>)> {
    let outcome = svc::create_expense(&s.db, a.company_id, a.user_id, req).await?;
    let status = if outcome.replayed { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(outcome)))
}

pub async fn void_expense(
    State(s): State<SharedState>,
    Extension(a): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(req): Json<ExpenseVoid>,
) -> ApiResult<Json<Expense>> {
    Ok(Json(svc::void_expense(&s.db, a.company_id, a.user_id, id, req).await?))
}
