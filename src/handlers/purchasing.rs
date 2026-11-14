use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
        purchasing::*,
    },
    services::purchasing_service::{self, PurchasingError},
    state::SharedState,
};

fn map_error(error: PurchasingError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        PurchasingError::NotFound => (
            StatusCode::NOT_FOUND,
            "PURCHASE_ORDER_NOT_FOUND",
            "The purchase order was not found",
        ),
        PurchasingError::SupplierNotFound => (
            StatusCode::NOT_FOUND,
            "SUPPLIER_NOT_FOUND",
            "The supplier was not found or is inactive",
        ),
        PurchasingError::InvalidSupplier => (
            StatusCode::BAD_REQUEST,
            "INVALID_SUPPLIER",
            "The selected supplier is invalid",
        ),
        PurchasingError::EmptyOrder => (
            StatusCode::BAD_REQUEST,
            "EMPTY_PURCHASE_ORDER",
            "A purchase order must contain at least one item",
        ),
        PurchasingError::InvalidItems | PurchasingError::ItemNotFound => (
            StatusCode::BAD_REQUEST,
            "INVALID_PURCHASE_ORDER_ITEM",
            "One or more purchase order items are invalid",
        ),
        PurchasingError::InvalidQuantity => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUANTITY",
            "Quantities must be greater than zero",
        ),
        PurchasingError::InvalidPrice => (
            StatusCode::BAD_REQUEST,
            "INVALID_PRICE",
            "Prices and tax amounts cannot be negative",
        ),
        PurchasingError::InvalidDiscount => (
            StatusCode::BAD_REQUEST,
            "INVALID_DISCOUNT",
            "The discount cannot exceed the order subtotal and tax",
        ),
        PurchasingError::InvalidDate => (
            StatusCode::BAD_REQUEST,
            "INVALID_ORDER_DATE",
            "The expected delivery date cannot precede the order date",
        ),
        PurchasingError::InvalidStatus => (
            StatusCode::BAD_REQUEST,
            "INVALID_PURCHASE_STATUS",
            "The requested purchase order transition is invalid",
        ),
        PurchasingError::VersionConflict => (
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            "The purchase order has changed. Refresh and try again",
        ),
        PurchasingError::OrderNotEditable => (
            StatusCode::CONFLICT,
            "ORDER_NOT_EDITABLE",
            "This purchase order cannot be edited or cancelled in its current state",
        ),
        PurchasingError::OrderNotReceivable => (
            StatusCode::CONFLICT,
            "ORDER_NOT_RECEIVABLE",
            "Only submitted or partially received orders can receive goods",
        ),
        PurchasingError::LocationNotFound => (
            StatusCode::NOT_FOUND,
            "LOCATION_NOT_FOUND",
            "The inventory location was not found or is inactive",
        ),
        PurchasingError::ExcessiveReceipt => (
            StatusCode::CONFLICT,
            "EXCESSIVE_RECEIPT",
            "The received quantity exceeds the outstanding order quantity",
        ),
        PurchasingError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "IDEMPOTENCY_KEY_REUSED",
            "This idempotency key has already been used for different receipt details",
        ),
        PurchasingError::Database(error) => {
            eprintln!("Purchasing database error: {error}");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "PURCHASING_DATABASE_ERROR",
                "An internal purchasing error occurred",
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

pub async fn list_purchase_orders(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<PurchaseOrderQuery>,
) -> Result<Json<Vec<PurchaseOrder>>, (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::list_purchase_orders(&state.db, auth.company_id, query)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn get_purchase_order(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<PurchaseOrder>, (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::get_purchase_order(&state.db, auth.company_id, id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn create_purchase_order(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Json(request): Json<CreatePurchaseOrderRequest>,
) -> Result<(StatusCode, Json<PurchaseOrder>), (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::create_purchase_order(&state.db, auth.company_id, auth.user_id, request)
        .await
        .map(|order| (StatusCode::CREATED, Json(order)))
        .map_err(map_error)
}

pub async fn update_purchase_order(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(request): Json<UpdatePurchaseOrderRequest>,
) -> Result<Json<PurchaseOrder>, (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::update_purchase_order(&state.db, auth.company_id, id, request)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn submit_purchase_order(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::submit_purchase_order(&state.db, auth.company_id, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_error)
}

pub async fn cancel_purchase_order(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorResponse>)> {
    purchasing_service::cancel_purchase_order(&state.db, auth.company_id, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_error)
}

pub async fn receive_goods(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(order_id): Path<i64>,
    Json(request): Json<ReceiveGoodsRequest>,
) -> Result<(StatusCode, Json<GoodsReceipt>), (StatusCode, Json<ApiErrorResponse>)> {
    let outcome = purchasing_service::receive_goods(
        &state.db,
        auth.company_id,
        auth.user_id,
        order_id,
        request,
    )
    .await
    .map_err(map_error)?;

    let status = if outcome.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(outcome.receipt)))
}

pub async fn get_receipts(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<GoodsReceipt>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Verify that the order belongs to the authenticated company.
    purchasing_service::get_purchase_order(&state.db, auth.company_id, id)
        .await
        .map_err(map_error)?;

    purchasing_service::get_receipts(&state.db, auth.company_id, id)
        .await
        .map(Json)
        .map_err(map_error)
}
