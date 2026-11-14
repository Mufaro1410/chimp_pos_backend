// src/handlers/product.rs
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};

use crate::{
    models::{
        auth::AuthUser,
        product::{
            ApiErrorDetail, ApiErrorResponse, CreateProductRequest, Product, ProductListQuery,
            UpdateProductRequest,
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
            "The requested product was not found".to_string(),
        ),

        ProductError::AlreadyExists => (
            StatusCode::CONFLICT,
            "PRODUCT_ALREADY_EXISTS",
            "A product with these details already exists".to_string(),
        ),

        ProductError::InvalidPrice => (
            StatusCode::BAD_REQUEST,
            "INVALID_PRICE",
            "Selling price and cost price must be zero or greater".to_string(),
        ),

        ProductError::InvalidName => (
            StatusCode::BAD_REQUEST,
            "INVALID_NAME",
            "Product and variant names cannot be empty".to_string(),
        ),

        ProductError::InvalidUnit => (
            StatusCode::BAD_REQUEST,
            "INVALID_UNIT",
            "The selected unit of measurement is invalid".to_string(),
        ),

        ProductError::InvalidCategory => (
            StatusCode::BAD_REQUEST,
            "INVALID_CATEGORY",
            "The selected product category is invalid".to_string(),
        ),

        ProductError::VariantNotFound => (
            StatusCode::NOT_FOUND,
            "VARIANT_NOT_FOUND",
            "The requested product variant was not found".to_string(),
        ),

        ProductError::CannotDeactivateLastVariant => (
            StatusCode::CONFLICT,
            "LAST_ACTIVE_VARIANT",
            "A product must have at least one active variant".to_string(),
        ),

        ProductError::VersionConflict => (
            StatusCode::CONFLICT,
            "VERSION_CONFLICT",
            "The record has been modified. Please refresh and try again".to_string(),
        ),

        ProductError::InvalidQuery => (
            StatusCode::BAD_REQUEST,
            "INVALID_QUERY",
            "Invalid pagination or filter parameters (limit must be 1-100, offset >= 0)".to_string(),
        ),

        ProductError::NoInventoryLocations => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVENTORY_LOCATION_MISSING",
            "No inventory location is configured for this company".to_string(),
        ),

        ProductError::Database(err) => {
            eprintln!("Database error: {err}");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                "An unexpected database error occurred".to_string(),
            )
        }
    };

    (
        status,
        Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: code.to_string(),
                message,
            },
        }),
    )
}

pub async fn get_products(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<ProductListQuery>,
) -> Result<Json<Vec<Product>>, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::get_products(&state, auth.company_id, query)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn get_product(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<Json<Product>, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::get_product(&state, auth.company_id, id)
        .await
        .map(Json)
        .map_err(map_error)
}

pub async fn create_product(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Json(request): Json<CreateProductRequest>,
) -> Result<(StatusCode, Json<Product>), (StatusCode, Json<ApiErrorResponse>)> {
    product_service::create_product(&state, auth.company_id, request)
        .await
        .map(|product| (StatusCode::CREATED, Json(product)))
        .map_err(map_error)
}

pub async fn update_product(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateProductRequest>,
) -> Result<(StatusCode, Json<Product>), (StatusCode, Json<ApiErrorResponse>)> {
    product_service::update_product(&state, auth.company_id, id, request)
        .await
        .map(|product| (StatusCode::OK, Json(product)))
        .map_err(map_error)
}

pub async fn delete_product(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorResponse>)> {
    product_service::delete_product(&state, auth.company_id, id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(map_error)
}
