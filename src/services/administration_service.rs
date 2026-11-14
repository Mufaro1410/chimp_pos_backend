use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    models::{administration::*, currency::is_valid_currency_code},
    utils::password::hash_password,
};

#[derive(Debug)]
pub enum AdminError {
    NotFound,
    Invalid(&'static str),
    Conflict(&'static str),
    Database(sqlx::Error),
    Internal,
}
#[derive(sqlx::FromRow)]
struct RoleRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    role_key: Option<String>,
    is_system_role: bool,
    is_active: bool,
    version: i32,
}
impl From<sqlx::Error> for AdminError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

fn clean(value: &str, min: usize, max: usize) -> Result<String, AdminError> {
    let value = value.trim();
    if !(min..=max).contains(&value.chars().count()) {
        return Err(AdminError::Invalid(
            "A text field is outside its allowed length",
        ));
    }
    Ok(value.to_owned())
}
fn optional(value: &mut Option<String>, max: usize) -> Result<(), AdminError> {
    if let Some(original) = value.as_ref() {
        let trimmed = original.trim();
        if trimmed.is_empty() {
            *value = None;
        } else if trimmed.chars().count() > max {
            return Err(AdminError::Invalid(
                "A text field exceeds its maximum length",
            ));
        } else {
            *value = Some(trimmed.to_owned());
        }
    }
    Ok(())
}
fn valid_email(email: &Option<String>) -> bool {
    email.as_ref().is_none_or(|e| {
        e.len() <= 254
            && e.split_once('@').is_some_and(|(a, b)| {
                !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.')
            })
    })
}

pub async fn shop(db: &PgPool, company: Uuid) -> Result<Shop, AdminError> {
    sqlx::query_as::<_,Shop>(r#"SELECT id, name, business_type, description, email, phone, address, city, country_code::text AS country_code, timezone, locale, default_currency::text AS default_currency, status, version, updated_at FROM companies WHERE id=$1"#).bind(company)
        .fetch_optional(db).await?.ok_or(AdminError::NotFound)
}
pub async fn update_shop(
    db: &PgPool,
    company: Uuid,
    mut q: ShopUpdate,
) -> Result<Shop, AdminError> {
    q.name = clean(&q.name, 2, 150)?;
    for (v, max) in [
        (&mut q.business_type, 80),
        (&mut q.description, 4000),
        (&mut q.email, 254),
        (&mut q.phone, 30),
        (&mut q.address, 2000),
        (&mut q.city, 100),
    ] {
        optional(v, max)?;
    }
    if !valid_email(&q.email) {
        return Err(AdminError::Invalid("Email address is invalid"));
    }
    q.country_code = q.country_code.trim().to_ascii_uppercase();
    q.default_currency = q.default_currency.trim().to_ascii_uppercase();
    if q.country_code.len() != 2
        || !q.country_code.bytes().all(|b| b.is_ascii_uppercase())
        || !is_valid_currency_code(&q.default_currency)
        || q.timezone.parse::<chrono_tz::Tz>().is_err()
        || !q
            .locale
            .split('-')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        || q.locale.split('-').count() != 2
    {
        return Err(AdminError::Invalid(
            "Country, currency, timezone or locale is invalid",
        ));
    }
    let mut tx = db.begin().await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT default_currency::text FROM companies WHERE id=$1 FOR UPDATE")
            .bind(company)
            .fetch_optional(&mut *tx)
            .await?;
    let current = current.ok_or(AdminError::NotFound)?;
    if current.trim() != q.default_currency {
        let has_financial_history: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM products WHERE company_id=$1) OR EXISTS(SELECT 1 FROM customers WHERE company_id=$1) OR EXISTS(SELECT 1 FROM suppliers WHERE company_id=$1) OR EXISTS(SELECT 1 FROM sales WHERE company_id=$1) OR EXISTS(SELECT 1 FROM purchase_orders WHERE company_id=$1) OR EXISTS(SELECT 1 FROM cash_register_sessions WHERE company_id=$1) OR EXISTS(SELECT 1 FROM customer_payments WHERE company_id=$1) OR EXISTS(SELECT 1 FROM supplier_payments WHERE company_id=$1) OR EXISTS(SELECT 1 FROM supplier_invoices WHERE company_id=$1)").bind(company).fetch_one(&mut *tx).await?;
        if has_financial_history {
            return Err(AdminError::Conflict(
                "Default currency cannot be changed after financial or purchasing records exist",
            ));
        }
    }
    let row=sqlx::query_as::<_,Shop>(r#"UPDATE companies SET name=$1,business_type=$2,description=$3,email=$4,phone=$5,address=$6,city=$7,country_code=$8,timezone=$9,locale=$10,default_currency=$11,updated_at=NOW(),version=version+1 WHERE id=$12 AND version=$13 RETURNING id,name,business_type,description,email,phone,address,city,country_code::text AS country_code,timezone,locale,default_currency::text AS default_currency,status,version,updated_at"#).bind(q.name).bind(q.business_type).bind(q.description).bind(q.email).bind(q.phone).bind(q.address).bind(q.city).bind(q.country_code).bind(q.timezone).bind(q.locale).bind(q.default_currency).bind(company).bind(q.expected_version).fetch_optional(&mut *tx).await?.ok_or(AdminError::Conflict("Shop version is stale"))?;
    tx.commit().await?;
    Ok(row)
}

async fn permissions_for_role(
    db: impl sqlx::Executor<'_, Database = Postgres>,
    role: Uuid,
) -> Result<Vec<Permission>, sqlx::Error> {
    sqlx::query_as::<_,Permission>("SELECT p.id,p.permission_key,p.resource,p.action,p.description FROM permissions p JOIN role_permissions rp ON rp.permission_id=p.id WHERE rp.role_id=$1 ORDER BY p.permission_key").bind(role).fetch_all(db).await
}
async fn load_user(db: &PgPool, company: Uuid, id: Uuid) -> Result<ManagedUser, AdminError> {
    let mut u=sqlx::query_as::<_,ManagedUser>("SELECT id,company_id,name,email,username,status,last_login_at,created_at,updated_at,version FROM users WHERE company_id=$1 AND id=$2").bind(company).bind(id).fetch_optional(db).await?.ok_or(AdminError::NotFound)?;
    u.roles=sqlx::query_as::<_,RoleSummary>("SELECT r.id,r.name,r.role_key,r.is_system_role FROM roles r JOIN user_roles ur ON ur.role_id=r.id AND ur.company_id=r.company_id WHERE ur.user_id=$1 AND ur.company_id=$2 AND r.is_active=TRUE ORDER BY r.name").bind(id).bind(company).fetch_all(db).await?;
    Ok(u)
}
pub async fn users(
    db: &PgPool,
    company: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Page<ManagedUser>, AdminError> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);
    let ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM users WHERE company_id=$1 ORDER BY created_at DESC,id LIMIT $2 OFFSET $3",
    )
    .bind(company)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(load_user(db, company, id).await?);
    }
    Ok(Page {
        items,
        limit,
        offset,
    })
}
pub async fn user(db: &PgPool, company: Uuid, id: Uuid) -> Result<ManagedUser, AdminError> {
    load_user(db, company, id).await
}

