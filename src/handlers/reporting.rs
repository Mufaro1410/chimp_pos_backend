use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde_json::Value;

use crate::{
    models::{
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
        reporting::ReportQuery,
    },
    services::reporting_service::{self, ReportingError},
    state::SharedState,
};

fn error(err: ReportingError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match err {
        ReportingError::Invalid(message) => {
            (StatusCode::BAD_REQUEST, "INVALID_REPORT_FILTER", message)
        }
        ReportingError::NotFound => (
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The requested report resource was not found",
        ),
        ReportingError::Database(err) => {
            eprintln!("Reporting query failed: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "REPORT_QUERY_FAILED",
                "The report could not be generated",
            )
        }
    };
    (
        status,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }),
    )
}
macro_rules! report_handler {
    ($name:ident, $service:ident) => {
        pub async fn $name(
            State(state): State<SharedState>,
            Extension(auth): Extension<AuthUser>,
            Query(query): Query<ReportQuery>,
        ) -> Result<Json<Value>, (StatusCode, Json<ApiErrorResponse>)> {
            reporting_service::$service(&state.db, auth.company_id, &query)
                .await
                .map(Json)
                .map_err(error)
        }
    };
}
report_handler!(sales_summary, sales_summary);
report_handler!(sales_by_product, sales_by_product);
report_handler!(sales_by_category, sales_by_category);
report_handler!(sales_by_payment_method, sales_by_payment_method);
report_handler!(sales_transactions, sales_transactions);
report_handler!(sales_returns, sales_returns);
report_handler!(gross_profit, gross_profit);
report_handler!(inventory_stock, inventory_stock);
report_handler!(inventory_valuation, inventory_valuation);
report_handler!(inventory_movements, inventory_movements);
report_handler!(inventory_activity, inventory_activity);
report_handler!(purchasing_summary, purchasing_summary);
report_handler!(purchasing_by_supplier, purchasing_by_supplier);
report_handler!(goods_received, goods_received);
report_handler!(outstanding_purchase_orders, outstanding_purchase_orders);
report_handler!(cash_sessions, cash_sessions);
report_handler!(cash_movements, cash_movements);

pub async fn dashboard(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<ReportQuery>,
) -> Result<Json<Value>, (StatusCode, Json<ApiErrorResponse>)> {
    reporting_service::dashboard(&state.db, auth.company_id, &query)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn supplier_purchase_history(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
    Query(query): Query<ReportQuery>,
) -> Result<Json<Value>, (StatusCode, Json<ApiErrorResponse>)> {
    reporting_service::supplier_purchase_history(&state.db, auth.company_id, id, &query)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn cash_registers(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<ApiErrorResponse>)> {
    reporting_service::cash_registers(&state.db, auth.company_id)
        .await
        .map(Json)
        .map_err(error)
}
