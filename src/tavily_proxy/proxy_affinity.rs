impl TavilyProxy {
    pub async fn lock_mcp_session_init(
        &self,
        auth_token_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Option<McpSessionInitGuard>, ProxyError> {
        let Some(token_id) = auth_token_id else {
            return Ok(None);
        };

        let subject = Self::mcp_session_init_lock_subject(user_id, token_id);
        let lock = {
            let mut locks = self.mcp_session_init_locks.lock().await;
            if locks.len() > 1024 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(&subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(subject.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                &subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;
        Ok(Some(McpSessionInitGuard {
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        }))
    }

    pub async fn lock_mcp_session_request(
        &self,
        proxy_session_id: &str,
    ) -> Result<McpSessionRequestGuard, ProxyError> {
        let subject = Self::mcp_session_request_lock_subject(proxy_session_id);
        let lock = {
            let mut locks = self.mcp_session_request_locks.lock().await;
            if locks.len() > 4096 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(&subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(subject.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                &subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;
        Ok(McpSessionRequestGuard {
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    /// Serialize quota/billing work per effective quota subject across both the local process
    /// and any other instances sharing the same SQLite database.
    pub async fn lock_token_billing(
        &self,
        token_id: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let current_subject = self.billing_subject_for_token(token_id).await?;
        let mut subjects = self
            .key_store
            .list_pending_billing_subjects_for_token(token_id)
            .await?;
        subjects.push(current_subject.clone());
        subjects.sort();
        subjects.dedup();

        let mut current_guard: Option<TokenBillingGuard> = None;
        let mut extra_guards: Vec<TokenBillingGuard> = Vec::new();
        for subject in subjects {
            let guard = self.lock_billing_subject(&subject).await?;
            self.reconcile_pending_billing_for_subject(guard.billing_subject())
                .await?;
            if subject == current_subject {
                current_guard = Some(guard);
            } else {
                extra_guards.push(guard);
            }
        }
        drop(extra_guards);

        current_guard.ok_or_else(|| {
            ProxyError::Other(format!(
                "failed to acquire billing guard for current subject {current_subject}",
            ))
        })
    }

    pub(crate) async fn lock_research_key_usage(
        &self,
        key_id: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let subject = format!("research-key:{key_id}");
        let lock = {
            let mut locks = self.research_key_locks.lock().await;
            if locks.len() > 256 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(&subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(subject.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                &subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;

        Ok(TokenBillingGuard {
            billing_subject: subject,
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    async fn rebind_user_primary_affinity(
        &self,
        user_id: &str,
        old_key_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let lease = match self.key_store.acquire_active_key_excluding(old_key_id).await {
            Ok(lease) => lease,
            Err(ProxyError::NoAvailableKeys) => self.key_store.acquire_key().await?,
            Err(err) => return Err(err),
        };
        self.key_store
            .sync_user_primary_api_key_affinity(user_id, &lease.id)
            .await?;
        if let Some(old_key_id) = old_key_id
            && old_key_id != lease.id
        {
            self.key_store
                .revoke_mcp_sessions_for_user_key(user_id, old_key_id, "primary_api_key_rebound")
                .await?;
        }
        Ok(lease)
    }

    async fn rebind_token_primary_affinity(
        &self,
        token_id: &str,
        old_key_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let lease = match self.key_store.acquire_active_key_excluding(old_key_id).await {
            Ok(lease) => lease,
            Err(ProxyError::NoAvailableKeys) => self.key_store.acquire_key().await?,
            Err(err) => return Err(err),
        };
        self.key_store
            .set_token_primary_api_key_affinity(token_id, None, &lease.id)
            .await?;
        if let Some(old_key_id) = old_key_id
            && old_key_id != lease.id
        {
            self.key_store
                .revoke_mcp_sessions_for_token_key(token_id, old_key_id, "primary_api_key_rebound")
                .await?;
        }
        Ok(lease)
    }

    async fn rank_mcp_session_affinity_candidate_keys(
        &self,
        token_id: &str,
        user_id: Option<&str>,
        desired_count: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let mut candidates = self.key_store.list_mcp_session_candidate_key_ids().await?;
        if candidates.is_empty() {
            return Err(ProxyError::NoAvailableKeys);
        }

        let subject = Self::mcp_session_affinity_subject(user_id, token_id);
        candidates.sort_by(|left, right| {
            Self::mcp_session_affinity_score(&subject, right)
                .cmp(&Self::mcp_session_affinity_score(&subject, left))
                .then_with(|| left.cmp(right))
        });
        candidates.truncate(desired_count.clamp(1, candidates.len() as i64).max(1) as usize);
        Ok(candidates)
    }

    async fn build_mcp_session_init_candidates(
        &self,
        token_id: &str,
        user_id: Option<&str>,
        ranked: &[String],
        now: i64,
    ) -> Result<Vec<McpSessionInitCandidate>, ProxyError> {
        let cooldowns = self
            .key_store
            .list_active_api_key_transient_backoffs(ranked, MCP_SESSION_INIT_BACKOFF_SCOPE, now)
            .await?;
        let recent_rate_limited_counts = self
            .key_store
            .list_recent_rate_limited_request_counts_for_keys(
                ranked,
                now - MCP_SESSION_INIT_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let recent_counts = self
            .key_store
            .list_recent_billable_request_counts_for_keys(
                ranked,
                now - MCP_SESSION_INIT_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let active_counts = if let Some(user_id) = user_id {
            self.key_store
                .list_active_mcp_session_counts_for_user(user_id, ranked, now)
                .await?
        } else {
            self.key_store
                .list_active_mcp_session_counts_for_token(token_id, ranked, now)
                .await?
        };
        let last_used_at = self.key_store.list_api_key_last_used_at(ranked).await?;

        let mut candidates = ranked
            .iter()
            .enumerate()
            .map(|(stable_rank_index, key_id)| McpSessionInitCandidate {
                key_id: key_id.clone(),
                stable_rank_index,
                cooldown_until: cooldowns.get(key_id).map(|state| state.cooldown_until),
                recent_rate_limited_count: recent_rate_limited_counts
                    .get(key_id)
                    .copied()
                    .unwrap_or(0),
                recent_billable_request_count: recent_counts.get(key_id).copied().unwrap_or(0),
                active_session_count: active_counts.get(key_id).copied().unwrap_or(0),
                last_used_at: last_used_at.get(key_id).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>();
        Self::order_mcp_session_init_candidates(&mut candidates);
        Ok(candidates)
    }

    async fn maybe_arm_mcp_session_init_backoff(
        &self,
        key_id: &str,
        headers: &HeaderMap,
        analysis: &AttemptAnalysis,
    ) -> Result<bool, ProxyError> {
        if analysis.failure_kind.as_deref() != Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429) {
            return Ok(false);
        }

        let now = Utc::now().timestamp();
        let retry_after_secs = Self::mcp_session_init_retry_after_secs(headers, now);
        let cooldown_until = now + retry_after_secs;

        Ok(self
            .key_store
            .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                key_id,
                scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
                cooldown_until,
                retry_after_secs,
                reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                source_request_log_id: None,
                now,
            })
            .await?
            .is_some())
    }

    pub(crate) async fn acquire_key_for_mcp_session_init(
        &self,
        auth_token_id: Option<&str>,
    ) -> Result<McpSessionInitSelection, ProxyError> {
        let Some(token_id) = auth_token_id else {
            return Ok(McpSessionInitSelection {
                lease: self.key_store.acquire_key().await?,
                key_effect: KeyEffect::none(),
            });
        };

        let user_id = self.key_store.find_user_id_by_token(token_id).await?;
        let settings = self.key_store.get_system_settings().await?;
        let ranked = self
            .rank_mcp_session_affinity_candidate_keys(
                token_id,
                user_id.as_deref(),
                settings.mcp_session_affinity_key_count,
            )
            .await?;
        let now = Utc::now().timestamp();
        let ordered = self
            .build_mcp_session_init_candidates(token_id, user_id.as_deref(), &ranked, now)
            .await?;
        let preferred_key_id = ordered.first().map(|candidate| candidate.key_id.clone());
        let preferred_effect = Self::mcp_session_init_selection_effect(&ordered);

        for candidate in ordered {
            let key_id = candidate.key_id.clone();
            if let Some(lease) = self
                .key_store
                .try_acquire_affinity_specific_key(&key_id)
                .await?
            {
                let key_effect = if preferred_key_id.as_deref() == Some(key_id.as_str()) {
                    preferred_effect.clone()
                } else {
                    KeyEffect::none()
                };
                return Ok(McpSessionInitSelection { lease, key_effect });
            }
        }

        Err(ProxyError::NoAvailableKeys)
    }

    async fn resolve_http_project_affinity_context(
        &self,
        auth_token_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<Option<HttpProjectAffinityContext>, ProxyError> {
        let Some(project_id) = project_id.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let Some(token_id) = auth_token_id else {
            return Ok(None);
        };

        let user_id = self.key_store.find_user_id_by_token(token_id).await?;
        let owner_subject = match user_id.as_deref() {
            Some(user_id) => format!("user:{user_id}"),
            None => format!("token:{token_id}"),
        };
        let project_id_hash = Self::sha256_hex(project_id);
        Ok(Some(HttpProjectAffinityContext {
            affinity_subject: Self::http_project_affinity_subject(&owner_subject, &project_id_hash),
            owner_subject,
            project_id_hash,
        }))
    }

    async fn rank_http_project_affinity_candidate_keys(
        &self,
        affinity_subject: &str,
        desired_count: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let mut candidates = self.key_store.list_mcp_session_candidate_key_ids().await?;
        if candidates.is_empty() {
            return Err(ProxyError::NoAvailableKeys);
        }

        candidates.sort_by(|left, right| {
            Self::affinity_subject_score(affinity_subject, right)
                .cmp(&Self::affinity_subject_score(affinity_subject, left))
                .then_with(|| left.cmp(right))
        });
        candidates.truncate(desired_count.clamp(1, candidates.len() as i64).max(1) as usize);
        Ok(candidates)
    }

    async fn build_http_project_affinity_candidates(
        &self,
        ranked: &[String],
        now: i64,
    ) -> Result<Vec<HttpProjectAffinityCandidate>, ProxyError> {
        let cooldowns = self
            .key_store
            .list_active_api_key_transient_backoffs(
                ranked,
                HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE,
                now,
            )
            .await?;
        let recent_rate_limited_counts = self
            .key_store
            .list_recent_rate_limited_request_counts_for_keys(
                ranked,
                now - HTTP_PROJECT_AFFINITY_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let recent_billable_counts = self
            .key_store
            .list_recent_billable_request_counts_for_keys(
                ranked,
                now - HTTP_PROJECT_AFFINITY_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let last_used_at = self.key_store.list_api_key_last_used_at(ranked).await?;

        let mut candidates = ranked
            .iter()
            .enumerate()
            .map(|(stable_rank_index, key_id)| HttpProjectAffinityCandidate {
                key_id: key_id.clone(),
                stable_rank_index,
                cooldown_until: cooldowns.get(key_id).map(|state| state.cooldown_until),
                recent_rate_limited_count: recent_rate_limited_counts
                    .get(key_id)
                    .copied()
                    .unwrap_or(0),
                recent_billable_request_count: recent_billable_counts
                    .get(key_id)
                    .copied()
                    .unwrap_or(0),
                last_used_at: last_used_at.get(key_id).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>();
        Self::order_http_project_affinity_candidates(&mut candidates);
        Ok(candidates)
    }

    async fn maybe_arm_http_project_affinity_backoff(
        &self,
        key_id: &str,
        headers: &HeaderMap,
        analysis: &AttemptAnalysis,
        project_affinity: Option<&HttpProjectAffinityContext>,
    ) -> Result<bool, ProxyError> {
        if project_affinity.is_none()
            || analysis.failure_kind.as_deref() != Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        {
            return Ok(false);
        }

        let now = Utc::now().timestamp();
        let retry_after_secs = Self::mcp_session_init_retry_after_secs(headers, now);
        let cooldown_until = now + retry_after_secs;

        Ok(self
            .key_store
            .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                key_id,
                scope: HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE,
                cooldown_until,
                retry_after_secs,
                reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                source_request_log_id: None,
                now,
            })
            .await?
            .is_some())
    }

    pub(crate) async fn acquire_key_for_http_project(
        &self,
        auth_token_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<Option<HttpProjectAffinitySelection>, ProxyError> {
        let Some(context) = self
            .resolve_http_project_affinity_context(auth_token_id, project_id)
            .await?
        else {
            return Ok(None);
        };

        let existing_binding = self
            .key_store
            .get_http_project_api_key_affinity(&context.owner_subject, &context.project_id_hash)
            .await?;
        let existing_key_id = existing_binding
            .as_ref()
            .map(|binding| binding.api_key_id.clone());
        let now = Utc::now().timestamp();

        if let Some(existing_key_id) = existing_key_id.as_deref() {
            let backoff = self
                .key_store
                .list_active_api_key_transient_backoffs(
                    &[existing_key_id.to_string()],
                    HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE,
                    now,
                )
                .await?;
            let is_cooled = backoff.contains_key(existing_key_id);
            if !is_cooled
                && let Some(lease) = self
                    .key_store
                    .try_acquire_affinity_specific_key(existing_key_id)
                    .await?
            {
                return Ok(Some(HttpProjectAffinitySelection {
                    lease,
                    binding_effect: Self::http_project_affinity_reused_effect(),
                    selection_effect: KeyEffect::none(),
                }));
            }
        }

        let desired_count = self
            .key_store
            .get_system_settings()
            .await?
            .mcp_session_affinity_key_count;
        let ranked = self
            .rank_http_project_affinity_candidate_keys(&context.affinity_subject, desired_count)
            .await?;
        let ordered = self
            .build_http_project_affinity_candidates(&ranked, now)
            .await?;
        let selection_effect = Self::http_project_affinity_selection_effect(&ordered);
        let existing_key_cooled = existing_key_id.as_ref().is_some_and(|key_id| {
            ordered
                .iter()
                .find(|candidate| candidate.key_id == *key_id)
                .and_then(|candidate| candidate.cooldown_until)
                .is_some()
        });

        for candidate in ordered {
            let key_id = candidate.key_id.clone();
            if let Some(lease) = self
                .key_store
                .try_acquire_affinity_specific_key(&key_id)
                .await?
            {
                self.key_store
                    .set_http_project_api_key_affinity(
                        &context.owner_subject,
                        &context.project_id_hash,
                        &lease.id,
                    )
                    .await?;

                let (binding_effect, selection_effect) = match existing_key_id.as_deref() {
                    None => (
                        Self::http_project_affinity_bound_effect(),
                        selection_effect.clone(),
                    ),
                    Some(existing_key_id) if existing_key_id == lease.id => (
                        Self::http_project_affinity_reused_effect(),
                        if existing_key_cooled {
                            selection_effect.clone()
                        } else {
                            KeyEffect::none()
                        },
                    ),
                    Some(_) if existing_key_cooled => (
                        Self::http_project_affinity_rebound_effect(),
                        if selection_effect.code != KEY_EFFECT_NONE {
                            selection_effect.clone()
                        } else {
                            KeyEffect::new(
                                KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
                                "HTTP project affinity skipped a cooled bound key",
                            )
                        },
                    ),
                    Some(_) => (
                        Self::http_project_affinity_rebound_effect(),
                        selection_effect.clone(),
                    ),
                };

                return Ok(Some(HttpProjectAffinitySelection {
                    lease,
                    binding_effect,
                    selection_effect,
                }));
            }
        }

        Err(ProxyError::NoAvailableKeys)
    }

    pub(crate) async fn acquire_key_for_rebalance_mcp_http_call(
        &self,
    ) -> Result<ApiKeyLease, ProxyError> {
        let ranked = self.key_store.list_mcp_session_candidate_key_ids().await?;
        if ranked.is_empty() {
            return Err(ProxyError::NoAvailableKeys);
        }

        let now = Utc::now().timestamp();
        let ordered = self
            .build_rebalance_mcp_http_candidates(&ranked, now)
            .await?;
        for candidate in ordered {
            if let Some(lease) = self
                .key_store
                .try_acquire_affinity_specific_key(&candidate.key_id)
                .await?
            {
                return Ok(lease);
            }
        }

        Err(ProxyError::NoAvailableKeys)
    }

    async fn build_rebalance_mcp_http_candidates(
        &self,
        ranked: &[String],
        now: i64,
    ) -> Result<Vec<HttpProjectAffinityCandidate>, ProxyError> {
        let cooldowns = self
            .key_store
            .list_active_api_key_transient_backoffs(ranked, REBALANCE_MCP_HTTP_BACKOFF_SCOPE, now)
            .await?;
        let recent_rate_limited_counts = self
            .key_store
            .list_recent_rate_limited_request_counts_for_keys(
                ranked,
                now - HTTP_PROJECT_AFFINITY_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let recent_billable_counts = self
            .key_store
            .list_recent_billable_request_counts_for_keys(
                ranked,
                now - HTTP_PROJECT_AFFINITY_RECENT_PRESSURE_WINDOW_SECS,
            )
            .await?;
        let last_used_at = self.key_store.list_api_key_last_used_at(ranked).await?;

        let mut candidates = ranked
            .iter()
            .enumerate()
            .map(|(stable_rank_index, key_id)| HttpProjectAffinityCandidate {
                key_id: key_id.clone(),
                stable_rank_index,
                cooldown_until: cooldowns.get(key_id).map(|state| state.cooldown_until),
                recent_rate_limited_count: recent_rate_limited_counts
                    .get(key_id)
                    .copied()
                    .unwrap_or(0),
                recent_billable_request_count: recent_billable_counts
                    .get(key_id)
                    .copied()
                    .unwrap_or(0),
                last_used_at: last_used_at.get(key_id).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>();
        Self::order_http_project_affinity_candidates(&mut candidates);
        Ok(candidates)
    }

    async fn maybe_arm_rebalance_mcp_http_backoff(
        &self,
        key_id: &str,
        headers: &HeaderMap,
        analysis: &AttemptAnalysis,
    ) -> Result<bool, ProxyError> {
        if analysis.failure_kind.as_deref() != Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429) {
            return Ok(false);
        }

        let now = Utc::now().timestamp();
        let retry_after_secs = Self::mcp_session_init_retry_after_secs(headers, now);
        let cooldown_until = now + retry_after_secs;

        Ok(self
            .key_store
            .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                key_id,
                scope: REBALANCE_MCP_HTTP_BACKOFF_SCOPE,
                cooldown_until,
                retry_after_secs,
                reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                source_request_log_id: None,
                now,
            })
            .await?
            .is_some())
    }

    pub(crate) async fn acquire_key_for(
        &self,
        auth_token_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let Some(token_id) = auth_token_id else {
            // No token id (e.g. certain internal or dev flows) → plain global scheduling.
            return self.key_store.acquire_key().await;
        };

        if let Some(user_id) = self.key_store.find_user_id_by_token(token_id).await? {
            let user_primary = self
                .key_store
                .get_user_primary_api_key_affinity(&user_id)
                .await?;
            let token_primary = self
                .key_store
                .get_token_primary_api_key_affinity(token_id)
                .await?;
            let legacy_primary = if user_primary.is_none() && token_primary.is_none() {
                self.key_store
                    .find_recent_primary_candidate_for_user(&user_id)
                    .await?
            } else {
                None
            };

            let mut candidates = Vec::new();
            if let Some(user_primary) = user_primary.as_ref() {
                candidates.push((user_primary.clone(), false));
            }
            if let Some(token_primary) = token_primary.as_ref()
                && token_primary.user_id.as_deref() == Some(user_id.as_str())
                && !candidates
                    .iter()
                    .any(|(candidate, _)| candidate == &token_primary.api_key_id)
            {
                candidates.push((token_primary.api_key_id.clone(), false));
            }
            if let Some(legacy_primary) = legacy_primary.as_ref()
                && !candidates
                    .iter()
                    .any(|(candidate, _)| candidate == legacy_primary)
            {
                candidates.push((legacy_primary.clone(), true));
            }

            for (key_id, sync_on_acquire) in candidates {
                if let Some(lease) = self
                    .key_store
                    .try_acquire_affinity_specific_key(&key_id)
                    .await?
                {
                    if sync_on_acquire {
                        self.key_store
                            .sync_user_primary_api_key_affinity(&user_id, &lease.id)
                            .await?;
                    }
                    return Ok(lease);
                }
            }

            if user_primary.is_some() || token_primary.is_some() {
                return self
                    .rebind_user_primary_affinity(&user_id, user_primary.as_deref())
                    .await;
            }

            let lease = self.key_store.acquire_key().await?;
            self.key_store
                .sync_user_primary_api_key_affinity(&user_id, &lease.id)
                .await?;
            return Ok(lease);
        }

        if let Some(token_primary) = self
            .key_store
            .get_token_primary_api_key_affinity(token_id)
            .await?
        {
            if let Some(lease) = self
                .key_store
                .try_acquire_affinity_specific_key(&token_primary.api_key_id)
                .await?
            {
                return Ok(lease);
            }

            return self
                .rebind_token_primary_affinity(token_id, Some(&token_primary.api_key_id))
                .await;
        }

        let lease = self.key_store.acquire_key().await?;
        self.key_store
            .set_token_primary_api_key_affinity(token_id, None, &lease.id)
            .await?;
        Ok(lease)
    }

    pub(crate) async fn acquire_key_for_research_request(
        &self,
        auth_token_id: Option<&str>,
        research_request_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let now = Utc::now().timestamp();

        if let Some(request_id) = research_request_id {
            let mut candidate_key_id = {
                let mut state = self.research_request_affinity.lock().await;
                state.get_candidate(request_id, now)
            };

            if candidate_key_id.is_none()
                && let Some((key_id, owner_token_id)) = self
                    .key_store
                    .get_research_request_affinity(request_id, now)
                    .await?
            {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                candidate_key_id = Some(key_id);
            }

            if let Some(key_id) = candidate_key_id {
                if let Some(lease) = self
                    .key_store
                    .try_acquire_affinity_specific_key(&key_id)
                    .await?
                {
                    return Ok(lease);
                }
                return Err(ProxyError::NoAvailableKeys);
            }
        }

        self.acquire_key_for(auth_token_id).await
    }

    pub(crate) async fn populate_research_request_affinity_caches(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        now: i64,
    ) {
        {
            let mut state = self.research_request_affinity.lock().await;
            state.record_mapping(request_id, key_id, now);
        }
        let mut owner_state = self.research_request_owner_affinity.lock().await;
        owner_state.record_mapping(request_id, token_id, now);
    }

    pub(crate) async fn record_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.populate_research_request_affinity_caches(request_id, key_id, token_id, now)
            .await;
        self.key_store
            .save_research_request_affinity(
                request_id,
                key_id,
                token_id,
                now + RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            )
            .await
    }

    pub async fn is_research_request_owned_by(
        &self,
        request_id: &str,
        token_id: Option<&str>,
    ) -> Result<bool, ProxyError> {
        let Some(token_id) = token_id else {
            return Ok(false);
        };

        let now = Utc::now().timestamp();
        if let Some(owner) = {
            let mut state = self.research_request_owner_affinity.lock().await;
            state.get_candidate(request_id, now)
        } {
            return Ok(owner == token_id);
        }

        match self
            .key_store
            .get_research_request_affinity(request_id, now)
            .await
        {
            Ok(Some((key_id, owner_token_id))) => {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                Ok(owner_token_id == token_id)
            }
            Ok(None) => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub(crate) async fn reconcile_key_health(
        &self,
        lease: &ApiKeyLease,
        source: &str,
        analysis: &AttemptAnalysis,
        auth_token_id: Option<&str>,
    ) -> Result<KeyEffect, ProxyError> {
        match &analysis.key_health_action {
            KeyHealthAction::None => {
                if analysis.status != OUTCOME_SUCCESS {
                    return Ok(KeyEffect::none());
                }
                if self
                    .key_store
                    .is_low_quota_depleted_this_month(&lease.id)
                    .await?
                {
                    return Ok(KeyEffect::none());
                }
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let changed = self.key_store.restore_active_status(&lease.secret).await?;
                if !changed {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_RESTORE_ACTIVE.to_string(),
                        operation_summary: "自动恢复为 active".to_string(),
                        reason_code: None,
                        reason_summary: Some("成功请求触发从 exhausted 恢复".to_string()),
                        reason_detail: Some(format!("source={source}")),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_RESTORED_ACTIVE,
                    "The system automatically restored this exhausted key to active",
                ))
            }
            KeyHealthAction::MarkExhausted => {
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let changed = self.key_store.mark_quota_exhausted(&lease.secret).await?;
                if analysis.tavily_status_code == Some(432) {
                    let _ = self
                        .key_store
                        .record_low_quota_depletion_if_needed(
                            &lease.id,
                            self.low_quota_depletion_threshold,
                        )
                        .await?;
                }
                if !changed {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_MARK_EXHAUSTED.to_string(),
                        operation_summary: "自动标记为 exhausted".to_string(),
                        reason_code: Some("quota_exhausted".to_string()),
                        reason_summary: Some("上游额度耗尽".to_string()),
                        reason_detail: Some(format!("source={source}")),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_MARKED_EXHAUSTED,
                    "The system automatically marked this key as exhausted",
                ))
            }
            KeyHealthAction::Quarantine(decision) => {
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let inserted = self
                    .key_store
                    .quarantine_key_by_id(
                        &lease.id,
                        source,
                        &decision.reason_code,
                        &decision.reason_summary,
                        &decision.reason_detail,
                    )
                    .await?;
                if !inserted {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_QUARANTINE.to_string(),
                        operation_summary: "自动隔离 Key".to_string(),
                        reason_code: Some(decision.reason_code.clone()),
                        reason_summary: Some(decision.reason_summary.clone()),
                        reason_detail: Some(decision.reason_detail.clone()),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_QUARANTINED,
                    "The system automatically quarantined this key",
                ))
            }
        }
    }

    pub(crate) async fn maybe_quarantine_usage_error(
        &self,
        key_id: &str,
        source: &str,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        let ProxyError::UsageHttp { status, body } = err else {
            return Ok(());
        };
        let Some(decision) =
            classify_quarantine_reason(Some(status.as_u16() as i64), body.as_bytes())
        else {
            return Ok(());
        };
        let before = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let inserted = self
            .key_store
            .quarantine_key_by_id(
                key_id,
                source,
                &decision.reason_code,
                &decision.reason_summary,
                &decision.reason_detail,
            )
            .await?;
        if inserted {
            let after = self.key_store.fetch_key_state_snapshot(key_id).await?;
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.to_string(),
                    source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                    operation_code: MAINTENANCE_OP_AUTO_QUARANTINE.to_string(),
                    operation_summary: "自动隔离 Key".to_string(),
                    reason_code: Some(decision.reason_code),
                    reason_summary: Some(decision.reason_summary),
                    reason_detail: Some(decision.reason_detail),
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: None,
                    actor_user_id: None,
                    actor_display_name: None,
                    status_before: before.status,
                    status_after: after.status,
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at: Utc::now().timestamp(),
                })
                .await?;
        }
        Ok(())
    }
}
