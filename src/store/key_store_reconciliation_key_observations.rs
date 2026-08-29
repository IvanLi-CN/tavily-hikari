/// A successful usage response for one upstream key. These rows are local,
/// rebuildable observations; the work row and settlement ledger remain the
/// replicated reconciliation truth.
pub(crate) struct ReconciliationKeyObservation {
    pub(crate) key_id: String,
    pub(crate) upstream_usage: i64,
}

impl KeyStore {
    pub(crate) async fn reconciliation_key_observations(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        work_generation: i64,
        key_ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT key_id, upstream_usage FROM upstream_reconciliation_key_observations \
             WHERE token_id = ",
        );
        query
            .push_bind(&candidate.token_id)
            .push(" AND period_code = ")
            .push_bind(&candidate.period_code)
            .push(" AND work_generation = ")
            .push_bind(work_generation)
            .push(" AND key_id IN (");
        {
            let mut separated = query.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        query.push(") ORDER BY key_id");
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::CandidateHydrate)
            .await?;
        let rows_result = query
            .build_query_as::<(String, i64)>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn persist_reconciliation_key_observations(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        work_generation: i64,
        observations: &[ReconciliationKeyObservation],
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<bool, ProxyError> {
        if observations.is_empty() {
            return Ok(true);
        }
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        if !self
            .lock_reconciliation_work_generation(&mut tx, candidate, fence)
            .await?
        {
            tx.rollback().await?;
            return Ok(false);
        }
        // A new usage generation must never reuse a previous generation's
        // local observations. Stale rows are rebuildable and are removed only
        // after the current claim/generation fence has been acquired.
        sqlx::query(
            "DELETE FROM upstream_reconciliation_key_observations \
             WHERE token_id = ? AND period_code = ? AND work_generation <> ?",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(work_generation)
        .execute(&mut *tx)
        .await?;
        for observation in observations {
            sqlx::query(
                r#"INSERT INTO upstream_reconciliation_key_observations (
                       token_id, period_code, work_generation, key_id,
                       upstream_usage, observed_at
                   ) VALUES (?, ?, ?, ?, ?, ?)
                   ON CONFLICT(token_id, period_code, work_generation, key_id)
                   DO UPDATE SET upstream_usage = excluded.upstream_usage,
                                 observed_at = excluded.observed_at"#,
            )
            .bind(&candidate.token_id)
            .bind(&candidate.period_code)
            .bind(work_generation)
            .bind(&observation.key_id)
            .bind(observation.upstream_usage)
            .bind(self.backend_time.now_ts())
            .execute(&mut *tx)
            .await?;
        }
        tx.finish(Ok(())).await?;
        Ok(true)
    }

    async fn clear_reconciliation_key_observations(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            "DELETE FROM upstream_reconciliation_key_observations \
             WHERE token_id = ? AND period_code = ?",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
