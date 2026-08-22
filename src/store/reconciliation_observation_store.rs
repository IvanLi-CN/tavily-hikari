#[derive(sqlx::FromRow)]
struct ReconciliationRunObservationRow {
    mode: String,
    projection_state: String,
    projection_scanned_rows: i64,
    projection_batch_size: i64,
    projection_transaction_p95_ms: i64,
    cursor_advanced: i64,
    hydrate_ms: i64,
    first_remote_ms: Option<i64>,
    remote_ms: i64,
    finalization_ms: i64,
    research_ms: i64,
    settled: i64,
    no_adjustment: i64,
    observed: i64,
    upstream_429: i64,
    transport_failure: i64,
    semantic_failure: i64,
    local_pressure: i64,
    last_transport_kind: Option<String>,
    last_transport_kind_at: Option<i64>,
    last_retryable_outcome: Option<String>,
    continuation_reason: Option<String>,
    next_retry_at: Option<i64>,
    observed_at: i64,
}

pub(crate) struct ReconciliationRunObservationWrite {
    pub(crate) claimed_job: Option<(i64, i64)>,
    pub(crate) mode: &'static str,
    pub(crate) hydrate_ms: i64,
    pub(crate) first_remote_ms: Option<i64>,
    pub(crate) remote_ms: i64,
    pub(crate) finalization_ms: i64,
    pub(crate) research_ms: i64,
    pub(crate) settled: i64,
    pub(crate) no_adjustment: i64,
    pub(crate) observed: i64,
    pub(crate) upstream_429: i64,
    pub(crate) transport_failure: i64,
    pub(crate) semantic_failure: i64,
    pub(crate) local_pressure: i64,
    pub(crate) last_transport_kind: Option<&'static str>,
    pub(crate) last_retryable_outcome: Option<&'static str>,
    pub(crate) continuation_reason: Option<&'static str>,
    pub(crate) next_retry_at: Option<i64>,
}

