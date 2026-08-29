const RECONCILIATION_SETTLEMENT_MODE_ACTUAL: &str = "actual";
const RECONCILIATION_SETTLEMENT_MODE_SHADOW: &str = "shadow";
const RECONCILIATION_STATUS_SETTLED: &str = "settled";
const RECONCILIATION_STATUS_DEGRADED: &str = "degraded";
const RECONCILIATION_STATUS_SHADOW_SETTLED: &str = "shadow_settled";
const RECONCILIATION_STATUS_SHADOW_DEGRADED: &str = "shadow_degraded";
pub(crate) const RECONCILIATION_STATUS_RATE_LIMITED: &str = "rate_limited";
pub(crate) const RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT: &str =
    "local_usage_rate_limit";
pub(crate) const RECONCILIATION_RETRY_REASON_UPSTREAM_429: &str = "upstream429";
pub(crate) const RECONCILIATION_RETRY_REASON_MISSING_ELIGIBLE_UPSTREAM_KEY: &str =
    "missing_eligible_upstream_key";
pub(crate) const RECONCILIATION_RETRY_REASON_OTHER: &str = "other";
pub(crate) const RECONCILIATION_OUTCOME_SETTLED: &str = "settled";
pub(crate) const RECONCILIATION_OUTCOME_NO_ADJUSTMENT: &str = "no_adjustment";
pub(crate) const RECONCILIATION_OUTCOME_OBSERVED: &str = "observed";
pub(crate) const RECONCILIATION_OUTCOME_UPSTREAM_429: &str = "upstream_429";
pub(crate) const RECONCILIATION_OUTCOME_TRANSPORT_FAILURE: &str = "transport_failure";
pub(crate) const RECONCILIATION_OUTCOME_SEMANTIC_FAILURE: &str = "semantic_failure";
pub(crate) const RECONCILIATION_OUTCOME_MISSING_ELIGIBLE_UPSTREAM_KEY: &str =
    "missing_eligible_upstream_key";
pub(crate) const RECONCILIATION_OUTCOME_LOCAL_PRESSURE: &str = "local_pressure";
pub(crate) const META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1: &str =
    "upstream_reconciliation_work_projection_complete_v1";
const RECONCILIATION_PROJECTION_MIN_BATCH: i64 = 25;
const RECONCILIATION_PROJECTION_MAX_BATCH: i64 = 100;
const RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS: [i64; 6] = [10, 25, 50, 100, 250, 251];
const RECONCILIATION_RECENT_LANE_BUDGET: i64 = 12;
const RECONCILIATION_BACKLOG_LANE_BUDGET: i64 = 8;
const RECONCILIATION_QUEUE_ESTIMATE_LIMIT: i64 = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) struct UpstreamReconciliationRunAdmissionState {
    pub(crate) claim_current: bool,
    pub(crate) shadow_ready: bool,
    pub(crate) mode: ReconciliationMode,
    pub(crate) global_backoff_level: i64,
    pub(crate) global_backoff_until: i64,
    pub(crate) local_backoff_level: i64,
    pub(crate) local_backoff_until: i64,
}

enum ReconciliationCandidateScope {
    Recent { start: i64, end: i64 },
    Backlog { before: i64 },
}

type UpstreamReconciliationCandidateRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
);

#[derive(Clone)]
struct UpstreamReconciliationCandidateWork {
    candidate: UpstreamReconciliationCandidate,
    work_generation: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct ReconciliationWorkFence {
    pub(crate) work_generation: i64,
    pub(crate) claimed_job: Option<(i64, i64)>,
}




fn upstream_reconciliation_shadow_ready(settings: &SystemSettings) -> bool {
    settings.upstream_project_id_mode == UpstreamProjectIdMode::AccessToken
        && settings.api_rebalance_enabled
        && settings.rebalance_mcp_enabled
}

pub(crate) fn classify_reconciliation_retry_reason(reason: Option<&str>) -> &'static str {
    let Some(reason) = reason else {
        return RECONCILIATION_RETRY_REASON_OTHER;
    };
    if reason == RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT {
        return RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT;
    }
    if reason == RECONCILIATION_RETRY_REASON_UPSTREAM_429 {
        return RECONCILIATION_RETRY_REASON_UPSTREAM_429;
    }
    if reason == RECONCILIATION_RETRY_REASON_MISSING_ELIGIBLE_UPSTREAM_KEY {
        return RECONCILIATION_RETRY_REASON_MISSING_ELIGIBLE_UPSTREAM_KEY;
    }
    if reason.starts_with("usage http error 429 ") {
        return RECONCILIATION_RETRY_REASON_UPSTREAM_429;
    }
    RECONCILIATION_RETRY_REASON_OTHER
}

