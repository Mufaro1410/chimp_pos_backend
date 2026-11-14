use std::collections::HashSet;

use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

use crate::models::sales::*;

#[derive(Debug)]
pub enum SalesError {
    Database(Error),
    NotFound,
    InvalidItems,
    EmptySale,
    InvalidQuantity,
    InvalidPrice,
    InvalidDiscount,
    InvalidTax,
    InvalidPayment,
    InvalidPaymentMethod,
    InvalidStatus,
    InvalidLocation,
    InvalidVariant,
    DecimalQuantityNotAllowed,
    VersionConflict,
    SaleNotEditable,
    SaleNotCheckoutable,
    InsufficientStock,
    DiscountPermissionRequired,
    CreditPermissionRequired,
    IdempotencyConflict,
    CashRegisterRequired,
    CashSessionNotOpen,
}

#[derive(Debug)]
pub struct CheckoutOutcome {
    pub sale: Sale,
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

fn validate_items(items: &[SaleItemRequest]) -> Result<(), SalesError> {
    if items.is_empty() {
        return Err(SalesError::EmptySale);
    }

    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item.variant_id) {
            return Err(SalesError::InvalidItems);
        }

        if item.quantity <= Decimal::ZERO || !validate_decimal(item.quantity) {
            return Err(SalesError::InvalidQuantity);
        }

        if item.unit_price < Decimal::ZERO || !validate_decimal(item.unit_price) {
            return Err(SalesError::InvalidPrice);
        }

        let discount = item.discount_amount.unwrap_or(Decimal::ZERO);
        let tax_rate = item.tax_rate.unwrap_or(Decimal::ZERO);

        if discount < Decimal::ZERO || !validate_decimal(discount) {
            return Err(SalesError::InvalidDiscount);
        }

        if tax_rate < Decimal::ZERO || tax_rate > Decimal::from(100) || !validate_decimal(tax_rate)
        {
            return Err(SalesError::InvalidTax);
        }
    }

    Ok(())
}

async fn has_permission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    user_id: Uuid,
    permission: &str,
) -> Result<bool, SalesError> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM user_roles ur
            JOIN roles r
              ON r.id = ur.role_id
             AND r.company_id = ur.company_id
            JOIN role_permissions rp
              ON rp.role_id = r.id
            JOIN permissions p
              ON p.id = rp.permission_id
            WHERE ur.user_id = $1
              AND ur.company_id = $2
              AND r.is_active = TRUE
              AND (ur.expires_at IS NULL OR ur.expires_at > NOW())
              AND p.permission_key = $3
        )
        "#,
    )
    .bind(user_id)
    .bind(company_id)
    .bind(permission)
    .fetch_one(&mut **tx)
    .await
    .map_err(SalesError::Database)
}

/// Lock the customer first, then read the derived receivable with a new SQL
/// statement snapshot. Call before locking a sale/document in the same tx.
pub async fn customer_credit_permits(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    customer_id: i64,
    additional_debt: Decimal,
) -> Result<bool, SalesError> {
    let policy = sqlx::query(
        "SELECT credit_enabled,credit_limit FROM customers WHERE id=$1 AND company_id=$2 AND is_active=TRUE FOR UPDATE",
    )
    .bind(customer_id)
    .bind(company_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(SalesError::Database)?
    .ok_or(SalesError::InvalidPayment)?;
    let enabled: bool = policy
        .try_get("credit_enabled")
        .map_err(SalesError::Database)?;
    let limit: Decimal = policy
        .try_get("credit_limit")
        .map_err(SalesError::Database)?;
    let balance: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(balance,0) FROM customer_account_balances WHERE customer_id=$1 AND company_id=$2",
    )
    .bind(customer_id)
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(SalesError::Database)?;
    Ok(enabled && balance + additional_debt <= limit)
}

