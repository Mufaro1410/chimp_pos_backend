use axum::{
    Router, middleware,
    routing::{get, patch, post, put},
};

use crate::{
    handlers::{
        administration::{
            assign_roles, create_user, get_role, get_shop, get_user, list_permissions, list_roles,
            list_users, update_role_permissions, update_shop, update_user,
        },
        audit::list_audit_logs,
        auth::{login_handler, me_handler, refresh_handler, register},
        catalog::{
            create_category, create_location, list_categories, list_locations, list_units,
            update_category, update_location,
        },
        expenses::{create_expense, expense_summary, get_expense, list_expenses, void_expense},
        financial_reversals::{
            reverse_customer_credit_note, reverse_customer_payment, reverse_supplier_credit_note,
            reverse_supplier_payment, get_customer_payment, get_supplier_payment,
        },
        cash_register::{
            close_session, create_movement, create_register, get_open_session, get_session,
            get_session_summary, list_movements, list_registers, open_session,
        },
        financial_operations::{
            create_supplier_invoice, customer_balance, customer_outstanding, customer_statement,
            get_supplier_invoice, post_customer_credit_note, post_customer_payment,
            post_supplier_credit_note, post_supplier_payment, supplier_balance, supplier_invoices,
            supplier_outstanding, supplier_statement,
        },
        health,
        inventory::{adjust_stock, get_movements, get_stock},
        product::{create_product, delete_product, get_product, get_products, update_product},
        purchasing::{
            cancel_purchase_order, create_purchase_order, get_purchase_order, get_receipts,
            list_purchase_orders, receive_goods, submit_purchase_order, update_purchase_order,
        },
        reporting::{
            cash_movements as report_cash_movements, cash_registers as report_cash_registers,
            cash_sessions as report_cash_sessions, dashboard as report_dashboard,
            goods_received as report_goods_received, gross_profit as report_gross_profit,
            inventory_activity as report_inventory_activity,
            inventory_movements as report_inventory_movements,
            inventory_stock as report_inventory_stock,
            inventory_valuation as report_inventory_valuation,
            outstanding_purchase_orders as report_outstanding_purchase_orders,
            purchasing_by_supplier as report_purchasing_by_supplier,
            purchasing_summary as report_purchasing_summary,
            sales_by_category as report_sales_by_category,
            sales_by_payment_method as report_sales_by_payment_method,
            sales_by_product as report_sales_by_product, sales_returns as report_sales_returns,
            sales_summary as report_sales_summary, sales_transactions as report_sales_transactions,
            supplier_purchase_history as report_supplier_purchase_history,
        },
        sales::{
            cancel_sale, checkout_sale, create_sale, get_receipt, get_sale, list_sales, update_sale,
        },
        sales_returns::{
            create_return, get_void, list_refunds, list_returns, refund_return, void_sale,
        },
        trading_parties::{
            create_customer, create_supplier, list_customers, list_suppliers, update_customer,
            update_customer_credit_policy, update_supplier,
        },
        variant::{
            create_product_variant, deactivate_product_variant, get_product_variants,
            update_product_variant,
        },
    },
    state::SharedState,
    utils::{auth_middleware::auth_middleware, permission_middleware::permission_middleware},
};

