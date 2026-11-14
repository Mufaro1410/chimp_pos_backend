use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ReturnItemRequest {
    pub sale_item_id: i64,
    pub quantity: Decimal,
    pub item_condition: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSaleReturnRequest {
    pub reason: String,
    pub notes: Option<String>,
    pub idempotency_key: Uuid,
    pub items: Vec<ReturnItemRequest>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateRefundRequest {
    pub idempotency_key: Uuid,
    pub payment_method: String,
    pub amount: Decimal,
    pub register_id: Option<i64>,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VoidRefundRequest {
    pub payment_method: String,
    pub amount: Decimal,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VoidSaleRequest {
    pub idempotency_key: Uuid,
    pub reason: String,
    pub notes: Option<String>,
    pub register_id: Option<i64>,
    pub refunds: Vec<VoidRefundRequest>,
}

#[derive(Debug, Serialize)]
pub struct SaleReturn {
    pub id: i64,
    pub sale_id: i64,
    pub location_id: i64,
    pub reason: String,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub items: Vec<SaleReturnItem>,
}

#[derive(Debug, Serialize)]
pub struct SaleReturnItem {
    pub id: i64,
    pub sale_item_id: i64,
    pub variant_id: i64,
    pub product_name: String,
    pub variant_name: String,
    pub quantity: Decimal,
    pub item_condition: String,
    pub refundable_amount: Decimal,
}

#[derive(Debug, Serialize)]
pub struct SaleRefund {
    pub id: i64,
    pub sale_id: i64,
    pub return_id: Option<i64>,
    pub void_id: Option<i64>,
    pub cash_session_id: Option<i64>,
    pub payment_method: String,
    pub amount: Decimal,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub refunded_by: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct SaleVoid {
    pub id: i64,
    pub sale_id: i64,
    pub reason: String,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
