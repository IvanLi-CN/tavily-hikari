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
    pub(crate) async fn record_upstream_reconciliation_engine_observation(
        &self,
        observation: ReconciliationRunObservationWrite,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ScheduledJobControl)
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
        tx.finish(Ok(())).await
    }
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
