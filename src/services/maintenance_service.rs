//! Database housekeeping.
use sqlx::PgPool;

/// Refreshes planner statistics for the application's own tables. PostgreSQL has none after a
/// restore and only stale ones after a bulk import; with stale statistics some report/list queries
/// pick catastrophic plans (seconds instead of milliseconds). ANALYZE only samples each table, so
/// it is cheap to run regularly.
///
/// Tables are analysed one by one in the `public` schema rather than with a bare `ANALYZE`, which
/// would also try (and, for a non-superuser, fail noisily) on PostgreSQL's shared system catalogs.
pub async fn analyze(db: &PgPool) -> Result<(), sqlx::Error> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT quote_ident(tablename) FROM pg_tables \
         WHERE schemaname = 'public' AND tableowner = current_user ORDER BY tablename",
    )
    .fetch_all(db)
    .await?;
    for table in tables {
        // One table failing (e.g. dropped concurrently) must not stop the rest.
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(format!("ANALYZE public.{table}")))
            .execute(db)
            .await
        {
            tracing::warn!("ANALYZE of {table} failed: {e}");
        }
    }
    Ok(())
}
