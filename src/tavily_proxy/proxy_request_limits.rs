impl TavilyProxy {
    pub fn current_request_rate_limit(&self) -> i64 {
        self.token_request_limit.current_request_limit()
    }

    pub fn default_request_rate_verdict(
        &self,
        scope: RequestRateScope,
    ) -> TokenHourlyRequestVerdict {
        TokenHourlyRequestVerdict::new(
            0,
            self.current_request_rate_limit(),
            request_rate_limit_window_minutes(),
            scope,
            0,
        )
    }

    pub fn default_request_rate_view(&self, scope: RequestRateScope) -> RequestRateView {
        self.default_request_rate_verdict(scope).request_rate()
    }

    /// Check and update the hourly *raw request* usage for a token.
    /// This limiter counts every authenticated request (regardless of MCP method)
    /// within the last rolling hour and enforces `TOKEN_HOURLY_REQUEST_LIMIT`.
    pub async fn check_token_hourly_requests(
        &self,
        token_id: &str,
    ) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        self.token_request_limit.check(token_id).await
    }

    /// Read-only snapshot of hourly raw request usage for a set of tokens.
    /// Used by dashboards / leaderboards; does not increment counters.
    pub async fn token_hourly_any_snapshot(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        self.token_request_limit.snapshot_many(token_ids).await
    }

    #[cfg(test)]
    pub(crate) async fn debug_token_request_limiter_subject_count(&self) -> usize {
        self.token_request_limit.debug_memory_subject_count().await
    }

    #[cfg(test)]
    pub(crate) async fn debug_prune_idle_token_request_subjects_at(&self, now_ts: i64) {
        self.token_request_limit
            .debug_prune_idle_subjects_at(now_ts)
            .await;
    }

    /// Read-only snapshot of current token quota usage (hour / day / month).
    pub async fn token_quota_snapshot(
        &self,
        token_id: &str,
    ) -> Result<Option<TokenQuotaVerdict>, ProxyError> {
        let now = Utc::now();
        let verdict = self.token_quota.snapshot_for_token(token_id, now).await?;
        Ok(Some(verdict))
    }

    /// Token logs (page-based pagination)
    #[allow(clippy::too_many_arguments)]
    pub async fn token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<TokenLogsPage, ProxyError> {
        self.key_store
            .fetch_token_logs_page(
                token_id,
                page,
                per_page,
                since,
                until,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                key_id,
                operational_class,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn token_logs_list(
        &self,
        token_id: &str,
        page_size: i64,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
    ) -> Result<TokenLogsCursorPage, ProxyError> {
        self.key_store
            .fetch_token_logs_cursor_page(
                token_id,
                page_size,
                since,
                until,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                key_id,
                operational_class,
                cursor,
                direction,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn token_logs_catalog(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<RequestLogsCatalog, ProxyError> {
        self.key_store
            .fetch_token_logs_catalog(
                token_id,
                since,
                until,
                TokenLogsCatalogFilters {
                    request_kinds,
                    result_status,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    key_id,
                    operational_class,
                },
            )
            .await
    }

    pub async fn token_request_log_bodies(
        &self,
        token_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store
            .fetch_token_log_bodies(token_id, log_id)
            .await
    }

    pub async fn token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        self.key_store
            .fetch_token_log_request_kind_options(
                token_id,
                since,
                until,
                TokenLogsCatalogFilters {
                    request_kinds: &[],
                    result_status: None,
                    key_effect_code: None,
                    binding_effect_code: None,
                    selection_effect_code: None,
                    key_id: None,
                    operational_class: None,
                },
            )
            .await
    }

    /// Hourly breakdown for recent N hours (success + non-success aggregated as error).
    pub async fn token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        self.key_store
            .fetch_token_hourly_breakdown(token_id, hours)
            .await
    }

    /// Generic usage series for arbitrary window and granularity.
    pub async fn token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        self.key_store
            .fetch_token_usage_series(token_id, since, until, bucket_secs)
            .await
    }

    /// 根据 ID 获取真实 API key，仅供管理员调用。
    pub async fn get_api_key_secret(&self, key_id: &str) -> Result<Option<String>, ProxyError> {
        self.key_store.fetch_api_key_secret(key_id).await
    }

    /// Admin: add or undelete an API key. Returns the key ID.
    pub async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.key_store.add_or_undelete_key(api_key).await
    }

    /// Admin: add or undelete an API key and optionally assign it to a group.
    pub async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .add_or_undelete_key_in_group(api_key, group)
            .await
    }

    /// Admin: add/undelete an API key and return the upsert status.
    pub async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status(api_key)
            .await
    }

    /// Admin: add/undelete an API key in the provided group and return the upsert status.
    pub async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status_in_group(api_key, group)
            .await
    }

    /// Admin: add/undelete an API key in the provided group and refresh registration metadata
    /// when the caller provides a new registration IP.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key,
                group,
                registration_ip,
                registration_region,
                None,
                false,
            )
            .await
    }

    /// Admin: add/undelete an API key, then bind it to the most relevant forward proxy node
    /// based on registration IP/region before persisting the affinity.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        geo_origin: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            api_key,
            group,
            registration_ip,
            registration_region,
            geo_origin,
            None,
        )
        .await
    }

    /// Admin: add/undelete an API key and persist the caller-selected proxy node when provided.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        geo_origin: &str,
        preferred_primary_proxy_key: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let has_fresh_registration_metadata =
            registration_ip.is_some() || registration_region.is_some();
        let is_hint_only_affinity =
            !has_fresh_registration_metadata && preferred_primary_proxy_key.is_some();
        let proxy_affinity = if has_fresh_registration_metadata {
            Some(
                self.select_proxy_affinity_for_registration_with_hint(
                    api_key,
                    geo_origin,
                    registration_ip,
                    registration_region,
                    preferred_primary_proxy_key,
                )
                .await?,
            )
        } else if let Some(preferred_primary_proxy_key) = preferred_primary_proxy_key {
            Some(
                self.select_proxy_affinity_for_hint_only(
                    api_key,
                    geo_origin,
                    preferred_primary_proxy_key,
                )
                .await?,
            )
        } else {
            None
        };
        let result = self
            .key_store
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key,
                group,
                registration_ip,
                registration_region,
                proxy_affinity.as_ref(),
                is_hint_only_affinity,
            )
            .await?;
        self.remove_proxy_affinity_record_from_cache(&result.0)
            .await;
        Ok(result)
    }

    /// Admin: soft delete a key by ID.
    pub async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.soft_delete_key_by_id(key_id).await
    }

    /// Admin: disable a key by ID.
    pub async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.disable_key_by_id(key_id).await
    }

    /// Admin: enable a key by ID (from disabled/exhausted -> active).
    pub async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.enable_key_by_id(key_id).await
    }

    /// Admin: clear the active quarantine record for a key.
    pub async fn clear_key_quarantine_by_id(&self, key_id: &str) -> Result<bool, ProxyError> {
        self.clear_key_quarantine_by_id_with_actor(key_id, MaintenanceActor::default())
            .await
    }

    /// Admin: clear the active quarantine record for a key and append an audit record when changed.
    pub async fn clear_key_quarantine_by_id_with_actor(
        &self,
        key_id: &str,
        actor: MaintenanceActor,
    ) -> Result<bool, ProxyError> {
        let before = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let changed = self.key_store.clear_key_quarantine_by_id(key_id).await?;
        if changed {
            let after = self.key_store.fetch_key_state_snapshot(key_id).await?;
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.to_string(),
                    source: MAINTENANCE_SOURCE_ADMIN.to_string(),
                    operation_code: MAINTENANCE_OP_MANUAL_CLEAR_QUARANTINE.to_string(),
                    operation_summary: "管理员手动解除隔离".to_string(),
                    reason_code: None,
                    reason_summary: Some("管理员解除当前 quarantine".to_string()),
                    reason_detail: None,
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: actor.auth_token_id,
                    actor_user_id: actor.actor_user_id,
                    actor_display_name: actor.actor_display_name,
                    status_before: before.status,
                    status_after: after.status,
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at: Utc::now().timestamp(),
                })
                .await?;
        }
        Ok(changed)
    }

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
    }

    /// Admin dashboard period summary windows based on server-local day/month boundaries.
    pub async fn summary_windows(&self) -> Result<SummaryWindows, ProxyError> {
        const SUMMARY_WINDOWS_CACHE_TTL: Duration = Duration::from_secs(0);

        loop {
            let waiter = {
                let mut cache = self.summary_windows_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && cached.generated_at.elapsed() < SUMMARY_WINDOWS_CACHE_TTL
                {
                    return Ok(cached.value.clone());
                }
                if cache.loading {
                    Some(cache.notify.clone().notified_owned())
                } else {
                    cache.loading = true;
                    None
                }
            };

            if let Some(waiter) = waiter {
                waiter.await;
                continue;
            }

            let mut load_guard = SummaryWindowsLoadGuard::new(self.summary_windows_cache.clone());
            let summary = self.summary_windows_at(Local::now()).await;
            let mut cache = self.summary_windows_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = summary.as_ref() {
                cache.cached = Some(CachedSummaryWindows {
                    generated_at: Instant::now(),
                    value: value.clone(),
                });
            }
            cache.notify.notify_waiters();
            load_guard.disarm();
            return summary;
        }
    }

    pub async fn dashboard_hourly_request_window(
        &self,
    ) -> Result<DashboardHourlyRequestWindow, ProxyError> {
        const DASHBOARD_HOURLY_REQUEST_WINDOW_CACHE_TTL: Duration = Duration::from_secs(0);

        loop {
            let waiter = {
                let mut cache = self.dashboard_hourly_request_window_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && cached.generated_at.elapsed() < DASHBOARD_HOURLY_REQUEST_WINDOW_CACHE_TTL
                {
                    return Ok(cached.value.clone());
                }
                if cache.loading {
                    Some(cache.notify.clone().notified_owned())
                } else {
                    cache.loading = true;
                    None
                }
            };

            if let Some(waiter) = waiter {
                waiter.await;
                continue;
            }

            let mut load_guard = DashboardHourlyRequestWindowLoadGuard::new(
                self.dashboard_hourly_request_window_cache.clone(),
            );
            let window = self.dashboard_hourly_request_window_at(Utc::now()).await;
            let mut cache = self.dashboard_hourly_request_window_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = window.as_ref() {
                cache.cached = Some(CachedDashboardHourlyRequestWindow {
                    generated_at: Instant::now(),
                    value: value.clone(),
                });
            }
            cache.notify.notify_waiters();
            load_guard.disarm();
            return window;
        }
    }

    pub(crate) async fn dashboard_hourly_request_window_at(
        &self,
        now: chrono::DateTime<Utc>,
    ) -> Result<DashboardHourlyRequestWindow, ProxyError> {
        const DASHBOARD_HOURLY_BUCKET_SECS: i64 = 3600;
        const DASHBOARD_HOURLY_VISIBLE_BUCKETS: i64 = 25;
        const DASHBOARD_HOURLY_RETAINED_BUCKETS: i64 = 49;

        let current_hour_start = start_of_local_hour_utc_ts(now.with_timezone(&Local));

        self.key_store
            .fetch_dashboard_hourly_request_window(
                current_hour_start,
                DASHBOARD_HOURLY_BUCKET_SECS,
                DASHBOARD_HOURLY_VISIBLE_BUCKETS,
                DASHBOARD_HOURLY_RETAINED_BUCKETS,
            )
            .await
    }

    pub(crate) async fn summary_windows_at(
        &self,
        now: chrono::DateTime<Local>,
    ) -> Result<SummaryWindows, ProxyError> {
        let today_start = start_of_local_day_utc_ts(now);
        let yesterday_start = previous_local_day_start_utc_ts(now);
        let month_start = start_of_local_month_utc_ts(now);
        let month_quota_charge_start = start_of_month(now.with_timezone(&Utc)).timestamp();
        let today_end = now.with_timezone(&Utc).timestamp().saturating_add(1);
        let yesterday_same_time_end = previous_local_same_time_utc_ts(now).saturating_add(1);

        self.key_store
            .fetch_summary_windows(
                today_start,
                today_end,
                yesterday_start,
                yesterday_same_time_end,
                month_start,
                month_quota_charge_start,
            )
            .await
    }

    /// Public metrics: successful requests today and this month.
    pub async fn success_breakdown(
        &self,
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let resolved_daily_window =
            daily_window.unwrap_or_else(|| server_local_day_window_utc(now.with_timezone(&Local)));
        self.key_store
            .fetch_success_breakdown(
                month_start,
                resolved_daily_window.start,
                resolved_daily_window.end,
            )
            .await
    }

    /// Token-scoped success/failure breakdown.
    pub async fn token_success_breakdown(
        &self,
        token_id: &str,
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let resolved_daily_window =
            daily_window.unwrap_or_else(|| server_local_day_window_utc(now.with_timezone(&Local)));
        self.key_store
            .fetch_token_success_failure(
                token_id,
                month_start,
                resolved_daily_window.start,
                resolved_daily_window.end,
            )
            .await
    }

    pub(crate) fn sanitize_headers(&self, headers: &HeaderMap, path: &str) -> SanitizedHeaders {
        if path.starts_with("/mcp") {
            sanitize_mcp_headers_inner(headers)
        } else {
            sanitize_headers_inner(headers, &self.upstream, &self.upstream_origin)
        }
    }

    pub async fn find_user_id_by_token(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        self.key_store.find_user_id_by_token(token_id).await
    }

    pub async fn get_active_mcp_session(
        &self,
        proxy_session_id: &str,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        self.key_store
            .get_active_mcp_session(proxy_session_id, Utc::now().timestamp())
            .await
    }

    pub async fn token_has_active_mcp_session(&self, token_id: &str) -> Result<bool, ProxyError> {
        self.key_store
            .has_active_mcp_sessions_for_token(token_id, Utc::now().timestamp())
            .await
    }

    pub async fn create_mcp_session(
        &self,
        upstream_session_id: &str,
        upstream_key_id: &str,
        auth_token_id: Option<&str>,
        user_id: Option<&str>,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        let now = Utc::now().timestamp();
        let proxy_session_id = nanoid!(24);
        self.key_store
            .create_or_replace_mcp_session(&McpSessionBinding {
                proxy_session_id: proxy_session_id.clone(),
                upstream_session_id: Some(upstream_session_id.to_string()),
                upstream_key_id: Some(upstream_key_id.to_string()),
                auth_token_id: auth_token_id.map(str::to_string),
                user_id: user_id.map(str::to_string),
                protocol_version: protocol_version.map(str::to_string),
                last_event_id: last_event_id.map(str::to_string),
                gateway_mode: MCP_GATEWAY_MODE_UPSTREAM.to_string(),
                experiment_variant: MCP_EXPERIMENT_VARIANT_CONTROL.to_string(),
                ab_bucket: None,
                routing_subject_hash: None,
                fallback_reason: None,
                rate_limited_until: None,
                last_rate_limited_at: None,
                last_rate_limit_reason: None,
                created_at: now,
                updated_at: now,
                expires_at: now + MCP_SESSION_RETENTION_SECS,
                revoked_at: None,
                revoke_reason: None,
            })
            .await?;
        Ok(proxy_session_id)
    }

    pub async fn touch_mcp_session(
        &self,
        proxy_session_id: &str,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .touch_mcp_session(
                proxy_session_id,
                protocol_version,
                last_event_id,
                now,
                now + MCP_SESSION_RETENTION_SECS,
            )
            .await
    }

    pub async fn update_mcp_session_upstream_identity(
        &self,
        proxy_session_id: &str,
        upstream_session_id: &str,
        protocol_version: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .update_mcp_session_upstream_identity(
                proxy_session_id,
                upstream_session_id,
                protocol_version,
                now,
                now + MCP_SESSION_RETENTION_SECS,
            )
            .await
    }

    pub async fn mark_mcp_session_rate_limited(
        &self,
        proxy_session_id: &str,
        rate_limited_until: i64,
        reason: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .mark_mcp_session_rate_limited(
                proxy_session_id,
                rate_limited_until,
                reason,
                now,
                now + MCP_SESSION_RETENTION_SECS,
            )
            .await
    }

    pub async fn clear_mcp_session_rate_limit(
        &self,
        proxy_session_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .clear_mcp_session_rate_limit(proxy_session_id, now, now + MCP_SESSION_RETENTION_SECS)
            .await
    }

    pub async fn annotate_request_log_key_effect_if_none(
        &self,
        request_log_id: i64,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .set_request_log_key_effect_if_none(request_log_id, key_effect_code, key_effect_summary)
            .await
    }

    pub async fn revoke_mcp_session(
        &self,
        proxy_session_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .revoke_mcp_session(proxy_session_id, reason)
            .await
    }
}
