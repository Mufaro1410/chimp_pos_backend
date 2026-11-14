// src/models/product.rs
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<i64>,
    pub track_inventory: bool,
    pub is_active: bool,
    pub version: i32,
    pub variants: Vec<ProductVariant>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProductListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub search: Option<String>,
    pub category_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ProductVariant {
    pub id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub unit_id: i64,
    pub unit_code: String,
    pub unit_name: String,
    pub unit_symbol: String,
    pub selling_price: Decimal,
    pub cost_price: Option<Decimal>,
    pub is_default: bool,
    pub is_active: bool,
    pub version: i32,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductRequest {
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<i64>,

    #[serde(default = "default_true")]
    pub track_inventory: bool,

    pub variant: ProductVariantRequest,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductRequest {
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<i64>,

    #[serde(default = "default_true")]
    pub track_inventory: bool,

    pub variant: ProductVariantRequest,
}

#[derive(Debug, Deserialize)]
pub struct ProductVariantRequest {
    pub name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub unit_id: i64,
    pub selling_price: Decimal,
    pub cost_price: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductVariantRequest {
    pub name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub unit_id: i64,
    pub selling_price: Decimal,
    pub cost_price: Option<Decimal>,

    // The version the client last retrieved.
    pub expected_version: i32,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

// Internal SQL query structure.
// Used to flatten product and variant data returned by JOINs.

#[derive(Debug, FromRow)]
pub(crate) struct ProductQueryRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<i64>,
    pub track_inventory: bool,
    pub is_active: bool,
    pub version: i32,

    pub variant_id: i64,
    pub variant_name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub unit_id: i64,
    pub unit_code: String,
    pub unit_name: String,
    pub unit_symbol: String,
    pub selling_price: Decimal,
    pub cost_price: Option<Decimal>,
    pub is_default: bool,
    pub variant_is_active: bool,
    pub variant_version: i32,
}
