impl TavilyProxy {
    pub(crate) async fn dashboard_quota_charge_snapshot(
        &self,
        bounds: SummaryWindowBounds,
        stale_key_count: i64,
    ) -> Result<DashboardQuotaChargeSnapshot, ProxyError> {
        let token = self
            .key_store
            .fetch_dashboard_quota_charge_token(
                stale_key_count,
                bounds.month_quota_charge_start,
                bounds.today_end,
            )
            .await?;

        loop {
            let waiter = {
                let mut cache = self.dashboard_quota_charge_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && cached.token == token
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

            let mut load_guard =
                DashboardQuotaChargeLoadGuard::new(self.dashboard_quota_charge_cache.clone());
            let rebuild_started = Instant::now();
            let snapshot = self.key_store.fetch_dashboard_quota_charge_snapshot(bounds).await;
            crate::emit_perf_log(
                crate::DbLogStatus::Info,
                "admin_read",
                "dashboard_overview_phase",
                rebuild_started.elapsed(),
                crate::PerfLogScope {
                    route: Some("/api/dashboard/overview"),
                    scope: Some("dashboard"),
                    phase: Some("quota_charge_rebuild"),
                    degraded: Some(if snapshot.is_ok() { "ok" } else { "error" }),
                    ..Default::default()
                },
            );
            let mut cache = self.dashboard_quota_charge_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = snapshot.as_ref() {
                cache.cached = Some(CachedDashboardQuotaChargeSnapshot {
                    token,
                    value: value.clone(),
                });
            }
            cache.notify.notify_waiters();
            load_guard.disarm();
            return snapshot;
        }
    }

    pub async fn dashboard_recent_alerts_summary(
        &self,
        window_hours: i64,
    ) -> Result<RecentAlertsSummary, ProxyError> {
        let token = self
            .key_store
            .fetch_recent_alerts_summary_token(window_hours)
            .await?;

        loop {
            let waiter = {
                let mut cache = self.dashboard_recent_alerts_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && cached.token == token
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

            let mut load_guard =
                DashboardRecentAlertsLoadGuard::new(self.dashboard_recent_alerts_cache.clone());
            let rebuild_started = Instant::now();
            let summary = self.key_store.fetch_recent_alerts_summary(window_hours).await;
            crate::emit_perf_log(
                crate::DbLogStatus::Info,
                "admin_read",
                "dashboard_overview_phase",
                rebuild_started.elapsed(),
                crate::PerfLogScope {
                    route: Some("/api/dashboard/overview"),
                    scope: Some("dashboard"),
                    phase: Some("recent_alerts_rebuild"),
                    degraded: Some(if summary.is_ok() { "ok" } else { "last_good_or_error" }),
                    ..Default::default()
                },
            );
            let mut cache = self.dashboard_recent_alerts_cache.lock().await;
            cache.loading = false;
            match summary {
                Ok(mut value) => {
                    value.coverage = "ok".to_string();
                    value.stale = false;
                    value.error = None;
                    cache.cached = Some(CachedDashboardRecentAlertsSummary {
                        token,
                        value: value.clone(),
                    });
                    cache.notify.notify_waiters();
                    load_guard.disarm();
                    return Ok(value);
                }
                Err(err) => {
                    if let Some(cached) = cache.cached.as_ref() {
                        let mut stale = cached.value.clone();
                        stale.coverage = "last_good".to_string();
                        stale.stale = true;
                        stale.error = Some(err.to_string());
                        cache.notify.notify_waiters();
                        load_guard.disarm();
                        return Ok(stale);
                    }
                    cache.notify.notify_waiters();
                    load_guard.disarm();
                    return Err(err);
                }
            }
        }
    }

    pub async fn dashboard_quota_charge_token(
        &self,
        stale_key_count: i64,
        month_quota_charge_start: i64,
        today_end: i64,
    ) -> Result<[i64; 6], ProxyError> {
        self.key_store
            .fetch_dashboard_quota_charge_token(
                stale_key_count,
                month_quota_charge_start,
                today_end,
            )
            .await
    }

    pub async fn dashboard_recent_alerts_token(
        &self,
        window_hours: i64,
    ) -> Result<[i64; 4], ProxyError> {
        self.key_store
            .fetch_recent_alerts_summary_token(window_hours)
            .await
    }

    pub async fn user_rankings_snapshot(&self) -> Result<UserRankingsSnapshot, ProxyError> {
        const USER_RANKINGS_CACHE_TTL: Duration = Duration::from_secs(10);
        const USER_RANKINGS_REFRESH_INTERVAL_SECS: i64 = 10;

        loop {
            let waiter = {
                let mut cache = self.user_rankings_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && self
                        .backend_time
                        .instant_now()
                        .saturating_duration_since(cached.generated_at)
                        < USER_RANKINGS_CACHE_TTL
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

            let mut load_guard = UserRankingsLoadGuard::new(self.user_rankings_cache.clone());
            let generated_at = self.backend_time.now_ts();
            let snapshot = self
                .key_store
                .fetch_user_rankings_snapshot(generated_at, USER_RANKINGS_REFRESH_INTERVAL_SECS)
                .await;
            let mut cache = self.user_rankings_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = snapshot.as_ref() {
                cache.cached = Some(CachedUserRankingsSnapshot {
                    generated_at: self.backend_time.instant_now(),
                    value: value.clone(),
                });
            }
            cache.notify.notify_waiters();
            load_guard.disarm();
            return snapshot;
        }
    }

    pub async fn analysis_pressure_snapshot(&self) -> Result<AnalysisPressureSnapshot, ProxyError> {
        const ANALYSIS_PRESSURE_CACHE_TTL: Duration = Duration::from_secs(10);

        loop {
            let waiter = {
                let mut cache = self.analysis_pressure_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && self
                        .backend_time
                        .instant_now()
                        .saturating_duration_since(cached.generated_at)
                        < ANALYSIS_PRESSURE_CACHE_TTL
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

            let mut load_guard = AnalysisPressureLoadGuard::new(self.analysis_pressure_cache.clone());
            let generated_at = self.backend_time.now_ts();
            let snapshot = self.analysis_pressure_snapshot_uncached(generated_at).await;
            let mut cache = self.analysis_pressure_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = snapshot.as_ref() {
                cache.cached = Some(CachedAnalysisPressureSnapshot {
                    generated_at: self.backend_time.instant_now(),
                    value: value.clone(),
                });
            }
            cache.notify.notify_waiters();
            load_guard.disarm();
            return snapshot;
        }
    }

    async fn analysis_pressure_snapshot_uncached(
        &self,
        generated_at: i64,
    ) -> Result<AnalysisPressureSnapshot, ProxyError> {
        const PRESSURE_WINDOW_SECONDS: i64 = SECS_PER_HOUR;
        const PRESSURE_24H_POINT_COUNT: usize = 288;
        const SERVER_7D_POINT_COUNT: usize = 168;
        const SERVER_7D_MA_WINDOWS: &[(AnalysisPressureMovingAverageKey, i64)] = &[
            (AnalysisPressureMovingAverageKey::Sma6h, 6),
            (AnalysisPressureMovingAverageKey::Sma24h, 24),
        ];

        let local_now = self.backend_time.local_now();
        let current_five_minute_bucket_start =
            generated_at - generated_at.rem_euclid(SECS_PER_FIVE_MINUTES);
        let current_hour_bucket_start = start_of_local_hour_utc_ts(local_now);
        let current_24h_start =
            current_five_minute_bucket_start - (PRESSURE_24H_POINT_COUNT as i64 - 1) * SECS_PER_FIVE_MINUTES;
        let previous_24h_start = current_24h_start - SECS_PER_DAY;
        let seven_day_start = current_hour_bucket_start - 167 * SECS_PER_HOUR;
        let pressure_warmup_bucket_count =
            rolling_pressure_warmup_bucket_count(SECS_PER_FIVE_MINUTES, PRESSURE_WINDOW_SECONDS);
        let pressure_warmup_seconds =
            pressure_warmup_bucket_count as i64 * SECS_PER_FIVE_MINUTES;

        let current_24h_buckets = self
            .key_store
            .fetch_server_pressure_points(
                "five_minute",
                current_24h_start - pressure_warmup_seconds,
                current_five_minute_bucket_start + SECS_PER_FIVE_MINUTES,
            )
            .await?;
        let previous_24h_buckets = self
            .key_store
            .fetch_server_pressure_points(
                "five_minute",
                previous_24h_start - pressure_warmup_seconds,
                previous_24h_start + PRESSURE_24H_POINT_COUNT as i64 * SECS_PER_FIVE_MINUTES,
            )
            .await?;
        let current_24h_bucket_slots = build_pressure_slot_series(
            current_24h_start - pressure_warmup_seconds,
            PRESSURE_24H_POINT_COUNT + pressure_warmup_bucket_count,
            SECS_PER_FIVE_MINUTES,
            &current_24h_buckets,
        );
        let previous_24h_bucket_slots = build_pressure_slot_series(
            previous_24h_start - pressure_warmup_seconds,
            PRESSURE_24H_POINT_COUNT + pressure_warmup_bucket_count,
            SECS_PER_FIVE_MINUTES,
            &previous_24h_buckets,
        );
        let current_24h_slots = trim_rolling_pressure_warmup(
            build_rolling_pressure_series(
                &current_24h_bucket_slots,
                SECS_PER_FIVE_MINUTES,
                PRESSURE_WINDOW_SECONDS,
            ),
            pressure_warmup_bucket_count,
        );
        let previous_24h_slots_raw = trim_rolling_pressure_warmup(
            build_rolling_pressure_series(
                &previous_24h_bucket_slots,
                SECS_PER_FIVE_MINUTES,
                PRESSURE_WINDOW_SECONDS,
            ),
            pressure_warmup_bucket_count,
        );
        let previous_pressure = previous_24h_slots_raw
            .last()
            .map(|point| point.pressure)
            .unwrap_or_default();
        let previous_24h_slots = previous_24h_slots_raw
            .into_iter()
            .map(|mut point| {
                point.display_bucket_start =
                    point.bucket_start.saturating_add(previous_to_current_display_shift_secs(
                        point.bucket_start,
                        local_now,
                    ));
                point
            })
            .collect::<Vec<_>>();

        let current_distribution = self.user_business_calls_1h_window.current_distribution().await;
        let mut active_rows = current_distribution
            .iter()
            .filter(|row| row.counts.total_count() > 0)
            .collect::<Vec<_>>();
        active_rows.sort_by(|left, right| {
            right
                .counts
                .total_count()
                .cmp(&left.counts.total_count())
                .then_with(|| left.user_id.cmp(&right.user_id))
        });
        let identities = self
            .get_admin_user_identities(
                &active_rows
                    .iter()
                    .map(|row| row.user_id.clone())
                    .collect::<Vec<_>>(),
            )
            .await?;
        let all_user_stats = self.get_admin_user_list_stats().await?;
        let rows = active_rows
            .into_iter()
            .map(|row| {
                let identity = identities.get(&row.user_id);
                AnalysisCurrentUserPressureRow {
                    user_id: row.user_id.clone(),
                    display_name: identity.and_then(|user| user.display_name.clone()),
                    username: identity.and_then(|user| user.username.clone()),
                    avatar_url: None,
                    pressure: row.counts.total_count(),
                    success_count: row.counts.success_count,
                    failure_count: row.counts.failure_count,
                }
            })
            .collect::<Vec<_>>();
        let mut row_pressures = rows.iter().map(|row| row.pressure).collect::<Vec<_>>();
        row_pressures.sort_unstable();
        let active_users = rows.len() as i64;
        let zero_pressure_users = all_user_stats.total_users.saturating_sub(active_users);
        let current_pressure = rows.iter().map(|row| row.pressure).sum::<i64>();

        let server_7d_warmup_hours = SERVER_7D_MA_WINDOWS
            .iter()
            .map(|(_key, window_hours)| window_hours.saturating_sub(1))
            .max()
            .unwrap_or_default();
        let server_7d_warmup_start = seven_day_start - server_7d_warmup_hours * SECS_PER_HOUR;
        let server_7d_bucket_points = build_pressure_slot_series(
            server_7d_warmup_start,
            SERVER_7D_POINT_COUNT + server_7d_warmup_hours as usize,
            SECS_PER_HOUR,
            &self
                .key_store
                .fetch_server_pressure_points(
                    "hour",
                    server_7d_warmup_start,
                    current_hour_bucket_start + SECS_PER_HOUR,
                )
                .await?,
        );
        let server_7d_points_raw =
            build_rolling_pressure_series(&server_7d_bucket_points, SECS_PER_HOUR, SECS_PER_HOUR);
        let server_7d_points = server_7d_points_raw
            .iter()
            .skip(server_7d_warmup_hours as usize)
            .cloned()
            .collect::<Vec<_>>();
        let server_7d_moving_averages = SERVER_7D_MA_WINDOWS
            .iter()
            .map(|(key, window_hours)| AnalysisPressureMovingAverageSeries {
                key: *key,
                window_hours: *window_hours,
                points: build_pressure_moving_average_series(
                    &server_7d_points_raw,
                    *window_hours as usize,
                    server_7d_warmup_hours as usize,
                ),
            })
            .collect::<Vec<_>>();

        Ok(AnalysisPressureSnapshot {
            generated_at,
            server_24h: AnalysisServerPressure24h {
                window_minutes: 60,
                bucket_seconds: SECS_PER_FIVE_MINUTES,
                current_peak: peak_pressure_point(&current_24h_slots),
                previous_peak: peak_pressure_point(&previous_24h_slots),
                current: current_24h_slots.clone(),
                previous: previous_24h_slots,
            },
            current_user_distribution: AnalysisCurrentUserPressureDistribution {
                window_minutes: 60,
                rows,
                summary: AnalysisCurrentUserPressureSummary {
                    active_users,
                    zero_pressure_users,
                    median: percentile_pressure(&row_pressures, 50),
                    p90: percentile_pressure(&row_pressures, 90),
                    peak: row_pressures.last().copied().unwrap_or_default(),
                    current_pressure,
                    vs_yesterday_delta: current_pressure - previous_pressure,
                },
            },
            server_7d: AnalysisServerPressure7d {
                bucket_seconds: SECS_PER_HOUR,
                moving_averages: server_7d_moving_averages,
                peak: peak_pressure_point(&server_7d_points),
                points: server_7d_points,
            },
        })
    }

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

    pub(crate) async fn user_request_rate_recent_timestamps(
        &self,
        user_id: &str,
    ) -> Vec<i64> {
        self.token_request_limit.recent_timestamps_for_user(user_id).await
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
        let now = self.backend_time.now_utc();
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

    /// Admin: return the submitted API keys that already exist and are not soft-deleted.
    pub async fn fetch_active_existing_api_keys(
        &self,
        api_keys: &[String],
    ) -> Result<HashSet<String>, ProxyError> {
        self.key_store.fetch_active_existing_api_keys(api_keys).await
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
                    created_at: self.backend_time.now_ts(),
                })
                .await?;
        }
        Ok(changed)
    }

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
    }

    pub async fn summary_without_flush(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary_without_flush().await
    }

    /// Admin dashboard period summary windows based on server-local day/month boundaries.
    pub async fn summary_windows(&self) -> Result<SummaryWindows, ProxyError> {
        const SUMMARY_WINDOWS_CACHE_TTL: Duration = Duration::from_secs(0);

        loop {
            let waiter = {
                let mut cache = self.summary_windows_cache.lock().await;
                if let Some(cached) = cache.cached.as_ref()
                    && self.backend_time.instant_now().saturating_duration_since(cached.generated_at)
                        < SUMMARY_WINDOWS_CACHE_TTL
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
            let summary = self.summary_windows_at(self.backend_time.local_now()).await;
            let mut cache = self.summary_windows_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = summary.as_ref() {
                cache.cached = Some(CachedSummaryWindows {
                    generated_at: self.backend_time.instant_now(),
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
                    && self
                        .backend_time
                        .instant_now()
                        .saturating_duration_since(cached.generated_at)
                        < DASHBOARD_HOURLY_REQUEST_WINDOW_CACHE_TTL
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
            let window = self
                .dashboard_hourly_request_window_at(self.backend_time.now_utc())
                .await;
            let mut cache = self.dashboard_hourly_request_window_cache.lock().await;
            cache.loading = false;
            if let Ok(value) = window.as_ref() {
                cache.cached = Some(CachedDashboardHourlyRequestWindow {
                    generated_at: self.backend_time.instant_now(),
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
        let month_period_end = crate::shift_local_month_start_utc_ts(month_start, 1);
        let previous_month_start = previous_local_month_start_utc_ts(now);
        let month_quota_charge_start = start_of_month(now.with_timezone(&Utc)).timestamp();
        let today_end = now.with_timezone(&Utc).timestamp().saturating_add(1);
        let today_period_end = next_local_day_start_utc_ts(today_start);
        let today_elapsed = today_end.saturating_sub(today_start);
        let yesterday_end = yesterday_start.saturating_add(today_elapsed);

        let bounds = SummaryWindowBounds {
            today_start,
            today_end,
            today_period_end,
            yesterday_start,
            yesterday_end,
            month_start,
            month_quota_charge_start,
            month_period_end,
            previous_month_start,
            previous_month_end: month_start,
        };
        let now_ts = today_end.saturating_sub(1);
        let hot_active_since = now_ts.saturating_sub(2 * 60 * 60);
        let hot_stale_before = now_ts.saturating_sub(15 * 60);
        let cold_stale_before = now_ts.saturating_sub(24 * 60 * 60);
        let stale_key_count = self
            .dashboard_stale_key_count(hot_active_since, hot_stale_before, cold_stale_before)
            .await?;
        let mut windows = self.key_store.fetch_summary_windows(bounds).await?;
        let quota_charge = self
            .dashboard_quota_charge_snapshot(bounds, stale_key_count)
            .await?;
        windows.today.quota_charge.upstream_actual_credits = quota_charge.today.upstream_actual_credits;
        windows.today.quota_charge.sampled_key_count = quota_charge.today.sampled_key_count;
        windows.today.quota_charge.stale_key_count = quota_charge.today.stale_key_count;
        windows.today.quota_charge.latest_sync_at = quota_charge.today.latest_sync_at;
        windows.yesterday.quota_charge.upstream_actual_credits =
            quota_charge.yesterday.upstream_actual_credits;
        windows.yesterday.quota_charge.sampled_key_count = quota_charge.yesterday.sampled_key_count;
        windows.yesterday.quota_charge.stale_key_count = quota_charge.yesterday.stale_key_count;
        windows.yesterday.quota_charge.latest_sync_at = quota_charge.yesterday.latest_sync_at;
        windows.month.quota_charge.upstream_actual_credits = quota_charge.month.upstream_actual_credits;
        windows.month.quota_charge.sampled_key_count = quota_charge.month.sampled_key_count;
        windows.month.quota_charge.stale_key_count = quota_charge.month.stale_key_count;
        windows.month.quota_charge.latest_sync_at = quota_charge.month.latest_sync_at;
        Ok(windows)
    }

    pub async fn dashboard_month_series(
        &self,
        summary_windows: &SummaryWindows,
    ) -> Result<DashboardMonthSeries, ProxyError> {
        self.key_store.fetch_dashboard_month_series(summary_windows).await
    }

    pub async fn latest_dashboard_quota_sync_sample_at(&self) -> Result<Option<i64>, ProxyError> {
        self.key_store
            .fetch_latest_dashboard_quota_sync_sample_at()
            .await
    }

    pub async fn dashboard_rollup_freshness_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 19], ProxyError> {
        self.key_store
            .fetch_dashboard_rollup_freshness_signature(range_start)
            .await
    }

    pub async fn dashboard_rollup_freshness_signature_without_flush(
        &self,
        range_start: i64,
    ) -> Result<[i64; 19], ProxyError> {
        self.key_store
            .fetch_dashboard_rollup_freshness_signature_without_flush(range_start)
            .await
    }

    pub async fn pending_dashboard_rollup_freshness_signature(&self) -> [i64; 10] {
        self.key_store
            .request_stats_coalescer
            .pending_dashboard_freshness_signature()
            .await
    }

    #[doc(hidden)]
    pub async fn debug_enqueue_dashboard_credit_rollups(&self, created_at: i64, credits: i64) {
        self.key_store
            .request_stats_coalescer
            .enqueue_dashboard_credit_rollups(created_at, credits)
            .await;
    }

    pub async fn dashboard_api_key_lifecycle_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 3], ProxyError> {
        self.key_store
            .fetch_dashboard_api_key_lifecycle_signature(range_start)
            .await
    }

    pub async fn dashboard_quarantine_lifecycle_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 3], ProxyError> {
        self.key_store
            .fetch_dashboard_quarantine_lifecycle_signature(range_start)
            .await
    }

    pub async fn dashboard_exhausted_lifecycle_signature(
        &self,
        range_start: i64,
        range_end: i64,
    ) -> Result<[i64; 3], ProxyError> {
        self.key_store
            .fetch_dashboard_exhausted_lifecycle_signature(range_start, range_end)
            .await
    }

    pub async fn dashboard_quota_sample_signature(
        &self,
        window_start: i64,
        window_end: i64,
    ) -> Result<[i64; 4], ProxyError> {
        self.key_store
            .fetch_dashboard_quota_sample_signature(window_start, window_end)
            .await
    }

    pub async fn dashboard_stale_key_count(
        &self,
        hot_active_since: i64,
        hot_stale_before: i64,
        cold_stale_before: i64,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .fetch_dashboard_stale_key_count(
                hot_active_since,
                hot_stale_before,
                cold_stale_before,
            )
            .await
    }

    /// Public metrics: successful requests today and this month.
    pub async fn success_breakdown(
        &self,
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let now = self.backend_time.now_utc();
        let month_start = start_of_month(now).timestamp();
        let resolved_daily_window =
            daily_window.unwrap_or_else(|| server_local_day_window_utc(now.with_timezone(&Local)));
        self.key_store
            .fetch_success_breakdown_from_dashboard_rollups(
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
        let now = self.backend_time.now_utc();
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
            .get_active_mcp_session(proxy_session_id, self.backend_time.now_ts())
            .await
    }

    pub async fn token_has_active_mcp_session(&self, token_id: &str) -> Result<bool, ProxyError> {
        self.key_store
            .has_active_mcp_sessions_for_token(token_id, self.backend_time.now_ts())
            .await
    }

    pub async fn token_has_active_non_rebalance_mcp_session(
        &self,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .has_active_non_rebalance_mcp_session_for_token(token_id, self.backend_time.now_ts())
            .await
    }

    pub async fn latest_active_mcp_session_for_token(
        &self,
        token_id: &str,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        self.key_store
            .get_latest_active_mcp_session_for_token(token_id, self.backend_time.now_ts())
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
        let now = self.backend_time.now_ts();
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
        let now = self.backend_time.now_ts();
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
        let now = self.backend_time.now_ts();
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
        let now = self.backend_time.now_ts();
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
        let now = self.backend_time.now_ts();
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

fn build_pressure_slot_series(
    start: i64,
    count: usize,
    bucket_seconds: i64,
    points: &[AnalysisPressurePoint],
) -> Vec<AnalysisPressurePoint> {
    let lookup = points
        .iter()
        .map(|point| (point.bucket_start, point.clone()))
        .collect::<HashMap<_, _>>();
    (0..count)
        .map(|index| {
            let bucket_start = start + index as i64 * bucket_seconds;
            lookup
                .get(&bucket_start)
                .cloned()
                .unwrap_or(AnalysisPressurePoint {
                    bucket_start,
                    display_bucket_start: bucket_start,
                    pressure: 0,
                    success_count: 0,
                    failure_count: 0,
                })
        })
        .collect()
}

fn build_rolling_pressure_series(
    bucket_points: &[AnalysisPressurePoint],
    bucket_seconds: i64,
    window_seconds: i64,
) -> Vec<AnalysisPressurePoint> {
    let max_buckets = rolling_pressure_bucket_count(bucket_seconds, window_seconds);
    let mut rolling_success = 0_i64;
    let mut rolling_failure = 0_i64;
    let mut recent = std::collections::VecDeque::<(i64, i64)>::new();

    bucket_points
        .iter()
        .map(|point| {
            rolling_success += point.success_count;
            rolling_failure += point.failure_count;
            recent.push_back((point.success_count, point.failure_count));
            while recent.len() > max_buckets {
                if let Some((success, failure)) = recent.pop_front() {
                    rolling_success -= success;
                    rolling_failure -= failure;
                }
            }

            AnalysisPressurePoint {
                bucket_start: point.bucket_start,
                display_bucket_start: point.display_bucket_start,
                pressure: rolling_success + rolling_failure,
                success_count: rolling_success,
                failure_count: rolling_failure,
            }
        })
        .collect()
}

fn rolling_pressure_bucket_count(bucket_seconds: i64, window_seconds: i64) -> usize {
    (window_seconds / bucket_seconds).max(1) as usize
}

fn rolling_pressure_warmup_bucket_count(bucket_seconds: i64, window_seconds: i64) -> usize {
    rolling_pressure_bucket_count(bucket_seconds, window_seconds).saturating_sub(1)
}

fn trim_rolling_pressure_warmup(
    points: Vec<AnalysisPressurePoint>,
    warmup_bucket_count: usize,
) -> Vec<AnalysisPressurePoint> {
    points.into_iter().skip(warmup_bucket_count).collect()
}

fn previous_to_current_display_shift_secs(
    previous_bucket_start: i64,
    fallback_now: chrono::DateTime<Local>,
) -> i64 {
    let Some(previous_utc) = chrono::Utc.timestamp_opt(previous_bucket_start, 0).single() else {
        return SECS_PER_DAY;
    };
    let previous_local = previous_utc.with_timezone(&Local);
    let current_date = previous_local
        .date_naive()
        .succ_opt()
        .unwrap_or_else(|| previous_local.date_naive());
    let naive = current_date.and_time(previous_local.time());
    local_naive_datetime_utc_ts(naive, fallback_now).saturating_sub(previous_bucket_start)
}

fn peak_pressure_point(points: &[AnalysisPressurePoint]) -> Option<AnalysisPressurePeak> {
    points
        .iter()
        .max_by(|left, right| {
            left.pressure
                .cmp(&right.pressure)
                .then_with(|| right.bucket_start.cmp(&left.bucket_start))
        })
        .map(|point| AnalysisPressurePeak {
            bucket_start: point.bucket_start,
            display_bucket_start: point.display_bucket_start,
            pressure: point.pressure,
        })
}

fn build_pressure_moving_average_series(
    points: &[AnalysisPressurePoint],
    window_size: usize,
    visible_skip_count: usize,
) -> Vec<AnalysisPressureMovingAveragePoint> {
    if window_size == 0 {
        return Vec::new();
    }

    let mut rolling_sum = 0_i64;
    let mut recent = std::collections::VecDeque::<i64>::new();
    let mut averaged_points = Vec::with_capacity(points.len().saturating_sub(visible_skip_count));

    for point in points {
        rolling_sum += point.pressure;
        recent.push_back(point.pressure);
        while recent.len() > window_size {
            if let Some(removed) = recent.pop_front() {
                rolling_sum -= removed;
            }
        }

        if recent.len() == window_size {
            averaged_points.push(AnalysisPressureMovingAveragePoint {
                bucket_start: point.bucket_start,
                display_bucket_start: point.display_bucket_start,
                value: rolling_sum / window_size as i64,
            });
        }
    }

    averaged_points
        .into_iter()
        .skip(visible_skip_count.saturating_sub(window_size.saturating_sub(1)))
        .collect()
}

fn percentile_pressure(values: &[i64], percentile: usize) -> i64 {
    if values.is_empty() {
        return 0;
    }
    let clamped = percentile.clamp(0, 100);
    let index = ((values.len() - 1) * clamped) / 100;
    values[index]
}
