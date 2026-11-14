use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::catalog::*,
};

fn clean(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn name_ok(name: &str, max: usize) -> bool {
    let n = name.trim().chars().count();
    n >= 1 && n <= max
}

pub async fn list_units(db: &PgPool) -> ApiResult<Vec<Unit>> {
    Ok(sqlx::query_as::<_, Unit>(
        "SELECT id, code, name, symbol, allows_decimal, is_active \
         FROM units_of_measure WHERE is_active ORDER BY name",
    )
    .fetch_all(db)
    .await?)
}

// ---------------------------------------------------------------- categories

pub async fn list_categories(db: &PgPool, company: Uuid) -> ApiResult<Vec<Category>> {
    Ok(sqlx::query_as::<_, Category>(
        "SELECT id, parent_id, name, description, is_active, version \
         FROM categories WHERE company_id = $1 ORDER BY lower(name)",
    )
    .bind(company)
    .fetch_all(db)
    .await?)
}

pub async fn create_category(
    db: &PgPool,
    company: Uuid,
    req: CategoryCreate,
) -> ApiResult<Category> {
    if !name_ok(&req.name, 100) {
        return Err(ApiError::bad_request(
            "INVALID_NAME",
            "Category name must be 1-100 characters",
        ));
    }
    Ok(sqlx::query_as::<_, Category>(
        "INSERT INTO categories (company_id, parent_id, name, description) \
         VALUES ($1,$2,$3,$4) \
         RETURNING id, parent_id, name, description, is_active, version",
    )
    .bind(company)
    .bind(req.parent_id)
    .bind(req.name.trim())
    .bind(clean(req.description))
    .fetch_one(db)
    .await?)
}

pub async fn update_category(
    db: &PgPool,
    company: Uuid,
    id: i64,
    req: CategoryUpdate,
) -> ApiResult<Category> {
    if !name_ok(&req.name, 100) {
        return Err(ApiError::bad_request(
            "INVALID_NAME",
            "Category name must be 1-100 characters",
        ));
    }
    if let Some(parent) = req.parent_id {
        // Reject cycles: this category must not be an ancestor of the new parent.
        let cycle: bool = sqlx::query_scalar(
            "WITH RECURSIVE anc AS ( \
               SELECT id, parent_id FROM categories WHERE id=$1 AND company_id=$2 \
               UNION ALL \
               SELECT c.id, c.parent_id FROM categories c JOIN anc ON c.id = anc.parent_id \
               WHERE c.company_id=$2 ) \
             SELECT EXISTS (SELECT 1 FROM anc WHERE id=$3)",
        )
        .bind(parent)
        .bind(company)
        .bind(id)
        .fetch_one(db)
        .await?;
        if cycle {
            return Err(ApiError::bad_request(
                "CATEGORY_CYCLE",
                "A category cannot be moved under itself or its descendants",
            ));
        }
    }
    let row = sqlx::query_as::<_, Category>(
        "UPDATE categories SET name=$1, description=$2, parent_id=$3, is_active=$4, \
         version=version+1, updated_at=NOW() \
         WHERE id=$5 AND company_id=$6 AND version=$7 \
         RETURNING id, parent_id, name, description, is_active, version",
    )
    .bind(req.name.trim())
    .bind(clean(req.description))
    .bind(req.parent_id)
    .bind(req.is_active)
    .bind(id)
    .bind(company)
    .bind(req.expected_version)
    .fetch_optional(db)
    .await?;
    match row {
        Some(c) => Ok(c),
        None => Err(missing_or_conflict(db, "categories", id, company).await?),
    }
}

async fn missing_or_conflict(
    db: &PgPool,
    table: &'static str,
    id: i64,
    company: Uuid,
) -> ApiResult<ApiError> {
    let sql = match table {
        "categories" => "SELECT EXISTS(SELECT 1 FROM categories WHERE id=$1 AND company_id=$2)",
        _ => "SELECT EXISTS(SELECT 1 FROM inventory_locations WHERE id=$1 AND company_id=$2)",
    };
    let exists: bool = sqlx::query_scalar(sql)
        .bind(id)
        .bind(company)
        .fetch_one(db)
        .await?;
    Ok(if exists {
        ApiError::conflict(
            "VERSION_CONFLICT",
            "The record has been modified. Please refresh and try again",
        )
    } else {
        ApiError::not_found("NOT_FOUND", "The requested record was not found")
    })
}

// ----------------------------------------------------------------- locations

pub async fn list_locations(db: &PgPool, company: Uuid) -> ApiResult<Vec<Location>> {
    Ok(sqlx::query_as::<_, Location>(
        "SELECT id, name, code, location_type, address, is_default, is_active, version \
         FROM inventory_locations WHERE company_id=$1 ORDER BY is_default DESC, name",
    )
    .bind(company)
    .fetch_all(db)
    .await?)
}

fn valid_location_type(t: &str) -> bool {
    t == "store" || t == "warehouse"
}

pub async fn create_location(
    db: &PgPool,
    company: Uuid,
    req: LocationCreate,
) -> ApiResult<Location> {
    let code = req.code.trim().to_uppercase();
    let ltype = req.location_type.unwrap_or_else(|| "store".into());
    if !name_ok(&req.name, 100)
        || code.is_empty()
        || code.len() > 30
        || !valid_location_type(&ltype)
    {
        return Err(ApiError::bad_request(
            "INVALID_LOCATION",
            "Name (1-100), code (1-30) and type (store or warehouse) are required",
        ));
    }
    let mut tx = db.begin().await?;
    let location = sqlx::query_as::<_, Location>(
        "INSERT INTO inventory_locations (company_id, name, code, location_type, address) \
         VALUES ($1,$2,$3,$4,$5) \
         RETURNING id, name, code, location_type, address, is_default, is_active, version",
    )
    .bind(company)
    .bind(req.name.trim())
    .bind(&code)
    .bind(&ltype)
    .bind(clean(req.address))
    .fetch_one(&mut *tx)
    .await?;
    // Pre-create balance rows so the new location shows up in stock listings.
    sqlx::query(
        "INSERT INTO stock_balances (company_id, location_id, variant_id) \
         SELECT company_id, $2, id FROM product_variants WHERE company_id=$1 \
         ON CONFLICT DO NOTHING",
    )
    .bind(company)
    .bind(location.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(location)
}

pub async fn update_location(
    db: &PgPool,
    company: Uuid,
    id: i64,
    req: LocationUpdate,
) -> ApiResult<Location> {
    let ltype = req.location_type.unwrap_or_else(|| "store".into());
    if !name_ok(&req.name, 100) || !valid_location_type(&ltype) {
        return Err(ApiError::bad_request(
            "INVALID_LOCATION",
            "Name (1-100) and type (store or warehouse) are required",
        ));
    }
    let row = sqlx::query_as::<_, Location>(
        "UPDATE inventory_locations SET name=$1, location_type=$2, address=$3, is_active=$4, \
         version=version+1, updated_at=NOW() \
         WHERE id=$5 AND company_id=$6 AND version=$7 AND ($4 OR NOT is_default) \
         RETURNING id, name, code, location_type, address, is_default, is_active, version",
    )
    .bind(req.name.trim())
    .bind(&ltype)
    .bind(clean(req.address))
    .bind(req.is_active)
    .bind(id)
    .bind(company)
    .bind(req.expected_version)
    .fetch_optional(db)
    .await?;
    match row {
        Some(l) => Ok(l),
        None => {
            let is_default: Option<bool> = sqlx::query_scalar(
                "SELECT is_default FROM inventory_locations WHERE id=$1 AND company_id=$2",
            )
            .bind(id)
            .bind(company)
            .fetch_optional(db)
            .await?;
            if is_default == Some(true) && !req.is_active {
                return Err(ApiError::conflict(
                    "DEFAULT_LOCATION",
                    "The default location cannot be deactivated",
                ));
            }
            Err(missing_or_conflict(db, "locations", id, company).await?)
        }
    }
}
