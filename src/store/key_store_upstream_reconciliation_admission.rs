type UpstreamReconciliationLastRunStats = (Option<i64>, i64, i64, i64, i64, bool);

impl KeyStore {
    pub(crate) async fn upstream_reconciliation_global_backoff_state(
        &self,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.read_reconciliation_backoff_state(
            META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
        )
        .await
    }

    pub(crate) async fn upstream_reconciliation_last_run_stats(
        &self,
    ) -> Result<UpstreamReconciliationLastRunStats, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result: Result<UpstreamReconciliationLastRunStats, ProxyError> = async {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT key, value FROM meta WHERE key IN (?, ?, ?, ?, ?, ?)",
            )
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1)
            .fetch_all(&mut *conn)
            .await?;
            let values = rows.into_iter().collect::<std::collections::HashMap<_, _>>();
            let value_i64 = |key: &str| {
                values
                    .get(key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            Ok((
                values
                    .get(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1)
                    .and_then(|value| value.parse::<i64>().ok()),
                value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1),
                value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1),
                value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1),
                value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1),
                value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1) != 0,
            ))
        }
        .await;
        let close = conn.close().await;
        match (result, close) {
            (Ok(stats), Ok(())) => Ok(stats),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    async fn begin_reconciliation_control(&self) -> Result<SqliteImmediateTransaction, ProxyError> {
        self.sqlite_runtime
            .begin_immediate(SqliteOperation::ScheduledJobControl)
            .await
    }

    async fn sync_upstream_reconciliation_representative_locked<T>(
        transaction: &mut T,
        now: i64,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError>
    where
        T: std::ops::DerefMut<Target = sqlx::SqliteConnection>,
    {
        let available_at = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(CAST(value AS INTEGER)) FROM meta WHERE key IN (?, ?)",
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1)
        .fetch_one(&mut **transaction)
        .await?
        .unwrap_or(0);
        if let Some((job_id, claim_generation)) = claimed_job {
            if available_at <= now {
                if !Self::reconciliation_claim_is_current_locked(
                    transaction,
                    Some((job_id, claim_generation)),
                )
                .await?
                {
                    return Err(ProxyError::StaleClaim {
                        job_id,
                        claim_generation,
                    });
                }
                return Ok(());
            }
            let updated = sqlx::query(
                r#"UPDATE scheduled_jobs
                   SET status = 'queued', available_at = ?, started_at = NULL,
                       finished_at = NULL, message = 'reconciliation backoff active'
                   WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
            )
            .bind(available_at)
            .bind(job_id)
            .bind(claim_generation)
            .execute(&mut **transaction)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            return Ok(());
        }
        if available_at > now {
            sqlx::query(
                r#"UPDATE scheduled_jobs
                   SET available_at = MAX(available_at, ?)
                   WHERE job_type = 'upstream_reconciliation'
                     AND status = 'queued' AND trigger_source = 'auto'"#,
            )
            .bind(available_at)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                r#"INSERT INTO scheduled_jobs (
                     job_type, trigger_source, status, attempt, queued_at, available_at
                   )
                   SELECT 'upstream_reconciliation', 'auto', 'queued', 1, ?, ?
                   WHERE NOT EXISTS (
                     SELECT 1 FROM scheduled_jobs
                     WHERE job_type = 'upstream_reconciliation'
                       AND status IN ('queued', 'running')
                   )"#,
            )
            .bind(now)
            .bind(available_at)
            .execute(&mut **transaction)
            .await?;
        } else {
            sqlx::query(
                r#"UPDATE scheduled_jobs
                   SET status = 'abandoned', finished_at = ?,
                       message = 'reconciliation backoff recovered'
                   WHERE job_type = 'upstream_reconciliation'
                     AND status = 'queued' AND trigger_source = 'auto'"#,
            )
            .bind(now)
            .execute(&mut **transaction)
            .await?;
        }
        Ok(())
    }
}
