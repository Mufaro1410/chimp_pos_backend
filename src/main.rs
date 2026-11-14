use std::{sync::Arc, time::Duration};

use axum::serve;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;

use chimpPOS_backend::{routes, state::AppState};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Structured logging. Control with RUST_LOG (default: info for the app, warn for the rest).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,chimpPOS_backend=info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    if jwt_secret.len() < 32 {
        panic!("JWT_SECRET must contain at least 32 characters");
    }

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());

    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    // Opt-in: apply pending migrations at startup (RUN_MIGRATIONS=true).
    // Off by default so an existing database is never changed unexpectedly.
    if std::env::var("RUN_MIGRATIONS")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
    {
        println!("Applying pending database migrations...");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("Database migration failed");
        println!("Migrations are up to date");
    }

    let state = Arc::new(AppState { db, jwt_secret });
    let app = routes::create_routes(state);

    let address = format!("{host}:{port}");
    let listener = TcpListener::bind(&address)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {address}: {e}"));

    println!("Server running at http://{address}");

    serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server failed");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    println!("Shutting down gracefully...");
}
