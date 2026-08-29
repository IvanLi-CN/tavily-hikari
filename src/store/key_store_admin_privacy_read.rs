// The privacy-status endpoint is an administrative diagnostic. Build its entire
// response from one short read snapshot so a busy pool cannot mix generations.

async fn admin_privacy_meta_values(
    snapshot: &mut SqliteReadSnapshot,
) -> Result<StdHashMap<String, String>, ProxyError> {
    snapshot.ensure_cooperative_run_budget()?;
    let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM meta")
        .fetch_all(&mut **snapshot)
        .await?;
    let mut values = StdHashMap::with_capacity(rows.len());
    for (key, value) in rows {
        snapshot.ensure_cooperative_run_budget()?;
        values.insert(key, value);
    }
    Ok(values)
}

fn admin_privacy_meta_i64(values: &StdHashMap<String, String>, key: &str) -> Option<i64> {
    values.get(key).and_then(|value| value.parse().ok())
}

async fn admin_privacy_alert_source_fence(
    snapshot: &mut SqliteReadSnapshot,
    source_kind: &str,
) -> Result<Option<(i64, String)>, ProxyError> {
    snapshot.ensure_cooperative_run_budget()?;
    let row = match source_kind {
        ALERT_SOURCE_AUTH_TOKEN_LOG => sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT created_at, id
                 FROM auth_token_logs
                WHERE failure_kind = 'upstream_rate_limited_429'
                   OR result_status = 'quota_exhausted'
                ORDER BY created_at DESC, id DESC
                LIMIT 1"#,
        )
        .fetch_optional(&mut **snapshot)
        .await?
        .map(|(occurred_at, id)| (occurred_at, format!("atl:{id:020}"))),
        ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => sqlx::query_as::<_, (i64, String)>(
            r#"SELECT occurred_at, source_id
                 FROM (
                    SELECT created_at AS occurred_at, id AS source_id
                      FROM api_key_maintenance_records
                     WHERE COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                    UNION ALL
                    SELECT created_at AS occurred_at, id AS source_id
                      FROM api_key_maintenance_records
                     WHERE source = 'system'
                       AND operation_code = 'auto_mark_exhausted'
                       AND reason_code = 'quota_exhausted'
                 )
                ORDER BY occurred_at DESC, source_id DESC
                LIMIT 1"#,
        )
        .fetch_optional(&mut **snapshot)
        .await?
        .map(|(occurred_at, id)| (occurred_at, format!("maint:{id}"))),
        ALERT_SOURCE_SCHEDULED_JOB => sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                 FROM scheduled_jobs
                WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                ORDER BY COALESCE(finished_at, started_at, queued_at) DESC, id DESC
                LIMIT 1"#,
        )
        .fetch_optional(&mut **snapshot)
        .await?
        .map(|(occurred_at, id)| (occurred_at, format!("job:{id:020}"))),
        other => {
            return Err(ProxyError::Other(format!(
                "unknown alert projection source: {other}"
            )));
        }
    };
    Ok(row)
}

