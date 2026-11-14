use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Shop {
    pub id: Uuid,
    pub name: String,
    pub business_type: Option<String>,
    pub description: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub timezone: String,
    pub locale: String,
    pub default_currency: String,
    pub status: String,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopUpdate {
    pub expected_version: i32,
    pub name: String,
    pub business_type: Option<String>,
    pub description: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub timezone: String,
    pub locale: String,
    pub default_currency: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ManagedUser {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub username: String,
    pub status: String,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    #[sqlx(skip)]
    pub roles: Vec<RoleSummary>,
}

#[derive(Debug, Serialize, Clone, sqlx::FromRow)]
pub struct RoleSummary {
    pub id: Uuid,
    pub name: String,
    pub role_key: Option<String>,
    pub is_system_role: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserCreate {
    pub name: String,
    pub username: String,
    pub email: Option<String>,
    /// Temporary credential supplied by the administrator. A delivery/reset flow is not yet available.
    pub initial_password: String,
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserUpdate {
    pub expected_version: i32,
    pub name: String,
    pub email: Option<String>,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserRoleUpdate {
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub role_key: Option<String>,
    pub is_system_role: bool,
    pub is_active: bool,
    pub version: i32,
    #[sqlx(skip)]
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Serialize, Clone, sqlx::FromRow)]
pub struct Permission {
    pub id: Uuid,
    pub permission_key: String,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolePermissionUpdate {
    pub expected_version: i32,
    pub permission_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: i64,
    pub offset: i64,
}
