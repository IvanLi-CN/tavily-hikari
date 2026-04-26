impl KeyStore {
    async fn list_active_mcp_session_counts_by_subject(
        &self,
        subject_column: &str,
        subject_value: &str,
        key_ids: &[String],
        now: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(format!(
            "SELECT upstream_key_id, COUNT(*) AS session_count FROM mcp_sessions \
             WHERE {subject_column} = "
        ));
        builder.push_bind(subject_value);
        builder.push(" AND revoked_at IS NULL AND expires_at > ");
        builder.push_bind(now);
        builder.push(" AND upstream_key_id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(") GROUP BY upstream_key_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn list_active_mcp_session_counts_for_user(
        &self,
        user_id: &str,
        key_ids: &[String],
        now: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.list_active_mcp_session_counts_by_subject("user_id", user_id, key_ids, now)
            .await
    }

    pub(crate) async fn list_active_mcp_session_counts_for_token(
        &self,
        token_id: &str,
        key_ids: &[String],
        now: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.list_active_mcp_session_counts_by_subject("auth_token_id", token_id, key_ids, now)
            .await
    }

    pub(crate) async fn has_active_mcp_sessions_for_token(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            r#"SELECT EXISTS(
                   SELECT 1
                     FROM mcp_sessions
                    WHERE auth_token_id = ?
                      AND revoked_at IS NULL
                      AND expires_at > ?
                    LIMIT 1
               )"#,
        )
        .bind(token_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists != 0)
    }

    pub(crate) async fn list_active_api_key_transient_backoffs(
        &self,
        key_ids: &[String],
        scope: &str,
        now: i64,
    ) -> Result<HashMap<String, ApiKeyTransientBackoffState>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT key_id, cooldown_until, retry_after_secs FROM api_key_transient_backoffs \
             WHERE scope = ",
        );
        builder.push_bind(scope);
        builder.push(" AND cooldown_until > ");
        builder.push_bind(now);
        builder.push(" AND key_id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(key_id, cooldown_until, retry_after_secs)| {
                (
                    key_id,
                    ApiKeyTransientBackoffState {
                        cooldown_until,
                        retry_after_secs,
                    },
                )
            })
            .collect())
    }

    pub(crate) async fn arm_api_key_transient_backoff(
        &self,
        arm: ApiKeyTransientBackoffArm<'_>,
    ) -> Result<Option<ApiKeyTransientBackoffState>, ProxyError> {
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
        .fetch_optional(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO api_key_transient_backoffs (
                key_id,
                scope,
                cooldown_until,
                retry_after_secs,
                reason_code,
                source_request_log_id,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(key_id, scope) DO UPDATE SET
                cooldown_until = MAX(api_key_transient_backoffs.cooldown_until, excluded.cooldown_until),
                retry_after_secs = CASE
                    WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until
                        THEN excluded.retry_after_secs
                    ELSE api_key_transient_backoffs.retry_after_secs
                END,
                reason_code = COALESCE(excluded.reason_code, api_key_transient_backoffs.reason_code),
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
        .execute(&self.pool)
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
        .fetch_one(&self.pool)
        .await?;

        if previous_cooldown.is_some_and(|previous| previous >= current.0) {
            return Ok(None);
        }

        Ok(Some(ApiKeyTransientBackoffState {
            cooldown_until: current.0,
            retry_after_secs: current.1,
        }))
    }

    pub(crate) async fn set_api_key_transient_backoff_request_log_id(
        &self,
        key_id: &str,
        scope: &str,
        request_log_id: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE api_key_transient_backoffs
            SET source_request_log_id = ?, updated_at = ?
            WHERE key_id = ? AND scope = ?
            "#,
        )
        .bind(request_log_id)
        .bind(now)
        .bind(key_id)
        .bind(scope)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_recent_billable_request_counts_for_keys(
        &self,
        key_ids: &[String],
        since: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key");
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let stored_counts_business_quota_sql =
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body");
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body");
        let effective_counts_business_quota_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_counts_business_quota_sql} ELSE {stored_counts_business_quota_sql} END"
        );
        let effective_non_billable_mcp_sql =
            token_request_kind_non_billable_mcp_sql(&effective_request_kind_sql);

        let mut builder = QueryBuilder::<Sqlite>::new(
            "
            SELECT api_key_id, COUNT(*) AS request_count
            FROM request_logs
            WHERE created_at >= "
                .to_string(),
        );
        builder.push_bind(since);
        builder.push(" AND api_key_id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(")");
        builder.push(format!(
            " AND (({effective_request_kind_sql}) LIKE 'api:%' \
                OR (({effective_request_kind_sql}) LIKE 'mcp:%' AND NOT {effective_non_billable_mcp_sql}))"
        ));
        builder.push(format!(" AND ({effective_counts_business_quota_sql}) = 1"));
        builder.push(" GROUP BY api_key_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn list_recent_rate_limited_request_counts_for_keys(
        &self,
        key_ids: &[String],
        since: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT api_key_id, COUNT(*) AS request_count FROM request_logs WHERE created_at >= ",
        );
        builder.push_bind(since);
        builder.push(" AND failure_kind = ");
        builder.push_bind(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429);
        builder.push(" AND api_key_id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(") GROUP BY api_key_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn list_api_key_last_used_at(
        &self,
        key_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder =
            QueryBuilder::<Sqlite>::new("SELECT id, last_used_at FROM api_keys WHERE id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn delete_stale_mcp_sessions(
        &self,
        now: i64,
        revoked_retention_threshold: i64,
    ) -> Result<i64, ProxyError> {
        let result = sqlx::query(
            r#"
            DELETE FROM mcp_sessions
            WHERE expires_at <= ?
               OR (revoked_at IS NOT NULL AND updated_at <= ?)
            "#,
        )
        .bind(now)
        .bind(revoked_retention_threshold)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as i64)
    }

    pub(crate) async fn delete_expired_api_key_transient_backoffs(
        &self,
        now: i64,
    ) -> Result<i64, ProxyError> {
        let result = sqlx::query(
            r#"
            DELETE FROM api_key_transient_backoffs
            WHERE cooldown_until <= ?
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as i64)
    }

    pub(crate) async fn set_request_log_key_effect_if_none(
        &self,
        request_log_id: i64,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE request_logs
            SET key_effect_code = ?, key_effect_summary = ?
            WHERE id = ? AND key_effect_code = ?
            "#,
        )
        .bind(key_effect_code)
        .bind(key_effect_summary)
        .bind(request_log_id)
        .bind(KEY_EFFECT_NONE)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_api_key_binding_counts_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT user_id, COUNT(*) AS api_key_count
               FROM (
                   SELECT DISTINCT user_id, api_key_id
                   FROM user_api_key_bindings
                   WHERE user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(
            r#")
                   UNION
                   SELECT DISTINCT user_id, api_key_id
                   FROM api_key_user_usage_buckets
                   WHERE user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(
            r#")
               )
               GROUP BY user_id"#,
        );

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn fetch_key_sticky_users_page(
        &self,
        key_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedApiKeyStickyUsers, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let now = Local::now();
        let today_start = start_of_local_day_utc_ts(now);
        let yesterday_start = previous_local_day_start_utc_ts(now);
        let month_start = start_of_local_month_utc_ts(now);
        let oldest_daily_date = now
            .date_naive()
            .checked_sub_days(chrono::Days::new(6))
            .unwrap_or_else(|| now.date_naive());
        let oldest_daily_start = local_date_start_utc_ts(oldest_daily_date, now);
        let usage_since = month_start.min(oldest_daily_start);

        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_api_key_bindings WHERE api_key_id = ?",
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
            ),
        >(
            r#"
            SELECT
                b.user_id,
                b.last_success_at,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0) AS yesterday_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0) AS yesterday_failure_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0) AS today_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0) AS today_failure_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start >= ? THEN u.success_credits ELSE 0 END), 0) AS month_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start >= ? THEN u.failure_credits ELSE 0 END), 0) AS month_failure_credits
            FROM user_api_key_bindings b
            LEFT JOIN api_key_user_usage_buckets u
              ON u.api_key_id = b.api_key_id
             AND u.user_id = b.user_id
             AND u.bucket_secs = ?
             AND u.bucket_start >= ?
            WHERE b.api_key_id = ?
            GROUP BY b.user_id, b.last_success_at
            ORDER BY
                (COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0)
                + COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0)) DESC,
                b.last_success_at DESC,
                b.user_id ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(yesterday_start)
        .bind(yesterday_start)
        .bind(today_start)
        .bind(today_start)
        .bind(month_start)
        .bind(month_start)
        .bind(SECS_PER_DAY)
        .bind(usage_since)
        .bind(key_id)
        .bind(today_start)
        .bind(today_start)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let user_ids = rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>();
        let identities = self.get_admin_user_identities(&user_ids).await?;

        let bucket_starts = (0..7_i64)
            .map(|index| oldest_daily_start + index * SECS_PER_DAY)
            .collect::<Vec<_>>();
        let daily_rows = if user_ids.is_empty() {
            Vec::new()
        } else {
            let mut builder = QueryBuilder::new(
                "SELECT user_id, bucket_start, success_credits, failure_credits \
                 FROM api_key_user_usage_buckets \
                 WHERE api_key_id = ",
            );
            builder.push_bind(key_id);
            builder.push(" AND bucket_secs = ");
            builder.push_bind(SECS_PER_DAY);
            builder.push(" AND bucket_start >= ");
            builder.push_bind(oldest_daily_start);
            builder.push(" AND user_id IN (");
            {
                let mut separated = builder.separated(", ");
                for user_id in &user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(") ORDER BY user_id ASC, bucket_start ASC");
            builder
                .build_query_as::<(String, i64, i64, i64)>()
                .fetch_all(&self.pool)
                .await?
        };

        let mut daily_map = HashMap::<String, HashMap<i64, StickyCreditsWindow>>::new();
        for (user_id, bucket_start, success_credits, failure_credits) in daily_rows {
            daily_map.entry(user_id).or_default().insert(
                bucket_start,
                StickyCreditsWindow {
                    success_credits,
                    failure_credits,
                },
            );
        }

        let mut items = Vec::with_capacity(rows.len());
        for (
            user_id,
            last_success_at,
            yesterday_success_credits,
            yesterday_failure_credits,
            today_success_credits,
            today_failure_credits,
            month_success_credits,
            month_failure_credits,
        ) in rows
        {
            let Some(user) = identities.get(&user_id).cloned() else {
                continue;
            };
            let user_daily = daily_map.get(&user_id);
            let daily_buckets = bucket_starts
                .iter()
                .map(|bucket_start| {
                    let bucket = user_daily
                        .and_then(|items| items.get(bucket_start))
                        .cloned()
                        .unwrap_or_default();
                    ApiKeyUserUsageBucket {
                        bucket_start: *bucket_start,
                        bucket_end: bucket_start.saturating_add(SECS_PER_DAY),
                        success_credits: bucket.success_credits,
                        failure_credits: bucket.failure_credits,
                    }
                })
                .collect::<Vec<_>>();

            items.push(ApiKeyStickyUser {
                user,
                last_success_at,
                yesterday: StickyCreditsWindow {
                    success_credits: yesterday_success_credits,
                    failure_credits: yesterday_failure_credits,
                },
                today: StickyCreditsWindow {
                    success_credits: today_success_credits,
                    failure_credits: today_failure_credits,
                },
                month: StickyCreditsWindow {
                    success_credits: month_success_credits,
                    failure_credits: month_failure_credits,
                },
                daily_buckets,
            });
        }

        Ok(PaginatedApiKeyStickyUsers {
            items,
            total,
            page,
            per_page,
        })
    }

    pub(crate) async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Ok(false);
        }

        let defaults = AccountQuotaLimits::legacy_defaults();
        let current = self.ensure_account_quota_limits(user_id).await?;
        let requested = AccountQuotaLimits {
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults: false,
        };
        let inherits_defaults = if current.inherits_defaults && requested.same_limits_as(&defaults)
        {
            1
        } else {
            0
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    hourly_any_limit = excluded.hourly_any_limit,
                    hourly_limit = excluded.hourly_limit,
                    daily_limit = excluded.daily_limit,
                    monthly_limit = excluded.monthly_limit,
                    inherits_defaults = excluded.inherits_defaults,
                    updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(inherits_defaults)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        self.record_effective_account_quota_snapshot_at(user_id, now)
            .await?;
        Ok(true)
    }

    pub(crate) async fn update_account_business_quota_limits(
        &self,
        user_id: &str,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        let current = self.ensure_account_quota_limits(user_id).await?;
        self.update_account_quota_limits(
            user_id,
            current.hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
        )
        .await
    }

    pub(crate) async fn backfill_account_quota_inherits_defaults_v1(
        &self,
    ) -> Result<(), ProxyError> {
        let defaults = AccountQuotaLimits::legacy_defaults();
        // Legacy rows do not record whether they were following defaults or manually customized.
        // Only rows that already match the current env tuple are safe to keep on the default-track;
        // every other tuple is conservatively treated as a custom baseline so upgrades never clobber
        // admin-set quotas.
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET inherits_defaults = CASE
                   WHEN hourly_any_limit = ?
                    AND hourly_limit = ?
                    AND daily_limit = ?
                    AND monthly_limit = ?
                   THEN 1
                   ELSE 0
               END"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn sync_account_quota_limits_with_defaults(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let defaults = AccountQuotaLimits::legacy_defaults();
        let affected_user_ids = sqlx::query_scalar::<_, String>(
            r#"SELECT user_id
               FROM account_quota_limits
               WHERE inherits_defaults = 1"#,
        )
        .fetch_all(&self.pool)
        .await?;
        let updated = sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = ?,
                   hourly_limit = ?,
                   daily_limit = ?,
                   monthly_limit = ?,
                   updated_at = ?
               WHERE inherits_defaults = 1"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_all_account_quota_resolutions().await;
        if updated.rows_affected() > 0 {
            self.record_effective_account_quota_snapshots_for_users_at(&affected_user_ids, now)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn account_quota_zero_base_cutover_at(&self) -> Result<i64, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1)
            .await?
            .unwrap_or(i64::MAX))
    }

    pub(crate) async fn default_account_quota_limits_for_user(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        let user_created_at =
            sqlx::query_scalar::<_, i64>("SELECT created_at FROM users WHERE id = ? LIMIT 1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        Ok(default_account_quota_limits_for_created_at(
            user_created_at,
            cutover_at,
        ))
    }

    pub(crate) async fn default_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        let mut builder = QueryBuilder::new("SELECT id, created_at FROM users WHERE id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(user_id, created_at)| {
                (
                    user_id,
                    default_account_quota_limits_for_created_at(created_at, cutover_at),
                )
            })
            .collect())
    }

    pub(crate) async fn ensure_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        if let Some(existing) = self.fetch_account_quota_limits(user_id).await? {
            return Ok(existing);
        }

        let now = Utc::now().timestamp();
        let defaults = self.default_account_quota_limits_for_user(user_id).await?;
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO NOTHING"#,
        )
        .bind(user_id)
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(if defaults.inherits_defaults { 1 } else { 0 })
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.fetch_account_quota_limits(user_id)
            .await?
            .ok_or_else(|| {
                ProxyError::Other(format!(
                    "account quota limits missing after ensure for user {user_id}"
                ))
            })
    }

    pub(crate) async fn ensure_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<(), ProxyError> {
        if user_ids.is_empty() {
            return Ok(());
        }

        let existing = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let missing_user_ids: Vec<String> = user_ids
            .iter()
            .filter(|user_id| !existing.contains_key(*user_id))
            .cloned()
            .collect();
        if missing_user_ids.is_empty() {
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let defaults_by_user = self
            .default_account_quota_limits_for_users(&missing_user_ids)
            .await?;

        let mut builder = QueryBuilder::new(
            "INSERT INTO account_quota_limits (user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults, created_at, updated_at) ",
        );
        builder.push_values(&missing_user_ids, |mut b, user_id| {
            let defaults = defaults_by_user
                .get(user_id)
                .cloned()
                .unwrap_or_else(AccountQuotaLimits::zero_base);
            b.push_bind(user_id)
                .push_bind(defaults.hourly_any_limit)
                .push_bind(defaults.hourly_limit)
                .push_bind(defaults.daily_limit)
                .push_bind(defaults.monthly_limit)
                .push_bind(if defaults.inherits_defaults { 1 } else { 0 })
                .push_bind(now)
                .push_bind(now);
        });
        builder.push(" ON CONFLICT(user_id) DO NOTHING");
        builder.build().execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn fetch_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountQuotaLimits>, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit,
                      COALESCE(inherits_defaults, 1)
               FROM account_quota_limits
               WHERE user_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults)| {
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                )
            },
        ))
    }

    pub(crate) async fn fetch_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, COALESCE(inherits_defaults, 1) FROM account_quota_limits WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (
            user_id,
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults,
        ) in rows
        {
            map.insert(
                user_id,
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                ),
            );
        }
        Ok(map)
    }

    pub(crate) async fn fetch_user_blocked_key_base_limit(&self) -> Result<i64, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_USER_BLOCKED_KEY_BASE_LIMIT_V1)
            .await?
            .unwrap_or(USER_MONTHLY_BROKEN_LIMIT_DEFAULT)
            .max(0))
    }

    pub(crate) async fn fetch_account_monthly_broken_limit(
        &self,
        user_id: &str,
    ) -> Result<i64, ProxyError> {
        self.ensure_account_quota_limits(user_id).await?;
        let base_limit = self.fetch_user_blocked_key_base_limit().await?;
        let delta = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(monthly_blocked_key_limit_delta, monthly_broken_limit - ?) FROM account_quota_limits WHERE user_id = ? LIMIT 1",
        )
        .bind(USER_MONTHLY_BROKEN_LIMIT_DEFAULT)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((base_limit + delta).max(0))
    }

    pub(crate) async fn fetch_account_monthly_broken_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        self.ensure_account_quota_limits_for_users(user_ids).await?;
        let base_limit = self.fetch_user_blocked_key_base_limit().await?;
        let mut builder = QueryBuilder::new("SELECT user_id, COALESCE(monthly_blocked_key_limit_delta, monthly_broken_limit - ");
        builder.push_bind(USER_MONTHLY_BROKEN_LIMIT_DEFAULT);
        builder.push(") FROM account_quota_limits WHERE user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(user_id, delta)| (user_id, (base_limit + delta).max(0)))
            .collect())
    }

    pub(crate) async fn update_account_monthly_broken_limit(
        &self,
        user_id: &str,
        monthly_broken_limit: i64,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Ok(false);
        }

        self.ensure_account_quota_limits(user_id).await?;
        let base_limit = self.fetch_user_blocked_key_base_limit().await?;
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET monthly_broken_limit = ?, monthly_blocked_key_limit_delta = ?, updated_at = ?
               WHERE user_id = ?"#,
        )
        .bind(monthly_broken_limit)
        .bind(monthly_broken_limit - base_limit)
        .bind(now)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    pub(crate) async fn record_manual_key_breakage_fanout(
        &self,
        key_id: &str,
        key_status: &str,
        reason_code: Option<&str>,
        reason_summary: Option<&str>,
        _actor: &MaintenanceActor,
        break_at: i64,
    ) -> Result<(), ProxyError> {
        let user_rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            r#"
            SELECT u.id, u.display_name, u.username
            FROM user_api_key_bindings b
            JOIN users u ON u.id = b.user_id
            WHERE b.api_key_id = ?
            ORDER BY u.username ASC, u.id ASC
            "#,
        )
        .bind(key_id)
        .fetch_all(&self.pool)
        .await?;
        let token_ids = sqlx::query_scalar::<_, String>(
            "SELECT token_id FROM token_api_key_bindings WHERE api_key_id = ? ORDER BY token_id ASC",
        )
        .bind(key_id)
        .fetch_all(&self.pool)
        .await?;
        if user_rows.is_empty() && token_ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        // Manual maintenance is billed to whichever subjects were still bound to the key.
        for (user_id, display_name, username) in &user_rows {
            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_USER,
                user_id,
                key_id,
                break_at,
                key_status,
                reason_code,
                reason_summary,
                BROKEN_KEY_SOURCE_MANUAL,
                None,
                Some(user_id.as_str()),
                display_name.as_deref().or(username.as_deref()),
                None,
            )
            .await?;
        }
        for token_id in &token_ids {
            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_TOKEN,
                token_id,
                key_id,
                break_at,
                key_status,
                reason_code,
                reason_summary,
                BROKEN_KEY_SOURCE_MANUAL,
                Some(token_id.as_str()),
                None,
                None,
                None,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn backfill_current_month_auto_subject_breakages(
        &self,
    ) -> Result<(), ProxyError> {
        let month_start = start_of_month(Utc::now()).timestamp();
        let rows =
            sqlx::query_as::<_, (String, String, String, i64, Option<String>, Option<String>)>(
                r#"
            SELECT
                key_id,
                auth_token_id,
                operation_code,
                created_at,
                reason_code,
                reason_summary
            FROM api_key_maintenance_records
            WHERE source = ?
              AND created_at >= ?
              AND auth_token_id IS NOT NULL
              AND operation_code = ?
              AND COALESCE(reason_code, '') IN (?, ?, ?)
            ORDER BY created_at ASC, key_id ASC
            "#,
            )
            .bind(MAINTENANCE_SOURCE_SYSTEM)
            .bind(month_start)
            .bind(MAINTENANCE_OP_AUTO_QUARANTINE)
            .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
            .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
            .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
            .fetch_all(&self.pool)
            .await?;
        if rows.is_empty() {
            return Ok(());
        }

        let mut token_ids: Vec<String> = rows
            .iter()
            .map(|(_, token_id, _, _, _, _)| token_id.clone())
            .collect();
        token_ids.sort_unstable();
        token_ids.dedup();
        let token_bindings = self.list_user_bindings_for_tokens(&token_ids).await?;

        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();
        let user_map = self.get_admin_user_identities(&user_ids).await?;

        let mut tx = self.pool.begin().await?;
        for (key_id, token_id, operation_code, created_at, reason_code, reason_summary) in rows {
            let _operation_code = operation_code;
            let key_status = KEY_EFFECT_QUARANTINED;
            let breaker_user_id = token_bindings.get(&token_id).cloned();
            let breaker_identity = breaker_user_id
                .as_ref()
                .and_then(|user_id| user_map.get(user_id));
            let breaker_display = breaker_identity.and_then(|identity| {
                identity
                    .display_name
                    .clone()
                    .or(identity.username.clone())
                    .or(Some(identity.user_id.clone()))
            });

            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_TOKEN,
                &token_id,
                &key_id,
                created_at,
                key_status,
                reason_code.as_deref(),
                reason_summary.as_deref(),
                BROKEN_KEY_SOURCE_AUTO,
                Some(&token_id),
                breaker_user_id.as_deref(),
                breaker_display.as_deref(),
                None,
            )
            .await?;

            if let Some(user_id) = breaker_user_id.as_deref() {
                self.upsert_subject_key_breakage_tx(
                    &mut tx,
                    BROKEN_KEY_SUBJECT_USER,
                    user_id,
                    &key_id,
                    created_at,
                    key_status,
                    reason_code.as_deref(),
                    reason_summary.as_deref(),
                    BROKEN_KEY_SOURCE_AUTO,
                    Some(&token_id),
                    Some(user_id),
                    breaker_display.as_deref(),
                    None,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn fetch_monthly_broken_counts_for_users(
        &self,
        user_ids: &[String],
        month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT skb.subject_id, COUNT(*) AS broken_count
               FROM subject_key_breakages skb
               JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
               LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
               WHERE skb.subject_kind = "#,
        );
        builder.push_bind(BROKEN_KEY_SUBJECT_USER);
        builder.push(" AND skb.month_start = ");
        builder.push_bind(month_start);
        builder.push(" AND aq.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.subject_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY skb.subject_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn fetch_monthly_broken_counts_for_tokens(
        &self,
        token_ids: &[String],
        month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT skb.subject_id, COUNT(*) AS broken_count
               FROM subject_key_breakages skb
               JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
               LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
               WHERE skb.subject_kind = "#,
        );
        builder.push_bind(BROKEN_KEY_SUBJECT_TOKEN);
        builder.push(" AND skb.month_start = ");
        builder.push_bind(month_start);
        builder.push(" AND aq.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.subject_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY skb.subject_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn list_monthly_broken_subjects_for_tokens(
        &self,
        token_ids: &[String],
        month_start: i64,
    ) -> Result<HashSet<String>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT DISTINCT skb.subject_id
               FROM subject_key_breakages skb
               JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
               LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
               WHERE skb.subject_kind = "#,
        );
        builder.push_bind(BROKEN_KEY_SUBJECT_TOKEN);
        builder.push(" AND skb.month_start = ");
        builder.push_bind(month_start);
        builder.push(" AND aq.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.subject_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_scalar::<String>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    async fn fetch_monthly_broken_related_users_for_keys(
        &self,
        key_ids: &[String],
    ) -> Result<HashMap<String, Vec<MonthlyBrokenKeyRelatedUser>>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT DISTINCT
                    b.api_key_id,
                    u.id,
                    u.display_name,
                    u.username
               FROM user_api_key_bindings b
               JOIN users u ON u.id = b.user_id
               WHERE b.api_key_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(") ORDER BY b.api_key_id ASC, u.username ASC, u.id ASC");

        let rows = builder
            .build_query_as::<(String, String, Option<String>, Option<String>)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map: HashMap<String, Vec<MonthlyBrokenKeyRelatedUser>> = HashMap::new();
        for (key_id, user_id, display_name, username) in rows {
            map.entry(key_id)
                .or_default()
                .push(MonthlyBrokenKeyRelatedUser {
                    user_id,
                    display_name,
                    username,
                });
        }
        Ok(map)
    }

    pub(crate) async fn fetch_monthly_broken_keys_page(
        &self,
        subject_kind: &str,
        subject_id: &str,
        page: i64,
        per_page: i64,
        month_start: i64,
    ) -> Result<PaginatedMonthlyBrokenKeys, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;

        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM subject_key_breakages skb
            JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
            LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE skb.subject_kind = ?
              AND skb.subject_id = ?
              AND skb.month_start = ?
              AND aq.reason_code IN (?, ?, ?)
              AND skb.reason_code IN (?, ?, ?)
            "#,
        )
        .bind(subject_kind)
        .bind(subject_id)
        .bind(month_start)
        .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
        .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
        .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
        .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
        .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
        .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                i64,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            r#"
            SELECT
                skb.key_id,
                CASE WHEN aq.key_id IS NOT NULL THEN ? ELSE ak.status END AS current_status,
                COALESCE(aq.reason_code, skb.reason_code) AS reason_code,
                COALESCE(aq.reason_summary, skb.reason_summary) AS reason_summary,
                skb.latest_break_at,
                skb.source,
                skb.breaker_token_id,
                skb.breaker_user_id,
                skb.breaker_user_display_name,
                skb.manual_actor_display_name
            FROM subject_key_breakages skb
            JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
            LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE skb.subject_kind = ?
              AND skb.subject_id = ?
              AND skb.month_start = ?
              AND aq.reason_code IN (?, ?, ?)
              AND skb.reason_code IN (?, ?, ?)
            ORDER BY skb.latest_break_at DESC, skb.key_id ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(KEY_EFFECT_QUARANTINED)
        .bind(subject_kind)
        .bind(subject_id)
        .bind(month_start)
        .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
        .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
        .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
        .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
        .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
        .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let key_ids: Vec<String> = rows.iter().map(|row| row.0.clone()).collect();
        let mut related_users = self
            .fetch_monthly_broken_related_users_for_keys(&key_ids)
            .await?;
        let items = rows
            .into_iter()
            .map(
                |(
                    key_id,
                    current_status,
                    reason_code,
                    reason_summary,
                    latest_break_at,
                    source,
                    breaker_token_id,
                    breaker_user_id,
                    breaker_user_display_name,
                    manual_actor_display_name,
                )| MonthlyBrokenKeyDetail {
                    key_id: key_id.clone(),
                    current_status,
                    reason_code,
                    reason_summary,
                    latest_break_at,
                    source,
                    breaker_token_id,
                    breaker_user_id,
                    breaker_user_display_name,
                    manual_actor_display_name,
                    related_users: related_users.remove(&key_id).unwrap_or_default(),
                },
            )
            .collect();

        Ok(PaginatedMonthlyBrokenKeys {
            items,
            total,
            page,
            per_page,
        })
    }

    pub(crate) async fn seed_linuxdo_system_tags(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        for level in 0..=4 {
            let system_key = linuxdo_system_key_for_level(level);
            let display_name = format!("L{level}");
            sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(system_key) DO UPDATE SET
                        name = excluded.name,
                        display_name = excluded.display_name,
                        icon = excluded.icon,
                        updated_at = excluded.updated_at"#,
            )
            .bind(&system_key)
            .bind(&system_key)
            .bind(display_name)
            .bind(USER_TAG_ICON_LINUXDO)
            .bind(&system_key)
            .bind(USER_TAG_EFFECT_QUOTA_DELTA)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn infer_linuxdo_system_tag_default_deltas_from_rows(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.len() != 5 {
            return Ok(None);
        }
        let mut expected: Option<(i64, i64, i64, i64)> = None;
        for (effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta) in rows {
            if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA {
                return Ok(None);
            }
            let current = (hourly_any_delta, hourly_delta, daily_delta, monthly_delta);
            match expected {
                Some(previous) if previous != current => return Ok(None),
                Some(_) => {}
                None => expected = Some(current),
            }
        }
        Ok(expected)
    }

    async fn list_user_ids_for_linuxdo_system_tags(&self) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            r#"SELECT DISTINCT b.user_id
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE t.system_key LIKE 'linuxdo_l%'"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn get_linuxdo_system_tag_default_deltas_meta(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let Some(raw) = self
            .get_meta_string(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
            .await?
        else {
            return Ok(None);
        };
        Ok(parse_linuxdo_system_tag_default_deltas(&raw))
    }

    pub(crate) async fn set_linuxdo_system_tag_default_deltas_meta(
        &self,
        value: (i64, i64, i64, i64),
    ) -> Result<(), ProxyError> {
        self.set_meta_string(
            META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1,
            &format_linuxdo_system_tag_default_deltas(value),
        )
        .await
    }

    pub(crate) async fn allow_registration(&self) -> Result<bool, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ALLOW_REGISTRATION_V1)
            .await?
            .unwrap_or(1)
            != 0)
    }

    pub(crate) async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.set_meta_i64(META_KEY_ALLOW_REGISTRATION_V1, if allow { 1 } else { 0 })
            .await?;
        Ok(allow)
    }

    pub(crate) async fn get_system_settings(&self) -> Result<SystemSettings, ProxyError> {
        let request_rate_limit = self
            .get_meta_i64(META_KEY_REQUEST_RATE_LIMIT_V1)
            .await?
            .unwrap_or(REQUEST_RATE_LIMIT)
            .max(REQUEST_RATE_LIMIT_MIN);
        let count = self
            .get_meta_i64(META_KEY_MCP_SESSION_AFFINITY_KEY_COUNT_V1)
            .await?
            .unwrap_or(MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT)
            .clamp(
                MCP_SESSION_AFFINITY_KEY_COUNT_MIN,
                MCP_SESSION_AFFINITY_KEY_COUNT_MAX,
            );
        let rebalance_mcp_enabled = self
            .get_meta_i64(META_KEY_REBALANCE_MCP_ENABLED_V1)
            .await?
            .unwrap_or(i64::from(REBALANCE_MCP_ENABLED_DEFAULT))
            != 0;
        let rebalance_mcp_session_percent = self
            .get_meta_i64(META_KEY_REBALANCE_MCP_SESSION_PERCENT_V1)
            .await?
            .unwrap_or(REBALANCE_MCP_SESSION_PERCENT_DEFAULT)
            .clamp(
                REBALANCE_MCP_SESSION_PERCENT_MIN,
                REBALANCE_MCP_SESSION_PERCENT_MAX,
            );
        let user_blocked_key_base_limit = self.fetch_user_blocked_key_base_limit().await?;
        Ok(SystemSettings {
            request_rate_limit,
            mcp_session_affinity_key_count: count,
            rebalance_mcp_enabled,
            rebalance_mcp_session_percent,
            user_blocked_key_base_limit,
        })
    }

    pub(crate) async fn set_system_settings(
        &self,
        settings: &SystemSettings,
    ) -> Result<SystemSettings, ProxyError> {
        if settings.request_rate_limit < REQUEST_RATE_LIMIT_MIN {
            return Err(ProxyError::Other(format!(
                "request_rate_limit must be at least {}",
                REQUEST_RATE_LIMIT_MIN,
            )));
        }
        if !(MCP_SESSION_AFFINITY_KEY_COUNT_MIN..=MCP_SESSION_AFFINITY_KEY_COUNT_MAX)
            .contains(&settings.mcp_session_affinity_key_count)
        {
            return Err(ProxyError::Other(format!(
                "mcp_session_affinity_key_count must be between {} and {}",
                MCP_SESSION_AFFINITY_KEY_COUNT_MIN, MCP_SESSION_AFFINITY_KEY_COUNT_MAX,
            )));
        }
        if !(REBALANCE_MCP_SESSION_PERCENT_MIN..=REBALANCE_MCP_SESSION_PERCENT_MAX)
            .contains(&settings.rebalance_mcp_session_percent)
        {
            return Err(ProxyError::Other(format!(
                "rebalance_mcp_session_percent must be between {} and {}",
                REBALANCE_MCP_SESSION_PERCENT_MIN, REBALANCE_MCP_SESSION_PERCENT_MAX,
            )));
        }
        if settings.user_blocked_key_base_limit < 0 {
            return Err(ProxyError::Other(
                "user_blocked_key_base_limit must be a non-negative integer".to_string(),
            ));
        }
        self.set_meta_i64(META_KEY_REQUEST_RATE_LIMIT_V1, settings.request_rate_limit)
            .await?;
        self.set_meta_i64(
            META_KEY_MCP_SESSION_AFFINITY_KEY_COUNT_V1,
            settings.mcp_session_affinity_key_count,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REBALANCE_MCP_ENABLED_V1,
            i64::from(settings.rebalance_mcp_enabled),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REBALANCE_MCP_SESSION_PERCENT_V1,
            settings.rebalance_mcp_session_percent,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_USER_BLOCKED_KEY_BASE_LIMIT_V1,
            settings.user_blocked_key_base_limit,
        )
        .await?;
        self.record_request_rate_limit_snapshot_at(
            settings.request_rate_limit,
            Utc::now().timestamp(),
        )
        .await?;
        self.get_system_settings().await
    }

    pub(crate) async fn set_mcp_session_affinity_key_count(
        &self,
        count: i64,
    ) -> Result<SystemSettings, ProxyError> {
        let mut settings = self.get_system_settings().await?;
        settings.mcp_session_affinity_key_count = count;
        self.set_system_settings(&settings).await
    }

    pub(crate) async fn sync_linuxdo_system_tag_default_deltas_with_env(
        &self,
    ) -> Result<(), ProxyError> {
        let current = linuxdo_system_tag_default_deltas();
        let previous = match self.get_linuxdo_system_tag_default_deltas_meta().await? {
            Some(value) => value,
            None => self
                .infer_linuxdo_system_tag_default_deltas_from_rows()
                .await?
                .unwrap_or(current),
        };
        if previous == current {
            self.set_linuxdo_system_tag_default_deltas_meta(current)
                .await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let affected_user_ids = self.list_user_ids_for_linuxdo_system_tags().await?;
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = ?
                 AND hourly_delta = ?
                 AND daily_delta = ?
                 AND monthly_delta = ?"#,
        )
        .bind(current.0)
        .bind(current.1)
        .bind(current.2)
        .bind(current.3)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .bind(previous.0)
        .bind(previous.1)
        .bind(previous.2)
        .bind(previous.3)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
            self.record_effective_account_quota_snapshots_for_users_at(&affected_user_ids, now)
                .await?;
        }
        self.set_linuxdo_system_tag_default_deltas_meta(current)
            .await?;
        Ok(())
    }

    pub(crate) async fn backfill_linuxdo_system_tag_default_deltas_v1(
        &self,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        let affected_user_ids = self.list_user_ids_for_linuxdo_system_tags().await?;
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = 0
                 AND hourly_delta = 0
                 AND daily_delta = 0
                 AND monthly_delta = 0"#,
        )
        .bind(hourly_any_delta)
        .bind(hourly_delta)
        .bind(daily_delta)
        .bind(monthly_delta)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
            self.record_effective_account_quota_snapshots_for_users_at(&affected_user_ids, now)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let changed_at = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;
        self.sync_linuxdo_system_tag_binding_in_tx(&mut tx, user_id, trust_level)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolution(user_id).await;
        self.record_effective_account_quota_snapshot_at(user_id, changed_at)
            .await?;
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_in_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let Some(level) = normalize_linuxdo_trust_level(trust_level) else {
            return Ok(());
        };
        let desired_key = linuxdo_system_key_for_level(level);
        let Some((tag_id,)) =
            sqlx::query_as::<_, (String,)>("SELECT id FROM user_tags WHERE system_key = ? LIMIT 1")
                .bind(&desired_key)
                .fetch_optional(&mut **tx)
                .await?
        else {
            eprintln!(
                "linuxdo system tag sync skipped for user {} trust_level {:?}: missing system tag for LinuxDo trust level {}",
                user_id, trust_level, level
            );
            return Ok(());
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"DELETE FROM user_tag_bindings
               WHERE user_id = ?
                 AND tag_id IN (
                     SELECT id FROM user_tags WHERE system_key LIKE 'linuxdo_l%'
                 )
                 AND tag_id <> ?"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .bind(USER_TAG_SOURCE_SYSTEM_LINUXDO)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_best_effort(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) {
        if let Err(err) = self
            .sync_linuxdo_system_tag_binding(user_id, trust_level)
            .await
        {
            eprintln!(
                "linuxdo system tag sync error for user {} trust_level {:?}: {}",
                user_id, trust_level, err
            );
        }
    }

    pub(crate) async fn backfill_linuxdo_user_tag_bindings(&self) -> Result<(), ProxyError> {
        let rows = sqlx::query_as::<_, (String, Option<i64>)>(
            r#"SELECT user_id, trust_level
               FROM oauth_accounts
               WHERE provider = 'linuxdo'"#,
        )
        .fetch_all(&self.pool)
        .await?;
        for (user_id, trust_level) in rows {
            self.sync_linuxdo_system_tag_binding(&user_id, trust_level)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn fetch_user_tag_by_id(
        &self,
        tag_id: &str,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
            ),
        >(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               WHERE t.id = ?
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               LIMIT 1"#,
        )
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            )| UserTagRecord {
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            },
        ))
    }

    pub(crate) async fn list_user_tags(&self) -> Result<Vec<UserTagRecord>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, String, i64, i64, i64, i64, i64)>(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                )| UserTagRecord {
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                },
            )
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<UserTagRecord, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        for _ in 0..8 {
            let id = random_string(ALPHABET, 8);
            let inserted = sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(_) => {
                    return self
                        .fetch_user_tag_by_id(&id)
                        .await?
                        .ok_or_else(|| ProxyError::Other("created user tag missing".to_string()));
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    if db_err.message().contains("user_tags.name") {
                        return Err(ProxyError::Other(
                            "user tag name already exists".to_string(),
                        ));
                    }
                    continue;
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        Err(ProxyError::Other(
            "failed to allocate unique user tag id".to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_user_tag(
        &self,
        tag_id: &str,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(None);
        };
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let now = Utc::now().timestamp();
        if existing.is_system() {
            if existing.name != name
                || existing.display_name != display_name
                || existing.icon.as_deref() != icon
            {
                return Err(ProxyError::Other(
                    "system user tags only allow effect updates".to_string(),
                ));
            }
            sqlx::query(
                r#"UPDATE user_tags
                   SET effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        } else {
            let updated = sqlx::query(
                r#"UPDATE user_tags
                   SET name = ?,
                       display_name = ?,
                       icon = ?,
                       effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await;
            match updated {
                Ok(_) => {}
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    return Err(ProxyError::Other(
                        "user tag name already exists".to_string(),
                    ));
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        self.record_effective_account_quota_snapshots_for_users_at(&affected_user_ids, now)
            .await?;
        self.fetch_user_tag_by_id(tag_id).await
    }

    pub(crate) async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if existing.is_system() {
            return Err(ProxyError::Other(
                "system user tags cannot be deleted".to_string(),
            ));
        }
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_tag_bindings WHERE tag_id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM user_tags WHERE id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        self.record_effective_account_quota_snapshots_for_users_at(
            &affected_user_ids,
            Utc::now().timestamp(),
        )
        .await?;
        Ok(true)
    }

    pub(crate) async fn bind_user_tag_to_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        let user_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if user_exists == 0 {
            return Ok(false);
        }
        let Some(tag) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if tag.is_system() {
            return Err(ProxyError::Other(
                "system user tags are managed by the server".to_string(),
            ));
        }
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .bind(USER_TAG_SOURCE_MANUAL)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        self.record_effective_account_quota_snapshot_at(user_id, now)
            .await?;
        Ok(true)
    }

    pub(crate) async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        let binding = sqlx::query_as::<_, (String, Option<String>)>(
            r#"SELECT b.source, t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND b.tag_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((source, system_key)) = binding else {
            return Ok(false);
        };
        if source != USER_TAG_SOURCE_MANUAL || system_key.is_some() {
            return Err(ProxyError::Other(
                "system-managed user tag bindings are read-only".to_string(),
            ));
        }
        sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ? AND tag_id = ?")
            .bind(user_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        self.record_effective_account_quota_snapshot_at(user_id, Utc::now().timestamp())
            .await?;
        Ok(true)
    }

    pub(crate) async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<UserTagBindingRecord>>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            r#"SELECT
                 b.user_id,
                 b.source,
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC");

        let rows = builder
            .build_query_as::<(
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;
        let mut map: HashMap<String, Vec<UserTagBindingRecord>> = HashMap::new();
        for (
            user_id,
            source,
            tag_id,
            name,
            display_name,
            icon,
            system_key,
            effect_kind,
            hourly_any_delta,
            hourly_delta,
            daily_delta,
            monthly_delta,
        ) in rows
        {
            map.entry(user_id.clone())
                .or_default()
                .push(UserTagBindingRecord {
                    source,
                    tag: UserTagRecord {
                        id: tag_id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        user_count: 0,
                    },
                });
        }
        Ok(map)
    }

    pub(crate) async fn list_user_tag_bindings_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserTagBindingRecord>, ProxyError> {
        Ok(self
            .list_user_tag_bindings_for_users(&[user_id.to_string()])
            .await?
            .remove(user_id)
            .unwrap_or_default())
    }

    pub(crate) async fn resolve_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.ensure_account_quota_limits_for_users(user_ids).await?;
        let base_limits = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let tag_bindings = self.list_user_tag_bindings_for_users(user_ids).await?;
        let defaults = AccountQuotaLimits::zero_base();
        let mut map = HashMap::new();
        for user_id in user_ids {
            let base = base_limits
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| defaults.clone());
            let tags = tag_bindings.get(user_id).cloned().unwrap_or_default();
            map.insert(
                user_id.clone(),
                build_account_quota_resolution(base, tags).effective,
            );
        }
        Ok(map)
    }

    pub(crate) async fn resolve_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaResolution, ProxyError> {
        if let Some(cached) = self.cached_account_quota_resolution(user_id).await {
            return Ok(cached);
        }

        let base = self.ensure_account_quota_limits(user_id).await?;
        let tags = self.list_user_tag_bindings_for_user(user_id).await?;
        let resolution = build_account_quota_resolution(base, tags);
        self.cache_account_quota_resolution(user_id, &resolution)
            .await;
        Ok(resolution)
    }

    pub(crate) async fn fetch_user_log_metrics_bulk(
        &self,
        user_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, UserLogMetricsSummary>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        let mut builder = QueryBuilder::new(
            r#"
            SELECT
              b.user_id,
              COALESCE(SUM(CASE WHEN l.result_status = "#,
        );
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_failure, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_failure, ");
        builder.push(
            r#"MAX(l.created_at) AS last_activity
            FROM user_token_bindings b
            LEFT JOIN auth_token_logs l ON l.token_id = b.token_id
            WHERE b.user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY b.user_id");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    user_id,
                    daily_success,
                    daily_failure,
                    monthly_success,
                    monthly_failure,
                    last_activity,
                )| {
                    (
                        user_id,
                        UserLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                },
            )
            .collect())
    }

    pub(crate) async fn fetch_token_log_metrics_bulk(
        &self,
        token_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, TokenLogMetricsSummary>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        let mut builder = QueryBuilder::new(
            r#"
            SELECT
              l.token_id,
              COALESCE(SUM(CASE WHEN l.result_status = "#,
        );
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_failure, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_failure, ");
        builder.push(
            r#"MAX(l.created_at) AS last_activity
            FROM auth_token_logs l
            WHERE l.token_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY l.token_id");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    token_id,
                    daily_success,
                    daily_failure,
                    monthly_success,
                    monthly_failure,
                    last_activity,
                )| {
                    (
                        token_id,
                        TokenLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                },
            )
            .collect())
    }

    pub(crate) async fn insert_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let state = random_string(ALPHABET, 48);
            let res = sqlx::query(
                r#"INSERT INTO oauth_login_states
                   (state, provider, redirect_to, binding_hash, bind_token_id, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&state)
            .bind(provider)
            .bind(redirect_to.map(str::trim).filter(|value| !value.is_empty()))
            .bind(
                binding_hash
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
            .bind(bind_token_id.map(str::trim).filter(|value| !value.is_empty()))
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => return Ok(state),
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = if let Some(hash) = binding_hash
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash = ?
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash IS NULL
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((redirect_to, bind_token_id)) = row else {
            tx.rollback().await.ok();
            return Ok(None);
        };

        let updated = sqlx::query(
            r#"UPDATE oauth_login_states
               SET consumed_at = ?
               WHERE state = ? AND provider = ? AND consumed_at IS NULL"#,
        )
        .bind(now)
        .bind(state)
        .bind(provider)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(None);
        }

        tx.commit().await?;
        Ok(Some(OAuthLoginStatePayload {
            redirect_to,
            bind_token_id,
        }))
    }

}
