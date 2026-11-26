/// PostgreSQL implementation of the upstream privacy diagnostic.
///
/// The SQLite version in `key_store_admin_privacy_read.rs` builds the same response from one
/// cooperative read snapshot. This twin keeps the statement set and the assembly identical while
/// running every read inside one repeatable-read transaction, so the diagnostic cannot mix
/// generations.
impl KeyStore {
    async fn admin_privacy_meta_values_pg(
        session: &mut crate::store::PgReadSession,
    ) -> Result<StdHashMap<String, String>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM core.meta")
            .fetch_all(&mut *session.connection())
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn upstream_privacy_status_pg(
        &self,
    ) -> Result<UpstreamPrivacyStatus, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let status = self.upstream_privacy_status_in_pg_session(&mut session).await?;
        session.finish().await?;
        Ok(status)
    }

    async fn upstream_privacy_status_in_pg_session(
        &self,
        session: &mut crate::store::PgReadSession,
    ) -> Result<UpstreamPrivacyStatus, ProxyError> {

        #[cfg(debug_assertions)]
        self.wait_for_admin_privacy_read_pause_if_installed().await;
        let now = self.backend_time.now_ts();
        let period = business_period_for_timestamp(now);
        let day_window = server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let meta = Self::admin_privacy_meta_values_pg(session).await?;
        let meta_i64 = |key| admin_privacy_meta_i64(&meta, key);
        let upstream_project_id_mode = meta
            .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
            .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
            .unwrap_or_default();
        let upstream_project_id_fixed_value = meta
            .get(META_KEY_UPSTREAM_PROJECT_ID_FIXED_VALUE_V1)
            .cloned()
            .unwrap_or_default();
        let upstream_mcp_user_agent = meta
            .get(META_KEY_UPSTREAM_MCP_USER_AGENT_V1)
            .cloned()
            .unwrap_or_default();
        let api_rebalance_enabled = meta_i64(META_KEY_API_REBALANCE_ENABLED_V1)
            .unwrap_or(i64::from(API_REBALANCE_ENABLED_DEFAULT))
            != 0;
        let rebalance_mcp_enabled = meta_i64(META_KEY_REBALANCE_MCP_ENABLED_V1)
            .unwrap_or(i64::from(REBALANCE_MCP_ENABLED_DEFAULT))
            != 0;
        let upstream_precise_reconciliation_enabled = meta_i64(
            META_KEY_UPSTREAM_PRECISE_RECONCILIATION_ENABLED_V1,
        )
        .unwrap_or(0)
            != 0;
                let active_upstream_mcp_sessions: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*)
                 FROM core.mcp_sessions
                WHERE gateway_mode = $1
                  AND revoked_at IS NULL
                  AND expires_at > $2"#,
        )
        .bind(MCP_GATEWAY_MODE_UPSTREAM)
        .bind(now)
        .fetch_one(&mut *session.connection())
        .await?;
        let stored_epoch = meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1).unwrap_or(0);
        let mode_ready = upstream_project_id_mode == UpstreamProjectIdMode::AccessToken;
        let sessions_ready = active_upstream_mcp_sessions == 0;
        let gates = vec![
            UpstreamPrivacyGate {
                key: "accessTokenMode".to_string(),
                ready: mode_ready,
                detail: format!("{upstream_project_id_mode:?}"),
            },
            UpstreamPrivacyGate {
                key: "apiRebalance".to_string(),
                ready: api_rebalance_enabled,
                detail: if api_rebalance_enabled { "enabled" } else { "disabled" }.to_string(),
            },
            UpstreamPrivacyGate {
                key: "mcpRebalance".to_string(),
                ready: rebalance_mcp_enabled,
                detail: if rebalance_mcp_enabled { "enabled" } else { "disabled" }.to_string(),
            },
            UpstreamPrivacyGate {
                key: "controlSessionsDrained".to_string(),
                ready: sessions_ready,
                detail: active_upstream_mcp_sessions.to_string(),
            },
        ];

        let observed_at = meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1);
                let has_eligible: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1
                  FROM core.upstream_reconciliation_work w
             LEFT JOIN core.upstream_reconciliation_settlements s
                    ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                 WHERE w.period_end + 600 <= $1
                   AND w.work_generation > w.completed_generation
                   AND GREATEST(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                       THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= $2
                   AND (w.period_end + 86400 <= $3 OR NOT EXISTS (
                       SELECT 1 FROM core.upstream_reconciliation_research r
                        WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                          AND r.terminal_at IS NULL))
                 LIMIT 1)"#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *session.connection())
        .await?;
                let oldest_period_end: Option<i64> = sqlx::query_scalar(
            r#"SELECT w.period_end
                 FROM core.upstream_reconciliation_work w
            LEFT JOIN core.upstream_reconciliation_settlements s
                   ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                WHERE w.period_end + 600 <= $1
                  AND w.work_generation > w.completed_generation
                  AND GREATEST(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                      THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= $2
                  AND (w.period_end + 86400 <= $3 OR NOT EXISTS (
                      SELECT 1 FROM core.upstream_reconciliation_research r
                       WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                         AND r.terminal_at IS NULL))
                ORDER BY w.period_end ASC
                LIMIT 1"#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_optional(&mut *session.connection())
        .await?;
        let queue_estimate = if observed_at.is_some() {
                        Some(
                sqlx::query_scalar::<_, i64>(&format!(
                    r#"SELECT COUNT(*) FROM (
                        SELECT 1 FROM core.upstream_reconciliation_work w
                        LEFT JOIN core.upstream_reconciliation_settlements s
                          ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                        WHERE w.period_end + 600 <= $1
                          AND w.work_generation > w.completed_generation
                          AND GREATEST(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                              THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= $2
                          AND (w.period_end + 86400 <= $3 OR NOT EXISTS (
                              SELECT 1 FROM core.upstream_reconciliation_research r
                               WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                                 AND r.terminal_at IS NULL))
                        LIMIT {})"#,
                    RECONCILIATION_QUEUE_ESTIMATE_LIMIT
                ))
                .bind(now)
                .bind(now)
                .bind(now)
                .fetch_one(&mut *session.connection())
                .await?,
            )
        } else {
            None
        };
        let reconciliation_observation = ReconciliationObservation {
            observed_at,
            coverage: if observed_at.is_some() { "bounded" } else { "unknown" }.to_string(),
            queue_estimate,
            has_eligible,
            oldest_candidate_age_secs: oldest_period_end
                .map(|period_end| now.saturating_sub(period_end).max(0)),
        };

                let retry_rows = sqlx::query_as::<_, (Option<String>, i64)>(
            r#"SELECT degraded_reason, COUNT(*)
                 FROM core.upstream_reconciliation_settlements
                WHERE status = $1
                GROUP BY degraded_reason"#,
        )
        .bind(RECONCILIATION_STATUS_RATE_LIMITED)
        .fetch_all(&mut *session.connection())
        .await?;
        let mut retry_buckets = UpstreamReconciliationRetryBuckets {
            upstream_429: 0,
            local_usage_rate_limit: 0,
            missing_eligible_upstream_key: 0,
            other: 0,
        };
        for (reason, count) in retry_rows {
                        match classify_reconciliation_retry_reason(reason.as_deref()) {
                RECONCILIATION_RETRY_REASON_UPSTREAM_429 => retry_buckets.upstream_429 += count,
                RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT => {
                    retry_buckets.local_usage_rate_limit += count
                }
                RECONCILIATION_RETRY_REASON_MISSING_ELIGIBLE_UPSTREAM_KEY => {
                    retry_buckets.missing_eligible_upstream_key += count
                }
                _ => retry_buckets.other += count,
            }
        }
                retry_buckets.missing_eligible_upstream_key = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core.upstream_reconciliation_work \
             WHERE work_generation > completed_generation AND last_outcome = $1",
        )
        .bind(RECONCILIATION_OUTCOME_MISSING_ELIGIBLE_UPSTREAM_KEY)
        .fetch_one(&mut *session.connection())
        .await?;

                let bound_rows = sqlx::query_as::<_, (String, i64)>(
            r#"SELECT u.key_id, COUNT(DISTINCT CASE
                      WHEN u.billing_subject LIKE 'account:%' THEN SUBSTRING(u.billing_subject FROM 9)
                  END) AS bound_users
                 FROM core.upstream_reconciliation_usage u
                WHERE u.period_code = $1
                GROUP BY u.key_id
                HAVING bound_users > 0
                ORDER BY bound_users DESC, u.key_id ASC"#,
        )
        .bind(&period.code)
        .fetch_all(&mut *session.connection())
        .await?;
                let pending_project_rows = sqlx::query_as::<_, (String, i64)>(
            r#"SELECT u.key_id, COUNT(DISTINCT u.project_id) AS pending_project_ids
                 FROM core.upstream_reconciliation_usage u
            LEFT JOIN core.upstream_reconciliation_settlements s
                   ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                WHERE u.period_code = $1
                  AND (s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited'))
                GROUP BY u.key_id
                HAVING pending_project_ids > 0
                ORDER BY pending_project_ids DESC, u.key_id ASC"#,
        )
        .bind(&period.code)
        .fetch_all(&mut *session.connection())
        .await?;

                let (observed_accounts, accounts_with_settled_period, fully_terminal_accounts, observed_periods, settled_periods, degraded_periods, pending_periods) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
                r#"WITH windows AS (
                    SELECT u.token_id, u.period_code, MIN(u.billing_subject) AS billing_subject,
                           MAX(CASE WHEN s.status = 'settled' THEN 1 ELSE 0 END) AS settled,
                           MAX(CASE WHEN s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded') THEN 1 ELSE 0 END) AS terminal,
                           MAX(CASE WHEN s.status IN ('degraded', 'shadow_degraded') THEN 1 ELSE 0 END) AS degraded
                      FROM core.upstream_reconciliation_usage u
                 LEFT JOIN core.upstream_reconciliation_settlements s
                        ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                     WHERE u.period_start >= $1 AND u.period_start < $2
                  GROUP BY u.token_id, u.period_code
                ), accounts AS (
                    SELECT billing_subject, COUNT(*) AS observed, SUM(settled) AS settled,
                           SUM(terminal) AS terminal, SUM(degraded) AS degraded
                      FROM windows WHERE billing_subject LIKE 'account:%' GROUP BY billing_subject
                )
                SELECT COUNT(*), CAST(COALESCE(SUM(CASE WHEN settled > 0 THEN 1 ELSE 0 END), 0) AS BIGINT),
                       CAST(COALESCE(SUM(CASE WHEN terminal = observed THEN 1 ELSE 0 END), 0) AS BIGINT),
                       CAST(COALESCE(SUM(observed), 0) AS BIGINT), CAST(COALESCE(SUM(settled), 0) AS BIGINT),
                       CAST(COALESCE(SUM(degraded), 0) AS BIGINT), CAST(COALESCE(SUM(observed - terminal), 0) AS BIGINT)
                  FROM accounts"#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&mut *session.connection())
            .await?;
                let (research_total, research_terminal, research_pending, research_unavailable, research_pollable_pending) = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"SELECT COUNT(DISTINCT r.request_id),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL AND r.poll_resolution = 'unavailable' THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL AND r.poll_resolution = 'pollable' THEN r.request_id END)
                  FROM core.upstream_reconciliation_research r
                  JOIN core.upstream_reconciliation_usage u
                    ON u.token_id = r.token_id AND u.period_code = r.period_code
                 WHERE u.period_start >= $1 AND u.period_start < $2"#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_one(&mut *session.connection())
        .await?;
                let (credentials_cooling_keys, earliest_credentials_retry_at) =
            sqlx::query_as::<_, (i64, Option<i64>)>(
                r#"SELECT COUNT(DISTINCT key_id), MIN(cooldown_until)
                     FROM core.api_key_transient_backoffs
                    WHERE scope = 'reconciliation_research_credentials'
                      AND cooldown_until > $1"#,
            )
            .bind(now)
            .fetch_one(&mut *session.connection())
            .await?;
                let (last_poll_outcome, last_poll_observed_at) = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
            r#"SELECT last_poll_outcome, COALESCE(last_polled_at, updated_at)
                 FROM core.upstream_reconciliation_research
                WHERE last_poll_outcome IS NOT NULL
                ORDER BY COALESCE(last_polled_at, updated_at) DESC, request_id DESC
                LIMIT 1"#,
        )
        .fetch_optional(&mut *session.connection())
        .await?
        .unwrap_or((None, None));
                let oldest_eligible_wait_secs: i64 = sqlx::query_scalar(
            r#"SELECT COALESCE(MAX($1 - next_poll_at), 0)
                  FROM core.upstream_reconciliation_research
                 WHERE terminal_at IS NULL
                   AND poll_resolution = 'pollable'
                   AND next_poll_at > 0
                   AND next_poll_at <= $2"#,
        )
        .bind(now)
        .bind(now)
        .fetch_one(&mut *session.connection())
        .await?;
                let key_rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
            r#"SELECT u.key_id,
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN s.settlement_key IS NULL
                            OR s.status IN ('pending', 'waiting', 'rate_limited') THEN u.project_id END)
                  FROM core.upstream_reconciliation_usage u
             LEFT JOIN core.upstream_reconciliation_research r
                    ON r.token_id = u.token_id AND r.period_code = u.period_code AND r.key_id = u.key_id
             LEFT JOIN core.upstream_reconciliation_settlements s
                    ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                 WHERE u.period_start >= $1 AND u.period_start < $2
              GROUP BY u.key_id
                HAVING COUNT(DISTINCT r.request_id) > 0
                    OR COUNT(DISTINCT CASE WHEN s.settlement_key IS NULL
                         OR s.status IN ('pending', 'waiting', 'rate_limited') THEN u.project_id END) > 0
              ORDER BY 3 DESC, 4 DESC, u.key_id ASC"#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_all(&mut *session.connection())
        .await?;
                let backoff_rows = sqlx::query_as::<_, (String, i64, Option<String>)>(
            r#"SELECT key_id, cooldown_until, reason_code
                 FROM core.api_key_transient_backoffs
                WHERE scope IN ('period_reconciliation', 'reconciliation_research_credentials')
                  AND cooldown_until > $1
                ORDER BY cooldown_until DESC"#,
        )
        .bind(now)
        .fetch_all(&mut *session.connection())
        .await?;
        let mut backoffs = StdHashMap::with_capacity(backoff_rows.len());
        for (key_id, cooldown_until, reason) in backoff_rows {
                        backoffs.insert(key_id, (cooldown_until, reason));
        }
        let daily_reconciliation_progress = DailyReconciliationProgress {
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
            research_unavailable,
            research_pollable_pending,
        };
        let (
            active_started_at,
            last_window_started_at,
            last_window_ended_at,
            last_window_terminal_delta,
            last_window_pending_delta,
            last_window_unavailable_delta,
            last_window_pollable_pending_delta,
        ) = {
                        sqlx::query_as::<_, (i64, Option<i64>, Option<i64>, i64, i64, i64, i64)>(
            r#"SELECT active_started_at, last_window_started_at, last_window_ended_at,
                       last_window_terminal_delta, last_window_pending_delta,
                       last_window_unavailable_delta, last_window_pollable_pending_delta
                  FROM core.upstream_reconciliation_research_progress_window
                 WHERE id = 'local'"#,
        )
        .fetch_one(&mut *session.connection())
        .await?
        };
        let complete_research_window = last_window_started_at.zip(last_window_ended_at);
        let (research_window_started_at, research_window_ended_at, research_window_seconds) =
            if let Some((started_at, ended_at)) = complete_research_window {
                (
                    Some(started_at),
                    Some(ended_at),
                    ended_at.saturating_sub(started_at),
                )
            } else {
                (
                    (active_started_at > 0).then_some(active_started_at),
                    None,
                    now.saturating_sub(active_started_at).max(0),
                )
            };
        let reconciliation_research_progress_window = ReconciliationResearchProgressWindow {
            window_started_at: research_window_started_at,
            window_ended_at: research_window_ended_at,
            window_seconds: research_window_seconds,
            terminal_delta: complete_research_window
                .map(|_| last_window_terminal_delta)
                .unwrap_or(0),
            pending_delta: complete_research_window
                .map(|_| last_window_pending_delta)
                .unwrap_or(0),
            unavailable_delta: complete_research_window
                .map(|_| last_window_unavailable_delta)
                .unwrap_or(0),
            pollable_pending_delta: complete_research_window
                .map(|_| last_window_pollable_pending_delta)
                .unwrap_or(0),
            terminal_rate_positive: complete_research_window.is_some()
                && last_window_terminal_delta > 0,
            poll_resolution_rate_positive: complete_research_window.is_some()
                && (last_window_terminal_delta > 0 || last_window_unavailable_delta > 0),
            pending_non_growing: complete_research_window.is_some()
                && last_window_pending_delta <= 0,
            complete: complete_research_window.is_some(),
        };
        let mut daily_reconciliation_by_key = Vec::with_capacity(key_rows.len());
        for (key_id, terminal_research, pending_research, pending_project_ids) in key_rows {
                        let cooldown = backoffs.get(&key_id);
            daily_reconciliation_by_key.push(DailyReconciliationKeyProgress {
                key_id_hint: key_id.chars().take(12).collect(),
                terminal_research,
                pending_research,
                pending_project_ids,
                cooldown_until: cooldown.map(|(until, _)| *until),
                cooldown_reason: cooldown.and_then(|(_, reason)| reason.clone()),
            });
        }
                let degraded_observed: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM (SELECT 1 FROM core.upstream_reconciliation_settlements \
             WHERE status IN ('degraded', 'shadow_degraded') LIMIT {})",
            RECONCILIATION_QUEUE_ESTIMATE_LIMIT.saturating_add(1)
        ))
        .fetch_one(&mut *session.connection())
        .await?;
        // Keep the legacy global 429 fields readable for rolling compatibility,
        // but they are diagnostic history only. Per-key cooldowns below remain
        // the only actionable reconciliation retry state.
        let (global_pressure_streak, global_backoff_level, global_backoff_until) = (
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1).unwrap_or(0),
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1).unwrap_or(0),
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1).unwrap_or(0),
        );
        let (local_pressure_streak, local_backoff_level, local_backoff_until) = (
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1).unwrap_or(0),
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1).unwrap_or(0),
            meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1).unwrap_or(0),
        );
                let run_row = sqlx::query_as::<_, ReconciliationRunObservationRow>(
            r#"SELECT o.mode,
                      CASE WHEN p.completed != 0 THEN 'complete'
                           WHEN p.last_defer_reason IS NOT NULL THEN 'deferred'
                           ELSE o.projection_state END AS projection_state,
                      p.scanned_rows AS projection_scanned_rows,
                      p.batch_size AS projection_batch_size,
                      p.transaction_p95_ms AS projection_transaction_p95_ms,
                      o.cursor_advanced,
                      o.hydrate_ms, o.first_remote_ms, o.remote_ms, o.finalization_ms, o.research_ms,
                      o.settled_count AS settled, o.no_adjustment_count AS no_adjustment,
                      o.observed_count AS observed, o.upstream_429_count AS upstream_429,
                      o.transport_failure_count AS transport_failure,
                      o.semantic_failure_count AS semantic_failure,
                      o.local_pressure_count AS local_pressure,
                      o.partial_key_observation_count AS partial_key_observations,
                      o.multi_key_pending_count AS multi_key_pending,
                      o.remote_attempt_budget_defer_count AS remote_attempt_budget_defers,
                      o.resumed_run_count AS resumed_runs,
                      o.terminal_run_count AS terminal_runs,
                      o.last_transport_kind, o.last_transport_kind_at, o.last_retryable_outcome,
                      COALESCE(o.continuation_reason, p.last_defer_reason) AS continuation_reason,
                      CASE WHEN o.next_retry_at IS NULL AND p.next_retry_at <= 0 THEN NULL
                           ELSE GREATEST(COALESCE(o.next_retry_at, 0), p.next_retry_at) END AS next_retry_at,
                      o.observed_at
                 FROM core.upstream_reconciliation_run_observation o
                 JOIN core.upstream_reconciliation_projection_state p ON p.id = o.id
                WHERE o.id = 'local'"#,
        )
        .fetch_one(&mut *session.connection())
        .await?;
        let reconciliation_run_observation = ReconciliationRunObservation {
            mode: run_row.mode,
            projection_state: run_row.projection_state,
            projection_scanned_rows: run_row.projection_scanned_rows,
            projection_batch_size: run_row.projection_batch_size,
            projection_transaction_p95_ms: run_row.projection_transaction_p95_ms,
            cursor_advanced: run_row.cursor_advanced != 0,
            hydrate_ms: run_row.hydrate_ms,
            first_remote_ms: run_row.first_remote_ms,
            remote_ms: run_row.remote_ms,
            finalization_ms: run_row.finalization_ms,
            research_ms: run_row.research_ms,
            settled: run_row.settled,
            no_adjustment: run_row.no_adjustment,
            observed: run_row.observed,
            upstream_429: run_row.upstream_429,
            transport_failure: run_row.transport_failure,
            semantic_failure: run_row.semantic_failure,
            local_pressure: run_row.local_pressure,
            partial_key_observations: run_row.partial_key_observations,
            multi_key_pending: run_row.multi_key_pending,
            remote_attempt_budget_defers: run_row.remote_attempt_budget_defers,
            resumed_runs: run_row.resumed_runs,
            terminal_runs: run_row.terminal_runs,
            last_transport_kind: run_row.last_transport_kind,
            last_transport_kind_at: run_row.last_transport_kind_at,
            last_retryable_outcome: run_row.last_retryable_outcome,
            continuation_reason: run_row.continuation_reason,
            next_retry_at: run_row.next_retry_at,
            observed_at: (run_row.observed_at > 0).then_some(run_row.observed_at),
        };
                let (mode, activation_period_code, activation_period_start, legacy_active, paused_reason, transitioned_at) =
            sqlx::query_as::<_, (String, Option<String>, Option<i64>, i64, Option<String>, i64)>(
                r#"SELECT mode, activation_period_code, activation_period_start, legacy_active,
                          paused_reason, transitioned_at
                     FROM core.upstream_reconciliation_control_state
                    WHERE id = 'local'"#,
            )
            .fetch_one(&mut *session.connection())
            .await?;
        let controller_mode = ReconciliationMode::parse(&mode).ok_or_else(|| {
            ProxyError::Other("invalid persisted upstream reconciliation mode".to_string())
        })?;
        let dashboard_alert_projection = self.alert_projection_status_pg().await?;
                let recent_adjustment_rows = sqlx::query_as::<_, (String, String, String, String, i64, Option<String>, i64)>(
            r#"SELECT settlement_key, token_id, billing_subject, period_code, delta_credits,
                      degraded_reason, created_at
                 FROM core.billing_reconciliation_adjustments
                ORDER BY created_at DESC
                LIMIT 10"#,
        )
        .fetch_all(&mut *session.connection())
        .await?;
        let mut recent_adjustments = Vec::with_capacity(recent_adjustment_rows.len());
        for (
            settlement_key,
            token_id,
            billing_subject,
            period_code,
            delta_credits,
            degraded_reason,
            created_at,
        ) in recent_adjustment_rows
        {
                        recent_adjustments.push(UpstreamReconciliationAdjustment {
                settlement_key,
                token_id_hint: token_id.chars().take(8).collect(),
                billing_subject_kind: billing_subject.split(':').next().unwrap_or("unknown").to_string(),
                period_code,
                delta_credits,
                degraded_reason,
                created_at,
            });
        }
        let value_i64 = |key| meta_i64(key).unwrap_or(0);
        let shadow_ready = mode_ready && api_rebalance_enabled && rebalance_mcp_enabled;
        let next_epoch_at = if legacy_active != 0
            && controller_mode == ReconciliationMode::Active
            && shadow_ready
            && sessions_ready
        {
            Some(if stored_epoch > 0 { stored_epoch } else { period.ends_at })
        } else {
            activation_period_start
        };
        let mut current_period_bound_users_by_key = Vec::with_capacity(bound_rows.len());
        for (key_id, count) in bound_rows {
                        current_period_bound_users_by_key.push(UpstreamKeyActivityPoint {
                key_id_hint: key_id.chars().take(12).collect(),
                count,
            });
        }
        let mut current_period_pending_project_ids_by_key =
            Vec::with_capacity(pending_project_rows.len());
        for (key_id, count) in pending_project_rows {
                        current_period_pending_project_ids_by_key.push(UpstreamKeyActivityPoint {
                key_id_hint: key_id.chars().take(12).collect(),
                count,
            });
        }
                Ok(UpstreamPrivacyStatus {
            phase: controller_mode.as_str().to_string(),
            configured_project_id_mode: upstream_project_id_mode,
            effective_project_id_mode: upstream_project_id_mode,
            fixed_project_id_configured: !upstream_project_id_fixed_value.is_empty(),
            configured_mcp_user_agent: upstream_mcp_user_agent.clone(),
            effective_mcp_user_agent: (!upstream_mcp_user_agent.is_empty()).then_some(upstream_mcp_user_agent),
            upstream_precise_reconciliation_enabled,
            http_allowed_headers: vec!["accept", "accept-encoding", "content-type", "x-project-id (policy injected)"].into_iter().map(str::to_string).collect(),
            control_mcp_allowed_headers: vec!["accept", "accept-encoding", "cache-control", "content-type", "last-event-id", "mcp-protocol-version", "mcp-session-id", "pragma", "user-agent (configured only)"].into_iter().map(str::to_string).collect(),
            completed_gates: gates.iter().filter(|gate| gate.ready).count() as i64,
            total_gates: gates.len() as i64,
            gates,
            active_upstream_mcp_sessions,
            current_period_code: period.code,
            current_period_ends_at: period.ends_at,
            next_epoch_at,
            pending_research: Some(daily_reconciliation_progress.research_pending),
            queued_settlements: reconciliation_observation.queue_estimate,
            degraded_settlements: degraded_observed.min(RECONCILIATION_QUEUE_ESTIMATE_LIMIT),
            degraded_settlements_capped: degraded_observed > RECONCILIATION_QUEUE_ESTIMATE_LIMIT,
            last_reconciliation_run_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1),
            last_shadow_adjustment_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_SHADOW_ADJUSTMENT_AT_V1),
            last_reconciliation_enqueue_error_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ENQUEUE_ERROR_AT_V1),
            last_research_sweep_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1),
            last_research_terminal_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1),
            reconciliation_pressure_streak: global_pressure_streak,
            reconciliation_backoff_level: global_backoff_level,
            reconciliation_backoff_until: (global_backoff_until > now).then_some(global_backoff_until),
            reconciliation_last_duration_ms: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1),
            reconciliation_last_attempted: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1),
            reconciliation_last_settled: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1),
            reconciliation_last_no_adjustment: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1),
            reconciliation_last_upstream_429: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1),
            reconciliation_last_budget_exhausted: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1) != 0,
            reconciliation_observation,
            reconciliation_local_backoff: ReconciliationLocalBackoff {
                pressure_streak: local_pressure_streak,
                level: local_backoff_level,
                available_at: (local_backoff_until > now).then_some(local_backoff_until),
                last_recovered_at: meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1),
            },
            reconciliation_run_observation,
            reconciliation_research_progress_window,
            reconciliation_research_poll_diagnostics: ReconciliationResearchPollDiagnostics {
                unavailable: research_unavailable,
                pollable_pending: research_pollable_pending,
                credentials_cooling_keys,
                earliest_credentials_retry_at,
                last_poll_outcome,
                last_poll_observed_at,
                foreground_pressure_defers: 0,
                remote_lease_defers: 0,
                read_budget_defers: 0,
                control_defers: 0,
                remote_lease_waits: 0,
                remote_lease_wait_ms: 0,
                longest_eligible_wait_secs: oldest_eligible_wait_secs,
            },
            reconciliation_controller: ReconciliationControllerStatus {
                mode: controller_mode.as_str().to_string(),
                activation_period_code,
                activation_period_start,
                legacy_active: legacy_active != 0,
                paused_reason,
                transitioned_at: (transitioned_at > 0).then_some(transitioned_at),
            },
            dashboard_alert_projection: DashboardAlertProjectionStatus {
                coverage: dashboard_alert_projection.coverage,
                observed_at: dashboard_alert_projection.observed_at,
                stale_reason: dashboard_alert_projection.stale_reason,
            },
            retry_buckets,
            current_period_bound_users_by_key,
            current_period_pending_project_ids_by_key,
            daily_reconciliation_progress,
            daily_reconciliation_by_key,
            recent_adjustments,
            generated_at: now,
            coverage: "ok".to_string(),
            observed_at: Some(now),
            stale_reason: None,
        })
    
    }
}
