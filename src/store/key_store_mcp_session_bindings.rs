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
}
