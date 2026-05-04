impl KeyStore {
    async fn migrate_log_effect_buckets(&self) -> Result<(), ProxyError> {
        let binding_codes = [
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND,
        ];
        let selection_codes = [
            KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED,
            KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED,
            KEY_EFFECT_MCP_SESSION_INIT_PRESSURE_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED,
        ];
        debug_assert!(
            binding_codes
                .iter()
                .all(|code| is_binding_effect_code(code))
        );
        debug_assert!(
            selection_codes
                .iter()
                .all(|code| is_selection_effect_code(code))
        );
        debug_assert!(
            [
                KEY_EFFECT_NONE,
                KEY_EFFECT_QUARANTINED,
                KEY_EFFECT_MARKED_EXHAUSTED,
                KEY_EFFECT_RESTORED_ACTIVE,
                "cleared_quarantine",
                KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET,
                KEY_EFFECT_MCP_SESSION_RETRY_WAITED,
                KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED,
            ]
            .iter()
            .all(|code| is_key_effect_code(code))
        );

        for table in ["request_logs", "auth_token_logs"] {
            let binding_sql = format!(
                "UPDATE {table}
                 SET binding_effect_code = key_effect_code,
                     binding_effect_summary = key_effect_summary,
                     key_effect_code = 'none',
                     key_effect_summary = NULL
                 WHERE key_effect_code IN (?, ?, ?)
                   AND (binding_effect_code IS NULL OR TRIM(binding_effect_code) = '' OR binding_effect_code = 'none')
                   AND (selection_effect_code IS NULL OR TRIM(selection_effect_code) = '' OR selection_effect_code = 'none')"
            );
            sqlx::query(&binding_sql)
                .bind(binding_codes[0])
                .bind(binding_codes[1])
                .bind(binding_codes[2])
                .execute(&self.pool)
                .await?;

            let selection_sql = format!(
                "UPDATE {table}
                 SET selection_effect_code = key_effect_code,
                     selection_effect_summary = key_effect_summary,
                     key_effect_code = 'none',
                     key_effect_summary = NULL
                 WHERE key_effect_code IN (?, ?, ?, ?, ?, ?)
                   AND (binding_effect_code IS NULL OR TRIM(binding_effect_code) = '' OR binding_effect_code = 'none')
                   AND (selection_effect_code IS NULL OR TRIM(selection_effect_code) = '' OR selection_effect_code = 'none')"
            );
            sqlx::query(&selection_sql)
                .bind(selection_codes[0])
                .bind(selection_codes[1])
                .bind(selection_codes[2])
                .bind(selection_codes[3])
                .bind(selection_codes[4])
                .bind(selection_codes[5])
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    fn push_request_logs_scope<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        scoped_key_id: Option<&'a str>,
        since: Option<i64>,
    ) -> bool {
        builder.push(" WHERE visibility = ");
        builder.push_bind(REQUEST_LOG_VISIBILITY_VISIBLE);
        let mut has_where = true;
        if let Some(key_id) = scoped_key_id {
            builder.push(" AND api_key_id = ");
            builder.push_bind(key_id);
            has_where = true;
        }
        if let Some(since) = since {
            builder.push(if has_where {
                " AND created_at >= "
            } else {
                " WHERE created_at >= "
            });
            builder.push_bind(since);
            has_where = true;
        }
        has_where
    }

    fn push_request_logs_filters<'a, 'b>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        filters: RequestLogFilterParams<'a, 'b>,
    ) {
        let RequestLogFilterParams {
            request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            key_id,
            stored_request_kind_sql,
            legacy_request_kind_predicate_sql,
            legacy_request_kind_sql,
            mut has_where,
        } = filters;
        if let Some(result_status) = result_status {
            builder.push(if has_where {
                " AND result_status = "
            } else {
                " WHERE result_status = "
            });
            builder.push_bind(result_status.to_string());
            has_where = true;
        }
        if let Some(key_effect_code) = key_effect_code {
            builder.push(if has_where {
                " AND key_effect_code = "
            } else {
                " WHERE key_effect_code = "
            });
            builder.push_bind(key_effect_code.to_string());
            has_where = true;
        }
        if let Some(binding_effect_code) = binding_effect_code {
            builder.push(if has_where {
                " AND binding_effect_code = "
            } else {
                " WHERE binding_effect_code = "
            });
            builder.push_bind(binding_effect_code.to_string());
            has_where = true;
        }
        if let Some(selection_effect_code) = selection_effect_code {
            builder.push(if has_where {
                " AND selection_effect_code = "
            } else {
                " WHERE selection_effect_code = "
            });
            builder.push_bind(selection_effect_code.to_string());
            has_where = true;
        }
        if let Some(auth_token_id) = auth_token_id {
            builder.push(if has_where {
                " AND auth_token_id = "
            } else {
                " WHERE auth_token_id = "
            });
            builder.push_bind(auth_token_id.to_string());
            has_where = true;
        }
        if let Some(key_id) = key_id {
            builder.push(if has_where {
                " AND api_key_id = "
            } else {
                " WHERE api_key_id = "
            });
            builder.push_bind(key_id.to_string());
            has_where = true;
        }
        if !request_kinds.is_empty() {
            builder.push(if has_where { " AND " } else { " WHERE " });
            Self::push_request_kind_filter_clause(
                builder,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql,
                legacy_request_kind_sql,
                request_kinds,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_token_logs_catalog_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        token_id: &'a str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'a>,
        stored_request_kind_sql: &'a str,
        legacy_request_kind_predicate_sql: &'a str,
        legacy_request_kind_sql: &'a str,
        stored_operational_class_case_sql: &'a str,
        legacy_operational_class_case_sql: &'a str,
        stored_result_bucket_sql: &'a str,
        legacy_result_bucket_sql: &'a str,
    ) {
        let normalized_request_kinds = Self::normalize_request_kind_filters(filters.request_kinds);
        builder.push(" WHERE token_id = ");
        builder.push_bind(token_id);
        builder.push(" AND created_at >= ");
        builder.push_bind(since);
        if let Some(until) = until {
            builder.push(" AND created_at < ");
            builder.push_bind(until);
        }
        if let Some(key_effect_code) = filters.key_effect_code {
            builder.push(" AND key_effect_code = ");
            builder.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = filters.binding_effect_code {
            builder.push(" AND binding_effect_code = ");
            builder.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = filters.selection_effect_code {
            builder.push(" AND selection_effect_code = ");
            builder.push_bind(selection_effect_code);
        }
        if let Some(key_id) = filters.key_id {
            builder.push(" AND api_key_id = ");
            builder.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            builder.push(" AND ");
            Self::push_request_kind_filter_clause(
                builder,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql,
                legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = filters.result_status {
            builder.push(" AND ");
            Self::push_result_bucket_filter_clause(
                builder,
                result_status,
                legacy_request_kind_predicate_sql,
                stored_result_bucket_sql,
                legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = filters.operational_class {
            builder.push(" AND ");
            Self::push_operational_class_filter_clause(
                builder,
                operational_class,
                legacy_request_kind_predicate_sql,
                stored_operational_class_case_sql,
                legacy_operational_class_case_sql,
            );
        }
    }

    fn request_log_catalog_bucket_start_sql(created_at_sql: &str) -> String {
        created_at_sql.to_string()
    }

    fn clamp_request_logs_rollup_since(since: Option<i64>) -> Option<i64> {
        let retention_since =
            request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days());
        Some(since.unwrap_or(retention_since).max(retention_since))
    }

    fn request_log_catalog_rollup_exprs(prefix: &str) -> Vec<String> {
        let col = |name: &str| -> String {
            if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}{name}")
            }
        };
        let stored_request_kind_sql = col("request_kind_key");
        let effective_request_kind_sql = stored_request_kind_sql.clone();
        let counts_business_quota_sql =
            request_log_counts_business_quota_sql(&effective_request_kind_sql, &col("request_body"));
        let operational_class_sql = request_log_operational_class_case_sql(
            &effective_request_kind_sql,
            &counts_business_quota_sql,
            &col("result_status"),
            &format!("COALESCE({}, '')", col("failure_kind")),
        );
        let result_bucket_sql = result_bucket_case_sql(&operational_class_sql, &col("result_status"));
        let request_kind_label_sql = format!(
            "COALESCE(NULLIF(TRIM({}), ''), {})",
            col("request_kind_label"),
            canonical_request_kind_label_sql(&effective_request_kind_sql)
        );

        vec![
            Self::request_log_catalog_bucket_start_sql(&col("created_at")),
            format!("COALESCE(NULLIF(TRIM({effective_request_kind_sql}), ''), 'unknown')"),
            format!("COALESCE(NULLIF(TRIM({request_kind_label_sql}), ''), 'Unknown')"),
            format!("COALESCE(NULLIF(TRIM({result_bucket_sql}), ''), 'unknown')"),
            format!(
                "COALESCE(NULLIF(TRIM({}), ''), '{}')",
                col("key_effect_code"),
                KEY_EFFECT_NONE
            ),
            format!(
                "COALESCE(NULLIF(TRIM({}), ''), '{}')",
                col("binding_effect_code"),
                KEY_EFFECT_NONE
            ),
            format!(
                "COALESCE(NULLIF(TRIM({}), ''), '{}')",
                col("selection_effect_code"),
                KEY_EFFECT_NONE
            ),
            format!("COALESCE(NULLIF(TRIM({}), ''), '')", col("auth_token_id")),
            format!("COALESCE(NULLIF(TRIM({}), ''), '')", col("api_key_id")),
            format!("COALESCE(NULLIF(TRIM({operational_class_sql}), ''), 'other')"),
        ]
    }

    fn request_log_catalog_rollup_columns() -> &'static str {
        "bucket_start, request_kind_key, request_kind_label, result_bucket, key_effect_code, binding_effect_code, selection_effect_code, auth_token_id, api_key_id, operational_class"
    }

    fn request_log_catalog_rollup_has_canonical_kind(prefix: &str) -> String {
        canonical_request_kind_stored_predicate_sql(&format!("{prefix}request_kind_key"))
    }

    fn request_log_catalog_rollup_pk_match(exprs: &[String]) -> String {
        [
            ("bucket_start", &exprs[0]),
            ("request_kind_key", &exprs[1]),
            ("request_kind_label", &exprs[2]),
            ("result_bucket", &exprs[3]),
            ("key_effect_code", &exprs[4]),
            ("binding_effect_code", &exprs[5]),
            ("selection_effect_code", &exprs[6]),
            ("auth_token_id", &exprs[7]),
            ("api_key_id", &exprs[8]),
            ("operational_class", &exprs[9]),
        ]
        .into_iter()
        .map(|(column, expr)| format!("{column} = {expr}"))
        .collect::<Vec<_>>()
        .join(" AND ")
    }

    fn push_request_log_catalog_rollup_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        scoped_key_id: Option<&'a str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'a>,
    ) {
        let normalized_request_kinds = Self::normalize_request_kind_filters(filters.request_kinds);
        builder.push(" WHERE 1 = 1");
        if let Some(since) = since {
            builder.push(" AND bucket_start >= ");
            builder.push_bind(since);
        }
        if let Some(scoped_key_id) = scoped_key_id {
            builder.push(" AND api_key_id = ");
            builder.push_bind(scoped_key_id);
        }
        if let Some(result_status) = filters.result_status {
            builder.push(" AND result_bucket = ");
            builder.push_bind(result_status);
        }
        if let Some(key_effect_code) = filters.key_effect_code {
            builder.push(" AND key_effect_code = ");
            builder.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = filters.binding_effect_code {
            builder.push(" AND binding_effect_code = ");
            builder.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = filters.selection_effect_code {
            builder.push(" AND selection_effect_code = ");
            builder.push_bind(selection_effect_code);
        }
        if let Some(auth_token_id) = filters.auth_token_id {
            builder.push(" AND auth_token_id = ");
            builder.push_bind(auth_token_id);
        }
        if let Some(key_id) = filters.key_id {
            builder.push(" AND api_key_id = ");
            builder.push_bind(key_id);
        }
        if let Some(operational_class) = filters.operational_class {
            builder.push(" AND operational_class = ");
            builder.push_bind(operational_class);
        }
        if !normalized_request_kinds.is_empty() {
            builder.push(" AND request_kind_key IN (");
            let mut separated = builder.separated(", ");
            for request_kind in normalized_request_kinds {
                separated.push_bind(request_kind);
            }
            builder.push(")");
        }
    }

    async fn fetch_request_logs_rollup_total(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<i64, ProxyError> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT COALESCE(SUM(request_count), 0) FROM request_log_catalog_rollups",
        );
        Self::push_request_log_catalog_rollup_filters(&mut query, scoped_key_id, since, filters);
        query
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::from)
    }

    async fn fetch_request_log_request_kind_options(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        type RequestKindOptionRow = (String, String, i64);
        let mut legacy_query = QueryBuilder::<Sqlite>::new(
            "SELECT request_kind_key, request_kind_label, SUM(request_count) AS request_count FROM request_log_catalog_rollups",
        );
        Self::push_request_log_catalog_rollup_filters(
            &mut legacy_query,
            scoped_key_id,
            since,
            filters,
        );
        legacy_query.push(" GROUP BY 1, 2");

        let rows = legacy_query
            .build_query_as::<RequestKindOptionRow>()
            .fetch_all(&self.pool)
            .await?;
        let mut options_by_key = BTreeMap::<String, (String, i64)>::new();
        for (key, label, count) in rows {
            match options_by_key.get_mut(&key) {
                Some((current_label, current_count))
                    if prefer_request_kind_label(current_label, &label) =>
                {
                    *current_label = label;
                    *current_count += count;
                }
                Some((_, current_count)) => {
                    *current_count += count;
                }
                None => {
                    options_by_key.insert(key, (label, count));
                }
            }
        }

        Ok(options_by_key
            .into_iter()
            .map(|(key, (label, count))| TokenRequestKindOption {
                protocol_group: token_request_kind_protocol_group(&key).to_string(),
                billing_group: token_request_kind_billing_group(&key).to_string(),
                key,
                label,
                count,
            })
            .collect())
    }

    async fn fetch_request_log_facet_options(
        &self,
        column_expr: &str,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        require_non_empty: bool,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let column_expr = match column_expr {
            "key_effect_code" => "key_effect_code",
            "binding_effect_code" => "binding_effect_code",
            "selection_effect_code" => "selection_effect_code",
            "auth_token_id" => "auth_token_id",
            "api_key_id" => "api_key_id",
            _ => unreachable!("unsupported request log rollup facet column"),
        };
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {column_expr} AS value, SUM(request_count) AS count FROM request_log_catalog_rollups"
        ));
        Self::push_request_log_catalog_rollup_filters(&mut query, scoped_key_id, since, filters);
        if require_non_empty {
            query.push(" AND TRIM(");
            query.push(column_expr);
            query.push(") <> ''");
        }
        query.push(" GROUP BY 1 ORDER BY count DESC, value ASC");

        let rows = query.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| -> Result<LogFacetOption, sqlx::Error> {
                Ok(LogFacetOption {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    async fn fetch_request_log_result_facet_options(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "
            SELECT
                result_bucket AS value,
                SUM(request_count) AS count
            FROM request_log_catalog_rollups
            ",
        );
        Self::push_request_log_catalog_rollup_filters(&mut query, scoped_key_id, since, filters);
        query.push(" GROUP BY 1 ORDER BY count DESC, value ASC");

        let rows = query.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| -> Result<LogFacetOption, sqlx::Error> {
                Ok(LogFacetOption {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_request_logs_catalog(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        include_token_facets: bool,
        include_key_facets: bool,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<RequestLogsCatalog, ProxyError> {
        let since = Self::clamp_request_logs_rollup_since(since);
        let cache_key = Self::request_logs_catalog_filters_are_empty(filters).then(|| {
            Self::request_logs_catalog_cache_key(
                scoped_key_id,
                since,
                include_token_facets,
                include_key_facets,
            )
        });
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(cached) = self.cached_request_logs_catalog(cache_key).await
        {
            return Ok(cached);
        }

        let request_kind_options = self
            .fetch_request_log_request_kind_options(scoped_key_id, since, filters)
            .await?;
        let results = self
            .fetch_request_log_result_facet_options(scoped_key_id, since, filters)
            .await?;
        let key_effects = self
            .fetch_request_log_facet_options(
                "key_effect_code",
                scoped_key_id,
                since,
                false,
                filters,
            )
            .await?;
        let binding_effects = self
            .fetch_request_log_facet_options(
                "binding_effect_code",
                scoped_key_id,
                since,
                false,
                filters,
            )
            .await?;
        let selection_effects = self
            .fetch_request_log_facet_options(
                "selection_effect_code",
                scoped_key_id,
                since,
                false,
                filters,
            )
            .await?;
        let tokens = if include_token_facets {
            self.fetch_request_log_facet_options(
                "auth_token_id",
                scoped_key_id,
                since,
                true,
                filters,
            )
            .await?
        } else {
            Vec::new()
        };
        let keys = if include_key_facets {
            self.fetch_request_log_facet_options("api_key_id", scoped_key_id, since, true, filters)
                .await?
        } else {
            Vec::new()
        };

        let catalog = RequestLogsCatalog {
            retention_days: effective_request_logs_retention_days(),
            request_kind_options,
            facets: RequestLogPageFacets {
                results,
                key_effects,
                binding_effects,
                selection_effects,
                tokens,
                keys,
            },
        };
        if let Some(cache_key) = cache_key {
            self.cache_request_logs_catalog(cache_key, &catalog).await;
        }
        Ok(catalog)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_request_logs_cursor_page(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        page_size: i64,
    ) -> Result<RequestLogsCursorPage, ProxyError> {
        let page_size = page_size.clamp(1, 200);
        let query_limit = page_size + 1;
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key");
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let stored_counts_business_quota_sql =
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body");
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body");
        let legacy_operational_class_case_sql = request_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            &legacy_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");
        let effective_counts_business_quota_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_counts_business_quota_sql} ELSE {stored_counts_business_quota_sql} END"
        );
        let effective_non_billable_mcp_sql =
            token_request_kind_non_billable_mcp_sql(&effective_request_kind_sql);
        let effective_request_kind_protocol_group_sql = format!(
            "CASE WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) LIKE 'mcp:%' THEN 'mcp' ELSE 'api' END"
        );
        let effective_request_kind_billing_group_sql = format!(
            "
            CASE
                WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) IN (
                    'api:research-result',
                    'api:usage',
                    'api:unknown-path'
                ) THEN 'non_billable'
                WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) = 'mcp:batch'
                    AND {effective_counts_business_quota_sql} = 0
                    THEN 'non_billable'
                WHEN {effective_non_billable_mcp_sql} THEN 'non_billable'
                ELSE 'billable'
            END
            "
        );
        let effective_operational_class_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_operational_class_case_sql} ELSE {stored_operational_class_case_sql} END"
        );

        let mut items_query = QueryBuilder::<Sqlite>::new(format!(
            r#"
            SELECT
                id,
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                {effective_request_kind_sql} AS request_kind_key,
                {effective_request_kind_label_sql} AS request_kind_label,
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                binding_effect_code,
                binding_effect_summary,
                selection_effect_code,
                selection_effect_summary,
                gateway_mode,
                experiment_variant,
                proxy_session_id,
                routing_subject_hash,
                upstream_operation,
                fallback_reason,
                NULL AS request_body,
                NULL AS response_body,
                forwarded_headers,
                dropped_headers,
                {effective_operational_class_sql} AS operational_class,
                {effective_request_kind_protocol_group_sql} AS request_kind_protocol_group,
                {effective_request_kind_billing_group_sql} AS request_kind_billing_group,
                created_at
            FROM request_logs
            "#
        ));
        let has_where = Self::push_request_logs_scope(&mut items_query, scoped_key_id, since);
        Self::push_request_logs_filters(
            &mut items_query,
            RequestLogFilterParams {
                request_kinds: &normalized_request_kinds,
                result_status: None,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                key_id,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql: &legacy_request_kind_predicate_sql,
                legacy_request_kind_sql: &legacy_request_kind_sql,
                has_where,
            },
        );
        if let Some(result_status) = result_status {
            items_query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut items_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            items_query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut items_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        Self::push_desc_cursor_clause(
            &mut items_query,
            "created_at",
            "id",
            cursor,
            direction,
            true,
        );
        match direction {
            RequestLogsCursorDirection::Older => {
                items_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
            }
            RequestLogsCursorDirection::Newer => {
                items_query.push(" ORDER BY created_at ASC, id ASC LIMIT ");
            }
        }
        items_query.push_bind(query_limit);

        let mut rows = items_query.build().fetch_all(&self.pool).await?;
        let has_more = rows.len() as i64 > page_size;
        if has_more {
            rows.truncate(page_size as usize);
        }
        if matches!(direction, RequestLogsCursorDirection::Newer) {
            rows.reverse();
        }
        let items = rows
            .into_iter()
            .map(Self::map_request_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        let has_older = match direction {
            RequestLogsCursorDirection::Older => has_more,
            RequestLogsCursorDirection::Newer => cursor.is_some(),
        };
        let has_newer = match direction {
            RequestLogsCursorDirection::Older => cursor.is_some(),
            RequestLogsCursorDirection::Newer => has_more,
        };
        let recovery_cursor = cursor.cloned();

        Ok(RequestLogsCursorPage {
            next_cursor: has_older
                .then(|| {
                    items
                        .last()
                        .map(Self::request_logs_cursor_for_record)
                        .or_else(|| {
                            matches!(direction, RequestLogsCursorDirection::Newer)
                                .then(|| recovery_cursor.clone())
                                .flatten()
                        })
                })
                .flatten(),
            prev_cursor: has_newer
                .then(|| {
                    items
                        .first()
                        .map(Self::request_logs_cursor_for_record)
                        .or_else(|| {
                            matches!(direction, RequestLogsCursorDirection::Older)
                                .then(|| recovery_cursor.clone())
                                .flatten()
                        })
                })
                .flatten(),
            items,
            page_size,
            has_older,
            has_newer,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_request_logs_page(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
        include_token_facets: bool,
        include_key_facets: bool,
        include_bodies: bool,
    ) -> Result<RequestLogsPage, ProxyError> {
        let since = Self::clamp_request_logs_rollup_since(since);
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key");
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let stored_counts_business_quota_sql =
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body");
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body");
        let legacy_operational_class_case_sql = request_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            &legacy_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");
        let effective_counts_business_quota_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_counts_business_quota_sql} ELSE {stored_counts_business_quota_sql} END"
        );
        let effective_non_billable_mcp_sql =
            token_request_kind_non_billable_mcp_sql(&effective_request_kind_sql);
        let effective_request_kind_protocol_group_sql = format!(
            "CASE WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) LIKE 'mcp:%' THEN 'mcp' ELSE 'api' END"
        );
        let effective_request_kind_billing_group_sql = format!(
            "
            CASE
                WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) IN (
                    'api:research-result',
                    'api:usage',
                    'api:unknown-path'
                ) THEN 'non_billable'
                WHEN LOWER(TRIM(COALESCE({effective_request_kind_sql}, ''))) = 'mcp:batch'
                    AND {effective_counts_business_quota_sql} = 0
                    THEN 'non_billable'
                WHEN {effective_non_billable_mcp_sql} THEN 'non_billable'
                ELSE 'billable'
            END
            "
        );
        let effective_operational_class_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_operational_class_case_sql} ELSE {stored_operational_class_case_sql} END"
        );

        let total_filters = RequestLogsCatalogFilters {
            request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            key_id,
            operational_class,
        };
        let total = self
            .fetch_request_logs_rollup_total(scoped_key_id, since, total_filters)
            .await?;

        let request_body_select = if include_bodies {
            "request_body"
        } else {
            "NULL AS request_body"
        };
        let response_body_select = if include_bodies {
            "response_body"
        } else {
            "NULL AS response_body"
        };
        let mut items_query = QueryBuilder::<Sqlite>::new(format!(
            r#"
            SELECT
                id,
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                {effective_request_kind_sql} AS request_kind_key,
                {effective_request_kind_label_sql} AS request_kind_label,
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                binding_effect_code,
                binding_effect_summary,
                selection_effect_code,
                selection_effect_summary,
                gateway_mode,
                experiment_variant,
                proxy_session_id,
                routing_subject_hash,
                upstream_operation,
                fallback_reason,
                {request_body_select},
                {response_body_select},
                forwarded_headers,
                dropped_headers,
                {effective_operational_class_sql} AS operational_class,
                {effective_request_kind_protocol_group_sql} AS request_kind_protocol_group,
                {effective_request_kind_billing_group_sql} AS request_kind_billing_group,
                created_at
            FROM request_logs
            "#
        ));
        let has_where = Self::push_request_logs_scope(&mut items_query, scoped_key_id, since);
        Self::push_request_logs_filters(
            &mut items_query,
            RequestLogFilterParams {
                request_kinds: &normalized_request_kinds,
                result_status: None,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                key_id,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql: &legacy_request_kind_predicate_sql,
                legacy_request_kind_sql: &legacy_request_kind_sql,
                has_where,
            },
        );
        if let Some(result_status) = result_status {
            items_query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut items_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            items_query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut items_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        items_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        items_query.push_bind(per_page);
        items_query.push(" OFFSET ");
        items_query.push_bind(offset);
        let rows = items_query.build().fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(Self::map_request_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        let empty_filters = RequestLogsCatalogFilters {
            request_kinds: &[],
            result_status: None,
            key_effect_code: None,
            binding_effect_code: None,
            selection_effect_code: None,
            auth_token_id: None,
            key_id: None,
            operational_class: None,
        };
        let request_kind_options = self
            .fetch_request_log_request_kind_options(scoped_key_id, since, empty_filters)
            .await?;
        let results = self
            .fetch_request_log_result_facet_options(scoped_key_id, since, empty_filters)
            .await?;
        let key_effects = self
            .fetch_request_log_facet_options(
                "key_effect_code",
                scoped_key_id,
                since,
                false,
                empty_filters,
            )
            .await?;
        let binding_effects = self
            .fetch_request_log_facet_options(
                "binding_effect_code",
                scoped_key_id,
                since,
                false,
                empty_filters,
            )
            .await?;
        let selection_effects = self
            .fetch_request_log_facet_options(
                "selection_effect_code",
                scoped_key_id,
                since,
                false,
                empty_filters,
            )
            .await?;
        let tokens = if include_token_facets {
            self.fetch_request_log_facet_options(
                "auth_token_id",
                scoped_key_id,
                since,
                true,
                empty_filters,
            )
            .await?
        } else {
            Vec::new()
        };
        let keys = if include_key_facets {
            self.fetch_request_log_facet_options(
                "api_key_id",
                scoped_key_id,
                since,
                true,
                empty_filters,
            )
            .await?
        } else {
            Vec::new()
        };

        Ok(RequestLogsPage {
            items,
            total,
            request_kind_options,
            facets: RequestLogPageFacets {
                results,
                key_effects,
                binding_effects,
                selection_effects,
                tokens,
                keys,
            },
        })
    }

    pub(crate) async fn ensure_request_log_catalog_rollup_schema(
        &self,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_log_catalog_rollups (
                bucket_start INTEGER NOT NULL,
                request_kind_key TEXT NOT NULL,
                request_kind_label TEXT NOT NULL,
                result_bucket TEXT NOT NULL,
                key_effect_code TEXT NOT NULL,
                binding_effect_code TEXT NOT NULL,
                selection_effect_code TEXT NOT NULL,
                auth_token_id TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                operational_class TEXT NOT NULL,
                request_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (
                    bucket_start,
                    request_kind_key,
                    request_kind_label,
                    result_bucket,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    auth_token_id,
                    api_key_id,
                    operational_class
                )
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        for sql in [
            r#"CREATE INDEX IF NOT EXISTS idx_request_log_catalog_rollups_kind_time
               ON request_log_catalog_rollups(request_kind_key, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_request_log_catalog_rollups_result_time
               ON request_log_catalog_rollups(result_bucket, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_request_log_catalog_rollups_token_time
               ON request_log_catalog_rollups(auth_token_id, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_request_log_catalog_rollups_key_time
               ON request_log_catalog_rollups(api_key_id, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_request_log_catalog_rollups_operational_time
               ON request_log_catalog_rollups(operational_class, bucket_start DESC)"#,
        ] {
            sqlx::query(sql).execute(&self.pool).await?;
        }

        let canonical_new_request_kind_sql =
            request_log_request_kind_key_sql("NEW.path", "NEW.request_body", "NEW.request_kind_key");
        let canonical_new_request_kind_label_sql =
            canonical_request_kind_label_sql(&canonical_new_request_kind_sql);
        let canonical_new_request_kind_detail_sql = format!(
            "
            CASE
                WHEN LOWER(COALESCE(NEW.path, '')) LIKE '/mcp/%' THEN NEW.path
                WHEN LOWER(TRIM(COALESCE(NEW.request_kind_key, ''))) LIKE 'mcp:tool:%'
                    THEN SUBSTR(TRIM(NEW.request_kind_key), 10)
                WHEN {canonical_new_request_kind_sql} = 'mcp:unknown-payload' THEN NEW.path
                ELSE NULL
            END
            "
        );
        let legacy_new_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql("NEW.request_kind_key");
        let legacy_row_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql("request_kind_key");
        let canonical_insert_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_canonical_request_kind_insert
            AFTER INSERT ON request_logs
            WHEN {legacy_new_request_kind_predicate_sql}
            BEGIN
                UPDATE request_logs
                SET request_kind_key = COALESCE(NULLIF(TRIM({canonical_new_request_kind_sql}), ''), 'api:unknown-path'),
                    request_kind_label = COALESCE(NULLIF(TRIM({canonical_new_request_kind_label_sql}), ''), 'Unknown'),
                    request_kind_detail = COALESCE(NULLIF(TRIM(NEW.request_kind_detail), ''), {canonical_new_request_kind_detail_sql})
                WHERE id = NEW.id
                  AND {legacy_row_request_kind_predicate_sql};
            END
            "#
        );
        sqlx::query(&canonical_insert_trigger)
            .execute(&self.pool)
            .await?;

        let canonical_update_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_canonical_request_kind_update
            AFTER UPDATE OF path, request_body, request_kind_key ON request_logs
            WHEN {legacy_new_request_kind_predicate_sql}
            BEGIN
                UPDATE request_logs
                SET request_kind_key = COALESCE(NULLIF(TRIM({canonical_new_request_kind_sql}), ''), 'api:unknown-path'),
                    request_kind_label = COALESCE(NULLIF(TRIM({canonical_new_request_kind_label_sql}), ''), 'Unknown'),
                    request_kind_detail = COALESCE(NULLIF(TRIM(NEW.request_kind_detail), ''), {canonical_new_request_kind_detail_sql})
                WHERE id = NEW.id
                  AND {legacy_row_request_kind_predicate_sql};
            END
            "#
        );
        sqlx::query(&canonical_update_trigger)
            .execute(&self.pool)
            .await?;

        let insert_exprs = Self::request_log_catalog_rollup_exprs("NEW.");
        let insert_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_catalog_rollup_insert
            AFTER INSERT ON request_logs
            WHEN NEW.visibility = 'visible' AND {}
            BEGIN
                INSERT INTO request_log_catalog_rollups (
                    {},
                    request_count,
                    updated_at
                )
                VALUES (
                    {},
                    1,
                    CAST(strftime('%s', 'now') AS INTEGER)
                )
                ON CONFLICT({}) DO UPDATE SET
                    request_count = request_count + 1,
                    updated_at = excluded.updated_at;
            END
            "#,
            Self::request_log_catalog_rollup_has_canonical_kind("NEW."),
            Self::request_log_catalog_rollup_columns(),
            insert_exprs.join(", "),
            Self::request_log_catalog_rollup_columns(),
        );
        sqlx::query(&insert_trigger).execute(&self.pool).await?;

        let delete_exprs = Self::request_log_catalog_rollup_exprs("OLD.");
        let delete_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_catalog_rollup_delete
            AFTER DELETE ON request_logs
            WHEN OLD.visibility = 'visible' AND {}
            BEGIN
                UPDATE request_log_catalog_rollups
                SET request_count = request_count - 1,
                    updated_at = CAST(strftime('%s', 'now') AS INTEGER)
                WHERE {};
                DELETE FROM request_log_catalog_rollups
                WHERE request_count <= 0;
            END
            "#,
            Self::request_log_catalog_rollup_has_canonical_kind("OLD."),
            Self::request_log_catalog_rollup_pk_match(&delete_exprs),
        );
        sqlx::query(&delete_trigger).execute(&self.pool).await?;

        let update_columns = "
            visibility,
            created_at,
            request_kind_key,
            request_kind_label,
            result_status,
            business_credits,
            failure_kind,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            api_key_id,
            path,
            request_body
        ";
        let update_delete_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_catalog_rollup_update_old
            AFTER UPDATE OF {update_columns} ON request_logs
            WHEN OLD.visibility = 'visible' AND {}
            BEGIN
                UPDATE request_log_catalog_rollups
                SET request_count = request_count - 1,
                    updated_at = CAST(strftime('%s', 'now') AS INTEGER)
                WHERE {};
                DELETE FROM request_log_catalog_rollups
                WHERE request_count <= 0;
            END
            "#,
            Self::request_log_catalog_rollup_has_canonical_kind("OLD."),
            Self::request_log_catalog_rollup_pk_match(&delete_exprs),
        );
        sqlx::query(&update_delete_trigger)
            .execute(&self.pool)
            .await?;

        let update_insert_trigger = format!(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_request_logs_catalog_rollup_update_new
            AFTER UPDATE OF {update_columns} ON request_logs
            WHEN NEW.visibility = 'visible' AND {}
            BEGIN
                INSERT INTO request_log_catalog_rollups (
                    {},
                    request_count,
                    updated_at
                )
                VALUES (
                    {},
                    1,
                    CAST(strftime('%s', 'now') AS INTEGER)
                )
                ON CONFLICT({}) DO UPDATE SET
                    request_count = request_count + 1,
                    updated_at = excluded.updated_at;
            END
            "#,
            Self::request_log_catalog_rollup_has_canonical_kind("NEW."),
            Self::request_log_catalog_rollup_columns(),
            insert_exprs.join(", "),
            Self::request_log_catalog_rollup_columns(),
        );
        sqlx::query(&update_insert_trigger)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub(crate) async fn rebuild_request_log_catalog_rollups(&self) -> Result<(), ProxyError> {
        let since = request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days());
        let exprs = Self::request_log_catalog_rollup_exprs("");
        let canonical_request_kind_predicate_sql =
            canonical_request_kind_stored_predicate_sql("request_kind_key");
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql("request_kind_key");
        let canonicalize_legacy_rows_sql = format!(
            r#"
            UPDATE request_logs
            SET request_kind_key = request_kind_key
            WHERE visibility = 'visible'
              AND created_at >= ?
              AND {legacy_request_kind_predicate_sql}
            "#
        );
        sqlx::query(&canonicalize_legacy_rows_sql)
            .bind(since)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM request_log_catalog_rollups")
            .execute(&self.pool)
            .await?;
        let rebuild_sql = format!(
            r#"
            INSERT INTO request_log_catalog_rollups (
                {},
                request_count,
                updated_at
            )
            SELECT
                {},
                COUNT(*) AS request_count,
                CAST(strftime('%s', 'now') AS INTEGER) AS updated_at
            FROM request_logs
            WHERE visibility = 'visible'
              AND created_at >= ?
              AND {canonical_request_kind_predicate_sql}
            GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
            "#,
            Self::request_log_catalog_rollup_columns(),
            exprs.join(", "),
        );
        sqlx::query(&rebuild_sql)
            .bind(since)
            .execute(&self.pool)
            .await?;
        self.invalidate_request_logs_catalog_cache().await;
        Ok(())
    }

    pub(crate) async fn fetch_api_key_secret(
        &self,
        key_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let secret =
            sqlx::query_scalar::<_, String>("SELECT api_key FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(secret)
    }

    pub(crate) async fn fetch_api_key_id_by_secret(
        &self,
        secret: &str,
    ) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM api_keys WHERE api_key = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(secret)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_state_snapshot(
        &self,
        key_id: &str,
    ) -> Result<KeyStateSnapshot, ProxyError> {
        let status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT status FROM api_keys WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_quarantines
            WHERE key_id = ? AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        Ok(KeyStateSnapshot {
            status,
            quarantined,
        })
    }

    pub(crate) async fn insert_api_key_maintenance_record(
        &self,
        record: ApiKeyMaintenanceRecord,
    ) -> Result<(), ProxyError> {
        let auth_token_id = if let Some(auth_token_id) = record.auth_token_id.as_deref() {
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(auth_token_id)
                .fetch_optional(&self.pool)
                .await?
                .map(|_| auth_token_id.to_string())
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO api_key_maintenance_records (
                id,
                key_id,
                source,
                operation_code,
                operation_summary,
                reason_code,
                reason_summary,
                reason_detail,
                request_log_id,
                auth_token_log_id,
                auth_token_id,
                actor_user_id,
                actor_display_name,
                status_before,
                status_after,
                quarantine_before,
                quarantine_after,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(record.id)
        .bind(record.key_id)
        .bind(record.source)
        .bind(record.operation_code)
        .bind(record.operation_summary)
        .bind(record.reason_code)
        .bind(record.reason_summary)
        .bind(record.reason_detail)
        .bind(record.request_log_id)
        .bind(record.auth_token_log_id)
        .bind(auth_token_id)
        .bind(record.actor_user_id)
        .bind(record.actor_display_name)
        .bind(record.status_before)
        .bind(record.status_after)
        .bind(i64::from(record.quarantine_before))
        .bind(i64::from(record.quarantine_after))
        .bind(record.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn update_quota_for_key(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE api_keys
               SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ?
             WHERE id = ?"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn record_quota_sync_sample(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
        source: &str,
    ) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO api_key_quota_sync_samples (
                key_id,
                quota_limit,
                quota_remaining,
                captured_at,
                source
            ) VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(source)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"UPDATE api_keys
               SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ?
             WHERE id = ?"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = Utc::now().timestamp();
        let threshold = now - older_than_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
            )
            ORDER BY CASE WHEN quota_synced_at IS NULL OR quota_synced_at = 0 THEN 0 ELSE 1 END, quota_synced_at ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn list_keys_pending_hot_quota_sync(
        &self,
        active_within_secs: i64,
        stale_after_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = Utc::now().timestamp();
        let active_since = now - active_within_secs;
        let stale_before = now - stale_after_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND last_used_at >= ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
              )
            ORDER BY last_used_at DESC, quota_synced_at ASC, id ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(active_since)
        .bind(stale_before)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        let started_at = Utc::now().timestamp();
        let res = sqlx::query(
            r#"INSERT INTO scheduled_jobs (job_type, key_id, status, attempt, started_at)
               VALUES (?, ?, 'running', ?, ?)"#,
        )
        .bind(job_type)
        .bind(key_id)
        .bind(attempt)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub(crate) async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE scheduled_jobs SET status = ?, message = ?, finished_at = ? WHERE id = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            r#"SELECT
                    j.id,
                    j.job_type,
                    j.key_id,
                    k.group_name AS key_group,
                    j.status,
                    j.attempt,
                    j.message,
                    j.started_at,
                    j.finished_at
                FROM scheduled_jobs j
                LEFT JOIN api_keys k ON k.id = j.key_id
                ORDER BY j.started_at DESC, j.id DESC
                LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub(crate) async fn list_recent_job_signatures(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, String, Option<i64>)>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        sqlx::query_as::<_, (i64, String, Option<i64>)>(
            r#"
            SELECT id, status, finished_at
            FROM scheduled_jobs
            ORDER BY started_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64, JobGroupCounts), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100) as i64;
        let offset = ((page - 1) as i64).saturating_mul(per_page);

        let where_clause = Self::scheduled_job_group_filter_clause(group, "j.job_type");
        let count_where_clause = Self::scheduled_job_group_filter_clause(group, "job_type");

        let count_query = format!("SELECT COUNT(*) FROM scheduled_jobs {}", count_where_clause);
        let total: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(&self.pool)
            .await?;
        let group_counts = self.fetch_recent_job_group_counts().await?;

        let select_query = format!(
            r#"
            SELECT
                j.id,
                j.job_type,
                j.key_id,
                k.group_name AS key_group,
                j.status,
                j.attempt,
                j.message,
                j.started_at,
                j.finished_at
            FROM scheduled_jobs j
            LEFT JOIN api_keys k ON k.id = j.key_id
            {}
            ORDER BY j.started_at DESC, j.id DESC
            LIMIT ? OFFSET ?
            "#,
            where_clause
        );

        let rows = sqlx::query(&select_query)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((items, total, group_counts))
    }

    fn scheduled_job_group_filter_clause(group: &str, column: &str) -> String {
        let condition = match group {
            "quota" => format!(
                "{column} = 'quota_sync' OR {column} = 'quota_sync/manual' OR {column} = 'quota_sync/hot'"
            ),
            "usage" => format!("{column} = 'token_usage_rollup' OR {column} = 'usage_aggregation'"),
            "logs" => format!(
                "{column} = 'auth_token_logs_gc' OR {column} = 'request_logs_gc' OR {column} = 'log_cleanup'"
            ),
            "geo" => format!("{column} = 'forward_proxy_geo_refresh'"),
            "linuxdo" => format!("{column} = 'linuxdo_user_status_sync'"),
            _ => return String::new(),
        };
        format!("WHERE {condition}")
    }

    async fn fetch_recent_job_group_counts(&self) -> Result<JobGroupCounts, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) AS all_count,
                COALESCE(SUM(CASE WHEN job_type = 'quota_sync' OR job_type = 'quota_sync/manual' OR job_type = 'quota_sync/hot' THEN 1 ELSE 0 END), 0) AS quota_count,
                COALESCE(SUM(CASE WHEN job_type = 'token_usage_rollup' OR job_type = 'usage_aggregation' THEN 1 ELSE 0 END), 0) AS usage_count,
                COALESCE(SUM(CASE WHEN job_type = 'auth_token_logs_gc' OR job_type = 'request_logs_gc' OR job_type = 'log_cleanup' THEN 1 ELSE 0 END), 0) AS logs_count,
                COALESCE(SUM(CASE WHEN job_type = 'forward_proxy_geo_refresh' THEN 1 ELSE 0 END), 0) AS geo_count,
                COALESCE(SUM(CASE WHEN job_type = 'linuxdo_user_status_sync' THEN 1 ELSE 0 END), 0) AS linuxdo_count
            FROM scheduled_jobs
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(JobGroupCounts {
            all: row.try_get("all_count")?,
            quota: row.try_get("quota_count")?,
            usage: row.try_get("usage_count")?,
            logs: row.try_get("logs_count")?,
            geo: row.try_get("geo_count")?,
            linuxdo: row.try_get("linuxdo_count")?,
        })
    }

    pub(crate) async fn get_meta_string(&self, key: &str) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn get_meta_i64(&self, key: &str) -> Result<Option<i64>, ProxyError> {
        let value = self.get_meta_string(key).await?;

        if let Some(v) = value {
            match v.parse::<i64>() {
                Ok(parsed) => Ok(Some(parsed)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn set_meta_string(&self, key: &str, value: &str) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_meta_i64(&self, key: &str, value: i64) -> Result<(), ProxyError> {
        let v = value.to_string();
        self.set_meta_string(key, &v).await
    }

    pub(crate) async fn fetch_summary(&self) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(total_requests), 0) AS total_requests,
                COALESCE(SUM(success_count), 0) AS success_count,
                COALESCE(SUM(error_count), 0) AS error_count,
                COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL AND tb.key_id IS NULL THEN 1 ELSE 0 END), 0) AS active_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS exhausted_keys,
                COALESCE(SUM(CASE WHEN aq.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS quarantined_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL AND tb.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS temporary_isolated_keys
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            LEFT JOIN (
                SELECT key_id, MAX(cooldown_until) AS cooldown_until
                FROM api_key_transient_backoffs
                WHERE cooldown_until > strftime('%s', 'now')
                  AND reason_code = 'upstream_unknown_403'
                GROUP BY key_id
            ) AS tb
              ON tb.key_id = ak.id
            WHERE ak.deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .bind(STATUS_ACTIVE)
        .fetch_one(&self.pool)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(last_used_at) FROM api_keys WHERE deleted_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?
        .and_then(normalize_timestamp);

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0) AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0) AS total_quota_remaining
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            quarantined_keys: key_counts_row.try_get("quarantined_keys")?,
            temporary_isolated_keys: key_counts_row.try_get("temporary_isolated_keys")?,
            last_activity,
            total_quota_limit: quotas_row.try_get("total_quota_limit")?,
            total_quota_remaining: quotas_row.try_get("total_quota_remaining")?,
        })
    }

    async fn fetch_visible_request_log_floor_since(
        &self,
        since: i64,
    ) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT MIN(created_at)
            FROM request_logs
            WHERE visibility = ?
              AND created_at >= ?
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    async fn fetch_visible_request_log_window_metrics(
        &self,
        start: i64,
        end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        if start >= end {
            return Ok(SummaryWindowMetrics::default());
        }

        let request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key");
        let request_value_bucket_case_sql =
            request_value_bucket_sql(&request_kind_sql, "request_body");
        let query = format!(
            r#"
            WITH scoped_logs AS (
                SELECT
                    result_status,
                    ({request_value_bucket_case_sql}) AS request_value_bucket
                FROM request_logs
                WHERE visibility = ?
                  AND created_at >= ?
                  AND created_at < ?
            )
            SELECT
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS success_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS error_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS quota_exhausted_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'valuable' AND result_status = ? THEN 1 ELSE 0 END), 0) AS valuable_success_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'valuable' AND result_status IN (?, ?) THEN 1 ELSE 0 END), 0) AS valuable_failure_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'other' AND result_status = ? THEN 1 ELSE 0 END), 0) AS other_success_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'other' AND result_status IN (?, ?) THEN 1 ELSE 0 END), 0) AS other_failure_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'unknown' THEN 1 ELSE 0 END), 0) AS unknown_count
            FROM scoped_logs
            "#,
        );
        let row = sqlx::query(&query)
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(start)
            .bind(end)
            .bind(OUTCOME_SUCCESS)
            .bind(OUTCOME_ERROR)
            .bind(OUTCOME_QUOTA_EXHAUSTED)
            .bind(OUTCOME_SUCCESS)
            .bind(OUTCOME_ERROR)
            .bind(OUTCOME_QUOTA_EXHAUSTED)
            .bind(OUTCOME_SUCCESS)
            .bind(OUTCOME_ERROR)
            .bind(OUTCOME_QUOTA_EXHAUSTED)
            .fetch_one(&self.pool)
            .await?;

        Ok(SummaryWindowMetrics {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
            valuable_success_count: row.try_get("valuable_success_count")?,
            valuable_failure_count: row.try_get("valuable_failure_count")?,
            other_success_count: row.try_get("other_success_count")?,
            other_failure_count: row.try_get("other_failure_count")?,
            unknown_count: row.try_get("unknown_count")?,
            upstream_exhausted_key_count: 0,
            new_keys: 0,
            new_quarantines: 0,
            quota_charge: SummaryQuotaCharge::default(),
        })
    }

    async fn fetch_api_key_usage_bucket_window_metrics(
        &self,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let row = if let Some(bucket_start_before) = bucket_start_before {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= ?
                  AND bucket_start < ?
                "#,
            )
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= ?
                "#,
            )
            .bind(bucket_start_at_least)
            .fetch_one(&self.pool)
            .await?
        };

        Ok(SummaryWindowMetrics {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
            valuable_success_count: row.try_get("valuable_success_count")?,
            valuable_failure_count: row.try_get("valuable_failure_count")?,
            other_success_count: row.try_get("other_success_count")?,
            other_failure_count: row.try_get("other_failure_count")?,
            unknown_count: row.try_get("unknown_count")?,
            upstream_exhausted_key_count: 0,
            new_keys: 0,
            new_quarantines: 0,
            quota_charge: SummaryQuotaCharge::default(),
        })
    }

    async fn fetch_utc_month_gap_bucket_metrics(
        &self,
        month_start: i64,
        month_request_log_floor: Option<i64>,
        gap_fallback_end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let gap_end = match month_request_log_floor {
            Some(floor) if floor > month_start => floor,
            Some(_) => return Ok(SummaryWindowMetrics::default()),
            None => gap_fallback_end,
        };
        if gap_end <= month_start {
            return Ok(SummaryWindowMetrics::default());
        }

        let first_bucket_start = local_day_bucket_start_utc_ts(month_start);
        let first_exact_bucket_start = if first_bucket_start == month_start {
            month_start
        } else {
            next_local_day_start_utc_ts(first_bucket_start)
        };
        let last_gap_bucket_start = local_day_bucket_start_utc_ts(gap_end);

        let mut backfill = SummaryWindowMetrics::default();
        if first_exact_bucket_start < last_gap_bucket_start {
            add_summary_window_metrics(
                &mut backfill,
                &self
                    .fetch_api_key_usage_bucket_window_metrics(
                        first_exact_bucket_start,
                        Some(last_gap_bucket_start),
                    )
                    .await?,
            );
        }

        if gap_end > last_gap_bucket_start && last_gap_bucket_start >= month_start {
            let last_gap_bucket_end = next_local_day_start_utc_ts(last_gap_bucket_start);
            let full_day_bucket = self
                .fetch_api_key_usage_bucket_window_metrics(
                    last_gap_bucket_start,
                    Some(last_gap_bucket_end),
                )
                .await?;
            let retained_tail = self
                .fetch_visible_request_log_window_metrics(gap_end, last_gap_bucket_end)
                .await?;
            add_summary_window_metrics(
                &mut backfill,
                &subtract_summary_window_metrics(&full_day_bucket, &retained_tail),
            );
        }

        Ok(backfill)
    }

    async fn fetch_dashboard_rollup_window_metrics_tx(
        tx: &mut Transaction<'_, Sqlite>,
        bucket_secs: i64,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let row = if let Some(bucket_start_before) = bucket_start_before {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(local_estimated_credits), 0) AS local_estimated_credits
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                  AND bucket_start < ?
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(&mut **tx)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(local_estimated_credits), 0) AS local_estimated_credits
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_start_at_least)
            .fetch_one(&mut **tx)
            .await?
        };

        Ok(SummaryWindowMetrics {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
            valuable_success_count: row.try_get("valuable_success_count")?,
            valuable_failure_count: row.try_get("valuable_failure_count")?,
            other_success_count: row.try_get("other_success_count")?,
            other_failure_count: row.try_get("other_failure_count")?,
            unknown_count: row.try_get("unknown_count")?,
            upstream_exhausted_key_count: 0,
            new_keys: 0,
            new_quarantines: 0,
            quota_charge: SummaryQuotaCharge {
                local_estimated_credits: row.try_get("local_estimated_credits")?,
                ..SummaryQuotaCharge::default()
            },
        })
    }

    async fn fetch_dashboard_rollup_success_count_tx(
        tx: &mut Transaction<'_, Sqlite>,
        bucket_secs: i64,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        if bucket_start_at_least >= bucket_start_before {
            return Ok(0);
        }

        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(success_count), 0)
            FROM dashboard_request_rollup_buckets
            WHERE bucket_secs = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            "#,
        )
        .bind(bucket_secs)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&mut **tx)
        .await
        .map_err(ProxyError::Database)
    }

    async fn fetch_dashboard_rollup_success_count_for_range_tx(
        tx: &mut Transaction<'_, Sqlite>,
        start: i64,
        end: i64,
    ) -> Result<i64, ProxyError> {
        if start >= end {
            return Ok(0);
        }

        let start_day = local_day_bucket_start_utc_ts(start);
        let first_full_day_start = if start_day == start {
            start
        } else {
            next_local_day_start_utc_ts(start_day)
        };
        let end_day = local_day_bucket_start_utc_ts(end);
        let full_day_end = if end_day == end { end } else { end_day };

        let mut cursor = start;
        let mut success_count = 0;

        let leading_minute_end = end.min(first_full_day_start);
        if cursor < leading_minute_end {
            success_count += Self::fetch_dashboard_rollup_success_count_tx(
                tx,
                SECS_PER_MINUTE,
                cursor,
                leading_minute_end,
            )
            .await?;
            cursor = leading_minute_end;
        }

        if cursor < full_day_end {
            success_count += Self::fetch_dashboard_rollup_success_count_tx(
                tx,
                SECS_PER_DAY,
                cursor,
                full_day_end,
            )
            .await?;
            cursor = full_day_end;
        }

        if cursor < end {
            success_count += Self::fetch_dashboard_rollup_success_count_tx(
                tx,
                SECS_PER_MINUTE,
                cursor,
                end,
            )
            .await?;
        }

        Ok(success_count)
    }

    async fn fetch_visible_request_log_success_count_tx(
        tx: &mut Transaction<'_, Sqlite>,
        start: i64,
        end: i64,
    ) -> Result<i64, ProxyError> {
        if start >= end {
            return Ok(0);
        }

        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0)
            FROM request_logs
            WHERE visibility = ?
              AND created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(start)
        .bind(end)
        .fetch_one(&mut **tx)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn fetch_success_breakdown_from_dashboard_rollups(
        &self,
        month_start: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let now = Utc::now().timestamp();
        let month_request_log_floor = self
            .fetch_visible_request_log_floor_since(month_start)
            .await?;
        let historical_month_success = self
            .fetch_utc_month_gap_bucket_metrics(month_start, month_request_log_floor, now)
            .await?
            .success_count;
        let mut tx = self.pool.begin().await?;
        let (retained_partial_minute_success, dashboard_month_start) =
            match month_request_log_floor {
                Some(floor) if floor > month_start => {
                    let minute_start = floor.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
                    if floor == minute_start {
                        (0, floor)
                    } else {
                        let next_minute_start = minute_start.saturating_add(SECS_PER_MINUTE);
                        let partial_minute_success = Self::fetch_visible_request_log_success_count_tx(
                            &mut tx,
                            floor,
                            next_minute_start.min(now.saturating_add(1)),
                        )
                        .await?;
                        (partial_minute_success, next_minute_start)
                    }
                }
                Some(_) => (0, month_start),
                None => (0, now.saturating_add(1)),
            };
        let dashboard_month_success = Self::fetch_dashboard_rollup_success_count_for_range_tx(
            &mut tx,
            dashboard_month_start,
            now.saturating_add(1),
        )
        .await?;
        let daily_success =
            Self::fetch_dashboard_rollup_success_count_for_range_tx(&mut tx, day_start, day_end)
                .await?;
        tx.commit().await?;

        Ok(SuccessBreakdown {
            monthly_success: historical_month_success
                + retained_partial_minute_success
                + dashboard_month_success,
            daily_success,
        })
    }

    async fn fetch_dashboard_rollup_month_metrics_tx(
        tx: &mut Transaction<'_, Sqlite>,
        month_start: i64,
        today_start: i64,
        today_end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let mut month_metrics = SummaryWindowMetrics::default();
        let month_partial_bucket_start = local_day_bucket_start_utc_ts(month_start);
        let month_full_day_start = if month_partial_bucket_start == month_start {
            month_start
        } else {
            next_local_day_start_utc_ts(month_partial_bucket_start)
        };
        if month_start < month_full_day_start.min(today_start) {
            add_summary_window_metrics(
                &mut month_metrics,
                &Self::fetch_dashboard_rollup_window_metrics_tx(
                    tx,
                    SECS_PER_MINUTE,
                    month_start,
                    Some(month_full_day_start.min(today_start)),
                )
                .await?,
            );
        }
        if month_full_day_start < today_start {
            add_summary_window_metrics(
                &mut month_metrics,
                &Self::fetch_dashboard_rollup_window_metrics_tx(
                    tx,
                    SECS_PER_DAY,
                    month_full_day_start,
                    Some(today_start),
                )
                .await?,
            );
        }
        add_summary_window_metrics(
            &mut month_metrics,
            &Self::fetch_dashboard_rollup_window_metrics_tx(
                tx,
                SECS_PER_MINUTE,
                today_start,
                Some(today_end),
            )
            .await?,
        );

        Ok(month_metrics)
    }

    pub(crate) async fn fetch_summary_windows(
        &self,
        today_start: i64,
        today_end: i64,
        yesterday_start: i64,
        yesterday_end: i64,
        month_start: i64,
        month_quota_charge_start: i64,
    ) -> Result<SummaryWindows, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let sample_window_start = yesterday_start.min(month_quota_charge_start);
        let now_ts = today_end.saturating_sub(1);
        let hot_active_since = now_ts.saturating_sub(2 * 60 * 60);
        let hot_stale_before = now_ts.saturating_sub(15 * 60);
        let cold_stale_before = now_ts.saturating_sub(24 * 60 * 60);
        let today_metrics = Self::fetch_dashboard_rollup_window_metrics_tx(
            &mut tx,
            SECS_PER_MINUTE,
            today_start,
            Some(today_end),
        )
        .await?;
        let yesterday_metrics = Self::fetch_dashboard_rollup_window_metrics_tx(
            &mut tx,
            SECS_PER_MINUTE,
            yesterday_start,
            Some(yesterday_end),
        )
        .await?;
        let month_metrics = Self::fetch_dashboard_rollup_month_metrics_tx(
            &mut tx,
            month_start,
            today_start,
            today_end,
        )
        .await?;
        let month_charge_metrics = Self::fetch_dashboard_rollup_month_metrics_tx(
            &mut tx,
            month_quota_charge_start,
            today_start,
            today_end,
        )
        .await?;

        let lifecycle_row = sqlx::query(
            r#"
            SELECT
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS today_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS yesterday_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS month_upstream_exhausted_key_count
            FROM api_key_maintenance_records
            WHERE source = ?
              AND operation_code = ?
              AND reason_code = ?
              AND created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(today_start)
        .bind(today_end)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(month_start)
        .bind(today_end)
        .bind(MAINTENANCE_SOURCE_SYSTEM)
        .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(yesterday_start.min(month_start))
        .bind(today_end)
        .fetch_one(&mut *tx)
        .await?;

        let month_lifecycle_row = sqlx::query(
            r#"
            SELECT
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_keys
                    WHERE created_at >= ?
                ) AS month_new_keys,
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_key_quarantines
                    WHERE created_at >= ?
                ) AS month_new_quarantines
            "#,
        )
        .bind(month_start)
        .bind(month_start)
        .fetch_one(&mut *tx)
        .await?;

        let sample_rows = sqlx::query(
            r#"
            WITH window_rows AS (
                SELECT key_id, quota_remaining, captured_at
                FROM api_key_quota_sync_samples
                WHERE captured_at >= ?
                  AND captured_at < ?
            ),
            sampled_keys AS (
                SELECT DISTINCT key_id FROM window_rows
            ),
            baseline_rows AS (
                SELECT s.key_id, s.quota_remaining, s.captured_at
                FROM api_key_quota_sync_samples s
                INNER JOIN (
                    SELECT key_id, MAX(captured_at) AS captured_at
                    FROM api_key_quota_sync_samples
                    WHERE captured_at < ?
                      AND key_id IN (SELECT key_id FROM sampled_keys)
                    GROUP BY key_id
                ) latest
                    ON latest.key_id = s.key_id
                   AND latest.captured_at = s.captured_at
            )
            SELECT key_id, quota_remaining, captured_at
            FROM window_rows
            UNION ALL
            SELECT key_id, quota_remaining, captured_at
            FROM baseline_rows
            ORDER BY key_id ASC, captured_at ASC
            "#,
        )
        .bind(sample_window_start)
        .bind(today_end)
        .bind(sample_window_start)
        .fetch_all(&mut *tx)
        .await?;

        let stale_key_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(COUNT(*), 0)
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND CASE
                  WHEN last_used_at >= ? THEN (
                      quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
                  )
                  ELSE (
                      quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
                  )
              END
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(hot_active_since)
        .bind(hot_stale_before)
        .bind(cold_stale_before)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        let mut today_charge = QuotaChargeAccumulator::default();
        let mut yesterday_charge = QuotaChargeAccumulator::default();
        let mut month_charge = QuotaChargeAccumulator::default();
        let mut today_sampled_keys = std::collections::HashSet::new();
        let mut yesterday_sampled_keys = std::collections::HashSet::new();
        let mut month_sampled_keys = std::collections::HashSet::new();
        let mut current_key: Option<String> = None;
        let mut previous_sample: Option<QuotaSyncSampleRow> = None;

        for row in sample_rows {
            let key_id: String = row.try_get("key_id")?;
            if current_key.as_deref() != Some(key_id.as_str()) {
                current_key = Some(key_id.clone());
                previous_sample = None;
            }

            let sample = QuotaSyncSampleRow {
                quota_remaining: row.try_get("quota_remaining")?,
                captured_at: row.try_get("captured_at")?,
            };
            let delta = previous_sample
                .map(|previous| (previous.quota_remaining - sample.quota_remaining).max(0))
                .unwrap_or(0);

            if sample.captured_at >= month_quota_charge_start && sample.captured_at < today_end {
                month_charge.upstream_actual_credits += delta;
                month_sampled_keys.insert(key_id.clone());
                if month_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    month_charge.latest_sync_at = Some(sample.captured_at);
                }
            }
            if sample.captured_at >= today_start && sample.captured_at < today_end {
                today_charge.upstream_actual_credits += delta;
                today_sampled_keys.insert(key_id.clone());
                if today_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    today_charge.latest_sync_at = Some(sample.captured_at);
                }
            }
            if sample.captured_at >= yesterday_start && sample.captured_at < yesterday_end {
                yesterday_charge.upstream_actual_credits += delta;
                yesterday_sampled_keys.insert(key_id.clone());
                if yesterday_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    yesterday_charge.latest_sync_at = Some(sample.captured_at);
                }
            }

            previous_sample = Some(sample);
        }

        today_charge.sampled_key_count = today_sampled_keys.len() as i64;
        today_charge.stale_key_count = stale_key_count;
        yesterday_charge.sampled_key_count = yesterday_sampled_keys.len() as i64;
        yesterday_charge.stale_key_count = stale_key_count;
        month_charge.sampled_key_count = month_sampled_keys.len() as i64;
        month_charge.stale_key_count = stale_key_count;

        Ok(SummaryWindows {
            today: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("today_upstream_exhausted_key_count")?,
                quota_charge: SummaryQuotaCharge {
                    upstream_actual_credits: today_charge.upstream_actual_credits,
                    sampled_key_count: today_charge.sampled_key_count,
                    stale_key_count: today_charge.stale_key_count,
                    latest_sync_at: today_charge.latest_sync_at,
                    ..today_metrics.quota_charge
                },
                ..today_metrics
            },
            yesterday: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("yesterday_upstream_exhausted_key_count")?,
                quota_charge: SummaryQuotaCharge {
                    upstream_actual_credits: yesterday_charge.upstream_actual_credits,
                    sampled_key_count: yesterday_charge.sampled_key_count,
                    stale_key_count: yesterday_charge.stale_key_count,
                    latest_sync_at: yesterday_charge.latest_sync_at,
                    ..yesterday_metrics.quota_charge
                },
                ..yesterday_metrics
            },
            month: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("month_upstream_exhausted_key_count")?,
                new_keys: month_lifecycle_row.try_get("month_new_keys")?,
                new_quarantines: month_lifecycle_row.try_get("month_new_quarantines")?,
                quota_charge: SummaryQuotaCharge {
                    local_estimated_credits: month_charge_metrics
                        .quota_charge
                        .local_estimated_credits,
                    upstream_actual_credits: month_charge.upstream_actual_credits,
                    sampled_key_count: month_charge.sampled_key_count,
                    stale_key_count: month_charge.stale_key_count,
                    latest_sync_at: month_charge.latest_sync_at,
                },
                ..month_metrics
            },
        })
    }

    pub(crate) async fn fetch_dashboard_hourly_request_window(
        &self,
        current_hour_start: i64,
        bucket_seconds: i64,
        visible_buckets: i64,
        retained_buckets: i64,
    ) -> Result<DashboardHourlyRequestWindow, ProxyError> {
        if bucket_seconds <= 0 || visible_buckets <= 0 || retained_buckets <= 0 {
            return Ok(DashboardHourlyRequestWindow {
                bucket_seconds,
                visible_buckets,
                retained_buckets,
                buckets: Vec::new(),
            });
        }

        let series_start = current_hour_start
            .saturating_sub(bucket_seconds.saturating_mul(retained_buckets.saturating_sub(1)));
        let range_end = current_hour_start.saturating_add(bucket_seconds);
        let hour_alignment_offset = current_hour_start.rem_euclid(bucket_seconds);
        let rows = sqlx::query(
            r#"
            WITH RECURSIVE hour_series(bucket_start) AS (
                SELECT ? AS bucket_start
                UNION ALL
                SELECT bucket_start + ?
                FROM hour_series
                WHERE bucket_start + ? <= ?
            ),
            aggregated AS (
                SELECT
                    ((bucket_start - ?) / ?) * ? + ? AS hour_bucket_start,
                    COALESCE(SUM(other_success_count), 0) AS secondary_success,
                    COALESCE(SUM(valuable_success_count), 0) AS primary_success,
                    COALESCE(SUM(other_failure_count), 0) AS secondary_failure,
                    COALESCE(SUM(valuable_failure_429_count), 0) AS primary_failure_429,
                    COALESCE(
                        SUM(
                            CASE
                                WHEN valuable_failure_count > valuable_failure_429_count
                                    THEN valuable_failure_count - valuable_failure_429_count
                                ELSE 0
                            END
                        ),
                        0
                    ) AS primary_failure_other,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(mcp_non_billable), 0) AS mcp_non_billable,
                    COALESCE(SUM(mcp_billable), 0) AS mcp_billable,
                    COALESCE(SUM(api_non_billable), 0) AS api_non_billable,
                    COALESCE(SUM(api_billable), 0) AS api_billable
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                  AND bucket_start < ?
                GROUP BY hour_bucket_start
            )
            SELECT
                hour_series.bucket_start,
                COALESCE(aggregated.secondary_success, 0) AS secondary_success,
                COALESCE(aggregated.primary_success, 0) AS primary_success,
                COALESCE(aggregated.secondary_failure, 0) AS secondary_failure,
                COALESCE(aggregated.primary_failure_429, 0) AS primary_failure_429,
                COALESCE(aggregated.primary_failure_other, 0) AS primary_failure_other,
                COALESCE(aggregated.unknown_count, 0) AS unknown_count,
                COALESCE(aggregated.mcp_non_billable, 0) AS mcp_non_billable,
                COALESCE(aggregated.mcp_billable, 0) AS mcp_billable,
                COALESCE(aggregated.api_non_billable, 0) AS api_non_billable,
                COALESCE(aggregated.api_billable, 0) AS api_billable
            FROM hour_series
            LEFT JOIN aggregated ON aggregated.hour_bucket_start = hour_series.bucket_start
            ORDER BY hour_series.bucket_start ASC
            "#,
        )
        .bind(series_start)
        .bind(bucket_seconds)
        .bind(bucket_seconds)
        .bind(current_hour_start)
        .bind(hour_alignment_offset)
        .bind(bucket_seconds)
        .bind(bucket_seconds)
        .bind(hour_alignment_offset)
        .bind(SECS_PER_MINUTE)
        .bind(series_start)
        .bind(range_end)
        .fetch_all(&self.pool)
        .await?;

        let buckets = rows
            .into_iter()
            .map(|row| {
                Ok(DashboardHourlyRequestBucket {
                    bucket_start: row.try_get("bucket_start")?,
                    secondary_success: row.try_get("secondary_success")?,
                    primary_success: row.try_get("primary_success")?,
                    secondary_failure: row.try_get("secondary_failure")?,
                    primary_failure_429: row.try_get("primary_failure_429")?,
                    primary_failure_other: row.try_get("primary_failure_other")?,
                    unknown: row.try_get("unknown_count")?,
                    mcp_non_billable: row.try_get("mcp_non_billable")?,
                    mcp_billable: row.try_get("mcp_billable")?,
                    api_non_billable: row.try_get("api_non_billable")?,
                    api_billable: row.try_get("api_billable")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(DashboardHourlyRequestWindow {
            bucket_seconds,
            visible_buckets,
            retained_buckets,
            buckets,
        })
    }

    #[cfg(test)]
    pub(crate) async fn fetch_success_breakdown(
        &self,
        month_since: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let month_request_log_floor = self
            .fetch_visible_request_log_floor_since(month_since)
            .await?;
        let bucket_month_success = self
            .fetch_utc_month_gap_bucket_metrics(
                month_since,
                month_request_log_floor,
                Utc::now().timestamp(),
            )
            .await?
            .success_count;
        let scan_floor = month_since.min(day_start);
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN created_at >= ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS daily_success
            FROM request_logs
            WHERE visibility = ?
              AND created_at >= ?
            "#,
        )
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(day_end)
        .bind(OUTCOME_SUCCESS)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(scan_floor)
        .fetch_one(&self.pool)
        .await?;

        Ok(SuccessBreakdown {
            monthly_success: bucket_month_success + row.try_get::<i64, _>("monthly_success")?,
            daily_success: row.try_get("daily_success")?,
        })
    }

    pub(crate) async fn fetch_token_success_failure(
        &self,
        token_id: &str,
        month_since: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let scan_floor = month_since.min(day_start);
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs
            WHERE token_id = ?
              AND created_at >= ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(day_end)
        .bind(OUTCOME_ERROR)
        .bind(day_start)
        .bind(day_end)
        .bind(token_id)
        .bind(scan_floor)
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }
}