impl KeyStore {
    async fn apply_upstream_reconciliation_local_backoff_locked<T>(
        transaction: &mut T,
        pressure: bool,
        now: i64,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(i64, i64, i64), ProxyError>
    where
        T: std::ops::DerefMut<Target = sqlx::SqliteConnection>,
    {
        let (previous_streak, previous_level, _) = Self::reconciliation_backoff_state_locked(
            transaction,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
        )
        .await?;
        let (streak, level, until) = if pressure {
            let streak = previous_streak.saturating_add(1);
            let level = if streak < 3 {
                0
            } else {
                previous_level.saturating_add(1).clamp(1, 4)
            };
            let delay_secs = match level {
                1 => 30,
                2 => 60,
                3 => 120,
                4 => 300,
                _ => 0,
            };
            (streak, level, now.saturating_add(delay_secs))
        } else {
            (0, 0, 0)
        };
        if !Self::reconciliation_claim_is_current_locked(transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query(
            r#"INSERT INTO meta (key, value) VALUES (?, ?), (?, ?), (?, ?)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1)
        .bind(streak.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1)
        .bind(level.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1)
        .bind(until.to_string())
        .execute(&mut **transaction)
        .await?;
        if !pressure && previous_level > 0 {
            sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1)
                .bind(now.to_string())
                .execute(&mut **transaction)
                .await?;
        }
        Ok((streak, level, until))
    }

    async fn record_reconciliation_research_progress_window_locked<T>(
        &self,
        transaction: &mut T,
        now: i64,
    ) -> Result<(), ProxyError>
    where
        T: std::ops::DerefMut<Target = sqlx::SqliteConnection>,
    {
        const WINDOW_SECS: i64 = 10 * 60;
        let day_window =
            server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&chrono::Local));
        let (
            active_period_start,
            active_started_at,
            baseline_terminal_count,
            baseline_pending_count,
        ) = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"SELECT active_period_start, active_started_at,
                       baseline_terminal_count, baseline_pending_count
                  FROM upstream_reconciliation_research_progress_window
                 WHERE id = 'local'"#,
        )
        .fetch_one(&mut **transaction)
        .await?;
        let window_end = active_started_at.saturating_add(WINDOW_SECS);
        let sample_at = if active_period_start == day_window.start
            && active_started_at > 0
            && now >= window_end
        {
            window_end
        } else {
            now
        };
        let (terminal_count, pending_count) = sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT COUNT(DISTINCT CASE
                            WHEN r.terminal_at IS NOT NULL AND r.terminal_at <= ? THEN r.request_id
                        END),
                       COUNT(DISTINCT CASE
                            WHEN r.terminal_at IS NULL OR r.terminal_at > ? THEN r.request_id
                        END)
                  FROM upstream_reconciliation_research r
                  JOIN upstream_reconciliation_usage u
                    ON u.token_id = r.token_id AND u.period_code = r.period_code
                 WHERE u.period_start >= ? AND u.period_start < ?
                   AND r.created_at <= ?"#,
        )
        .bind(sample_at)
        .bind(sample_at)
        .bind(day_window.start)
        .bind(day_window.end)
        .bind(sample_at)
        .fetch_one(&mut **transaction)
        .await?;

        if active_period_start != day_window.start || active_started_at <= 0 {
            sqlx::query(
                r#"UPDATE upstream_reconciliation_research_progress_window
                       SET active_period_start = ?, active_started_at = ?,
                           baseline_terminal_count = ?, baseline_pending_count = ?,
                           last_window_started_at = NULL, last_window_ended_at = NULL,
                           last_window_terminal_delta = 0, last_window_pending_delta = 0
                     WHERE id = 'local'"#,
            )
            .bind(day_window.start)
            .bind(now)
            .bind(terminal_count)
            .bind(pending_count)
            .execute(&mut **transaction)
            .await?;
        } else if now >= window_end {
            sqlx::query(
                r#"UPDATE upstream_reconciliation_research_progress_window
                       SET last_window_started_at = ?, last_window_ended_at = ?,
                           last_window_terminal_delta = ?, last_window_pending_delta = ?,
                           active_started_at = ?, baseline_terminal_count = ?,
                           baseline_pending_count = ?
                     WHERE id = 'local'"#,
            )
            .bind(active_started_at)
            .bind(window_end)
            .bind(terminal_count.saturating_sub(baseline_terminal_count))
            .bind(pending_count.saturating_sub(baseline_pending_count))
            .bind(window_end)
            .bind(terminal_count)
            .bind(pending_count)
            .execute(&mut **transaction)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn finalize_deferred_upstream_reconciliation_claim(
        &self,
        job_id: i64,
        claim_generation: i64,
        reason: &'static str,
        retry_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            let (_, _, local_backoff_until) =
                Self::apply_upstream_reconciliation_local_backoff_locked(
                    &mut tx,
                    true,
                    now,
                    Some((job_id, claim_generation)),
                )
                .await?;
            let available_at = retry_at.max(local_backoff_until).max(now);
            let message = format!(
                "outcome=sqlite_admission_deferred defer_reason={reason} retry_at={available_at}"
            );
            let updated = sqlx::query(
                r#"UPDATE scheduled_jobs
                   SET status = 'success', message = ?, finished_at = ?
                   WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
            )
            .bind(message)
            .bind(now)
            .bind(job_id)
            .bind(claim_generation)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            sqlx::query(
                r#"UPDATE upstream_reconciliation_run_observation
                   SET local_pressure_count = local_pressure_count + 1,
                       last_retryable_outcome = 'local_pressure',
                       continuation_reason = ?, next_retry_at = ?, observed_at = ?
                   WHERE id = 'local'"#,
            )
            .bind(reason)
            .bind(available_at)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            self.record_reconciliation_research_progress_window_locked(&mut tx, now)
                .await?;

            if let Some((continuation_id, status, trigger_source)) =
                Self::scheduled_job_lookup_active_locked(&mut tx, "upstream_reconciliation", None)
                    .await?
            {
                if status == "queued" {
                    sqlx::query(
                        "UPDATE scheduled_jobs SET available_at = MIN(available_at, ?) WHERE id = ?",
                    )
                    .bind(available_at)
                    .bind(continuation_id)
                    .execute(&mut *tx)
                    .await?;
                }
                return Ok(ScheduledJobEnqueueResult {
                    job_id: continuation_id,
                    created: false,
                    promoted: false,
                    status,
                    trigger_source,
                });
            }
            let inserted = sqlx::query(
                r#"INSERT INTO scheduled_jobs (
                       job_type, trigger_source, status, attempt, queued_at, available_at,
                       started_at, finished_at
                   ) VALUES ('upstream_reconciliation', 'auto', 'queued', 1, ?, ?, NULL, NULL)"#,
            )
            .bind(now)
            .bind(available_at)
            .execute(&mut *tx)
            .await?;
            Ok(ScheduledJobEnqueueResult {
                job_id: inserted.last_insert_rowid(),
                created: true,
                promoted: false,
                status: "queued".to_string(),
                trigger_source: "auto".to_string(),
            })
        }
        .await;
        match result {
            Ok(result) => {
                tx.finish(Ok(())).await?;
                Ok(result)
            }
            Err(error) => {
                tx.finish(Err(error)).await?;
                unreachable!("failed reconciliation finalization transaction committed")
            }
        }
    }

    pub(crate) async fn record_upstream_reconciliation_engine_observation(
        &self,
        observation: ReconciliationRunObservationWrite,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        if let Some((job_id, claim_generation)) = observation.claimed_job {
            let claim_current: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM scheduled_jobs WHERE id = ? AND status = 'running' AND claim_generation = ?)",
            )
            .bind(job_id)
            .bind(claim_generation)
            .fetch_one(&mut *tx)
            .await?;
            if claim_current == 0 {
                tx.rollback().await?;
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
        }
        sqlx::query(
            r#"UPDATE upstream_reconciliation_run_observation
               SET mode = ?, hydrate_ms = ?, first_remote_ms = ?, remote_ms = ?,
                   finalization_ms = ?, research_ms = ?, settled_count = ?,
                   no_adjustment_count = ?, observed_count = ?, upstream_429_count = ?,
                   transport_failure_count = ?, semantic_failure_count = ?,
                   local_pressure_count = ?,
                   last_transport_kind = COALESCE(?, last_transport_kind),
                   last_transport_kind_at = COALESCE(?, last_transport_kind_at),
                   last_retryable_outcome = CASE
                       WHEN ? IN ('upstream_429', 'transport_failure', 'semantic_failure', 'local_pressure')
                           THEN ?
                       WHEN ? IN ('settled', 'no_adjustment', 'observed')
                           THEN NULL
                       ELSE last_retryable_outcome
                   END,
                   continuation_reason = ?, next_retry_at = ?,
                   observed_at = ?
               WHERE id = 'local'"#,
        )
        .bind(observation.mode)
        .bind(observation.hydrate_ms)
        .bind(observation.first_remote_ms)
        .bind(observation.remote_ms)
        .bind(observation.finalization_ms)
        .bind(observation.research_ms)
        .bind(observation.settled)
        .bind(observation.no_adjustment)
        .bind(observation.observed)
        .bind(observation.upstream_429)
        .bind(observation.transport_failure)
        .bind(observation.semantic_failure)
        .bind(observation.local_pressure)
        .bind(observation.last_transport_kind)
        .bind(observation.last_transport_kind.map(|_| now))
        .bind(observation.last_retryable_outcome)
        .bind(observation.last_retryable_outcome)
        .bind(observation.continuation_reason)
        .bind(observation.continuation_reason)
        .bind(observation.next_retry_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        self.record_reconciliation_research_progress_window_locked(&mut tx, now)
            .await?;
        tx.finish(Ok(())).await
    }
    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_run_observation(
        &self,
    ) -> Result<ReconciliationRunObservation, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let row: ReconciliationRunObservationRow = sqlx::query_as(
                r#"SELECT o.mode,
                          CASE WHEN p.completed != 0 THEN 'complete'
                               WHEN p.last_defer_reason IS NOT NULL THEN 'deferred'
                               ELSE o.projection_state END AS projection_state,
                          p.scanned_rows AS projection_scanned_rows,
                          p.batch_size AS projection_batch_size,
                          p.transaction_p95_ms AS projection_transaction_p95_ms,
                          o.cursor_advanced, o.hydrate_ms, o.first_remote_ms,
                          o.remote_ms, o.finalization_ms, o.research_ms,
                          o.settled_count AS settled,
                          o.no_adjustment_count AS no_adjustment,
                          o.observed_count AS observed,
                          o.upstream_429_count AS upstream_429,
                          o.transport_failure_count AS transport_failure,
                          o.semantic_failure_count AS semantic_failure,
                          o.local_pressure_count AS local_pressure,
                          o.last_transport_kind,
                          o.last_transport_kind_at,
                          o.last_retryable_outcome,
                          COALESCE(o.continuation_reason, p.last_defer_reason)
                              AS continuation_reason,
                          CASE
                            WHEN o.next_retry_at IS NULL AND p.next_retry_at <= 0 THEN NULL
                            ELSE MAX(COALESCE(o.next_retry_at, 0), p.next_retry_at)
                          END AS next_retry_at,
                          o.observed_at
                   FROM upstream_reconciliation_run_observation o
                   JOIN upstream_reconciliation_projection_state p ON p.id = o.id
                   WHERE o.id = 'local'"#,
            )
            .fetch_one(&mut *conn)
            .await?;
        conn.close().await?;
        Ok(ReconciliationRunObservation {
            mode: row.mode,
            projection_state: row.projection_state,
            projection_scanned_rows: row.projection_scanned_rows,
            projection_batch_size: row.projection_batch_size,
            projection_transaction_p95_ms: row.projection_transaction_p95_ms,
            cursor_advanced: row.cursor_advanced != 0,
            hydrate_ms: row.hydrate_ms,
            first_remote_ms: row.first_remote_ms,
            remote_ms: row.remote_ms,
            finalization_ms: row.finalization_ms,
            research_ms: row.research_ms,
            settled: row.settled,
            no_adjustment: row.no_adjustment,
            observed: row.observed,
            upstream_429: row.upstream_429,
            transport_failure: row.transport_failure,
            semantic_failure: row.semantic_failure,
            local_pressure: row.local_pressure,
            last_transport_kind: row.last_transport_kind,
            last_transport_kind_at: row.last_transport_kind_at,
            last_retryable_outcome: row.last_retryable_outcome,
            continuation_reason: row.continuation_reason,
            next_retry_at: row.next_retry_at,
            observed_at: (row.observed_at > 0).then_some(row.observed_at),
        })
    }
}
