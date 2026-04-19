impl TavilyProxy {
    // ----- Public auth token management API -----

    /// Validate an access token in format `th-<id>-<secret>` and record usage.
    /// Returns true if valid and enabled.
    pub async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        self.key_store.validate_access_token(token).await
    }

    /// Admin: create a new access token with optional note.
    pub async fn create_access_token(
        &self,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.create_access_token(note).await
    }

    /// Admin: batch create access tokens with required group name.
    pub async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        self.key_store
            .create_access_tokens_batch(group, count, note)
            .await
    }

    /// Admin: list tokens for management.
    pub async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_access_tokens().await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    pub async fn list_dashboard_disabled_tokens(
        &self,
        limit: usize,
    ) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_disabled_access_tokens(limit).await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    pub async fn list_dashboard_disabled_token_ids(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store.list_disabled_access_token_ids(limit).await
    }

    /// Admin: list tokens paginated.
    pub async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let (mut tokens, total) = self
            .key_store
            .list_access_tokens_paged(page, per_page)
            .await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok((tokens, total))
    }

    pub(crate) async fn populate_token_quota(
        &self,
        tokens: &mut [AuthToken],
    ) -> Result<(), ProxyError> {
        if tokens.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
        let verdicts = self.token_quota.snapshot_many(&ids).await?;
        let token_bindings = self.key_store.list_user_bindings_for_tokens(&ids).await?;
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % 60);
        let local_now = now.with_timezone(&Local);
        let hour_window_start = minute_bucket - 59 * 60;
        let day_window_start = start_of_local_day_utc_ts(local_now);
        let day_window_end = next_local_day_start_utc_ts(day_window_start);
        let token_hourly_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();
        let account_hourly_oldest = self
            .key_store
            .earliest_account_usage_bucket_since_bulk(
                &user_ids,
                GRANULARITY_MINUTE,
                hour_window_start,
            )
            .await?;
        let month_start = start_of_month(now);
        let next_month_reset = start_of_next_month(month_start).timestamp();
        for token in tokens.iter_mut() {
            if let Some(verdict) = verdicts.get(&token.id) {
                let hourly_oldest = if let Some(user_id) = token_bindings.get(&token.id) {
                    account_hourly_oldest.get(user_id).copied()
                } else {
                    token_hourly_oldest.get(&token.id).copied()
                };
                token.quota_hourly_reset_at = if verdict.hourly_used > 0 {
                    hourly_oldest.map(|bucket| bucket + SECS_PER_HOUR)
                } else {
                    None
                };
                token.quota_daily_reset_at = if verdict.daily_used > 0 {
                    Some(day_window_end)
                } else {
                    None
                };
                token.quota_monthly_reset_at = if verdict.monthly_used > 0 {
                    Some(next_month_reset)
                } else {
                    None
                };
                token.quota = Some(verdict.clone());
            }
        }
        Ok(())
    }

    /// Admin: delete a token by id code.
    pub async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        self.key_store.delete_access_token(id).await
    }

    /// Admin: set token enabled/disabled.
    pub async fn set_access_token_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        self.key_store.set_access_token_enabled(id, enabled).await
    }

    /// Admin: update token note.
    pub async fn update_access_token_note(&self, id: &str, note: &str) -> Result<(), ProxyError> {
        self.key_store.update_access_token_note(id, note).await
    }

    /// Admin: get full token string for copy.
    pub async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store.get_access_token_secret(id).await
    }

    /// Admin: rotate token secret while keeping the same token id.
    /// Returns the new full token string (th-<id>-<secret>).
    pub async fn rotate_access_token_secret(
        &self,
        id: &str,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.rotate_access_token_secret(id).await
    }

    /// Create a one-time OAuth login state with TTL for CSRF/replay protection.
    pub async fn create_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            None,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash.
    pub async fn create_oauth_login_state_with_binding(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            binding_hash,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash and token id.
    pub async fn create_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .insert_oauth_login_state(provider, redirect_to, ttl_secs, binding_hash, bind_token_id)
            .await
    }

    /// Consume and invalidate an OAuth login state. Returns redirect target when valid.
    pub async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, None)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state bound to optional browser context hash.
    pub async fn consume_oauth_login_state_with_binding(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, binding_hash)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state and return all payload fields.
    pub async fn consume_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        self.key_store
            .consume_oauth_login_state(provider, state, binding_hash)
            .await
    }

    /// Upsert local user identity from third-party OAuth profile.
    pub async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.key_store.upsert_oauth_account(profile).await
    }

    /// Refresh third-party OAuth profile without mutating the user's real last_login_at timestamp.
    pub async fn refresh_oauth_account_profile(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.key_store.refresh_oauth_account_profile(profile).await
    }

    /// Refresh third-party OAuth profile and atomically rotate the persisted refresh token.
    pub async fn refresh_oauth_account_profile_with_refresh_token(
        &self,
        profile: &OAuthAccountProfile,
        refresh_token_ciphertext: &str,
        refresh_token_nonce: &str,
    ) -> Result<UserIdentity, ProxyError> {
        self.key_store
            .refresh_oauth_account_profile_with_refresh_token(
                profile,
                refresh_token_ciphertext,
                refresh_token_nonce,
            )
            .await
    }

    /// Check whether a third-party account already exists locally.
    pub async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .oauth_account_exists(provider, provider_user_id)
            .await
    }

    /// Persist encrypted refresh token material for an OAuth account.
    pub async fn set_oauth_account_refresh_token(
        &self,
        provider: &str,
        provider_user_id: &str,
        refresh_token_ciphertext: &str,
        refresh_token_nonce: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .set_oauth_account_refresh_token(
                provider,
                provider_user_id,
                refresh_token_ciphertext,
                refresh_token_nonce,
            )
            .await
    }

    /// Update whether a local user can authenticate.
    pub async fn set_user_active_status(
        &self,
        user_id: &str,
        active: bool,
    ) -> Result<(), ProxyError> {
        self.key_store.set_user_active_status(user_id, active).await
    }

    /// List OAuth accounts that can be refreshed offline.
    pub async fn list_oauth_accounts_with_refresh_token(
        &self,
        provider: &str,
    ) -> Result<Vec<OAuthAccountRefreshTokenRecord>, ProxyError> {
        self.key_store
            .list_oauth_accounts_with_refresh_token(provider)
            .await
    }

    /// Record a successful profile sync attempt for an OAuth account.
    pub async fn record_oauth_account_profile_sync_success(
        &self,
        provider: &str,
        provider_user_id: &str,
        attempted_at: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .record_oauth_account_profile_sync_success(provider, provider_user_id, attempted_at)
            .await
    }

    /// Record a failed profile sync attempt for an OAuth account.
    pub async fn record_oauth_account_profile_sync_failure(
        &self,
        provider: &str,
        provider_user_id: &str,
        attempted_at: i64,
        error: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .record_oauth_account_profile_sync_failure(
                provider,
                provider_user_id,
                attempted_at,
                error,
            )
            .await
    }

    /// Read whether first-time third-party registration is enabled.
    pub async fn allow_registration(&self) -> Result<bool, ProxyError> {
        self.key_store.allow_registration().await
    }

    /// Persist whether first-time third-party registration is enabled.
    pub async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.key_store.set_allow_registration(allow).await
    }

    /// Ensure one-to-one user token binding exists, creating a token only when missing.
    pub async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding(user_id, note)
            .await
    }

    /// Ensure binding with an optional preferred token id. Falls back to default behavior.
    pub async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding_with_preferred(user_id, note, preferred_token_id)
            .await
    }

    /// Fetch current user token by user_id. Does not auto-recreate when unavailable.
    pub async fn get_user_token(&self, user_id: &str) -> Result<UserTokenLookup, ProxyError> {
        self.key_store.get_user_token(user_id).await
    }

    /// List tokens bound to the specified user.
    pub async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_user_tokens(user_id).await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    /// Verify whether a token belongs to the specified user.
    pub async fn is_user_token_bound(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.is_user_token_bound(user_id, token_id).await
    }

    /// Get a token secret only when the token belongs to the specified user.
    pub async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store
            .get_user_token_secret(user_id, token_id)
            .await
    }

    /// User-level quota and usage summary for dashboard.
    pub async fn user_dashboard_summary(
        &self,
        user_id: &str,
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<UserDashboardSummary, ProxyError> {
        let mut summaries = self
            .user_dashboard_summaries_for_users(&[user_id.to_string()], daily_window)
            .await?;
        Ok(summaries.remove(user_id).unwrap_or(UserDashboardSummary {
            request_rate: self.default_request_rate_view(RequestRateScope::User),
            hourly_any_used: 0,
            hourly_any_limit: 0,
            quota_hourly_used: 0,
            quota_hourly_limit: 0,
            quota_daily_used: 0,
            quota_daily_limit: 0,
            quota_monthly_used: 0,
            quota_monthly_limit: 0,
            daily_success: 0,
            daily_failure: 0,
            monthly_success: 0,
            monthly_failure: 0,
            last_activity: None,
        }))
    }

    /// Admin: resolve dashboard summaries for many users without N+1 queries.
    pub async fn user_dashboard_summaries_for_users(
        &self,
        user_ids: &[String],
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<HashMap<String, UserDashboardSummary>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let server_daily_window = server_local_day_window_utc(now.with_timezone(&Local));
        let resolved_daily_window = daily_window.unwrap_or(server_daily_window);

        let mut deduped_user_ids = user_ids.to_vec();
        deduped_user_ids.sort_unstable();
        deduped_user_ids.dedup();

        let account_limits = self
            .key_store
            .resolve_account_quota_limits_bulk(&deduped_user_ids)
            .await?;
        let request_rate_totals = self
            .token_request_limit
            .snapshot_for_users(&deduped_user_ids)
            .await?;
        let minute_bucket = now.timestamp() - (now.timestamp() % SECS_PER_MINUTE);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let hourly_totals = self
            .key_store
            .sum_account_usage_buckets_bulk(
                &deduped_user_ids,
                GRANULARITY_MINUTE,
                hour_window_start,
            )
            .await?;
        let daily_totals = self
            .key_store
            .sum_account_usage_buckets_bulk(
                &deduped_user_ids,
                GRANULARITY_DAY,
                server_daily_window.start,
            )
            .await?;
        let legacy_daily_totals = self
            .key_store
            .sum_account_usage_buckets_bulk_between(
                &deduped_user_ids,
                GRANULARITY_HOUR,
                server_daily_window.start,
                server_daily_window.end,
            )
            .await?;
        let monthly_totals = self
            .key_store
            .fetch_account_monthly_counts(&deduped_user_ids, month_start)
            .await?;
        let log_metrics = self
            .key_store
            .fetch_user_log_metrics_bulk(
                &deduped_user_ids,
                resolved_daily_window.start,
                resolved_daily_window.end,
            )
            .await?;
        let default_limits = AccountQuotaLimits::zero_base();

        Ok(deduped_user_ids
            .into_iter()
            .map(|user_id| {
                let limits = account_limits
                    .get(&user_id)
                    .cloned()
                    .unwrap_or_else(|| default_limits.clone());
                let metrics = log_metrics.get(&user_id).cloned().unwrap_or_default();
                let request_rate =
                    request_rate_totals
                        .get(&user_id)
                        .cloned()
                        .unwrap_or_else(|| self.default_request_rate_verdict(RequestRateScope::User));
                (
                    user_id.clone(),
                    UserDashboardSummary {
                        request_rate: request_rate.request_rate(),
                        hourly_any_used: request_rate.hourly_used,
                        hourly_any_limit: request_rate.hourly_limit,
                        quota_hourly_used: hourly_totals.get(&user_id).copied().unwrap_or(0),
                        quota_hourly_limit: limits.hourly_limit,
                        quota_daily_used: daily_totals.get(&user_id).copied().unwrap_or(0)
                            + legacy_daily_totals.get(&user_id).copied().unwrap_or(0),
                        quota_daily_limit: limits.daily_limit,
                        quota_monthly_used: monthly_totals.get(&user_id).copied().unwrap_or(0),
                        quota_monthly_limit: limits.monthly_limit,
                        daily_success: metrics.daily_success,
                        daily_failure: metrics.daily_failure,
                        monthly_success: metrics.monthly_success,
                        monthly_failure: metrics.monthly_failure,
                        last_activity: metrics.last_activity,
                    },
                )
            })
            .collect())
    }

    pub async fn token_log_metrics_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenLogMetricsSummary>, ProxyError> {
        let daily_window = server_local_day_window_utc(Local::now());
        self.key_store
            .fetch_token_log_metrics_bulk(token_ids, daily_window.start, daily_window.end)
            .await
    }

    pub async fn list_api_key_binding_counts_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.key_store
            .list_api_key_binding_counts_for_users(user_ids)
            .await
    }

    async fn backfill_current_month_broken_key_subjects(&self) -> Result<(), ProxyError> {
        self.key_store
            .backfill_current_month_auto_subject_breakages()
            .await
    }

    pub async fn fetch_account_monthly_broken_limit(
        &self,
        user_id: &str,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .fetch_account_monthly_broken_limit(user_id)
            .await
    }

    pub async fn fetch_account_monthly_broken_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.key_store
            .fetch_account_monthly_broken_limits_bulk(user_ids)
            .await
    }

    pub async fn update_account_monthly_broken_limit(
        &self,
        user_id: &str,
        monthly_broken_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_monthly_broken_limit(user_id, monthly_broken_limit)
            .await
    }

    pub async fn fetch_monthly_broken_counts_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_counts_for_users(user_ids, start_of_month(Utc::now()).timestamp())
            .await
    }

    pub async fn fetch_monthly_broken_counts_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_counts_for_tokens(
                token_ids,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn list_monthly_broken_subjects_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashSet<String>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .list_monthly_broken_subjects_for_tokens(
                token_ids,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn fetch_user_monthly_broken_keys(
        &self,
        user_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedMonthlyBrokenKeys, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_keys_page(
                BROKEN_KEY_SUBJECT_USER,
                user_id,
                page,
                per_page,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn fetch_token_monthly_broken_keys(
        &self,
        token_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedMonthlyBrokenKeys, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_keys_page(
                BROKEN_KEY_SUBJECT_TOKEN,
                token_id,
                page,
                per_page,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    /// Admin: list users with pagination and optional fuzzy query.
    pub async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        self.key_store
            .list_admin_users_paged(page, per_page, query, tag_id)
            .await
    }

    /// Admin: list the full filtered user set prior to sorting and pagination.
    pub async fn list_admin_users_filtered(
        &self,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<Vec<AdminUserIdentity>, ProxyError> {
        self.key_store
            .list_admin_users_filtered(query, tag_id)
            .await
    }

    /// Admin: get a single user identity by id.
    pub async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        self.key_store.get_admin_user_identity(user_id).await
    }

    /// Admin: resolve token owners in bulk for management views.
    pub async fn get_admin_token_owners(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let token_bindings = self
            .key_store
            .list_user_bindings_for_tokens(token_ids)
            .await?;
        if token_bindings.is_empty() {
            return Ok(HashMap::new());
        }

        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();

        let user_map = self.key_store.get_admin_user_identities(&user_ids).await?;
        let mut owners = HashMap::with_capacity(token_bindings.len());
        for (token_id, user_id) in token_bindings {
            if let Some(identity) = user_map.get(&user_id) {
                owners.insert(token_id, identity.clone());
            }
        }
        Ok(owners)
    }

    /// Admin: upsert account quota limits for a user.
    pub async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_quota_limits(
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
            )
            .await
    }

    /// Admin: update only business quota limits and preserve deprecated raw request fields.
    pub async fn update_account_business_quota_limits(
        &self,
        user_id: &str,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_business_quota_limits(user_id, hourly_limit, daily_limit, monthly_limit)
            .await
    }

    /// Admin: list all user tag definitions.
    pub async fn list_user_tags(&self) -> Result<Vec<AdminUserTag>, ProxyError> {
        Ok(self
            .key_store
            .list_user_tags()
            .await?
            .into_iter()
            .map(|tag| to_admin_user_tag(&tag))
            .collect())
    }

    /// Admin: create a custom user tag.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<AdminUserTag, ProxyError> {
        self.key_store
            .create_user_tag(
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| to_admin_user_tag(&tag))
    }

    /// Admin: update an existing user tag definition.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_user_tag(
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
    ) -> Result<Option<AdminUserTag>, ProxyError> {
        self.key_store
            .update_user_tag(
                tag_id,
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| tag.map(|it| to_admin_user_tag(&it)))
    }

    /// Admin: delete a custom user tag definition.
    pub async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        self.key_store.delete_user_tag(tag_id).await
    }

    /// Admin: bind a custom tag to a user.
    pub async fn bind_user_tag_to_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.bind_user_tag_to_user(user_id, tag_id).await
    }

    /// Admin: unbind a tag from a user.
    pub async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .unbind_user_tag_from_user(user_id, tag_id)
            .await
    }

    /// Admin: list tag bindings for a set of users.
    pub async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<AdminUserTagBinding>>, ProxyError> {
        let bindings = self
            .key_store
            .list_user_tag_bindings_for_users(user_ids)
            .await?;
        Ok(bindings
            .into_iter()
            .map(|(user_id, items)| {
                (
                    user_id,
                    items
                        .into_iter()
                        .map(|binding| to_admin_user_tag_binding(&binding))
                        .collect(),
                )
            })
            .collect())
    }

    /// Admin: resolve base/effective quota and breakdown for a user.
    pub async fn get_admin_user_quota_details(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserQuotaDetails>, ProxyError> {
        let Some(_) = self.key_store.get_admin_user_identity(user_id).await? else {
            return Ok(None);
        };
        let resolution = self
            .key_store
            .resolve_account_quota_resolution(user_id)
            .await?;
        Ok(Some(AdminUserQuotaDetails {
            base: to_admin_quota_limit_set(&resolution.base),
            effective: to_admin_quota_limit_set(&resolution.effective),
            breakdown: resolution
                .breakdown
                .iter()
                .map(to_admin_quota_breakdown_entry)
                .collect(),
            tags: resolution
                .tags
                .iter()
                .map(to_admin_user_tag_binding)
                .collect(),
        }))
    }

    /// Create persisted user session.
    pub async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        self.key_store
            .create_user_session(user, session_max_age_secs)
            .await
    }

    /// Lookup valid user session from cookie token.
    pub async fn get_user_session(&self, token: &str) -> Result<Option<UserSession>, ProxyError> {
        self.key_store.get_user_session(token).await
    }

    /// Revoke persisted user session token.
    pub async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        self.key_store.revoke_user_session(token).await
    }

}
