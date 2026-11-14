use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

use crate::models::sales_returns::*;

#[derive(Debug)]
pub enum SalesReturnsError {
    Database(Error),
    NotFound,
    InvalidRequest,
    InvalidReason,
    InvalidQuantity,
    InvalidCondition,
    InvalidPayment,
    InvalidPaymentMethod,
    SaleNotCompleted,
    SaleAlreadyVoided,
    SaleHasReturns,
    ReturnQuantityExceeded,
    RefundExceedsEligibleAmount,
    DecimalQuantityNotAllowed,
    IdempotencyConflict,
    PermissionDenied,
    CashRegisterRequired,
    CashSessionNotOpen,
    InsufficientCash,
}

pub struct IdempotentOutcome<T> {
    pub value: T,
    pub replayed: bool,
}

fn clean(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() { None } else { Some(v) }
    })
}

fn valid_decimal(value: Decimal) -> bool {
    value.scale() <= 4 && value.abs() < Decimal::from(1_000_000_000_000_000_i64)
}

async fn has_permission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    user_id: Uuid,
    permission: &str,
) -> Result<bool, SalesReturnsError> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM user_roles ur
            JOIN roles r
              ON r.id = ur.role_id
             AND r.company_id = ur.company_id
            JOIN role_permissions rp ON rp.role_id = r.id
            JOIN permissions p ON p.id = rp.permission_id
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
    .map_err(SalesReturnsError::Database)
}

async fn require_permission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    user_id: Uuid,
    permission: &str,
) -> Result<(), SalesReturnsError> {
    if !has_permission(tx, company_id, user_id, permission).await? {
        return Err(SalesReturnsError::PermissionDenied);
    }
    Ok(())
}