pub fn create_routes(state: SharedState) -> Router {
    let administration_routes = Router::new()
        .route("/api/v1/shop", get(get_shop).put(update_shop))
        .route("/api/v1/users", get(list_users).post(create_user))
        .route("/api/v1/users/{id}", get(get_user).put(update_user))
        .route("/api/v1/users/{id}/roles", put(assign_roles))
        .route("/api/v1/roles", get(list_roles))
        .route("/api/v1/roles/{id}", get(get_role))
        .route(
            "/api/v1/roles/{id}/permissions",
            put(update_role_permissions),
        )
        .route("/api/v1/permissions", get(list_permissions))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    // Product routes require authentication AND authorization.
    let product_routes = Router::new()
        .route("/api/v1/products", get(get_products).post(create_product))
        .route(
            "/api/v1/products/{id}",
            get(get_product).put(update_product).delete(delete_product),
        )
        .route(
            "/api/v1/products/{product_id}/variants",
            get(get_product_variants).post(create_product_variant),
        )
        .route(
            "/api/v1/products/{product_id}/variants/{variant_id}",
            put(update_product_variant).delete(deactivate_product_variant),
        )
        .route("/api/v1/inventory/stock", get(get_stock))
        .route("/api/v1/inventory/movements", get(get_movements))
        .route("/api/v1/inventory/adjustments", post(adjust_stock))
        // Check permissions after authentication.
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        // Authentication must execute before permission checking.
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let purchasing_routes = Router::new()
        .route(
            "/api/v1/purchases",
            get(list_purchase_orders).post(create_purchase_order),
        )
        .route(
            "/api/v1/purchases/{id}",
            get(get_purchase_order).put(update_purchase_order),
        )
        .route("/api/v1/purchases/{id}/submit", post(submit_purchase_order))
        .route("/api/v1/purchases/{id}/cancel", post(cancel_purchase_order))
        .route(
            "/api/v1/purchases/{id}/receipts",
            get(get_receipts).post(receive_goods),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let trading_party_routes = Router::new()
        .route(
            "/api/v1/customers",
            get(list_customers).post(create_customer),
        )
        .route("/api/v1/customers/{id}", put(update_customer))
        .route(
            "/api/v1/customers/{id}/credit-policy",
            patch(update_customer_credit_policy),
        )
        .route(
            "/api/v1/suppliers",
            get(list_suppliers).post(create_supplier),
        )
        .route("/api/v1/suppliers/{id}", put(update_supplier))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let financial_routes = Router::new()
        .route("/api/v1/customers/{id}/account", get(customer_balance))
        .route("/api/v1/customers/{id}/statement", get(customer_statement))
        .route(
            "/api/v1/customers/{id}/outstanding-sales",
            get(customer_outstanding),
        )
        .route(
            "/api/v1/customers/{id}/payments",
            post(post_customer_payment),
        )
        .route(
            "/api/v1/customers/{id}/credit-notes",
            post(post_customer_credit_note),
        )
        .route("/api/v1/suppliers/{id}/account", get(supplier_balance))
        .route("/api/v1/suppliers/{id}/statement", get(supplier_statement))
        .route(
            "/api/v1/suppliers/{id}/outstanding-invoices",
            get(supplier_outstanding),
        )
        .route(
            "/api/v1/suppliers/{id}/invoices",
            get(supplier_invoices).post(create_supplier_invoice),
        )
        .route(
            "/api/v1/suppliers/{id}/payments",
            post(post_supplier_payment),
        )
        .route(
            "/api/v1/suppliers/{id}/credit-notes",
            post(post_supplier_credit_note),
        )
        .route("/api/v1/supplier-invoices/{id}", get(get_supplier_invoice))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let reporting_routes = Router::new()
        .route("/api/v1/reports/dashboard", get(report_dashboard))
        .route("/api/v1/reports/sales/summary", get(report_sales_summary))
        .route(
            "/api/v1/reports/sales/by-product",
            get(report_sales_by_product),
        )
        .route(
            "/api/v1/reports/sales/by-category",
            get(report_sales_by_category),
        )
        .route(
            "/api/v1/reports/sales/by-payment-method",
            get(report_sales_by_payment_method),
        )
        .route(
            "/api/v1/reports/sales/transactions",
            get(report_sales_transactions),
        )
        .route("/api/v1/reports/sales/returns", get(report_sales_returns))
        .route(
            "/api/v1/reports/sales/gross-profit",
            get(report_gross_profit),
        )
        .route(
            "/api/v1/reports/inventory/stock",
            get(report_inventory_stock),
        )
        .route(
            "/api/v1/reports/inventory/valuation",
            get(report_inventory_valuation),
        )
        .route(
            "/api/v1/reports/inventory/movements",
            get(report_inventory_movements),
        )
        .route(
            "/api/v1/reports/inventory/activity",
            get(report_inventory_activity),
        )
        .route(
            "/api/v1/reports/purchasing/summary",
            get(report_purchasing_summary),
        )
        .route(
            "/api/v1/reports/purchasing/by-supplier",
            get(report_purchasing_by_supplier),
        )
        .route(
            "/api/v1/reports/purchasing/goods-received",
            get(report_goods_received),
        )
        .route(
            "/api/v1/reports/purchasing/outstanding-orders",
            get(report_outstanding_purchase_orders),
        )
        .route(
            "/api/v1/reports/suppliers/{id}/purchase-history",
            get(report_supplier_purchase_history),
        )
        .route("/api/v1/reports/cash/registers", get(report_cash_registers))
        .route("/api/v1/reports/cash/sessions", get(report_cash_sessions))
        .route("/api/v1/reports/cash/movements", get(report_cash_movements))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let sales_routes = Router::new()
        .route("/api/v1/sales", get(list_sales).post(create_sale))
        .route("/api/v1/sales/{id}", get(get_sale).put(update_sale))
        .route("/api/v1/sales/{id}/checkout", post(checkout_sale))
        .route("/api/v1/sales/{id}/receipt", get(get_receipt))
        .route("/api/v1/sales/{id}/cancel", post(cancel_sale))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let sales_returns_routes = Router::new()
        .route(
            "/api/v1/sales/{id}/returns",
            get(list_returns).post(create_return),
        )
        .route(
            "/api/v1/sales/returns/{return_id}/refunds",
            post(refund_return),
        )
        .route("/api/v1/sales/{id}/refunds", get(list_refunds))
        .route("/api/v1/sales/{id}/void", get(get_void).post(void_sale))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let cash_register_routes = Router::new()
        .route(
            "/api/v1/cash-registers",
            get(list_registers).post(create_register),
        )
        .route(
            "/api/v1/cash-registers/{register_id}/sessions",
            post(open_session),
        )
        .route(
            "/api/v1/cash-registers/{register_id}/sessions/open",
            get(get_open_session),
        )
        .route("/api/v1/cash-sessions/{session_id}", get(get_session))
        .route(
            "/api/v1/cash-sessions/{session_id}/summary",
            get(get_session_summary),
        )
        .route(
            "/api/v1/cash-sessions/{session_id}/movements",
            get(list_movements).post(create_movement),
        )
        .route(
            "/api/v1/cash-sessions/{session_id}/close",
            post(close_session),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            permission_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));


    let layers = |router: Router<SharedState>| {
        router
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                permission_middleware,
            ))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
    };

    let catalog_routes = layers(
        Router::new()
            .route("/api/v1/units", get(list_units))
            .route(
                "/api/v1/categories",
                get(list_categories).post(create_category),
            )
            .route("/api/v1/categories/{id}", put(update_category))
            .route(
                "/api/v1/locations",
                get(list_locations).post(create_location),
            )
            .route("/api/v1/locations/{id}", put(update_location)),
    );

    let expense_routes = layers(
        Router::new()
            .route("/api/v1/expenses", get(list_expenses).post(create_expense))
            .route("/api/v1/expenses/summary", get(expense_summary))
            .route("/api/v1/expenses/{id}", get(get_expense))
            .route("/api/v1/expenses/{id}/void", post(void_expense)),
    );

    let reversal_routes = layers(
        Router::new()
            .route("/api/v1/customer-payments/{id}", get(get_customer_payment))
            .route("/api/v1/supplier-payments/{id}", get(get_supplier_payment))
            .route(
                "/api/v1/customer-payments/{id}/reversals",
                post(reverse_customer_payment),
            )
            .route(
                "/api/v1/supplier-payments/{id}/reversals",
                post(reverse_supplier_payment),
            )
            .route(
                "/api/v1/customer-credit-notes/{id}/reversals",
                post(reverse_customer_credit_note),
            )
            .route(
                "/api/v1/supplier-credit-notes/{id}/reversals",
                post(reverse_supplier_credit_note),
            ),
    );

    let audit_routes = layers(Router::new().route("/api/v1/audit-logs", get(list_audit_logs)));

    // The identity endpoint requires authentication,
    // but does not require a specific business permission.
    let identity_routes = Router::new()
        .route("/api/v1/auth/me", get(me_handler))
        .route("/api/v1/auth/refresh", post(refresh_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .merge(administration_routes)
        .merge(product_routes)
        .merge(identity_routes)
        .merge(purchasing_routes)
        .merge(sales_routes)
        .merge(trading_party_routes)
        .merge(financial_routes)
        .merge(reporting_routes)
        .merge(sales_returns_routes)
        .merge(cash_register_routes)
        .merge(catalog_routes)
        .merge(expense_routes)
        .merge(reversal_routes)
        .merge(audit_routes)
        // Public routes.
        .route("/health", get(health::health_check))
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login_handler))
        .layer(cors_layer())
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(30),
        ))
        .with_state(state)
}

