//! Database housekeeping.
use sqlx::PgPool;

/// Refreshes planner statistics. PostgreSQL has none after a restore and only stale ones after a
/// bulk import; with stale statistics some report/list queries pick catastrophic plans (seconds
/// instead of milliseconds). ANALYZE only samples each table, so it is cheap to run regularly.
pub async fn analyze(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("ANALYZE").execute(db).await?;
    Ok(())
}
