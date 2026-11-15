//! Sessions and password lifecycle.
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    errors::api_error::{ApiError, ApiResult},
    models::session::*,
    utils::{
        jwt::{generate_access_token_versioned, token_ttl_seconds},
        password::{hash_password, verify_password},
        tokens::{new_opaque_token, sha256_hex},
    },
};

/// A refresh token that was just rotated may be presented again for this long (a client that
/// lost the response to its refresh call retries with the old token). Beyond it, reuse is
/// treated as theft and the whole login session is revoked. (REFRESH_ROTATION_GRACE_SECONDS)
fn rotation_grace_secs() -> i64 {
    std::env::var("REFRESH_ROTATION_GRACE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| (0..=300).contains(v))
        .unwrap_or(30)
}
const RESET_TOKEN_MINUTES: i64 = 30;

pub fn refresh_ttl_days() -> i32 {
    std::env::var("REFRESH_TTL_DAYS")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| (1..=365).contains(v))
        .unwrap_or(30)
}

fn unauthorized() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::UNAUTHORIZED,
        "INVALID_REFRESH_TOKEN",
        "The session has expired. Please sign in again",
    )
}

pub fn validate_new_password(password: &str, username: &str) -> ApiResult<()> {
    let len = password.chars().count();
    if !(15..=128).contains(&len) {
        return Err(ApiError::bad_request(
            "WEAK_PASSWORD",
            "Password must be between 15 and 128 characters",
        ));
    }
    if password.trim().is_empty() || password.eq_ignore_ascii_case(username) {
        return Err(ApiError::bad_request(
            "WEAK_PASSWORD",
            "Password must not be blank or equal to the username",
        ));
    }
    Ok(())
}

async fn hash(password: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|e| {
            tracing::error!("password hashing failed: {e}");
            ApiError::internal()
        })
}

async fn verify(password: String, stored: String) -> ApiResult<bool> {
    tokio::task::spawn_blocking(move || verify_password(&password, &stored))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|e| {
            tracing::error!("password verification failed: {e}");
            ApiError::internal()
        })
}

fn clean_label(label: &Option<String>) -> Option<String> {
    label
        .as_ref()
        .map(|s| s.trim().chars().take(100).collect::<String>())
        .filter(|s| !s.is_empty())
}

/// Creates a refresh token in `family`. Returns the raw token (shown to the client once).
pub async fn insert_refresh_token(
    conn: &mut PgConnection,
    company: Uuid,
    user: Uuid,
    family: Uuid,
    device: Option<&str>,
) -> Result<String, sqlx::Error> {
    let raw = new_opaque_token();
    sqlx::query(
        "INSERT INTO refresh_tokens (company_id, user_id, family_id, token_hash, device_label, expires_at) \
         VALUES ($1,$2,$3,$4,$5, NOW() + make_interval(days => $6))",
    )
    .bind(company)
    .bind(user)
    .bind(family)
    .bind(sha256_hex(&raw))
    .bind(device)
    .bind(refresh_ttl_days())
    .execute(conn)
    .await?;
    Ok(raw)
}

fn token_response(
    secret: &str,
    company: Uuid,
    user: Uuid,
    token_version: i32,
    refresh: String,
    must_change: bool,
) -> ApiResult<TokenResponse> {
    let access =
        generate_access_token_versioned(user, company, secret, token_version).map_err(|e| {
            tracing::error!("access token generation failed: {e:?}");
            ApiError::internal()
        })?;
    Ok(TokenResponse {
        access_token: access,
        token_type: "Bearer".into(),
        expires_in: token_ttl_seconds(),
        refresh_token: refresh,
        refresh_expires_in: refresh_ttl_days() as u64 * 86_400,
        must_change_password: must_change,
    })
}

async fn revoke_user_tokens(
    conn: &mut PgConnection,
    company: Uuid,
    user: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = NOW(), revoked_reason = $3 \
         WHERE user_id = $1 AND company_id = $2 AND revoked_at IS NULL",
    )
    .bind(user)
    .bind(company)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}

// ------------------------------------------------------------------- refresh

