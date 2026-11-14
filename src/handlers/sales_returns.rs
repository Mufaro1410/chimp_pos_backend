use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        product::{ApiErrorDetail, ApiErrorResponse},
        sales_returns::*,
    },
    services::sales_returns_service::{self, SalesReturnsError},
    state::SharedState,
};

fn map_error(error: SalesReturnsError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        SalesReturnsError::NotFound => (
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The requested record was not found.",
        ),
        SalesReturnsError::SaleNotCompleted => (
            StatusCode::CONFLICT,
            "SALE_NOT_COMPLETED",
            "Only completed sales can be returned or voided.",
        ),
        SalesReturnsError::SaleAlreadyVoided => (
            StatusCode::CONFLICT,
            "SALE_ALREADY_VOIDED",
            "This sale has already been voided.",
        ),
        SalesReturnsError::SaleHasReturns => (
            StatusCode::CONFLICT,
            "SALE_HAS_RETURNS",
            "A sale with existing returns or refunds cannot be voided.",
        ),
        SalesReturnsError::ReturnQuantityExceeded => (
            StatusCode::CONFLICT,
            "RETURN_QUANTITY_EXCEEDED",
            "The requested return exceeds the quantity originally sold.",
        ),
        SalesReturnsError::RefundExceedsEligibleAmount => (
            StatusCode::CONFLICT,
            "REFUND_EXCEEDS_ELIGIBLE_AMOUNT",
            "The refund exceeds the remaining refundable amount.",
        ),
        SalesReturnsError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "IDEMPOTENCY_CONFLICT",
            "This idempotency key was already used with a different request.",
        ),
        SalesReturnsError::PermissionDenied => (
            StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            "You do not have permission to perform this operation.",
        ),
        SalesReturnsError::CashRegisterRequired => (
            StatusCode::CONFLICT,
            "CASH_REGISTER_REQUIRED",
            "Select a register with an open cash session before issuing a cash refund.",
        ),
        SalesReturnsError::CashSessionNotOpen => (
            StatusCode::CONFLICT,
            "CASH_SESSION_NOT_OPEN",
            "The selected register has no active cash session for this location.",
        ),
        SalesReturnsError::InsufficientCash => (
            StatusCode::CONFLICT,
            "INSUFFICIENT_REGISTER_CASH",
            "The register does not have enough available cash to issue this refund.",
        ),
        SalesReturnsError::InvalidReason => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_REASON",
            "A valid reason is required.",
        ),
        SalesReturnsError::InvalidQuantity => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_QUANTITY",
            "The quantity must be positive and valid.",
        ),
        SalesReturnsError::InvalidCondition => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_CONDITION",
            "Condition must be resellable, damaged or unsellable.",
        ),
        SalesReturnsError::InvalidPayment => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_PAYMENT",
            "The payment amount is invalid.",
        ),
        SalesReturnsError::InvalidPaymentMethod => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_PAYMENT_METHOD",
            "The payment method is invalid.",
        ),
        SalesReturnsError::DecimalQuantityNotAllowed => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "DECIMAL_QUANTITY_NOT_ALLOWED",
            "This unit does not allow fractional quantities.",
        ),
        SalesReturnsError::InvalidRequest => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_REQUEST",
            "The request is invalid.",
        ),
        SalesReturnsError::Database(error) => {
            tracing::error!("Sales returns database error: {:?}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "An unexpected error occurred.",
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

pub async fn create_return(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(sale_id): Path<i64>,
    Json(request): Json<CreateSaleReturnRequest>,
) -> Result<(StatusCode, Json<SaleReturn>), (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::create_return(
        &state.db,
        user.company_id,
        user.user_id,
        sale_id,
        request,
    )
    .await
    .map_err(map_error)?;

    let status = if result.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(result.value)))
}

pub async fn list_returns(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(sale_id): Path<i64>,
) -> Result<Json<Vec<SaleReturn>>, (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::list_returns(&state.db, user.company_id, sale_id)
        .await
        .map_err(map_error)?;

    Ok(Json(result))
}

pub async fn refund_return(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(return_id): Path<i64>,
    Json(request): Json<CreateRefundRequest>,
) -> Result<(StatusCode, Json<SaleRefund>), (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::refund_return(
        &state.db,
        user.company_id,
        user.user_id,
        return_id,
        request,
    )
    .await
    .map_err(map_error)?;

    let status = if result.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(result.value)))
}

pub async fn list_refunds(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(sale_id): Path<i64>,
) -> Result<Json<Vec<SaleRefund>>, (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::list_refunds(&state.db, user.company_id, sale_id)
        .await
        .map_err(map_error)?;

    Ok(Json(result))
}

pub async fn void_sale(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(sale_id): Path<i64>,
    Json(request): Json<VoidSaleRequest>,
) -> Result<(StatusCode, Json<SaleVoid>), (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::void_sale(
        &state.db,
        user.company_id,
        user.user_id,
        sale_id,
        request,
    )
    .await
    .map_err(map_error)?;

    let status = if result.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(result.value)))
}

pub async fn get_void(
    State(state): State<SharedState>,
    Extension(user): Extension<AuthUser>,
    Path(sale_id): Path<i64>,
) -> Result<Json<Option<SaleVoid>>, (StatusCode, Json<ApiErrorResponse>)> {
    let result = sales_returns_service::get_void(&state.db, user.company_id, sale_id)
        .await
        .map_err(map_error)?;

    Ok(Json(result))
}