async fn admin_privacy_alert_projection_status(
    snapshot: &mut SqliteReadSnapshot,
    now: i64,
) -> Result<AlertProjectionStatus, ProxyError> {
    snapshot.ensure_cooperative_run_budget()?;
    let (
        sources,
        observed_at,
        idle_sources,
        fresh_sources,
        stale_reason,
        history_sources,
        idle_history_sources,
    ) = sqlx::query_as::<_, (i64, Option<i64>, i64, i64, Option<String>, i64, i64)>(
        r#"SELECT COUNT(tail.source_kind), MIN(tail.observed_at),
                  SUM(CASE WHEN tail.phase = 'idle' THEN 1 ELSE 0 END),
                  SUM(CASE WHEN tail.observed_at IS NOT NULL AND tail.observed_at >= ? THEN 1 ELSE 0 END),
                  MAX(tail.stale_reason),
                  COUNT(history.source_kind),
                  SUM(CASE WHEN history.phase = 'idle' THEN 1 ELSE 0 END)
             FROM observability.dashboard_alert_projection_state AS tail
             LEFT JOIN observability.dashboard_alert_projection_history_state AS history
               ON history.source_kind = tail.source_kind"#,
    )
    .bind(now.saturating_sub(ALERT_PROJECTION_STALE_SECS))
    .fetch_one(&mut **snapshot)
    .await?;
    let observations_expired = sources == ALERT_PROJECTION_SOURCES.len() as i64
        && idle_sources == sources
        && fresh_sources != sources;
    let mut recent_coverage = if sources == ALERT_PROJECTION_SOURCES.len() as i64
        && idle_sources == sources
        && fresh_sources == sources
    {
        "ok"
    } else if stale_reason.is_some() || observations_expired {
        "stale"
    } else {
        "projecting"
    };
    if recent_coverage == "projecting" && stale_reason.is_none() {
        snapshot.ensure_cooperative_run_budget()?;
        let incomplete_sources = sqlx::query_scalar::<_, String>(
            r#"SELECT source_kind
                 FROM observability.dashboard_alert_projection_state
                WHERE phase <> 'idle'
                ORDER BY source_kind ASC"#,
        )
        .fetch_all(&mut **snapshot)
        .await?;
        if !incomplete_sources.is_empty() {
            let mut sources_have_events = false;
            for source_kind in incomplete_sources {
                if admin_privacy_alert_source_fence(snapshot, &source_kind)
                    .await?
                    .is_some()
                {
                    sources_have_events = true;
                    break;
                }
            }
            if !sources_have_events {
                recent_coverage = "ok";
            }
        }
    }
    let coverage = if recent_coverage == "ok"
        && history_sources == ALERT_PROJECTION_SOURCES.len() as i64
        && idle_history_sources == history_sources
    {
        "ok"
    } else if recent_coverage == "stale" {
        "stale"
    } else {
        "projecting"
    };
    Ok(AlertProjectionStatus {
        coverage: coverage.to_string(),
        recent_coverage: recent_coverage.to_string(),
        observed_at,
        stale_reason: stale_reason.or_else(|| {
            observations_expired.then(|| "observation_expired".to_string())
        }),
    })
}

