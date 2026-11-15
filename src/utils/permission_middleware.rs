use axum::{
    extract::{Extension, State},
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::{models::auth::AuthUser, state::SharedState};

fn required_permission(method: &Method, path: &str) -> Option<&'static str> {
    match (method, path) {
        // ---- catalogue lookups
        (&Method::GET, "/api/v1/units") => Some("products.view"),
        (&Method::GET, "/api/v1/categories") => Some("categories.view"),
        (&Method::POST, "/api/v1/categories") => Some("categories.manage"),
        (&Method::PUT, p) if p.starts_with("/api/v1/categories/") => Some("categories.manage"),
        (&Method::GET, "/api/v1/locations") => Some("inventory.view"),
        (&Method::POST, "/api/v1/locations") => Some("settings.manage"),
        (&Method::PUT, p) if p.starts_with("/api/v1/locations/") => Some("settings.manage"),
        // ---- expenses
        (&Method::GET, "/api/v1/expenses") => Some("expenses.view"),
        (&Method::GET, p) if p.starts_with("/api/v1/expenses/") => Some("expenses.view"),
        (&Method::POST, "/api/v1/expenses") => Some("expenses.manage"),
        (&Method::POST, p) if p.starts_with("/api/v1/expenses/") && p.ends_with("/void") => {
            Some("expenses.manage")
        }
        // ---- password administration
        (&Method::POST, p) if p.starts_with("/api/v1/users/") && p.ends_with("/reset-password") => {
            Some("users.manage")
        }
        // ---- offline sync
        (&Method::GET, "/api/v1/sync/bootstrap") => Some("sync.use"),
        (&Method::GET, p) if p.starts_with("/api/v1/sync/pull/") => Some("sync.use"),
        (&Method::POST, "/api/v1/sync/push") => Some("sync.use"),
        (&Method::GET, "/api/v1/sync/operations") => Some("sync.use"),
        (&Method::POST, p) if p.starts_with("/api/v1/sync/operations/") => Some("sync.manage"),
        // ---- audit
        (&Method::GET, "/api/v1/audit-logs") => Some("audit_logs.view"),
        // ---- financial reversals
        (&Method::GET, p) if p.starts_with("/api/v1/customer-payments/") => Some("customers.view"),
        (&Method::GET, p) if p.starts_with("/api/v1/supplier-payments/") => Some("suppliers.view"),
        (&Method::POST, p)
            if p.starts_with("/api/v1/customer-payments/") && p.ends_with("/reversals") =>
        {
            Some("customers.payment")
        }
        (&Method::POST, p)
            if p.starts_with("/api/v1/supplier-payments/") && p.ends_with("/reversals") =>
        {
            Some("suppliers.payment")
        }
        (&Method::POST, p)
            if p.starts_with("/api/v1/customer-credit-notes/") && p.ends_with("/reversals") =>
        {
            Some("customers.credit")
        }
        (&Method::POST, p)
            if p.starts_with("/api/v1/supplier-credit-notes/") && p.ends_with("/reversals") =>
        {
            Some("suppliers.invoice")
        }
        // Shop and administration endpoints
        (&Method::GET, "/api/v1/shop") => Some("settings.view"),
        (&Method::PUT, "/api/v1/shop") => Some("settings.manage"),
        (&Method::GET, "/api/v1/users") => Some("users.view"),
        (&Method::GET, p) if p.starts_with("/api/v1/users/") && !p.ends_with("/roles") => {
            Some("users.view")
        }
        (&Method::POST, "/api/v1/users") => Some("users.manage"),
        (&Method::PUT, p) if p.starts_with("/api/v1/users/") && p.ends_with("/roles") => {
            Some("roles.manage")
        }
        (&Method::PUT, p) if p.starts_with("/api/v1/users/") => Some("users.manage"),
        (&Method::GET, "/api/v1/roles") => Some("roles.view"),
        (&Method::GET, "/api/v1/permissions") => Some("roles.view"),
        (&Method::GET, p) if p.starts_with("/api/v1/roles/") => Some("roles.view"),
        (&Method::PUT, p) if p.starts_with("/api/v1/roles/") && p.ends_with("/permissions") => {
            Some("roles.manage")
        }
        // Product permissions
        (&Method::GET, "/api/v1/products") => Some("products.view"),

        (&Method::POST, "/api/v1/products") => Some("products.create"),

        (&Method::POST, p) if p.starts_with("/api/v1/products/") && p.ends_with("/variants") => {
            Some("products.create")
        }

        (&Method::GET, p) if p.starts_with("/api/v1/products/") => Some("products.view"),

        (&Method::PUT, p) if p.starts_with("/api/v1/products/") => Some("products.update"),

        (&Method::DELETE, p) if p.starts_with("/api/v1/products/") => Some("products.archive"),

        // Inventory permissions
        (&Method::GET, "/api/v1/inventory/stock") => Some("inventory.view"),

        (&Method::GET, "/api/v1/inventory/movements") => Some("inventory.view"),

        (&Method::POST, "/api/v1/inventory/adjustments") => Some("inventory.adjust"),

        // Purchasing permissions
        (&Method::GET, "/api/v1/purchases") => Some("purchases.view"),
        (&Method::POST, "/api/v1/purchases") => Some("purchases.create"),

        (&Method::GET, p) if p.starts_with("/api/v1/purchases/") => {
            if p.ends_with("/submit") {
                Some("purchases.update")
            } else if p.ends_with("/cancel") {
                Some("purchases.cancel")
            } else if p.ends_with("/receipts") {
                Some("purchases.view")
            } else {
                Some("purchases.view")
            }
        }

        (&Method::PUT, p) if p.starts_with("/api/v1/purchases/") => Some("purchases.update"),

        (&Method::POST, p) if p.starts_with("/api/v1/purchases/") => {
            if p.ends_with("/submit") {
                Some("purchases.update")
            } else if p.ends_with("/cancel") {
                Some("purchases.cancel")
            } else if p.ends_with("/receipts") {
                Some("purchases.receive")
            } else {
                None
            }
        }

        // Sales permissions
        (&Method::GET, "/api/v1/reports") => Some("reports.view"),
        (&Method::GET, p) if p.starts_with("/api/v1/reports/") => Some("reports.view"),
        (&Method::GET, "/api/v1/sales") => Some("sales.view"),
        (&Method::POST, "/api/v1/sales") => Some("sales.create"),

        (&Method::GET, p)
            if p.starts_with("/api/v1/sales/")
                && !p.ends_with("/checkout")
                && !p.ends_with("/cancel") =>
        {
            Some("sales.view")
        }

        (&Method::PUT, p) if p.starts_with("/api/v1/sales/") => Some("sales.update"),

        (&Method::POST, p) if p.starts_with("/api/v1/sales/") && p.ends_with("/checkout") => {
            Some("sales.checkout")
        }

        (&Method::POST, p) if p.starts_with("/api/v1/sales/") && p.ends_with("/cancel") => {
            Some("sales.cancel")
        }

        // Sales Retuns, Void, Refund permissions
        (&Method::POST, p)
            if p.starts_with("/api/v1/sales/")
                && p.ends_with("/returns")
                && p.split('/').count() == 6 =>
        {
            Some("sales.return")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/sales/")
                && p.ends_with("/returns")
                && p.split('/').count() == 6 =>
        {
            Some("sales.view")
        }

        (&Method::POST, p)
            if p.starts_with("/api/v1/sales/returns/") && p.ends_with("/refunds") =>
        {
            Some("sales.refund")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/sales/")
                && p.ends_with("/refunds")
                && p.split('/').count() == 6 =>
        {
            Some("sales.view")
        }

        (&Method::POST, p)
            if p.starts_with("/api/v1/sales/")
                && p.ends_with("/void")
                && p.split('/').count() == 6 =>
        {
            Some("sales.void")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/sales/")
                && p.ends_with("/void")
                && p.split('/').count() == 6 =>
        {
            Some("sales.view")
        }

        // Customer and supplier management
        (&Method::GET, "/api/v1/customers") => Some("customers.view"),
        (&Method::POST, "/api/v1/customers") => Some("customers.profile.manage"),
        (&Method::PUT, p) if p.starts_with("/api/v1/customers/") => {
            Some("customers.profile.manage")
        }
        (&Method::PATCH, p)
            if p.starts_with("/api/v1/customers/") && p.ends_with("/credit-policy") =>
        {
            Some("customers.credit")
        }
        (&Method::GET, p)
            if p.starts_with("/api/v1/customers/")
                && (p.ends_with("/account")
                    || p.ends_with("/statement")
                    || p.ends_with("/outstanding-sales")) =>
        {
            Some("customers.view")
        }
        (&Method::POST, p) if p.starts_with("/api/v1/customers/") && p.ends_with("/payments") => {
            Some("customers.payment")
        }
        (&Method::POST, p)
            if p.starts_with("/api/v1/customers/") && p.ends_with("/credit-notes") =>
        {
            Some("customers.credit")
        }
        (&Method::GET, "/api/v1/suppliers") => Some("suppliers.view"),
        (&Method::POST, "/api/v1/suppliers") => Some("suppliers.manage"),
        (&Method::PUT, p) if p.starts_with("/api/v1/suppliers/") => Some("suppliers.manage"),
        (&Method::GET, p)
            if p.starts_with("/api/v1/suppliers/")
                && (p.ends_with("/account")
                    || p.ends_with("/statement")
                    || p.ends_with("/outstanding-invoices")
                    || p.ends_with("/invoices")) =>
        {
            Some("suppliers.view")
        }
        (&Method::POST, p) if p.starts_with("/api/v1/suppliers/") && p.ends_with("/invoices") => {
            Some("suppliers.invoice")
        }
        (&Method::POST, p) if p.starts_with("/api/v1/suppliers/") && p.ends_with("/payments") => {
            Some("suppliers.payment")
        }
        (&Method::POST, p)
            if p.starts_with("/api/v1/suppliers/") && p.ends_with("/credit-notes") =>
        {
            Some("suppliers.invoice")
        }
        (&Method::GET, p) if p.starts_with("/api/v1/supplier-invoices/") => Some("suppliers.view"),

        // Cash register permissions
        (&Method::GET, "/api/v1/cash-registers") => Some("cash_register.view"),

        (&Method::POST, "/api/v1/cash-registers") => Some("cash_register.manage"),

        (&Method::POST, p)
            if p.starts_with("/api/v1/cash-registers/") && p.ends_with("/sessions") =>
        {
            Some("cash_register.open")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/cash-registers/") && p.ends_with("/sessions/open") =>
        {
            Some("cash_register.view")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/cash-sessions/")
                && (p.ends_with("/summary") || p.ends_with("/movements")) =>
        {
            Some("cash_register.view")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/cash-sessions/")
                && !p.ends_with("/movements")
                && !p.ends_with("/summary") =>
        {
            Some("cash_register.view")
        }

        (&Method::GET, p)
            if p.starts_with("/api/v1/cash-sessions/") && p.ends_with("/movements") =>
        {
            Some("cash_register.view")
        }

        (&Method::POST, p)
            if p.starts_with("/api/v1/cash-sessions/") && p.ends_with("/movements") =>
        {
            Some("cash_register.movement")
        }

        (&Method::POST, p) if p.starts_with("/api/v1/cash-sessions/") && p.ends_with("/close") => {
            Some("cash_register.close")
        }

        _ => None,
    }
}

pub async fn permission_middleware(
    State(state): State<SharedState>,
    Extension(auth): Extension<AuthUser>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    use crate::errors::api_error::status_response;

    let method = request.method().clone();
    let path = request.uri().path().to_string();

    let Some(permission) = required_permission(&method, &path) else {
        return status_response(StatusCode::FORBIDDEN);
    };

    let has_permission: Result<bool, sqlx::Error> = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM user_roles ur
            JOIN roles r
                ON r.id = ur.role_id AND r.company_id = ur.company_id
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
    .bind(auth.user_id)
    .bind(auth.company_id)
    .bind(permission)
    .fetch_one(&state.db)
    .await;

    match has_permission {
        Ok(true) => {}
        Ok(false) => return status_response(StatusCode::FORBIDDEN),
        Err(err) => {
            eprintln!("Permission lookup failed: {err}");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let response = next.run(request).await;

    // Audit trail: record every successful state-changing request (no bodies, no query strings).
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        && response.status().is_success()
    {
        let db = state.db.clone();
        let status = response.status().as_u16() as i32;
        let method = method.to_string();
        tokio::spawn(async move {
            if let Err(err) = sqlx::query(
                "INSERT INTO audit_logs (company_id, user_id, method, path, status_code) \
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(auth.company_id)
            .bind(auth.user_id)
            .bind(method)
            .bind(path)
            .bind(status)
            .execute(&db)
            .await
            {
                eprintln!("Audit log write failed: {err}");
            }
        });
    }

    response
}

#[cfg(test)]
mod tests {
    use super::required_permission;
    use axum::http::Method;

    #[test]
    fn administration_paths_map_to_existing_rbac_permissions() {
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/shop"),
            Some("settings.view")
        );
        assert_eq!(
            required_permission(&Method::PUT, "/api/v1/shop"),
            Some("settings.manage")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/users"),
            Some("users.manage")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/users/abc"),
            Some("users.view")
        );
        assert_eq!(
            required_permission(&Method::PUT, "/api/v1/users/abc/roles"),
            Some("roles.manage")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/roles/abc"),
            Some("roles.view")
        );
        assert_eq!(
            required_permission(&Method::PUT, "/api/v1/roles/abc/permissions"),
            Some("roles.manage")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/permissions"),
            Some("roles.view")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/customers/4/account"),
            Some("customers.view")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/customers/4/statement"),
            Some("customers.view")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/customers/4/payments"),
            Some("customers.payment")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/customers/4/credit-notes"),
            Some("customers.credit")
        );
        assert_eq!(
            required_permission(&Method::GET, "/api/v1/suppliers/4/outstanding-invoices"),
            Some("suppliers.view")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/suppliers/4/invoices"),
            Some("suppliers.invoice")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/suppliers/4/payments"),
            Some("suppliers.payment")
        );
        assert_eq!(
            required_permission(&Method::POST, "/api/v1/suppliers/4/credit-notes"),
            Some("suppliers.invoice")
        );
        assert_eq!(
            required_permission(&Method::DELETE, "/api/v1/users/abc"),
            None
        );
        for path in [
            "/api/v1/reports/dashboard",
            "/api/v1/reports/sales/summary",
            "/api/v1/reports/inventory/valuation",
            "/api/v1/reports/purchasing/outstanding-orders",
            "/api/v1/reports/cash/sessions",
        ] {
            assert_eq!(
                required_permission(&Method::GET, path),
                Some("reports.view"),
                "{path}"
            );
        }
    }
}