pub async fn refresh(db: &PgPool, secret: &str, req: RefreshRequest) -> ApiResult<TokenResponse> {
    let raw = req.refresh_token.trim();
    if raw.is_empty() || raw.len() > 200 {
        return Err(unauthorized());
    }
    let mut tx = db.begin().await?;

    #[derive(sqlx::FromRow)]
    struct Row {
        id: i64,
        company_id: Uuid,
        user_id: Uuid,
        family_id: Uuid,
        expired: bool,
        in_grace: bool,
        revoked_reason: Option<String>,
        device_label: Option<String>,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, company_id, user_id, family_id, expires_at <= NOW() AS expired, \
                (rotated_at IS NOT NULL AND NOW() - rotated_at <= make_interval(secs => $2)) AS in_grace, \
                revoked_reason, device_label \
         FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(sha256_hex(raw))
    .bind(rotation_grace_secs() as f64)
    .fetch_optional(&mut *tx)
    .await?;
    let row = row.ok_or_else(unauthorized)?;

    if row.expired {
        return Err(unauthorized());
    }
    match row.revoked_reason.as_deref() {
        None => {}
        Some("rotated") if row.in_grace => {}
        Some("rotated") => {
            // A token that was already exchanged is being replayed: assume it leaked.
            tracing::warn!(user = %row.user_id, "refresh token reuse detected; revoking session family");
            sqlx::query(
                "UPDATE refresh_tokens SET revoked_at = NOW(), revoked_reason = 'reuse_detected' \
                 WHERE family_id = $1 AND revoked_at IS NULL",
            )
            .bind(row.family_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Err(unauthorized());
        }
        Some(_) => return Err(unauthorized()),
    }

    let account: Option<(String, String, bool, i32)> = sqlx::query_as(
        "SELECT u.status, c.status, u.must_change_password, u.token_version FROM users u \
         JOIN companies c ON c.id = u.company_id WHERE u.id = $1 AND u.company_id = $2",
    )
    .bind(row.user_id)
    .bind(row.company_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_status, company_status, must_change, token_version)) = account else {
        return Err(unauthorized());
    };
    if user_status != "active" || company_status != "active" {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = NOW(), revoked_reason = 'account_inactive' \
             WHERE family_id = $1 AND revoked_at IS NULL",
        )
        .bind(row.family_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Err(unauthorized());
    }

    if row.revoked_reason.is_none() {
        sqlx::query(
            "UPDATE refresh_tokens SET rotated_at = NOW(), revoked_at = NOW(), revoked_reason = 'rotated' WHERE id = $1",
        )
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
    }
    let label = clean_label(&req.device_label).or(row.device_label);
    let new_refresh = insert_refresh_token(
        &mut tx,
        row.company_id,
        row.user_id,
        row.family_id,
        label.as_deref(),
    )
    .await?;
    tx.commit().await?;

    token_response(
        secret,
        row.company_id,
        row.user_id,
        token_version,
        new_refresh,
        must_change,
    )
}

pub async fn logout(db: &PgPool, req: LogoutRequest) -> ApiResult<()> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = NOW(), revoked_reason = 'logout' \
         WHERE revoked_at IS NULL AND family_id = \
           (SELECT family_id FROM refresh_tokens WHERE token_hash = $1)",
    )
    .bind(sha256_hex(req.refresh_token.trim()))
    .execute(db)
    .await?;
    Ok(())
}

pub async fn logout_all(db: &PgPool, company: Uuid, user: Uuid) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    revoke_user_tokens(&mut tx, company, user, "logout_all").await?;
    // Also kill outstanding access tokens, not just refresh tokens.
    sqlx::query(
        "UPDATE users SET token_version = token_version + 1 WHERE id = $1 AND company_id = $2",
    )
    .bind(user)
    .bind(company)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

// ----------------------------------------------------------------- passwords

pub async fn change_password(
    db: &PgPool,
    secret: &str,
    company: Uuid,
    user: Uuid,
    req: ChangePasswordRequest,
) -> ApiResult<TokenResponse> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT password_hash, username FROM users WHERE id = $1 AND company_id = $2 AND status = 'active'",
    )
    .bind(user)
    .bind(company)
    .fetch_optional(db)
    .await?;
    let (stored, username) = row.ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Account not available",
        )
    })?;

    validate_new_password(&req.new_password, &username)?;
    if !verify(req.current_password.clone(), stored).await? {
        return Err(ApiError::bad_request(
            "INVALID_CURRENT_PASSWORD",
            "The current password is incorrect",
        ));
    }
    if req.new_password == req.current_password {
        return Err(ApiError::bad_request(
            "SAME_PASSWORD",
            "The new password must differ from the current one",
        ));
    }
    let new_hash = hash(req.new_password).await?;

    let mut tx = db.begin().await?;
    let new_version: i32 = sqlx::query_scalar(
        "UPDATE users SET password_hash = $3, must_change_password = FALSE, password_changed_at = NOW(), \
         failed_login_attempts = 0, locked_until = NULL, token_version = token_version + 1, version = version + 1, updated_at = NOW() \
         WHERE id = $1 AND company_id = $2 RETURNING token_version",
    )
    .bind(user)
    .bind(company)
    .bind(new_hash)
    .fetch_one(&mut *tx)
    .await?;
    // Every other device must sign in again with the new password.
    revoke_user_tokens(&mut tx, company, user, "password_changed").await?;
    let refresh = insert_refresh_token(
        &mut tx,
        company,
        user,
        Uuid::new_v4(),
        Some("password change"),
    )
    .await?;
    tx.commit().await?;
    token_response(secret, company, user, new_version, refresh, false)
}

