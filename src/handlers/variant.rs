use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        product::{
            ApiErrorDetail, ApiErrorResponse, ProductVariant, ProductVariantRequest,
            UpdateProductVariantRequest,
        },
    },
    services::product_service::{self, ProductError},
    state::SharedState,
};

fn map_error(error: ProductError) -> (StatusCode, Json<ApiErrorResponse>) {
    let (status, code, message) = match error {
        ProductError::NotFound => (
            StatusCode::NOT_FOUND,
            "PRODUCT_NOT_FOUND",
            "The requested product was not found",
        ),

        ProductError::VariantNotFound => (
            StatusCode::NOT_FOUND,
            "VARIANT_NOT_FOUND",
            "The requested product variant was not found",
        ),

        ProductError::AlreadyExists => (
            StatusCode::CONFLICT,
            "VARIANT_ALREADY_EXISTS",
            "A variant with this SKU or barcode already exists",
        ),

        ProductError::InvalidName => (
            StatusCode::BAD_REQUEST,
            "INVALID_NAME",
            "Variant name cannot be empty",
        ),

        ProductError::InvalidPrice => (
            StatusCode::BAD_REQUEST,
            "INVALID_PRICE",
            "Selling price and cost price must be zero or greater",
        ),

        ProductError::InvalidUnit => (
            StatusCode::BAD_REQUEST,
            "INVALID_UNIT",
            "The selected unit of measurement is invalid",
        ),

        ProductError::InvalidCategory => (
            StatusCode::BAD_REQUEST,
            "INVALID_CATEGORY",
            "The selected product category is invalid",
        ),

        ProductError::CannotDeactivateLastVariant => (
            StatusCode::CONFLICT,
            "LAST_ACTIVE_VARIANT",
            "A product must have at least one active variant",
        ),

        ProductError::VersionConflict => (
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            "This variant has changed. Retrieve the latest version and try again",
        ),

        ProductError::InvalidQuery => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUERY",
            "Invalid query parameters",
        ),

        ProductError::NoInventoryLocations => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVENTORY_LOCATION_MISSING",
            "No inventory locations are configured for this company",
        ),

        ProductError::Database(err) => {
            eprintln!("Variant database error: {err}");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                "An unexpected database error occurred",
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

pub async fn get_product_variants(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(product_id): Path<i64>,
) -> Result<Json<Vec<ProductVariant>>, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::get_product_variants(&state, auth.company_id, product_id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn create_product_variant(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(product_id): Path<i64>,
    Json(request): Json<ProductVariantRequest>,
) -> Result<(StatusCode, Json<ProductVariant>), (StatusCode, Json<ApiErrorResponse>)> {
    product_service::create_product_variant(&state, auth.company_id, product_id, request)
        .await
        .map(|variant| (StatusCode::CREATED, Json(variant)))
        .map_err(map_error)
}

pub async fn update_product_variant(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path((product_id, variant_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateProductVariantRequest>,
) -> Result<Json<ProductVariant>, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::update_product_variant(
        &state,
        auth.company_id,
        product_id,
        variant_id,
        request,
    )
    .await
    .map(Json)
    .map_err(map_error)
}

pub async fn deactivate_product_variant(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path((product_id, variant_id)): Path<(i64, i64)>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::deactivate_product_variant(&state, auth.company_id, product_id, variant_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(map_error)
}
