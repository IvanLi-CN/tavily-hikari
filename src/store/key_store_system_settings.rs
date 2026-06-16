impl KeyStore {
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
        let api_rebalance_enabled = self
            .get_meta_i64(META_KEY_API_REBALANCE_ENABLED_V1)
            .await?
            .unwrap_or(i64::from(API_REBALANCE_ENABLED_DEFAULT))
            != 0;
        let api_rebalance_percent = self
            .get_meta_i64(META_KEY_API_REBALANCE_PERCENT_V1)
            .await?
            .unwrap_or(API_REBALANCE_PERCENT_DEFAULT)
            .clamp(API_REBALANCE_PERCENT_MIN, API_REBALANCE_PERCENT_MAX);
        let recharge_feature_enabled = self
            .get_meta_i64(META_KEY_RECHARGE_FEATURE_ENABLED_V1)
            .await?
            .unwrap_or(0)
            != 0;
        let recharge_user_enabled = self
            .get_meta_i64(META_KEY_RECHARGE_USER_ENABLED_V1)
            .await?
            .unwrap_or(0)
            != 0;
        let admin_default_active_users_only = self
            .get_meta_i64(META_KEY_ADMIN_DEFAULT_ACTIVE_USERS_ONLY_V1)
            .await?
            .unwrap_or(0)
            != 0;
        let user_blocked_key_base_limit = self.fetch_user_blocked_key_base_limit().await?;
        let global_ip_limit = self
            .get_meta_i64(META_KEY_GLOBAL_IP_LIMIT_V1)
            .await?
            .unwrap_or(GLOBAL_IP_LIMIT_DEFAULT)
            .max(0);
        let defaults = TrustedClientIpSettings::default();
        let trusted_proxy_cidrs = self
            .get_meta_string(META_KEY_TRUSTED_PROXY_CIDRS_V1)
            .await?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .map(|values| normalize_trusted_proxy_cidrs(&values))
            .unwrap_or(defaults.trusted_proxy_cidrs);
        let trusted_client_ip_headers = self
            .get_meta_string(META_KEY_TRUSTED_CLIENT_IP_HEADERS_V1)
            .await?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .map(|values| normalize_trusted_client_ip_headers(&values))
            .unwrap_or(defaults.trusted_client_ip_headers);
        let mut retention_defaults = default_request_log_retention_settings();
        retention_defaults.max_log_retention_days =
            effective_request_logs_retention_days().min(REQUEST_LOG_RETENTION_DAYS_MAX);
        let request_log_retention = normalize_request_log_retention_settings(
            &RequestLogRetentionSettings {
                max_log_retention_days: self
                    .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_MAX_DAYS_V1)
                    .await?
                    .unwrap_or(retention_defaults.max_log_retention_days),
                heavy_usage_threshold_percent: self
                    .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_HEAVY_THRESHOLD_PERCENT_V1)
                    .await?
                    .unwrap_or(retention_defaults.heavy_usage_threshold_percent),
                global: RequestLogRetentionProfile {
                    business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_GLOBAL_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.global.business_body_days),
                    non_business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_GLOBAL_NON_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.global.non_business_body_days),
                    non_success_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_GLOBAL_NON_SUCCESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.global.non_success_body_days),
                },
                heavy_usage: RequestLogRetentionProfile {
                    business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_HEAVY_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.heavy_usage.business_body_days),
                    non_business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_HEAVY_NON_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.heavy_usage.non_business_body_days),
                    non_success_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_HEAVY_NON_SUCCESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.heavy_usage.non_success_body_days),
                },
                debug_shared: RequestLogRetentionProfile {
                    business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_DEBUG_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.debug_shared.business_body_days),
                    non_business_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_DEBUG_NON_BUSINESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.debug_shared.non_business_body_days),
                    non_success_body_days: self
                        .get_meta_i64(META_KEY_REQUEST_LOG_RETENTION_DEBUG_NON_SUCCESS_BODY_DAYS_V1)
                        .await?
                        .unwrap_or(retention_defaults.debug_shared.non_success_body_days),
                },
            },
        )?;
        let settings = SystemSettings {
            request_rate_limit,
            mcp_session_affinity_key_count: count,
            rebalance_mcp_enabled,
            rebalance_mcp_session_percent,
            api_rebalance_enabled,
            api_rebalance_percent,
            recharge_feature_enabled,
            recharge_user_enabled,
            admin_default_active_users_only,
            user_blocked_key_base_limit,
            global_ip_limit,
            trusted_proxy_cidrs,
            trusted_client_ip_headers,
            request_log_retention,
        };
        Ok(settings)
    }

    pub(crate) async fn get_request_log_retention_settings_cached(
        &self,
    ) -> Result<RequestLogRetentionSettings, ProxyError> {
        if let Some(settings) = self.request_log_retention_cache.read().await.clone() {
            return Ok(settings);
        }
        let settings = self.get_system_settings().await?.request_log_retention;
        *self.request_log_retention_cache.write().await = Some(settings.clone());
        Ok(settings)
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
        if !(API_REBALANCE_PERCENT_MIN..=API_REBALANCE_PERCENT_MAX)
            .contains(&settings.api_rebalance_percent)
        {
            return Err(ProxyError::Other(format!(
                "api_rebalance_percent must be between {} and {}",
                API_REBALANCE_PERCENT_MIN, API_REBALANCE_PERCENT_MAX,
            )));
        }
        if settings.user_blocked_key_base_limit < 0 {
            return Err(ProxyError::Other(
                "user_blocked_key_base_limit must be a non-negative integer".to_string(),
            ));
        }
        if settings.global_ip_limit < 0 {
            return Err(ProxyError::Other(
                "global_ip_limit must be a non-negative integer".to_string(),
            ));
        }
        let trusted_client_ip = validate_trusted_client_ip_settings(&TrustedClientIpSettings {
            trusted_proxy_cidrs: settings.trusted_proxy_cidrs.clone(),
            trusted_client_ip_headers: settings.trusted_client_ip_headers.clone(),
        })?;
        let previous_request_log_retention =
            self.get_system_settings().await?.request_log_retention;
        let request_log_retention =
            normalize_request_log_retention_settings(&settings.request_log_retention)?;
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
            META_KEY_API_REBALANCE_ENABLED_V1,
            i64::from(settings.api_rebalance_enabled),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_API_REBALANCE_PERCENT_V1,
            settings.api_rebalance_percent,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_RECHARGE_FEATURE_ENABLED_V1,
            i64::from(settings.recharge_feature_enabled),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_RECHARGE_USER_ENABLED_V1,
            i64::from(settings.recharge_user_enabled),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_ADMIN_DEFAULT_ACTIVE_USERS_ONLY_V1,
            i64::from(settings.admin_default_active_users_only),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_USER_BLOCKED_KEY_BASE_LIMIT_V1,
            settings.user_blocked_key_base_limit,
        )
        .await?;
        self.set_meta_i64(META_KEY_GLOBAL_IP_LIMIT_V1, settings.global_ip_limit)
            .await?;
        self.set_meta_string(
            META_KEY_TRUSTED_PROXY_CIDRS_V1,
            &serde_json::to_string(&trusted_client_ip.trusted_proxy_cidrs)
                .unwrap_or_else(|_| "[]".to_string()),
        )
        .await?;
        self.set_meta_string(
            META_KEY_TRUSTED_CLIENT_IP_HEADERS_V1,
            &serde_json::to_string(&trusted_client_ip.trusted_client_ip_headers)
                .unwrap_or_else(|_| "[]".to_string()),
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_MAX_DAYS_V1,
            request_log_retention.max_log_retention_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_HEAVY_THRESHOLD_PERCENT_V1,
            request_log_retention.heavy_usage_threshold_percent,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_GLOBAL_BUSINESS_BODY_DAYS_V1,
            request_log_retention.global.business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_GLOBAL_NON_BUSINESS_BODY_DAYS_V1,
            request_log_retention.global.non_business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_GLOBAL_NON_SUCCESS_BODY_DAYS_V1,
            request_log_retention.global.non_success_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_HEAVY_BUSINESS_BODY_DAYS_V1,
            request_log_retention.heavy_usage.business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_HEAVY_NON_BUSINESS_BODY_DAYS_V1,
            request_log_retention.heavy_usage.non_business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_HEAVY_NON_SUCCESS_BODY_DAYS_V1,
            request_log_retention.heavy_usage.non_success_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_DEBUG_BUSINESS_BODY_DAYS_V1,
            request_log_retention.debug_shared.business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_DEBUG_NON_BUSINESS_BODY_DAYS_V1,
            request_log_retention.debug_shared.non_business_body_days,
        )
        .await?;
        self.set_meta_i64(
            META_KEY_REQUEST_LOG_RETENTION_DEBUG_NON_SUCCESS_BODY_DAYS_V1,
            request_log_retention.debug_shared.non_success_body_days,
        )
        .await?;
        self.record_request_rate_limit_snapshot_at(
            settings.request_rate_limit,
            self.backend_time.now_ts(),
        )
        .await?;
        let saved_settings = SystemSettings {
            request_rate_limit: settings.request_rate_limit,
            mcp_session_affinity_key_count: settings.mcp_session_affinity_key_count,
            rebalance_mcp_enabled: settings.rebalance_mcp_enabled,
            rebalance_mcp_session_percent: settings.rebalance_mcp_session_percent,
            api_rebalance_enabled: settings.api_rebalance_enabled,
            api_rebalance_percent: settings.api_rebalance_percent,
            recharge_feature_enabled: settings.recharge_feature_enabled,
            recharge_user_enabled: settings.recharge_user_enabled,
            admin_default_active_users_only: settings.admin_default_active_users_only,
            user_blocked_key_base_limit: settings.user_blocked_key_base_limit,
            global_ip_limit: settings.global_ip_limit,
            trusted_proxy_cidrs: trusted_client_ip.trusted_proxy_cidrs,
            trusted_client_ip_headers: trusted_client_ip.trusted_client_ip_headers,
            request_log_retention: request_log_retention.clone(),
        };
        *self.request_log_retention_cache.write().await = Some(request_log_retention.clone());
        if previous_request_log_retention.max_log_retention_days
            != request_log_retention.max_log_retention_days
        {
            self.rebuild_request_log_catalog_rollups().await?;
            self.set_meta_i64(
                META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS,
                request_log_retention.max_log_retention_days,
            )
            .await?;
            self.set_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE, 1)
                .await?;
        } else {
            self.invalidate_request_logs_catalog_cache().await;
        }
        if previous_request_log_retention != request_log_retention {
            self.clear_request_log_body_gc_cursor().await?;
        }
        Ok(saved_settings)
    }

    pub(crate) async fn get_admin_totp_secret_record(
        &self,
    ) -> Result<Option<(String, String, i64)>, ProxyError> {
        let Some(ciphertext) = self
            .get_meta_string(META_KEY_ADMIN_TOTP_SECRET_CIPHERTEXT_V1)
            .await?
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let Some(nonce) = self
            .get_meta_string(META_KEY_ADMIN_TOTP_SECRET_NONCE_V1)
            .await?
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let enabled_at = self
            .get_meta_i64(META_KEY_ADMIN_TOTP_ENABLED_AT_V1)
            .await?
            .unwrap_or(0);
        Ok(Some((ciphertext, nonce, enabled_at)))
    }

    pub(crate) async fn set_admin_totp_secret_record(
        &self,
        ciphertext: &str,
        nonce: &str,
        enabled_at: i64,
    ) -> Result<(), ProxyError> {
        self.set_meta_string(META_KEY_ADMIN_TOTP_SECRET_CIPHERTEXT_V1, ciphertext)
            .await?;
        self.set_meta_string(META_KEY_ADMIN_TOTP_SECRET_NONCE_V1, nonce)
            .await?;
        self.set_meta_i64(META_KEY_ADMIN_TOTP_ENABLED_AT_V1, enabled_at)
            .await?;
        self.clear_admin_totp_failures().await?;
        Ok(())
    }

    pub(crate) async fn clear_admin_totp_secret_record(&self) -> Result<(), ProxyError> {
        self.set_meta_string(META_KEY_ADMIN_TOTP_SECRET_CIPHERTEXT_V1, "")
            .await?;
        self.set_meta_string(META_KEY_ADMIN_TOTP_SECRET_NONCE_V1, "")
            .await?;
        self.set_meta_i64(META_KEY_ADMIN_TOTP_ENABLED_AT_V1, 0)
            .await?;
        self.clear_admin_totp_failures().await?;
        Ok(())
    }

    pub(crate) async fn get_admin_totp_failure_state(&self) -> Result<(i64, i64), ProxyError> {
        let count = self
            .get_meta_i64(META_KEY_ADMIN_TOTP_FAILURE_COUNT_V1)
            .await?
            .unwrap_or(0);
        let locked_until = self
            .get_meta_i64(META_KEY_ADMIN_TOTP_LOCKED_UNTIL_V1)
            .await?
            .unwrap_or(0);
        Ok((count, locked_until))
    }

    pub(crate) async fn set_admin_totp_failure_state(
        &self,
        count: i64,
        locked_until: i64,
    ) -> Result<(), ProxyError> {
        self.set_meta_i64(META_KEY_ADMIN_TOTP_FAILURE_COUNT_V1, count)
            .await?;
        self.set_meta_i64(META_KEY_ADMIN_TOTP_LOCKED_UNTIL_V1, locked_until)
            .await?;
        Ok(())
    }

    pub(crate) async fn clear_admin_totp_failures(&self) -> Result<(), ProxyError> {
        self.set_admin_totp_failure_state(0, 0).await
    }
}
