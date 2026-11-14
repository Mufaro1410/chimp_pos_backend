// src/services/product_service.rs
use rust_decimal::Decimal;
use sqlx::{Error, PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    models::product::{
        CreateProductRequest, Product, ProductListQuery, ProductQueryRow, ProductVariant,
        ProductVariantRequest, UpdateProductRequest, UpdateProductVariantRequest,
    },
    state::AppState,
};

#[derive(Debug)]
pub enum ProductError {
    NotFound,
    VariantNotFound,
    AlreadyExists,
    Database(Error),
    InvalidPrice,
    InvalidName,
    InvalidUnit,
    InvalidCategory,
    InvalidQuery,
    CannotDeactivateLastVariant,
    VersionConflict,
    NoInventoryLocations,
}

fn map_database_error(error: Error) -> ProductError {
    if let Error::Database(ref db_error) = error {
        if db_error.is_unique_violation() {
            return ProductError::AlreadyExists;
        }
    }

    ProductError::Database(error)
}

fn validate_product(name: &str, variant: &ProductVariantRequest) -> Result<(), ProductError> {
    if name.trim().is_empty() || variant.name.trim().is_empty() {
        return Err(ProductError::InvalidName);
    }

    if variant.selling_price < Decimal::ZERO
        || variant
            .cost_price
            .is_some_and(|price| price < Decimal::ZERO)
    {
        return Err(ProductError::InvalidPrice);
    }

    Ok(())
}

async fn validate_references(
    connection: &mut PgConnection,
    company_id: Uuid,
    category_id: Option<i64>,
    unit_id: i64,
) -> Result<(), ProductError> {
    if let Some(category_id) = category_id {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM categories
                WHERE id = $1
                  AND company_id = $2
                  AND is_active = TRUE
            )
            "#,
        )
        .bind(category_id)
        .bind(company_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(ProductError::Database)?;

        if !exists {
            return Err(ProductError::InvalidCategory);
        }
    }

    let unit_exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM units_of_measure
            WHERE id = $1
              AND is_active = TRUE
        )
        "#,
    )
    .bind(unit_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(ProductError::Database)?;

    if !unit_exists {
        return Err(ProductError::InvalidUnit);
    }

    Ok(())
}

fn validate_variant(variant: &ProductVariantRequest) -> Result<(), ProductError> {
    if variant.name.trim().is_empty() {
        return Err(ProductError::InvalidName);
    }

    if variant.selling_price < Decimal::ZERO
        || variant
            .cost_price
            .is_some_and(|price| price < Decimal::ZERO)
    {
        return Err(ProductError::InvalidPrice);
    }

    Ok(())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_product_list_query(
    query: ProductListQuery,
) -> Result<(i64, i64, Option<i64>, Option<String>), ProductError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    if !(1..=100).contains(&limit) || offset < 0 {
        return Err(ProductError::InvalidQuery);
    }

    if query.category_id.is_some_and(|id| id <= 0) {
        return Err(ProductError::InvalidQuery);
    }

    let search = query
        .search
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if search
        .as_ref()
        .is_some_and(|value| value.chars().count() > 100)
    {
        return Err(ProductError::InvalidQuery);
    }

    Ok((limit, offset, query.category_id, search))
}

// Locks the parent product before any variant modification.
async fn lock_product(
    connection: &mut PgConnection,
    company_id: Uuid,
    product_id: i64,
) -> Result<(), ProductError> {
    let product = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM products
        WHERE id = $1
          AND company_id = $2
          AND is_active = TRUE
        FOR UPDATE
        "#,
    )
    .bind(product_id)
    .bind(company_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(ProductError::Database)?;

    if product.is_none() {
        return Err(ProductError::NotFound);
    }

    Ok(())
}

async fn fetch_variant(
    state: &AppState,
    company_id: Uuid,
    product_id: i64,
    variant_id: i64,
) -> Result<ProductVariant, ProductError> {
    let product = get_product(state, company_id, product_id).await?;

    product
        .variants
        .into_iter()
        .find(|variant| variant.id == variant_id)
        .ok_or(ProductError::VariantNotFound)
}