pub async fn admin_reset_password(
    db: &PgPool,
    company: Uuid,
    actor: Uuid,
    target: Uuid,
    req: AdminResetPasswordRequest,
) -> ApiResult<()> {
    if actor == target {
        return Err(ApiError::bad_request(
            "USE_CHANGE_PASSWORD",
            "Use change-password for your own account",
        ));
    }
    let username: Option<String> =
        sqlx::query_scalar("SELECT username FROM users WHERE id = $1 AND company_id = $2")
            .bind(target)
            .bind(company)
            .fetch_optional(db)
            .await?;
    let username =
        username.ok_or_else(|| ApiError::not_found("USER_NOT_FOUND", "User not found"))?;
    validate_new_password(&req.new_password, &username)?;

    // Only an owner may reset an owner's password.
    let is_owner = |user: Uuid| async move {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id AND r.company_id = ur.company_id \
             WHERE ur.user_id = $1 AND ur.company_id = $2 AND r.role_key = 'owner' AND r.is_system_role)",
        )
        .bind(user)
        .bind(company)
        .fetch_one(db)
        .await
    };
    if is_owner(target).await? && !is_owner(actor).await? {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "OWNER_PROTECTED",
            "Only an owner can reset an owner's password",
        ));
    }

    let new_hash = hash(req.new_password).await?;
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE users SET password_hash = $3, must_change_password = $4, password_changed_at = NOW(), \
         failed_login_attempts = 0, locked_until = NULL, token_version = token_version + 1, version = version + 1, updated_at = NOW() \
         WHERE id = $1 AND company_id = $2",
    )
    .bind(target)
    .bind(company)
    .bind(new_hash)
    .bind(req.require_change)
    .execute(&mut *tx)
    .await?;
    revoke_user_tokens(&mut tx, company, target, "admin_reset").await?;
    tx.commit().await?;
    Ok(())
}

/// Always succeeds from the caller's point of view (no account enumeration).
/// Delivery is pluggable; only `PASSWORD_RESET_DELIVERY=log` (development) is implemented:
/// the token is written to the server log. Wire email/SMS here for production.
pub async fn forgot_password(db: &PgPool, req: ForgotPasswordRequest) -> ApiResult<()> {
    let mode = std::env::var("PASSWORD_RESET_DELIVERY").unwrap_or_default();
    if mode != "log" {
        return Ok(());
    }
    let ident = req.identifier.trim().to_lowercase();
    if ident.is_empty() || ident.len() > 255 {
        return Ok(());
    }
    let user: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, username FROM users WHERE company_id = $1 AND status = 'active' \
         AND (lower(username) = $2 OR lower(email) = $2) LIMIT 1",
    )
    .bind(req.company_id)
    .bind(&ident)
    .fetch_optional(db)
    .await?;
    let Some((user_id, username)) = user else {
        return Ok(());
    };

    let token = new_opaque_token();
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE password_reset_tokens SET used_at = NOW() WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO password_reset_tokens (company_id, user_id, token_hash, expires_at) \
         VALUES ($1,$2,$3, NOW() + make_interval(mins => $4))",
    )
    .bind(req.company_id)
    .bind(user_id)
    .bind(sha256_hex(&token))
    .bind(RESET_TOKEN_MINUTES as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::warn!(target: "password_reset", "PASSWORD RESET TOKEN for '{username}' (valid {RESET_TOKEN_MINUTES} min): {token}");
    Ok(())
}

pub async fn reset_password(db: &PgPool, req: ResetPasswordRequest) -> ApiResult<()> {
    let invalid = || {
        ApiError::bad_request(
            "INVALID_RESET_TOKEN",
            "The reset link is invalid or has expired",
        )
    };
    let token = req.token.trim();
    if token.is_empty() || token.len() > 200 {
        return Err(invalid());
    }
    let mut tx = db.begin().await?;
    let row: Option<(i64, Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, user_id, company_id FROM password_reset_tokens \
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > NOW() FOR UPDATE",
    )
    .bind(sha256_hex(token))
    .fetch_optional(&mut *tx)
    .await?;
    let (id, user, company) = row.ok_or_else(invalid)?;
    let username: String = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1 AND company_id = $2 AND status = 'active'",
    )
    .bind(user)
    .bind(company)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(invalid)?;
    validate_new_password(&req.new_password, &username)?;
    let new_hash = hash(req.new_password).await?;
    sqlx::query(
        "UPDATE users SET password_hash = $3, must_change_password = FALSE, password_changed_at = NOW(), \
         failed_login_attempts = 0, locked_until = NULL, token_version = token_version + 1, version = version + 1, updated_at = NOW() \
         WHERE id = $1 AND company_id = $2",
    )
    .bind(user)
    .bind(company)
    .bind(new_hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    revoke_user_tokens(&mut tx, company, user, "password_reset").await?;
    tx.commit().await?;
    Ok(())
}

