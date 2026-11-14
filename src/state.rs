use sqlx::PgPool;
use std::sync::Arc;

pub struct AppState {
    pub db: PgPool,
    pub jwt_secret: String,
}

pub type SharedState = Arc<AppState>;