impl KeyStore {
    pub(crate) async fn upstream_privacy_status_from_snapshot(
        &self,
        snapshot: &mut SqliteReadSnapshot,
    ) -> Result<UpstreamPrivacyStatus, ProxyError> {
        #[cfg(debug_assertions)]
        self.wait_for_admin_privacy_read_pause_if_installed().await;
        let now = self.backend_time.now_ts();
        let period = business_period_for_timestamp(now);
        let day_window = server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let meta = admin_privacy_meta_values(snapshot).await?;
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
        snapshot.ensure_cooperative_run_budget()?;
        let active_upstream_mcp_sessions: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*)
                 FROM mcp_sessions
                WHERE gateway_mode = ?
                  AND revoked_at IS NULL
                  AND expires_at > ?"#,
        )
        .bind(MCP_GATEWAY_MODE_UPSTREAM)
        .bind(now)
        .fetch_one(&mut **snapshot)
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
        snapshot.ensure_cooperative_run_budget()?;
        let has_eligible: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1
                  FROM upstream_reconciliation_work w
             LEFT JOIN upstream_reconciliation_settlements s
                    ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                 WHERE w.period_end + 600 <= ?
                   AND w.work_generation > w.completed_generation
                   AND MAX(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                       THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= ?
                   AND (w.period_end + 86400 <= ? OR NOT EXISTS (
                       SELECT 1 FROM upstream_reconciliation_research r
                        WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                          AND r.terminal_at IS NULL))
                 LIMIT 1)"#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(&mut **snapshot)
        .await?;
        snapshot.ensure_cooperative_run_budget()?;
        let oldest_period_end: Option<i64> = sqlx::query_scalar(
            r#"SELECT w.period_end
                 FROM upstream_reconciliation_work w
            LEFT JOIN upstream_reconciliation_settlements s
                   ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                WHERE w.period_end + 600 <= ?
                  AND w.work_generation > w.completed_generation
                  AND MAX(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                      THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= ?
                  AND (w.period_end + 86400 <= ? OR NOT EXISTS (
                      SELECT 1 FROM upstream_reconciliation_research r
                       WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                         AND r.terminal_at IS NULL))
                ORDER BY w.period_end ASC
                LIMIT 1"#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_optional(&mut **snapshot)
        .await?;
        let queue_estimate = if observed_at.is_some() {
            snapshot.ensure_cooperative_run_budget()?;
            Some(
                sqlx::query_scalar::<_, i64>(&format!(
                    r#"SELECT COUNT(*) FROM (
                        SELECT 1 FROM upstream_reconciliation_work w
                        LEFT JOIN upstream_reconciliation_settlements s
                          ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code
                        WHERE w.period_end + 600 <= ?
                          AND w.work_generation > w.completed_generation
                          AND MAX(w.next_attempt_at, CASE WHEN s.status IN ('pending', 'waiting', 'rate_limited')
                              THEN COALESCE(s.next_attempt_at, 0) ELSE 0 END) <= ?
                          AND (w.period_end + 86400 <= ? OR NOT EXISTS (
                              SELECT 1 FROM upstream_reconciliation_research r
                               WHERE r.token_id = w.token_id AND r.period_code = w.period_code
                                 AND r.terminal_at IS NULL))
                        LIMIT {})"#,
                    RECONCILIATION_QUEUE_ESTIMATE_LIMIT
                ))
                .bind(now)
                .bind(now)
                .bind(now)
                .fetch_one(&mut **snapshot)
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

        snapshot.ensure_cooperative_run_budget()?;
        let retry_rows = sqlx::query_as::<_, (Option<String>, i64)>(
            r#"SELECT degraded_reason, COUNT(*)
                 FROM upstream_reconciliation_settlements
                WHERE status = ?
                GROUP BY degraded_reason"#,
        )
        .bind(RECONCILIATION_STATUS_RATE_LIMITED)
        .fetch_all(&mut **snapshot)
        .await?;
        let mut retry_buckets = UpstreamReconciliationRetryBuckets {
            upstream_429: 0,
            local_usage_rate_limit: 0,
            missing_eligible_upstream_key: 0,
            other: 0,
        };
        for (reason, count) in retry_rows {
            snapshot.ensure_cooperative_run_budget()?;
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
        snapshot.ensure_cooperative_run_budget()?;
        retry_buckets.missing_eligible_upstream_key = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upstream_reconciliation_work \
             WHERE work_generation > completed_generation AND last_outcome = ?",
        )
        .bind(RECONCILIATION_OUTCOME_MISSING_ELIGIBLE_UPSTREAM_KEY)
        .fetch_one(&mut **snapshot)
        .await?;

        snapshot.ensure_cooperative_run_budget()?;
        let bound_rows = sqlx::query_as::<_, (String, i64)>(
            r#"SELECT u.key_id, COUNT(DISTINCT CASE
                      WHEN u.billing_subject LIKE 'account:%' THEN SUBSTR(u.billing_subject, 9)
                  END) AS bound_users
                 FROM upstream_reconciliation_usage u
                WHERE u.period_code = ?
                GROUP BY u.key_id
                HAVING bound_users > 0
                ORDER BY bound_users DESC, u.key_id ASC"#,
        )
        .bind(&period.code)
        .fetch_all(&mut **snapshot)
        .await?;
        snapshot.ensure_cooperative_run_budget()?;
        let pending_project_rows = sqlx::query_as::<_, (String, i64)>(
            r#"SELECT u.key_id, COUNT(DISTINCT u.project_id) AS pending_project_ids
                 FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_settlements s
                   ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                WHERE u.period_code = ?
                  AND (s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited'))
                GROUP BY u.key_id
                HAVING pending_project_ids > 0
                ORDER BY pending_project_ids DESC, u.key_id ASC"#,
        )
        .bind(&period.code)
        .fetch_all(&mut **snapshot)
        .await?;

        snapshot.ensure_cooperative_run_budget()?;
        let (observed_accounts, accounts_with_settled_period, fully_terminal_accounts, observed_periods, settled_periods, degraded_periods, pending_periods) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
                r#"WITH windows AS (
                    SELECT u.token_id, u.period_code, MIN(u.billing_subject) AS billing_subject,
                           MAX(CASE WHEN s.status = 'settled' THEN 1 ELSE 0 END) AS settled,
                           MAX(CASE WHEN s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded') THEN 1 ELSE 0 END) AS terminal,
                           MAX(CASE WHEN s.status IN ('degraded', 'shadow_degraded') THEN 1 ELSE 0 END) AS degraded
                      FROM upstream_reconciliation_usage u
                 LEFT JOIN upstream_reconciliation_settlements s
                        ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                     WHERE u.period_start >= ? AND u.period_start < ?
                  GROUP BY u.token_id, u.period_code
                ), accounts AS (
                    SELECT billing_subject, COUNT(*) AS observed, SUM(settled) AS settled,
                           SUM(terminal) AS terminal, SUM(degraded) AS degraded
                      FROM windows WHERE billing_subject LIKE 'account:%' GROUP BY billing_subject
                )
                SELECT COUNT(*), COALESCE(SUM(CASE WHEN settled > 0 THEN 1 ELSE 0 END), 0),
                       COALESCE(SUM(CASE WHEN terminal = observed THEN 1 ELSE 0 END), 0),
                       COALESCE(SUM(observed), 0), COALESCE(SUM(settled), 0),
                       COALESCE(SUM(degraded), 0), COALESCE(SUM(observed - terminal), 0)
                  FROM accounts"#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&mut **snapshot)
            .await?;
        snapshot.ensure_cooperative_run_budget()?;
        let (research_total, research_terminal, research_pending) = sqlx::query_as::<_, (i64, i64, i64)>(
            r#"SELECT COUNT(DISTINCT r.request_id),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END)
                  FROM upstream_reconciliation_research r
                  JOIN upstream_reconciliation_usage u
                    ON u.token_id = r.token_id AND u.period_code = r.period_code
                 WHERE u.period_start >= ? AND u.period_start < ?"#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_one(&mut **snapshot)
        .await?;
        snapshot.ensure_cooperative_run_budget()?;
        let key_rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
            r#"SELECT u.key_id,
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                       COUNT(DISTINCT CASE WHEN s.settlement_key IS NULL
                            OR s.status IN ('pending', 'waiting', 'rate_limited') THEN u.project_id END)
                  FROM upstream_reconciliation_usage u
             LEFT JOIN upstream_reconciliation_research r
                    ON r.token_id = u.token_id AND r.period_code = u.period_code AND r.key_id = u.key_id
             LEFT JOIN upstream_reconciliation_settlements s
                    ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                 WHERE u.period_start >= ? AND u.period_start < ?
              GROUP BY u.key_id
                HAVING COUNT(DISTINCT r.request_id) > 0
                    OR COUNT(DISTINCT CASE WHEN s.settlement_key IS NULL
                         OR s.status IN ('pending', 'waiting', 'rate_limited') THEN u.project_id END) > 0
              ORDER BY 3 DESC, 4 DESC, u.key_id ASC"#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_all(&mut **snapshot)
        .await?;
        snapshot.ensure_cooperative_run_budget()?;
        let backoff_rows = sqlx::query_as::<_, (String, i64, Option<String>)>(
            r#"SELECT key_id, cooldown_until, reason_code
                 FROM api_key_transient_backoffs
                WHERE scope = 'period_reconciliation' AND cooldown_until > ?"#,
        )
        .bind(now)
        .fetch_all(&mut **snapshot)
        .await?;
        let mut backoffs = StdHashMap::with_capacity(backoff_rows.len());
        for (key_id, cooldown_until, reason) in backoff_rows {
            snapshot.ensure_cooperative_run_budget()?;
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
        };
        let (
            active_started_at,
            last_window_started_at,
            last_window_ended_at,
            last_window_terminal_delta,
            last_window_pending_delta,
        ) = {
            snapshot.ensure_cooperative_run_budget()?;
            sqlx::query_as::<_, (i64, Option<i64>, Option<i64>, i64, i64)>(
            r#"SELECT active_started_at, last_window_started_at, last_window_ended_at,
                       last_window_terminal_delta, last_window_pending_delta
                  FROM upstream_reconciliation_research_progress_window
                 WHERE id = 'local'"#,
        )
        .fetch_one(&mut **snapshot)
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
            terminal_rate_positive: complete_research_window.is_some()
                && last_window_terminal_delta > 0,
            pending_non_growing: complete_research_window.is_some()
                && last_window_pending_delta <= 0,
            complete: complete_research_window.is_some(),
        };
        let mut daily_reconciliation_by_key = Vec::with_capacity(key_rows.len());
        for (key_id, terminal_research, pending_research, pending_project_ids) in key_rows {
            snapshot.ensure_cooperative_run_budget()?;
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
        snapshot.ensure_cooperative_run_budget()?;
        let degraded_observed: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM (SELECT 1 FROM upstream_reconciliation_settlements \
             WHERE status IN ('degraded', 'shadow_degraded') LIMIT {})",
            RECONCILIATION_QUEUE_ESTIMATE_LIMIT.saturating_add(1)
        ))
        .fetch_one(&mut **snapshot)
        .await?;
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
        snapshot.ensure_cooperative_run_budget()?;
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
                           ELSE MAX(COALESCE(o.next_retry_at, 0), p.next_retry_at) END AS next_retry_at,
                      o.observed_at
                 FROM upstream_reconciliation_run_observation o
                 JOIN upstream_reconciliation_projection_state p ON p.id = o.id
                WHERE o.id = 'local'"#,
        )
        .fetch_one(&mut **snapshot)
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
        snapshot.ensure_cooperative_run_budget()?;
        let (mode, activation_period_code, activation_period_start, legacy_active, paused_reason, transitioned_at) =
            sqlx::query_as::<_, (String, Option<String>, Option<i64>, i64, Option<String>, i64)>(
                r#"SELECT mode, activation_period_code, activation_period_start, legacy_active,
                          paused_reason, transitioned_at
                     FROM upstream_reconciliation_control_state
                    WHERE id = 'local'"#,
            )
            .fetch_one(&mut **snapshot)
            .await?;
        let controller_mode = ReconciliationMode::parse(&mode).ok_or_else(|| {
            ProxyError::Other("invalid persisted upstream reconciliation mode".to_string())
        })?;
        let dashboard_alert_projection = admin_privacy_alert_projection_status(snapshot, now).await?;
        snapshot.ensure_cooperative_run_budget()?;
        let recent_adjustment_rows = sqlx::query_as::<_, (String, String, String, String, i64, Option<String>, i64)>(
            r#"SELECT settlement_key, token_id, billing_subject, period_code, delta_credits,
                      degraded_reason, created_at
                 FROM billing_reconciliation_adjustments
                ORDER BY created_at DESC
                LIMIT 10"#,
        )
        .fetch_all(&mut **snapshot)
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
            snapshot.ensure_cooperative_run_budget()?;
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
            snapshot.ensure_cooperative_run_budget()?;
            current_period_bound_users_by_key.push(UpstreamKeyActivityPoint {
                key_id_hint: key_id.chars().take(12).collect(),
                count,
            });
        }
        let mut current_period_pending_project_ids_by_key =
            Vec::with_capacity(pending_project_rows.len());
        for (key_id, count) in pending_project_rows {
            snapshot.ensure_cooperative_run_budget()?;
            current_period_pending_project_ids_by_key.push(UpstreamKeyActivityPoint {
                key_id_hint: key_id.chars().take(12).collect(),
                count,
            });
        }
        snapshot.ensure_cooperative_run_budget()?;
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
