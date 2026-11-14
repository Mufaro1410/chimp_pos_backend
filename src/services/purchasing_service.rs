use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use std::collections::HashSet;
use uuid::Uuid;

use crate::models::purchasing::*;

#[derive(Debug)]
pub enum PurchasingError {
    Database(Error),
    NotFound,
    SupplierNotFound,
    InvalidSupplier,
    InvalidItems,
    InvalidQuantity,
    InvalidPrice,
    InvalidDiscount,
    InvalidDate,
    InvalidStatus,
    VersionConflict,
    EmptyOrder,
    OrderNotEditable,
    OrderNotReceivable,
    LocationNotFound,
    ItemNotFound,
    ExcessiveReceipt,
    IdempotencyConflict,
}

#[derive(Debug)]
pub struct ReceiveGoodsOutcome {
    pub receipt: GoodsReceipt,
    pub replayed: bool,
}

fn normalize_notes(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn validate_decimal(value: Decimal) -> bool {
    value.scale() <= 4 && value.abs() < Decimal::from(1_000_000_000_000_000_i64)
}

fn validate_items(items: &[PurchaseOrderItemRequest]) -> Result<(), PurchasingError> {
    if items.is_empty() {
        return Err(PurchasingError::EmptyOrder);
    }

    for item in items {
        if item.ordered_quantity <= Decimal::ZERO || !validate_decimal(item.ordered_quantity) {
            return Err(PurchasingError::InvalidQuantity);
        }

        if item.unit_cost < Decimal::ZERO || !validate_decimal(item.unit_cost) {
            return Err(PurchasingError::InvalidPrice);
        }
    }

    Ok(())
}

fn validate_receipt_items(items: &[ReceiveGoodsItemRequest]) -> Result<(), PurchasingError> {
    if items.is_empty() {
        return Err(PurchasingError::EmptyOrder);
    }

    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item.purchase_order_item_id) {
            return Err(PurchasingError::InvalidItems);
        }

        if item.quantity_received <= Decimal::ZERO || !validate_decimal(item.quantity_received) {
            return Err(PurchasingError::InvalidQuantity);
        }

        if let Some(cost) = item.unit_cost {
            if cost < Decimal::ZERO || !validate_decimal(cost) {
                return Err(PurchasingError::InvalidPrice);
            }
        }
    }

    Ok(())
}

async fn validate_supplier(
    db: &PgPool,
    company_id: Uuid,
    supplier_id: i64,
) -> Result<(), PurchasingError> {
    let valid = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM suppliers
            WHERE id = $1
              AND company_id = $2
              AND is_active = TRUE
        )
        "#,
    )
    .bind(supplier_id)
    .bind(company_id)
    .fetch_one(db)
    .await
    .map_err(PurchasingError::Database)?;

    if !valid {
        return Err(PurchasingError::SupplierNotFound);
    }

    Ok(())
}

