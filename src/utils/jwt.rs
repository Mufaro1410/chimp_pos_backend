use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::auth_error::AuthError;

/// Access-token lifetime. Override with JWT_TTL_SECONDS (60..=86400); default 1 hour.
pub fn token_ttl_seconds() -> u64 {
    std::env::var("JWT_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| (60..=86_400).contains(v))
        .unwrap_or(3600)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub company_id: String,
    pub iat: usize,
    pub exp: usize,
    pub jti: String,
    pub iss: String,
}

pub fn generate_access_token(
    user_id: Uuid,
    company_id: Uuid,
    secret: &str,
) -> Result<String, AuthError> {
    if secret.len() < 32 {
        return Err(AuthError::Internal);
    }

    let now = chrono::Utc::now().timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        company_id: company_id.to_string(),
        iat: now,
        exp: now + token_ttl_seconds() as usize,
        jti: Uuid::new_v4().to_string(),
        iss: "chimp-pos".to_string(),
    };

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".to_string());

    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AuthError::Internal)
}
