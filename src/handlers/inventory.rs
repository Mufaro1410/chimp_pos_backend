use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        inventory::{MovementQuery, StockAdjustmentRequest, StockQuery},
        product::{ApiErrorDetail, ApiErrorResponse},
    },
    services::inventory_service::{self, InventoryError},
    state::SharedState,
};

fn map_error(error: InventoryError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        InventoryError::InvalidAdjustment => (
            StatusCode::BAD_REQUEST,
            "INVALID_ADJUSTMENT",
            "The adjustment type and quantity direction are invalid",
        ),
        InventoryError::InvalidQuantity => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUANTITY",
            "The adjustment quantity cannot be zero",
        ),
        InventoryError::InsufficientStock => (
            StatusCode::CONFLICT,
            "INSUFFICIENT_STOCK",
            "The adjustment would reduce stock below the reserved quantity",
        ),
        InventoryError::LocationNotFound => (
            StatusCode::NOT_FOUND,
            "LOCATION_NOT_FOUND",
            "The inventory location was not found or is inactive",
        ),
        InventoryError::VariantNotFound => (
            StatusCode::NOT_FOUND,
            "VARIANT_NOT_FOUND",
            "The inventory-tracked product variant was not found",
        ),
        InventoryError::BalanceNotFound => (
            StatusCode::NOT_FOUND,
            "STOCK_BALANCE_NOT_FOUND",
            "The stock balance could not be found",
        ),
        InventoryError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "IDEMPOTENCY_KEY_REUSED",
            "This idempotency key has already been used for a different adjustment",
        ),
        InventoryError::Database(error) => {
            eprintln!("Inventory database error: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INVENTORY_DATABASE_ERROR",
                "An internal inventory error occurred",
            )
        }
    };

    (
        status,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: code.to_string(),
                message: message.to_string(),
            },
        }),
    )
}

pub async fn get_stock(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<StockQuery>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<ApiErrorResponse>)> {
    let stock = inventory_service::get_stock(&state.db, auth.company_id, query)
        .await
        .map_err(map_error)?;

    Ok(Json(stock))
}

pub async fn get_movements(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<MovementQuery>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<ApiErrorResponse>)> {
    let movements = inventory_service::get_movements(&state.db, auth.company_id, query)
        .await
        .map_err(map_error)?;

    Ok(Json(movements))
}

pub async fn adjust_stock(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Json(request): Json<StockAdjustmentRequest>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<ApiErrorResponse>)> {
    let response =
        inventory_service::adjust_stock(&state.db, auth.company_id, auth.user_id, request)
            .await
            .map_err(map_error)?;

    Ok((StatusCode::CREATED, Json(response)))
}
