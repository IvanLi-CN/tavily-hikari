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
pub(crate) const RECONCILIATION_RETRY_REASON_OTHER: &str = "other";
const RECONCILIATION_RECENT_LANE_BUDGET: i64 = 12;
const RECONCILIATION_BACKLOG_LANE_BUDGET: i64 = 8;

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
    String,
);

fn upstream_reconciliation_shadow_ready(settings: &SystemSettings) -> bool {
    settings.upstream_project_id_mode == UpstreamProjectIdMode::AccessToken
        && settings.api_rebalance_enabled
        && settings.rebalance_mcp_enabled
}

fn upstream_reconciliation_precise_cutover_configured(settings: &SystemSettings) -> bool {
    upstream_reconciliation_shadow_ready(settings) && settings.upstream_precise_reconciliation_enabled
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
    if reason.contains("usage http error 429") || reason.contains("429 Too Many Requests") {
        return RECONCILIATION_RETRY_REASON_UPSTREAM_429;
    }
    RECONCILIATION_RETRY_REASON_OTHER
}

impl KeyStore {
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
        if !settings.upstream_precise_reconciliation_enabled {
            return Ok(true);
        }
        Ok(!self.refresh_upstream_reconciliation_epoch().await?.0)
    }

    pub(crate) async fn upstream_reconciliation_runtime_markers(
        &self,
    ) -> Result<(Option<i64>, Option<i64>, Option<i64>), ProxyError> {
        Ok((
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_SHADOW_ADJUSTMENT_AT_V1)
                .await?,
            self.get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ENQUEUE_ERROR_AT_V1)
                .await?,
        ))
    }

    pub(crate) async fn mark_upstream_reconciliation_run_completed_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1, timestamp)
            .await
    }

    pub(crate) async fn mark_upstream_reconciliation_enqueue_error_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_LAST_ENQUEUE_ERROR_AT_V1, timestamp)
            .await
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
        let precise_cutover_ready = if upstream_reconciliation_precise_cutover_configured(&settings)
        {
            self.refresh_upstream_reconciliation_epoch().await?.0
        } else {
            false
        };
        let settlement_mode = if precise_cutover_ready {
            RECONCILIATION_SETTLEMENT_MODE_ACTUAL
        } else {
            RECONCILIATION_SETTLEMENT_MODE_SHADOW
        };
        let now = self.backend_time.now_ts();
        let period = business_period_for_timestamp(now);
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
        Ok(Some(period))
    }

    pub(crate) async fn mark_upstream_reconciliation_research_terminal(
        &self,
        request_id: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let changed = sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET terminal_at = COALESCE(terminal_at, ?), updated_at = ?
            WHERE request_id = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(request_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }

    fn build_upstream_reconciliation_candidates(
        &self,
        rows: Vec<UpstreamReconciliationCandidateRow>,
        now: i64,
    ) -> Vec<UpstreamReconciliationCandidate> {
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
                    _scheduling_key_id,
                )| {
                    let degraded = pending_research > 0
                        && now >= period_end.saturating_add(86_400);
                    if pending_research > 0 && !degraded {
                        return None;
                    }
                    Some(UpstreamReconciliationCandidate {
                        token_id,
                        period_code,
                        project_id,
                        billing_subject,
                        settlement_mode,
                        period_start,
                        period_end,
                        pending_research,
                        degraded,
                    })
                },
            )
            .collect()
    }

    async fn query_upstream_reconciliation_candidates(
        &self,
        now: i64,
        limit: i64,
        newest_first: bool,
        scope: ReconciliationCandidateScope,
    ) -> Result<Vec<UpstreamReconciliationCandidate>, ProxyError> {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            WITH candidate_windows AS (
            SELECT
                u.token_id,
                u.period_code AS period_code,
                MIN(u.project_id) AS project_id,
                MIN(u.billing_subject) AS billing_subject,
                MIN(u.settlement_mode) AS settlement_mode,
                MIN(u.period_start) AS period_start,
                MAX(u.period_end) AS period_end,
                MIN(u.key_id) AS scheduling_key_id,
                COALESCE((
                    SELECT COUNT(*)
                    FROM upstream_reconciliation_research r
                    WHERE r.token_id = u.token_id
                      AND r.period_code = u.period_code
                      AND r.terminal_at IS NULL
                ), 0) AS pending_research
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE u.period_end + 600 <= "#,
        );
        query
            .push_bind(now)
            .push(
                r#"
              AND (s.settlement_key IS NULL OR (
                    s.status IN ('pending', 'waiting', 'rate_limited')
                    AND COALESCE(s.next_attempt_at, 0) <= "#,
            )
            .push_bind(now)
            .push(" ))");
        match scope {
            ReconciliationCandidateScope::Recent { start, end } => {
                query
                    .push(" AND u.period_start >= ")
                    .push_bind(start)
                    .push(" AND u.period_start < ")
                    .push_bind(end);
            }
            ReconciliationCandidateScope::Backlog { before } => {
                query
                    .push(" AND u.period_start < ")
                    .push_bind(before);
            }
        }
        query
            .push(
                r#"
            GROUP BY u.token_id, u.period_code
            )"#,
            );
        if newest_first {
            query
                .push(
                    r#",
            ranked_recent_candidates AS (
                SELECT
                    token_id,
                    period_code,
                    project_id,
                    billing_subject,
                    settlement_mode,
                    period_start,
                    period_end,
                    pending_research,
                    scheduling_key_id,
                    ROW_NUMBER() OVER (
                        PARTITION BY scheduling_key_id
                        ORDER BY period_end DESC
                    ) AS key_slot
                FROM candidate_windows
                WHERE pending_research = 0
                   OR period_end + 86400 <= "#,
                )
                .push_bind(now)
                .push(
                    r#"
            )
            SELECT
                token_id,
                period_code,
                project_id,
                billing_subject,
                settlement_mode,
                period_start,
                period_end,
                pending_research,
                scheduling_key_id
            FROM ranked_recent_candidates
            ORDER BY key_slot ASC, period_end DESC
            LIMIT "#,
                )
                .push_bind(limit.max(1));
        } else {
            query
                .push(
                    r#"
            SELECT
                token_id,
                period_code,
                project_id,
                billing_subject,
                settlement_mode,
                period_start,
                period_end,
                pending_research,
                scheduling_key_id
            FROM candidate_windows
            WHERE pending_research = 0
               OR period_end + 86400 <= "#,
                )
                .push_bind(now)
                .push(
                    r#"
            ORDER BY period_end ASC
            LIMIT "#,
                )
                .push_bind(limit.max(1));
        }
        let rows = query
            .build_query_as::<UpstreamReconciliationCandidateRow>()
            .fetch_all(&self.pool)
            .await?;
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

        let mut candidates =
            Vec::with_capacity((recent_candidate_count + backlog_candidate_count) as usize);
        candidates.extend(
            recent_candidates
                .iter()
                .take(recent_candidate_count as usize)
                .cloned(),
        );
        candidates.extend(
            backlog_candidates
                .iter()
                .take(backlog_candidate_count as usize)
                .cloned(),
        );
        Ok(UpstreamReconciliationCandidateBatch {
            candidates,
            recent_lane_budget,
            backlog_lane_budget,
            recent_candidate_count,
            backlog_candidate_count,
        })
    }

    pub(crate) async fn reconciliation_key_ids(
        &self,
        token_id: &str,
        period_code: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT key_id
            FROM upstream_reconciliation_usage
            WHERE token_id = ? AND period_code = ?
            ORDER BY key_id ASC
            "#,
        )
        .bind(token_id)
        .bind(period_code)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn reconciliation_local_billed_credits(
        &self,
        candidate: &UpstreamReconciliationCandidate,
    ) -> Result<i64, ProxyError> {
        sqlx::query_scalar(
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
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn reserve_upstream_usage_attempt(
        &self,
        key_id: &str,
    ) -> Result<Result<(), i64>, ProxyError> {
        let now = self.backend_time.now_ts();
        let threshold = now - 600;
        let mut tx = self.pool.begin().await?;
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
            tx.commit().await?;
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
        tx.commit().await?;
        Ok(Ok(()))
    }

    pub(crate) async fn mark_reconciliation_retry(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        status: &str,
        next_attempt_at: i64,
        reason: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let settlement_key = format!("v1:{}:{}", candidate.token_id, candidate.period_code);
        let normalized_reason = reason.map(|value| classify_reconciliation_retry_reason(Some(value)));
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
        .bind(next_attempt_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_reconciliation_key_retry(
        &self,
        key_id: &str,
        next_attempt_at: i64,
        reason: Option<&str>,
    ) -> Result<i64, ProxyError> {
        let now = self.backend_time.now_ts();
        let normalized_reason = classify_reconciliation_retry_reason(reason);
        let changed = sqlx::query(
            r#"
            WITH candidate_windows AS (
                SELECT
                    u.token_id AS token_id,
                    u.period_code AS period_code,
                    MIN(u.project_id) AS project_id,
                    MIN(u.billing_subject) AS billing_subject,
                    MIN(u.period_start) AS period_start,
                    MAX(u.period_end) AS period_end,
                    COALESCE((
                        SELECT COUNT(*)
                        FROM upstream_reconciliation_research r
                        WHERE r.token_id = u.token_id
                          AND r.period_code = u.period_code
                          AND r.terminal_at IS NULL
                    ), 0) AS pending_research
                FROM upstream_reconciliation_usage u
                LEFT JOIN upstream_reconciliation_settlements s
                  ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                WHERE u.key_id = ?
                  AND u.period_end + 600 <= ?
                  AND (s.settlement_key IS NULL OR (
                        s.status IN ('pending', 'waiting', 'rate_limited')
                        AND COALESCE(s.next_attempt_at, 0) <= ?
                  ))
                GROUP BY u.token_id, u.period_code
            )
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, degraded_reason, next_attempt_at,
                attempt_count, created_at, updated_at
            )
            SELECT
                'v1:' || token_id || ':' || period_code,
                token_id,
                period_code,
                project_id,
                billing_subject,
                period_start,
                period_end,
                ?,
                ?,
                ?,
                1,
                ?,
                ?
            FROM candidate_windows
            WHERE pending_research = 0 OR period_end + 86400 <= ?
            ON CONFLICT(settlement_key) DO UPDATE SET
                status = excluded.status,
                degraded_reason = excluded.degraded_reason,
                next_attempt_at = MAX(
                    COALESCE(upstream_reconciliation_settlements.next_attempt_at, 0),
                    excluded.next_attempt_at
                ),
                attempt_count = upstream_reconciliation_settlements.attempt_count + 1,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key_id)
        .bind(now)
        .bind(now)
        .bind(RECONCILIATION_STATUS_RATE_LIMITED)
        .bind(normalized_reason)
        .bind(next_attempt_at)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected() as i64;
        Ok(changed)
    }

    pub(crate) async fn settle_upstream_reconciliation(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        upstream_usage: i64,
        local_billed_credits: i64,
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
            .unwrap_or_else(Utc::now);
        let day_bucket = start_of_local_day_utc_ts(attributed_utc.with_timezone(&Local));
        let month_start = start_of_month(attributed_utc).timestamp();
        let mut tx = self.pool.begin().await?;
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
            tx.rollback().await?;
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
        tx.commit().await?;
        Ok(true)
    }

    pub(crate) async fn settle_upstream_reconciliation_shadow(
        &self,
        candidate: &UpstreamReconciliationCandidate,
        upstream_usage: i64,
        local_billed_credits: i64,
    ) -> Result<bool, ProxyError> {
        let started_at = std::time::Instant::now();
        let now = self.backend_time.now_ts();
        let settlement_key = format!("v1:{}:{}", candidate.token_id, candidate.period_code);
        let delta = upstream_usage.saturating_sub(local_billed_credits);
        let attributed_at = candidate.period_end.saturating_sub(60);
        let mut tx = self.pool.begin().await?;
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
            tx.rollback().await?;
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
        tx.commit().await?;
        tracing::info!(
            component = "reconciliation",
            event = "shadow_adjustment_written",
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            job_type = "upstream_reconciliation",
            settlement_key,
            period_code = %candidate.period_code,
            delta_credits = delta,
            degraded = candidate.degraded,
        );
        Ok(true)
    }

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
                _ => {
                    buckets.other += count;
                }
            }
        }
        Ok(buckets)
    }

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

    pub(crate) async fn upstream_reconciliation_queue_counts(
        &self,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = self.backend_time.now_ts();
        let pending_research = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upstream_reconciliation_research WHERE terminal_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        let queued = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM (
                SELECT
                    u.token_id,
                    u.period_code,
                    MAX(u.period_end) AS period_end,
                    COALESCE((
                        SELECT COUNT(*)
                        FROM upstream_reconciliation_research r
                        WHERE r.token_id = u.token_id
                          AND r.period_code = u.period_code
                          AND r.terminal_at IS NULL
                    ), 0) AS pending_research
                FROM upstream_reconciliation_usage u
                LEFT JOIN upstream_reconciliation_settlements s
                  ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                WHERE s.settlement_key IS NULL
                   OR s.status IN ('pending', 'waiting', 'rate_limited')
                GROUP BY u.token_id, u.period_code
            ) pending_windows
            WHERE pending_windows.period_end + 600 <= ?
              AND (
                    pending_windows.pending_research = 0
                    OR pending_windows.period_end + 86400 <= ?
              )
            "#,
        )
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        let degraded = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upstream_reconciliation_settlements WHERE status IN ('degraded', 'shadow_degraded')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((pending_research, queued, degraded))
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
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
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
                ") THEN 1 ELSE 0 END), 0) AS terminal_windows \
                 FROM relevant_windows w \
                 LEFT JOIN upstream_reconciliation_settlements s \
                   ON s.settlement_key = 'v1:' || w.token_id || ':' || w.period_code \
                 GROUP BY w.billing_subject",
            );
        let shadow_window_rows = shadow_window_query
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        for (user_id, total_windows, terminal_windows) in shadow_window_rows {
            let entry = projections
                .entry(user_id)
                .or_insert(AccountShadowDailyProjection {
                    confirmed_delta_credits: 0,
                    observed_window_count: 0,
                    resolved_window_count: 0,
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
                });
            entry.observed_window_count += total_windows;
            entry.resolved_window_count += terminal_windows;
            entry.shadow_observed_window_count += total_windows;
            entry.shadow_resolved_window_count += terminal_windows;
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
                    shadow_observed_window_count: 0,
                    shadow_resolved_window_count: 0,
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
