use rust_decimal::Decimal;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

use crate::models::cash_register::*;

#[derive(Debug)]
pub enum CashRegisterError {
    Database(Error),
    NotFound,
    InvalidRequest,
    InvalidAmount,
    InsufficientCash,
    RegisterInactive,
    RegisterAlreadyExists,
    SessionAlreadyOpen,
    SessionNotOpen,
    SessionNotClosed,
    IdempotencyConflict,
}

fn valid_amount(value: Decimal) -> bool {
    value >= Decimal::ZERO
        && value.scale() <= 4
        && value.abs() < Decimal::from(1_000_000_000_000_000_i64)
}

fn normalize(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() { None } else { Some(v) }
    })
}

async fn load_register(
    db: &PgPool,
    company_id: Uuid,
    register_id: i64,
) -> Result<CashRegister, CashRegisterError> {
    let row = sqlx::query(
        r#"
        SELECT id, location_id, code, name, is_active,
               created_at, updated_at
        FROM cash_registers
        WHERE id = $1 AND company_id = $2
        "#,
    )
    .bind(register_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    Ok(CashRegister {
        id: row.try_get("id").map_err(CashRegisterError::Database)?,
        location_id: row
            .try_get("location_id")
            .map_err(CashRegisterError::Database)?,
        code: row.try_get("code").map_err(CashRegisterError::Database)?,
        name: row.try_get("name").map_err(CashRegisterError::Database)?,
        is_active: row
            .try_get("is_active")
            .map_err(CashRegisterError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(CashRegisterError::Database)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(CashRegisterError::Database)?,
    })
}

async fn load_session(
    db: &PgPool,
    company_id: Uuid,
    session_id: i64,
) -> Result<CashRegisterSession, CashRegisterError> {
    let row = sqlx::query(
        r#"
        SELECT
            id, register_id, location_id, status,
            opening_float, opening_notes,
            opened_by, opened_at,
            closed_by, closed_at,
            expected_cash, closing_cash_count, cash_variance,
            closing_notes, version
        FROM cash_register_sessions
        WHERE id = $1 AND company_id = $2
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    Ok(CashRegisterSession {
        id: row.try_get("id").map_err(CashRegisterError::Database)?,
        register_id: row
            .try_get("register_id")
            .map_err(CashRegisterError::Database)?,
        location_id: row
            .try_get("location_id")
            .map_err(CashRegisterError::Database)?,
        status: row.try_get("status").map_err(CashRegisterError::Database)?,
        opening_float: row
            .try_get("opening_float")
            .map_err(CashRegisterError::Database)?,
        opening_notes: row
            .try_get("opening_notes")
            .map_err(CashRegisterError::Database)?,
        opened_by: row
            .try_get("opened_by")
            .map_err(CashRegisterError::Database)?,
        opened_at: row
            .try_get("opened_at")
            .map_err(CashRegisterError::Database)?,
        closed_by: row
            .try_get("closed_by")
            .map_err(CashRegisterError::Database)?,
        closed_at: row
            .try_get("closed_at")
            .map_err(CashRegisterError::Database)?,
        expected_cash: row
            .try_get("expected_cash")
            .map_err(CashRegisterError::Database)?,
        closing_cash_count: row
            .try_get("closing_cash_count")
            .map_err(CashRegisterError::Database)?,
        cash_variance: row
            .try_get("cash_variance")
            .map_err(CashRegisterError::Database)?,
        closing_notes: row
            .try_get("closing_notes")
            .map_err(CashRegisterError::Database)?,
        version: row
            .try_get("version")
            .map_err(CashRegisterError::Database)?,
    })
}

pub async fn create_register(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    request: CreateCashRegisterRequest,
) -> Result<CashRegister, CashRegisterError> {
    let code = request.code.trim().to_uppercase();
    let name = request.name.trim().to_string();

    if code.is_empty() || name.is_empty() || code.len() > 30 || name.len() > 100 {
        return Err(CashRegisterError::InvalidRequest);
    }

    let register_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO cash_registers (
            company_id, location_id, code, name, created_by
        )
        SELECT $1, id, $3, $4, $5
        FROM inventory_locations
        WHERE id = $2
          AND company_id = $1
          AND is_active = TRUE
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(request.location_id)
    .bind(&code)
    .bind(&name)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(|error| {
        if let Error::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                return CashRegisterError::RegisterAlreadyExists;
            }
        }
        CashRegisterError::Database(error)
    })?
    .ok_or(CashRegisterError::NotFound)?;

    load_register(db, company_id, register_id).await
}

pub async fn list_registers(
    db: &PgPool,
    company_id: Uuid,
    location_id: Option<i64>,
) -> Result<Vec<CashRegister>, CashRegisterError> {
    let rows = sqlx::query(
        r#"
        SELECT id, location_id, code, name, is_active,
               created_at, updated_at
        FROM cash_registers
        WHERE company_id = $1
          AND ($2::BIGINT IS NULL OR location_id = $2)
        ORDER BY location_id, name
        "#,
    )
    .bind(company_id)
    .bind(location_id)
    .fetch_all(db)
    .await
    .map_err(CashRegisterError::Database)?;

    rows.into_iter()
        .map(|row| {
            Ok(CashRegister {
                id: row.try_get("id").map_err(CashRegisterError::Database)?,
                location_id: row
                    .try_get("location_id")
                    .map_err(CashRegisterError::Database)?,
                code: row.try_get("code").map_err(CashRegisterError::Database)?,
                name: row.try_get("name").map_err(CashRegisterError::Database)?,
                is_active: row
                    .try_get("is_active")
                    .map_err(CashRegisterError::Database)?,
                created_at: row
                    .try_get("created_at")
                    .map_err(CashRegisterError::Database)?,
                updated_at: row
                    .try_get("updated_at")
                    .map_err(CashRegisterError::Database)?,
            })
        })
        .collect()
}

pub async fn open_session(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    register_id: i64,
    request: OpenCashSessionRequest,
) -> Result<CashRegisterSession, CashRegisterError> {
    if !valid_amount(request.opening_float) {
        return Err(CashRegisterError::InvalidAmount);
    }

    let mut tx = db.begin().await.map_err(CashRegisterError::Database)?;

    let register = sqlx::query(
        r#"
        SELECT id, is_active
        FROM cash_registers
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(register_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    let active: bool = register
        .try_get("is_active")
        .map_err(CashRegisterError::Database)?;

    if !active {
        return Err(CashRegisterError::RegisterInactive);
    }

    let location_id: i64 = sqlx::query_scalar(
        "SELECT location_id FROM cash_registers WHERE id = $1 AND company_id = $2",
    )
    .bind(register_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?;

    let session_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO cash_register_sessions (
            company_id, register_id, location_id,
            opening_float, opening_notes, opened_by
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(register_id)
    .bind(location_id)
    .bind(request.opening_float)
    .bind(normalize(request.opening_notes))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        if let Error::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                return CashRegisterError::SessionAlreadyOpen;
            }
        }
        CashRegisterError::Database(error)
    })?;

    tx.commit().await.map_err(CashRegisterError::Database)?;

    load_session(db, company_id, session_id).await
}

pub async fn get_session(
    db: &PgPool,
    company_id: Uuid,
    session_id: i64,
) -> Result<CashRegisterSession, CashRegisterError> {
    load_session(db, company_id, session_id).await
}

pub async fn get_open_session(
    db: &PgPool,
    company_id: Uuid,
    register_id: i64,
) -> Result<CashRegisterSession, CashRegisterError> {
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id
        FROM cash_register_sessions
        WHERE company_id = $1
          AND register_id = $2
          AND status = 'open'
        "#,
    )
    .bind(company_id)
    .bind(register_id)
    .fetch_optional(db)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::SessionNotOpen)?;

    load_session(db, company_id, id).await
}