async fn insert_order_items(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    purchase_order_id: i64,
    items: &[PurchaseOrderItemRequest],
) -> Result<Decimal, PurchasingError> {
    let mut subtotal = Decimal::ZERO;

    for item in items {
        let row = sqlx::query(
            r#"
            SELECT
                pv.id AS variant_id,
                pv.name AS variant_name,
                pv.sku,
                p.name AS product_name,
                p.track_inventory,
                u.code AS unit_code,
                u.name AS unit_name
            FROM product_variants pv
            JOIN products p
              ON p.id = pv.product_id
             AND p.company_id = pv.company_id
            JOIN units_of_measure u
              ON u.id = pv.unit_id
            WHERE pv.id = $1
              AND pv.company_id = $2
              AND pv.is_active = TRUE
              AND p.is_active = TRUE
              AND p.track_inventory = TRUE
            "#,
        )
        .bind(item.variant_id)
        .bind(company_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(PurchasingError::Database)?
        .ok_or(PurchasingError::ItemNotFound)?;

        let product_name: String = row
            .try_get("product_name")
            .map_err(PurchasingError::Database)?;

        let variant_name: String = row
            .try_get("variant_name")
            .map_err(PurchasingError::Database)?;

        let sku: Option<String> = row.try_get("sku").map_err(PurchasingError::Database)?;

        let unit_code: String = row
            .try_get("unit_code")
            .map_err(PurchasingError::Database)?;

        let unit_name: String = row
            .try_get("unit_name")
            .map_err(PurchasingError::Database)?;

        let line_total = (item.ordered_quantity * item.unit_cost).round_dp(4);

        sqlx::query(
            r#"
            INSERT INTO purchase_order_items (
                company_id,
                purchase_order_id,
                variant_id,
                product_name,
                variant_name,
                sku,
                unit_code,
                unit_name,
                ordered_quantity,
                received_quantity,
                unit_cost,
                line_total
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,0,$10,$11)
            "#,
        )
        .bind(company_id)
        .bind(purchase_order_id)
        .bind(item.variant_id)
        .bind(product_name)
        .bind(variant_name)
        .bind(sku)
        .bind(unit_code)
        .bind(unit_name)
        .bind(item.ordered_quantity)
        .bind(item.unit_cost)
        .bind(line_total)
        .execute(&mut **tx)
        .await
        .map_err(PurchasingError::Database)?;

        subtotal += line_total;
    }

    Ok(subtotal)
}

async fn allocate_order_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
) -> Result<i64, PurchasingError> {
    sqlx::query_scalar(
        r#"
        INSERT INTO purchase_order_sequences (
            company_id,
            next_number
        )
        VALUES ($1, 2)
        ON CONFLICT (company_id)
        DO UPDATE SET next_number =
            purchase_order_sequences.next_number + 1
        RETURNING next_number - 1
        "#,
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(PurchasingError::Database)
}

async fn allocate_receipt_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
) -> Result<i64, PurchasingError> {
    sqlx::query_scalar(
        r#"
        INSERT INTO goods_receipt_sequences (
            company_id,
            next_number
        )
        VALUES ($1, 2)
        ON CONFLICT (company_id)
        DO UPDATE SET next_number =
            goods_receipt_sequences.next_number + 1
        RETURNING next_number - 1
        "#,
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(PurchasingError::Database)
}

pub async fn create_purchase_order(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    request: CreatePurchaseOrderRequest,
) -> Result<PurchaseOrder, PurchasingError> {
    validate_items(&request.items)?;

    let tax = request.tax_total.unwrap_or(Decimal::ZERO);
    let discount = request.discount_total.unwrap_or(Decimal::ZERO);

    if tax < Decimal::ZERO || discount < Decimal::ZERO {
        return Err(PurchasingError::InvalidPrice);
    }

    validate_supplier(db, company_id, request.supplier_id).await?;

    let mut tx = db.begin().await.map_err(PurchasingError::Database)?;

    let order_number = allocate_order_number(&mut tx, company_id).await?;

    let order_date = request
        .order_date
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    if request
        .expected_delivery_date
        .is_some_and(|date| date < order_date)
    {
        return Err(PurchasingError::InvalidDate);
    }

    let order_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO purchase_orders (
            company_id,
            supplier_id,
            order_number,
            status,
            order_date,
            expected_delivery_date,
            tax_total,
            discount_total,
            created_by,
            notes
        )
        VALUES ($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(request.supplier_id)
    .bind(order_number)
    .bind(order_date)
    .bind(request.expected_delivery_date)
    .bind(tax)
    .bind(discount)
    .bind(user_id)
    .bind(normalize_notes(request.notes))
    .fetch_one(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    let subtotal = insert_order_items(&mut tx, company_id, order_id, &request.items).await?;

    if discount > subtotal + tax {
        return Err(PurchasingError::InvalidDiscount);
    }

    let total = subtotal + tax - discount;

    sqlx::query(
        r#"
        UPDATE purchase_orders
        SET subtotal = $1,
            total_amount = $2,
            updated_at = NOW()
        WHERE id = $3
          AND company_id = $4
        "#,
    )
    .bind(subtotal)
    .bind(total)
    .bind(order_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    tx.commit().await.map_err(PurchasingError::Database)?;

    get_purchase_order(db, company_id, order_id).await
}

pub async fn update_purchase_order(
    db: &PgPool,
    company_id: Uuid,
    order_id: i64,
    request: UpdatePurchaseOrderRequest,
) -> Result<PurchaseOrder, PurchasingError> {
    validate_items(&request.items)?;

    let tax = request.tax_total.unwrap_or(Decimal::ZERO);
    let discount = request.discount_total.unwrap_or(Decimal::ZERO);

    if tax < Decimal::ZERO || discount < Decimal::ZERO {
        return Err(PurchasingError::InvalidPrice);
    }

    validate_supplier(db, company_id, request.supplier_id).await?;

    let mut tx = db.begin().await.map_err(PurchasingError::Database)?;

    let order = sqlx::query(
        r#"
        SELECT status, version, order_date
        FROM purchase_orders
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?
    .ok_or(PurchasingError::NotFound)?;

    let status: String = order.try_get("status").map_err(PurchasingError::Database)?;
    let version: i32 = order
        .try_get("version")
        .map_err(PurchasingError::Database)?;

    if status != "draft" {
        return Err(PurchasingError::OrderNotEditable);
    }

    if version != request.expected_version {
        return Err(PurchasingError::VersionConflict);
    }

    let order_date = request.order_date.unwrap_or(
        order
            .try_get("order_date")
            .map_err(PurchasingError::Database)?,
    );

    if request
        .expected_delivery_date
        .is_some_and(|date| date < order_date)
    {
        return Err(PurchasingError::InvalidDate);
    }

    sqlx::query(
        "DELETE FROM purchase_order_items WHERE purchase_order_id = $1 AND company_id = $2",
    )
    .bind(order_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    let subtotal = insert_order_items(&mut tx, company_id, order_id, &request.items).await?;

    if discount > subtotal + tax {
        return Err(PurchasingError::InvalidDiscount);
    }

    let total = subtotal + tax - discount;

    sqlx::query(
        r#"
        UPDATE purchase_orders
        SET supplier_id = $1,
            order_date = $2,
            expected_delivery_date = $3,
            tax_total = $4,
            discount_total = $5,
            subtotal = $6,
            total_amount = $7,
            notes = $8,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $9 AND company_id = $10
        "#,
    )
    .bind(request.supplier_id)
    .bind(order_date)
    .bind(request.expected_delivery_date)
    .bind(tax)
    .bind(discount)
    .bind(subtotal)
    .bind(total)
    .bind(normalize_notes(request.notes))
    .bind(order_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    tx.commit().await.map_err(PurchasingError::Database)?;

    get_purchase_order(db, company_id, order_id).await
}

pub async fn submit_purchase_order(
    db: &PgPool,
    company_id: Uuid,
    order_id: i64,
) -> Result<(), PurchasingError> {
    let result = sqlx::query(
        r#"
        UPDATE purchase_orders
        SET status = 'ordered',
            submitted_at = NOW(),
            updated_at = NOW(),
            version = version + 1
        WHERE id = $1
          AND company_id = $2
          AND status = 'draft'
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .execute(db)
    .await
    .map_err(PurchasingError::Database)?;

    if result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM purchase_orders WHERE id=$1 AND company_id=$2)",
        )
        .bind(order_id)
        .bind(company_id)
        .fetch_one(db)
        .await
        .map_err(PurchasingError::Database)?;

        return Err(if exists {
            PurchasingError::OrderNotEditable
        } else {
            PurchasingError::NotFound
        });
    }

    Ok(())
}

pub async fn cancel_purchase_order(
    db: &PgPool,
    company_id: Uuid,
    order_id: i64,
) -> Result<(), PurchasingError> {
    let result = sqlx::query(
        r#"
        UPDATE purchase_orders
        SET status = 'cancelled',
            cancelled_at = NOW(),
            updated_at = NOW(),
            version = version + 1
        WHERE id = $1
          AND company_id = $2
          AND status IN ('draft', 'ordered')
          AND NOT EXISTS (
              SELECT 1
              FROM goods_receipts gr
              WHERE gr.purchase_order_id = purchase_orders.id
                AND gr.company_id = purchase_orders.company_id
          )
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .execute(db)
    .await
    .map_err(PurchasingError::Database)?;

    if result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM purchase_orders WHERE id=$1 AND company_id=$2)",
        )
        .bind(order_id)
        .bind(company_id)
        .fetch_one(db)
        .await
        .map_err(PurchasingError::Database)?;

        return Err(if exists {
            PurchasingError::OrderNotEditable
        } else {
            PurchasingError::NotFound
        });
    }

    Ok(())
}

pub async fn get_purchase_order(
    db: &PgPool,
    company_id: Uuid,
    order_id: i64,
) -> Result<PurchaseOrder, PurchasingError> {
    let row = sqlx::query(
        r#"
        SELECT
            po.id,
            po.order_number,
            po.supplier_id,
            s.name AS supplier_name,
            po.status,
            po.order_date,
            po.expected_delivery_date,
            po.subtotal,
            po.tax_total,
            po.discount_total,
            po.total_amount,
            po.notes,
            po.created_at,
            po.version
        FROM purchase_orders po
        JOIN suppliers s
          ON s.id = po.supplier_id
         AND s.company_id = po.company_id
        WHERE po.id = $1
          AND po.company_id = $2
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(PurchasingError::Database)?
    .ok_or(PurchasingError::NotFound)?;

    let item_rows = sqlx::query(
        r#"
        SELECT
            id,
            variant_id,
            product_name,
            variant_name,
            sku,
            unit_code,
            unit_name,
            ordered_quantity,
            received_quantity,
            ordered_quantity - received_quantity AS outstanding_quantity,
            unit_cost,
            line_total
        FROM purchase_order_items
        WHERE purchase_order_id = $1
          AND company_id = $2
        ORDER BY id
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(PurchasingError::Database)?;

    let mut items = Vec::new();

    for row in item_rows {
        items.push(PurchaseOrderItem {
            id: row.try_get("id").map_err(PurchasingError::Database)?,
            variant_id: row
                .try_get("variant_id")
                .map_err(PurchasingError::Database)?,
            product_name: row
                .try_get("product_name")
                .map_err(PurchasingError::Database)?,
            variant_name: row
                .try_get("variant_name")
                .map_err(PurchasingError::Database)?,
            sku: row.try_get("sku").map_err(PurchasingError::Database)?,
            unit_code: row
                .try_get("unit_code")
                .map_err(PurchasingError::Database)?,
            unit_name: row
                .try_get("unit_name")
                .map_err(PurchasingError::Database)?,
            ordered_quantity: row
                .try_get("ordered_quantity")
                .map_err(PurchasingError::Database)?,
            received_quantity: row
                .try_get("received_quantity")
                .map_err(PurchasingError::Database)?,
            outstanding_quantity: row
                .try_get("outstanding_quantity")
                .map_err(PurchasingError::Database)?,
            unit_cost: row
                .try_get("unit_cost")
                .map_err(PurchasingError::Database)?,
            line_total: row
                .try_get("line_total")
                .map_err(PurchasingError::Database)?,
        });
    }

    Ok(PurchaseOrder {
        id: row.try_get("id").map_err(PurchasingError::Database)?,
        order_number: row
            .try_get("order_number")
            .map_err(PurchasingError::Database)?,
        supplier_id: row
            .try_get("supplier_id")
            .map_err(PurchasingError::Database)?,
        supplier_name: row
            .try_get("supplier_name")
            .map_err(PurchasingError::Database)?,
        status: row.try_get("status").map_err(PurchasingError::Database)?,
        order_date: row
            .try_get("order_date")
            .map_err(PurchasingError::Database)?,
        expected_delivery_date: row
            .try_get("expected_delivery_date")
            .map_err(PurchasingError::Database)?,
        subtotal: row.try_get("subtotal").map_err(PurchasingError::Database)?,
        tax_total: row
            .try_get("tax_total")
            .map_err(PurchasingError::Database)?,
        discount_total: row
            .try_get("discount_total")
            .map_err(PurchasingError::Database)?,
        total_amount: row
            .try_get("total_amount")
            .map_err(PurchasingError::Database)?,
        notes: row.try_get("notes").map_err(PurchasingError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(PurchasingError::Database)?,
        version: row.try_get("version").map_err(PurchasingError::Database)?,
        items,
    })
}

pub async fn list_purchase_orders(
    db: &PgPool,
    company_id: Uuid,
    query: PurchaseOrderQuery,
) -> Result<Vec<PurchaseOrder>, PurchasingError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    if let Some(status) = query.status.as_deref() {
        let valid_statuses = [
            "draft",
            "ordered",
            "partially_received",
            "received",
            "cancelled",
        ];

        if !valid_statuses.contains(&status) {
            return Err(PurchasingError::InvalidStatus);
        }
    }

    let ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM purchase_orders
        WHERE company_id = $1
          AND ($2::BIGINT IS NULL OR supplier_id = $2)
          AND ($3::VARCHAR IS NULL OR status = $3)
        ORDER BY created_at DESC, id DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(company_id)
    .bind(query.supplier_id)
    .bind(query.status)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(PurchasingError::Database)?;

    let mut orders = Vec::new();

    for id in ids {
        orders.push(get_purchase_order(db, company_id, id).await?);
    }

    Ok(orders)
}

pub async fn receive_goods(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    order_id: i64,
    request: ReceiveGoodsRequest,
) -> Result<ReceiveGoodsOutcome, PurchasingError> {
    if request.items.is_empty() {
        return Err(PurchasingError::EmptyOrder);
    }

    for item in &request.items {
        if item.quantity_received <= Decimal::ZERO {
            return Err(PurchasingError::InvalidQuantity);
        }

        if item.unit_cost.is_some_and(|cost| cost < Decimal::ZERO) {
            return Err(PurchasingError::InvalidPrice);
        }
    }

    validate_receipt_items(&request.items)?;

    let mut tx = db.begin().await.map_err(PurchasingError::Database)?;

    let lock_key = format!("{}:{}", company_id, request.idempotency_key);

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

    // Lock the order so simultaneous receipts cannot over-receive it.
    let order = sqlx::query(
        r#"
        SELECT id, supplier_id, status
        FROM purchase_orders
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?
    .ok_or(PurchasingError::NotFound)?;

    let status: String = order.try_get("status").map_err(PurchasingError::Database)?;
    let supplier_id: i64 = order
        .try_get("supplier_id")
        .map_err(PurchasingError::Database)?;

    // Idempotency: a previously recorded receipt must never add stock again.
    let existing = sqlx::query(
        r#"
        SELECT id, receipt_number, location_id, notes, received_by, received_at
        FROM goods_receipts
        WHERE company_id = $1
          AND idempotency_key = $2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    if let Some(existing) = existing {
        let receipt_order_id: i64 = sqlx::query_scalar(
            "SELECT purchase_order_id FROM goods_receipts WHERE id = $1 AND company_id = $2",
        )
        .bind(
            existing
                .try_get::<i64, _>("id")
                .map_err(PurchasingError::Database)?,
        )
        .bind(company_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        let existing_id: i64 = existing.try_get("id").map_err(PurchasingError::Database)?;
        let existing_location: i64 = existing
            .try_get("location_id")
            .map_err(PurchasingError::Database)?;
        let existing_notes: Option<String> = existing
            .try_get("notes")
            .map_err(PurchasingError::Database)?;

        if receipt_order_id != order_id
            || existing_location != request.location_id
            || existing_notes != normalize_notes(request.notes.clone())
        {
            return Err(PurchasingError::IdempotencyConflict);
        }

        let existing_lines = sqlx::query(
            r#"
            SELECT purchase_order_item_id, quantity_received, unit_cost
            FROM goods_receipt_items
            WHERE goods_receipt_id = $1 AND company_id = $2
            ORDER BY purchase_order_item_id
            "#,
        )
        .bind(existing_id)
        .bind(company_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        let mut expected = request.items.iter().collect::<Vec<_>>();
        expected.sort_by_key(|item| item.purchase_order_item_id);

        if existing_lines.len() != expected.len() {
            return Err(PurchasingError::IdempotencyConflict);
        }

        for (stored, submitted) in existing_lines.iter().zip(expected.iter()) {
            let stored_id: i64 = stored
                .try_get("purchase_order_item_id")
                .map_err(PurchasingError::Database)?;
            let stored_qty: Decimal = stored
                .try_get("quantity_received")
                .map_err(PurchasingError::Database)?;
            let stored_cost: Decimal = stored
                .try_get("unit_cost")
                .map_err(PurchasingError::Database)?;

            // The stored cost is the effective cost. A missing submitted cost
            // means the purchase-order cost, which is checked below.
            let po_cost: Decimal = sqlx::query_scalar(
                "SELECT unit_cost FROM purchase_order_items WHERE id = $1 AND company_id = $2",
            )
            .bind(submitted.purchase_order_item_id)
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(PurchasingError::Database)?
            .ok_or(PurchasingError::ItemNotFound)?;

            if stored_id != submitted.purchase_order_item_id
                || stored_qty != submitted.quantity_received
                || stored_cost != submitted.unit_cost.unwrap_or(po_cost)
            {
                return Err(PurchasingError::IdempotencyConflict);
            }
        }

        let response = load_receipt(&mut tx, company_id, existing_id, order_id).await?;
        tx.commit().await.map_err(PurchasingError::Database)?;
        return Ok(ReceiveGoodsOutcome {
            receipt: response,
            replayed: true,
        });
    }

    if !matches!(status.as_str(), "ordered" | "partially_received") {
        return Err(PurchasingError::OrderNotReceivable);
    }

    let location_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM inventory_locations
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
    .map_err(PurchasingError::Database)?;

    if !location_exists {
        return Err(PurchasingError::LocationNotFound);
    }

    let receipt_number = allocate_receipt_number(&mut tx, company_id).await?;

    let receipt_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO goods_receipts (
            company_id,
            purchase_order_id,
            location_id,
            receipt_number,
            idempotency_key,
            notes,
            received_by
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(order_id)
    .bind(request.location_id)
    .bind(receipt_number)
    .bind(request.idempotency_key)
    .bind(normalize_notes(request.notes.clone()))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    for item in &request.items {
        let po_item = sqlx::query(
            r#"
            SELECT
                poi.variant_id,
                poi.ordered_quantity,
                poi.received_quantity,
                poi.unit_cost,
                poi.sku
            FROM purchase_order_items poi
            WHERE poi.id = $1
              AND poi.purchase_order_id = $2
              AND poi.company_id = $3
            FOR UPDATE
            "#,
        )
        .bind(item.purchase_order_item_id)
        .bind(order_id)
        .bind(company_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?
        .ok_or(PurchasingError::ItemNotFound)?;

        let variant_id: i64 = po_item
            .try_get("variant_id")
            .map_err(PurchasingError::Database)?;
        let ordered: Decimal = po_item
            .try_get("ordered_quantity")
            .map_err(PurchasingError::Database)?;
        let received: Decimal = po_item
            .try_get("received_quantity")
            .map_err(PurchasingError::Database)?;
        let po_cost: Decimal = po_item
            .try_get("unit_cost")
            .map_err(PurchasingError::Database)?;
        let sku: Option<String> = po_item.try_get("sku").map_err(PurchasingError::Database)?;

        if item.quantity_received > ordered - received {
            return Err(PurchasingError::ExcessiveReceipt);
        }

        let actual_cost = item.unit_cost.unwrap_or(po_cost);

        sqlx::query(
            r#"
            INSERT INTO goods_receipt_items (
                company_id,
                goods_receipt_id,
                purchase_order_item_id,
                quantity_received,
                unit_cost
            )
            VALUES ($1,$2,$3,$4,$5)
            "#,
        )
        .bind(company_id)
        .bind(receipt_id)
        .bind(item.purchase_order_item_id)
        .bind(item.quantity_received)
        .bind(actual_cost)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        sqlx::query(
            r#"
            UPDATE purchase_order_items
            SET received_quantity = received_quantity + $1,
                updated_at = NOW(),
                version = version + 1
            WHERE id = $2 AND company_id = $3
            "#,
        )
        .bind(item.quantity_received)
        .bind(item.purchase_order_item_id)
        .bind(company_id)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        // Ensure balance exists, then lock it.
        sqlx::query(
            r#"
            INSERT INTO stock_balances (
                company_id, location_id, variant_id
            )
            VALUES ($1,$2,$3)
            ON CONFLICT (location_id, variant_id) DO NOTHING
            "#,
        )
        .bind(company_id)
        .bind(request.location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        let balance = sqlx::query(
            r#"
            SELECT quantity_on_hand, reserved_quantity
            FROM stock_balances
            WHERE company_id = $1
              AND location_id = $2
              AND variant_id = $3
            FOR UPDATE
            "#,
        )
        .bind(company_id)
        .bind(request.location_id)
        .bind(variant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        let old_quantity: Decimal = balance
            .try_get("quantity_on_hand")
            .map_err(PurchasingError::Database)?;
        let reserved: Decimal = balance
            .try_get("reserved_quantity")
            .map_err(PurchasingError::Database)?;

        let new_quantity = old_quantity + item.quantity_received;

        sqlx::query(
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
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        sqlx::query(
            r#"
            INSERT INTO stock_movements (
                company_id,
                location_id,
                variant_id,
                movement_type,
                quantity_delta,
                reference_type,
                reference_id,
                idempotency_key,
                notes,
                created_by
            )
            VALUES (
                $1,$2,$3,'purchase_receipt',$4,
                'goods_receipt',$5,$6,$7,$8
            )
            "#,
        )
        .bind(company_id)
        .bind(request.location_id)
        .bind(variant_id)
        .bind(item.quantity_received)
        .bind(receipt_id.to_string())
        .bind(Uuid::new_v4())
        .bind(normalize_notes(request.notes.clone()))
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        sqlx::query(
            r#"
            INSERT INTO supplier_products (
                company_id,
                supplier_id,
                variant_id,
                supplier_sku,
                last_cost_price
            )
            VALUES ($1,$2,$3,$4,$5)
            ON CONFLICT (supplier_id, variant_id)
            DO UPDATE SET
                company_id = EXCLUDED.company_id,
                supplier_sku = COALESCE(
                    EXCLUDED.supplier_sku,
                    supplier_products.supplier_sku
                ),
                last_cost_price = EXCLUDED.last_cost_price,
                updated_at = NOW()
            "#,
        )
        .bind(company_id)
        .bind(supplier_id)
        .bind(variant_id)
        .bind(sku)
        .bind(actual_cost)
        .execute(&mut *tx)
        .await
        .map_err(PurchasingError::Database)?;

        // Keep the reserved quantity untouched.
        let _ = reserved;
    }

    let all_received = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT NOT EXISTS (
            SELECT 1
            FROM purchase_order_items
            WHERE purchase_order_id = $1
              AND company_id = $2
              AND received_quantity < ordered_quantity
        )
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    let new_status = if all_received {
        "received"
    } else {
        "partially_received"
    };

    sqlx::query(
        r#"
        UPDATE purchase_orders
        SET status = $1,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $2 AND company_id = $3
        "#,
    )
    .bind(new_status)
    .bind(order_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(PurchasingError::Database)?;

    let response = load_receipt(&mut tx, company_id, receipt_id, order_id).await?;

    tx.commit().await.map_err(PurchasingError::Database)?;

    Ok(ReceiveGoodsOutcome {
        receipt: response,
        replayed: false,
    })
}

async fn load_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    receipt_id: i64,
    order_id: i64,
) -> Result<GoodsReceipt, PurchasingError> {
    let row = sqlx::query(
        r#"
        SELECT id, receipt_number, location_id, received_by, notes, received_at
        FROM goods_receipts
        WHERE id = $1
          AND company_id = $2
          AND purchase_order_id = $3
        "#,
    )
    .bind(receipt_id)
    .bind(company_id)
    .bind(order_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(PurchasingError::Database)?;

    let rows = sqlx::query(
        r#"
        SELECT id, purchase_order_item_id, quantity_received, unit_cost
        FROM goods_receipt_items
        WHERE goods_receipt_id = $1
          AND company_id = $2
        ORDER BY id
        "#,
    )
    .bind(receipt_id)
    .bind(company_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(PurchasingError::Database)?;

    let mut items = Vec::new();

    for item in rows {
        items.push(GoodsReceiptItem {
            id: item.try_get("id").map_err(PurchasingError::Database)?,
            purchase_order_item_id: item
                .try_get("purchase_order_item_id")
                .map_err(PurchasingError::Database)?,
            quantity_received: item
                .try_get("quantity_received")
                .map_err(PurchasingError::Database)?,
            unit_cost: item
                .try_get("unit_cost")
                .map_err(PurchasingError::Database)?,
        });
    }

    Ok(GoodsReceipt {
        id: row.try_get("id").map_err(PurchasingError::Database)?,
        receipt_number: row
            .try_get("receipt_number")
            .map_err(PurchasingError::Database)?,
        purchase_order_id: order_id,
        location_id: row
            .try_get("location_id")
            .map_err(PurchasingError::Database)?,
        received_by: row
            .try_get("received_by")
            .map_err(PurchasingError::Database)?,
        notes: row.try_get("notes").map_err(PurchasingError::Database)?,
        received_at: row
            .try_get("received_at")
            .map_err(PurchasingError::Database)?,
        items,
    })
}

pub async fn get_receipts(
    db: &PgPool,
    company_id: Uuid,
    order_id: i64,
) -> Result<Vec<GoodsReceipt>, PurchasingError> {
    let ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM goods_receipts
        WHERE purchase_order_id = $1
          AND company_id = $2
        ORDER BY received_at DESC
        "#,
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(PurchasingError::Database)?;

    let mut receipts = Vec::new();

    for id in ids {
        let mut tx = db.begin().await.map_err(PurchasingError::Database)?;
        receipts.push(load_receipt(&mut tx, company_id, id, order_id).await?);
        tx.commit().await.map_err(PurchasingError::Database)?;
    }

    Ok(receipts)
}
