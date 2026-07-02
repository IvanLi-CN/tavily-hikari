impl KeyStore {
    fn legacy_request_logs_select_exprs(
        source_columns: &HashSet<String>,
    ) -> Vec<(&'static str, String)> {
        let has = |column: &str| source_columns.contains(column);
        let source = |column: &str| format!("legacy.{column}");
        let nullable_source = |column: &str| {
            if has(column) {
                source(column)
            } else {
                "NULL".to_string()
            }
        };

        vec![
            ("id", "legacy.id".to_string()),
            (
                "api_key_id",
                if has("api_key_id") {
                    source("api_key_id")
                } else if has("api_key") {
                    "(SELECT id FROM api_keys WHERE api_keys.api_key = legacy.api_key LIMIT 1)"
                        .to_string()
                } else {
                    "NULL".to_string()
                },
            ),
            ("auth_token_id", nullable_source("auth_token_id")),
            ("request_user_id", nullable_source("request_user_id")),
            ("method", source("method")),
            ("path", source("path")),
            ("query", nullable_source("query")),
            ("status_code", nullable_source("status_code")),
            ("tavily_status_code", nullable_source("tavily_status_code")),
            ("error_message", nullable_source("error_message")),
            (
                "result_status",
                if has("result_status") {
                    source("result_status")
                } else {
                    "'unknown'".to_string()
                },
            ),
            ("request_kind_key", nullable_source("request_kind_key")),
            ("request_kind_label", nullable_source("request_kind_label")),
            ("request_kind_detail", nullable_source("request_kind_detail")),
            ("counts_business_quota", nullable_source("counts_business_quota")),
            ("business_credits", nullable_source("business_credits")),
            ("failure_kind", nullable_source("failure_kind")),
            (
                "key_effect_code",
                if has("key_effect_code") {
                    source("key_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            ("key_effect_summary", nullable_source("key_effect_summary")),
            (
                "binding_effect_code",
                if has("binding_effect_code") {
                    source("binding_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            (
                "binding_effect_summary",
                nullable_source("binding_effect_summary"),
            ),
            (
                "selection_effect_code",
                if has("selection_effect_code") {
                    source("selection_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            (
                "selection_effect_summary",
                nullable_source("selection_effect_summary"),
            ),
            ("gateway_mode", nullable_source("gateway_mode")),
            ("experiment_variant", nullable_source("experiment_variant")),
            ("proxy_session_id", nullable_source("proxy_session_id")),
            ("routing_subject_hash", nullable_source("routing_subject_hash")),
            ("upstream_operation", nullable_source("upstream_operation")),
            ("fallback_reason", nullable_source("fallback_reason")),
            ("request_body", nullable_source("request_body")),
            ("response_body", nullable_source("response_body")),
            ("request_body_bytes", nullable_source("request_body_bytes")),
            ("response_body_bytes", nullable_source("response_body_bytes")),
            ("request_body_sha256", nullable_source("request_body_sha256")),
            ("response_body_sha256", nullable_source("response_body_sha256")),
            ("body_retention_days", nullable_source("body_retention_days")),
            ("body_retention_profile", nullable_source("body_retention_profile")),
            ("body_cleaned_reason", nullable_source("body_cleaned_reason")),
            ("body_cleaned_at", nullable_source("body_cleaned_at")),
            ("forwarded_headers", nullable_source("forwarded_headers")),
            ("dropped_headers", nullable_source("dropped_headers")),
            ("remote_addr", nullable_source("remote_addr")),
            ("client_ip", nullable_source("client_ip")),
            ("client_ip_source", nullable_source("client_ip_source")),
            (
                "client_ip_trusted",
                if has("client_ip_trusted") {
                    source("client_ip_trusted")
                } else {
                    "0".to_string()
                },
            ),
            ("ip_headers", nullable_source("ip_headers")),
            (
                "visibility",
                if has("visibility") {
                    source("visibility")
                } else {
                    format!("'{}'", REQUEST_LOG_VISIBILITY_VISIBLE)
                },
            ),
            ("created_at", source("created_at")),
        ]
    }

    async fn copy_legacy_request_logs_into_observability(&self) -> Result<(), ProxyError> {
        if !self.main_table_exists("request_logs").await? {
            return Ok(());
        }

        let source_columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('request_logs', 'main')",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
        if source_columns.is_empty() {
            return Ok(());
        }

        let select_exprs = Self::legacy_request_logs_select_exprs(&source_columns);
        let target_columns = select_exprs
            .iter()
            .map(|(column, _)| *column)
            .collect::<Vec<_>>();
        let source_exprs = select_exprs
            .iter()
            .map(|(_, expr)| expr.as_str())
            .collect::<Vec<_>>();

        let copy_sql = format!(
            r#"
            INSERT INTO observability.request_logs ({})
            SELECT {}
            FROM main.request_logs AS legacy
            WHERE NOT EXISTS (
                SELECT 1
                FROM observability.request_logs AS obs
                WHERE obs.id = legacy.id
            )
            ORDER BY legacy.id ASC
            "#,
            target_columns.join(", "),
            source_exprs.join(", "),
        );
        sqlx::query(&copy_sql).execute(&self.pool).await?;
        Ok(())
    }
}