pub(crate) async fn current_cash_balance(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    session_id: i64,
) -> Result<Decimal, CashRegisterError> {
    sqlx::query_scalar(
        r#"
        SELECT
            s.opening_float
            + COALESCE((SELECT SUM(p.amount) FROM sale_payments p
                WHERE p.company_id = s.company_id AND p.cash_session_id = s.id
                  AND p.payment_method = 'cash'), 0)
            - COALESCE((SELECT SUM(r.amount) FROM sale_refunds r
                WHERE r.company_id = s.company_id AND r.cash_session_id = s.id
                  AND r.payment_method = 'cash'), 0)
            + COALESCE((SELECT SUM(cp.amount) FROM customer_payments cp
                WHERE cp.company_id=s.company_id AND cp.cash_session_id=s.id AND cp.payment_method='cash'),0)
            - COALESCE((SELECT SUM(sp.amount) FROM supplier_payments sp
                WHERE sp.company_id=s.company_id AND sp.cash_session_id=s.id AND sp.payment_method='cash'),0)
            - COALESCE((SELECT SUM(r.amount) FROM customer_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0)
            + COALESCE((SELECT SUM(r.amount) FROM supplier_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0)
            + COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type = 'cash_in'), 0)
            - COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type IN ('cash_out', 'paid_out', 'cash_drop')), 0)
        FROM cash_register_sessions s
        WHERE s.id = $1 AND s.company_id = $2 AND s.status = 'open'
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::SessionNotOpen)
}

