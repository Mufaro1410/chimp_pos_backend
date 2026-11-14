use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpenseCreate {
    pub idempotency_key: Uuid,
    pub category: String,
    pub description: String,
    pub amount: Decimal,
    pub expense_date: Option<NaiveDate>,
    /// cash | card | mobile_money | bank_transfer | other
    pub payment_method: String,
    /// Required when payment_method is `cash` (the till the money is taken from).
    pub cash_session_id: Option<i64>,
    pub supplier_id: Option<i64>,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpenseVoid {
    pub expected_version: i32,
    pub reason: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExpenseQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExpenseSummaryQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Expense {
    pub id: i64,
    pub expense_number: i64,
    pub category: String,
    pub description: String,
    pub amount: Decimal,
    pub currency_code: String,
    pub expense_date: NaiveDate,
    pub payment_method: String,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub supplier_id: Option<i64>,
    pub cash_session_id: Option<i64>,
    pub status: String,
    pub voided_at: Option<DateTime<Utc>>,
    pub void_reason: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct ExpenseOutcome {
    pub expense: Expense,
    pub replayed: bool,
}
