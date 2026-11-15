use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    errors::auth_error::AuthError,
    models::auth::{
        LoginRequest, LoginResponse, LoginUserResponse, RegisterRequest, RegisterResponse,
    },
    models::currency::is_valid_currency_code,
    state::AppState,
    utils::password::{hash_password, verify_password},
};

/// Canonical default permission sets. Keep in sync with the
/// `20261113090000_functional_backend_completion.sql` reconciliation migration.
pub const MANAGER_PERMISSIONS: &[&str] = &[
    "dashboard.view",
    "categories.view",
    "categories.manage",
    "customers.view",
    "customers.manage",
    "customers.profile.manage",
    "customers.payment",
    "products.view",
    "products.create",
    "products.update",
    "inventory.view",
    "inventory.adjust",
    "inventory.receive",
    "inventory.transfer",
    "sales.view",
    "sales.create",
    "sales.update",
    "sales.cancel",
    "sales.checkout",
    "sales.discount",
    "sales.return",
    "sales.refund",
    "sales.void",
    "cash_register.view",
    "cash_register.open",
    "cash_register.close",
    "cash_register.movement",
    "suppliers.view",
    "suppliers.manage",
    "suppliers.invoice",
    "suppliers.payment",
    "purchases.view",
    "purchases.create",
    "purchases.update",
    "purchases.cancel",
    "purchases.receive",
    "expenses.view",
    "expenses.manage",
    "reports.view",
    "reports.export",
    "users.view",
    "sync.use",
    "sync.manage",
];

pub const CASHIER_PERMISSIONS: &[&str] = &[
    "dashboard.view",
    "products.view",
    "inventory.view",
    "customers.view",
    "customers.manage",
    "customers.profile.manage",
    "customers.payment",
    "sales.view",
    "sales.create",
    "sales.update",
    "sales.cancel",
    "sales.checkout",
    "cash_register.view",
    "cash_register.open",
    "cash_register.close",
    "cash_register.movement",
    "sync.use",
];

fn map_database_error(error: sqlx::Error) -> AuthError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return AuthError::AlreadyExists;
        }
    }

    AuthError::Database(error)
}

fn validate_registration_request(request: &mut RegisterRequest) -> Result<(), AuthError> {
    // Normalize names
    request.company.name = request.company.name.trim().to_string();
    request.owner.name = request.owner.name.trim().to_string();
    request.owner.username = request.owner.username.trim().to_lowercase();

    if request.company.name.len() < 2 || request.company.name.len() > 120 {
        return Err(AuthError::Validation(
            "Company name must be between 2 and 120 characters".into(),
        ));
    }

    if request.owner.name.chars().count() < 2 || request.owner.name.chars().count() > 120 {
        return Err(AuthError::Validation(
            "Owner name must be between 2 and 120 characters".into(),
        ));
    }

    let username = &request.owner.username;

    if !(3..=32).contains(&username.len())
        || !username
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
    {
        return Err(AuthError::Validation(
            "Username must be 3–32 characters and contain only letters, numbers, dots or underscores".into()
        ));
    }

    // Normalize and validate email
    if let Some(email) = &mut request.owner.email {
        let normalized = email.trim().to_lowercase();

        if normalized.is_empty() {
            request.owner.email = None;
        } else {
            let valid = normalized.len() <= 254
                && normalized.split_once('@').is_some_and(|(local, domain)| {
                    !local.is_empty()
                        && !domain.is_empty()
                        && domain.contains('.')
                        && !domain.starts_with('.')
                        && !domain.ends_with('.')
                });

            if !valid {
                return Err(AuthError::Validation(
                    "Please provide a valid email address".into(),
                ));
            }

            *email = normalized;
        }
    }

    // Password limits
    let password = &request.owner.password;

    if password.chars().count() < 15 || password.len() > 128 {
        return Err(AuthError::Validation(
            "Password must contain at least 15 characters and no more than 128 bytes".into(),
        ));
    }

    // Country code
    if let Some(country) = &mut request.company.country_code {
        *country = country.trim().to_uppercase();

        if country.len() != 2 || !country.bytes().all(|c| c.is_ascii_uppercase()) {
            return Err(AuthError::Validation(
                "Country code must be a valid two-letter code".into(),
            ));
        }
    }

    // Currency code
    if let Some(currency) = &mut request.company.default_currency {
        *currency = currency.trim().to_uppercase();

        if !is_valid_currency_code(currency) {
            return Err(AuthError::Validation(
                "Currency must be a three-letter ISO currency code".into(),
            ));
        }
    }

    // Timezone
    if let Some(timezone) = &request.company.timezone {
        if timezone.trim().parse::<chrono_tz::Tz>().is_err() {
            return Err(AuthError::Validation("Invalid timezone".into()));
        }
    }

    // Basic locale validation
    if let Some(locale) = &mut request.company.locale {
        *locale = locale.trim().to_string();

        let valid = locale.split('-').count() == 2
            && locale
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric()));

        if !valid {
            return Err(AuthError::Validation(
                "Locale must use a language-region format, e.g. en-ZW".into(),
            ));
        }
    }

    Ok(())
}

