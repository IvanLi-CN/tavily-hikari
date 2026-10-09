impl TavilyProxy {
    /// Sync usage/quota for specific key via Tavily Usage API base (e.g., https://api.tavily.com).
    pub async fn sync_key_quota(
        &self,
        key_id: &str,
        usage_base: &str,
        source: &str,
    ) -> Result<(i64, i64), ProxyError> {
        let Some(secret) = self.key_store.fetch_api_key_secret(key_id).await? else {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        };
        let (limit, remaining) = match self
            .fetch_usage_quota_for_secret(
                &secret,
                usage_base,
                Some(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS)),
                Some(key_id),
                None,
                "quota_sync",
            )
            .await
        {
            Ok(quota) => quota,
            Err(err) => {
                let err = normalize_quota_sync_fetch_error(err);
                self.maybe_quarantine_usage_error(key_id, "/api/tavily/usage", &err)
                    .await?;
                return Err(err);
            }
        };
        let now = self.backend_time.now_ts();
        self.key_store
            .record_quota_sync_sample(key_id, limit, remaining, now, source)
            .await?;
        self.clear_transient_backoffs_after_success(key_id, source, None)
            .await?;
        Ok((limit, remaining))
    }

    pub async fn quota_sync_api_key_secret(&self, key_id: &str) -> Result<String, ProxyError> {
        self.key_store
            .fetch_api_key_secret(key_id)
            .await?
            .ok_or_else(|| ProxyError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn fetch_usage_quota_for_sync_secret(
        &self,
        secret: &str,
        usage_base: &str,
        key_id: &str,
    ) -> Result<(i64, i64), ProxyError> {
        self.fetch_usage_quota_for_secret(
            secret,
            usage_base,
            Some(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS)),
            Some(key_id),
            None,
            "quota_sync",
        )
        .await
        .map_err(normalize_quota_sync_fetch_error)
    }

    /// Prepare the local proxy plan before taking the shared outbound lease.
    /// This is the scheduler-facing path for automatic quota work: local
    /// maintenance and plan construction remain outside the actual request
    /// lease, while the lease is held through the outbound send and response.
    pub async fn fetch_usage_quota_for_sync_secret_with_admission(
        &self,
        secret: &str,
        usage_base: &str,
        key_id: &str,
        admission: std::sync::Arc<crate::RemoteAttemptAdmissionController>,
        manual_remote_attempt: bool,
    ) -> Result<(i64, i64), ProxyError> {
        let plan = self.prepare_forward_proxy_plan(key_id).await;
        let remote_attempt = if manual_remote_attempt {
            admission.acquire_manual_attempt().await
        } else {
            admission.acquire_attempt().await
        }
        .map_err(|reason| ProxyError::Other(reason.to_string()))?;
        self.fetch_usage_quota_for_secret_with_plan(
            secret,
            usage_base,
            Some(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS)),
            Some(key_id),
            None,
            "quota_sync",
            Some(remote_attempt),
            Some(plan),
        )
        .await
        .map_err(normalize_quota_sync_fetch_error)
    }

    /// M4 dispatch-guard seam (plan todo 19): stored provider stamp of a
    /// key row for the quota-sync lanes. `None` when the key row does not
    /// exist — callers keep their legacy missing-key behavior.
    pub async fn quota_sync_key_provider(
        &self,
        key_id: &str,
    ) -> Result<Option<crate::providers::ProviderId>, ProxyError> {
        self.key_store.fetch_api_key_provider_by_id(key_id).await
    }

    /// Provider-scoped pending list for non-Tavily quota-sync lanes (plan
    /// todo 19). The Tavily lanes keep their dedicated functions (which
    /// carry the M4 `provider='tavily'` hard filter); this additive seam
    /// exists so a provider lane can only ever enumerate its OWN keys.
    pub async fn list_keys_pending_provider_quota_sync(
        &self,
        provider: crate::providers::ProviderId,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .list_keys_pending_provider_quota_sync(provider, older_than_secs)
            .await
    }

    /// Provider-scoped hot-lane pending list (plan todo 19), mirroring
    /// `list_keys_pending_hot_quota_sync` for non-Tavily providers.
    pub async fn list_keys_pending_hot_provider_quota_sync(
        &self,
        provider: crate::providers::ProviderId,
        active_within_secs: i64,
        stale_after_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .list_keys_pending_hot_provider_quota_sync(
                provider,
                active_within_secs,
                stale_after_secs,
            )
            .await
    }
}
