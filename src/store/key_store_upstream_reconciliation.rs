const RECONCILIATION_SETTLEMENT_MODE_ACTUAL: &str = "actual";
const RECONCILIATION_SETTLEMENT_MODE_SHADOW: &str = "shadow";
const RECONCILIATION_STATUS_SHADOW_SETTLED: &str = "shadow_settled";
const RECONCILIATION_STATUS_SHADOW_DEGRADED: &str = "shadow_degraded";

impl KeyStore {
    pub(crate) async fn count_active_control_mcp_sessions(
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
        let active_control_sessions = self.count_active_control_mcp_sessions(now).await?;
        let static_ready = settings.upstream_project_id_mode == UpstreamProjectIdMode::AccessToken
            && settings.api_rebalance_enabled
            && settings.api_rebalance_percent == 100
            && settings.rebalance_mcp_enabled
            && settings.rebalance_mcp_session_percent == 100;
        let current = self
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1)
            .await?
            .unwrap_or(0);
        if !static_ready || active_control_sessions > 0 {
            if current != 0 {
                self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1, 0)
                    .await?;
            }
            return Ok((false, 0, active_control_sessions));
        }
        let ready_after = if current <= 0 {
            let next = business_period_for_timestamp(now).ends_at;
            self.set_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1, next)
                .await?;
            next
        } else {
            current
        };
        Ok((now >= ready_after, ready_after, active_control_sessions))
    }

    pub(crate) async fn record_upstream_reconciliation_usage(
        &self,
        token_id: &str,
        key_id: &str,
        billing_subject: &str,
        research_request_id: Option<&str>,
    ) -> Result<Option<BusinessPeriod>, ProxyError> {
        let (eligible, _, _) = self.refresh_upstream_reconciliation_epoch().await?;
        if !eligible {
            return Ok(None);
        }
        let settlement_mode = if self
            .get_system_settings()
            .await?
            .upstream_precise_reconciliation_enabled
        {
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

    pub(crate) async fn next_upstream_reconciliation_candidates(
        &self,
        limit: i64,
    ) -> Result<Vec<UpstreamReconciliationCandidate>, ProxyError> {
        let now = self.backend_time.now_ts();
        let rows = sqlx::query_as::<_, (String, String, String, String, String, i64, i64, i64)>(
            r#"
            SELECT
                u.token_id,
                u.period_code,
                MIN(u.project_id),
                MIN(u.billing_subject),
                MIN(u.settlement_mode),
                MIN(u.period_start),
                MAX(u.period_end),
                COALESCE((
                    SELECT COUNT(*)
                    FROM upstream_reconciliation_research r
                    WHERE r.token_id = u.token_id
                      AND r.period_code = u.period_code
                      AND r.terminal_at IS NULL
                ), 0)
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE u.period_end + 600 <= ?
              AND (s.settlement_key IS NULL OR (
                    s.status IN ('pending', 'waiting', 'rate_limited')
                    AND COALESCE(s.next_attempt_at, 0) <= ?
              ))
            GROUP BY u.token_id, u.period_code
            ORDER BY MAX(u.period_end) ASC
            LIMIT ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
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
            .collect())
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
        .bind(reason)
        .bind(next_attempt_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
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
            "degraded"
        } else {
            "settled"
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
        tx.commit().await?;
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

    pub(crate) async fn shadow_daily_reconciled_usage_for_accounts(
        &self,
        user_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut query = QueryBuilder::new(
            "SELECT SUBSTR(billing_subject, 9) AS user_id, COALESCE(SUM(delta_credits), 0) \
             FROM billing_reconciliation_shadow_adjustments \
             WHERE billing_subject IN (",
        );
        {
            let mut separated = query.separated(", ");
            user_ids.iter().for_each(|user_id| {
                separated.push_bind(format!("account:{user_id}"));
            });
        }
        query
            .push(") AND attributed_at >= ")
            .push_bind(day_start)
            .push(" AND attributed_at < ")
            .push_bind(day_end)
            .push(" GROUP BY billing_subject");
        let rows = query
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }
}
