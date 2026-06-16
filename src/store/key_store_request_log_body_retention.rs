impl KeyStore {
    fn request_log_body_days_for_profile(
        profile: &RequestLogRetentionProfile,
        result_status: &str,
        request_value_bucket: RequestValueBucket,
    ) -> i64 {
        if result_status != OUTCOME_SUCCESS {
            return profile.non_success_body_days;
        }
        match request_value_bucket {
            RequestValueBucket::Valuable => profile.business_body_days,
            RequestValueBucket::Other | RequestValueBucket::Unknown => profile.non_business_body_days,
        }
    }

    fn request_log_body_min_possible_cursor_days(
        settings: &RequestLogRetentionSettings,
        result_status: &str,
        request_value_bucket: RequestValueBucket,
        has_user: bool,
    ) -> i64 {
        let mut days = Self::request_log_body_days_for_profile(
            &settings.global,
            result_status,
            request_value_bucket,
        );
        if has_user {
            days = days.min(Self::request_log_body_days_for_profile(
                &settings.heavy_usage,
                result_status,
                request_value_bucket,
            ));
            days = days.min(Self::request_log_body_days_for_profile(
                &settings.debug_shared,
                result_status,
                request_value_bucket,
            ));
        }
        days
    }

    fn request_log_body_cursor_retention_days(
        settings: &RequestLogRetentionSettings,
        retention_decision: &RequestLogBodyRetentionDecision,
        result_status: &str,
        request_value_bucket: RequestValueBucket,
        has_user: bool,
    ) -> i64 {
        if retention_decision.profile == REQUEST_LOG_BODY_RETENTION_PROFILE_DEBUG_SHARED {
            retention_decision.days
        } else {
            Self::request_log_body_min_possible_cursor_days(
                settings,
                result_status,
                request_value_bucket,
                has_user,
            )
        }
    }

    async fn request_log_user_is_heavy_usage(
        &self,
        user_id: &str,
        threshold_percent: i64,
        additional_usage: i64,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_utc();
        let now_ts = now.timestamp();
        let current_hour = now_ts - now_ts.rem_euclid(SECS_PER_HOUR);
        let rolling_hour_start = current_hour.saturating_sub(23 * SECS_PER_HOUR);
        let local_now = now.with_timezone(&Local);
        let day_start = start_of_local_day_utc_ts(local_now);
        let day_end = next_local_day_start_utc_ts(day_start);
        let current_day_used = self
            .sum_account_usage_buckets(user_id, GRANULARITY_DAY, day_start)
            .await?
            + self
                .sum_account_usage_buckets_between(user_id, GRANULARITY_HOUR, day_start, day_end)
                .await?;
        let rolling_hour_used = self
            .sum_account_usage_buckets_between(
                user_id,
                GRANULARITY_HOUR,
                rolling_hour_start,
                current_hour.saturating_add(SECS_PER_HOUR),
            )
            .await?;
        let recent_minute_used = self
            .sum_account_usage_buckets_between(
                user_id,
                GRANULARITY_MINUTE,
                rolling_hour_start,
                now_ts.saturating_add(SECS_PER_MINUTE),
            )
            .await?;
        let used = current_day_used
            .max(rolling_hour_used)
            .max(recent_minute_used)
            .saturating_add(additional_usage.max(0));
        let resolution = self.resolve_account_quota_resolution(user_id).await?;
        let daily_limit = resolution.effective.daily_limit;
        Ok(daily_limit > 0
            && i128::from(used).saturating_mul(100)
                >= i128::from(daily_limit).saturating_mul(i128::from(threshold_percent)))
    }

    async fn request_log_body_retention_decision(
        &self,
        settings: &RequestLogRetentionSettings,
        user_id: Option<&str>,
        result_status: &str,
        request_value_bucket: RequestValueBucket,
        additional_usage: i64,
        mode: RequestLogBodyRetentionDecisionMode,
    ) -> Result<RequestLogBodyRetentionDecision, ProxyError> {
        if let Some(user_id) = user_id {
            if mode.include_debug_shared && self.user_debug_info_shared(user_id).await? {
                return Ok(RequestLogBodyRetentionDecision {
                    days: Self::request_log_body_days_for_profile(
                    &settings.debug_shared,
                    result_status,
                    request_value_bucket,
                    ),
                    profile: REQUEST_LOG_BODY_RETENTION_PROFILE_DEBUG_SHARED,
                });
            }

            if mode.include_heavy_usage && self
                .request_log_user_is_heavy_usage(
                    user_id,
                    settings.heavy_usage_threshold_percent,
                    additional_usage,
                )
                .await?
            {
                return Ok(RequestLogBodyRetentionDecision {
                    days: Self::request_log_body_days_for_profile(
                    &settings.heavy_usage,
                    result_status,
                    request_value_bucket,
                    ),
                    profile: REQUEST_LOG_BODY_RETENTION_PROFILE_HEAVY_USAGE,
                });
            }
        }

        Ok(RequestLogBodyRetentionDecision {
            days: Self::request_log_body_days_for_profile(
                &settings.global,
                result_status,
                request_value_bucket,
            ),
            profile: REQUEST_LOG_BODY_RETENTION_PROFILE_GLOBAL,
        })
    }

    fn request_log_body_storage_decision<'a>(
        retention_days: i64,
        retention_profile: &'static str,
        input: RequestLogBodyStorageInput<'a>,
    ) -> RequestLogBodyStorageDecision<'a> {
        let request_body_bytes = input.request_body.len() as i64;
        let response_body_bytes = input.response_body.len() as i64;
        let body_cleaned_reason = (retention_days <= 0
            && (request_body_bytes > 0 || response_body_bytes > 0))
            .then_some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO);
        RequestLogBodyStorageDecision {
            request_body: (retention_days > 0).then_some(input.request_body),
            response_body: (retention_days > 0).then_some(input.response_body),
            request_body_bytes,
            response_body_bytes,
            request_body_sha256: sha256_hex_bytes(input.request_body),
            response_body_sha256: sha256_hex_bytes(input.response_body),
            body_retention_days: retention_days,
            body_retention_profile: retention_profile,
            body_cleaned_reason,
            body_cleaned_at: body_cleaned_reason.map(|_| input.created_at),
        }
    }

    pub(crate) async fn log_attempt(&self, entry: AttemptLog<'_>) -> Result<i64, ProxyError> {
        let created_at = self.backend_time.now_ts();
        let status_code = entry.status.map(|code| code.as_u16() as i64);
        let failure_kind = entry.failure_kind.map(str::to_string).or_else(|| {
            if entry.outcome == OUTCOME_ERROR {
                classify_failure_kind(
                    entry.path,
                    status_code,
                    entry.tavily_status_code,
                    entry.error,
                    entry.response_body,
                )
            } else {
                None
            }
        });
        let key_effect_summary = entry.key_effect_summary.map(str::to_string);
        let binding_effect_summary = entry.binding_effect_summary.map(str::to_string);
        let selection_effect_summary = entry.selection_effect_summary.map(str::to_string);
        let request_kind = normalize_request_kind_for_response_context(
            classify_token_request_kind(entry.path, Some(entry.request_body)),
            ResponseRequestKindContext {
                method: entry.method.as_str(),
                path: entry.path,
                http_status: status_code,
                tavily_status: entry.tavily_status_code,
                failure_kind: failure_kind.as_deref(),
                error_message: entry.error,
                response_body: entry.response_body,
            },
        );

        let forwarded_json =
            serde_json::to_string(entry.forwarded_headers).unwrap_or_else(|_| "[]".to_string());
        let dropped_json =
            serde_json::to_string(entry.dropped_headers).unwrap_or_else(|_| "[]".to_string());
        let request_user_id = if let Some(token_id) = entry.auth_token_id {
            self.resolve_request_rollup_user_id(token_id, None).await?
        } else {
            None
        };
        let remote_addr = entry.client_ip.and_then(|info| info.remote_addr.as_deref());
        let client_ip = entry.client_ip.and_then(|info| info.client_ip.as_deref());
        let client_ip_source = entry
            .client_ip
            .and_then(|info| info.client_ip_source.as_deref());
        let client_ip_trusted = entry
            .client_ip
            .map(|info| i64::from(info.client_ip_trusted))
            .unwrap_or(0);
        let ip_headers_json = entry
            .client_ip
            .map(|info| serde_json::to_string(&info.ip_headers))
            .transpose()
            .unwrap_or_else(|_| Some("[]".to_string()));

        let request_value_bucket =
            request_value_bucket_for_request_log(&request_kind.key, Some(entry.request_body));
        let counts_business_quota =
            request_log_counts_business_quota(&request_kind.key, Some(entry.request_body));
        let retention_usage_delta =
            i64::from(counts_business_quota && entry.outcome == OUTCOME_SUCCESS);
        let request_log_retention = self.get_request_log_retention_settings_cached().await?;
        let retention_decision = self
            .request_log_body_retention_decision(
                &request_log_retention,
                request_user_id.as_deref(),
                entry.outcome,
                request_value_bucket,
                retention_usage_delta,
                RequestLogBodyRetentionDecisionMode {
                    include_debug_shared: true,
                    include_heavy_usage: false,
                },
            )
            .await?;
        let body_storage = Self::request_log_body_storage_decision(
            retention_decision.days,
            retention_decision.profile,
            RequestLogBodyStorageInput {
                request_body: entry.request_body,
                response_body: entry.response_body,
                created_at,
            },
        );
        let dashboard_rollup_counts = Self::dashboard_rollup_counts_for_request(
            &request_kind.key,
            Some(entry.request_body),
            entry.outcome,
            failure_kind.as_deref(),
            0,
            counts_business_quota,
        );
        let request_log_catalog_key = (entry.outcome != OUTCOME_UNKNOWN
            && request_kind.key.trim() != "api:unknown-path")
            .then(|| {
                Self::request_log_catalog_rollup_key_for_request(
                    created_at,
                    &request_kind.key,
                    &request_kind.label,
                    counts_business_quota,
                    entry.outcome,
                    failure_kind.as_deref(),
                    entry.key_effect_code,
                    entry.binding_effect_code,
                    entry.selection_effect_code,
                    entry.auth_token_id,
                    entry.key_id,
                )
            });
        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO observability.request_logs (
                api_key_id,
                auth_token_id,
                request_user_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                counts_business_quota,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                binding_effect_code,
                binding_effect_summary,
                selection_effect_code,
                selection_effect_summary,
                gateway_mode,
                experiment_variant,
                proxy_session_id,
                routing_subject_hash,
                upstream_operation,
                fallback_reason,
                request_body,
                response_body,
                request_body_bytes,
                response_body_bytes,
                request_body_sha256,
                response_body_sha256,
                body_retention_days,
                body_retention_profile,
                body_cleaned_reason,
                body_cleaned_at,
                forwarded_headers,
                dropped_headers,
                remote_addr,
                client_ip,
                client_ip_source,
                client_ip_trusted,
                ip_headers,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(entry.key_id)
        .bind(entry.auth_token_id)
        .bind(request_user_id.as_deref())
        .bind(entry.method.as_str())
        .bind(entry.path)
        .bind(entry.query)
        .bind(status_code)
        .bind(entry.tavily_status_code)
        .bind(entry.error)
        .bind(entry.outcome)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(i64::from(counts_business_quota))
        .bind(None::<i64>)
        .bind(failure_kind)
        .bind(entry.key_effect_code)
        .bind(key_effect_summary)
        .bind(entry.binding_effect_code)
        .bind(binding_effect_summary)
        .bind(entry.selection_effect_code)
        .bind(selection_effect_summary)
        .bind(entry.gateway_mode)
        .bind(entry.experiment_variant)
        .bind(entry.proxy_session_id)
        .bind(entry.routing_subject_hash)
        .bind(entry.upstream_operation)
        .bind(entry.fallback_reason)
        .bind(body_storage.request_body)
        .bind(body_storage.response_body)
        .bind(body_storage.request_body_bytes)
        .bind(body_storage.response_body_bytes)
        .bind(body_storage.request_body_sha256)
        .bind(body_storage.response_body_sha256)
        .bind(body_storage.body_retention_days)
        .bind(body_storage.body_retention_profile)
        .bind(body_storage.body_cleaned_reason)
        .bind(body_storage.body_cleaned_at)
        .bind(forwarded_json)
        .bind(dropped_json)
        .bind(remote_addr)
        .bind(client_ip)
        .bind(client_ip_source)
        .bind(client_ip_trusted)
        .bind(ip_headers_json)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        self.request_stats_coalescer
            .enqueue_request_log_rollups(
                entry.key_id,
                entry.auth_token_id.unwrap_or_default(),
                request_user_id.as_deref(),
                created_at,
                dashboard_rollup_counts,
                request_log_catalog_key,
            )
            .await;
        Ok(request_log_id)
    }

    pub(crate) fn api_key_metrics_from_clause() -> &'static str {
        r#"
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            LEFT JOIN (
                SELECT
                    key_id,
                    MAX(cooldown_until) AS transient_backoff_cooldown_until,
                    MAX(retry_after_secs) AS transient_backoff_retry_after_secs,
                    GROUP_CONCAT(scope, ',') AS transient_backoff_scopes
                FROM api_key_transient_backoffs
                WHERE cooldown_until > strftime('%s', 'now')
                  AND reason_code = 'upstream_unknown_403'
                GROUP BY key_id
            ) AS tb
            ON tb.key_id = ak.id
            WHERE ak.deleted_at IS NULL
        "#
    }

    pub(crate) fn api_key_metrics_query(include_quarantine_detail: bool) -> String {
        let quarantine_detail_sql = if include_quarantine_detail {
            "aq.reason_detail AS quarantine_reason_detail,"
        } else {
            "NULL AS quarantine_reason_detail,"
        };
        format!(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.registration_ip,
                ak.registration_region,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                {quarantine_detail_sql}
                aq.created_at AS quarantine_created_at,
                tb.transient_backoff_cooldown_until,
                tb.transient_backoff_retry_after_secs,
                tb.transient_backoff_scopes,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            {}
            "#,
            Self::api_key_metrics_from_clause(),
        )
    }

    pub(crate) fn map_api_key_metrics_row(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<ApiKeyMetrics, sqlx::Error> {
        let id: String = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let group_name: Option<String> = row.try_get("group_name")?;
        let registration_ip: Option<String> = row.try_get("registration_ip")?;
        let registration_region: Option<String> = row.try_get("registration_region")?;
        let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
        let last_used_at: i64 = row.try_get("last_used_at")?;
        let deleted_at: Option<i64> = row.try_get("deleted_at")?;
        let quota_limit: Option<i64> = row.try_get("quota_limit")?;
        let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
        let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
        let total_requests: i64 = row.try_get("total_requests")?;
        let success_count: i64 = row.try_get("success_count")?;
        let error_count: i64 = row.try_get("error_count")?;
        let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
        let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
        let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
        let quarantine_reason_summary: Option<String> = row.try_get("quarantine_reason_summary")?;
        let quarantine_reason_detail: Option<String> = row.try_get("quarantine_reason_detail")?;
        let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;
        let transient_backoff_cooldown_until: Option<i64> =
            row.try_get("transient_backoff_cooldown_until")?;
        let transient_backoff_retry_after_secs: Option<i64> =
            row.try_get("transient_backoff_retry_after_secs")?;
        let transient_backoff_scopes: Option<String> = row.try_get("transient_backoff_scopes")?;
        let is_temporary_isolated = status == STATUS_ACTIVE
            && quarantine_source.is_none()
            && transient_backoff_cooldown_until.is_some();

        Ok(ApiKeyMetrics {
            id,
            status,
            group_name: normalize_optional_api_key_field(group_name),
            registration_ip: normalize_optional_api_key_field(registration_ip),
            registration_region: normalize_optional_api_key_field(registration_region),
            status_changed_at: status_changed_at.and_then(normalize_timestamp),
            last_used_at: normalize_timestamp(last_used_at),
            deleted_at: deleted_at.and_then(normalize_timestamp),
            quota_limit,
            quota_remaining,
            quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                source,
                reason_code: quarantine_reason_code.unwrap_or_default(),
                reason_summary: quarantine_reason_summary.unwrap_or_default(),
                reason_detail: quarantine_reason_detail.unwrap_or_default(),
                created_at: quarantine_created_at.unwrap_or_default(),
            }),
            transient_backoff: is_temporary_isolated.then(|| ApiKeyTransientBackoff {
                reason_code: FAILURE_KIND_UPSTREAM_UNKNOWN_403.to_string(),
                cooldown_until: transient_backoff_cooldown_until.unwrap_or_default(),
                retry_after_secs: transient_backoff_retry_after_secs.unwrap_or_default(),
                scopes: transient_backoff_scopes
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|scope| !scope.is_empty())
                    .map(str::to_string)
                    .collect(),
            }),
        })
    }

    pub(crate) fn normalize_api_key_groups(groups: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for group in groups {
            let value = group.trim().to_string();
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_regions(regions: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for region in regions {
            let value = region.trim().to_string();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_statuses(statuses: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for status in statuses {
            let value = status.trim().to_ascii_lowercase();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn push_api_key_group_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        groups: &'a [String],
    ) {
        if groups.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if group.is_empty() {
                builder.push("(TRIM(COALESCE(ak.group_name, '')) = '')");
            } else {
                builder
                    .push("(TRIM(COALESCE(ak.group_name, '')) = ")
                    .push_bind(group)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_status_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        statuses: &'a [String],
    ) {
        if statuses.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, status) in statuses.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if status == "quarantined" {
                builder.push("(aq.key_id IS NOT NULL)");
            } else if status == "temporary_isolated" {
                builder.push(
                    "(aq.key_id IS NULL AND ak.status = 'active' AND tb.key_id IS NOT NULL)",
                );
            } else if status == STATUS_ACTIVE {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status)
                    .push(" AND tb.key_id IS NULL)");
            } else {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_registration_ip_filter<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        registration_ip: Option<&'a str>,
    ) {
        let Some(registration_ip) = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        builder
            .push(" AND TRIM(COALESCE(ak.registration_ip, '')) = ")
            .push_bind(registration_ip);
    }

    pub(crate) fn push_api_key_region_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        regions: &'a [String],
    ) {
        if regions.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, region) in regions.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            builder
                .push("(TRIM(COALESCE(ak.registration_region, '')) = ")
                .push_bind(region)
                .push(")");
        }
        builder.push(")");
    }

}