pub async fn register_company(
    db: &PgPool,
    mut request: RegisterRequest,
) -> Result<RegisterResponse, AuthError> {
    // 1. Validate required fields.
    validate_registration_request(&mut request)?;
    let company_name = request.company.name.trim();
    let owner_name = request.owner.name.trim();
    let username = request.owner.username.trim();

    // 2. Hash the password before opening the transaction.
    let password_hash =
        hash_password(&request.owner.password).map_err(AuthError::PasswordHashing)?;

    // 3. Start the database transaction.
    let mut tx = db.begin().await.map_err(map_database_error)?;

    // 4. Create the company.
    let company_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO companies (
            name,
            business_type,
            country_code,
            timezone,
            locale,
            default_currency
        )
        VALUES (
            $1,
            $2,
            COALESCE($3, 'ZW'),
            COALESCE($4, 'Africa/Harare'),
            COALESCE($5, 'en-ZW'),
            COALESCE($6, 'USD')
        )
        RETURNING id
        "#,
    )
    .bind(company_name)
    .bind(request.company.business_type.as_deref())
    .bind(request.company.country_code.as_deref())
    .bind(request.company.timezone.as_deref())
    .bind(request.company.locale.as_deref())
    .bind(request.company.default_currency.as_deref())
    .fetch_one(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // 4b. Every company needs a default stock location, otherwise products
    // cannot hold stock and sales / receiving / adjustments cannot work.
    sqlx::query(
        r#"
        INSERT INTO inventory_locations (company_id, name, code, location_type, is_default)
        VALUES ($1, 'Main Store', 'MAIN', 'store', TRUE)
        "#,
    )
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // 5. Create the owner account.
    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (
            company_id,
            name,
            username,
            email,
            password_hash
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(company_id)
    .bind(owner_name)
    .bind(username)
    .bind(request.owner.email.as_deref())
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // 6. Create the four default system roles.
    let default_roles = [
        ("Owner", "Full access to all business operations.", "owner"),
        ("Admin", "Business and staff administration.", "admin"),
        ("Manager", "Day-to-day business operations.", "manager"),
        (
            "Cashier",
            "Sales and customer service operations.",
            "cashier",
        ),
    ];

    let mut created_roles = 0;

    for (name, description, role_key) in default_roles {
        sqlx::query(
            r#"
            INSERT INTO roles (
                company_id,
                name,
                description,
                is_system_role,
                is_active,
                role_key
            )
            VALUES ($1, $2, $3, TRUE, TRUE, $4)
            "#,
        )
        .bind(company_id)
        .bind(name)
        .bind(description)
        .bind(role_key)
        .execute(&mut *tx)
        .await
        .map_err(map_database_error)?;

        created_roles += 1;
    }

    if created_roles != 4 {
        return Err(AuthError::Internal);
    }

    // 7. Assign permissions to each role.
    sqlx::query(
        r#"
        INSERT INTO role_permissions (role_id, permission_id)
        SELECT r.id, p.id
        FROM roles r
        CROSS JOIN permissions p
        WHERE r.company_id = $1
        AND r.is_system_role = TRUE
        AND r.role_key = 'owner'
        "#,
    )
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // Admin: all permissions except role and settings management.
    sqlx::query(
        r#"
        INSERT INTO role_permissions (role_id, permission_id)
        SELECT r.id, p.id
        FROM roles r
        CROSS JOIN permissions p
        WHERE r.company_id = $1
        AND r.is_system_role = TRUE
        AND r.role_key = 'admin'
        AND p.permission_key NOT IN ('roles.manage', 'settings.manage')
        "#,
    )
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // Manager permissions.
    sqlx::query(
        r#"
        INSERT INTO role_permissions (role_id, permission_id)
        SELECT r.id, p.id
        FROM roles r
        JOIN permissions p ON p.permission_key = ANY($2::text[])
        WHERE r.company_id = $1
        AND r.is_system_role = TRUE
        AND r.role_key = 'manager'
        "#,
    )
    .bind(company_id)
    .bind(MANAGER_PERMISSIONS.to_vec())
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // Cashier permissions.
    sqlx::query(
        r#"
        INSERT INTO role_permissions (role_id, permission_id)
        SELECT r.id, p.id
        FROM roles r
        JOIN permissions p ON p.permission_key = ANY($2::text[])
        WHERE r.company_id = $1
        AND r.is_system_role = TRUE
        AND r.role_key = 'cashier'
        "#,
    )
    .bind(company_id)
    .bind(CASHIER_PERMISSIONS.to_vec())
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    // 7.1 Verify the critical permission grants for every default role.
    // Avoid a hard-coded total permission count: new modules can add permissions
    // without requiring this registration workflow to be rewritten.
    let default_roles_valid: bool = sqlx::query_scalar(
        r#"
        SELECT
            (SELECT COUNT(*) FROM roles
             WHERE company_id = $1 AND is_system_role = TRUE
               AND role_key IN ('owner', 'admin', 'manager', 'cashier')) = 4
            AND NOT EXISTS (
                SELECT 1
                FROM (VALUES
                    ('owner', 'cash_register.view'),
                    ('owner', 'cash_register.manage'),
                    ('owner', 'cash_register.open'),
                    ('owner', 'cash_register.close'),
                    ('owner', 'cash_register.movement'),
                    ('admin', 'cash_register.view'),
                    ('admin', 'cash_register.manage'),
                    ('admin', 'cash_register.open'),
                    ('admin', 'cash_register.close'),
                    ('admin', 'cash_register.movement'),
                    ('manager', 'cash_register.view'),
                    ('manager', 'cash_register.open'),
                    ('manager', 'cash_register.close'),
                    ('manager', 'cash_register.movement'),
                    ('manager', 'sales.checkout'),
                    ('manager', 'sales.return'),
                    ('cashier', 'cash_register.view'),
                    ('cashier', 'cash_register.open'),
                    ('cashier', 'cash_register.close'),
                    ('cashier', 'cash_register.movement'),
                    ('cashier', 'sales.checkout')
                ) AS required(role_key, permission_key)
                JOIN roles r
                  ON r.company_id = $1
                 AND r.is_system_role = TRUE
                 AND r.role_key = required.role_key
                LEFT JOIN permissions p
                  ON p.permission_key = required.permission_key
                LEFT JOIN role_permissions rp
                  ON rp.role_id = r.id
                 AND rp.permission_id = p.id
                WHERE p.id IS NULL OR rp.role_id IS NULL
            )
        "#,
    )
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_database_error)?;

    if !default_roles_valid {
        return Err(AuthError::Internal);
    }

    // 8. Assign the Owner role to the newly created user.
    let owner_assignment = sqlx::query(
        r#"
        INSERT INTO user_roles(company_id, user_id, role_id, assigned_by)
        SELECT $1, $2, r.id, $2
        FROM roles r
        WHERE r.company_id = $1
        AND r.role_key = 'owner'
        AND r.is_system_role = TRUE
        "#,
    )
    .bind(company_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    if owner_assignment.rows_affected() != 1 {
        return Err(AuthError::Internal);
    }

    // 9. Commit the transaction.
    tx.commit().await.map_err(map_database_error)?;

    Ok(RegisterResponse {
        company_id,
        company_name: company_name.to_string(),
        user_id,
        owner_name: owner_name.to_string(),
        username: username.to_string(),
        message: "Company registered successfully".into(),
    })
}

#[derive(Debug, sqlx::FromRow)]
struct LoginUser {
    id: Uuid,
    company_id: Uuid,
    name: String,
    username: String,
    password_hash: String,
    status: String,
    locked: bool,
    must_change_password: bool,
    token_version: i32,
}

fn lockout_policy() -> (i32, i32) {
    let max = std::env::var("LOGIN_MAX_ATTEMPTS")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v >= 3)
        .unwrap_or(5);
    let minutes = std::env::var("LOGIN_LOCKOUT_MINUTES")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(15);
    (max, minutes)
}

