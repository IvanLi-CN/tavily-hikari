/// PostgreSQL twins of the request-log GC transaction helpers.
///
/// The SQLite implementations stay in `key_store_request_logs_gc.rs`; these variants are
/// reachable only through the attached PostgreSQL pool.
impl KeyStore {
    async fn lock_request_log_gc_batch_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProxyError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tavily_hikari.request_logs_gc'))")
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn set_gc_statement_budget_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        deadline: tokio::time::Instant,
    ) -> Result<(), ProxyError> {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let millis = (remaining.as_millis() as i64).max(1);
        sqlx::query("SELECT set_config('lock_timeout', $1, true)")
            .bind(format!("{millis}ms"))
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

}