pub async fn create_movement(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    session_id: i64,
    request: CashMovementRequest,
) -> Result<CashMovement, CashRegisterError> {
    if !["cash_in", "cash_out", "paid_out", "cash_drop"].contains(&request.movement_type.as_str()) {
        return Err(CashRegisterError::InvalidRequest);
    }

    if request.amount <= Decimal::ZERO || !valid_amount(request.amount) {
        return Err(CashRegisterError::InvalidAmount);
    }

    let legacy_fingerprint = format!(
        "{}|{}|{}|{}",
        request.movement_type,
        request.amount,
        request.reference.as_deref().unwrap_or(""),
        request.notes.as_deref().unwrap_or("")
    );
    let fingerprint = format!("session:{}|{}", session_id, legacy_fingerprint);

    let mut tx = db.begin().await.map_err(CashRegisterError::Database)?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{}:{}", company_id, request.idempotency_key))
        .execute(&mut *tx)
        .await
        .map_err(CashRegisterError::Database)?;

    if let Some(row) = sqlx::query(
        r#"
        SELECT id, register_session_id, movement_type, amount,
               reference, notes, created_by, created_at,
               request_fingerprint
        FROM cash_movements
        WHERE company_id = $1 AND idempotency_key = $2
        "#,
    )
    .bind(company_id)
    .bind(request.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?
    {
        let stored: String = row
            .try_get("request_fingerprint")
            .map_err(CashRegisterError::Database)?;
        let stored_session_id: i64 = row
            .try_get("register_session_id")
            .map_err(CashRegisterError::Database)?;

        if stored_session_id != session_id
            || (stored != fingerprint && stored != legacy_fingerprint)
        {
            return Err(CashRegisterError::IdempotencyConflict);
        }

        let movement = CashMovement {
            id: row.try_get("id").map_err(CashRegisterError::Database)?,
            register_session_id: row
                .try_get("register_session_id")
                .map_err(CashRegisterError::Database)?,
            movement_type: row
                .try_get("movement_type")
                .map_err(CashRegisterError::Database)?,
            amount: row.try_get("amount").map_err(CashRegisterError::Database)?,
            reference: row
                .try_get("reference")
                .map_err(CashRegisterError::Database)?,
            notes: row.try_get("notes").map_err(CashRegisterError::Database)?,
            created_by: row
                .try_get("created_by")
                .map_err(CashRegisterError::Database)?,
            created_at: row
                .try_get("created_at")
                .map_err(CashRegisterError::Database)?,
        };

        tx.commit().await.map_err(CashRegisterError::Database)?;
        return Ok(movement);
    }

    let status = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM cash_register_sessions
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    if status != "open" {
        return Err(CashRegisterError::SessionNotOpen);
    }

    if ["cash_out", "paid_out", "cash_drop"].contains(&request.movement_type.as_str()) {
        let available = current_cash_balance(&mut tx, company_id, session_id).await?;
        if available < request.amount {
            return Err(CashRegisterError::InsufficientCash);
        }
    }

    let row = sqlx::query(
        r#"
        INSERT INTO cash_movements (
            company_id, register_session_id, movement_type,
            amount, reference, notes, idempotency_key,
            request_fingerprint, created_by
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING id, register_session_id, movement_type, amount,
                  reference, notes, created_by, created_at
        "#,
    )
    .bind(company_id)
    .bind(session_id)
    .bind(&request.movement_type)
    .bind(request.amount)
    .bind(normalize(request.reference))
    .bind(normalize(request.notes))
    .bind(request.idempotency_key)
    .bind(fingerprint)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?;

    let movement = CashMovement {
        id: row.try_get("id").map_err(CashRegisterError::Database)?,
        register_session_id: row
            .try_get("register_session_id")
            .map_err(CashRegisterError::Database)?,
        movement_type: row
            .try_get("movement_type")
            .map_err(CashRegisterError::Database)?,
        amount: row.try_get("amount").map_err(CashRegisterError::Database)?,
        reference: row
            .try_get("reference")
            .map_err(CashRegisterError::Database)?,
        notes: row.try_get("notes").map_err(CashRegisterError::Database)?,
        created_by: row
            .try_get("created_by")
            .map_err(CashRegisterError::Database)?,
        created_at: row
            .try_get("created_at")
            .map_err(CashRegisterError::Database)?,
    };

    tx.commit().await.map_err(CashRegisterError::Database)?;

    Ok(movement)
}

pub async fn close_session(
    db: &PgPool,
    company_id: Uuid,
    user_id: Uuid,
    session_id: i64,
    request: CloseCashSessionRequest,
) -> Result<CashRegisterSession, CashRegisterError> {
    if !valid_amount(request.closing_cash_count) {
        return Err(CashRegisterError::InvalidAmount);
    }

    let mut tx = db.begin().await.map_err(CashRegisterError::Database)?;

    let status = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM cash_register_sessions
        WHERE id = $1 AND company_id = $2
        FOR UPDATE
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    if status != "open" {
        return Err(CashRegisterError::SessionNotOpen);
    }

    let totals = sqlx::query(
        r#"
        SELECT
            s.opening_float,

            COALESCE((
                SELECT SUM(p.amount)
                FROM sale_payments p
                WHERE p.company_id = s.company_id
                  AND p.cash_session_id = s.id
                  AND p.payment_method = 'cash'
            ), 0) AS cash_sales,

            COALESCE((SELECT SUM(cp.amount) FROM customer_payments cp
                WHERE cp.company_id=s.company_id AND cp.cash_session_id=s.id AND cp.payment_method='cash'),0) AS customer_receipts,
            COALESCE((SELECT SUM(sp.amount) FROM supplier_payments sp
                WHERE sp.company_id=s.company_id AND sp.cash_session_id=s.id AND sp.payment_method='cash'),0) AS supplier_payments,
            COALESCE((SELECT SUM(r.amount) FROM customer_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0) AS customer_payment_reversals,
            COALESCE((SELECT SUM(r.amount) FROM supplier_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0) AS supplier_payment_reversals,

            COALESCE((
                SELECT SUM(r.amount)
                FROM sale_refunds r
                WHERE r.company_id = s.company_id
                  AND r.cash_session_id = s.id
                  AND r.payment_method = 'cash'
            ), 0) AS cash_refunds,

            COALESCE((
                SELECT SUM(amount)
                FROM cash_movements
                WHERE company_id = s.company_id
                  AND register_session_id = s.id
                  AND movement_type = 'cash_in'
            ), 0) AS cash_in,

            COALESCE((
                SELECT SUM(amount)
                FROM cash_movements
                WHERE company_id = s.company_id
                  AND register_session_id = s.id
                  AND movement_type = 'cash_out'
            ), 0) AS cash_out,

            COALESCE((
                SELECT SUM(amount)
                FROM cash_movements
                WHERE company_id = s.company_id
                  AND register_session_id = s.id
                  AND movement_type = 'paid_out'
            ), 0) AS paid_out,

            COALESCE((
                SELECT SUM(amount)
                FROM cash_movements
                WHERE company_id = s.company_id
                  AND register_session_id = s.id
                  AND movement_type = 'cash_drop'
            ), 0) AS cash_drops

        FROM cash_register_sessions s
        WHERE s.id = $1 AND s.company_id = $2
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?;

    let opening: Decimal = totals
        .try_get("opening_float")
        .map_err(CashRegisterError::Database)?;
    let sales: Decimal = totals
        .try_get("cash_sales")
        .map_err(CashRegisterError::Database)?;
    let customer_receipts: Decimal = totals
        .try_get("customer_receipts")
        .map_err(CashRegisterError::Database)?;
    let supplier_payments: Decimal = totals
        .try_get("supplier_payments")
        .map_err(CashRegisterError::Database)?;
    let customer_payment_reversals: Decimal = totals
        .try_get("customer_payment_reversals")
        .map_err(CashRegisterError::Database)?;
    let supplier_payment_reversals: Decimal = totals
        .try_get("supplier_payment_reversals")
        .map_err(CashRegisterError::Database)?;
    let refunds: Decimal = totals
        .try_get("cash_refunds")
        .map_err(CashRegisterError::Database)?;
    let cash_in: Decimal = totals
        .try_get("cash_in")
        .map_err(CashRegisterError::Database)?;
    let cash_out: Decimal = totals
        .try_get("cash_out")
        .map_err(CashRegisterError::Database)?;
    let paid_out: Decimal = totals
        .try_get("paid_out")
        .map_err(CashRegisterError::Database)?;
    let drops: Decimal = totals
        .try_get("cash_drops")
        .map_err(CashRegisterError::Database)?;

    let expected = opening + sales + customer_receipts
        - refunds
        - supplier_payments
        - customer_payment_reversals
        + supplier_payment_reversals
        + cash_in
        - cash_out
        - paid_out
        - drops;
    let variance = request.closing_cash_count - expected;

    if expected < Decimal::ZERO {
        return Err(CashRegisterError::InvalidRequest);
    }

    sqlx::query(
        r#"
        UPDATE cash_register_sessions
        SET status = 'closed',
            closed_by = $1,
            closed_at = NOW(),
            expected_cash = $2,
            closing_cash_count = $3,
            cash_variance = $4,
            closing_notes = $5,
            version = version + 1
        WHERE id = $6 AND company_id = $7
        "#,
    )
    .bind(user_id)
    .bind(expected)
    .bind(request.closing_cash_count)
    .bind(variance)
    .bind(normalize(request.closing_notes))
    .bind(session_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(CashRegisterError::Database)?;

    tx.commit().await.map_err(CashRegisterError::Database)?;

    load_session(db, company_id, session_id).await
}

pub async fn get_session_summary(
    db: &PgPool,
    company_id: Uuid,
    session_id: i64,
) -> Result<CashSessionSummary, CashRegisterError> {
    let session = load_session(db, company_id, session_id).await?;
    let row = sqlx::query(
        r#"
        SELECT
            COALESCE((SELECT SUM(p.amount) FROM sale_payments p
                WHERE p.company_id = s.company_id AND p.cash_session_id = s.id
                  AND p.payment_method = 'cash'), 0) AS total_cash_sales,
            COALESCE((SELECT SUM(cp.amount) FROM customer_payments cp
                WHERE cp.company_id=s.company_id AND cp.cash_session_id=s.id AND cp.payment_method='cash'),0) AS total_customer_receipts,
            COALESCE((SELECT SUM(sp.amount) FROM supplier_payments sp
                WHERE sp.company_id=s.company_id AND sp.cash_session_id=s.id AND sp.payment_method='cash'),0) AS total_supplier_payments,
            COALESCE((SELECT SUM(r.amount) FROM customer_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0) AS total_customer_payment_reversals,
            COALESCE((SELECT SUM(r.amount) FROM supplier_payment_reversals r
                WHERE r.company_id=s.company_id AND r.cash_session_id=s.id AND r.payment_method='cash'),0) AS total_supplier_payment_reversals,
            COALESCE((SELECT SUM(r.amount) FROM sale_refunds r
                WHERE r.company_id = s.company_id AND r.cash_session_id = s.id
                  AND r.payment_method = 'cash'), 0) AS total_cash_refunds,
            COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type = 'cash_in'), 0) AS total_cash_in,
            COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type = 'cash_out'), 0) AS total_cash_out,
            COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type = 'paid_out'), 0) AS total_paid_out,
            COALESCE((SELECT SUM(m.amount) FROM cash_movements m
                WHERE m.company_id = s.company_id AND m.register_session_id = s.id
                  AND m.movement_type = 'cash_drop'), 0) AS total_cash_drops
        FROM cash_register_sessions s
        WHERE s.id = $1 AND s.company_id = $2
        "#,
    )
    .bind(session_id)
    .bind(company_id)
    .fetch_optional(db)
    .await
    .map_err(CashRegisterError::Database)?
    .ok_or(CashRegisterError::NotFound)?;

    let total_cash_sales: Decimal = row
        .try_get("total_cash_sales")
        .map_err(CashRegisterError::Database)?;

    let total_customer_receipts: Decimal = row
        .try_get("total_customer_receipts")
        .map_err(CashRegisterError::Database)?;
    let total_supplier_payments: Decimal = row
        .try_get("total_supplier_payments")
        .map_err(CashRegisterError::Database)?;
    let total_customer_payment_reversals: Decimal = row
        .try_get("total_customer_payment_reversals")
        .map_err(CashRegisterError::Database)?;
    let total_supplier_payment_reversals: Decimal = row
        .try_get("total_supplier_payment_reversals")
        .map_err(CashRegisterError::Database)?;
    let total_cash_refunds: Decimal = row
        .try_get("total_cash_refunds")
        .map_err(CashRegisterError::Database)?;

    let total_cash_in: Decimal = row
        .try_get("total_cash_in")
        .map_err(CashRegisterError::Database)?;

    let total_cash_out: Decimal = row
        .try_get("total_cash_out")
        .map_err(CashRegisterError::Database)?;

    let total_paid_out: Decimal = row
        .try_get("total_paid_out")
        .map_err(CashRegisterError::Database)?;

    let total_cash_drops: Decimal = row
        .try_get("total_cash_drops")
        .map_err(CashRegisterError::Database)?;

    let calculated_expected_cash =
        session.opening_float + total_cash_sales + total_customer_receipts
            - total_cash_refunds
            - total_supplier_payments
            - total_customer_payment_reversals
            + total_supplier_payment_reversals
            + total_cash_in
            - total_cash_out
            - total_paid_out
            - total_cash_drops;

    Ok(CashSessionSummary {
        session,
        total_cash_sales,
        total_cash_refunds,
        total_customer_receipts,
        total_supplier_payments,
        total_customer_payment_reversals,
        total_supplier_payment_reversals,
        total_cash_in,
        total_cash_out,
        total_paid_out,
        total_cash_drops,
        calculated_expected_cash,
    })
}

pub async fn list_movements(
    db: &PgPool,
    company_id: Uuid,
    session_id: i64,
) -> Result<Vec<CashMovement>, CashRegisterError> {
    load_session(db, company_id, session_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT id, register_session_id, movement_type, amount,
               reference, notes, created_by, created_at
        FROM cash_movements
        WHERE company_id = $1 AND register_session_id = $2
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(company_id)
    .bind(session_id)
    .fetch_all(db)
    .await
    .map_err(CashRegisterError::Database)?;

    rows.into_iter()
        .map(|row| {
            Ok(CashMovement {
                id: row.try_get("id").map_err(CashRegisterError::Database)?,
                register_session_id: row
                    .try_get("register_session_id")
                    .map_err(CashRegisterError::Database)?,
                movement_type: row
                    .try_get("movement_type")
                    .map_err(CashRegisterError::Database)?,
                amount: row.try_get("amount").map_err(CashRegisterError::Database)?,
                reference: row
                    .try_get("reference")
                    .map_err(CashRegisterError::Database)?,
                notes: row.try_get("notes").map_err(CashRegisterError::Database)?,
                created_by: row
                    .try_get("created_by")
                    .map_err(CashRegisterError::Database)?,
                created_at: row
                    .try_get("created_at")
                    .map_err(CashRegisterError::Database)?,
            })
        })
        .collect()
}
