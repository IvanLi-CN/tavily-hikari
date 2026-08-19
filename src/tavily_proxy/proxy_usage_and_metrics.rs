#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct RebalanceAuditEntry {
    pub auth_token_id: Option<String>,
    pub method: Method,
    pub path: String,
    pub request_body: Vec<u8>,
    pub response_status: StatusCode,
    pub tavily_status_code: Option<i64>,
    pub response_body: Vec<u8>,
    pub result_status: String,
    pub failure_kind: Option<String>,
    pub proxy_session_id: Option<String>,
    pub routing_subject_hash: Option<String>,
    pub fallback_reason: Option<String>,
}

impl RebalanceAuditEntry {
    fn payload_len(&self) -> usize {
        self.path.len()
            .saturating_add(self.request_body.len())
            .saturating_add(self.response_body.len())
            .saturating_add(self.auth_token_id.as_deref().map_or(0, str::len))
            .saturating_add(self.proxy_session_id.as_deref().map_or(0, str::len))
            .saturating_add(self.routing_subject_hash.as_deref().map_or(0, str::len))
            .saturating_add(self.fallback_reason.as_deref().map_or(0, str::len))
    }
}

impl TavilyProxy {
    #[doc(hidden)]
    pub async fn enqueue_rebalance_audit(&self, entry: RebalanceAuditEntry) -> bool {
        const MAX_AUDITS: usize = 64;
        const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
        let entry_size = entry.payload_len();
        let spawn_flush = {
            let mut writer = self.observability_deferred_writer.lock().await;
            if entry_size > MAX_PAYLOAD_BYTES
                || writer.rebalance_audits.len() >= MAX_AUDITS
                || writer
                    .rebalance_audit_payload_bytes
                    .saturating_add(entry_size)
                    > MAX_PAYLOAD_BYTES
            {
                writer.rebalance_audit_stale = true;
                return false;
            }
            writer.rebalance_audit_payload_bytes = writer
                .rebalance_audit_payload_bytes
                .saturating_add(entry_size);
            writer.rebalance_audits.push_back(entry);
            let spawn_flush = !writer.rebalance_audit_flush_running;
            if spawn_flush {
                writer.rebalance_audit_flush_running = true;
            }
            spawn_flush
        };
        if spawn_flush {
            let proxy = self.clone();
            tokio::spawn(async move { proxy.flush_rebalance_audits().await });
        }
        true
    }

    async fn flush_rebalance_audits(&self) {
        loop {
            let batch = {
                let mut writer = self.observability_deferred_writer.lock().await;
                let mut batch = Vec::new();
                while batch.len() < 10 {
                    let Some(entry) = writer.rebalance_audits.pop_front() else {
                        break;
                    };
                    writer.rebalance_audit_payload_bytes = writer
                        .rebalance_audit_payload_bytes
                        .saturating_sub(entry.payload_len());
                    batch.push(entry);
                }
                if batch.is_empty() {
                    writer.rebalance_audit_flush_running = false;
                }
                batch
            };
            if batch.is_empty() {
                return;
            }
            let permit = match self.key_store.try_admit_observability_deferred_write() {
                Ok(permit) => permit,
                Err(reason) => {
                    self.requeue_rebalance_audits(batch).await;
                    tracing::debug!(
                        component = "observability",
                        event = "rebalance_audit_deferred",
                        defer_reason = reason.as_str(),
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            for entry in batch {
                match self.key_store.log_rebalance_audit_entry(&entry).await {
                    Ok((_request_log_id, created_at)) => {
                        if self
                            .record_server_pressure_event(
                                // Rebalance audit rows intentionally do not carry the
                                // request-log fields used as rebuild source facts. Keep
                                // their derived delta through a source-fenced rebuild.
                                None,
                                created_at,
                                &entry.result_status,
                            )
                            .await
                            .is_err()
                        {
                            let mut writer = self.observability_deferred_writer.lock().await;
                            writer.pressure_stale = true;
                        }
                    }
                    Err(error) => {
                        let transient = crate::store::is_transient_sqlite_write_error(&error);
                        let mut writer = self.observability_deferred_writer.lock().await;
                        writer.rebalance_audit_stale = true;
                        drop(writer);
                        if transient {
                            tracing::debug!(
                                component = "observability",
                                event = "rebalance_audit_flush_deferred",
                                defer_reason = "sqlite_contention",
                            );
                        } else {
                            tracing::warn!(
                                component = "observability",
                                event = "rebalance_audit_flush_failed",
                                error_kind = "sqlite_write",
                            );
                        }
                    }
                }
            }
            drop(permit);
        }
    }

    async fn requeue_rebalance_audits(&self, mut entries: Vec<RebalanceAuditEntry>) {
        let mut writer = self.observability_deferred_writer.lock().await;
        while let Some(entry) = entries.pop() {
            let entry_size = entry.payload_len();
            if writer.rebalance_audits.len() >= 64
                || writer
                    .rebalance_audit_payload_bytes
                    .saturating_add(entry_size)
                    > 1024 * 1024
            {
                writer.rebalance_audit_stale = true;
                continue;
            }
            writer.rebalance_audit_payload_bytes = writer
                .rebalance_audit_payload_bytes
                .saturating_add(entry_size);
            writer.rebalance_audits.push_front(entry);
        }
    }

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
        client_ip: Option<&ClientIpInfo>,
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
            client_ip,
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
        client_ip: Option<&ClientIpInfo>,
    ) -> Result<i64, ProxyError> {
        let request_log_id = self
            .key_store
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
                client_ip,
            })
            .await?;

        if let Err(err) = self
            .record_local_request_log_pressure_event(request_log_id)
            .await
        {
            tracing::warn!(
                component = "analysis_pressure",
                event = "server_pressure_local_request_log_upsert_failed",
                request_log_id,
                error = %err,
                "failed to update server pressure buckets for local request log"
            );
        }

        Ok(request_log_id)
    }

