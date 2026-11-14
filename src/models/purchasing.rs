use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct PurchaseOrderItemRequest {
    pub variant_id: i64,
    pub ordered_quantity: Decimal,
    pub unit_cost: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct CreatePurchaseOrderRequest {
    pub supplier_id: i64,
    pub order_date: Option<NaiveDate>,
    pub expected_delivery_date: Option<NaiveDate>,
    pub tax_total: Option<Decimal>,
    pub discount_total: Option<Decimal>,
    pub notes: Option<String>,
    pub items: Vec<PurchaseOrderItemRequest>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePurchaseOrderRequest {
    pub expected_version: i32,
    pub supplier_id: i64,
    pub order_date: Option<NaiveDate>,
    pub expected_delivery_date: Option<NaiveDate>,
    pub tax_total: Option<Decimal>,
    pub discount_total: Option<Decimal>,
    pub notes: Option<String>,
    pub items: Vec<PurchaseOrderItemRequest>,
}

#[derive(Debug, Deserialize)]
pub struct PurchaseOrderQuery {
    pub supplier_id: Option<i64>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PurchaseOrder {
    pub id: i64,
    pub order_number: i64,
    pub supplier_id: i64,
    pub supplier_name: String,
    pub status: String,
    pub order_date: NaiveDate,
    pub expected_delivery_date: Option<NaiveDate>,
    pub subtotal: Decimal,
    pub tax_total: Decimal,
    pub discount_total: Decimal,
    pub total_amount: Decimal,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub version: i32,
    pub items: Vec<PurchaseOrderItem>,
}

#[derive(Debug, Serialize)]
pub struct PurchaseOrderItem {
    pub id: i64,
    pub variant_id: i64,
    pub product_name: String,
    pub variant_name: String,
    pub sku: Option<String>,
    pub unit_code: String,
    pub unit_name: String,
    pub ordered_quantity: Decimal,
    pub received_quantity: Decimal,
    pub outstanding_quantity: Decimal,
    pub unit_cost: Decimal,
    pub line_total: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct ReceiveGoodsRequest {
    pub location_id: i64,
    pub idempotency_key: Uuid,
    pub notes: Option<String>,
    pub items: Vec<ReceiveGoodsItemRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ReceiveGoodsItemRequest {
    pub purchase_order_item_id: i64,
    pub quantity_received: Decimal,
    pub unit_cost: Option<Decimal>,
}

#[derive(Debug, Serialize)]
pub struct GoodsReceipt {
    pub id: i64,
    pub receipt_number: i64,
    pub purchase_order_id: i64,
    pub location_id: i64,
    pub received_by: Uuid,
    pub notes: Option<String>,
    pub received_at: DateTime<Utc>,
    pub items: Vec<GoodsReceiptItem>,
}

#[derive(Debug, Serialize)]
pub struct GoodsReceiptItem {
    pub id: i64,
    pub purchase_order_item_id: i64,
    pub quantity_received: Decimal,
    pub unit_cost: Decimal,
}
