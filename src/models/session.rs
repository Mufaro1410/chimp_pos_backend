use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub refresh_token: String,
    pub device_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminResetPasswordRequest {
    pub new_password: String,
    /// The user must choose their own password at next login (default true).
    #[serde(default = "default_true")]
    pub require_change: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgotPasswordRequest {
    pub company_id: Uuid,
    /// Username or email address.
    pub identifier: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: String,
    pub refresh_expires_in: u64,
    pub must_change_password: bool,
}
