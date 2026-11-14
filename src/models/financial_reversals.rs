use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct PaymentReversalRequest {
    pub idempotency_key: Uuid,
    pub amount: Decimal,
    /// The channel used to return/receive the reversing funds.
    pub payment_method: String,
    /// Required only when `payment_method` is cash; identifies the actual session.
    pub cash_session_id: Option<i64>,
    pub reason: String,
    pub allocations: Vec<AllocationReversalInput>,
}

#[derive(Debug, Deserialize)]
pub struct AllocationReversalInput {
    pub allocation_id: i64,
    pub amount: Decimal,
    pub idempotency_key: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct CreditNoteReversalRequest {
    pub idempotency_key: Uuid,
    pub amount: Decimal,
    pub reason: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ReversalOutcome {
    pub reversal_id: i64,
    pub replayed: bool,
}
