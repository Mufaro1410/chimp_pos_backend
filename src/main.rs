use std::{net::SocketAddr, sync::Arc, time::Duration};

use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

use chimpPOS_backend::{
    config,
    routes,
    services::{maintenance_service, session_service},
    state::AppState,
};

const USAGE: &str = "\
ChimpPOS backend

USAGE:
    chimpPOS_backend [serve]                          Run the API server (default)
    chimpPOS_backend migrate                          Apply pending database migrations and exit
    chimpPOS_backend reset-password <company_id> <username>
                                                      Set a user's password (reads it from stdin),
                                                      clears lockout and ends all their sessions
";

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // sqlx::query would log every statement slower than 1 s together with its full text
        // (migrations are huge); real database errors are returned to the caller anyway.
        tracing_subscriber::EnvFilter::new(
            "warn,chimpPOS_backend=info,tower_http=info,sqlx::query=error,sqlx::postgres::notice=error",
        )
    });
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if std::env::var("LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
    {
        builder.json().init();
    } else {
        builder.init();
    }
}

async fn connect(max_connections: u32, with_timeouts: bool) -> PgPool {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let mut options = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10));
    if with_timeouts {
        // Protect the database from runaway queries and abandoned transactions.
        options = options.after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '30s'")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("SET idle_in_transaction_session_timeout = '60s'")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        });
    }
    options
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL")
}

async fn run_migrations() {
    // Dedicated connection without the statement timeout: migrations may legitimately be slow.
    let pool = connect(1, false).await;
    println!("Applying pending database migrations...");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Database migration failed");
    println!("Migrations are up to date");
    if let Err(e) = maintenance_service::analyze(&pool).await {
        eprintln!("ANALYZE after migration failed (not fatal): {e}");
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    init_logging();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve().await,
        Some("migrate") => run_migrations().await,
        Some("reset-password") => reset_password(&args[1..]).await,
        Some("-h") | Some("--help") | Some("help") => print!("{USAGE}"),
        Some(other) => {
            eprintln!("Unknown command '{other}'\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

async fn reset_password(args: &[String]) {
    let (Some(company), Some(username)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let company: uuid::Uuid = company.parse().unwrap_or_else(|_| {
        eprintln!("company_id must be a UUID");
        std::process::exit(2);
    });
    eprintln!("Enter the new password for '{username}' (15-128 characters), then press Enter:");
    let mut password = String::new();
    std::io::stdin()
        .read_line(&mut password)
        .expect("could not read the password");
    let password = password.trim_end_matches(['\r', '\n']).to_string();

    let db = connect(2, false).await;
    match session_service::cli_reset_password(&db, company, username, password).await {
        Ok(()) => println!("Password updated. All of the user's sessions were ended."),
        Err(e) => {
            eprintln!("Failed: {} ({})", e.message, e.code);
            std::process::exit(1);
        }
    }
}

async fn serve() {
    let problems = config::configuration_problems();
    if !problems.is_empty() {
        if config::is_production() {
            for p in &problems {
                eprintln!("CONFIGURATION ERROR: {p}");
            }
            panic!("Refusing to start in production with an unsafe configuration");
        }
        for p in &problems {
            tracing::warn!("configuration warning: {p}");
        }
    }
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    if jwt_secret.len() < 32 {
        panic!("JWT_SECRET must contain at least 32 characters");
    }

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let max_connections: u32 = std::env::var("DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 2)
        .unwrap_or(10);

    // Opt-in: apply pending migrations at startup (RUN_MIGRATIONS=true).
    if std::env::var("RUN_MIGRATIONS")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
    {
        run_migrations().await;
    }

    let db = connect(max_connections, true).await;

    // Housekeeping: expired tokens and old finished sync entries.
    let maintenance_db = db.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(6 * 3600));
        loop {
            tick.tick().await;
            if let Err(e) = session_service::cleanup(&maintenance_db).await {
                tracing::error!("maintenance cleanup failed: {e}");
            }
        }
    });

    // Keep planner statistics fresh (see maintenance_service::analyze).
    let analyze_db = db.clone();
    let analyze_secs: u64 = std::env::var("ANALYZE_INTERVAL_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 10)
        .unwrap_or(300);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(analyze_secs));
        loop {
            tick.tick().await; // first tick fires immediately: fresh statistics at startup
            if let Err(e) = maintenance_service::analyze(&analyze_db).await {
                tracing::error!("periodic ANALYZE failed: {e}");
            }
        }
    });

    let state = Arc::new(AppState { db, jwt_secret });
    let app = routes::create_routes(state);

    let address = format!("{host}:{port}");
    let listener = TcpListener::bind(&address)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {address}: {e}"));

    println!("Server running at http://{address}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