async fn validate_location(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    location_id: i64,
) -> Result<(), SalesError> {
    let valid = sqlx::query_scalar::<_, bool>(
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
    .bind(location_id)
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(SalesError::Database)?;

    if !valid {
        return Err(SalesError::InvalidLocation);
    }

    Ok(())
}

pub async fn get_receipt(db: &PgPool, company_id: Uuid, sale_id: i64) -> Result<Sale, SalesError> {
    let sale = load_sale(db, company_id, sale_id).await?;

    if sale.status != "completed" {
        return Err(SalesError::SaleNotCheckoutable);
    }

    Ok(sale)
}

async fn allocate_sale_number(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
) -> Result<i64, SalesError> {
    sqlx::query_scalar(
        r#"
        INSERT INTO sale_sequences (company_id, next_number)
        VALUES ($1, 2)
        ON CONFLICT (company_id)
        DO UPDATE SET next_number =
            sale_sequences.next_number + 1
        RETURNING next_number - 1
        "#,
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(SalesError::Database)
}

async fn insert_sale_items(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    sale_id: i64,
    user_id: Uuid,
    items: &[SaleItemRequest],
) -> Result<(Decimal, Decimal, Decimal, Decimal), SalesError> {
    validate_items(items)?;

    let mut subtotal = Decimal::ZERO;
    let mut discount_total = Decimal::ZERO;
    let mut tax_total = Decimal::ZERO;
    let mut total = Decimal::ZERO;

    let mut needs_discount_permission = false;

    for item in items {
        let row = sqlx::query(
            r#"
            SELECT
                pv.id AS variant_id,
                pv.name AS variant_name,
                pv.sku,
                pv.barcode,
                pv.selling_price,
                p.name AS product_name,
                p.track_inventory,
                u.code AS unit_code,
                u.name AS unit_name,
                u.allows_decimal
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
              AND p.archived_at IS NULL
            "#,
        )
        .bind(item.variant_id)
        .bind(company_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(SalesError::Database)?
        .ok_or(SalesError::InvalidVariant)?;

        let product_name: String = row.try_get("product_name").map_err(SalesError::Database)?;
        let variant_name: String = row.try_get("variant_name").map_err(SalesError::Database)?;
        let sku: Option<String> = row.try_get("sku").map_err(SalesError::Database)?;
        let barcode: Option<String> = row.try_get("barcode").map_err(SalesError::Database)?;
        let unit_code: String = row.try_get("unit_code").map_err(SalesError::Database)?;
        let unit_name: String = row.try_get("unit_name").map_err(SalesError::Database)?;
        let allows_decimal: bool = row
            .try_get("allows_decimal")
            .map_err(SalesError::Database)?;
        let selling_price: Decimal = row.try_get("selling_price").map_err(SalesError::Database)?;

        if !allows_decimal && item.quantity.fract() != Decimal::ZERO {
            return Err(SalesError::DecimalQuantityNotAllowed);
        }

        if item.unit_price > selling_price {
            return Err(SalesError::InvalidPrice);
        }

        let discount = item.discount_amount.unwrap_or(Decimal::ZERO);
        let tax_rate = item.tax_rate.unwrap_or(Decimal::ZERO);

        if item.unit_price < selling_price || discount > Decimal::ZERO {
            needs_discount_permission = true;
        }

        let line_subtotal = (item.quantity * item.unit_price).round_dp(4);

        if discount > line_subtotal {
            return Err(SalesError::InvalidDiscount);
        }

        let taxable_amount = line_subtotal - discount;
        let tax_amount = (taxable_amount * tax_rate / Decimal::from(100)).round_dp(4);

        let line_total = taxable_amount + tax_amount;

        sqlx::query(
            r#"
            INSERT INTO sale_items (
                company_id,
                sale_id,
                variant_id,
                product_name,
                variant_name,
                sku,
                barcode,
                unit_code,
                unit_name,
                quantity,
                unit_price,
                discount_amount,
                tax_rate,
                tax_amount,
                line_subtotal,
                line_total
            )
            VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,
                $10,$11,$12,$13,$14,$15,$16
            )
            "#,
        )
        .bind(company_id)
        .bind(sale_id)
        .bind(item.variant_id)
        .bind(product_name)
        .bind(variant_name)
        .bind(sku)
        .bind(barcode)
        .bind(unit_code)
        .bind(unit_name)
        .bind(item.quantity)
        .bind(item.unit_price)
        .bind(discount)
        .bind(tax_rate)
        .bind(tax_amount)
        .bind(line_subtotal)
        .bind(line_total)
        .execute(&mut **tx)
        .await
        .map_err(SalesError::Database)?;

        subtotal += line_subtotal;
        discount_total += discount;
        tax_total += tax_amount;
        total += line_total;
    }

    if needs_discount_permission
        && !has_permission(tx, company_id, user_id, "sales.discount").await?
    {
        return Err(SalesError::DiscountPermissionRequired);
    }

    Ok((
        subtotal.round_dp(4),
        discount_total.round_dp(4),
        tax_total.round_dp(4),
        total.round_dp(4),
    ))
}

async fn load_sale(db: &PgPool, company_id: Uuid, sale_id: i64) -> Result<Sale, SalesError> {
    let row = sqlx::query(
        r#"
        SELECT
            id,
            sale_number,
            location_id,
            customer_id,
            status,
            btrim(currency_code)::text AS currency_code,
            subtotal,
            discount_total,
            tax_total,
            total_amount,
            notes,
            created_by,
            completed_by,
            completed_at,
            created_at,
            version
        FROM sales
        WHERE id = $1
          AND company_id = $2
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(SalesError::Database)?
    .ok_or(SalesError::NotFound)?;

    let item_rows = sqlx::query(
        r#"
        SELECT
            id,
            variant_id,
            product_name,
            variant_name,
            sku,
            barcode,
            unit_code,
            unit_name,
            quantity,
            unit_price,
            discount_amount,
            tax_rate,
            tax_amount,
            line_subtotal,
            line_total
        FROM sale_items
        WHERE sale_id = $1
          AND company_id = $2
        ORDER BY id
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(SalesError::Database)?;

    let mut items = Vec::new();

    for row in item_rows {
        items.push(SaleItem {
            id: row.try_get("id").map_err(SalesError::Database)?,
            variant_id: row.try_get("variant_id").map_err(SalesError::Database)?,
            product_name: row.try_get("product_name").map_err(SalesError::Database)?,
            variant_name: row.try_get("variant_name").map_err(SalesError::Database)?,
            sku: row.try_get("sku").map_err(SalesError::Database)?,
            barcode: row.try_get("barcode").map_err(SalesError::Database)?,
            unit_code: row.try_get("unit_code").map_err(SalesError::Database)?,
            unit_name: row.try_get("unit_name").map_err(SalesError::Database)?,
            quantity: row.try_get("quantity").map_err(SalesError::Database)?,
            unit_price: row.try_get("unit_price").map_err(SalesError::Database)?,
            discount_amount: row
                .try_get("discount_amount")
                .map_err(SalesError::Database)?,
            tax_rate: row.try_get("tax_rate").map_err(SalesError::Database)?,
            tax_amount: row.try_get("tax_amount").map_err(SalesError::Database)?,
            line_subtotal: row.try_get("line_subtotal").map_err(SalesError::Database)?,
            line_total: row.try_get("line_total").map_err(SalesError::Database)?,
        });
    }

    let payment_rows = sqlx::query(
        r#"
        SELECT
            id,
            payment_method,
            btrim(currency_code)::text AS currency_code,
            amount,
            tendered_amount,
            change_given,
            reference,
            notes,
            received_by,
            created_at
        FROM sale_payments
        WHERE sale_id = $1
          AND company_id = $2
        ORDER BY id
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(SalesError::Database)?;

    let mut payments = Vec::new();

    for row in payment_rows {
        payments.push(SalePayment {
            id: row.try_get("id").map_err(SalesError::Database)?,
            payment_method: row
                .try_get("payment_method")
                .map_err(SalesError::Database)?,
            currency_code: row.try_get("currency_code").map_err(SalesError::Database)?,
            amount: row.try_get("amount").map_err(SalesError::Database)?,
            tendered_amount: row
                .try_get("tendered_amount")
                .map_err(SalesError::Database)?,
            change_given: row.try_get("change_given").map_err(SalesError::Database)?,
            reference: row.try_get("reference").map_err(SalesError::Database)?,
            notes: row.try_get("notes").map_err(SalesError::Database)?,
            received_by: row.try_get("received_by").map_err(SalesError::Database)?,
            created_at: row.try_get("created_at").map_err(SalesError::Database)?,
        });
    }

    Ok(Sale {
        id: row.try_get("id").map_err(SalesError::Database)?,
        sale_number: row.try_get("sale_number").map_err(SalesError::Database)?,
        location_id: row.try_get("location_id").map_err(SalesError::Database)?,
        customer_id: row.try_get("customer_id").map_err(SalesError::Database)?,
        status: row.try_get("status").map_err(SalesError::Database)?,
        currency_code: row.try_get("currency_code").map_err(SalesError::Database)?,
        subtotal: row.try_get("subtotal").map_err(SalesError::Database)?,
        discount_total: row
            .try_get("discount_total")
            .map_err(SalesError::Database)?,
        tax_total: row.try_get("tax_total").map_err(SalesError::Database)?,
        total_amount: row.try_get("total_amount").map_err(SalesError::Database)?,
        notes: row.try_get("notes").map_err(SalesError::Database)?,
        created_by: row.try_get("created_by").map_err(SalesError::Database)?,
        completed_by: row.try_get("completed_by").map_err(SalesError::Database)?,
        completed_at: row.try_get("completed_at").map_err(SalesError::Database)?,
        created_at: row.try_get("created_at").map_err(SalesError::Database)?,
        version: row.try_get("version").map_err(SalesError::Database)?,
        items,
        payments,
    })
}

pub async fn create_sale(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    request: CreateSaleRequest,
) -> Result<Sale, SalesError> {
    validate_items(&request.items)?;

    let mut tx = db.begin().await.map_err(SalesError::Database)?;

    validate_location(&mut tx, company_id, request.location_id).await?;

    let sale_number = allocate_sale_number(&mut tx, company_id).await?;

    let sale_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO sales (
            company_id,
            location_id,
            sale_number,
            status,
            created_by,
            notes,
            customer_id,
            currency_code
        )
        SELECT $1,$2,$3,'draft',$4,$5,$6,c.default_currency
        FROM companies c WHERE c.id=$1
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(request.location_id)
    .bind(sale_number)
    .bind(user_id)
    .bind(normalize_notes(request.notes))
    .bind(request.customer_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    let (subtotal, discount, tax, total) =
        insert_sale_items(&mut tx, company_id, sale_id, user_id, &request.items).await?;

    sqlx::query(
        r#"
        UPDATE sales
        SET subtotal = $1,
            discount_total = $2,
            tax_total = $3,
            total_amount = $4,
            updated_at = NOW()
        WHERE id = $5
          AND company_id = $6
        "#,
    )
    .bind(subtotal)
    .bind(discount)
    .bind(tax)
    .bind(total)
    .bind(sale_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    tx.commit().await.map_err(SalesError::Database)?;

    load_sale(db, company_id, sale_id).await
}

pub async fn update_sale(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    sale_id: i64,
    request: UpdateSaleRequest,
) -> Result<Sale, SalesError> {
    validate_items(&request.items)?;

    let mut tx = db.begin().await.map_err(SalesError::Database)?;

    let row = sqlx::query(
        r#"
        SELECT status, version
        FROM sales
        WHERE id = $1
          AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesError::Database)?
    .ok_or(SalesError::NotFound)?;

    let status: String = row.try_get("status").map_err(SalesError::Database)?;
    let version: i32 = row.try_get("version").map_err(SalesError::Database)?;

    if status != "draft" {
        return Err(SalesError::SaleNotEditable);
    }

    if version != request.expected_version {
        return Err(SalesError::VersionConflict);
    }

    validate_location(&mut tx, company_id, request.location_id).await?;

    sqlx::query("DELETE FROM sale_items WHERE sale_id = $1 AND company_id = $2")
        .bind(sale_id)
        .bind(company_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

    let (subtotal, discount, tax, total) =
        insert_sale_items(&mut tx, company_id, sale_id, user_id, &request.items).await?;

    sqlx::query(
        r#"
        UPDATE sales
        SET location_id = $1,
            subtotal = $2,
            discount_total = $3,
            tax_total = $4,
            total_amount = $5,
            notes = $6,
            customer_id = $7,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $8
          AND company_id = $9
        "#,
    )
    .bind(request.location_id)
    .bind(subtotal)
    .bind(discount)
    .bind(tax)
    .bind(total)
    .bind(normalize_notes(request.notes))
    .bind(request.customer_id)
    .bind(sale_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    tx.commit().await.map_err(SalesError::Database)?;

    load_sale(db, company_id, sale_id).await
}

pub async fn get_sale(db: &PgPool, company_id: Uuid, sale_id: i64) -> Result<Sale, SalesError> {
    load_sale(db, company_id, sale_id).await
}

pub async fn list_sales(
    db: &PgPool,
    company_id: Uuid,
    query: SaleQuery,
) -> Result<Vec<Sale>, SalesError> {
    if let Some(status) = query.status.as_deref() {
        if !["draft", "completed", "cancelled"].contains(&status) {
            return Err(SalesError::InvalidStatus);
        }
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM sales
        WHERE company_id = $1
          AND ($2::VARCHAR IS NULL OR status = $2)
          AND ($3::BIGINT IS NULL OR location_id = $3)
        ORDER BY created_at DESC, id DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(company_id)
    .bind(query.status)
    .bind(query.location_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(SalesError::Database)?;

    let mut sales = Vec::new();

    for id in ids {
        sales.push(load_sale(db, company_id, id).await?);
    }

    Ok(sales)
}

pub async fn cancel_sale(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    sale_id: i64,
) -> Result<(), SalesError> {
    let result = sqlx::query(
        r#"
        UPDATE sales
        SET status = 'cancelled',
            cancelled_by = $1,
            cancelled_at = NOW(),
            updated_at = NOW(),
            version = version + 1
        WHERE id = $2
          AND company_id = $3
          AND status = 'draft'
        "#,
    )
    .bind(user_id)
    .bind(sale_id)
    .bind(company_id)
    .execute(db)
    .await
    .map_err(SalesError::Database)?;

    if result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sales WHERE id=$1 AND company_id=$2)",
        )
        .bind(sale_id)
        .bind(company_id)
        .fetch_one(db)
        .await
        .map_err(SalesError::Database)?;

        return Err(if exists {
            SalesError::SaleNotEditable
        } else {
            SalesError::NotFound
        });
    }

    Ok(())
}

pub async fn checkout_sale(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    sale_id: i64,
    request: CheckoutRequest,
) -> Result<CheckoutOutcome, SalesError> {
    let mut tx = db.begin().await.map_err(SalesError::Database)?;

    // Serialize requests using the same company/idempotency key.
    let lock_key = format!("{}:{}", company_id, request.idempotency_key);

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

    // Global account lock order starts with the party, then the sale/document.
    // Read the draft's current party, lock it, and verify the relation after the
    // sale lock below so concurrent draft edits cannot change the locked party.
    let initially_locked_customer: Option<i64> =
        sqlx::query_scalar("SELECT customer_id FROM sales WHERE id=$1 AND company_id=$2")
            .bind(sale_id)
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(SalesError::Database)?
            .ok_or(SalesError::NotFound)?;
    if let Some(customer_id) = initially_locked_customer {
        let locked: Option<i64> =
            sqlx::query_scalar("SELECT id FROM customers WHERE id=$1 AND company_id=$2 FOR UPDATE")
                .bind(customer_id)
                .bind(company_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(SalesError::Database)?;
        if locked.is_none() {
            return Err(SalesError::InvalidPayment);
        }
    }

    let row = sqlx::query(
        r#"
        SELECT
            status,
            version,
            location_id,
            total_amount,
            btrim(currency_code)::text AS currency_code,
            customer_id,
            checkout_idempotency_key
        FROM sales
        WHERE id = $1
          AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesError::Database)?
    .ok_or(SalesError::NotFound)?;

    let status: String = row.try_get("status").map_err(SalesError::Database)?;
    let version: i32 = row.try_get("version").map_err(SalesError::Database)?;
    let location_id: i64 = row.try_get("location_id").map_err(SalesError::Database)?;
    let total: Decimal = row.try_get("total_amount").map_err(SalesError::Database)?;
    let currency_code: String = row.try_get("currency_code").map_err(SalesError::Database)?;
    let customer_id: Option<i64> = row.try_get("customer_id").map_err(SalesError::Database)?;
    if customer_id != initially_locked_customer {
        return Err(SalesError::SaleNotCheckoutable);
    }
    let stored_key: Option<Uuid> = row
        .try_get("checkout_idempotency_key")
        .map_err(SalesError::Database)?;

    // Check whether this key was already used by another sale.
    let key_sale: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM sales
        WHERE company_id = $1
          AND checkout_idempotency_key = $2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    if key_sale.is_some_and(|id| id != sale_id) {
        return Err(SalesError::IdempotencyConflict);
    }

    // A retry of the same completed checkout returns its original result.
    if status == "completed" && stored_key == Some(request.idempotency_key) {
        let existing = sqlx::query(
            r#"
            SELECT payment_method, amount, tendered_amount, reference, notes
            FROM sale_payments
            WHERE sale_id = $1
              AND company_id = $2
            ORDER BY id
            "#,
        )
        .bind(sale_id)
        .bind(company_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

        let stored_register_id: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT MAX(s.register_id)
            FROM sale_payments p
            JOIN cash_register_sessions s
            ON s.id = p.cash_session_id
            AND s.company_id = p.company_id
            WHERE p.company_id = $1
            AND p.sale_id = $2
            AND p.payment_method = 'cash'
            "#,
        )
        .bind(company_id)
        .bind(sale_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

        if stored_register_id != request.register_id {
            return Err(SalesError::IdempotencyConflict);
        }

        let mut submitted = request.payments.iter().collect::<Vec<_>>();
        submitted.sort_by(|a, b| {
            a.payment_method
                .cmp(&b.payment_method)
                .then(a.amount.cmp(&b.amount))
                .then(a.tendered_amount.cmp(&b.tendered_amount))
        });

        if existing.len() != submitted.len() {
            return Err(SalesError::IdempotencyConflict);
        }

        let mut stored = Vec::new();

        for payment in existing {
            stored.push((
                payment
                    .try_get::<String, _>("payment_method")
                    .map_err(SalesError::Database)?,
                payment
                    .try_get::<Decimal, _>("amount")
                    .map_err(SalesError::Database)?,
                payment
                    .try_get::<Decimal, _>("tendered_amount")
                    .map_err(SalesError::Database)?,
                payment
                    .try_get::<Option<String>, _>("reference")
                    .map_err(SalesError::Database)?,
                payment
                    .try_get::<Option<String>, _>("notes")
                    .map_err(SalesError::Database)?,
            ));
        }

        stored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

        for (stored, submitted) in stored.iter().zip(submitted.iter()) {
            if stored.0 != submitted.payment_method
                || stored.1 != submitted.amount
                || stored.2 != submitted.tendered_amount
                || stored.3 != normalize_notes(submitted.reference.clone())
                || stored.4 != normalize_notes(submitted.notes.clone())
            {
                return Err(SalesError::IdempotencyConflict);
            }
        }

        let sale = load_sale(db, company_id, sale_id).await?;
        tx.commit().await.map_err(SalesError::Database)?;

        return Ok(CheckoutOutcome {
            sale,
            replayed: true,
        });
    }

    if status != "draft" {
        return Err(SalesError::SaleNotCheckoutable);
    }

    if version != request.expected_version {
        return Err(SalesError::VersionConflict);
    }

    let item_rows = sqlx::query(
        r#"
        SELECT variant_id, quantity
        FROM sale_items
        WHERE sale_id = $1
          AND company_id = $2
        ORDER BY variant_id
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    if item_rows.is_empty() {
        return Err(SalesError::EmptySale);
    }

    let mut payments_total = Decimal::ZERO;

    if total == Decimal::ZERO && !request.payments.is_empty() {
        return Err(SalesError::InvalidPayment);
    }

    for payment in &request.payments {
        if !["cash", "card", "mobile_money", "other"].contains(&payment.payment_method.as_str()) {
            return Err(SalesError::InvalidPaymentMethod);
        }

        if payment.amount <= Decimal::ZERO
            || !validate_decimal(payment.amount)
            || payment.tendered_amount < payment.amount
            || !validate_decimal(payment.tendered_amount)
        {
            return Err(SalesError::InvalidPayment);
        }

        if payment.payment_method != "cash" && payment.tendered_amount != payment.amount {
            return Err(SalesError::InvalidPayment);
        }

        payments_total += payment.amount;
    }

    if payments_total > total {
        return Err(SalesError::InvalidPayment);
    }

    // Walk-in sales must be paid in full. A named customer may leave a balance
    // only when credit is enabled and the resulting receivable is within limit.
    if payments_total < total {
        if !has_permission(&mut tx, company_id, user_id, "customers.credit").await? {
            return Err(SalesError::CreditPermissionRequired);
        }
        let customer_id = customer_id.ok_or(SalesError::InvalidPayment)?;
        if !customer_credit_permits(&mut tx, company_id, customer_id, total - payments_total)
            .await?
        {
            return Err(SalesError::InvalidPayment);
        }
    }

    // Resolve the active register session for cash payments.
    let has_cash_payment = request
        .payments
        .iter()
        .any(|payment| payment.payment_method == "cash");

    let cash_session_id: Option<i64> = if has_cash_payment {
        let register_id = request
            .register_id
            .ok_or(SalesError::CashRegisterRequired)?;

        let session_id = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT s.id
            FROM cash_register_sessions s
            JOIN cash_registers r
            ON r.id = s.register_id
            AND r.company_id = s.company_id
            AND r.location_id = s.location_id
            WHERE s.company_id = $1
            AND s.register_id = $2
            AND s.location_id = $3
            AND s.status = 'open'
            AND r.is_active = TRUE
            FOR UPDATE OF s, r
            "#,
        )
        .bind(company_id)
        .bind(register_id)
        .bind(location_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(SalesError::Database)?
        .ok_or(SalesError::CashSessionNotOpen)?;

        Some(session_id)
    } else {
        if request.register_id.is_some() {
            return Err(SalesError::InvalidPayment);
        }

        None
    };

    // Lock stock balances in variant ID order to reduce deadlock risk.
    for item in &item_rows {
        let variant_id: i64 = item.try_get("variant_id").map_err(SalesError::Database)?;
        let quantity: Decimal = item.try_get("quantity").map_err(SalesError::Database)?;

        let track_inventory = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT p.track_inventory
            FROM product_variants pv
            JOIN products p
              ON p.id = pv.product_id
             AND p.company_id = pv.company_id
            WHERE pv.id = $1
              AND pv.company_id = $2
              AND pv.is_active = TRUE
              AND p.is_active = TRUE
              AND p.archived_at IS NULL
            "#,
        )
        .bind(variant_id)
        .bind(company_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(SalesError::Database)?
        .ok_or(SalesError::InvalidVariant)?;

        if !track_inventory {
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO stock_balances (
                company_id,
                location_id,
                variant_id
            )
            VALUES ($1,$2,$3)
            ON CONFLICT (location_id, variant_id) DO NOTHING
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

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
        .bind(location_id)
        .bind(variant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

        let on_hand: Decimal = balance
            .try_get("quantity_on_hand")
            .map_err(SalesError::Database)?;
        let reserved: Decimal = balance
            .try_get("reserved_quantity")
            .map_err(SalesError::Database)?;

        if on_hand - reserved < quantity {
            return Err(SalesError::InsufficientStock);
        }

        let new_quantity = on_hand - quantity;

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
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;

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
                $1,$2,$3,'sale',$4,
                'sale',$5,$6,$7,$8
            )
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .bind(-quantity)
        .bind(sale_id.to_string())
        .bind(Uuid::new_v4())
        .bind(Some(format!("Sale {}", sale_id)))
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;
    }

    for payment in &request.payments {
        let change = if payment.payment_method == "cash" {
            payment.tendered_amount - payment.amount
        } else {
            Decimal::ZERO
        };

        sqlx::query(
            r#"
            INSERT INTO sale_payments (
                company_id,
                sale_id,
                payment_method,
                amount,
                tendered_amount,
                change_given,
                reference,
                notes,
                received_by,
                cash_session_id,
                currency_code
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            "#,
        )
        .bind(company_id)
        .bind(sale_id)
        .bind(&payment.payment_method)
        .bind(payment.amount)
        .bind(payment.tendered_amount)
        .bind(change)
        .bind(normalize_notes(payment.reference.clone()))
        .bind(normalize_notes(payment.notes.clone()))
        .bind(user_id)
        .bind(if payment.payment_method == "cash" {
            cash_session_id
        } else {
            None
        })
        .bind(&currency_code)
        .execute(&mut *tx)
        .await
        .map_err(SalesError::Database)?;
    }

    sqlx::query(
        r#"
        UPDATE sales
        SET status = 'completed',
            completed_by = $1,
            completed_at = NOW(),
            checkout_idempotency_key = $2,
            updated_at = NOW(),
            version = version + 1
        WHERE id = $3
          AND company_id = $4
          AND status = 'draft'
        "#,
    )
    .bind(user_id)
    .bind(request.idempotency_key)
    .bind(sale_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(SalesError::Database)?;

    // Load the completed sale within the same transaction.
    tx.commit().await.map_err(SalesError::Database)?;

    let sale = load_sale(db, company_id, sale_id).await?;

    Ok(CheckoutOutcome {
        sale,
        replayed: false,
    })
}
