use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

use crate::models::inventory::{
    MovementQuery, StockAdjustmentRequest, StockAdjustmentResponse, StockItem, StockMovement,
    StockQuery,
};

#[derive(Debug)]
pub enum InventoryError {
    Database(Error),
    InvalidAdjustment,
    InvalidQuantity,
    InsufficientStock,
    LocationNotFound,
    VariantNotFound,
    BalanceNotFound,
    IdempotencyConflict,
}

fn normalize_notes(notes: Option<String>) -> Option<String> {
    notes.and_then(|value| {
        let value = value.trim().to_string();

        if value.is_empty() { None } else { Some(value) }
    })
}

pub async fn get_stock(
    db: &PgPool,
    company_id: Uuid,
    query: StockQuery,
) -> Result<Vec<StockItem>, InventoryError> {
    let rows = sqlx::query_as::<_, StockItem>(
        r#"
        SELECT
            sb.location_id,
            il.name AS location_name,
            pv.id AS variant_id,
            p.id AS product_id,
            p.name AS product_name,
            pv.name AS variant_name,
            pv.sku,
            pv.barcode,
            u.code AS unit_code,
            u.name AS unit_name,
            sb.quantity_on_hand,
            sb.reserved_quantity,
            sb.quantity_on_hand - sb.reserved_quantity
                AS available_quantity,
            sb.version
        FROM stock_balances sb
        JOIN inventory_locations il
            ON il.id = sb.location_id
            AND il.company_id = sb.company_id
        JOIN product_variants pv
            ON pv.id = sb.variant_id
            AND pv.company_id = sb.company_id
        JOIN products p
            ON p.id = pv.product_id
            AND p.company_id = pv.company_id
        JOIN units_of_measure u
            ON u.id = pv.unit_id
        WHERE sb.company_id = $1
          AND il.is_active = TRUE
          AND p.is_active = TRUE
          AND pv.is_active = TRUE
          AND p.track_inventory = TRUE
          AND ($2::BIGINT IS NULL OR sb.location_id = $2)
          AND ($3::BIGINT IS NULL OR sb.variant_id = $3)
        ORDER BY p.name, pv.name, il.name
        "#,
    )
    .bind(company_id)
    .bind(query.location_id)
    .bind(query.variant_id)
    .fetch_all(db)
    .await
    .map_err(InventoryError::Database)?;

    Ok(rows)
}

