use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PullQuery {
    /// Opaque cursor returned by the previous pull. Omit for a full snapshot.
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PullResponse {
    pub entity: String,
    pub items: Vec<Value>,
    /// Store this and send it back on the next pull.
    pub next_cursor: String,
    /// true: call again immediately with `next_cursor` to get the next page.
    pub has_more: bool,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PushRequest {
    pub device_id: Option<String>,
    pub operations: Vec<PushOperation>,
}

#[derive(Debug, Deserialize)]
pub struct PushOperation {
    /// Client-generated UUID. Retrying with the same id never repeats the operation.
    pub client_op_id: Uuid,
    /// sale.checkout | expense.create | stock.adjust | customer.payment | cash.movement
    pub op_type: String,
    pub client_created_at: Option<DateTime<Utc>>,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
pub struct OpError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PushResult {
    pub client_op_id: Uuid,
    pub op_type: String,
    /// applied | duplicate | rejected | error | in_progress
    pub status: &'static str,
    pub result: Option<Value>,
    pub error: Option<OpError>,
    /// true: the failure was transient; resend the same operation unchanged.
    pub retryable: bool,
}

#[derive(Debug, Serialize)]
pub struct PushResponse {
    pub results: Vec<PushResult>,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OperationsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DismissRequest {
    pub note: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct SyncOperation {
    pub id: i64,
    pub client_op_id: Uuid,
    pub device_id: Option<String>,
    pub user_id: Uuid,
    pub op_type: String,
    pub status: String,
    pub payload: Value,
    pub result: Option<Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempts: i32,
    pub client_created_at: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RetryRequest {
    /// Execute under the reviewing manager's own authority instead of the original user's.
    /// Needed when the original user lacked a permission the operation requires, e.g. a cashier
    /// who sold at an out-of-date cached price (below list price needs `sales.discount`).
    /// The resulting record is attributed to the reviewer and the journal notes who approved it.
    #[serde(default)]
    pub run_as_reviewer: bool,
}