impl KeyStore {
    pub(crate) fn try_admit_upstream_reconciliation_projection(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::ReconciliationProjection)
    }

    pub(crate) async fn prewarm_upstream_reconciliation_projection_capacity(&self) {
        self.sqlite_runtime
            .prewarm_reconciliation_projection_capacity()
            .await;
    }

    pub(crate) async fn upstream_reconciliation_run_admission_state(
        &self,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<UpstreamReconciliationRunAdmissionState, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let read_result = async {
            let claim_current = if let Some((job_id, claim_generation)) = claimed_job {
                sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM scheduled_jobs WHERE id = ? AND status = 'running' AND claim_generation = ?)",
            )
            .bind(job_id)
            .bind(claim_generation)
            .fetch_one(&mut *conn)
                .await?
                    != 0
            } else {
                true
            };
            let rows: Vec<(String, String)> = sqlx::query_as(
                r#"
            SELECT key, value
            FROM meta
            WHERE key IN (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
        .bind(META_KEY_API_REBALANCE_ENABLED_V1)
        .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1)
            .fetch_all(&mut *conn)
            .await?;
            let mode: String = sqlx::query_scalar(
                "SELECT mode FROM upstream_reconciliation_control_state WHERE id = 'local'",
            )
            .fetch_one(&mut *conn)
            .await?;
            Ok((claim_current, rows, mode))
        }
        .await;
        let (claim_current, rows, mode) = conn.complete_query(read_result).await?;
        let values = rows.into_iter().collect::<std::collections::HashMap<_, _>>();
        let value_i64 = |key: &str| {
            values
                .get(key)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
        };
        let shadow_ready = values
            .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
            .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
            .unwrap_or_default()
            == UpstreamProjectIdMode::AccessToken
            && value_i64(META_KEY_API_REBALANCE_ENABLED_V1)
                != 0
            && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1)
                != 0;
        Ok(UpstreamReconciliationRunAdmissionState {
            claim_current,
            shadow_ready,
            mode: ReconciliationMode::parse(&mode).ok_or_else(|| {
                ProxyError::Other("invalid persisted upstream reconciliation mode".to_string())
            })?,
            global_backoff_level: value_i64(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1),
            global_backoff_until: value_i64(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1),
            local_backoff_level: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1),
            local_backoff_until: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1),
        })
    }

    pub(crate) async fn api_key_transient_backoff_state(
        &self,
        key_id: &str,
        scope: &str,
    ) -> Result<Option<ApiKeyTransientBackoffState>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let state = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT cooldown_until, retry_after_secs
            FROM api_key_transient_backoffs
            WHERE key_id = ? AND scope = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(scope)
        .fetch_optional(&mut *conn)
        .await?;
        conn.close().await?;
        Ok(state.map(|(cooldown_until, retry_after_secs)| ApiKeyTransientBackoffState {
            cooldown_until,
            retry_after_secs,
        }))
    }

    pub(crate) async fn arm_api_key_transient_backoff_claimed(
        &self,
        arm: ApiKeyTransientBackoffArm<'_>,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<Option<ApiKeyTransientBackoffState>, ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(
            &mut transaction,
            Some((job_id, claim_generation)),
        )
        .await?
        {
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        let previous_cooldown = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT cooldown_until
            FROM api_key_transient_backoffs
            WHERE key_id = ? AND scope = ?
            LIMIT 1
            "#,
        )
        .bind(arm.key_id)
        .bind(arm.scope)
        .fetch_optional(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO api_key_transient_backoffs (
                key_id, scope, cooldown_until, retry_after_secs, reason_code,
                source_request_log_id, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(key_id, scope) DO UPDATE SET
                cooldown_until = MAX(api_key_transient_backoffs.cooldown_until, excluded.cooldown_until),
                retry_after_secs = CASE
                    WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until
                        THEN excluded.retry_after_secs
                    ELSE api_key_transient_backoffs.retry_after_secs
                END,
                reason_code = CASE
                    WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until
                        THEN COALESCE(excluded.reason_code, api_key_transient_backoffs.reason_code)
                    ELSE api_key_transient_backoffs.reason_code
                END,
                source_request_log_id = COALESCE(
                    excluded.source_request_log_id,
                    api_key_transient_backoffs.source_request_log_id
                ),
                updated_at = CASE
                    WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until
                        THEN excluded.updated_at
                    ELSE api_key_transient_backoffs.updated_at
                END
            "#,
        )
        .bind(arm.key_id)
        .bind(arm.scope)
        .bind(arm.cooldown_until)
        .bind(arm.retry_after_secs)
        .bind(arm.reason_code)
        .bind(arm.source_request_log_id)
        .bind(arm.now)
        .bind(arm.now)
        .execute(&mut *transaction)
        .await?;
        let current = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT cooldown_until, retry_after_secs
            FROM api_key_transient_backoffs
            WHERE key_id = ? AND scope = ?
            LIMIT 1
            "#,
        )
        .bind(arm.key_id)
        .bind(arm.scope)
        .fetch_one(&mut *transaction)
        .await?;
        let state = previous_cooldown.is_none_or(|previous| previous < current.0).then_some(
            ApiKeyTransientBackoffState {
                cooldown_until: current.0,
                retry_after_secs: current.1,
            },
        );
        transaction.finish(Ok(())).await?;
        Ok(state)
    }

    pub(crate) async fn count_active_upstream_mcp_sessions(
        &self,
        now: i64,
    ) -> Result<i64, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM mcp_sessions
            WHERE gateway_mode = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            "#,
        )
        .bind(MCP_GATEWAY_MODE_UPSTREAM)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn refresh_upstream_reconciliation_epoch(
        &self,
    ) -> Result<(bool, i64, i64), ProxyError> {
        let now = self.backend_time.now_ts();
        let settings = self.get_system_settings().await?;
        let active_upstream_mcp_sessions = self.count_active_upstream_mcp_sessions(now).await?;
        let static_ready = upstream_reconciliation_shadow_ready(&settings);
        let current = self
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1)
            .await?
            .unwrap_or(0);
        if !static_ready || active_upstream_mcp_sessions > 0 {
            if current != 0 {
                self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1, 0)
                    .await?;
            }
            return Ok((false, 0, active_upstream_mcp_sessions));
        }
        let ready_after = if current <= 0 {
            let next = business_period_for_timestamp(now).ends_at;
            self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1, next)
                .await?;
            next
        } else {
            current
        };
        Ok((now >= ready_after, ready_after, active_upstream_mcp_sessions))
    }

    pub(crate) async fn upstream_reconciliation_shadow_compare_active_with_settings(
        &self,
        settings: &SystemSettings,
    ) -> Result<bool, ProxyError> {
        if !upstream_reconciliation_shadow_ready(settings) {
            return Ok(false);
        }
        let state = self.upstream_reconciliation_control_state().await?;
        match state.mode {
            ReconciliationMode::Compare | ReconciliationMode::ActivePaused => Ok(true),
            ReconciliationMode::Active if state.legacy_active => {
                let now = self.backend_time.now_ts();
                let active_upstream_mcp_sessions = self.count_active_upstream_mcp_sessions(now).await?;
                if active_upstream_mcp_sessions > 0 {
                    return Ok(true);
                }
                let ready_after = self
                    .load_or_initialize_upstream_reconciliation_ready_after(now)
                    .await?;
                Ok(now < ready_after)
            }
            ReconciliationMode::Active => Ok(state
                .activation_period_start
                .is_some_and(|boundary| self.backend_time.now_ts() < boundary)),
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_runtime_markers(
        &self,
    ) -> Result<(Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>), ProxyError> {
        Ok((
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_SHADOW_ADJUSTMENT_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ENQUEUE_ERROR_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1)
                .await?,
        ))
    }

    pub(crate) async fn mark_upstream_reconciliation_run_completed_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        let mut transaction = self.sqlite_runtime.begin_scheduled_job_control().await?;
        let result = sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
        .bind(timestamp.to_string())
        .execute(&mut *transaction)
        .await;
        match result {
            Ok(_) => transaction.finish(Ok(())).await,
            Err(err) => transaction.finish(Err(ProxyError::Database(err))).await,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_observation(
        &self,
    ) -> Result<ReconciliationObservation, ProxyError> {
        let now = self.backend_time.now_ts();
        let observed_at = self
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
            .await?;
        let has_eligible: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM upstream_reconciliation_work w
                LEFT JOIN upstream_reconciliation_settlements s
                  ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                WHERE w.period_end + 600 <= ?
                  AND w.work_generation > w.completed_generation
                  AND MAX(
                      w.next_attempt_at,
                      CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                           THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END
                  ) <= ?
                  AND (
                      w.period_end + 86400 <= ?
                      OR NOT EXISTS (
                          SELECT 1
                          FROM upstream_reconciliation_research r
                          WHERE r.token_id = w.token_id
                            AND r.period_code = w.period_code
                            AND r.terminal_at IS NULL
                      )
                  )
                LIMIT 1
            )
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        let oldest_period_end: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT w.period_end
            FROM upstream_reconciliation_work w
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
            WHERE w.period_end + 600 <= ?
              AND w.work_generation > w.completed_generation
              AND MAX(
                  w.next_attempt_at,
                  CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                       THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END
              ) <= ?
              AND (
                  w.period_end + 86400 <= ?
                  OR NOT EXISTS (
                      SELECT 1
                      FROM upstream_reconciliation_research r
                      WHERE r.token_id = w.token_id
                        AND r.period_code = w.period_code
                        AND r.terminal_at IS NULL
                  )
              )
            ORDER BY w.period_end ASC
            LIMIT 1
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        let queue_estimate = if observed_at.is_some() {
            Some(
                sqlx::query_scalar::<_, i64>(&format!(
                    r#"
                    SELECT COUNT(*)
                    FROM (
                        SELECT 1
                        FROM upstream_reconciliation_work w
                        LEFT JOIN upstream_reconciliation_settlements s
                          ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                        WHERE w.period_end + 600 <= ?
                          AND w.work_generation > w.completed_generation
                          AND MAX(
                              w.next_attempt_at,
                              CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                                   THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END
                          ) <= ?
                          AND (
                              w.period_end + 86400 <= ?
                              OR NOT EXISTS (
                                  SELECT 1
                                  FROM upstream_reconciliation_research r
                                  WHERE r.token_id = w.token_id
                                    AND r.period_code = w.period_code
                                    AND r.terminal_at IS NULL
                              )
                          )
                        LIMIT {}
                    )
                    "#,
                    RECONCILIATION_QUEUE_ESTIMATE_LIMIT
                ))
                .bind(now)
                .bind(now)
                .bind(now)
                .fetch_one(&self.pool)
                .await?,
            )
        } else {
            None
        };
        Ok(ReconciliationObservation {
            observed_at,
            coverage: if observed_at.is_some() {
                "bounded".to_string()
            } else {
                "unknown".to_string()
            },
            queue_estimate,
            has_eligible,
            oldest_candidate_age_secs: oldest_period_end
                .map(|period_end| now.saturating_sub(period_end).max(0)),
        })
    }

    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_degraded_estimate(
        &self,
    ) -> Result<(i64, bool), ProxyError> {
        let limit = RECONCILIATION_QUEUE_ESTIMATE_LIMIT.saturating_add(1);
        let observed: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM (\
             SELECT 1 FROM upstream_reconciliation_settlements \
             WHERE status IN ('degraded', 'shadow_degraded') \
             LIMIT {limit}\
             )"
        ))
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)?;
        Ok((
            observed.min(RECONCILIATION_QUEUE_ESTIMATE_LIMIT),
            observed > RECONCILIATION_QUEUE_ESTIMATE_LIMIT,
        ))
    }

    pub(crate) async fn upstream_reconciliation_local_backoff_state(
        &self,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.read_reconciliation_backoff_state(
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
        )
        .await
    }

    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_local_last_recovered_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1).await
    }

    pub(crate) async fn upstream_reconciliation_continuation_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = async {
        let work_at: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT MIN(MAX(
                w.next_attempt_at,
                w.period_end + 600,
                CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                     THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END
            ))
            FROM upstream_reconciliation_work w
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
            WHERE w.work_generation > w.completed_generation
              AND (
                  w.period_end + 86400 <= ?
                  OR NOT EXISTS (
                      SELECT 1
                      FROM upstream_reconciliation_research r
                      WHERE r.token_id = w.token_id
                        AND r.period_code = w.period_code
                        AND r.terminal_at IS NULL
                  )
              )
            "#,
        )
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        let research_at: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT MIN(CASE WHEN next_poll_at > 0 THEN next_poll_at ELSE ? END)
            FROM upstream_reconciliation_research
            WHERE terminal_at IS NULL
            "#,
        )
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        // A one-time versioned lifecycle flag keeps historical source projection off the hot
        // continuation read path. New usage is maintained by triggers and therefore never
        // reopens a completed period merely because the legacy cursor is absent.
        let projection_state: Option<(i64, i64)> = sqlx::query_as(
            "SELECT completed, next_retry_at FROM upstream_reconciliation_projection_state WHERE id = 'local'",
        )
        .fetch_optional(&mut *conn)
        .await?;
        // Historical projection is local maintenance, not a main-settlement retry. Keep the
        // representative delayed while no durable candidate exists so a disabled or not-yet
        // ready reconciliation configuration cannot spin an immediate worker loop.
        let projection_at = projection_state
            .filter(|(completed, _)| *completed == 0)
            .map(|(_, next_retry_at)| next_retry_at.max(now.saturating_add(1)));
        let Some(pending_at) = (match (work_at, research_at, projection_at) {
            (Some(work_at), Some(research_at), Some(projection_at)) => {
                Some(work_at.min(research_at).min(projection_at))
            }
            (Some(work_at), Some(research_at), None) => Some(work_at.min(research_at)),
            (Some(work_at), None, Some(projection_at)) => Some(work_at.min(projection_at)),
            (None, Some(research_at), Some(projection_at)) => Some(research_at.min(projection_at)),
            (Some(work_at), None, None) => Some(work_at),
            (None, Some(research_at), None) => Some(research_at),
            (None, None, Some(projection_at)) => Some(projection_at),
            (None, None, None) => None,
        }) else {
            return Ok(None);
        };
        let (_, _, global_until) = Self::reconciliation_backoff_state_locked(
            &mut conn,
            META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
        )
        .await?;
        let (_, _, local_until) = Self::reconciliation_backoff_state_locked(
            &mut conn,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
        )
        .await?;
        Ok(Some(pending_at.max(global_until).max(local_until).max(now)))
        }
        .await;
        let close = conn.close().await;
        match (result, close) {
            (Ok(continuation_at), Ok(())) => Ok(continuation_at),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    /// Returns a runnable representative wake. Durable reconciliation state remains intact while
    /// the shadow gate is disabled, but must not wake a worker that can only short-circuit.
    pub(crate) async fn upstream_reconciliation_representative_available_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        if !self.upstream_reconciliation_shadow_ready_for_control().await?
            || !self.reconciliation_controller_allows_representative().await?
        {
            return Ok(None);
        }
        self.upstream_reconciliation_continuation_at().await
    }

    pub(crate) async fn ensure_upstream_reconciliation_representative_job(
        &self,
    ) -> Result<(), ProxyError> {
        let Some(available_at) = self
            .upstream_reconciliation_representative_available_at()
            .await?
        else {
            return Ok(());
        };
        self.scheduled_job_enqueue_at(
            "upstream_reconciliation",
            "auto",
            None,
            1,
            available_at,
        )
        .await?;
        Ok(())
    }

    async fn upstream_reconciliation_shadow_ready_for_control(&self) -> Result<bool, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = async {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT key, value FROM meta WHERE key IN (?, ?, ?)",
            )
            .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
            .bind(META_KEY_API_REBALANCE_ENABLED_V1)
            .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
            .fetch_all(&mut *conn)
            .await?;
            let values = rows.into_iter().collect::<std::collections::HashMap<_, _>>();
            let value_i64 = |key: &str| {
                values
                    .get(key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            Ok(
                values
                    .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                    .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
                    .unwrap_or_default()
                    == UpstreamProjectIdMode::AccessToken
                    && value_i64(META_KEY_API_REBALANCE_ENABLED_V1) != 0
                    && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1) != 0,
            )
        }
        .await;
        let close = conn.close().await;
        match (result, close) {
            (Ok(shadow_ready), Ok(())) => Ok(shadow_ready),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    async fn read_reconciliation_backoff_state(
        &self,
        streak_key: &str,
        level_key: &str,
        until_key: &str,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = Self::reconciliation_backoff_state_locked(
            &mut conn,
            streak_key,
            level_key,
            until_key,
        )
        .await;
        let close = conn.close().await;
        match (result, close) {
            (Ok(state), Ok(())) => Ok(state),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    async fn reconciliation_backoff_state_locked<T>(
        connection: &mut T,
        streak_key: &str,
        level_key: &str,
        until_key: &str,
    ) -> Result<(i64, i64, i64), ProxyError>
    where
        T: std::ops::DerefMut<Target = sqlx::SqliteConnection>,
    {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, value FROM meta WHERE key IN (?, ?, ?)",
        )
        .bind(streak_key)
        .bind(level_key)
        .bind(until_key)
        .fetch_all(&mut **connection)
        .await?;
        let values = rows.into_iter().collect::<std::collections::HashMap<_, _>>();
        let value_i64 = |key: &str| {
            values
                .get(key)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
        };
        Ok((value_i64(streak_key), value_i64(level_key), value_i64(until_key)))
    }

    async fn reconciliation_claim_is_current_locked<T>(
        transaction: &mut T,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError>
    where
        T: std::ops::DerefMut<Target = sqlx::SqliteConnection>,
    {
        let Some((job_id, claim_generation)) = claimed_job else {
            return Ok(true);
        };
        let current: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM scheduled_jobs
                WHERE id = ? AND status = 'running' AND claim_generation = ?
            )
            "#,
        )
        .bind(job_id)
        .bind(claim_generation)
        .fetch_one(&mut **transaction)
        .await?;
        Ok(current != 0)
    }

    async fn update_upstream_reconciliation_local_backoff_inner(
        &self,
        pressure: bool,
        now: i64,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        let result = Self::apply_upstream_reconciliation_local_backoff_locked(
            &mut transaction,
            pressure,
            now,
            claimed_job,
        )
        .await;
        let (streak, level, until) = match result {
            Ok(state) => state,
            Err(error) => {
                transaction.finish(Err(error)).await?;
                unreachable!("failed reconciliation local backoff transaction committed")
            }
        };
        Self::sync_upstream_reconciliation_representative_locked(
            &mut transaction,
            now,
            claimed_job,
        )
        .await?;
        transaction.finish(Ok(())).await?;
        Ok((streak, level, until))
    }

    pub(crate) async fn update_upstream_reconciliation_local_backoff(
        &self,
        pressure: bool,
        now: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.update_upstream_reconciliation_local_backoff_inner(pressure, now, None)
            .await
    }

    pub(crate) async fn update_upstream_reconciliation_local_backoff_claimed(
        &self,
        pressure: bool,
        now: i64,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.update_upstream_reconciliation_local_backoff_inner(
            pressure,
            now,
            Some((job_id, claim_generation)),
        )
        .await
    }

    pub(crate) async fn record_upstream_reconciliation_run_stats(
        &self,
        duration_ms: i64,
        attempted: i64,
        settled: i64,
        no_adjustment: i64,
        upstream_429: i64,
        budget_exhausted: bool,
    ) -> Result<(), ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        let result = sqlx::query(
            r#"INSERT INTO meta (key, value) VALUES
                   (?, ?), (?, ?), (?, ?), (?, ?), (?, ?), (?, ?)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1)
        .bind(duration_ms.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1)
        .bind(attempted.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1)
        .bind(settled.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1)
        .bind(no_adjustment.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1)
        .bind(upstream_429.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1)
        .bind(if budget_exhausted { "1" } else { "0" })
        .execute(&mut *transaction)
        .await;
        match result {
            Ok(_) => transaction.finish(Ok(())).await,
            Err(err) => transaction.finish(Err(ProxyError::Database(err))).await,
        }
    }

    async fn update_upstream_reconciliation_global_backoff_inner(
        &self,
        pressure: bool,
        now: i64,
        retry_after_until: Option<i64>,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        let (previous_streak, previous_level, _) = Self::reconciliation_backoff_state_locked(
            &mut transaction,
            META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
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
                1 => 2 * 60,
                2 => 5 * 60,
                3 => 10 * 60,
                4 => 30 * 60,
                _ => 0,
            };
            (
                streak,
                level,
                now.saturating_add(delay_secs).max(retry_after_until.unwrap_or_default()),
            )
        } else {
            (0, 0, 0)
        };
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query(
            r#"INSERT INTO meta (key, value) VALUES (?, ?), (?, ?), (?, ?)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1)
        .bind(streak.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1)
        .bind(level.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1)
        .bind(until.to_string())
        .execute(&mut *transaction)
        .await?;
        if !pressure && previous_level > 0 {
            sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RECOVERED_AT_V1)
                .bind(now.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        Self::sync_upstream_reconciliation_representative_locked(
            &mut transaction,
            now,
            claimed_job,
        )
        .await?;
        transaction.finish(Ok(())).await?;
        Ok((streak, level, until))
    }

    pub(crate) async fn update_upstream_reconciliation_global_backoff(
        &self,
        pressure: bool,
        now: i64,
        retry_after_until: Option<i64>,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.update_upstream_reconciliation_global_backoff_inner(
            pressure,
            now,
            retry_after_until,
            None,
        )
        .await
    }

    pub(crate) async fn update_upstream_reconciliation_global_backoff_claimed(
        &self,
        pressure: bool,
        now: i64,
        retry_after_until: Option<i64>,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        self.update_upstream_reconciliation_global_backoff_inner(
            pressure,
            now,
            retry_after_until,
            Some((job_id, claim_generation)),
        )
        .await
    }

    pub(crate) async fn mark_upstream_reconciliation_enqueue_error_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ENQUEUE_ERROR_AT_V1, timestamp)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn mark_upstream_reconciliation_research_sweep_at_claimed(
        &self,
        timestamp: i64,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<(), ProxyError> {
        self.mark_upstream_reconciliation_research_sweep_at_inner(
            timestamp,
            Some((job_id, claim_generation)),
        )
        .await
    }

    #[cfg(test)]
    async fn mark_upstream_reconciliation_research_sweep_at_inner(
        &self,
        timestamp: i64,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
            .bind(timestamp.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.finish(Ok(())).await
    }

    pub(crate) async fn record_upstream_reconciliation_usage(
        &self,
        token_id: &str,
        key_id: &str,
        billing_subject: &str,
        research_request_id: Option<&str>,
    ) -> Result<Option<BusinessPeriod>, ProxyError> {
        let settings = self.get_system_settings().await?;
        if !upstream_reconciliation_shadow_ready(&settings) {
            return Ok(None);
        }
        let now = self.backend_time.now_ts();
        let period = business_period_for_timestamp(now);
        let Some(settlement_mode) = self
            .reconciliation_settlement_mode_for_period(&settings, &period)
            .await?
        else {
            return Ok(None);
        };
        let project_id = self
            .derive_upstream_project_id(token_id, &period.code)
            .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                settlement_mode, period_start, period_end, request_count,
                first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
            ON CONFLICT(token_id, key_id, period_code) DO UPDATE SET
                request_count = upstream_reconciliation_usage.request_count + 1,
                last_used_at = excluded.last_used_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(token_id)
        .bind(key_id)
        .bind(&period.code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(settlement_mode)
        .bind(period.starts_at)
        .bind(period.ends_at)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if let Some(request_id) = research_request_id {
            sqlx::query(
                r#"
                INSERT INTO upstream_reconciliation_research (
                    request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, NULL, ?)
                ON CONFLICT(request_id) DO UPDATE SET updated_at = excluded.updated_at
                "#,
            )
            .bind(request_id)
            .bind(token_id)
            .bind(key_id)
            .bind(&period.code)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.ensure_upstream_reconciliation_representative_job().await?;
        Ok(Some(period))
    }

    async fn load_or_initialize_upstream_reconciliation_ready_after(
        &self,
        now: i64,
    ) -> Result<i64, ProxyError> {
        if let Some(ready_after) = self
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1)
            .await?
            .filter(|value| *value > 0)
        {
            return Ok(ready_after);
        }
        let ready_after = business_period_for_timestamp(now).ends_at;
        self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1, ready_after)
            .await?;
        Ok(ready_after)
    }

    pub(crate) async fn mark_upstream_reconciliation_research_terminal(
        &self,
        request_id: &str,
    ) -> Result<bool, ProxyError> {
        self.mark_upstream_reconciliation_research_terminal_inner(request_id, None)
            .await
    }

    pub(crate) async fn mark_upstream_reconciliation_research_terminal_claimed(
        &self,
        request_id: &str,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<bool, ProxyError> {
        self.mark_upstream_reconciliation_research_terminal_inner(
            request_id,
            Some((job_id, claim_generation)),
        )
        .await
    }

    async fn mark_upstream_reconciliation_research_terminal_inner(
        &self,
        request_id: &str,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        let changed = sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET terminal_at = ?,
                last_polled_at = ?,
                next_poll_at = 0,
                poll_attempt_count = poll_attempt_count + 1,
                last_poll_outcome = 'terminal',
                last_poll_error_kind = NULL,
                updated_at = ?
            WHERE request_id = ? AND terminal_at IS NULL
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(request_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed > 0 {
            sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1)
                .bind(now.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        transaction.finish(Ok(())).await?;
        Ok(changed > 0)
    }

    pub(crate) async fn record_upstream_reconciliation_research_poll(
        &self,
        request_id: &str,
        next_poll_at: i64,
        outcome: &str,
        error_kind: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_upstream_reconciliation_research_poll_inner(
            request_id,
            next_poll_at,
            outcome,
            error_kind,
            None,
        )
        .await
    }

    pub(crate) async fn record_upstream_reconciliation_research_poll_claimed(
        &self,
        request_id: &str,
        next_poll_at: i64,
        outcome: &str,
        error_kind: Option<&str>,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<(), ProxyError> {
        self.record_upstream_reconciliation_research_poll_inner(
            request_id,
            next_poll_at,
            outcome,
            error_kind,
            Some((job_id, claim_generation)),
        )
        .await
    }

    async fn record_upstream_reconciliation_research_poll_inner(
        &self,
        request_id: &str,
        next_poll_at: i64,
        outcome: &str,
        error_kind: Option<&str>,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET last_polled_at = ?, next_poll_at = ?, poll_attempt_count = poll_attempt_count + 1,
                last_poll_outcome = ?, last_poll_error_kind = ?, updated_at = ?
            WHERE request_id = ? AND terminal_at IS NULL
            "#,
        )
        .bind(now)
        .bind(next_poll_at)
        .bind(outcome)
        .bind(error_kind)
        .bind(now)
        .bind(request_id)
        .execute(&mut *transaction)
        .await?;
        transaction.finish(Ok(())).await
    }

    fn build_upstream_reconciliation_candidates(
        &self,
        rows: Vec<UpstreamReconciliationCandidateRow>,
        now: i64,
    ) -> Vec<UpstreamReconciliationCandidateWork> {
        rows.into_iter()
            .filter_map(
                |(
                    token_id,
                    period_code,
                    project_id,
                    billing_subject,
                    settlement_mode,
                    period_start,
                    period_end,
                    pending_research,
                    work_generation,
                    _scheduling_key_id,
                )| {
                    let degraded = pending_research > 0
                        && now >= period_end.saturating_add(86_400);
                    if pending_research > 0 && !degraded {
                        return None;
                    }
                    Some(UpstreamReconciliationCandidateWork {
                        candidate: UpstreamReconciliationCandidate {
                            token_id,
                            period_code,
                            project_id,
                            billing_subject,
                            settlement_mode,
                            period_start,
                            period_end,
                            pending_research,
                            degraded,
                        },
                        work_generation,
                    })
                },
            )
            .collect()
    }

    /// Advance the legacy usage-to-work bootstrap independently from candidate
    /// selection. The live usage triggers maintain new work synchronously, so
    /// this bounded slice is only for historical rows that predate them.
    pub(crate) async fn advance_upstream_reconciliation_work_projection(
        &self,
    ) -> Result<ReconciliationProjectionSliceOutcome, ProxyError> {
        ReconciliationProjectionController::new(self)
            .advance_slice(None)
            .await
    }

    pub(crate) async fn advance_upstream_reconciliation_work_projection_claimed(
        &self,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<ReconciliationProjectionSliceOutcome, ProxyError> {
        ReconciliationProjectionController::new(self)
            .advance_slice(Some((job_id, claim_generation)))
            .await
    }

    async fn query_upstream_reconciliation_candidates(
        &self,
        now: i64,
        limit: i64,
        newest_first: bool,
        scope: ReconciliationCandidateScope,
    ) -> Result<Vec<UpstreamReconciliationCandidateWork>, ProxyError> {
        let read_kind = match &scope {
            ReconciliationCandidateScope::Recent { .. } => ReconciliationReadKind::CandidateRecent,
            ReconciliationCandidateScope::Backlog { .. } => ReconciliationReadKind::CandidateBacklog,
        };
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            WITH eligible AS (
            SELECT
                w.token_id,
                w.period_code,
                w.project_id,
                w.billing_subject,
                w.settlement_mode,
                w.period_start,
                w.period_end,
                w.scheduling_key_id,
                w.work_generation,
                CASE WHEN EXISTS (
                    SELECT 1
                    FROM upstream_reconciliation_research r
                    WHERE r.token_id = w.token_id
                      AND r.period_code = w.period_code
                      AND r.terminal_at IS NULL
                ) THEN 1 ELSE 0 END AS pending_research
            FROM upstream_reconciliation_work w
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
            WHERE w.period_end + 600 <= "#,
        );
        query
            .push_bind(now)
            .push(r#"
              AND w.work_generation > w.completed_generation
              AND MAX(
                  w.next_attempt_at,
                  CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                       THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END
              ) <= "#)
            .push_bind(now)
            .push(" ");
        match scope {
            ReconciliationCandidateScope::Recent { start, end } => {
                query
                    .push(" AND w.period_end >= ")
                    .push_bind(start)
                    .push(" AND w.period_end < ")
                    .push_bind(end);
            }
            ReconciliationCandidateScope::Backlog { before } => {
                query
                    .push(" AND w.period_end < ")
                    .push_bind(before);
            }
        }
        query
            .push(" AND (w.period_end + 86400 <= ")
            .push_bind(now)
            .push(
                r#" OR NOT EXISTS (
                    SELECT 1
                    FROM upstream_reconciliation_research r
                    WHERE r.token_id = w.token_id
                      AND r.period_code = w.period_code
                      AND r.terminal_at IS NULL
                ))
            ), ranked AS (
              SELECT eligible.*,
                     ROW_NUMBER() OVER (
                       PARTITION BY scheduling_key_id
                       ORDER BY period_end "#,
            )
            .push(if newest_first { "DESC" } else { "ASC" })
            .push(
                r#", token_id, period_code
                     ) AS key_slot
              FROM eligible
              WHERE pending_research = 0 OR period_end + 86400 <= "#,
            )
            .push_bind(now)
            .push(
                r#")
            SELECT
                token_id,
                period_code,
                project_id,
                billing_subject,
                settlement_mode,
                period_start,
                period_end,
                pending_research,
                work_generation,
                scheduling_key_id
            FROM ranked
            ORDER BY key_slot ASC, period_end "#,
            )
            .push(if newest_first { "DESC" } else { "ASC" })
            .push(
                r#", token_id ASC, period_code ASC
            LIMIT "#,
            )
            .push_bind(limit.max(1));
        let mut session = self.sqlite_runtime.begin_reconciliation_read(read_kind).await?;
        let rows_result = query
            .build_query_as::<UpstreamReconciliationCandidateRow>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        Ok(self.build_upstream_reconciliation_candidates(rows, now))
    }

    pub(crate) async fn next_upstream_reconciliation_candidates(
        &self,
        limit: i64,
    ) -> Result<UpstreamReconciliationCandidateBatch, ProxyError> {
        let now = self.backend_time.now_ts();
        let total_limit = limit.max(1);
        let day_window = server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let recent_start = day_window.start.saturating_sub(SECS_PER_DAY);
        let recent_end = day_window.end;
        let recent_lane_budget = total_limit.min(RECONCILIATION_RECENT_LANE_BUDGET);
        let backlog_lane_budget =
            total_limit.saturating_sub(recent_lane_budget).min(RECONCILIATION_BACKLOG_LANE_BUDGET);
        let recent_candidates = self
            .query_upstream_reconciliation_candidates(
                now,
                total_limit,
                true,
                ReconciliationCandidateScope::Recent {
                    start: recent_start,
                    end: recent_end,
                },
            )
            .await?;
        let backlog_candidates = self
            .query_upstream_reconciliation_candidates(
                now,
                total_limit,
                false,
                ReconciliationCandidateScope::Backlog {
                    before: recent_start,
                },
            )
            .await?;

        let mut recent_candidate_count =
            std::cmp::min(recent_candidates.len() as i64, recent_lane_budget);
        let mut backlog_candidate_count = std::cmp::min(
            backlog_candidates.len() as i64,
            std::cmp::min(
                backlog_lane_budget,
                total_limit.saturating_sub(recent_candidate_count),
            ),
        );
        let mut remaining_capacity =
            total_limit.saturating_sub(recent_candidate_count + backlog_candidate_count);
        let extra_recent_available =
            (recent_candidates.len() as i64).saturating_sub(recent_candidate_count);
        let extra_recent = std::cmp::min(extra_recent_available, remaining_capacity);
        recent_candidate_count += extra_recent;
        remaining_capacity = remaining_capacity.saturating_sub(extra_recent);
        let extra_backlog_available =
            (backlog_candidates.len() as i64).saturating_sub(backlog_candidate_count);
        let extra_backlog = std::cmp::min(extra_backlog_available, remaining_capacity);
        backlog_candidate_count += extra_backlog;

        let mut selected =
            Vec::with_capacity((recent_candidate_count + backlog_candidate_count) as usize);
        selected.extend(
            recent_candidates
                .iter()
                .take(recent_candidate_count as usize)
                .cloned(),
        );
        selected.extend(
            backlog_candidates
                .iter()
                .take(backlog_candidate_count as usize)
                .cloned(),
        );
        let work_generation_by_candidate = selected
            .iter()
            .map(|work| {
                (
                    (
                        work.candidate.token_id.clone(),
                        work.candidate.period_code.clone(),
                    ),
                    work.work_generation,
                )
            })
            .collect();
        let candidates = selected
            .into_iter()
            .map(|work| work.candidate)
            .collect();
        Ok(UpstreamReconciliationCandidateBatch {
            candidates,
            work_generation_by_candidate,
            recent_lane_budget,
            backlog_lane_budget,
            recent_candidate_count,
            backlog_candidate_count,
        })
    }

    pub(crate) async fn reconciliation_key_ids_batch(
        &self,
        candidates: &[(String, String)],
    ) -> Result<std::collections::HashMap<(String, String), Vec<String>>, ProxyError> {
        if candidates.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT token_id, period_code, key_id FROM upstream_reconciliation_usage WHERE ",
        );
        for (index, (token_id, period_code)) in candidates.iter().enumerate() {
            if index > 0 {
                query.push(" OR ");
            }
            query
                .push("(token_id = ")
                .push_bind(token_id)
                .push(" AND period_code = ")
                .push_bind(period_code)
                .push(")");
        }
        query.push(" ORDER BY token_id ASC, period_code ASC, key_id ASC");
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::CandidateHydrate)
            .await?;
        let rows_result = query
            .build_query_as::<(String, String, String)>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        let mut grouped = std::collections::HashMap::new();
        for (token_id, period_code, key_id) in rows {
            grouped
                .entry((token_id, period_code))
                .or_insert_with(Vec::new)
                .push(key_id);
        }
        Ok(grouped)
    }

    pub(crate) async fn reconciliation_local_billed_credits_batch(
        &self,
        candidates: &[UpstreamReconciliationCandidate],
    ) -> Result<std::collections::HashMap<(String, String), i64>, ProxyError> {
        if candidates.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "WITH requested(token_id, period_code, period_start, period_end) AS (VALUES ",
        );
        for (index, candidate) in candidates.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query
                .push("(")
                .push_bind(&candidate.token_id)
                .push(", ")
                .push_bind(&candidate.period_code)
                .push(", ")
                .push_bind(candidate.period_start)
                .push(", ")
                .push_bind(candidate.period_end)
                .push(")");
        }
        query.push(
            r#")
            SELECT requested.token_id, requested.period_code,
                   COALESCE(SUM(business_credits), 0)
            FROM requested
            LEFT JOIN billing_ledger
              ON billing_ledger.token_id = requested.token_id
             AND billing_ledger.billing_state = 'charged'
             AND billing_ledger.created_at >= requested.period_start
             AND billing_ledger.created_at < requested.period_end
             AND COALESCE(billing_ledger.business_credits, 0) > 0
            GROUP BY requested.token_id, requested.period_code
            "#,
        );
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::BillingHydrate)
            .await?;
        let rows_result = query
            .build_query_as::<(String, String, i64)>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.complete_query_or_defer(rows_result).await?;
        Ok(rows
            .into_iter()
            .map(|(token_id, period_code, credits)| ((token_id, period_code), credits))
            .collect())
    }

    pub(crate) async fn reconciliation_local_billed_credits_for_finalization(
        &self,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<i64, ProxyError> {
        let mut connection = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ReconciliationProjection)
            .await?;
        let query_result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(business_credits), 0)
            FROM billing_ledger
            WHERE token_id = ?
              AND billing_state = 'charged'
              AND created_at >= ?
              AND created_at < ?
              AND COALESCE(business_credits, 0) > 0
            "#,
        )
        .bind(&candidate.token_id)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .fetch_one(&mut *connection)
        .await;
        connection.complete_query(query_result).await
    }

    pub(crate) async fn reserve_upstream_usage_attempt(
        &self,
        key_id: &str,
    ) -> Result<Result<(), i64>, ProxyError> {
        let now = self.backend_time.now_ts();
        let threshold = now - 600;
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        sqlx::query("DELETE FROM upstream_usage_rate_attempts WHERE attempted_at <= ?")
            .bind(threshold)
            .execute(&mut *tx)
            .await?;
        let attempts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upstream_usage_rate_attempts WHERE key_id = ? AND attempted_at > ?",
        )
        .bind(key_id)
        .bind(threshold)
        .fetch_one(&mut *tx)
        .await?;
        if attempts >= 10 {
            let oldest: i64 = sqlx::query_scalar(
                "SELECT MIN(attempted_at) FROM upstream_usage_rate_attempts WHERE key_id = ? AND attempted_at > ?",
            )
            .bind(key_id)
            .bind(threshold)
            .fetch_one(&mut *tx)
            .await?;
            tx.finish(Ok(())).await?;
            return Ok(Err(oldest.saturating_add(600)));
        }
        sqlx::query(
            "INSERT INTO upstream_usage_rate_attempts (id, key_id, attempted_at) VALUES (?, ?, ?)",
        )
        .bind(nanoid!(18))
        .bind(key_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.finish(Ok(())).await?;
        Ok(Ok(()))
    }

    async fn lock_reconciliation_work_generation(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<bool, ProxyError> {
        let Some(fence) = fence else {
            return Ok(true);
        };
        let expected_generation = fence.work_generation;
        let expected_claim = fence.claimed_job;
        let (claim_job_id, claim_generation) = expected_claim
            .map(|(job_id, generation)| (Some(job_id), Some(generation)))
            .unwrap_or((None, None));
        let locked = sqlx::query(
            r#"
            UPDATE upstream_reconciliation_work
            SET updated_at = updated_at
            WHERE token_id = ? AND period_code = ?
              AND work_generation = ?
              AND completed_generation < work_generation
              AND (
                  ? IS NULL
                  OR EXISTS (
                      SELECT 1 FROM scheduled_jobs
                      WHERE id = ? AND status = 'running' AND claim_generation = ?
                  )
              )
            "#,
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(expected_generation)
        .bind(claim_job_id)
        .bind(claim_job_id)
        .bind(claim_generation)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        Ok(locked > 0)
    }

    async fn claim_reconciliation_work_completion(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
        fence: Option<ReconciliationWorkFence>,
        outcome: &str,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let Some(fence) = fence else {
            return Ok(true);
        };
        let expected_generation = fence.work_generation;
        let expected_claim = fence.claimed_job;
        let (claim_job_id, claim_generation) = expected_claim
            .map(|(job_id, generation)| (Some(job_id), Some(generation)))
            .unwrap_or((None, None));
        let claimed = sqlx::query(
            r#"
            UPDATE upstream_reconciliation_work
            SET completed_generation = ?,
                next_attempt_at = 0,
                last_outcome = ?,
                transport_failure_streak = 0,
                transport_retry_at = 0,
                semantic_failure_streak = 0,
                semantic_retry_at = 0,
                updated_at = ?
            WHERE token_id = ? AND period_code = ?
              AND work_generation = ?
              AND completed_generation < work_generation
              AND (
                  ? IS NULL
                  OR EXISTS (
                      SELECT 1 FROM scheduled_jobs
                      WHERE id = ? AND status = 'running' AND claim_generation = ?
                  )
              )
            "#,
        )
        .bind(expected_generation)
        .bind(outcome)
        .bind(now)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(expected_generation)
        .bind(claim_job_id)
        .bind(claim_job_id)
        .bind(claim_generation)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        Ok(claimed > 0)
    }

    async fn mark_reconciliation_work_completed(
        &self,
        tx: &mut SqliteImmediateTransaction,
        candidate: &UpstreamReconciliationCandidate,
        expected_generation: Option<i64>,
        outcome: &str,
        now: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE upstream_reconciliation_work
            SET completed_generation = COALESCE(?, work_generation),
                next_attempt_at = 0,
                last_outcome = ?,
                transport_failure_streak = 0,
                transport_retry_at = 0,
                semantic_failure_streak = 0,
                semantic_retry_at = 0,
                updated_at = ?
            WHERE token_id = ? AND period_code = ?
              AND completed_generation < work_generation
              AND (? IS NULL OR work_generation = ?)
            "#,
        )
        .bind(expected_generation)
        .bind(outcome)
        .bind(now)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(expected_generation)
        .bind(expected_generation)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_reconciliation_retry(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        status: &str,
        next_attempt_at: i64,
        reason: Option<&str>,
        outcome: &str,
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let settlement_key = format!("v1:{}:{}", candidate.token_id, candidate.period_code);
        let normalized_reason = reason.map(|value| classify_reconciliation_retry_reason(Some(value)));
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        if !self
            .lock_reconciliation_work_generation(&mut tx, candidate, fence)
            .await?
        {
            tx.rollback().await?;
            return Ok(());
        }
        let expected_generation = fence.map(|fence| fence.work_generation);
        let (transport_streak, persisted_transport_retry_at, semantic_streak, persisted_semantic_retry_at):
            (i64, i64, i64, i64) = sqlx::query_as(
                r#"SELECT transport_failure_streak, transport_retry_at,
                          semantic_failure_streak, semantic_retry_at
                   FROM upstream_reconciliation_work
                   WHERE token_id = ? AND period_code = ?"#,
            )
            .bind(&candidate.token_id)
            .bind(&candidate.period_code)
            .fetch_one(&mut *tx)
            .await?;
        let transport_retry_at = match outcome {
            RECONCILIATION_OUTCOME_TRANSPORT_FAILURE => {
                Some(now.saturating_add(
                    [30, 60, 120, 300][transport_streak.min(3) as usize],
                ))
            }
            _ => None,
        };
        let semantic_retry_at = match outcome {
            RECONCILIATION_OUTCOME_SEMANTIC_FAILURE => {
                Some(now.saturating_add(
                    [300, 900, 1800, 3600][semantic_streak.min(3) as usize],
                ))
            }
            _ => None,
        };
        let effective_next_attempt_at = next_attempt_at
            .max(transport_retry_at.unwrap_or(persisted_transport_retry_at))
            .max(semantic_retry_at.unwrap_or(persisted_semantic_retry_at));
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, degraded_reason, next_attempt_at,
                attempt_count, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
            ON CONFLICT(settlement_key) DO UPDATE SET
                status = excluded.status,
                degraded_reason = excluded.degraded_reason,
                next_attempt_at = excluded.next_attempt_at,
                attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(settlement_key)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(&candidate.project_id)
        .bind(&candidate.billing_subject)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .bind(status)
        .bind(normalized_reason)
        .bind(effective_next_attempt_at)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE upstream_reconciliation_work
            SET next_attempt_at = ?, last_outcome = ?,
                transport_failure_streak = transport_failure_streak + CASE WHEN ? = 'transport_failure' THEN 1 ELSE 0 END,
                transport_retry_at = CASE WHEN ? = 'transport_failure' THEN ? ELSE transport_retry_at END,
                semantic_failure_streak = semantic_failure_streak + CASE WHEN ? = 'semantic_failure' THEN 1 ELSE 0 END,
                semantic_retry_at = CASE WHEN ? = 'semantic_failure' THEN ? ELSE semantic_retry_at END,
                updated_at = ?
            WHERE token_id = ? AND period_code = ?
              AND completed_generation < work_generation
              AND (? IS NULL OR work_generation = ?)
            "#,
        )
        .bind(effective_next_attempt_at)
        .bind(outcome)
        .bind(outcome)
        .bind(outcome)
        .bind(transport_retry_at.unwrap_or(0))
        .bind(outcome)
        .bind(outcome)
        .bind(semantic_retry_at.unwrap_or(0))
        .bind(now)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(expected_generation)
        .bind(expected_generation)
        .execute(&mut *tx)
        .await?;
        tx.finish(Ok(())).await?;
        self.ensure_upstream_reconciliation_representative_job().await?;
        Ok(())
    }

    pub(crate) async fn settle_upstream_reconciliation(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        upstream_usage: i64,
        local_billed_credits: i64,
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let settlement_key = format!("v1:{}:{}", candidate.token_id, candidate.period_code);
        let delta = upstream_usage.saturating_sub(local_billed_credits);
        let attributed_at = candidate.period_end.saturating_sub(60);
        let minute_bucket = attributed_at - attributed_at.rem_euclid(SECS_PER_MINUTE);
        let same_local_day = local_day_bucket_start_utc_ts(attributed_at)
            == local_day_bucket_start_utc_ts(now);
        let attributed_utc = Utc
            .timestamp_opt(attributed_at, 0)
            .single()
            .unwrap_or_else(|| self.backend_time.now_utc());
        let day_bucket = start_of_local_day_utc_ts(attributed_utc.with_timezone(&Local));
        let month_start = start_of_month(attributed_utc).timestamp();
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        let completion_outcome = if delta == 0 {
            RECONCILIATION_OUTCOME_NO_ADJUSTMENT
        } else {
            RECONCILIATION_OUTCOME_SETTLED
        };
        let expected_generation = fence.map(|fence| fence.work_generation);
        if !self
            .claim_reconciliation_work_completion(
                &mut tx,
                candidate,
                fence,
                completion_outcome,
                now,
            )
            .await?
        {
            tx.rollback().await?;
            return Ok(false);
        }
        if delta == 0 {
            sqlx::query(
                r#"
                INSERT INTO upstream_reconciliation_settlements (
                    settlement_key, token_id, period_code, project_id, billing_subject,
                    period_start, period_end, status, upstream_usage, local_billed_credits,
                    delta_credits, degraded_reason, next_attempt_at, attempt_count,
                    created_at, updated_at, settled_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, NULL, 1, ?, ?, ?)
                ON CONFLICT(settlement_key) DO UPDATE SET
                    status = excluded.status,
                    upstream_usage = excluded.upstream_usage,
                    local_billed_credits = excluded.local_billed_credits,
                    delta_credits = excluded.delta_credits,
                    degraded_reason = excluded.degraded_reason,
                    next_attempt_at = NULL,
                    attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                    updated_at = excluded.updated_at,
                    settled_at = excluded.settled_at
                "#,
            )
            .bind(&settlement_key)
            .bind(&candidate.token_id)
            .bind(&candidate.period_code)
            .bind(&candidate.project_id)
            .bind(&candidate.billing_subject)
            .bind(candidate.period_start)
            .bind(candidate.period_end)
            .bind(if candidate.degraded {
                RECONCILIATION_STATUS_DEGRADED
            } else {
                RECONCILIATION_STATUS_SETTLED
            })
            .bind(upstream_usage)
            .bind(local_billed_credits)
            .bind(candidate.degraded.then_some("research_timeout_24h"))
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            self.mark_reconciliation_work_completed(
                &mut tx,
                candidate,
                expected_generation,
                RECONCILIATION_OUTCOME_NO_ADJUSTMENT,
                now,
            )
            .await?;
            tx.finish(Ok(())).await?;
            return Ok(true);
        }
        let inserted = sqlx::query(
            r#"
            INSERT OR IGNORE INTO billing_reconciliation_adjustments (
                settlement_key, token_id, billing_subject, period_code, delta_credits,
                attributed_at, degraded_reason, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&settlement_key)
        .bind(&candidate.token_id)
        .bind(&candidate.billing_subject)
        .bind(&candidate.period_code)
        .bind(delta)
        .bind(attributed_at)
        .bind(candidate.degraded.then_some("research_timeout_24h"))
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            self.mark_reconciliation_work_completed(
                &mut tx,
                candidate,
                expected_generation,
                RECONCILIATION_OUTCOME_SETTLED,
                now,
            )
            .await?;
            tx.finish(Ok(())).await?;
            return Ok(false);
        }
        let (subject_kind, subject_id) = candidate
            .billing_subject
            .split_once(':')
            .ok_or_else(|| ProxyError::Other("invalid reconciliation billing subject".to_string()))?;
        let (usage_table, id_column, monthly_table) = match subject_kind {
            "account" => ("account_usage_buckets", "user_id", "account_monthly_quota"),
            "token" => ("token_usage_buckets", "token_id", "auth_token_quota"),
            _ => {
                return Err(ProxyError::Other(
                    "unsupported reconciliation billing subject".to_string(),
                ));
            }
        };
        let mut quota_buckets = Vec::with_capacity(2);
        if same_local_day {
            quota_buckets.push((minute_bucket, GRANULARITY_MINUTE));
        }
        quota_buckets.push((day_bucket, GRANULARITY_DAY));
        for (bucket_start, granularity) in quota_buckets {
            let insert_sql = format!(
                "INSERT OR IGNORE INTO {usage_table} ({id_column}, bucket_start, granularity, count) VALUES (?, ?, ?, 0)"
            );
            sqlx::query(&insert_sql)
                .bind(subject_id)
                .bind(bucket_start)
                .bind(granularity)
                .execute(&mut *tx)
                .await?;
            let update_sql = format!(
                "UPDATE {usage_table} SET count = MAX(0, count + ?) WHERE {id_column} = ? AND bucket_start = ? AND granularity = ?"
            );
            sqlx::query(&update_sql)
                .bind(delta)
                .bind(subject_id)
                .bind(bucket_start)
                .bind(granularity)
                .execute(&mut *tx)
                .await?;
        }
        let monthly_id = if subject_kind == "account" {
            "user_id"
        } else {
            "token_id"
        };
        let monthly_insert = format!(
            "INSERT OR IGNORE INTO {monthly_table} ({monthly_id}, month_start, month_count) VALUES (?, ?, 0)"
        );
        sqlx::query(&monthly_insert)
            .bind(subject_id)
            .bind(month_start)
            .execute(&mut *tx)
            .await?;
        let monthly_update = format!(
            "UPDATE {monthly_table} SET month_count = CASE WHEN month_start = ? THEN MAX(0, month_count + ?) ELSE month_count END WHERE {monthly_id} = ?"
        );
        sqlx::query(&monthly_update)
            .bind(month_start)
            .bind(delta)
            .bind(subject_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, upstream_usage, local_billed_credits,
                delta_credits, degraded_reason, next_attempt_at, attempt_count,
                created_at, updated_at, settled_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 1, ?, ?, ?)
            ON CONFLICT(settlement_key) DO UPDATE SET
                status = excluded.status,
                upstream_usage = excluded.upstream_usage,
                local_billed_credits = excluded.local_billed_credits,
                delta_credits = excluded.delta_credits,
                degraded_reason = excluded.degraded_reason,
                next_attempt_at = NULL,
                attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                updated_at = excluded.updated_at,
                settled_at = excluded.settled_at
            "#,
        )
        .bind(&settlement_key)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(&candidate.project_id)
        .bind(&candidate.billing_subject)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .bind(if candidate.degraded {
            RECONCILIATION_STATUS_DEGRADED
        } else {
            RECONCILIATION_STATUS_SETTLED
        })
        .bind(upstream_usage)
        .bind(local_billed_credits)
        .bind(delta)
        .bind(candidate.degraded.then_some("research_timeout_24h"))
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        self.mark_reconciliation_work_completed(
            &mut tx,
            candidate,
            expected_generation,
            RECONCILIATION_OUTCOME_SETTLED,
            now,
        )
        .await?;
        tx.finish(Ok(())).await?;
        Ok(true)
    }

    pub(crate) async fn settle_upstream_reconciliation_shadow(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        upstream_usage: i64,
        local_billed_credits: i64,
        fence: Option<ReconciliationWorkFence>,
    ) -> Result<bool, ProxyError> {
        let started_at = std::time::Instant::now();
        let now = self.backend_time.now_ts();
        let settlement_key = format!("v1:{}:{}", candidate.token_id, candidate.period_code);
        let delta = upstream_usage.saturating_sub(local_billed_credits);
        let attributed_at = candidate.period_end.saturating_sub(60);
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        let completion_outcome = if delta == 0 {
            RECONCILIATION_OUTCOME_NO_ADJUSTMENT
        } else {
            RECONCILIATION_OUTCOME_OBSERVED
        };
        let expected_generation = fence.map(|fence| fence.work_generation);
        if !self
            .claim_reconciliation_work_completion(
                &mut tx,
                candidate,
                fence,
                completion_outcome,
                now,
            )
            .await?
        {
            tx.rollback().await?;
            return Ok(false);
        }
        if delta == 0 {
            sqlx::query(
                r#"
                INSERT INTO upstream_reconciliation_settlements (
                    settlement_key, token_id, period_code, project_id, billing_subject,
                    period_start, period_end, status, upstream_usage, local_billed_credits,
                    delta_credits, degraded_reason, next_attempt_at, attempt_count,
                    created_at, updated_at, settled_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, NULL, 1, ?, ?, ?)
                ON CONFLICT(settlement_key) DO UPDATE SET
                    status = excluded.status,
                    upstream_usage = excluded.upstream_usage,
                    local_billed_credits = excluded.local_billed_credits,
                    delta_credits = excluded.delta_credits,
                    degraded_reason = excluded.degraded_reason,
                    next_attempt_at = NULL,
                    attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                    updated_at = excluded.updated_at,
                    settled_at = excluded.settled_at
                "#,
            )
            .bind(&settlement_key)
            .bind(&candidate.token_id)
            .bind(&candidate.period_code)
            .bind(&candidate.project_id)
            .bind(&candidate.billing_subject)
            .bind(candidate.period_start)
            .bind(candidate.period_end)
            .bind(if candidate.degraded {
                RECONCILIATION_STATUS_SHADOW_DEGRADED
            } else {
                RECONCILIATION_STATUS_SHADOW_SETTLED
            })
            .bind(upstream_usage)
            .bind(local_billed_credits)
            .bind(candidate.degraded.then_some("research_timeout_24h"))
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO billing_reconciliation_shadow_adjustments (
                    settlement_key, token_id, billing_subject, period_code, delta_credits,
                    attributed_at, degraded_reason, created_at, updated_at
                ) VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?)
                "#,
            )
            .bind(&settlement_key)
            .bind(&candidate.token_id)
            .bind(&candidate.billing_subject)
            .bind(&candidate.period_code)
            .bind(attributed_at)
            .bind(candidate.degraded.then_some("research_timeout_24h"))
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            set_meta_i64_executor(
                &mut *tx,
                META_KEY_UPSTREAM_RECONCILIATION_LAST_SHADOW_ADJUSTMENT_AT_V1,
                now,
            )
            .await?;
            self.mark_reconciliation_work_completed(
                &mut tx,
                candidate,
                expected_generation,
                RECONCILIATION_OUTCOME_NO_ADJUSTMENT,
                now,
            )
            .await?;
            tx.finish(Ok(())).await?;
            return Ok(true);
        }
        let inserted = sqlx::query(
            r#"
            INSERT OR IGNORE INTO billing_reconciliation_shadow_adjustments (
                settlement_key, token_id, billing_subject, period_code, delta_credits,
                attributed_at, degraded_reason, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&settlement_key)
        .bind(&candidate.token_id)
        .bind(&candidate.billing_subject)
        .bind(&candidate.period_code)
        .bind(delta)
        .bind(attributed_at)
        .bind(candidate.degraded.then_some("research_timeout_24h"))
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            self.mark_reconciliation_work_completed(
                &mut tx,
                candidate,
                expected_generation,
                RECONCILIATION_OUTCOME_OBSERVED,
                now,
            )
            .await?;
            tx.finish(Ok(())).await?;
            return Ok(false);
        }
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, upstream_usage, local_billed_credits,
                delta_credits, degraded_reason, next_attempt_at, attempt_count,
                created_at, updated_at, settled_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 1, ?, ?, ?)
            ON CONFLICT(settlement_key) DO UPDATE SET
                status = excluded.status,
                upstream_usage = excluded.upstream_usage,
                local_billed_credits = excluded.local_billed_credits,
                delta_credits = excluded.delta_credits,
                degraded_reason = excluded.degraded_reason,
                next_attempt_at = NULL,
                attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                updated_at = excluded.updated_at,
                settled_at = excluded.settled_at
            "#,
        )
        .bind(&settlement_key)
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .bind(&candidate.project_id)
        .bind(&candidate.billing_subject)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .bind(if candidate.degraded {
            RECONCILIATION_STATUS_SHADOW_DEGRADED
        } else {
            RECONCILIATION_STATUS_SHADOW_SETTLED
        })
        .bind(upstream_usage)
        .bind(local_billed_credits)
        .bind(delta)
        .bind(candidate.degraded.then_some("research_timeout_24h"))
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        set_meta_i64_executor(
            &mut *tx,
            META_KEY_UPSTREAM_RECONCILIATION_LAST_SHADOW_ADJUSTMENT_AT_V1,
            now,
        )
        .await?;
        self.mark_reconciliation_work_completed(
            &mut tx,
            candidate,
            expected_generation,
            RECONCILIATION_OUTCOME_OBSERVED,
            now,
        )
        .await?;
        tx.finish(Ok(())).await?;
        Self::emit_shadow_adjustment_written_log(
            started_at.elapsed().as_millis() as u64,
            &settlement_key,
            &candidate.period_code,
            delta,
            candidate.degraded,
        );
        Ok(true)
    }

    fn emit_shadow_adjustment_written_log(
        elapsed_ms: u64,
        settlement_key: &str,
        period_code: &str,
        delta_credits: i64,
        degraded: bool,
    ) {
        tracing::debug!(
            component = "reconciliation",
            event = "shadow_adjustment_written",
            elapsed_ms,
            job_type = "upstream_reconciliation",
            settlement_key,
            period_code,
            delta_credits,
            degraded,
        );
    }

    #[allow(dead_code)]
    pub(crate) async fn recent_reconciliation_adjustments(
        &self,
        limit: i64,
    ) -> Result<Vec<UpstreamReconciliationAdjustment>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, i64, Option<String>, i64)>(
            r#"
            SELECT settlement_key, token_id, billing_subject, period_code, delta_credits,
                   degraded_reason, created_at
            FROM billing_reconciliation_adjustments
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    settlement_key,
                    token_id,
                    billing_subject,
                    period_code,
                    delta_credits,
                    degraded_reason,
                    created_at,
                )| UpstreamReconciliationAdjustment {
                    settlement_key,
                    token_id_hint: token_id.chars().take(8).collect(),
                    billing_subject_kind: billing_subject
                        .split(':')
                        .next()
                        .unwrap_or("unknown")
                        .to_string(),
                    period_code,
                    delta_credits,
                    degraded_reason,
                    created_at,
                },
            )
            .collect())
    }

    #[allow(dead_code)]
    pub(crate) async fn upstream_reconciliation_retry_buckets(
        &self,
    ) -> Result<UpstreamReconciliationRetryBuckets, ProxyError> {
        let rows = sqlx::query_as::<_, (Option<String>, i64)>(
            r#"
            SELECT degraded_reason, COUNT(*)
            FROM upstream_reconciliation_settlements
            WHERE status = ?
            GROUP BY degraded_reason
            "#,
        )
        .bind(RECONCILIATION_STATUS_RATE_LIMITED)
        .fetch_all(&self.pool)
        .await?;
        let mut buckets = UpstreamReconciliationRetryBuckets {
            upstream_429: 0,
            local_usage_rate_limit: 0,
            missing_eligible_upstream_key: 0,
            other: 0,
        };
        for (reason, count) in rows {
            match classify_reconciliation_retry_reason(reason.as_deref()) {
                RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT => {
                    buckets.local_usage_rate_limit += count;
                }
                RECONCILIATION_RETRY_REASON_UPSTREAM_429 => {
                    buckets.upstream_429 += count;
                }
                RECONCILIATION_RETRY_REASON_MISSING_ELIGIBLE_UPSTREAM_KEY => {
                    buckets.missing_eligible_upstream_key += count;
                }
                _ => {
                    buckets.other += count;
                }
            }
        }
        buckets.missing_eligible_upstream_key = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upstream_reconciliation_work \
             WHERE work_generation > completed_generation AND last_outcome = ?",
        )
        .bind(RECONCILIATION_OUTCOME_MISSING_ELIGIBLE_UPSTREAM_KEY)
        .fetch_one(&self.pool)
        .await?;
        Ok(buckets)
    }

    #[allow(dead_code)]
    pub(crate) async fn current_period_reconciliation_key_activity(
        &self,
        current_period_code: &str,
    ) -> Result<(Vec<UpstreamKeyActivityPoint>, Vec<UpstreamKeyActivityPoint>), ProxyError> {
        let bound_rows = sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT
                u.key_id,
                COUNT(DISTINCT CASE
                    WHEN u.billing_subject LIKE 'account:%' THEN SUBSTR(u.billing_subject, 9)
                END) AS bound_users
            FROM upstream_reconciliation_usage u
            WHERE u.period_code = ?
            GROUP BY u.key_id
            HAVING bound_users > 0
            ORDER BY bound_users DESC, u.key_id ASC
            "#,
        )
        .bind(current_period_code)
        .fetch_all(&self.pool)
        .await?;
        let pending_project_rows = sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT
                u.key_id,
                COUNT(DISTINCT u.project_id) AS pending_project_ids
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE u.period_code = ?
              AND (s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited'))
            GROUP BY u.key_id
            HAVING pending_project_ids > 0
            ORDER BY pending_project_ids DESC, u.key_id ASC
            "#,
        )
        .bind(current_period_code)
        .fetch_all(&self.pool)
        .await?;
        Ok((
            bound_rows
                .into_iter()
                .map(|(key_id, count)| UpstreamKeyActivityPoint {
                    key_id_hint: key_id.chars().take(12).collect(),
                    count,
                })
                .collect(),
            pending_project_rows
                .into_iter()
                .map(|(key_id, count)| UpstreamKeyActivityPoint {
                    key_id_hint: key_id.chars().take(12).collect(),
                    count,
                })
                .collect(),
        ))
    }


    #[allow(dead_code)]
    pub(crate) async fn daily_reconciliation_progress(
        &self,
    ) -> Result<(
        DailyReconciliationProgress,
        Vec<DailyReconciliationKeyProgress>,
    ), ProxyError> {
        let now = self.backend_time.now_ts();
        let day_window = server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let (observed_accounts, accounts_with_settled_period, fully_terminal_accounts, observed_periods, settled_periods, degraded_periods, pending_periods) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
                r#"
                WITH windows AS (
                    SELECT
                        u.token_id,
                        u.period_code,
                        MIN(u.billing_subject) AS billing_subject,
                        MAX(CASE WHEN s.status = 'settled' THEN 1 ELSE 0 END) AS settled,
                        MAX(CASE WHEN s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded') THEN 1 ELSE 0 END) AS terminal,
                        MAX(CASE WHEN s.status IN ('degraded', 'shadow_degraded') THEN 1 ELSE 0 END) AS degraded
                    FROM upstream_reconciliation_usage u
                    LEFT JOIN upstream_reconciliation_settlements s
                      ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                    WHERE u.period_start >= ? AND u.period_start < ?
                    GROUP BY u.token_id, u.period_code
                ), accounts AS (
                    SELECT
                        billing_subject,
                        COUNT(*) AS observed,
                        SUM(settled) AS settled,
                        SUM(terminal) AS terminal,
                        SUM(degraded) AS degraded
                    FROM windows
                    WHERE billing_subject LIKE 'account:%'
                    GROUP BY billing_subject
                )
                SELECT
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN settled > 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN terminal = observed THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(observed), 0),
                    COALESCE(SUM(settled), 0),
                    COALESCE(SUM(degraded), 0),
                    COALESCE(SUM(observed - terminal), 0)
                FROM accounts
                "#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&self.pool)
            .await?;
        let (research_total, research_terminal, research_pending) =
            sqlx::query_as::<_, (i64, i64, i64)>(
                r#"
                SELECT
                    COUNT(DISTINCT r.request_id),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END)
                FROM upstream_reconciliation_research r
                JOIN upstream_reconciliation_usage u
                  ON u.token_id = r.token_id AND u.period_code = r.period_code
                WHERE u.period_start >= ? AND u.period_start < ?
                "#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&self.pool)
            .await?;
        let key_rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
            r#"
            SELECT
                u.key_id,
                COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                COUNT(DISTINCT CASE
                    WHEN s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited')
                    THEN u.project_id
                END)
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_research r
              ON r.token_id = u.token_id AND r.period_code = u.period_code AND r.key_id = u.key_id
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE u.period_start >= ? AND u.period_start < ?
            GROUP BY u.key_id
            HAVING COUNT(DISTINCT r.request_id) > 0
                OR COUNT(DISTINCT CASE
                    WHEN s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited')
                    THEN u.project_id
                END) > 0
            ORDER BY 3 DESC, 4 DESC, u.key_id ASC
            "#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_all(&self.pool)
        .await?;
        let backoffs = sqlx::query_as::<_, (String, i64, Option<String>)>(
            r#"
            SELECT key_id, cooldown_until, reason_code
            FROM api_key_transient_backoffs
            WHERE scope = 'period_reconciliation' AND cooldown_until > ?
            "#,
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(key_id, cooldown_until, reason_code)| {
            (key_id, (cooldown_until, reason_code))
        })
        .collect::<HashMap<_, _>>();
        let progress = DailyReconciliationProgress {
            observed_accounts,
            accounts_with_settled_period,
            fully_terminal_accounts,
            observed_periods,
            settled_periods,
            degraded_periods,
            pending_periods,
            research_total,
            research_terminal,
            research_pending,
        };
        let by_key = key_rows
            .into_iter()
            .map(|(key_id, terminal_research, pending_research, pending_project_ids)| {
                let cooldown = backoffs.get(&key_id);
                DailyReconciliationKeyProgress {
                    key_id_hint: key_id.chars().take(12).collect(),
                    terminal_research,
                    pending_research,
                    pending_project_ids,
                    cooldown_until: cooldown.map(|(until, _)| *until),
                    cooldown_reason: cooldown.and_then(|(_, reason)| reason.clone()),
                }
            })
            .collect();
        Ok((progress, by_key))
    }

    pub(crate) async fn shadow_daily_projection_for_accounts(
        &self,
        user_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, AccountShadowDailyProjection>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut projections = HashMap::<String, AccountShadowDailyProjection>::new();
        let mut delta_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT SUBSTR(billing_subject, 9) AS user_id, COALESCE(SUM(delta_credits), 0) \
             FROM billing_reconciliation_shadow_adjustments \
             WHERE billing_subject IN (",
        );
        {
            let mut separated = delta_query.separated(", ");
            user_ids.iter().for_each(|user_id| {
                separated.push_bind(format!("account:{user_id}"));
            });
        }
        delta_query
            .push(") AND attributed_at >= ")
            .push_bind(day_start)
            .push(" AND attributed_at < ")
            .push_bind(day_end)
            .push(" GROUP BY billing_subject");
        let delta_rows = delta_query
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        for (user_id, confirmed_delta_credits) in delta_rows {
            projections.insert(
                user_id,
                AccountShadowDailyProjection {
                    confirmed_delta_credits,
                    observed_window_count: 0,
                    resolved_window_count: 0,
                    shadow_settled_credits_used: 0,
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
                    shadow_settled_window_count: 0,
                    shadow_degraded_window_count: 0,
                },
            );
        }

        let mut shadow_window_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "WITH relevant_windows AS (\
             SELECT u.token_id, u.period_code, u.billing_subject \
             FROM upstream_reconciliation_usage u \
             WHERE u.settlement_mode = ",
        );
        shadow_window_query
            .push_bind(RECONCILIATION_SETTLEMENT_MODE_SHADOW)
            .push(" AND u.billing_subject IN (");
        {
            let mut separated = shadow_window_query.separated(", ");
            user_ids.iter().for_each(|user_id| {
                separated.push_bind(format!("account:{user_id}"));
            });
        }
        shadow_window_query
            .push(") AND u.period_start >= ")
            .push_bind(day_start)
            .push(" AND u.period_start < ")
            .push_bind(day_end)
            .push(
                    " GROUP BY u.token_id, u.period_code, u.billing_subject) \
                     SELECT SUBSTR(w.billing_subject, 9) AS user_id, COUNT(*) AS total_windows, \
                     COALESCE(SUM(CASE WHEN s.status IN (",
            )
            .push_bind(RECONCILIATION_STATUS_SHADOW_SETTLED)
            .push(", ")
            .push_bind(RECONCILIATION_STATUS_SHADOW_DEGRADED)
            .push(
                ") THEN 1 ELSE 0 END), 0) AS terminal_windows, \
                 COALESCE(SUM(CASE WHEN s.status IN (",
            )
            .push_bind(RECONCILIATION_STATUS_SHADOW_SETTLED)
            .push(", ")
            .push_bind(RECONCILIATION_STATUS_SHADOW_DEGRADED)
            .push(
                ") THEN COALESCE(s.upstream_usage, 0) ELSE 0 END), 0) AS settled_shadow_usage, \
                 COALESCE(SUM(CASE WHEN s.status = 'shadow_settled' THEN 1 ELSE 0 END), 0) AS settled_windows, \
                 COALESCE(SUM(CASE WHEN s.status = 'shadow_degraded' THEN 1 ELSE 0 END), 0) AS degraded_windows \
                 FROM relevant_windows w \
                 LEFT JOIN upstream_reconciliation_settlements s \
                   ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code \
                 GROUP BY w.billing_subject",
            );
        let shadow_window_rows = shadow_window_query
            .build_query_as::<(String, i64, i64, i64, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        for (user_id, total_windows, terminal_windows, settled_shadow_usage, settled_windows, degraded_windows) in shadow_window_rows {
            let entry = projections
                .entry(user_id)
                .or_insert(AccountShadowDailyProjection {
                    confirmed_delta_credits: 0,
                    observed_window_count: 0,
                    resolved_window_count: 0,
                    shadow_settled_credits_used: 0,
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
                    shadow_settled_window_count: 0,
                    shadow_degraded_window_count: 0,
                });
            entry.observed_window_count += total_windows;
            entry.resolved_window_count += terminal_windows;
            entry.shadow_settled_credits_used += settled_shadow_usage;
            entry.shadow_observed_window_count += total_windows;
            entry.shadow_resolved_window_count += terminal_windows;
            entry.shadow_settled_window_count += settled_windows;
            entry.shadow_degraded_window_count += degraded_windows;
        }

        let mut actual_window_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "WITH actual_only_windows AS (\
             SELECT u.token_id, u.period_code, u.billing_subject \
             FROM upstream_reconciliation_usage u \
             WHERE u.settlement_mode = ",
        );
        actual_window_query
            .push_bind(RECONCILIATION_SETTLEMENT_MODE_ACTUAL)
            .push(" AND u.billing_subject IN (");
        {
            let mut separated = actual_window_query.separated(", ");
            user_ids.iter().for_each(|user_id| {
                separated.push_bind(format!("account:{user_id}"));
            });
        }
        actual_window_query
            .push(") AND u.period_start >= ")
            .push_bind(day_start)
            .push(" AND u.period_start < ")
            .push_bind(day_end)
            .push(" AND NOT EXISTS (\
                SELECT 1 \
                FROM upstream_reconciliation_usage shadow \
                WHERE shadow.token_id = u.token_id \
                  AND shadow.period_code = u.period_code \
                  AND shadow.billing_subject = u.billing_subject \
                  AND shadow.settlement_mode = ")
            .push_bind(RECONCILIATION_SETTLEMENT_MODE_SHADOW)
            .push(
                ") GROUP BY u.token_id, u.period_code, u.billing_subject) \
                 SELECT SUBSTR(w.billing_subject, 9) AS user_id, COUNT(*) AS total_windows, \
                 COALESCE(SUM(CASE WHEN s.status IN (",
            );
        actual_window_query
            .push_bind(RECONCILIATION_STATUS_SETTLED)
            .push(", ")
            .push_bind(RECONCILIATION_STATUS_DEGRADED)
            .push(
                ") THEN 1 ELSE 0 END), 0) AS terminal_windows \
                 FROM actual_only_windows w \
                 LEFT JOIN upstream_reconciliation_settlements s \
                   ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code \
                 GROUP BY w.billing_subject",
            );
        let actual_window_rows = actual_window_query
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        for (user_id, total_windows, terminal_windows) in actual_window_rows {
            let entry = projections
                .entry(user_id)
                .or_insert(AccountShadowDailyProjection {
                    confirmed_delta_credits: 0,
                    observed_window_count: 0,
                    resolved_window_count: 0,
                    shadow_settled_credits_used: 0,
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
                    shadow_settled_window_count: 0,
                    shadow_degraded_window_count: 0,
                });
            entry.observed_window_count += total_windows;
            entry.resolved_window_count += terminal_windows;
        }
        Ok(projections)
    }

    pub(crate) async fn shadow_daily_reconciled_usage_for_accounts(
        &self,
        user_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        Ok(self
            .shadow_daily_projection_for_accounts(user_ids, day_start, day_end)
            .await?
            .into_iter()
            .map(|(user_id, projection)| (user_id, projection.confirmed_delta_credits))
            .collect())
    }
}
