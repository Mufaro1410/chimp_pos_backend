use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow)]
pub struct StockItem {
    pub location_id: i64,
    pub location_name: String,
    pub variant_id: i64,
    pub product_id: i64,
    pub product_name: String,
    pub variant_name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub unit_code: String,
    pub unit_name: String,
    pub quantity_on_hand: Decimal,
    pub reserved_quantity: Decimal,
    pub available_quantity: Decimal,
    pub version: i32,
}

#[derive(Debug, Serialize, FromRow)]
pub struct StockMovement {
    pub id: i64,
    pub location_id: i64,
    pub variant_id: i64,
    pub movement_type: String,
    pub quantity_delta: Decimal,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub notes: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct StockQuery {
    pub location_id: Option<i64>,
    pub variant_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct MovementQuery {
    pub location_id: Option<i64>,
    pub variant_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct StockAdjustmentRequest {
    pub location_id: i64,
    pub variant_id: i64,
    pub movement_type: String,
    pub quantity_delta: Decimal,
    pub notes: Option<String>,
    pub idempotency_key: Uuid,
}

#[derive(Debug, Serialize)]
pub struct StockAdjustmentResponse {
    pub movement_id: i64,
    pub variant_id: i64,
    pub location_id: i64,
    pub movement_type: String,
    pub quantity_delta: Decimal,
    pub quantity_on_hand: Decimal,
    pub reserved_quantity: Decimal,
    pub available_quantity: Decimal,
    pub version: i32,
}
