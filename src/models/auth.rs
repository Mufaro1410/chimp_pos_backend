use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub company: RegisterCompanyRequest,
    pub owner: RegisterOwnerRequest,
}

#[derive(Debug, Deserialize)]
pub struct RegisterCompanyRequest {
    pub name: String,
    pub business_type: Option<String>,
    pub country_code: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub default_currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterOwnerRequest {
    pub name: String,
    pub username: String,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub company_id: uuid::Uuid,
    pub company_name: String,
    pub user_id: uuid::Uuid,
    pub owner_name: String,
    pub username: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub company_id: Uuid,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub user: LoginUserResponse,
}

#[derive(Debug, Serialize)]
pub struct LoginUserResponse {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub company_id: Uuid,
    pub token_id: Uuid,
}

#[derive(Debug, serde::Serialize)]
pub struct AuthMeResponse {
    pub user_id: Uuid,
    pub company_id: Uuid,
}
