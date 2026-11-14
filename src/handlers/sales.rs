use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
        sales::*,
    },
    services::sales_service::{self, SalesError},
    state::SharedState,
};

fn map_error(error: SalesError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        SalesError::NotFound => (
            StatusCode::NOT_FOUND,
            "SALE_NOT_FOUND",
            "The sale was not found",
        ),
        SalesError::EmptySale => (
            StatusCode::BAD_REQUEST,
            "EMPTY_SALE",
            "A sale must contain at least one item",
        ),
        SalesError::InvalidItems | SalesError::InvalidVariant => (
            StatusCode::BAD_REQUEST,
            "INVALID_SALE_ITEM",
            "One or more sale items are invalid",
        ),
        SalesError::InvalidQuantity => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUANTITY",
            "The quantity must be greater than zero",
        ),
        SalesError::DecimalQuantityNotAllowed => (
            StatusCode::BAD_REQUEST,
            "DECIMAL_QUANTITY_NOT_ALLOWED",
            "This unit does not allow fractional quantities",
        ),
        SalesError::InvalidPrice => (
            StatusCode::BAD_REQUEST,
            "INVALID_PRICE",
            "The submitted selling price is invalid",
        ),
        SalesError::InvalidDiscount => (
            StatusCode::BAD_REQUEST,
            "INVALID_DISCOUNT",
            "The discount is invalid",
        ),
        SalesError::CreditPermissionRequired => (
            StatusCode::FORBIDDEN,
            "CREDIT_PERMISSION_REQUIRED",
            "You do not have permission to complete a credit sale",
        ),

        SalesError::DiscountPermissionRequired => (
            StatusCode::FORBIDDEN,
            "DISCOUNT_PERMISSION_REQUIRED",
            "You do not have permission to apply discounts or override prices",
        ),
        SalesError::InvalidTax => (
            StatusCode::BAD_REQUEST,
            "INVALID_TAX",
            "The tax rate is invalid",
        ),
        SalesError::InvalidPayment | SalesError::InvalidPaymentMethod => (
            StatusCode::BAD_REQUEST,
            "INVALID_PAYMENT",
            "The payment details are invalid or do not balance with the sale total",
        ),
        SalesError::InvalidStatus => (
            StatusCode::BAD_REQUEST,
            "INVALID_SALE_STATUS",
            "The requested sale status is invalid",
        ),
        SalesError::InvalidLocation => (
            StatusCode::NOT_FOUND,
            "LOCATION_NOT_FOUND",
            "The inventory location was not found or is inactive",
        ),
        SalesError::VersionConflict => (
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            "The sale has changed. Refresh and try again",
        ),
        SalesError::SaleNotEditable => (
            StatusCode::CONFLICT,
            "SALE_NOT_EDITABLE",
            "This sale cannot be edited or cancelled in its current state",
        ),
        SalesError::SaleNotCheckoutable => (
            StatusCode::CONFLICT,
            "SALE_NOT_CHECKOUTABLE",
            "This sale cannot be checked out in its current state",
        ),
        SalesError::InsufficientStock => (
            StatusCode::CONFLICT,
            "INSUFFICIENT_STOCK",
            "There is not enough available stock to complete this sale",
        ),
        SalesError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "IDEMPOTENCY_KEY_REUSED",
            "This checkout key has already been used with different details",
        ),
        SalesError::CashRegisterRequired => (
            StatusCode::CONFLICT,
            "CASH_REGISTER_REQUIRED",
            "Select a register with an open cash session before accepting cash.",
        ),
        SalesError::CashSessionNotOpen => (
            StatusCode::CONFLICT,
            "CASH_SESSION_NOT_OPEN",
            "The selected register has no active cash session for this sale location.",
        ),
        SalesError::Database(error) => {
            eprintln!("Sales database error: {error}");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "SALES_DATABASE_ERROR",
                "An internal sales error occurred",
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

pub async fn list_sales(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<SaleQuery>,
) -> Result<Json<Vec<Sale>>, (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::list_sales(&state.db, auth.company_id, query)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn get_sale(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<Sale>, (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::get_sale(&state.db, auth.company_id, id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn create_sale(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Json(request): Json<CreateSaleRequest>,
) -> Result<(StatusCode, Json<Sale>), (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::create_sale(&state.db, auth.company_id, auth.user_id, request)
        .await
        .map(|sale| (StatusCode::CREATED, Json(sale)))
        .map_err(map_error)
}

pub async fn update_sale(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateSaleRequest>,
) -> Result<Json<Sale>, (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::update_sale(&state.db, auth.company_id, auth.user_id, id, request)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn checkout_sale(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(request): Json<CheckoutRequest>,
) -> Result<(StatusCode, Json<Sale>), (StatusCode, Json<ApiErrorResponse>)> {
    let outcome =
        sales_service::checkout_sale(&state.db, auth.company_id, auth.user_id, id, request)
            .await
            .map_err(map_error)?;

    let status = if outcome.replayed {
        StatusCode::OK
    } else {
        StatusCode::OK
    };

    Ok((status, Json(outcome.sale)))
}

pub async fn get_receipt(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<Sale>, (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::get_receipt(&state.db, auth.company_id, id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn cancel_sale(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorResponse>)> {
    sales_service::cancel_sale(&state.db, auth.company_id, auth.user_id, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_error)
}