    async fn record_local_request_log_pressure_event(
        &self,
        request_log_id: i64,
    ) -> Result<(), ProxyError> {
        if let Some(event) = self
            .key_store
            .fetch_server_pressure_event_for_request_log(request_log_id)
            .await?
        {
            let outcome = if event.result_status == OUTCOME_SUCCESS {
                UserBusinessCallOutcome::Success
            } else {
                UserBusinessCallOutcome::Failure
            };
            self.user_business_calls_1h_window
                .record_event(&event.user_id, event.request_log_id, event.created_at, outcome)
                .await;
            self.record_server_pressure_event(
                event.request_log_id,
                event.created_at,
                &event.result_status,
            )
                .await?;
        }
        Ok(())
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
        let now = self.backend_time.now_ts();
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

    pub async fn get_ha_full_master_node_id(&self) -> Result<Option<String>, ProxyError> {
        self.key_store.get_ha_full_master_node_id().await
    }

    pub async fn set_ha_full_master_node_id(&self, node_id: &str) -> Result<(), ProxyError> {
        self.key_store.set_ha_full_master_node_id(node_id).await
    }

    pub async fn update_mcp_session_rebalance_metadata(
        &self,
        proxy_session_id: &str,
        routing_subject_hash: Option<&str>,
        fallback_reason: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
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
        self.record_token_attempt_with_kind_request_log_metadata_receipt(
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
            binding_effect_code,
            binding_effect_summary,
            selection_effect_code,
            selection_effect_summary,
            request_log_id,
        )
        .await
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn record_token_attempt_with_kind_request_log_metadata_receipt(
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
    ) -> Result<UserBusinessCallEventBridgeReceipt, ProxyError> {
        let decision = self
            .key_store
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
            .await?;
        let receipt = self.apply_user_business_call_event_write(decision).await;
        if let UserBusinessCallEventBridgeReceipt::Applied {
            request_log_id,
            created_at,
            outcome,
        } = receipt
        {
            self.record_server_pressure_event(
                request_log_id,
                created_at,
                match outcome {
                    UserBusinessCallOutcome::Success => OUTCOME_SUCCESS,
                    UserBusinessCallOutcome::Failure => "error",
                },
            )
            .await?;
        }
        Ok(receipt)
    }

    async fn apply_user_business_call_event_write(
        &self,
        decision: UserBusinessCallEventWriteDecision,
    ) -> UserBusinessCallEventBridgeReceipt {
        let receipt = match decision {
            UserBusinessCallEventWriteDecision::Applied(event) => {
                let outcome = if event.result_status == OUTCOME_SUCCESS {
                    UserBusinessCallOutcome::Success
                } else {
                    UserBusinessCallOutcome::Failure
                };
                self.user_business_calls_1h_window
                    .record_event(&event.user_id, event.request_log_id, event.created_at, outcome)
                    .await;
                UserBusinessCallEventBridgeReceipt::Applied {
                    request_log_id: event.request_log_id,
                    created_at: event.created_at,
                    outcome,
                }
            }
            UserBusinessCallEventWriteDecision::Skipped {
                request_log_id,
                reason,
            } => UserBusinessCallEventBridgeReceipt::Skipped {
                request_log_id,
                reason,
            },
        };
        self.record_user_business_call_bridge_diagnostic(&receipt)
            .await;
        receipt
    }

    async fn record_user_business_call_bridge_diagnostic(
        &self,
        receipt: &UserBusinessCallEventBridgeReceipt,
    ) {
        const LOG_INTERVAL: Duration = Duration::from_secs(60);

        let now = self.backend_time.instant_now();
        let mut diagnostics = self.user_business_call_bridge_diagnostics.lock().await;
        match receipt {
            UserBusinessCallEventBridgeReceipt::Applied { .. } => {
                diagnostics.applied = diagnostics.applied.saturating_add(1);
            }
            UserBusinessCallEventBridgeReceipt::Skipped { reason, .. } => match reason {
                UserBusinessCallEventSkipReason::MissingUserId => {
                    diagnostics.missing_user_id = diagnostics.missing_user_id.saturating_add(1);
                }
                UserBusinessCallEventSkipReason::NotBusinessQuota => {
                    diagnostics.not_business_quota =
                        diagnostics.not_business_quota.saturating_add(1);
                }
                UserBusinessCallEventSkipReason::MissingUpstreamOperation => {
                    diagnostics.missing_upstream_operation =
                        diagnostics.missing_upstream_operation.saturating_add(1);
                }
                UserBusinessCallEventSkipReason::QuotaExhausted => {
                    diagnostics.quota_exhausted = diagnostics.quota_exhausted.saturating_add(1);
                }
            },
        }
        if now.saturating_duration_since(diagnostics.window_started_at) < LOG_INTERVAL {
            return;
        }
        tracing::info!(
            component = "user_business_calls_1h",
            event = "business_call_event_bridge_summary",
            window_secs = now
                .saturating_duration_since(diagnostics.window_started_at)
                .as_secs(),
            applied = diagnostics.applied,
            skipped_missing_user_id = diagnostics.missing_user_id,
            skipped_not_business_quota = diagnostics.not_business_quota,
            skipped_missing_upstream_operation = diagnostics.missing_upstream_operation,
            skipped_quota_exhausted = diagnostics.quota_exhausted,
            "business-call event bridge summary"
        );
        *diagnostics = UserBusinessCallBridgeDiagnostics::new(now);
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
        let (log_id, event) = self
            .key_store
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
            .await?;
        let receipt = self.apply_user_business_call_event_write(event).await;
        if let UserBusinessCallEventBridgeReceipt::Applied {
            request_log_id,
            created_at,
            outcome,
        } = receipt
        {
            self.record_server_pressure_event(
                request_log_id,
                created_at,
                match outcome {
                    UserBusinessCallOutcome::Success => OUTCOME_SUCCESS,
                    UserBusinessCallOutcome::Failure => "error",
                },
            )
            .await?;
        }
        Ok(log_id)
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

    pub async fn token_recent_logs_by_billing(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
        billing_filter: TokenLogBillingFilter,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        self.key_store
            .fetch_token_logs_by_billing(token_id, limit, before_id, billing_filter)
            .await
    }

    /// Check and update quota usage for a token. Returns the latest counts and verdict.
    pub async fn check_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        self.token_quota.check(token_id).await
    }

    /// Read-only snapshot of the current business quota usage for a token (hour/day/month).
    /// This does NOT increment any counters.
    pub async fn peek_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = self.backend_time.now_utc();
        self.token_quota.snapshot_for_token(token_id, now).await
    }

    /// Read-only snapshot for a locked billing subject. Use this when a request must keep the
    /// same quota subject from precheck through charge even if token bindings change mid-flight.
    pub async fn peek_token_quota_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = self.backend_time.now_utc();
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
