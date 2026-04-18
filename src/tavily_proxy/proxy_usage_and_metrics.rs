impl TavilyProxy {
    /// Record a token usage log. Intended for /mcp proxy handler.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_local_request_log_without_key(
        &self,
        auth_token_id: Option<&str>,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: StatusCode,
        mcp_status: Option<i64>,
        request_body: &[u8],
        response_body: &[u8],
        result_status: &str,
        failure_kind: Option<&str>,
        forwarded_headers: &[String],
        dropped_headers: &[String],
    ) -> Result<i64, ProxyError> {
        self.record_local_request_log_without_key_with_diagnostics(
            auth_token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            request_body,
            response_body,
            result_status,
            failure_kind,
            None,
            None,
            None,
            None,
            None,
            None,
            forwarded_headers,
            dropped_headers,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_local_request_log_without_key_with_diagnostics(
        &self,
        auth_token_id: Option<&str>,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: StatusCode,
        mcp_status: Option<i64>,
        request_body: &[u8],
        response_body: &[u8],
        result_status: &str,
        failure_kind: Option<&str>,
        gateway_mode: Option<&str>,
        experiment_variant: Option<&str>,
        proxy_session_id: Option<&str>,
        routing_subject_hash: Option<&str>,
        upstream_operation: Option<&str>,
        fallback_reason: Option<&str>,
        forwarded_headers: &[String],
        dropped_headers: &[String],
    ) -> Result<i64, ProxyError> {
        self.key_store
            .log_attempt(AttemptLog {
                key_id: None,
                auth_token_id,
                method,
                path,
                query,
                status: Some(http_status),
                tavily_status_code: mcp_status,
                error: None,
                request_body,
                response_body,
                outcome: result_status,
                failure_kind,
                key_effect_code: KEY_EFFECT_NONE,
                key_effect_summary: None,
                binding_effect_code: KEY_EFFECT_NONE,
                binding_effect_summary: None,
                selection_effect_code: KEY_EFFECT_NONE,
                selection_effect_summary: None,
                gateway_mode,
                experiment_variant,
                proxy_session_id,
                routing_subject_hash,
                upstream_operation,
                fallback_reason,
                forwarded_headers,
                dropped_headers,
            })
            .await
    }

    pub async fn create_or_replace_mcp_session_binding(
        &self,
        binding: &McpSessionBinding,
    ) -> Result<(), ProxyError> {
        self.key_store.create_or_replace_mcp_session(binding).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_or_replace_mcp_session_record(
        &self,
        proxy_session_id: &str,
        upstream_session_id: Option<&str>,
        upstream_key_id: Option<&str>,
        auth_token_id: Option<&str>,
        user_id: Option<&str>,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
        gateway_mode: &str,
        experiment_variant: &str,
        ab_bucket: Option<i64>,
        routing_subject_hash: Option<&str>,
        fallback_reason: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .create_or_replace_mcp_session(&McpSessionBinding {
                proxy_session_id: proxy_session_id.to_string(),
                upstream_session_id: upstream_session_id.map(str::to_string),
                upstream_key_id: upstream_key_id.map(str::to_string),
                auth_token_id: auth_token_id.map(str::to_string),
                user_id: user_id.map(str::to_string),
                protocol_version: protocol_version.map(str::to_string),
                last_event_id: last_event_id.map(str::to_string),
                gateway_mode: gateway_mode.to_string(),
                experiment_variant: experiment_variant.to_string(),
                ab_bucket,
                routing_subject_hash: routing_subject_hash.map(str::to_string),
                fallback_reason: fallback_reason.map(str::to_string),
                rate_limited_until: None,
                last_rate_limited_at: None,
                last_rate_limit_reason: None,
                created_at: now,
                updated_at: now,
                expires_at: now + MCP_SESSION_RETENTION_SECS,
                revoked_at: None,
                revoke_reason: None,
            })
            .await
    }

    pub async fn update_mcp_session_rebalance_metadata(
        &self,
        proxy_session_id: &str,
        routing_subject_hash: Option<&str>,
        fallback_reason: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .update_mcp_session_rebalance_metadata(
                proxy_session_id,
                routing_subject_hash,
                fallback_reason,
                now,
                now + MCP_SESSION_RETENTION_SECS,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<(), ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_token_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            &request_kind,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            binding_effect_code,
            binding_effect_summary,
            selection_effect_code,
            selection_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_with_kind_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            request_kind,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            request_kind,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_token_log(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                request_kind,
                failure_kind,
                key_effect_code.unwrap_or(KEY_EFFECT_NONE),
                key_effect_summary,
                binding_effect_code.unwrap_or(KEY_EFFECT_NONE),
                binding_effect_summary,
                selection_effect_code.unwrap_or(KEY_EFFECT_NONE),
                selection_effect_summary,
                request_log_id,
            )
            .await
    }

    /// Persist a billable attempt before quota counters are charged, so it can be replayed if the
    /// process crashes after the upstream call succeeds.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            binding_effect_code,
            binding_effect_summary,
            selection_effect_code,
            selection_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_with_kind_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            request_kind,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let billing_subject = self.billing_subject_for_token(token_id).await?;
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &billing_subject,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            binding_effect_code,
            binding_effect_summary,
            selection_effect_code,
            selection_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            &request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            binding_effect_code,
            binding_effect_summary,
            selection_effect_code,
            selection_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_with_kind_request_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        binding_effect_code: Option<&str>,
        binding_effect_summary: Option<&str>,
        selection_effect_code: Option<&str>,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .insert_token_log_pending_billing(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                business_credits,
                billing_subject,
                request_kind,
                api_key_id,
                failure_kind,
                key_effect_code.unwrap_or(KEY_EFFECT_NONE),
                key_effect_summary,
                binding_effect_code.unwrap_or(KEY_EFFECT_NONE),
                binding_effect_summary,
                selection_effect_code.unwrap_or(KEY_EFFECT_NONE),
                selection_effect_summary,
                request_log_id,
            )
            .await
    }

    pub async fn settle_pending_billing_attempt(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        self.key_store.apply_pending_billing_log(log_id).await
    }

    pub async fn annotate_pending_billing_attempt(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .annotate_pending_billing_log(log_id, message)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn force_pending_billing_claim_miss_once(&self, log_id: i64) {
        let mut forced = self
            .key_store
            .forced_pending_claim_miss_log_ids
            .lock()
            .await;
        forced.insert(log_id);
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn force_quota_subject_lock_loss_once_for_subject(&self, billing_subject: &str) {
        let mut forced = self
            .key_store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        forced.insert(billing_subject.to_string());
    }

    /// Token summary since a timestamp
    pub async fn token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        self.key_store
            .fetch_token_summary_since(token_id, since, until)
            .await
    }

    /// Token recent logs with optional before-id pagination
    pub async fn token_recent_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        self.key_store
            .fetch_token_logs(token_id, limit, before_id)
            .await
    }

    /// Check and update quota usage for a token. Returns the latest counts and verdict.
    pub async fn check_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        self.token_quota.check(token_id).await
    }

    /// Read-only snapshot of the current business quota usage for a token (hour/day/month).
    /// This does NOT increment any counters.
    pub async fn peek_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota.snapshot_for_token(token_id, now).await
    }

    /// Read-only snapshot for a locked billing subject. Use this when a request must keep the
    /// same quota subject from precheck through charge even if token bindings change mid-flight.
    pub async fn peek_token_quota_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota
            .snapshot_for_billing_subject(billing_subject, now)
            .await
    }

    /// Charge business quota usage for a token by Tavily credits (1:1).
    /// `credits <= 0` is treated as a no-op.
    pub async fn charge_token_quota(&self, token_id: &str, credits: i64) -> Result<(), ProxyError> {
        self.token_quota.charge(token_id, credits).await
    }
}
