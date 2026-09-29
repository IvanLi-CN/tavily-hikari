impl KeyStore {
    fn push_admin_mcp_session_binding_filters(
        builder: &mut QueryBuilder<'_, Sqlite>,
        query: &AdminMcpSessionBindingsQuery,
        now: i64,
        active_only: bool,
    ) {
        builder.push("gateway_mode = ");
        builder.push_bind(MCP_GATEWAY_MODE_UPSTREAM);

        if let Some(created_from) = query.created_from {
            builder.push(" AND created_at >= ");
            builder.push_bind(created_from);
        }
        if let Some(created_to) = query.created_to {
            builder.push(" AND created_at <= ");
            builder.push_bind(created_to);
        }
        if let Some(updated_from) = query.updated_from {
            builder.push(" AND updated_at >= ");
            builder.push_bind(updated_from);
        }
        if let Some(updated_to) = query.updated_to {
            builder.push(" AND updated_at <= ");
            builder.push_bind(updated_to);
        }

        if active_only {
            if matches!(query.status, AdminMcpSessionBindingFilterStatus::Revoked) {
                builder.push(" AND 1 = 0");
                return;
            }
            builder.push(" AND revoked_at IS NULL AND expires_at > ");
            builder.push_bind(now);
            return;
        }

        match query.status {
            AdminMcpSessionBindingFilterStatus::Active => {
                builder.push(" AND revoked_at IS NULL AND expires_at > ");
                builder.push_bind(now);
            }
            AdminMcpSessionBindingFilterStatus::Revoked => {
                builder.push(" AND revoked_at IS NOT NULL");
            }
            AdminMcpSessionBindingFilterStatus::All => {}
        }
    }

    pub(crate) async fn list_admin_mcp_session_bindings(
        &self,
        query: &AdminMcpSessionBindingsQuery,
    ) -> Result<AdminMcpSessionBindingsPage, ProxyError> {
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let now = self.backend_time.now_ts();
        let page = query.page.max(1);
        let per_page = query.per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;

        let mut total_builder =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM mcp_sessions WHERE ");
        Self::push_admin_mcp_session_binding_filters(&mut total_builder, query, now, false);
        let total = total_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;

        let mut active_matching_builder =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM mcp_sessions WHERE ");
        Self::push_admin_mcp_session_binding_filters(
            &mut active_matching_builder,
            query,
            now,
            true,
        );
        let active_matching_count = active_matching_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;

        let mut rows_builder = QueryBuilder::<Sqlite>::new(
            "SELECT proxy_session_id, auth_token_id, user_id, upstream_key_id, \
             created_at, updated_at, expires_at, revoked_at, revoke_reason \
             FROM mcp_sessions WHERE ",
        );
        Self::push_admin_mcp_session_binding_filters(&mut rows_builder, query, now, false);
        rows_builder.push(" ORDER BY updated_at DESC, proxy_session_id DESC LIMIT ");
        rows_builder.push_bind(per_page);
        rows_builder.push(" OFFSET ");
        rows_builder.push_bind(offset);
        let rows = rows_builder
            .build_query_as::<(
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
                i64,
                i64,
                Option<i64>,
                Option<String>,
            )>()
            .fetch_all(&self.pool)
            .await?;

        Ok(AdminMcpSessionBindingsPage {
            items: rows
                .into_iter()
                .map(
                    |(
                        proxy_session_id,
                        auth_token_id,
                        user_id,
                        upstream_key_id,
                        created_at,
                        updated_at,
                        expires_at,
                        revoked_at,
                        revoke_reason,
                    )| {
                        let status = if revoked_at.is_some() {
                            "revoked"
                        } else if expires_at > now {
                            "active"
                        } else {
                            "expired"
                        };
                        AdminMcpSessionBindingListItem {
                            proxy_session_id,
                            auth_token_id,
                            user_id,
                            upstream_key_id,
                            created_at,
                            updated_at,
                            expires_at,
                            status: status.to_string(),
                            revoked_at,
                            revoke_reason,
                        }
                    },
                )
                .collect(),
            total,
            page,
            per_page,
            active_matching_count,
        })
    }

    pub(crate) async fn revoke_admin_selected_mcp_session_bindings(
        &self,
        proxy_session_ids: &[String],
        reason: &str,
    ) -> Result<i64, ProxyError> {
        if proxy_session_ids.is_empty() {
            return Ok(0);
        }

        let now = self.backend_time.now_ts();
        let mut builder = QueryBuilder::<Sqlite>::new(
            "UPDATE mcp_sessions SET revoked_at = ",
        );
        builder.push_bind(now);
        builder.push(", revoke_reason = ");
        builder.push_bind(reason);
        builder.push(", updated_at = ");
        builder.push_bind(now);
        builder.push(" WHERE gateway_mode = ");
        builder.push_bind(MCP_GATEWAY_MODE_UPSTREAM);
        builder.push(" AND revoked_at IS NULL AND expires_at > ");
        builder.push_bind(now);
        builder.push(" AND proxy_session_id IN (");
        {
            let mut separated = builder.separated(", ");
            for proxy_session_id in proxy_session_ids {
                separated.push_bind(proxy_session_id);
            }
        }
        builder.push(")");

        Ok(builder.build().execute(&self.pool).await?.rows_affected() as i64)
    }

    pub(crate) async fn revoke_admin_filtered_mcp_session_bindings(
        &self,
        query: &AdminMcpSessionBindingsQuery,
        reason: &str,
    ) -> Result<i64, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut builder = QueryBuilder::<Sqlite>::new(
            "UPDATE mcp_sessions SET revoked_at = ",
        );
        builder.push_bind(now);
        builder.push(", revoke_reason = ");
        builder.push_bind(reason);
        builder.push(", updated_at = ");
        builder.push_bind(now);
        builder.push(" WHERE ");
        Self::push_admin_mcp_session_binding_filters(&mut builder, query, now, true);
        Ok(builder.build().execute(&self.pool).await?.rows_affected() as i64)
    }

    // -----------------------------------------------------------------------
    // Provider-aware session stamping + resolution (plan todo 15; amendment
    // M6). Additive functions only: every pre-existing statement above stays
    // byte-identical, and legacy/Tavily rows keep their NULL provider tag
    // (COALESCE(provider, 'tavily') below preserves the historical default).
    // -----------------------------------------------------------------------

    pub(crate) async fn create_or_replace_mcp_session_with_provider(
        &self,
        binding: &McpSessionBinding,
        provider: &str,
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
                provider,
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
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(proxy_session_id) DO UPDATE SET
                upstream_session_id = excluded.upstream_session_id,
                upstream_key_id = excluded.upstream_key_id,
                auth_token_id = excluded.auth_token_id,
                user_id = excluded.user_id,
                protocol_version = excluded.protocol_version,
                last_event_id = excluded.last_event_id,
                gateway_mode = excluded.gateway_mode,
                experiment_variant = excluded.experiment_variant,
                provider = excluded.provider,
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
        .bind(provider)
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

    /// Provider-scoped active-session resolution (M6): a session created on
    /// another provider surface never resolves here. Legacy NULL tags read as
    /// `tavily` so pre-provider rows stay resolvable on the Tavily surface.
    pub(crate) async fn get_active_mcp_session_for_provider(
        &self,
        proxy_session_id: &str,
        provider: &str,
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
              AND COALESCE(provider, 'tavily') = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(proxy_session_id)
        .bind(provider)
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

    /// Raw provider tag of a session row (None tag = legacy/Tavily default).
    /// Outer Option distinguishes "row exists" from "no row".
    pub(crate) async fn mcp_session_provider_tag(
        &self,
        proxy_session_id: &str,
    ) -> Result<Option<Option<String>>, ProxyError> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT provider FROM mcp_sessions WHERE proxy_session_id = ? LIMIT 1",
        )
        .bind(proxy_session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    /// Provider-scoped variant of `has_active_non_rebalance_mcp_session_for_token`
    /// (M6): sessions of other providers never suppress this provider's
    /// session-mode gating.
    pub(crate) async fn has_active_non_rebalance_mcp_session_for_token_in_provider(
        &self,
        token_id: &str,
        provider: &str,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let exists: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM mcp_sessions
                WHERE auth_token_id = ?
                  AND COALESCE(provider, 'tavily') = ?
                  AND gateway_mode <> ?
                  AND revoked_at IS NULL
                  AND expires_at > ?
            )
            "#,
        )
        .bind(token_id)
        .bind(provider)
        .bind(MCP_GATEWAY_MODE_REBALANCE)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists != 0)
    }

    /// Provider-scoped variant of `get_latest_active_mcp_session_for_token`
    /// (M6): a serpapi passthrough session never shadows the Tavily
    /// stateless-rebalance lookup (and vice versa).
    pub(crate) async fn get_latest_active_mcp_session_for_token_in_provider(
        &self,
        token_id: &str,
        provider: &str,
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
              AND COALESCE(provider, 'tavily') = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            ORDER BY updated_at DESC, expires_at DESC
            LIMIT 1
            "#,
        )
        .bind(token_id)
        .bind(provider)
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
}
