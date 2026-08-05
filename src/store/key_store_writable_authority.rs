impl KeyStore {
    async fn restore_authority_write_connection(
        mut conn: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    ) -> Result<(), ProxyError> {
        sqlx::query(&format!(
            "PRAGMA busy_timeout = {}",
            SQLITE_BUSY_TIMEOUT_DEFAULT.as_millis()
        ))
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    async fn acquire_authority_write_connection(
        &self,
    ) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, ProxyError> {
        // Leave headroom for both pool admission and SQLite's writer wait so
        // the complete authority write stays inside the 250 ms contract.
        const AUTHORITY_WRITE_STEP_BUDGET: Duration = Duration::from_millis(100);
        let mut conn = tokio::time::timeout(AUTHORITY_WRITE_STEP_BUDGET, self.pool.acquire())
            .await
            .map_err(|_| ProxyError::Database(sqlx::Error::PoolTimedOut))??;
        sqlx::query("PRAGMA busy_timeout = 100")
            .execute(&mut *conn)
            .await?;
        Ok(conn)
    }

    pub(crate) async fn with_writable_authority<T, F>(
        &self,
        expected_epoch: i64,
        operation: F,
    ) -> Result<T, ProxyError>
    where
        F: for<'tx> FnOnce(
            &'tx mut sqlx::Transaction<'_, sqlx::Sqlite>,
        ) -> futures_util::future::BoxFuture<'tx, Result<T, ProxyError>>,
    {
        let mut tx = self.pool.begin().await?;
        // Acquire SQLite's writer slot before checking the epoch so the business
        // commit and demotion epoch commit have one serialization order.
        sqlx::query(
            "UPDATE ha_node_state SET authority_epoch = authority_epoch WHERE id = 'local'",
        )
        .execute(&mut *tx)
        .await?;
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT authority_epoch FROM ha_node_state WHERE id = 'local' AND authority_phase = 'writable'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if current != Some(expected_epoch) {
            return Err(ProxyError::Other(format!(
                "stale writable authority epoch {expected_epoch}"
            )));
        }
        let result = operation(&mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub(crate) async fn get_writable_authority_state(
        &self,
    ) -> Result<WritableAuthorityState, ProxyError> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT authority_epoch, authority_phase FROM ha_node_state WHERE id = 'local'",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((epoch, phase)) = row else {
            return Ok(WritableAuthorityState::standby(0));
        };
        let phase = match phase.as_str() {
            "writable" => WritableAuthorityPhase::Writable,
            "demoting" => WritableAuthorityPhase::Demoting,
            _ => WritableAuthorityPhase::Standby,
        };
        Ok(WritableAuthorityState { epoch, phase })
    }

    pub(crate) async fn persist_writable_authority_phase(
        &self,
        phase: WritableAuthorityPhase,
    ) -> Result<WritableAuthorityState, ProxyError> {
        let phase = writable_authority_phase_str(phase);
        let mut conn = self.acquire_authority_write_connection().await?;
        let result = sqlx::query(
            r#"
            INSERT INTO ha_node_state (id, node_id, role, updated_at, authority_epoch, authority_phase)
            VALUES ('local', 'unknown', 'standby', ?, 0, ?)
            ON CONFLICT(id) DO UPDATE SET authority_phase = excluded.authority_phase,
                                                  updated_at = excluded.updated_at
            "#,
        )
        .bind(self.backend_time.now_ts())
        .bind(phase)
        .execute(&mut *conn)
        .await;
        Self::restore_authority_write_connection(conn).await?;
        result?;
        self.get_writable_authority_state().await
    }

    pub(crate) async fn advance_writable_authority_epoch(
        &self,
        next_phase: WritableAuthorityPhase,
    ) -> Result<WritableAuthorityState, ProxyError> {
        let conn = self.acquire_authority_write_connection().await?;
        let mut tx = ImmediateSqliteTransaction::begin(conn).await?;
        sqlx::query(
            r#"
            INSERT INTO ha_node_state (id, node_id, role, updated_at, authority_epoch, authority_phase)
            VALUES ('local', 'unknown', 'standby', ?, 1, ?)
            ON CONFLICT(id) DO UPDATE SET authority_epoch = ha_node_state.authority_epoch + 1,
                                                  authority_phase = excluded.authority_phase,
                                                  updated_at = excluded.updated_at
            "#,
        )
        .bind(self.backend_time.now_ts())
        .bind(writable_authority_phase_str(next_phase))
        .execute(&mut *tx)
        .await?;
        let (epoch, phase): (i64, String) = sqlx::query_as(
            "SELECT authority_epoch, authority_phase FROM ha_node_state WHERE id = 'local'",
        )
        .fetch_one(&mut *tx)
        .await?;
        let conn = tx.commit_connection().await?;
        Self::restore_authority_write_connection(conn).await?;
        Ok(WritableAuthorityState {
            epoch,
            phase: parse_writable_authority_phase(&phase),
        })
    }

    pub(crate) async fn authority_epoch_is_current(&self, expected: i64) -> Result<bool, ProxyError> {
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT authority_epoch FROM ha_node_state WHERE id = 'local' AND authority_phase = 'writable'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(current == Some(expected))
    }
}
