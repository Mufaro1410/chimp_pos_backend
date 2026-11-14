//! Opt-in admin integration scenarios. Uses only an already-provisioned
//! disposable database, refuses other database names, and never runs migrations.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chimpPOS_backend::{
    models::administration::{RolePermissionUpdate, UserCreate, UserRoleUpdate, UserUpdate},
    routes,
    services::administration_service,
    state::AppState,
    utils::jwt::generate_access_token,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{env, sync::Arc};
use tower::ServiceExt;
use uuid::Uuid;

async fn setup() -> Option<PgPool> {
    let url = env::var("CHIMPPOS_ADMIN_TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect admin disposable DB");
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("database name");
    assert!(
        name.starts_with("chimp_pos_admin_test"),
        "refusing a database without the chimp_pos_admin_test prefix"
    );
    Some(pool)
}

#[tokio::test]
async fn administration_tenant_permission_user_role_and_admin_invariants() {
    let Some(db) = setup().await else {
        eprintln!(
            "skipped: configure CHIMPPOS_ADMIN_TEST_DATABASE_URL for a disposable PostgreSQL database"
        );
        return;
    };
    let company = Uuid::new_v4();
    let other_company = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let foreign_user = Uuid::new_v4();
    let pw_hash = "integration-test-hash-not-used-for-login";
    sqlx::query("INSERT INTO companies(id,name) VALUES($1,'Admin integration test'),($2,'Other integration test')")
        .bind(company).bind(other_company).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO users(id,company_id,name,username,password_hash) VALUES($1,$2,'Test Owner',$3,$4),($5,$6,'Foreign User',$7,$4)")
        .bind(actor).bind(company).bind(format!("owner_{}", &company.to_string()[..8])).bind(pw_hash)
        .bind(foreign_user).bind(other_company).bind(format!("foreign_{}", &other_company.to_string()[..8])).execute(&db).await.unwrap();
    let admin_role = create_role(
        &db,
        company,
        "Admin integration",
        &[
            "users.view",
            "users.manage",
            "roles.view",
            "roles.manage",
            "settings.view",
            "settings.manage",
        ],
    )
    .await;
    let viewer_role = create_role(
        &db,
        company,
        "Viewer integration",
        &["users.view", "roles.view", "settings.view"],
    )
    .await;
    let custom_role = create_role(&db, company, "Custom integration", &["products.view"]).await;
    let foreign_role =
        create_role(&db, other_company, "Foreign integration", &["users.manage"]).await;
    sqlx::query(
        "INSERT INTO user_roles(company_id,user_id,role_id,assigned_by) VALUES($1,$2,$3,$2)",
    )
    .bind(company)
    .bind(actor)
    .bind(admin_role)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_roles(company_id,user_id,role_id,assigned_by) VALUES($1,$2,$3,$2)",
    )
    .bind(other_company)
    .bind(foreign_user)
    .bind(foreign_role)
    .execute(&db)
    .await
    .unwrap();

    assert!(
        administration_service::user(&db, company, foreign_user)
            .await
            .is_err()
    );
    assert!(
        administration_service::role(&db, company, foreign_role)
            .await
            .is_err()
    );
    assert!(
        administration_service::assign_roles(
            &db,
            company,
            actor,
            actor,
            UserRoleUpdate {
                role_ids: vec![foreign_role]
            }
        )
        .await
        .is_err()
    );

    let shop = administration_service::shop(&db, company).await.unwrap();
    let shop = administration_service::update_shop(
        &db,
        company,
        chimpPOS_backend::models::administration::ShopUpdate {
            expected_version: shop.version,
            name: "Renamed integration shop".into(),
            business_type: None,
            description: None,
            email: None,
            phone: None,
            address: Some("Test address".into()),
            city: Some("Test city".into()),
            country_code: "ZW".into(),
            timezone: "Africa/Harare".into(),
            locale: "en-ZW".into(),
            default_currency: "USD".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(shop.name, "Renamed integration shop");
    assert!(
        administration_service::update_shop(
            &db,
            company,
            chimpPOS_backend::models::administration::ShopUpdate {
                expected_version: shop.version - 1,
                name: shop.name.clone(),
                business_type: None,
                description: None,
                email: None,
                phone: None,
                address: None,
                city: None,
                country_code: "ZW".into(),
                timezone: "Africa/Harare".into(),
                locale: "en-ZW".into(),
                default_currency: "USD".into(),
            }
        )
        .await
        .is_err()
    );

    let created = administration_service::create_user(
        &db,
        company,
        actor,
        UserCreate {
            name: "New Clerk".into(),
            username: format!("clerk_{}", &company.to_string()[..8]),
            email: None,
            initial_password: "long-test-password-123".into(),
            role_ids: vec![viewer_role],
        },
    )
    .await
    .unwrap();
    assert_eq!(created.roles.len(), 1);
    let updated = administration_service::update_user(
        &db,
        company,
        created.id,
        UserUpdate {
            expected_version: created.version,
            name: "Updated Clerk".into(),
            email: Some("clerk@example.test".into()),
            status: "active".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "Updated Clerk");
    let reassigned = administration_service::assign_roles(
        &db,
        company,
        created.id,
        actor,
        UserRoleUpdate {
            role_ids: vec![custom_role],
        },
    )
    .await
    .unwrap();
    assert_eq!(reassigned.roles[0].id, custom_role);
    let permissions = administration_service::update_role_permissions(
        &db,
        company,
        custom_role,
        RolePermissionUpdate {
            expected_version: 1,
            permission_ids: sqlx::query_scalar(
                "SELECT id FROM permissions WHERE permission_key='products.view'",
            )
            .fetch_all(&db)
            .await
            .unwrap(),
        },
    )
    .await
    .unwrap();
    assert_eq!(permissions.permissions.len(), 1);
    assert!(
        administration_service::create_user(
            &db,
            company,
            actor,
            UserCreate {
                name: "Invalid".into(),
                username: "bad username".into(),
                email: None,
                initial_password: "short".into(),
                role_ids: vec![viewer_role],
            }
        )
        .await
        .is_err()
    );

    // The sole active administrator cannot deactivate themselves.
    assert!(
        administration_service::update_user(
            &db,
            company,
            actor,
            UserUpdate {
                expected_version: 1,
                name: "Test Owner".into(),
                email: None,
                status: "inactive".into(),
            }
        )
        .await
        .is_err()
    );
    assert!(
        administration_service::assign_roles(
            &db,
            company,
            actor,
            actor,
            UserRoleUpdate {
                role_ids: vec![viewer_role]
            },
        )
        .await
        .is_err()
    );

    // Authentication is required and existing RBAC permissions gate the route.
    let secret = "admin-integration-secret-that-is-at-least-32-bytes";
    let app = routes::create_routes(Arc::new(AppState {
        db: db.clone(),
        jwt_secret: secret.into(),
    }));
    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let token = generate_access_token(actor, company, secret).unwrap();
    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let target_token = generate_access_token(created.id, company, secret).unwrap();
    let denied = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/users")
                .header("authorization", format!("Bearer {target_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    // Explicitly remove only the uniquely named test fixtures from this disposable DB.
    sqlx::query("DELETE FROM user_roles WHERE company_id = ANY($1)")
        .bind(vec![company, other_company])
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE company_id = ANY($1)")
        .bind(vec![company, other_company])
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM roles WHERE company_id = ANY($1)")
        .bind(vec![company, other_company])
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM companies WHERE id = ANY($1)")
        .bind(vec![company, other_company])
        .execute(&db)
        .await
        .unwrap();
}

async fn create_role(db: &PgPool, company: Uuid, name: &str, permissions: &[&str]) -> Uuid {
    let id: Uuid =
        sqlx::query_scalar("INSERT INTO roles(company_id,name) VALUES($1,$2) RETURNING id")
            .bind(company)
            .bind(name)
            .fetch_one(db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO role_permissions(role_id,permission_id) SELECT $1,id FROM permissions WHERE permission_key=ANY($2)").bind(id).bind(permissions).execute(db).await.unwrap();
    id
}
