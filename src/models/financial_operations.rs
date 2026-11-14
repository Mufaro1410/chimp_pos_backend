use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationInput {
    pub document_id: i64,
    pub amount: Decimal,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerPaymentCreate {
    pub idempotency_key: Uuid,
    pub amount: Decimal,
    pub payment_method: String,
    pub cash_session_id: Option<i64>,
    pub reference: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub allocations: Vec<AllocationInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplierPaymentCreate {
    pub idempotency_key: Uuid,
    pub amount: Decimal,
    pub payment_method: String,
    pub cash_session_id: Option<i64>,
    pub reference: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub allocations: Vec<AllocationInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditNoteCreate {
    pub idempotency_key: Uuid,
    pub document_id: Option<i64>,
    pub amount: Decimal,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplierInvoiceCreate {
    pub idempotency_key: Uuid,
    pub invoice_number: String,
    pub supplier_reference: Option<String>,
    pub purchase_order_id: Option<i64>,
    pub invoice_date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub currency_code: String,
    pub additional_charges: Decimal,
    pub notes: Option<String>,
    pub lines: Vec<SupplierInvoiceLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplierInvoiceLineInput {
    pub description: String,
    pub quantity: Decimal,
    pub unit_cost: Decimal,
    pub tax_amount: Decimal,
    pub discount_amount: Decimal,
    pub purchase_order_item_id: Option<i64>,
    pub goods_receipt_item_id: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StatementQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PostingResult {
    pub id: i64,
    pub document_number: Option<i64>,
    pub replayed: bool,
}

#[derive(Debug, Serialize)]
pub struct AccountBalance {
    pub party_id: i64,
    pub currency_code: String,
    pub balance: Decimal,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StatementLine {
    pub event_type: String,
    pub document_id: i64,
    pub event_at: DateTime<Utc>,
    pub reference: Option<String>,
    pub balance_delta: Decimal,
    pub balance_after: Decimal,
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OutstandingDocument {
    pub id: i64,
    pub document_number: String,
    pub document_date: NaiveDate,
    pub total_amount: Decimal,
    pub allocated_amount: Decimal,
    pub credited_amount: Decimal,
    pub outstanding_amount: Decimal,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SupplierInvoiceLine {
    pub id: i64,
    pub description: String,
    pub quantity: Decimal,
    pub unit_cost: Decimal,
    pub tax_amount: Decimal,
    pub discount_amount: Decimal,
    pub line_total: Decimal,
    pub purchase_order_item_id: Option<i64>,
    pub goods_receipt_item_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SupplierInvoice {
    pub id: i64,
    pub supplier_id: i64,
    pub invoice_number: String,
    pub supplier_reference: Option<String>,
    pub purchase_order_id: Option<i64>,
    pub invoice_date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub currency_code: String,
    pub subtotal: Decimal,
    pub tax_total: Decimal,
    pub discount_total: Decimal,
    pub additional_charges: Decimal,
    pub total_amount: Decimal,
    pub status: String,
    pub posted_at: Option<DateTime<Utc>>,
    pub lines: Vec<SupplierInvoiceLine>,
}
