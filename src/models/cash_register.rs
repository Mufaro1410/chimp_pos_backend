use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateCashRegisterRequest {
    pub location_id: i64,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenCashSessionRequest {
    pub opening_float: Decimal,
    pub opening_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CashMovementRequest {
    pub movement_type: String,
    pub amount: Decimal,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub idempotency_key: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct CloseCashSessionRequest {
    pub closing_cash_count: Decimal,
    pub closing_notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CashRegister {
    pub id: i64,
    pub location_id: i64,
    pub code: String,
    pub name: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CashRegisterSession {
    pub id: i64,
    pub register_id: i64,
    pub location_id: i64,
    pub status: String,

    pub opening_float: Decimal,
    pub opening_notes: Option<String>,

    pub opened_by: Uuid,
    pub opened_at: DateTime<Utc>,

    pub closed_by: Option<Uuid>,
    pub closed_at: Option<DateTime<Utc>>,

    pub expected_cash: Option<Decimal>,
    pub closing_cash_count: Option<Decimal>,
    pub cash_variance: Option<Decimal>,

    pub closing_notes: Option<String>,
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct CashMovement {
    pub id: i64,
    pub register_session_id: i64,
    pub movement_type: String,
    pub amount: Decimal,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CashSessionSummary {
    pub session: CashRegisterSession,

    pub total_cash_sales: Decimal,
    pub total_cash_refunds: Decimal,
    pub total_customer_receipts: Decimal,
    pub total_supplier_payments: Decimal,
    pub total_customer_payment_reversals: Decimal,
    pub total_supplier_payment_reversals: Decimal,

    pub total_cash_in: Decimal,
    pub total_cash_out: Decimal,
    pub total_paid_out: Decimal,
    pub total_cash_drops: Decimal,

    pub calculated_expected_cash: Decimal,
}
