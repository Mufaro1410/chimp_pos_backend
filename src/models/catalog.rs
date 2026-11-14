use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, FromRow)]
pub struct Unit {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub allows_decimal: bool,
    pub is_active: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Category {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub version: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryCreate {
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryUpdate {
    pub expected_version: i32,
    pub name: String,
    pub description: Option<String>,
    pub parent_id: Option<i64>,
    pub is_active: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Location {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub location_type: String,
    pub address: Option<String>,
    pub is_default: bool,
    pub is_active: bool,
    pub version: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationCreate {
    pub name: String,
    pub code: String,
    pub location_type: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationUpdate {
    pub expected_version: i32,
    pub name: String,
    pub location_type: Option<String>,
    pub address: Option<String>,
    pub is_active: bool,
}
