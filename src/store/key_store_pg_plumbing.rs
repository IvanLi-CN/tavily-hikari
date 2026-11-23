/// PostgreSQL plumbing shared by the storage modules that have already been ported.
///
/// The pool lives on [`KeyStore`] itself; keeping the accessors here holds the module hub
/// within its line budget while the port is in progress.
impl KeyStore {
    /// PostgreSQL pool for the storage modules that have already been ported from the
    /// SQLite dialect. Absent when the process runs without a PostgreSQL target.
    #[allow(dead_code)] // consumed by the modules converted in this wave
    pub(crate) fn pg(&self) -> Result<&sqlx::PgPool, ProxyError> {
        self.pg_pool.as_ref().ok_or_else(|| {
            ProxyError::Other("postgresql store is not configured for this process".to_string())
        })
    }

    /// Attach a PostgreSQL pool to an existing store. Converted modules use it through
    /// [`KeyStore::pg`]; unconverted modules keep serving from SQLite until the cutover.
    pub(crate) async fn attach_postgres(&mut self, database_url: &str) -> Result<(), ProxyError> {
        let pg_store = crate::store::pg::PgStore::connect(database_url)
            .await
            .map_err(|err| ProxyError::Other(format!("failed to connect to postgresql: {err}")))?;
        self.pg_pool = Some(pg_store.pool().clone());
        Ok(())
    }
}