/// A valid Argon2 hash used to spend the same time on unknown users as on real ones, so response
/// timing does not reveal which usernames exist.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password("timing-equaliser-not-a-real-password").unwrap_or_default())
}

pub async fn login(state: &AppState, request: LoginRequest) -> Result<LoginResponse, AuthError> {
    // 1. Validate the request.
    let username = request.username.trim().to_lowercase();

    if username.is_empty() || request.password.is_empty() || request.password.len() > 1024 {
        return Err(AuthError::Unauthorized);
    }

    // 2. Find the user within the specified company.
    let user = sqlx::query_as::<_, LoginUser>(
        r#"
        SELECT id, company_id, name, username, password_hash, status,
               (locked_until IS NOT NULL AND locked_until > NOW()) AS locked,
               must_change_password, token_version
        FROM users
        WHERE company_id = $1
          AND LOWER(username) = $2
        LIMIT 1
        "#,
    )
    .bind(request.company_id)
    .bind(&username)
    .fetch_optional(&state.db)
    .await
    .map_err(map_database_error)?;

    // 3. Unknown or inactive account: burn equivalent hashing time, then fail generically.
    let Some(user) = user.filter(|u| u.status == "active") else {
        let password = request.password;
        let _ = tokio::task::spawn_blocking(move || verify_password(&password, dummy_hash())).await;
        return Err(AuthError::Unauthorized);
    };

    // 4. Temporarily locked after repeated failures.
    if user.locked {
        return Err(AuthError::TooManyAttempts);
    }

    // 5. Verify the password without blocking Tokio's async runtime.
    let password = request.password;
    let stored_hash = user.password_hash.clone();

    let password_valid =
        tokio::task::spawn_blocking(move || verify_password(&password, &stored_hash))
            .await
            .map_err(|_| AuthError::Internal)?
            .map_err(AuthError::PasswordHashing)?;

    if !password_valid {
        let (max_attempts, lock_minutes) = lockout_policy();
        sqlx::query(
            r#"
            UPDATE users
            SET failed_login_attempts = CASE WHEN failed_login_attempts + 1 >= $3 THEN 0 ELSE failed_login_attempts + 1 END,
                locked_until = CASE WHEN failed_login_attempts + 1 >= $3
                                    THEN NOW() + make_interval(mins => $4) ELSE locked_until END
            WHERE id = $1 AND company_id = $2
            "#,
        )
        .bind(user.id)
        .bind(user.company_id)
        .bind(max_attempts)
        .bind(lock_minutes)
        .execute(&state.db)
        .await
        .map_err(map_database_error)?;
        return Err(AuthError::Unauthorized);
    }

    // 6. Success: clear lockout state, start a session (refresh-token family).
    let mut tx = state.db.begin().await.map_err(map_database_error)?;
    sqlx::query(
        "UPDATE users SET last_login_at = NOW(), failed_login_attempts = 0, locked_until = NULL          WHERE id = $1 AND company_id = $2",
    )
    .bind(user.id)
    .bind(user.company_id)
    .execute(&mut *tx)
    .await
    .map_err(map_database_error)?;

    let device = request
        .device_label
        .as_deref()
        .map(|d| d.trim().chars().take(100).collect::<String>())
        .filter(|d| !d.is_empty());
    let refresh_token = crate::services::session_service::insert_refresh_token(
        &mut tx,
        user.company_id,
        user.id,
        Uuid::new_v4(),
        device.as_deref(),
    )
    .await
    .map_err(map_database_error)?;
    tx.commit().await.map_err(map_database_error)?;

    // 7. Issue the access token and return the login response.
    let access_token = crate::utils::jwt::generate_access_token_versioned(
        user.id,
        user.company_id,
        &state.jwt_secret,
        user.token_version,
    )?;

    Ok(LoginResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: crate::utils::jwt::token_ttl_seconds(),
        refresh_token,
        refresh_expires_in: crate::services::session_service::refresh_ttl_days() as u64 * 86_400,
        must_change_password: user.must_change_password,
        user: LoginUserResponse {
            id: user.id,
            company_id: user.company_id,
            name: user.name,
            username: user.username,
        },
    })
}
