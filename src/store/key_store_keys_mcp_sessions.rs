// MCP session store surface, extracted verbatim from key_store_keys.rs (post-rebase
// line-budget remediation; pure textual move inside the store include! family -
// same module scope, zero semantic or visibility changes).
impl KeyStore {
    pub(crate) async fn create_or_replace_mcp_session(
        &self,
        binding: &McpSessionBinding,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO mcp_sessions (
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(proxy_session_id) DO UPDATE SET
                upstream_session_id = excluded.upstream_session_id,
                upstream_key_id = excluded.upstream_key_id,
                auth_token_id = excluded.auth_token_id,
                user_id = excluded.user_id,
                protocol_version = excluded.protocol_version,
                last_event_id = excluded.last_event_id,
                gateway_mode = excluded.gateway_mode,
                experiment_variant = excluded.experiment_variant,
                ab_bucket = excluded.ab_bucket,
                routing_subject_hash = excluded.routing_subject_hash,
                fallback_reason = excluded.fallback_reason,
                rate_limited_until = excluded.rate_limited_until,
                last_rate_limited_at = excluded.last_rate_limited_at,
                last_rate_limit_reason = excluded.last_rate_limit_reason,
                updated_at = excluded.updated_at,
                expires_at = excluded.expires_at,
                revoked_at = excluded.revoked_at,
                revoke_reason = excluded.revoke_reason
            "#,
        )
        .bind(&binding.proxy_session_id)
        .bind(binding.upstream_session_id.as_deref())
        .bind(binding.upstream_key_id.as_deref())
        .bind(binding.auth_token_id.as_deref())
        .bind(binding.user_id.as_deref())
        .bind(binding.protocol_version.as_deref())
        .bind(binding.last_event_id.as_deref())
        .bind(&binding.gateway_mode)
        .bind(&binding.experiment_variant)
        .bind(binding.ab_bucket)
        .bind(binding.routing_subject_hash.as_deref())
        .bind(binding.fallback_reason.as_deref())
        .bind(binding.rate_limited_until)
        .bind(binding.last_rate_limited_at)
        .bind(binding.last_rate_limit_reason.as_deref())
        .bind(binding.created_at)
        .bind(binding.updated_at)
        .bind(binding.expires_at)
        .bind(binding.revoked_at)
        .bind(binding.revoke_reason.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_active_mcp_session(
        &self,
        proxy_session_id: &str,
        now: i64,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            FROM mcp_sessions
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(proxy_session_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| McpSessionBinding {
            proxy_session_id: row.get("proxy_session_id"),
            upstream_session_id: row.get("upstream_session_id"),
            upstream_key_id: row.get("upstream_key_id"),
            auth_token_id: row.get("auth_token_id"),
            user_id: row.get("user_id"),
            protocol_version: row.get("protocol_version"),
            last_event_id: row.get("last_event_id"),
            gateway_mode: row.get("gateway_mode"),
            experiment_variant: row.get("experiment_variant"),
            ab_bucket: row.get("ab_bucket"),
            routing_subject_hash: row.get("routing_subject_hash"),
            fallback_reason: row.get("fallback_reason"),
            rate_limited_until: row.get("rate_limited_until"),
            last_rate_limited_at: row.get("last_rate_limited_at"),
            last_rate_limit_reason: row.get("last_rate_limit_reason"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
            revoke_reason: row.get("revoke_reason"),
        }))
    }

    pub(crate) async fn get_latest_active_mcp_session_for_token(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            FROM mcp_sessions
            WHERE auth_token_id = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            ORDER BY updated_at DESC, expires_at DESC
            LIMIT 1
            "#,
        )
        .bind(token_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| McpSessionBinding {
            proxy_session_id: row.get("proxy_session_id"),
            upstream_session_id: row.get("upstream_session_id"),
            upstream_key_id: row.get("upstream_key_id"),
            auth_token_id: row.get("auth_token_id"),
            user_id: row.get("user_id"),
            protocol_version: row.get("protocol_version"),
            last_event_id: row.get("last_event_id"),
            gateway_mode: row.get("gateway_mode"),
            experiment_variant: row.get("experiment_variant"),
            ab_bucket: row.get("ab_bucket"),
            routing_subject_hash: row.get("routing_subject_hash"),
            fallback_reason: row.get("fallback_reason"),
            rate_limited_until: row.get("rate_limited_until"),
            last_rate_limited_at: row.get("last_rate_limited_at"),
            last_rate_limit_reason: row.get("last_rate_limit_reason"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
            revoke_reason: row.get("revoke_reason"),
        }))
    }

    pub(crate) async fn has_active_non_rebalance_mcp_session_for_token(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let exists: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM mcp_sessions
                WHERE auth_token_id = ?
                  AND gateway_mode <> ?
                  AND revoked_at IS NULL
                  AND expires_at > ?
            )
            "#,
        )
        .bind(token_id)
        .bind(MCP_GATEWAY_MODE_REBALANCE)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists != 0)
    }

    pub(crate) async fn touch_mcp_session(
        &self,
        proxy_session_id: &str,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                protocol_version = COALESCE(?, protocol_version),
                last_event_id = COALESCE(?, last_event_id),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(protocol_version)
        .bind(last_event_id)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_mcp_session_rate_limited(
        &self,
        proxy_session_id: &str,
        rate_limited_until: i64,
        reason: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                rate_limited_until = ?,
                last_rate_limited_at = ?,
                last_rate_limit_reason = ?,
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(rate_limited_until)
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn clear_mcp_session_rate_limit(
        &self,
        proxy_session_id: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                rate_limited_until = NULL,
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_mcp_session_upstream_identity(
        &self,
        proxy_session_id: &str,
        upstream_session_id: &str,
        protocol_version: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                upstream_session_id = ?,
                protocol_version = COALESCE(?, protocol_version),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(upstream_session_id)
        .bind(protocol_version)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_mcp_session_rebalance_metadata(
        &self,
        proxy_session_id: &str,
        routing_subject_hash: Option<&str>,
        fallback_reason: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                routing_subject_hash = COALESCE(?, routing_subject_hash),
                fallback_reason = COALESCE(?, fallback_reason),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(routing_subject_hash)
        .bind(fallback_reason)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_session(
        &self,
        proxy_session_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE proxy_session_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_sessions_for_user_key(
        &self,
        user_id: &str,
        upstream_key_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE user_id = ? AND upstream_key_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(user_id)
        .bind(upstream_key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_sessions_for_token_key(
        &self,
        token_id: &str,
        upstream_key_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE auth_token_id = ? AND upstream_key_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(token_id)
        .bind(upstream_key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
