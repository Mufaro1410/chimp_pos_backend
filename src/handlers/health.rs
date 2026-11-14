use axum::{Json, extract::State, http::StatusCode};

use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
}

pub async fn health_check(
    State(state): State<SharedState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|err| {
            eprintln!("Health check database error: {err}");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    Ok(Json(HealthResponse {
        status: "ok".to_string(),
        database: "connected".to_string(),
    }))
}
