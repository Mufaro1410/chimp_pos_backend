//! Shared JSON error type for new handlers. Produces the same body shape as the
//! existing modules: `{"error": {"code": "...", "message": "..."}}`.
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }
    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_SERVER_ERROR",
            "An unexpected error occurred",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &error {
            match db.code().as_deref() {
                Some("23505") => {
                    return Self::conflict(
                        "ALREADY_EXISTS",
                        "A record with these details already exists",
                    );
                }
                Some("23503") => {
                    return Self::bad_request(
                        "INVALID_REFERENCE",
                        "A referenced record does not exist or is not accessible",
                    );
                }
                Some("23514") | Some("22003") | Some("22P02") | Some("22007") => {
                    return Self::bad_request(
                        "CONSTRAINT_VIOLATION",
                        "The request violates a data rule: check amounts, dates and required fields",
                    );
                }
                _ => {}
            }
        }
        eprintln!("Database error: {error}");
        Self::internal()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Converts a bare status code (from middleware) into the standard JSON error body.
pub fn status_response(status: StatusCode) -> Response {
    let (code, message) = match status {
        StatusCode::UNAUTHORIZED => ("UNAUTHORIZED", "Authentication is required or the token is invalid or expired"),
        StatusCode::FORBIDDEN => ("PERMISSION_DENIED", "You do not have permission to perform this operation"),
        _ => ("INTERNAL_SERVER_ERROR", "An unexpected error occurred"),
    };
    ApiError::new(status, code, message).into_response()
}