// Pagination is performed on products before variants are joined.
// Therefore, variants belonging to one product are never split across pages.
async fn fetch_products(
    pool: &PgPool,
    company_id: Uuid,
    product_id: Option<i64>,
    category_id: Option<i64>,
    search: Option<String>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Product>, ProductError> {
    let rows = sqlx::query_as::<_, ProductQueryRow>(
        r#"
        WITH product_page AS (
            SELECT p.id, p.name
            FROM products p
            WHERE p.company_id = $1
              AND p.is_active = TRUE
              AND ($2::BIGINT IS NULL OR p.category_id = $2)
              AND (
                    $3::TEXT IS NULL
                    OR p.name ILIKE '%' || $3 || '%'
                    OR EXISTS (
                        SELECT 1
                        FROM product_variants sv
                        WHERE sv.product_id = p.id
                          AND sv.company_id = p.company_id
                          AND sv.is_active = TRUE
                          AND (
                              sv.name ILIKE '%' || $3 || '%'
                              OR COALESCE(sv.sku, '') ILIKE '%' || $3 || '%'
                              OR COALESCE(sv.barcode, '') ILIKE '%' || $3 || '%'
                          )
                    )
              )
              AND ($4::BIGINT IS NULL OR p.id = $4)
            ORDER BY p.name, p.id
            LIMIT $5 OFFSET $6
        )
        SELECT
            p.id,
            p.name,
            p.description,
            p.category_id,
            p.track_inventory,
            p.is_active,
            p.version,
            v.id AS variant_id,
            v.name AS variant_name,
            v.sku,
            v.barcode,
            v.unit_id,
            u.code AS unit_code,
            u.name AS unit_name,
            u.symbol AS unit_symbol,
            v.selling_price,
            v.cost_price,
            v.is_default,
            v.is_active AS variant_is_active,
            v.version AS variant_version
        FROM product_page page
        JOIN products p
            ON p.id = page.id
           AND p.company_id = $1
        JOIN product_variants v
            ON v.product_id = p.id
           AND v.company_id = p.company_id
        JOIN units_of_measure u
            ON u.id = v.unit_id
        ORDER BY page.name, p.id, v.id
        "#,
    )
    .bind(company_id)
    .bind(category_id)
    .bind(search)
    .bind(product_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(ProductError::Database)?;

    let mut products: Vec<Product> = Vec::new();

    for row in rows {
        let current_product_id = row.id;

        if products.last().map(|product| product.id) != Some(current_product_id) {
            products.push(Product {
                id: row.id,
                name: row.name,
                description: row.description,
                category_id: row.category_id,
                track_inventory: row.track_inventory,
                is_active: row.is_active,
                version: row.version,
                variants: Vec::new(),
            });
        }

        let variant = ProductVariant {
            id: row.variant_id,
            name: row.variant_name,
            sku: row.sku,
            barcode: row.barcode,
            unit_id: row.unit_id,
            unit_code: row.unit_code,
            unit_name: row.unit_name,
            unit_symbol: row.unit_symbol,
            selling_price: row.selling_price,
            cost_price: row.cost_price,
            is_default: row.is_default,
            is_active: row.variant_is_active,
            version: row.variant_version,
        };

        if let Some(product) = products.last_mut() {
            product.variants.push(variant);
        }
    }

    Ok(products)
}

pub async fn create_product(
    state: &AppState,
    company_id: Uuid,
    request: CreateProductRequest,
) -> Result<Product, ProductError> {
    validate_product(&request.name, &request.variant)?;

    let name = request.name.trim().to_string();
    let variant_name = request.variant.name.trim().to_string();

    let sku = normalize_optional_string(request.variant.sku);
    let barcode = normalize_optional_string(request.variant.barcode);

    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    validate_references(
        &mut transaction,
        company_id,
        request.category_id,
        request.variant.unit_id,
    )
    .await?;

    let product_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO products (
            company_id, name, price, description, category_id, track_inventory
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(&name)
    .bind(request.variant.selling_price)
    .bind(&request.description)
    .bind(request.category_id)
    .bind(request.track_inventory)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    let variant_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO product_variants (
            company_id, product_id, name, sku, barcode, unit_id,
            selling_price, cost_price, is_default
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, TRUE)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(product_id)
    .bind(&variant_name)
    .bind(&sku)
    .bind(&barcode)
    .bind(request.variant.unit_id)
    .bind(request.variant.selling_price)
    .bind(request.variant.cost_price)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    sqlx::query(
        r#"
        INSERT INTO stock_balances (company_id, location_id, variant_id)
        SELECT $1, id, $2
        FROM inventory_locations
        WHERE company_id = $1
          AND is_default = TRUE
        "#,
    )
    .bind(company_id)
    .bind(variant_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    transaction.commit().await.map_err(ProductError::Database)?;

    get_product(state, company_id, product_id).await
}

pub async fn get_product(
    state: &AppState,
    company_id: Uuid,
    id: i64,
) -> Result<Product, ProductError> {
    fetch_products(&state.db, company_id, Some(id), None, None, 1, 0)
        .await?
        .into_iter()
        .next()
        .ok_or(ProductError::NotFound)
}

pub async fn get_products(
    state: &AppState,
    company_id: Uuid,
    query: ProductListQuery,
) -> Result<Vec<Product>, ProductError> {
    let (limit, offset, category_id, search) = validate_product_list_query(query)?;

    fetch_products(
        &state.db,
        company_id,
        None,
        category_id,
        search,
        limit,
        offset,
    )
    .await
}

pub async fn update_product(
    state: &AppState,
    company_id: Uuid,
    id: i64,
    request: UpdateProductRequest,
) -> Result<Product, ProductError> {
    validate_product(&request.name, &request.variant)?;

    let name = request.name.trim().to_string();
    let variant_name = request.variant.name.trim().to_string();

    let sku = normalize_optional_string(request.variant.sku);
    let barcode = normalize_optional_string(request.variant.barcode);

    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    validate_references(
        &mut transaction,
        company_id,
        request.category_id,
        request.variant.unit_id,
    )
    .await?;

    let result = sqlx::query(
        r#"
        UPDATE products
        SET name = $1,
            description = $2,
            category_id = $3,
            track_inventory = $4,
            price = $5,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $6
          AND company_id = $7
          AND is_active = TRUE
        "#,
    )
    .bind(&name)
    .bind(&request.description)
    .bind(request.category_id)
    .bind(request.track_inventory)
    .bind(request.variant.selling_price)
    .bind(id)
    .bind(company_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    if result.rows_affected() == 0 {
        return Err(ProductError::NotFound);
    }

    let variant_result = sqlx::query(
        r#"
        UPDATE product_variants
        SET name = $1,
            sku = $2,
            barcode = $3,
            unit_id = $4,
            selling_price = $5,
            cost_price = $6,
            updated_at = NOW(),
            version = version + 1
        WHERE product_id = $7
          AND company_id = $8
          AND is_default = TRUE
          AND is_active = TRUE
        "#,
    )
    .bind(&variant_name)
    .bind(&sku)
    .bind(&barcode)
    .bind(request.variant.unit_id)
    .bind(request.variant.selling_price)
    .bind(request.variant.cost_price)
    .bind(id)
    .bind(company_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    if variant_result.rows_affected() == 0 {
        return Err(ProductError::NotFound);
    }

    transaction.commit().await.map_err(ProductError::Database)?;

    get_product(state, company_id, id).await
}

pub async fn delete_product(
    state: &AppState,
    company_id: Uuid,
    id: i64,
) -> Result<(), ProductError> {
    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    let result = sqlx::query(
        r#"
        UPDATE products
        SET is_active = FALSE,
            archived_at = NOW(),
            updated_at = NOW(),
            version = version + 1
        WHERE id = $1
          AND company_id = $2
          AND is_active = TRUE
        "#,
    )
    .bind(id)
    .bind(company_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    if result.rows_affected() == 0 {
        return Err(ProductError::NotFound);
    }

    sqlx::query(
        r#"
        UPDATE product_variants
        SET is_active = FALSE,
            updated_at = NOW(),
            version = version + 1
        WHERE product_id = $1
          AND company_id = $2
        "#,
    )
    .bind(id)
    .bind(company_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    transaction.commit().await.map_err(ProductError::Database)?;

    Ok(())
}

pub async fn get_product_variants(
    state: &AppState,
    company_id: Uuid,
    product_id: i64,
) -> Result<Vec<ProductVariant>, ProductError> {
    let product = get_product(state, company_id, product_id).await?;
    Ok(product.variants)
}

pub async fn create_product_variant(
    state: &AppState,
    company_id: Uuid,
    product_id: i64,
    request: ProductVariantRequest,
) -> Result<ProductVariant, ProductError> {
    validate_variant(&request)?;

    let name = request.name.trim().to_string();
    let sku = normalize_optional_string(request.sku);
    let barcode = normalize_optional_string(request.barcode);

    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    lock_product(&mut transaction, company_id, product_id).await?;
    validate_references(&mut transaction, company_id, None, request.unit_id).await?;

    let variant_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO product_variants (
            company_id, product_id, name, sku, barcode, unit_id,
            selling_price, cost_price, is_default
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(product_id)
    .bind(&name)
    .bind(&sku)
    .bind(&barcode)
    .bind(request.unit_id)
    .bind(request.selling_price)
    .bind(request.cost_price)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    let balance_result = sqlx::query(
        r#"
        INSERT INTO stock_balances (company_id, location_id, variant_id)
        SELECT company_id, id, $2
        FROM inventory_locations
        WHERE company_id = $1
        "#,
    )
    .bind(company_id)
    .bind(variant_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    if balance_result.rows_affected() == 0 {
        return Err(ProductError::NoInventoryLocations);
    }

    transaction.commit().await.map_err(ProductError::Database)?;

    fetch_variant(state, company_id, product_id, variant_id).await
}

pub async fn update_product_variant(
    state: &AppState,
    company_id: Uuid,
    product_id: i64,
    variant_id: i64,
    request: UpdateProductVariantRequest,
) -> Result<ProductVariant, ProductError> {
    if request.name.trim().is_empty() {
        return Err(ProductError::InvalidName);
    }

    if request.selling_price < Decimal::ZERO
        || request
            .cost_price
            .is_some_and(|price| price < Decimal::ZERO)
    {
        return Err(ProductError::InvalidPrice);
    }

    if request.expected_version <= 0 {
        return Err(ProductError::VersionConflict);
    }

    let name = request.name.trim().to_string();
    let sku = normalize_optional_string(request.sku);
    let barcode = normalize_optional_string(request.barcode);

    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    lock_product(&mut transaction, company_id, product_id).await?;
    validate_references(&mut transaction, company_id, None, request.unit_id).await?;

    let result = sqlx::query(
        r#"
        UPDATE product_variants
        SET name = $1,
            sku = $2,
            barcode = $3,
            unit_id = $4,
            selling_price = $5,
            cost_price = $6,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $7
          AND product_id = $8
          AND company_id = $9
          AND is_active = TRUE
          AND version = $10
        "#,
    )
    .bind(&name)
    .bind(&sku)
    .bind(&barcode)
    .bind(request.unit_id)
    .bind(request.selling_price)
    .bind(request.cost_price)
    .bind(variant_id)
    .bind(product_id)
    .bind(company_id)
    .bind(request.expected_version)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    if result.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM product_variants
                WHERE id = $1
                  AND product_id = $2
                  AND company_id = $3
                  AND is_active = TRUE
            )
            "#,
        )
        .bind(variant_id)
        .bind(product_id)
        .bind(company_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ProductError::Database)?;

        if exists {
            return Err(ProductError::VersionConflict);
        }

        return Err(ProductError::VariantNotFound);
    }

    transaction.commit().await.map_err(ProductError::Database)?;

    fetch_variant(state, company_id, product_id, variant_id).await
}

pub async fn deactivate_product_variant(
    state: &AppState,
    company_id: Uuid,
    product_id: i64,
    variant_id: i64,
) -> Result<(), ProductError> {
    let mut transaction = state.db.begin().await.map_err(ProductError::Database)?;

    lock_product(&mut transaction, company_id, product_id).await?;

    let variant = sqlx::query_as::<_, (i64, bool)>(
        r#"
        SELECT id, is_default
        FROM product_variants
        WHERE id = $1
          AND product_id = $2
          AND company_id = $3
          AND is_active = TRUE
        FOR UPDATE
        "#,
    )
    .bind(variant_id)
    .bind(product_id)
    .bind(company_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    let (_, is_default) = variant.ok_or(ProductError::VariantNotFound)?;

    let replacement = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM product_variants
        WHERE product_id = $1
          AND company_id = $2
          AND is_active = TRUE
          AND id <> $3
        ORDER BY created_at, id
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(product_id)
    .bind(company_id)
    .bind(variant_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    let replacement_id = replacement.ok_or(ProductError::CannotDeactivateLastVariant)?;

    if is_default {
        sqlx::query(
            r#"
            UPDATE product_variants
            SET is_default = FALSE
            WHERE id = $1
              AND product_id = $2
              AND company_id = $3
            "#,
        )
        .bind(variant_id)
        .bind(product_id)
        .bind(company_id)
        .execute(&mut *transaction)
        .await
        .map_err(ProductError::Database)?;

        sqlx::query(
            r#"
            UPDATE product_variants
            SET is_default = TRUE,
                updated_at = NOW(),
                version = version + 1
            WHERE id = $1
              AND product_id = $2
              AND company_id = $3
              AND is_active = TRUE
            "#,
        )
        .bind(replacement_id)
        .bind(product_id)
        .bind(company_id)
        .execute(&mut *transaction)
        .await
        .map_err(ProductError::Database)?;
    }

    sqlx::query(
        r#"
        UPDATE product_variants
        SET is_active = FALSE,
            is_default = FALSE,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $1
          AND product_id = $2
          AND company_id = $3
          AND is_active = TRUE
        "#,
    )
    .bind(variant_id)
    .bind(product_id)
    .bind(company_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProductError::Database)?;

    transaction.commit().await.map_err(ProductError::Database)?;

    Ok(())
}