pub async fn get_movements(
    db: &PgPool,
    company_id: Uuid,
    query: MovementQuery,
) -> Result<Vec<StockMovement>, InventoryError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let rows = sqlx::query_as::<_, StockMovement>(
        r#"
        SELECT
            id,
            location_id,
            variant_id,
            movement_type,
            quantity_delta,
            reference_type,
            reference_id,
            notes,
            created_by,
            created_at
        FROM stock_movements
        WHERE company_id = $1
          AND ($2::BIGINT IS NULL OR location_id = $2)
          AND ($3::BIGINT IS NULL OR variant_id = $3)
        ORDER BY created_at DESC, id DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(company_id)
    .bind(query.location_id)
    .bind(query.variant_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(InventoryError::Database)?;

    Ok(rows)
}

pub async fn adjust_stock(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    request: StockAdjustmentRequest,
) -> Result<StockAdjustmentResponse, InventoryError> {
    if request.quantity_delta == Decimal::ZERO {
        return Err(InventoryError::InvalidQuantity);
    }

    let valid_type = match request.movement_type.as_str() {
        "adjustment_in" => request.quantity_delta > Decimal::ZERO,
        "adjustment_out" => request.quantity_delta < Decimal::ZERO,
        "count_adjustment" => true,
        _ => false,
    };

    if !valid_type {
        return Err(InventoryError::InvalidAdjustment);
    }

    let notes = normalize_notes(request.notes);

    let mut tx = db.begin().await.map_err(InventoryError::Database)?;

    // Serialize requests using the same idempotency key.
    // Hash collisions only cause additional serialization;
    // they do not compromise correctness.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(request.idempotency_key.to_string())
        .execute(&mut *tx)
        .await
        .map_err(InventoryError::Database)?;

    // A repeated request must not change stock twice.
    let existing = sqlx::query(
        r#"
        SELECT
            id,
            location_id,
            variant_id,
            movement_type,
            quantity_delta,
            notes
        FROM stock_movements
        WHERE company_id = $1
          AND idempotency_key = $2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    if let Some(existing) = existing {
        let same_request = existing
            .try_get::<i64, _>("location_id")
            .map_err(InventoryError::Database)?
            == request.location_id
            && existing
                .try_get::<i64, _>("variant_id")
                .map_err(InventoryError::Database)?
                == request.variant_id
            && existing
                .try_get::<String, _>("movement_type")
                .map_err(InventoryError::Database)?
                == request.movement_type
            && existing
                .try_get::<Decimal, _>("quantity_delta")
                .map_err(InventoryError::Database)?
                == request.quantity_delta
            && existing
                .try_get::<Option<String>, _>("notes")
                .map_err(InventoryError::Database)?
                == notes;

        if !same_request {
            return Err(InventoryError::IdempotencyConflict);
        }

        let balance = sqlx::query(
            r#"
            SELECT quantity_on_hand, reserved_quantity, version
            FROM stock_balances
            WHERE company_id = $1
              AND location_id = $2
              AND variant_id = $3
            "#,
        )
        .bind(company_id)
        .bind(request.location_id)
        .bind(request.variant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(InventoryError::Database)?;

        let response = StockAdjustmentResponse {
            movement_id: existing.try_get("id").map_err(InventoryError::Database)?,
            variant_id: request.variant_id,
            location_id: request.location_id,
            movement_type: request.movement_type,
            quantity_delta: request.quantity_delta,
            quantity_on_hand: balance
                .try_get("quantity_on_hand")
                .map_err(InventoryError::Database)?,
            reserved_quantity: balance
                .try_get("reserved_quantity")
                .map_err(InventoryError::Database)?,
            available_quantity: balance
                .try_get::<Decimal, _>("quantity_on_hand")
                .map_err(InventoryError::Database)?
                - balance
                    .try_get::<Decimal, _>("reserved_quantity")
                    .map_err(InventoryError::Database)?,
            version: balance
                .try_get("version")
                .map_err(InventoryError::Database)?,
        };

        tx.commit().await.map_err(InventoryError::Database)?;
        return Ok(response);
    }

    // Confirm the location belongs to this company and is active.
    let location_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM inventory_locations
            WHERE id = $1
              AND company_id = $2
              AND is_active = TRUE
        )
        "#,
    )
    .bind(request.location_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    if !location_exists {
        return Err(InventoryError::LocationNotFound);
    }

    // Confirm the variant is active and inventory-tracked.
    let variant_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM product_variants pv
            JOIN products p
              ON p.id = pv.product_id
             AND p.company_id = pv.company_id
            WHERE pv.id = $1
              AND pv.company_id = $2
              AND pv.is_active = TRUE
              AND p.is_active = TRUE
              AND p.track_inventory = TRUE
        )
        "#,
    )
    .bind(request.variant_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    if !variant_exists {
        return Err(InventoryError::VariantNotFound);
    }

    // Ensure a balance row exists, then lock it.
    sqlx::query(
        r#"
        INSERT INTO stock_balances (
            company_id,
            location_id,
            variant_id
        )
        VALUES ($1, $2, $3)
        ON CONFLICT (location_id, variant_id) DO NOTHING
        "#,
    )
    .bind(company_id)
    .bind(request.location_id)
    .bind(request.variant_id)
    .execute(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    let balance = sqlx::query(
        r#"
        SELECT quantity_on_hand, reserved_quantity, version
        FROM stock_balances
        WHERE company_id = $1
          AND location_id = $2
          AND variant_id = $3
        FOR UPDATE
        "#,
    )
    .bind(company_id)
    .bind(request.location_id)
    .bind(request.variant_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(InventoryError::Database)?
    .ok_or(InventoryError::BalanceNotFound)?;

    let old_quantity: Decimal = balance
        .try_get("quantity_on_hand")
        .map_err(InventoryError::Database)?;

    let reserved_quantity: Decimal = balance
        .try_get("reserved_quantity")
        .map_err(InventoryError::Database)?;

    let version: i32 = balance
        .try_get("version")
        .map_err(InventoryError::Database)?;

    let new_quantity = old_quantity + request.quantity_delta;

    if new_quantity < reserved_quantity {
        return Err(InventoryError::InsufficientStock);
    }

    // Insert the immutable audit record.
    let movement_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO stock_movements (
            company_id,
            location_id,
            variant_id,
            movement_type,
            quantity_delta,
            idempotency_key,
            notes,
            created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(request.location_id)
    .bind(request.variant_id)
    .bind(&request.movement_type)
    .bind(request.quantity_delta)
    .bind(request.idempotency_key)
    .bind(&notes)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    // Update the current balance in the same transaction.
    let updated = sqlx::query(
        r#"
        UPDATE stock_balances
        SET quantity_on_hand = $1,
            updated_at = NOW(),
            version = version + 1
        WHERE company_id = $2
          AND location_id = $3
          AND variant_id = $4
        "#,
    )
    .bind(new_quantity)
    .bind(company_id)
    .bind(request.location_id)
    .bind(request.variant_id)
    .execute(&mut *tx)
    .await
    .map_err(InventoryError::Database)?;

    if updated.rows_affected() != 1 {
        return Err(InventoryError::BalanceNotFound);
    }

    tx.commit().await.map_err(InventoryError::Database)?;

    Ok(StockAdjustmentResponse {
        movement_id,
        variant_id: request.variant_id,
        location_id: request.location_id,
        movement_type: request.movement_type,
        quantity_delta: request.quantity_delta,
        quantity_on_hand: new_quantity,
        reserved_quantity,
        available_quantity: new_quantity - reserved_quantity,
        version: version + 1,
    })
}