async fn load_return(
    db: &PgPool,
    company_id: Uuid,
    return_id: i64,
) -> Result<SaleReturn, SalesReturnsError> {
    let row = sqlx::query(
        r#"
        SELECT id, sale_id, location_id, reason, notes, created_by, created_at
        FROM sale_returns
        WHERE id = $1 AND company_id = $2
        "#,
    )
    .bind(return_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::NotFound)?;

    let items = sqlx::query(
        r#"
        SELECT id, sale_item_id, variant_id, product_name, variant_name,
               quantity, item_condition, refundable_amount
        FROM sale_return_items
        WHERE return_id = $1 AND company_id = $2
        ORDER BY id
        "#,
    )
    .bind(return_id)
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(SalesReturnsError::Database)?
    .into_iter()
    .map(|r| {
        Ok(SaleReturnItem {
            id: r.try_get("id").map_err(SalesReturnsError::Database)?,
            sale_item_id: r
                .try_get("sale_item_id")
                .map_err(SalesReturnsError::Database)?,
            variant_id: r
                .try_get("variant_id")
                .map_err(SalesReturnsError::Database)?,
            product_name: r
                .try_get("product_name")
                .map_err(SalesReturnsError::Database)?,
            variant_name: r
                .try_get("variant_name")
                .map_err(SalesReturnsError::Database)?,
            quantity: r.try_get("quantity").map_err(SalesReturnsError::Database)?,
            item_condition: r
                .try_get("item_condition")
                .map_err(SalesReturnsError::Database)?,
            refundable_amount: r
                .try_get("refundable_amount")
                .map_err(SalesReturnsError::Database)?,
        })
    })
    .collect::<Result<Vec<_>, SalesReturnsError>>()?;

    Ok(SaleReturn {
        id: row.try_get("id").map_err(SalesReturnsError::Database)?,
        sale_id: row
            .try_get("sale_id")
            .map_err(SalesReturnsError::Database)?,
        location_id: row
            .try_get("location_id")
            .map_err(SalesReturnsError::Database)?,
        reason: row.try_get("reason").map_err(SalesReturnsError::Database)?,
        notes: row.try_get("notes").map_err(SalesReturnsError::Database)?,
        created_by: row
            .try_get("created_by")
            .map_err(SalesReturnsError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(SalesReturnsError::Database)?,
        items,
    })
}

pub async fn create_return(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    sale_id: i64,
    request: CreateSaleReturnRequest,
) -> Result<IdempotentOutcome<SaleReturn>, SalesReturnsError> {
    let reason = request.reason.trim().to_string();

    if reason.is_empty() || reason.len() > 1000 {
        return Err(SalesReturnsError::InvalidReason);
    }

    if request.items.is_empty() {
        return Err(SalesReturnsError::InvalidRequest);
    }

    let mut seen = std::collections::HashSet::new();
    for item in &request.items {
        if !seen.insert(item.sale_item_id)
            || item.quantity <= Decimal::ZERO
            || !valid_decimal(item.quantity)
        {
            return Err(SalesReturnsError::InvalidQuantity);
        }

        if !["resellable", "damaged", "unsellable"].contains(&item.item_condition.as_str()) {
            return Err(SalesReturnsError::InvalidCondition);
        }
    }

    let mut sorted_items = request
        .items
        .iter()
        .map(|i| {
            (
                i.sale_item_id,
                i.quantity.to_string(),
                i.item_condition.clone(),
            )
        })
        .collect::<Vec<_>>();
    sorted_items.sort();

    let legacy_fingerprint = format!(
        "{}|{}|{}",
        reason,
        clean(request.notes.clone()).unwrap_or_default(),
        serde_json::to_string(&sorted_items).map_err(|_| SalesReturnsError::InvalidRequest)?
    );
    let fingerprint = format!("v2|sale:{}|{}", sale_id, legacy_fingerprint);

    let mut tx = db.begin().await.map_err(SalesReturnsError::Database)?;

    require_permission(&mut tx, company_id, user_id, "sales.return").await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("return:{}:{}", company_id, request.idempotency_key))
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

    if let Some(existing) = sqlx::query(
        r#"
        SELECT id, sale_id, request_fingerprint
        FROM sale_returns
        WHERE company_id = $1 AND idempotency_key = $2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    {
        let existing_sale_id: i64 = existing
            .try_get("sale_id")
            .map_err(SalesReturnsError::Database)?;
        let existing_fingerprint: String = existing
            .try_get("request_fingerprint")
            .map_err(SalesReturnsError::Database)?;
        if existing_sale_id != sale_id
            || (existing_fingerprint != fingerprint && existing_fingerprint != legacy_fingerprint)
        {
            return Err(SalesReturnsError::IdempotencyConflict);
        }

        let existing_id: i64 = existing
            .try_get("id")
            .map_err(SalesReturnsError::Database)?;
        tx.commit().await.map_err(SalesReturnsError::Database)?;
        return Ok(IdempotentOutcome {
            value: load_return(db, company_id, existing_id).await?,
            replayed: true,
        });
    }

    let key_used = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM sale_returns WHERE company_id=$1 AND idempotency_key=$2)",
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if key_used {
        return Err(SalesReturnsError::IdempotencyConflict);
    }

    let sale = sqlx::query(
        r#"
        SELECT id, location_id, status
        FROM sales
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::NotFound)?;

    let status: String = sale
        .try_get("status")
        .map_err(SalesReturnsError::Database)?;
    let location_id: i64 = sale
        .try_get("location_id")
        .map_err(SalesReturnsError::Database)?;

    if status != "completed" {
        return Err(SalesReturnsError::SaleNotCompleted);
    }

    let voided = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM sale_voids WHERE company_id=$1 AND sale_id=$2)",
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if voided {
        return Err(SalesReturnsError::SaleAlreadyVoided);
    }

    let return_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO sale_returns (
            company_id, sale_id, location_id, reason, notes,
            idempotency_key, request_fingerprint, created_by
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .bind(location_id)
    .bind(&reason)
    .bind(clean(request.notes))
    .bind(request.idempotency_key)
    .bind(&fingerprint)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let mut return_items = request.items;
    return_items.sort_by_key(|item| item.sale_item_id);
    let mut stock_restorations: Vec<(i64, Decimal)> = Vec::new();

    for item in return_items {
        let original = sqlx::query(
            r#"
            SELECT
                si.id, si.variant_id, si.product_name, si.variant_name,
                si.quantity AS sold_quantity,
                si.line_total,
                si.unit_code,
                u.allows_decimal,
                p.track_inventory
            FROM sale_items si
            JOIN product_variants pv
              ON pv.id = si.variant_id AND pv.company_id = si.company_id
            JOIN products p
              ON p.id = pv.product_id AND p.company_id = pv.company_id
            JOIN units_of_measure u
              ON u.code = si.unit_code
            WHERE si.id = $1
              AND si.company_id = $2
              AND si.sale_id = $3
            FOR UPDATE OF si
            "#,
        )
        .bind(item.sale_item_id)
        .bind(company_id)
        .bind(sale_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?
        .ok_or(SalesReturnsError::InvalidRequest)?;

        let sold_quantity: Decimal = original
            .try_get("sold_quantity")
            .map_err(SalesReturnsError::Database)?;
        let line_total: Decimal = original
            .try_get("line_total")
            .map_err(SalesReturnsError::Database)?;
        let allows_decimal: bool = original
            .try_get("allows_decimal")
            .map_err(SalesReturnsError::Database)?;
        let track_inventory: bool = original
            .try_get("track_inventory")
            .map_err(SalesReturnsError::Database)?;

        if !allows_decimal && item.quantity.fract() != Decimal::ZERO {
            return Err(SalesReturnsError::DecimalQuantityNotAllowed);
        }

        let previously_returned: Decimal = sqlx::query_scalar(
            r#"
            SELECT COALESCE(SUM(quantity), 0)
            FROM sale_return_items
            WHERE company_id = $1 AND sale_item_id = $2
            "#,
        )
        .bind(company_id)
        .bind(item.sale_item_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        if previously_returned + item.quantity > sold_quantity {
            return Err(SalesReturnsError::ReturnQuantityExceeded);
        }

        let unit_value = (line_total / sold_quantity).round_dp(4);
        let refundable_amount = (unit_value * item.quantity).round_dp(4);

        let variant_id: i64 = original
            .try_get("variant_id")
            .map_err(SalesReturnsError::Database)?;
        let product_name: String = original
            .try_get("product_name")
            .map_err(SalesReturnsError::Database)?;
        let variant_name: String = original
            .try_get("variant_name")
            .map_err(SalesReturnsError::Database)?;

        sqlx::query(
            r#"
            INSERT INTO sale_return_items (
                company_id, return_id, sale_item_id, variant_id,
                product_name, variant_name, quantity,
                item_condition, refundable_amount
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(company_id)
        .bind(return_id)
        .bind(item.sale_item_id)
        .bind(variant_id)
        .bind(&product_name)
        .bind(&variant_name)
        .bind(item.quantity)
        .bind(&item.item_condition)
        .bind(refundable_amount)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        if track_inventory && item.item_condition == "resellable" {
            stock_restorations.push((variant_id, item.quantity));
        }
    }

    stock_restorations.sort_by_key(|(variant_id, _)| *variant_id);
    for (variant_id, quantity) in stock_restorations {
        sqlx::query(
            r#"
            INSERT INTO stock_balances (
                company_id, location_id, variant_id,
                quantity_on_hand, reserved_quantity, version
            )
            VALUES ($1,$2,$3,0,0,1)
            ON CONFLICT (location_id, variant_id) DO NOTHING
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        sqlx::query(
            r#"
            UPDATE stock_balances
            SET quantity_on_hand = quantity_on_hand + $1,
                version = version + 1,
                updated_at = NOW()
            WHERE company_id = $2 AND location_id = $3 AND variant_id = $4
            "#,
        )
        .bind(quantity)
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        sqlx::query(
            r#"
            INSERT INTO stock_movements (
                company_id, location_id, variant_id,
                movement_type, quantity_delta,
                reference_type, reference_id,
                idempotency_key, notes, created_by
            )
            VALUES ($1,$2,$3,'sale_return',$4,'sale_return',$5,$6,$7,$8)
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .bind(quantity)
        .bind(return_id.to_string())
        .bind(Uuid::new_v4())
        .bind(format!("Return against sale {}", sale_id))
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;
    }

    tx.commit().await.map_err(SalesReturnsError::Database)?;
    Ok(IdempotentOutcome {
        value: load_return(db, company_id, return_id).await?,
        replayed: false,
    })
}

async fn resolve_cash_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    location_id: i64,
    register_id: i64,
) -> Result<i64, SalesReturnsError> {
    sqlx::query_scalar::<_, i64>(
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
    .fetch_optional(&mut **tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::CashSessionNotOpen)
}

async fn ensure_cash_available(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    session_id: i64,
    refund_amount: Decimal,
) -> Result<(), SalesReturnsError> {
    let available: Decimal = sqlx::query_scalar(
        r#"
        SELECT
            s.opening_float
            + COALESCE((SELECT SUM(p.amount) FROM sale_payments p
                WHERE p.company_id = s.company_id AND p.cash_session_id = s.id
                  AND p.payment_method = 'cash'), 0)
            - COALESCE((SELECT SUM(rf.amount) FROM sale_refunds rf
                WHERE rf.company_id = s.company_id AND rf.cash_session_id = s.id
                  AND rf.payment_method = 'cash'), 0)
            + COALESCE((SELECT SUM(cp.amount) FROM customer_payments cp WHERE cp.company_id=s.company_id AND cp.cash_session_id=s.id AND cp.payment_method='cash'),0)
            - COALESCE((SELECT SUM(sp.amount) FROM supplier_payments sp WHERE sp.company_id=s.company_id AND sp.cash_session_id=s.id AND sp.payment_method='cash'),0)
            - COALESCE((SELECT SUM(r.amount) FROM customer_payment_reversals r WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0)
            + COALESCE((SELECT SUM(r.amount) FROM supplier_payment_reversals r WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0)
            + COALESCE((SELECT SUM(cm.amount) FROM cash_movements cm
                WHERE cm.company_id = s.company_id AND cm.register_session_id = s.id
                  AND cm.movement_type = 'cash_in'), 0)
            - COALESCE((SELECT SUM(cm.amount) FROM cash_movements cm
                WHERE cm.company_id = s.company_id AND cm.register_session_id = s.id
                  AND cm.movement_type IN ('cash_out', 'paid_out', 'cash_drop')), 0)
        FROM cash_register_sessions s
        WHERE s.id = $1 AND s.company_id = $2 AND s.status = 'open'
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::CashSessionNotOpen)?;

    if available < refund_amount {
        return Err(SalesReturnsError::InsufficientCash);
    }
    Ok(())
}

pub async fn refund_return(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    return_id: i64,
    request: CreateRefundRequest,
) -> Result<IdempotentOutcome<SaleRefund>, SalesReturnsError> {
    if request.amount <= Decimal::ZERO || !valid_decimal(request.amount) {
        return Err(SalesReturnsError::InvalidPayment);
    }

    if !["cash", "card", "mobile_money", "other"].contains(&request.payment_method.as_str()) {
        return Err(SalesReturnsError::InvalidPaymentMethod);
    }

    let legacy_fingerprint = format!(
        "{}|{}|{}|{}",
        request.payment_method,
        request.amount,
        clean(request.reference.clone()).unwrap_or_default(),
        clean(request.notes.clone()).unwrap_or_default()
    );
    let previous_fingerprint = format!("v2|return:{}|{}", return_id, legacy_fingerprint);
    let fingerprint = format!(
        "v3|return:{}|register:{:?}|{}",
        return_id, request.register_id, legacy_fingerprint
    );

    let mut tx = db.begin().await.map_err(SalesReturnsError::Database)?;

    require_permission(&mut tx, company_id, user_id, "sales.refund").await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("refund:{}:{}", company_id, request.idempotency_key))
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

    if let Some(existing) = sqlx::query(
        r#"
        SELECT id, sale_id, return_id, void_id, payment_method,
               amount, reference, notes, refunded_by, created_at,
               cash_session_id, request_fingerprint
        FROM sale_refunds
        WHERE company_id=$1 AND idempotency_key=$2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    {
        let existing_fingerprint: String = existing
            .try_get("request_fingerprint")
            .map_err(SalesReturnsError::Database)?;
        let existing_return_id: Option<i64> = existing
            .try_get("return_id")
            .map_err(SalesReturnsError::Database)?;
        if existing_return_id != Some(return_id)
            || (existing_fingerprint != fingerprint
                && existing_fingerprint != previous_fingerprint
                && existing_fingerprint != legacy_fingerprint)
        {
            return Err(SalesReturnsError::IdempotencyConflict);
        }

        let result = refund_from_row(&existing)?;
        tx.commit().await.map_err(SalesReturnsError::Database)?;
        return Ok(IdempotentOutcome {
            value: result,
            replayed: true,
        });
    }

    let sale_id: i64 =
        sqlx::query_scalar("SELECT sale_id FROM sale_returns WHERE id=$1 AND company_id=$2")
            .bind(return_id)
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(SalesReturnsError::Database)?
            .ok_or(SalesReturnsError::NotFound)?;

    let sale = sqlx::query(
        "SELECT status, location_id FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::NotFound)?;
    let sale_status: String = sale
        .try_get("status")
        .map_err(SalesReturnsError::Database)?;
    let sale_location_id: i64 = sale
        .try_get("location_id")
        .map_err(SalesReturnsError::Database)?;

    if sale_status != "completed" {
        return Err(SalesReturnsError::SaleNotCompleted);
    }

    let cash_session_id = if request.payment_method == "cash" {
        let register_id = request
            .register_id
            .ok_or(SalesReturnsError::CashRegisterRequired)?;
        let session_id =
            resolve_cash_session(&mut tx, company_id, sale_location_id, register_id).await?;
        ensure_cash_available(&mut tx, company_id, session_id, request.amount).await?;
        Some(session_id)
    } else {
        if request.register_id.is_some() {
            return Err(SalesReturnsError::InvalidRequest);
        }
        None
    };

    let sale_voided: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sale_voids WHERE company_id=$1 AND sale_id=$2)",
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if sale_voided {
        return Err(SalesReturnsError::SaleAlreadyVoided);
    }

    let locked_sale_id: i64 = sqlx::query_scalar(
        "SELECT sale_id FROM sale_returns WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(return_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::NotFound)?;

    if locked_sale_id != sale_id {
        return Err(SalesReturnsError::InvalidRequest);
    }

    let eligible: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(SUM(refundable_amount),0) FROM sale_return_items WHERE company_id=$1 AND return_id=$2",
    )
    .bind(company_id)
    .bind(return_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let already_refunded: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0) FROM sale_refunds WHERE company_id=$1 AND return_id=$2",
    )
    .bind(company_id)
    .bind(return_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if already_refunded + request.amount > eligible {
        return Err(SalesReturnsError::RefundExceedsEligibleAmount);
    }

    let id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO sale_refunds (
            company_id, sale_id, return_id, payment_method, amount,
            reference, notes, idempotency_key, request_fingerprint, refunded_by,
            cash_session_id, currency_code
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,
                (SELECT currency_code FROM sales WHERE id = $2 AND company_id = $1))
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .bind(return_id)
    .bind(&request.payment_method)
    .bind(request.amount)
    .bind(clean(request.reference))
    .bind(clean(request.notes))
    .bind(request.idempotency_key)
    .bind(&fingerprint)
    .bind(user_id)
    .bind(cash_session_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let row = sqlx::query(
        "SELECT id, sale_id, return_id, void_id, cash_session_id, payment_method, amount, reference, notes, refunded_by, created_at FROM sale_refunds WHERE id=$1 AND company_id=$2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let result = refund_from_row(&row)?;
    tx.commit().await.map_err(SalesReturnsError::Database)?;
    Ok(IdempotentOutcome {
        value: result,
        replayed: false,
    })
}

pub async fn void_sale(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    sale_id: i64,
    request: VoidSaleRequest,
) -> Result<IdempotentOutcome<SaleVoid>, SalesReturnsError> {
    let reason = request.reason.trim().to_string();
    if reason.is_empty() || reason.len() > 1000 || request.refunds.is_empty() {
        return Err(SalesReturnsError::InvalidRequest);
    }

    let total_refunds = request
        .refunds
        .iter()
        .try_fold(Decimal::ZERO, |sum, refund| {
            if refund.amount <= Decimal::ZERO || !valid_decimal(refund.amount) {
                return None;
            }
            if !["cash", "card", "mobile_money", "other"].contains(&refund.payment_method.as_str())
            {
                return None;
            }
            Some(sum + refund.amount)
        })
        .ok_or(SalesReturnsError::InvalidPayment)?;

    let legacy_fingerprint = format!(
        "{}|{}|{}",
        reason,
        clean(request.notes.clone()).unwrap_or_default(),
        serde_json::to_string(&request.refunds).map_err(|_| SalesReturnsError::InvalidRequest)?
    );
    let previous_fingerprint = format!("v2|sale:{}|{}", sale_id, legacy_fingerprint);
    let fingerprint = format!(
        "v3|sale:{}|register:{:?}|{}",
        sale_id, request.register_id, legacy_fingerprint
    );

    let mut tx = db.begin().await.map_err(SalesReturnsError::Database)?;

    require_permission(&mut tx, company_id, user_id, "sales.void").await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("void:{}:{}", company_id, request.idempotency_key))
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

    if let Some(row) = sqlx::query(
        "SELECT id, sale_id, reason, notes, created_by, created_at, request_fingerprint FROM sale_voids WHERE company_id=$1 AND idempotency_key=$2",
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    {
        let old_fingerprint: String = row.try_get("request_fingerprint").map_err(SalesReturnsError::Database)?;
        let existing_sale_id: i64 = row.try_get("sale_id").map_err(SalesReturnsError::Database)?;
        if existing_sale_id != sale_id
            || (old_fingerprint != fingerprint
                && old_fingerprint != previous_fingerprint
                && old_fingerprint != legacy_fingerprint)
        {
            return Err(SalesReturnsError::IdempotencyConflict);
        }

        let result = void_from_row(&row)?;
        tx.commit().await.map_err(SalesReturnsError::Database)?;
        return Ok(IdempotentOutcome { value: result, replayed: true });
    }

    let sale = sqlx::query(
        "SELECT id, location_id, status, total_amount FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE",
    )
    .bind(sale_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?
    .ok_or(SalesReturnsError::NotFound)?;

    let status: String = sale
        .try_get("status")
        .map_err(SalesReturnsError::Database)?;
    let location_id: i64 = sale
        .try_get("location_id")
        .map_err(SalesReturnsError::Database)?;
    let sale_total: Decimal = sale
        .try_get("total_amount")
        .map_err(SalesReturnsError::Database)?;

    if status != "completed" {
        return Err(SalesReturnsError::SaleNotCompleted);
    }

    if total_refunds != sale_total {
        return Err(SalesReturnsError::InvalidPayment);
    }

    let total_cash_refunds = request
        .refunds
        .iter()
        .filter(|refund| refund.payment_method == "cash")
        .map(|refund| refund.amount)
        .sum::<Decimal>();

    let cash_session_id = if total_cash_refunds > Decimal::ZERO {
        let register_id = request
            .register_id
            .ok_or(SalesReturnsError::CashRegisterRequired)?;
        let session_id =
            resolve_cash_session(&mut tx, company_id, location_id, register_id).await?;
        ensure_cash_available(&mut tx, company_id, session_id, total_cash_refunds).await?;
        Some(session_id)
    } else {
        if request.register_id.is_some() {
            return Err(SalesReturnsError::InvalidRequest);
        }
        None
    };

    let existing_void = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM sale_voids WHERE company_id=$1 AND sale_id=$2)",
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let existing_returns_or_refunds = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM sale_returns WHERE company_id=$1 AND sale_id=$2
            UNION ALL
            SELECT 1 FROM sale_refunds WHERE company_id=$1 AND sale_id=$2
        )
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if existing_void {
        return Err(SalesReturnsError::SaleAlreadyVoided);
    }

    if existing_returns_or_refunds {
        return Err(SalesReturnsError::SaleHasReturns);
    }

    let void_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO sale_voids (
            company_id, sale_id, reason, notes,
            idempotency_key, request_fingerprint, created_by
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .bind(&reason)
    .bind(clean(request.notes))
    .bind(request.idempotency_key)
    .bind(&fingerprint)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let items = sqlx::query(
        r#"
        SELECT si.variant_id, si.quantity, p.track_inventory
        FROM sale_items si
        JOIN product_variants pv
          ON pv.id=si.variant_id AND pv.company_id=si.company_id
        JOIN products p
          ON p.id=pv.product_id AND p.company_id=pv.company_id
        WHERE si.company_id=$1 AND si.sale_id=$2
        ORDER BY si.variant_id
        FOR UPDATE OF si
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    for item in items {
        let variant_id: i64 = item
            .try_get("variant_id")
            .map_err(SalesReturnsError::Database)?;
        let quantity: Decimal = item
            .try_get("quantity")
            .map_err(SalesReturnsError::Database)?;
        let track_inventory: bool = item
            .try_get("track_inventory")
            .map_err(SalesReturnsError::Database)?;

        if !track_inventory {
            continue;
        }

        sqlx::query(
            r#"
            INSERT INTO stock_balances (
                company_id, location_id, variant_id,
                quantity_on_hand, reserved_quantity, version
            )
            VALUES ($1,$2,$3,0,0,1)
            ON CONFLICT (location_id, variant_id) DO NOTHING
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        sqlx::query(
            r#"
            UPDATE stock_balances
            SET quantity_on_hand=quantity_on_hand+$1,
                version=version+1,
                updated_at=NOW()
            WHERE company_id=$2 AND location_id=$3 AND variant_id=$4
            "#,
        )
        .bind(quantity)
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;

        sqlx::query(
            r#"
            INSERT INTO stock_movements (
                company_id, location_id, variant_id, movement_type,
                quantity_delta, reference_type, reference_id,
                idempotency_key, notes, created_by
            )
            VALUES ($1,$2,$3,'sale_return',$4,'sale_void',$5,$6,$7,$8)
            "#,
        )
        .bind(company_id)
        .bind(location_id)
        .bind(variant_id)
        .bind(quantity)
        .bind(sale_id.to_string())
        .bind(Uuid::new_v4())
        .bind(format!("Stock reversal for voided sale {}", sale_id))
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;
    }

    for refund in request.refunds {
        let refund_key = Uuid::new_v4();
        let refund_fingerprint = format!(
            "void:{}:{}:{}",
            void_id, refund.payment_method, refund.amount
        );

        sqlx::query(
            r#"
            INSERT INTO sale_refunds (
                company_id, sale_id, void_id, payment_method, amount,
                reference, notes, idempotency_key, request_fingerprint, refunded_by,
                cash_session_id, currency_code
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,
                    (SELECT currency_code FROM sales WHERE id = $2 AND company_id = $1))
            "#,
        )
        .bind(company_id)
        .bind(sale_id)
        .bind(void_id)
        .bind(&refund.payment_method)
        .bind(refund.amount)
        .bind(clean(refund.reference))
        .bind(clean(refund.notes))
        .bind(refund_key)
        .bind(refund_fingerprint)
        .bind(user_id)
        .bind(if refund.payment_method == "cash" {
            cash_session_id
        } else {
            None
        })
        .execute(&mut *tx)
        .await
        .map_err(SalesReturnsError::Database)?;
    }

    // Mark the sale as cancelled after successfully recording
    // the void, inventory reversals and refunds.
    //
    // This runs inside the same transaction, so all operations
    // succeed or roll back together.
    let update_result = sqlx::query(
        r#"
        UPDATE sales
        SET
            status = 'cancelled',
            completed_by = NULL,
            completed_at = NULL,
            cancelled_by = $1,
            cancelled_at = NOW(),
            updated_at = NOW(),
            version = version + 1
        WHERE id = $2
        AND company_id = $3
        AND status = 'completed'
        "#,
    )
    .bind(user_id)
    .bind(sale_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    if update_result.rows_affected() != 1 {
        return Err(SalesReturnsError::SaleNotCompleted);
    }

    let row = sqlx::query(
        "SELECT id, sale_id, reason, notes, created_by, created_at FROM sale_voids WHERE id=$1 AND company_id=$2",
    )
    .bind(void_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(SalesReturnsError::Database)?;

    let result = void_from_row(&row)?;
    tx.commit().await.map_err(SalesReturnsError::Database)?;
    Ok(IdempotentOutcome {
        value: result,
        replayed: false,
    })
}

pub async fn list_returns(
    db: &PgPool,
    company_id: Uuid,
    sale_id: i64,
) -> Result<Vec<SaleReturn>, SalesReturnsError> {
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM sale_returns WHERE company_id=$1 AND sale_id=$2 ORDER BY created_at DESC",
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_all(db)
    .await
    .map_err(SalesReturnsError::Database)?;

    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        result.push(load_return(db, company_id, id).await?);
    }
    Ok(result)
}

pub async fn list_refunds(
    db: &PgPool,
    company_id: Uuid,
    sale_id: i64,
) -> Result<Vec<SaleRefund>, SalesReturnsError> {
    let rows = sqlx::query(
        r#"
        SELECT id, sale_id, return_id, void_id, cash_session_id, payment_method,
               amount, reference, notes, refunded_by, created_at
        FROM sale_refunds
        WHERE company_id=$1 AND sale_id=$2
        ORDER BY created_at DESC
        "#,
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_all(db)
    .await
    .map_err(SalesReturnsError::Database)?;

    rows.iter().map(refund_from_row).collect()
}

pub async fn get_void(
    db: &PgPool,
    company_id: Uuid,
    sale_id: i64,
) -> Result<Option<SaleVoid>, SalesReturnsError> {
    let row = sqlx::query(
        "SELECT id, sale_id, reason, notes, created_by, created_at FROM sale_voids WHERE company_id=$1 AND sale_id=$2",
    )
    .bind(company_id)
    .bind(sale_id)
    .fetch_optional(db)
    .await
    .map_err(SalesReturnsError::Database)?;

    row.as_ref().map(void_from_row).transpose()
}

fn refund_from_row(row: &sqlx::postgres::PgRow) -> Result<SaleRefund, SalesReturnsError> {
    Ok(SaleRefund {
        id: row.try_get("id").map_err(SalesReturnsError::Database)?,
        sale_id: row
            .try_get("sale_id")
            .map_err(SalesReturnsError::Database)?,
        return_id: row
            .try_get("return_id")
            .map_err(SalesReturnsError::Database)?,
        void_id: row
            .try_get("void_id")
            .map_err(SalesReturnsError::Database)?,
        cash_session_id: row
            .try_get("cash_session_id")
            .map_err(SalesReturnsError::Database)?,
        payment_method: row
            .try_get("payment_method")
            .map_err(SalesReturnsError::Database)?,
        amount: row.try_get("amount").map_err(SalesReturnsError::Database)?,
        reference: row
            .try_get("reference")
            .map_err(SalesReturnsError::Database)?,
        notes: row.try_get("notes").map_err(SalesReturnsError::Database)?,
        refunded_by: row
            .try_get("refunded_by")
            .map_err(SalesReturnsError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(SalesReturnsError::Database)?,
    })
}

fn void_from_row(row: &sqlx::postgres::PgRow) -> Result<SaleVoid, SalesReturnsError> {
    Ok(SaleVoid {
        id: row.try_get("id").map_err(SalesReturnsError::Database)?,
        sale_id: row
            .try_get("sale_id")
            .map_err(SalesReturnsError::Database)?,
        reason: row.try_get("reason").map_err(SalesReturnsError::Database)?,
        notes: row.try_get("notes").map_err(SalesReturnsError::Database)?,
        created_by: row
            .try_get("created_by")
            .map_err(SalesReturnsError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(SalesReturnsError::Database)?,
    })
}