/// Operator recovery (CLI): set a user's password directly, clear lockout, end all sessions.
pub async fn cli_reset_password(
    db: &PgPool,
    company: Uuid,
    username: &str,
    new_password: String,
) -> ApiResult<()> {
    let user: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE company_id = $1 AND lower(username) = lower($2)",
    )
    .bind(company)
    .bind(username)
    .fetch_optional(db)
    .await?;
    let user =
        user.ok_or_else(|| ApiError::not_found("USER_NOT_FOUND", "No such user in that company"))?;
    validate_new_password(&new_password, username)?;
    let new_hash = hash(new_password).await?;
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE users SET password_hash = $3, must_change_password = FALSE, password_changed_at = NOW(), \
         failed_login_attempts = 0, locked_until = NULL, status = 'active', token_version = token_version + 1, version = version + 1, updated_at = NOW() \
         WHERE id = $1 AND company_id = $2",
    )
    .bind(user)
    .bind(company)
    .bind(new_hash)
    .execute(&mut *tx)
    .await?;
    revoke_user_tokens(&mut tx, company, user, "cli_reset").await?;
    tx.commit().await?;
    Ok(())
}

/// Current user, role-derived permissions and password state (for client UI gating).
pub async fn me(db: &PgPool, company: Uuid, user: Uuid) -> ApiResult<Value> {
    let row: Option<(
        String,
        String,
        Option<String>,
        String,
        bool,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT name, username, email, status, must_change_password, last_login_at \
         FROM users WHERE id = $1 AND company_id = $2",
    )
    .bind(user)
    .bind(company)
    .fetch_optional(db)
    .await?;
    let (name, username, email, status, must_change, last_login) =
        row.ok_or_else(|| ApiError::not_found("USER_NOT_FOUND", "User not found"))?;
    let permissions: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(jsonb_agg(DISTINCT p.permission_key), '[]'::jsonb)
           FROM user_roles ur
           JOIN roles r ON r.id = ur.role_id AND r.company_id = ur.company_id
           JOIN role_permissions rp ON rp.role_id = r.id
           JOIN permissions p ON p.id = rp.permission_id
           WHERE ur.user_id = $1 AND ur.company_id = $2 AND r.is_active = TRUE
             AND (ur.expires_at IS NULL OR ur.expires_at > NOW())"#,
    )
    .bind(user)
    .bind(company)
    .fetch_one(db)
    .await?;
    let roles: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(jsonb_agg(DISTINCT r.role_key), '[]'::jsonb)
           FROM user_roles ur JOIN roles r ON r.id = ur.role_id AND r.company_id = ur.company_id
           WHERE ur.user_id = $1 AND ur.company_id = $2 AND r.is_active = TRUE
             AND (ur.expires_at IS NULL OR ur.expires_at > NOW())"#,
    )
    .bind(user)
    .bind(company)
    .fetch_one(db)
    .await?;
    Ok(json!({
        "user_id": user,
        "company_id": company,
        "name": name,
        "username": username,
        "email": email,
        "status": status,
        "must_change_password": must_change,
        "last_login_at": last_login,
        "roles": roles,
        "permissions": permissions,
    }))
}

/// Housekeeping: remove expired/used token rows and old finished sync journal entries.
pub async fn cleanup(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < NOW() - INTERVAL '7 days'")
        .execute(db)
        .await?;
    sqlx::query(
        "DELETE FROM password_reset_tokens WHERE expires_at < NOW() - INTERVAL '1 day' OR used_at < NOW() - INTERVAL '1 day'",
    )
    .execute(db)
    .await?;
    let retention_days: i32 = std::env::var("SYNC_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &i32| *v >= 7)
        .unwrap_or(90);
    sqlx::query(
        "DELETE FROM sync_operations WHERE status IN ('applied','dismissed') \
         AND processed_at < NOW() - make_interval(days => $1)",
    )
    .bind(retention_days)
    .execute(db)
    .await?;
    Ok(())
}
