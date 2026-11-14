// src/models/trading_parties.rs
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerCreate {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerUpdate {
    pub expected_version: i32,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
    pub is_active: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerCreditPolicyUpdate {
    pub expected_version: i32,
    pub credit_enabled: bool,
    pub credit_limit: Decimal,
    pub payment_terms_days: i32,
}
#[derive(Debug, Serialize)]
pub struct Customer {
    pub id: i64,
    pub customer_number: i64,
    pub currency_code: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
    pub credit_enabled: bool,
    pub credit_limit: Decimal,
    pub payment_terms_days: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}
#[derive(Debug, Deserialize)]
pub struct SupplierCreate {
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub payment_terms_days: i32,
}
#[derive(Debug, Deserialize)]
pub struct SupplierUpdate {
    pub expected_version: i32,
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
    pub payment_terms_days: i32,
    pub is_active: bool,
}
#[derive(Debug, Serialize)]
pub struct Supplier {
    pub id: i64,
    pub supplier_number: i64,
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_number: Option<String>,
    pub notes: Option<String>,
    pub payment_terms_days: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TradingPartyListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub search: Option<String>,
    pub is_active: Option<bool>,
}