/// CORS for browser / Tauri clients. Set CORS_ALLOWED_ORIGINS to a comma-separated list
/// (e.g. "https://app.example.com,tauri://localhost"). When unset, all origins are allowed
/// (development convenience; set it in production).
fn cors_layer() -> tower_http::cors::CorsLayer {
    use axum::http::{HeaderName, HeaderValue, Method, header};
    use tower_http::cors::{AllowOrigin, CorsLayer};

    let origins = match std::env::var("CORS_ALLOWED_ORIGINS") {
        Ok(list) if !list.trim().is_empty() => AllowOrigin::list(
            list.split(',')
                .filter_map(|o| HeaderValue::from_str(o.trim()).ok()),
        ),
        _ => AllowOrigin::any(),
    };

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static("idempotency-key"),
        ])
        .max_age(std::time::Duration::from_secs(3600))
}

#[cfg(test)]
mod tests {
    use super::create_routes;
    use crate::state::AppState;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn administration_endpoints_require_authentication() {
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://test:test@127.0.0.1/test")
            .expect("lazy pool URL should parse");
        let app = create_routes(Arc::new(AppState {
            db,
            jwt_secret: "test-secret-that-is-long-enough-for-tests".into(),
        }));
        for path in [
            "/api/v1/shop",
            "/api/v1/users",
            "/api/v1/roles",
            "/api/v1/customers/1/account",
            "/api/v1/suppliers/1/payments",
            "/api/v1/reports/dashboard",
            "/api/v1/reports/sales/summary",
            "/api/v1/reports/inventory/stock",
            "/api/v1/reports/purchasing/summary",
            "/api/v1/reports/cash/sessions",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }
}
