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

    #[error("Too many failed attempts")]
    TooManyAttempts,

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
            Self::TooManyAttempts => StatusCode::TOO_MANY_REQUESTS,
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
            Self::TooManyAttempts => {
                "Too many failed sign-in attempts. Please try again later".into()
            }
            _ => "An unexpected error occurred".into(),
        }
    }
}

impl From<AuthError> for crate::errors::api_error::ApiError {
    fn from(err: AuthError) -> Self {
        use crate::errors::api_error::ApiError;
        match &err {
            AuthError::Validation(m) => ApiError::bad_request("VALIDATION_ERROR", m.clone()),
            AuthError::AlreadyExists => ApiError::conflict("ALREADY_EXISTS", err.public_message()),
            AuthError::NotFound => ApiError::not_found("NOT_FOUND", err.public_message()),
            AuthError::Unauthorized => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                err.public_message(),
            ),
            AuthError::TooManyAttempts => ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "ACCOUNT_LOCKED",
                err.public_message(),
            ),
            _ => {
                tracing::error!("authentication error: {err:?}");
                ApiError::internal()
            }
        }
    }
}
