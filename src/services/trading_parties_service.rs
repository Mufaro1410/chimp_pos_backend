// src/services/trading_parties_service.rs
use crate::models::trading_parties::*;
use sqlx::{Error, PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub enum TradingPartyError {
    Database(Error),
    NotFound,
    Invalid,
    InvalidQuery,
    VersionConflict,
    Forbidden,
}

fn clean(v: Option<String>) -> Option<String> {
    v.and_then(|s| {
        let t = s.trim().to_owned();
        if t.is_empty() { None } else { Some(t) }
    })
}

fn valid_terms(n: i32) -> bool {
    (0..=3650).contains(&n)
}

fn validate_list_query(
    query: TradingPartyListQuery,
) -> Result<(i64, i64, Option<String>, Option<bool>), TradingPartyError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    if !(1..=100).contains(&limit) || offset < 0 {
        return Err(TradingPartyError::InvalidQuery);
    }

    let search = query
        .search
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if search
        .as_ref()
        .is_some_and(|value| value.chars().count() > 100)
    {
        return Err(TradingPartyError::InvalidQuery);
    }

    Ok((limit, offset, search, query.is_active))
}

pub async fn customers(
    db: &PgPool,
    company: Uuid,
    query: TradingPartyListQuery,
) -> Result<Vec<Customer>, TradingPartyError> {
    let (limit, offset, search, is_active) = validate_list_query(query)?;

    let rows = sqlx::query(
        r#"
        SELECT
            id,
            customer_number,
            btrim(currency_code)::text AS currency_code,
            name,
            email,
            phone,
            address,
            tax_number,
            notes,
            credit_enabled,
            credit_limit,
            payment_terms_days,
            is_active,
            created_at,
            updated_at,
            version
        FROM customers
        WHERE company_id = $1
          AND ($2::BOOLEAN IS NULL OR is_active = $2)
          AND (
                $3::TEXT IS NULL
                OR name ILIKE '%' || $3 || '%'
                OR COALESCE(email, '') ILIKE '%' || $3 || '%'
                OR COALESCE(phone, '') ILIKE '%' || $3 || '%'
          )
        ORDER BY name, id
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(company)
    .bind(is_active)
    .bind(search)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(TradingPartyError::Database)?;

    rows.into_iter().map(customer_from).collect()
}

fn customer_from(r: sqlx::postgres::PgRow) -> Result<Customer, TradingPartyError> {
    Ok(Customer {
        id: r.try_get("id").map_err(TradingPartyError::Database)?,
        customer_number: r
            .try_get("customer_number")
            .map_err(TradingPartyError::Database)?,
        currency_code: r
            .try_get("currency_code")
            .map_err(TradingPartyError::Database)?,
        name: r.try_get("name").map_err(TradingPartyError::Database)?,
        email: r.try_get("email").map_err(TradingPartyError::Database)?,
        phone: r.try_get("phone").map_err(TradingPartyError::Database)?,
        address: r.try_get("address").map_err(TradingPartyError::Database)?,
        tax_number: r
            .try_get("tax_number")
            .map_err(TradingPartyError::Database)?,
        notes: r.try_get("notes").map_err(TradingPartyError::Database)?,
        credit_enabled: r
            .try_get("credit_enabled")
            .map_err(TradingPartyError::Database)?,
        credit_limit: r
            .try_get("credit_limit")
            .map_err(TradingPartyError::Database)?,
        payment_terms_days: r
            .try_get("payment_terms_days")
            .map_err(TradingPartyError::Database)?,
        is_active: r
            .try_get("is_active")
            .map_err(TradingPartyError::Database)?,
        created_at: r
            .try_get("created_at")
            .map_err(TradingPartyError::Database)?,
        updated_at: r
            .try_get("updated_at")
            .map_err(TradingPartyError::Database)?,
        version: r.try_get("version").map_err(TradingPartyError::Database)?,
    })
}

pub async fn create_customer(
    db: &PgPool,
    company: Uuid,
    q: CustomerCreate,
) -> Result<Customer, TradingPartyError> {
    if q.name.trim().is_empty() {
        return Err(TradingPartyError::Invalid);
    }

    let mut tx = db.begin().await.map_err(TradingPartyError::Database)?;

    sqlx::query(
        "INSERT INTO customer_sequences(company_id,next_number) VALUES($1,1) ON CONFLICT DO NOTHING",
    )
    .bind(company)
    .execute(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let number: i64 = sqlx::query_scalar(
        "UPDATE customer_sequences SET next_number=next_number+1 WHERE company_id=$1 RETURNING next_number-1",
    )
    .bind(company)
    .fetch_one(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let row = sqlx::query(
        r#"
        INSERT INTO customers(
            company_id, customer_number, currency_code, name,
            email, phone, address, tax_number, notes
        )
        SELECT $1, $2, c.default_currency, $3, $4, $5, $6, $7, $8
        FROM companies c
        WHERE c.id = $1
        RETURNING
            id, customer_number, btrim(currency_code)::text AS currency_code,
            name, email, phone, address, tax_number, notes,
            credit_enabled, credit_limit, payment_terms_days,
            is_active, created_at, updated_at, version
        "#,
    )
    .bind(company)
    .bind(number)
    .bind(q.name.trim())
    .bind(clean(q.email))
    .bind(clean(q.phone))
    .bind(clean(q.address))
    .bind(clean(q.tax_number))
    .bind(clean(q.notes))
    .fetch_one(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let c = customer_from(row)?;

    tx.commit().await.map_err(TradingPartyError::Database)?;

    Ok(c)
}

pub async fn update_customer(
    db: &PgPool,
    company: Uuid,
    id: i64,
    q: CustomerUpdate,
) -> Result<Customer, TradingPartyError> {
    if q.name.trim().is_empty() {
        return Err(TradingPartyError::Invalid);
    }

    let row = sqlx::query(
        r#"
        UPDATE customers
        SET name=$1, email=$2, phone=$3, address=$4,
            tax_number=$5, notes=$6, is_active=$7,
            updated_at=NOW(), version=version+1
        WHERE id=$8 AND company_id=$9 AND version=$10
        RETURNING
            id, customer_number, btrim(currency_code)::text AS currency_code,
            name, email, phone, address, tax_number, notes,
            credit_enabled, credit_limit, payment_terms_days,
            is_active, created_at, updated_at, version
        "#,
    )
    .bind(q.name.trim())
    .bind(clean(q.email))
    .bind(clean(q.phone))
    .bind(clean(q.address))
    .bind(clean(q.tax_number))
    .bind(clean(q.notes))
    .bind(q.is_active)
    .bind(id)
    .bind(company)
    .bind(q.expected_version)
    .fetch_optional(db)
    .await
    .map_err(TradingPartyError::Database)?;

    if let Some(r) = row {
        return customer_from(r);
    }

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE id=$1 AND company_id=$2)")
            .bind(id)
            .bind(company)
            .fetch_one(db)
            .await
            .map_err(TradingPartyError::Database)?;

    Err(if exists {
        TradingPartyError::VersionConflict
    } else {
        TradingPartyError::NotFound
    })
}

pub async fn update_customer_credit_policy(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    id: i64,
    q: CustomerCreditPolicyUpdate,
) -> Result<Customer, TradingPartyError> {
    if q.credit_limit < rust_decimal::Decimal::ZERO || !valid_terms(q.payment_terms_days) {
        return Err(TradingPartyError::Invalid);
    }

    let mut tx = db.begin().await.map_err(TradingPartyError::Database)?;

    let allowed: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM user_roles ur
            JOIN roles r
              ON r.id=ur.role_id AND r.company_id=ur.company_id
            JOIN role_permissions rp ON rp.role_id=r.id
            JOIN permissions p ON p.id=rp.permission_id
            WHERE ur.user_id=$1
              AND ur.company_id=$2
              AND r.is_active=TRUE
              AND (ur.expires_at IS NULL OR ur.expires_at>NOW())
              AND p.permission_key='customers.credit'
        )
        "#,
    )
    .bind(user)
    .bind(company)
    .fetch_one(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    if !allowed {
        return Err(TradingPartyError::Forbidden);
    }

    let row = sqlx::query(
        r#"
        UPDATE customers
        SET credit_enabled=$1,
            credit_limit=$2,
            payment_terms_days=$3,
            updated_at=NOW(),
            version=version+1
        WHERE id=$4 AND company_id=$5 AND version=$6
        RETURNING
            id, customer_number, btrim(currency_code)::text AS currency_code,
            name, email, phone, address, tax_number, notes,
            credit_enabled, credit_limit, payment_terms_days,
            is_active, created_at, updated_at, version
        "#,
    )
    .bind(q.credit_enabled)
    .bind(q.credit_limit)
    .bind(q.payment_terms_days)
    .bind(id)
    .bind(company)
    .bind(q.expected_version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    if let Some(row) = row {
        let customer = customer_from(row)?;
        tx.commit().await.map_err(TradingPartyError::Database)?;
        return Ok(customer);
    }

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE id=$1 AND company_id=$2)")
            .bind(id)
            .bind(company)
            .fetch_one(&mut *tx)
            .await
            .map_err(TradingPartyError::Database)?;

    Err(if exists {
        TradingPartyError::VersionConflict
    } else {
        TradingPartyError::NotFound
    })
}

pub async fn suppliers(
    db: &PgPool,
    company: Uuid,
    query: TradingPartyListQuery,
) -> Result<Vec<Supplier>, TradingPartyError> {
    let (limit, offset, search, is_active) = validate_list_query(query)?;

    let rows = sqlx::query(
        r#"
        SELECT
            id,
            supplier_number,
            name,
            contact_person,
            email,
            phone,
            address,
            tax_number,
            notes,
            payment_terms_days,
            is_active,
            created_at,
            updated_at,
            version
        FROM suppliers
        WHERE company_id = $1
          AND ($2::BOOLEAN IS NULL OR is_active = $2)
          AND (
                $3::TEXT IS NULL
                OR name ILIKE '%' || $3 || '%'
                OR COALESCE(contact_person, '') ILIKE '%' || $3 || '%'
                OR COALESCE(email, '') ILIKE '%' || $3 || '%'
                OR COALESCE(phone, '') ILIKE '%' || $3 || '%'
          )
        ORDER BY name, id
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(company)
    .bind(is_active)
    .bind(search)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(TradingPartyError::Database)?;

    rows.into_iter().map(supplier_from).collect()
}

fn supplier_from(r: sqlx::postgres::PgRow) -> Result<Supplier, TradingPartyError> {
    Ok(Supplier {
        id: r.try_get("id").map_err(TradingPartyError::Database)?,
        supplier_number: r
            .try_get("supplier_number")
            .map_err(TradingPartyError::Database)?,
        name: r.try_get("name").map_err(TradingPartyError::Database)?,
        contact_person: r
            .try_get("contact_person")
            .map_err(TradingPartyError::Database)?,
        email: r.try_get("email").map_err(TradingPartyError::Database)?,
        phone: r.try_get("phone").map_err(TradingPartyError::Database)?,
        address: r.try_get("address").map_err(TradingPartyError::Database)?,
        tax_number: r
            .try_get("tax_number")
            .map_err(TradingPartyError::Database)?,
        notes: r.try_get("notes").map_err(TradingPartyError::Database)?,
        payment_terms_days: r
            .try_get("payment_terms_days")
            .map_err(TradingPartyError::Database)?,
        is_active: r
            .try_get("is_active")
            .map_err(TradingPartyError::Database)?,
        created_at: r
            .try_get("created_at")
            .map_err(TradingPartyError::Database)?,
        updated_at: r
            .try_get("updated_at")
            .map_err(TradingPartyError::Database)?,
        version: r.try_get("version").map_err(TradingPartyError::Database)?,
    })
}

pub async fn create_supplier(
    db: &PgPool,
    company: Uuid,
    q: SupplierCreate,
) -> Result<Supplier, TradingPartyError> {
    if q.name.trim().is_empty() || !valid_terms(q.payment_terms_days) {
        return Err(TradingPartyError::Invalid);
    }

    let mut tx = db.begin().await.map_err(TradingPartyError::Database)?;

    sqlx::query(
        "INSERT INTO supplier_sequences(company_id,next_number) VALUES($1,1) ON CONFLICT DO NOTHING",
    )
    .bind(company)
    .execute(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let n: i64 = sqlx::query_scalar(
        "UPDATE supplier_sequences SET next_number=next_number+1 WHERE company_id=$1 RETURNING next_number-1",
    )
    .bind(company)
    .fetch_one(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let r = sqlx::query(
        r#"
        INSERT INTO suppliers(
            company_id, supplier_number, name, contact_person, email,
            phone, address, tax_number, notes, payment_terms_days
        )
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING
            id, supplier_number, name, contact_person, email, phone,
            address, tax_number, notes, payment_terms_days,
            is_active, created_at, updated_at, version
        "#,
    )
    .bind(company)
    .bind(n)
    .bind(q.name.trim())
    .bind(clean(q.contact_person))
    .bind(clean(q.email))
    .bind(clean(q.phone))
    .bind(clean(q.address))
    .bind(clean(q.tax_number))
    .bind(clean(q.notes))
    .bind(q.payment_terms_days)
    .fetch_one(&mut *tx)
    .await
    .map_err(TradingPartyError::Database)?;

    let s = supplier_from(r)?;

    tx.commit().await.map_err(TradingPartyError::Database)?;

    Ok(s)
}

pub async fn update_supplier(
    db: &PgPool,
    company: Uuid,
    id: i64,
    q: SupplierUpdate,
) -> Result<Supplier, TradingPartyError> {
    if q.name.trim().is_empty() || !valid_terms(q.payment_terms_days) {
        return Err(TradingPartyError::Invalid);
    }

    let r = sqlx::query(
        r#"
        UPDATE suppliers
        SET name=$1,
            contact_person=$2,
            email=$3,
            phone=$4,
            address=$5,
            tax_number=$6,
            notes=$7,
            payment_terms_days=$8,
            is_active=$9,
            updated_at=NOW(),
            version=version+1
        WHERE id=$10 AND company_id=$11 AND version=$12
        RETURNING
            id, supplier_number, name, contact_person, email, phone,
            address, tax_number, notes, payment_terms_days,
            is_active, created_at, updated_at, version
        "#,
    )
    .bind(q.name.trim())
    .bind(clean(q.contact_person))
    .bind(clean(q.email))
    .bind(clean(q.phone))
    .bind(clean(q.address))
    .bind(clean(q.tax_number))
    .bind(clean(q.notes))
    .bind(q.payment_terms_days)
    .bind(q.is_active)
    .bind(id)
    .bind(company)
    .bind(q.expected_version)
    .fetch_optional(db)
    .await
    .map_err(TradingPartyError::Database)?;

    if let Some(r) = r {
        return supplier_from(r);
    }

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM suppliers WHERE id=$1 AND company_id=$2)")
            .bind(id)
            .bind(company)
            .fetch_one(db)
            .await
            .map_err(TradingPartyError::Database)?;

    Err(if exists {
        TradingPartyError::VersionConflict
    } else {
        TradingPartyError::NotFound
    })
}
