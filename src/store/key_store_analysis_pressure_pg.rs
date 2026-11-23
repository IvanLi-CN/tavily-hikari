/// PostgreSQL twins of the server-pressure helpers.
///
/// The SQLite implementations stay in `key_store_analysis_pressure.rs`; these variants are
/// reachable only through the attached PostgreSQL pool.
impl KeyStore {
    async fn lock_server_pressure_lane_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProxyError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tavily_hikari.server_pressure'))")
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

}