async fn validate_roles(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    role_ids: &[Uuid],
) -> Result<Vec<Uuid>, AdminError> {
    let mut ids = role_ids.to_vec();
    ids.sort();
    ids.dedup();
    if ids.len() != role_ids.len() {
        return Err(AdminError::Invalid("Role IDs must be unique"));
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM roles WHERE company_id=$1 AND id=ANY($2) AND is_active=TRUE",
    )
    .bind(company)
    .bind(&ids)
    .fetch_one(&mut **tx)
    .await?;
    if count != ids.len() as i64 {
        return Err(AdminError::Invalid(
            "One or more roles are unavailable in this shop",
        ));
    }
    Ok(ids)
}
async fn is_admin_user(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    user: Uuid,
) -> Result<bool, AdminError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN user_roles ur ON ur.user_id=u.id AND ur.company_id=u.company_id JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE u.id=$1 AND u.company_id=$2 AND u.status='active' AND r.is_active AND p.permission_key='users.manage' AND (ur.expires_at IS NULL OR ur.expires_at>NOW()))").bind(user).bind(company).fetch_one(&mut **tx).await?)
}
async fn ensure_admin_remains(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    target: Uuid,
    will_be_admin: bool,
) -> Result<(), AdminError> {
    if is_admin_user(tx, company, target).await? && !will_be_admin {
        let count:i64=sqlx::query_scalar("SELECT COUNT(DISTINCT u.id) FROM users u JOIN user_roles ur ON ur.user_id=u.id AND ur.company_id=u.company_id JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE u.company_id=$1 AND u.status='active' AND u.id<>$2 AND r.is_active AND p.permission_key='users.manage' AND (ur.expires_at IS NULL OR ur.expires_at>NOW())").bind(company).bind(target).fetch_one(&mut **tx).await?;
        if count == 0 {
            return Err(AdminError::Conflict(
                "Cannot remove the shop's last active administrator",
            ));
        }
    }
    Ok(())
}
async fn roles_include_admin(
    tx: &mut Transaction<'_, Postgres>,
    roles: &[Uuid],
) -> Result<bool, AdminError> {
    if roles.is_empty() {
        return Ok(false);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.role_id=ANY($1) AND p.permission_key='users.manage')").bind(roles).fetch_one(&mut **tx).await?)
}
pub async fn create_user(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    mut q: UserCreate,
) -> Result<ManagedUser, AdminError> {
    q.name = clean(&q.name, 2, 150)?;
    q.username = q.username.trim().to_ascii_lowercase();
    if !(3..=32).contains(&q.username.len())
        || !q
            .username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
        || q.initial_password.len() < 15
        || q.initial_password.len() > 128
    {
        return Err(AdminError::Invalid(
            "Username or initial password does not meet policy",
        ));
    }
    optional(&mut q.email, 254)?;
    if !valid_email(&q.email) {
        return Err(AdminError::Invalid("Email address is invalid"));
    }
    let hash = hash_password(&q.initial_password).map_err(|_| AdminError::Internal)?;
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AdminError::NotFound)?;
    let roles = validate_roles(&mut tx, company, &q.role_ids).await?;
    if roles.is_empty() {
        return Err(AdminError::Invalid("At least one active role is required"));
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO users(company_id,name,username,email,password_hash) VALUES($1,$2,$3,$4,$5) RETURNING id")
        .bind(company).bind(q.name).bind(q.username).bind(q.email).bind(hash)
        .fetch_one(&mut *tx).await.map_err(map_unique)?;
    for role in roles {
        sqlx::query(
            "INSERT INTO user_roles(company_id,user_id,role_id,assigned_by) VALUES($1,$2,$3,$4)",
        )
        .bind(company)
        .bind(id)
        .bind(role)
        .bind(actor)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    load_user(db, company, id).await
}
fn map_unique(e: sqlx::Error) -> AdminError {
    if matches!(&e,sqlx::Error::Database(d) if d.code().as_deref()==Some("23505")) {
        AdminError::Conflict("Username or email is already in use")
    } else {
        AdminError::Database(e)
    }
}
pub async fn update_user(
    db: &PgPool,
    company: Uuid,
    id: Uuid,
    mut q: UserUpdate,
) -> Result<ManagedUser, AdminError> {
    q.name = clean(&q.name, 2, 150)?;
    optional(&mut q.email, 254)?;
    if !valid_email(&q.email) || !matches!(q.status.as_str(), "active" | "inactive" | "suspended") {
        return Err(AdminError::Invalid("User email or status is invalid"));
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AdminError::NotFound)?;
    if id == Uuid::nil() {
        return Err(AdminError::NotFound);
    }
    let remains_admin = q.status == "active" && is_admin_user(&mut tx, company, id).await?;
    ensure_admin_remains(&mut tx, company, id, remains_admin).await?;
    let changed=sqlx::query("UPDATE users SET name=$1,email=$2,status=$3,updated_at=NOW(),version=version+1 WHERE id=$4 AND company_id=$5 AND version=$6").bind(q.name).bind(q.email).bind(q.status).bind(id).bind(company).bind(q.expected_version).execute(&mut *tx).await.map_err(map_unique)?.rows_affected();
    if changed == 0 {
        return Err(AdminError::Conflict(
            "User version is stale or the user does not exist",
        ));
    }
    tx.commit().await?;
    load_user(db, company, id).await
}
pub async fn assign_roles(
    db: &PgPool,
    company: Uuid,
    user: Uuid,
    actor: Uuid,
    q: UserRoleUpdate,
) -> Result<ManagedUser, AdminError> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AdminError::NotFound)?;
    let target_status: Option<String> =
        sqlx::query_scalar("SELECT status FROM users WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(company)
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?;
    let status = target_status.ok_or(AdminError::NotFound)?;
    let roles = validate_roles(&mut tx, company, &q.role_ids).await?;
    let will_admin = status == "active" && roles_include_admin(&mut tx, &roles).await?;
    ensure_admin_remains(&mut tx, company, user, will_admin).await?;
    sqlx::query("DELETE FROM user_roles WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    for role in roles {
        sqlx::query(
            "INSERT INTO user_roles(company_id,user_id,role_id,assigned_by) VALUES($1,$2,$3,$4)",
        )
        .bind(company)
        .bind(user)
        .bind(role)
        .bind(actor)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    load_user(db, company, user).await
}

pub async fn roles(db: &PgPool, company: Uuid) -> Result<Vec<Role>, AdminError> {
    let rows=sqlx::query_as::<_, RoleRow>("SELECT id,name,description,role_key,is_system_role,is_active,version FROM roles WHERE company_id=$1 ORDER BY is_system_role DESC,name").bind(company).fetch_all(db).await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(Role {
            id: r.id,
            name: r.name,
            description: r.description,
            role_key: r.role_key,
            is_system_role: r.is_system_role,
            is_active: r.is_active,
            version: r.version,
            permissions: permissions_for_role(db, r.id).await?,
        });
    }
    Ok(out)
}
pub async fn role(db: &PgPool, company: Uuid, id: Uuid) -> Result<Role, AdminError> {
    let r=sqlx::query_as::<_, RoleRow>("SELECT id,name,description,role_key,is_system_role,is_active,version FROM roles WHERE company_id=$1 AND id=$2").bind(company).bind(id).fetch_optional(db).await?.ok_or(AdminError::NotFound)?;
    Ok(Role {
        id: r.id,
        name: r.name,
        description: r.description,
        role_key: r.role_key,
        is_system_role: r.is_system_role,
        is_active: r.is_active,
        version: r.version,
        permissions: permissions_for_role(db, r.id).await?,
    })
}
pub async fn permission_catalog(db: &PgPool) -> Result<Vec<Permission>, AdminError> {
    Ok(sqlx::query_as::<_, Permission>("SELECT id,permission_key,resource,action,description FROM permissions ORDER BY resource,action").fetch_all(db).await?)
}
pub async fn update_role_permissions(
    db: &PgPool,
    company: Uuid,
    id: Uuid,
    q: RolePermissionUpdate,
) -> Result<Role, AdminError> {
    let mut p = q.permission_ids.clone();
    p.sort();
    p.dedup();
    if p.len() != q.permission_ids.len() {
        return Err(AdminError::Invalid("Permission IDs must be unique"));
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AdminError::NotFound)?;
    let role_state = sqlx::query_as::<_, (bool, i32)>(
        "SELECT is_system_role,version FROM roles WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(company)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AdminError::NotFound)?;
    if role_state.0 {
        return Err(AdminError::Conflict("Built-in roles cannot be edited"));
    }
    if role_state.1 != q.expected_version {
        return Err(AdminError::Conflict("Role version is stale"));
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM permissions WHERE id=ANY($1)")
        .bind(&p)
        .fetch_one(&mut *tx)
        .await?;
    if count != p.len() as i64 {
        return Err(AdminError::Invalid("One or more permissions do not exist"));
    }
    let forbidden:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM permissions WHERE id=ANY($1) AND permission_key IN ('roles.manage','settings.manage'))").bind(&p).fetch_one(&mut *tx).await?;
    if forbidden {
        return Err(AdminError::Invalid(
            "Custom roles cannot receive owner-only permissions",
        ));
    }
    let removes_user_admin = !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM permissions WHERE id=ANY($1) AND permission_key='users.manage')",
        )
        .bind(&p)
        .fetch_one(&mut *tx)
        .await?;
    if removes_user_admin {
        let existing_grant: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id=rp.permission_id WHERE rp.role_id=$1 AND p.permission_key='users.manage')",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if existing_grant {
            let remaining: i64 = sqlx::query_scalar(
                "SELECT COUNT(DISTINCT u.id) FROM users u JOIN user_roles ur ON ur.user_id=u.id AND ur.company_id=u.company_id JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id JOIN role_permissions rp ON rp.role_id=r.id JOIN permissions p ON p.id=rp.permission_id WHERE u.company_id=$1 AND u.status='active' AND r.is_active AND p.permission_key='users.manage' AND (ur.expires_at IS NULL OR ur.expires_at>NOW()) AND r.id<>$2",
            )
            .bind(company)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if remaining == 0 {
                return Err(AdminError::Conflict(
                    "Cannot remove users.manage from the last active administrator role",
                ));
            }
        }
    }
    sqlx::query("DELETE FROM role_permissions WHERE role_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for perm in p {
        sqlx::query("INSERT INTO role_permissions(role_id,permission_id) VALUES($1,$2)")
            .bind(id)
            .bind(perm)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE roles SET updated_at=NOW(),version=version+1 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    role(db, company, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn email_validation_rejects_malformed_values() {
        assert!(valid_email(&Some("person@example.com".into())));
        assert!(!valid_email(&Some("bad".into())));
    }
    #[test]
    fn identifiers_are_normalized_by_caller_validation() {
        assert!(clean(" Shop ", 2, 150).unwrap() == "Shop");
        assert!(clean(" ", 2, 150).is_err());
    }
}
