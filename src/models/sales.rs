use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ------------------------------------------------------------
// REQUESTS
// ------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SaleItemRequest {
    pub variant_id: i64,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub discount_amount: Option<Decimal>,
    pub tax_rate: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSaleRequest {
    pub location_id: i64,
    #[serde(default)]
    pub customer_id: Option<i64>,
    pub notes: Option<String>,
    pub items: Vec<SaleItemRequest>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSaleRequest {
    pub expected_version: i32,
    pub location_id: i64,
    #[serde(default)]
    pub customer_id: Option<i64>,
    pub notes: Option<String>,
    pub items: Vec<SaleItemRequest>,
}

#[derive(Debug, Deserialize)]
pub struct SaleQuery {
    pub status: Option<String>,
    pub location_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub expected_version: i32,
    pub idempotency_key: Uuid,
    pub register_id: Option<i64>,
    pub payments: Vec<SalePaymentRequest>,
}

#[derive(Debug, Deserialize)]
pub struct SalePaymentRequest {
    pub payment_method: String,
    pub amount: Decimal,
    pub tendered_amount: Decimal,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

// ------------------------------------------------------------
// RESPONSES
// ------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Sale {
    pub id: i64,
    pub sale_number: i64,
    pub location_id: i64,
    pub customer_id: Option<i64>,
    pub status: String,
    pub currency_code: String,

    pub subtotal: Decimal,
    pub discount_total: Decimal,
    pub tax_total: Decimal,
    pub total_amount: Decimal,

    pub notes: Option<String>,

    pub created_by: Uuid,
    pub completed_by: Option<Uuid>,
    pub completed_at: Option<DateTime<Utc>>,

    pub created_at: DateTime<Utc>,
    pub version: i32,

    pub items: Vec<SaleItem>,
    pub payments: Vec<SalePayment>,
}

#[derive(Debug, Serialize)]
pub struct SaleItem {
    pub id: i64,
    pub variant_id: i64,

    pub product_name: String,
    pub variant_name: String,

    pub sku: Option<String>,
    pub barcode: Option<String>,

    pub unit_code: String,
    pub unit_name: String,

    pub quantity: Decimal,
    pub unit_price: Decimal,

    pub discount_amount: Decimal,
    pub tax_rate: Decimal,
    pub tax_amount: Decimal,

    pub line_subtotal: Decimal,
    pub line_total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct SalePayment {
    pub id: i64,
    pub payment_method: String,
    pub currency_code: String,

    pub amount: Decimal,
    pub tendered_amount: Decimal,
    pub change_given: Decimal,

    pub reference: Option<String>,
    pub notes: Option<String>,

    pub received_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CheckoutOutcome {
    pub sale: Sale,
    pub replayed: bool,
}
