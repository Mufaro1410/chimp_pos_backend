use axum::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("{0}")]
    Validation(String),

    #[error("Username or email already exists")]
    AlreadyExists,

    #[error("Resource not found")]
    NotFound,

    #[error("Authentication failed")]
    Unauthorized,

    #[error("Database operation failed")]
    Database(#[from] sqlx::Error),

    #[error("Password hashing failed")]
    PasswordHashing(String),

    #[error("Internal server error")]
    Internal,
}

impl AuthError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::AlreadyExists => StatusCode::CONFLICT,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Database(_) | Self::PasswordHashing(_) | Self::Internal => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Validation(message) => message.clone(),
            Self::AlreadyExists => "Username or email already exists".into(),
            Self::NotFound => "Resource not found".into(),
            Self::Unauthorized => "Invalid credentials".into(),
            _ => "An unexpected error occurred".into(),
        }
    }
}
