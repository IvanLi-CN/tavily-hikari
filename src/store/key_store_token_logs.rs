impl KeyStore {
    pub async fn fetch_token_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = if let Some(bid) = before_id {
            sqlx::query(
                r#"
                SELECT id, api_key_id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END AS business_credits,
                       request_kind_key, request_kind_label, request_kind_detail,
                       counts_business_quota, result_status, error_message, failure_kind, key_effect_code,
                       key_effect_summary, binding_effect_code, binding_effect_summary,
                       selection_effect_code, selection_effect_summary,
                       gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                       upstream_operation, fallback_reason, created_at
                FROM auth_token_logs
                WHERE token_id = ? AND id < ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(bid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, api_key_id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END AS business_credits,
                       request_kind_key, request_kind_label, request_kind_detail,
                       counts_business_quota, result_status, error_message, failure_kind, key_effect_code,
                       key_effect_summary, binding_effect_code, binding_effect_summary,
                       selection_effect_code, selection_effect_summary,
                       gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                       upstream_operation, fallback_reason, created_at
                FROM auth_token_logs
                WHERE token_id = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(Self::map_token_log_row)
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn normalize_request_kind_filters(request_kinds: &[String]) -> Vec<String> {
        request_kinds
            .iter()
            .map(|value| canonical_request_kind_key_for_filter(value))
            .filter(|value| !value.trim().is_empty())
            .collect()
    }

    fn push_request_kind_filter_clause<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        stored_request_kind_sql: &str,
        legacy_request_kind_predicate_sql: &str,
        legacy_request_kind_sql: &str,
        request_kinds: &[String],
    ) {
        builder.push("(");
        builder.push(stored_request_kind_sql.to_string());
        builder.push(" IN (");
        {
            let mut separated = builder.separated(", ");
            for request_kind in request_kinds {
                separated.push_bind(request_kind.clone());
            }
            separated.push_unseparated(")");
        }
        builder.push(" OR (");
        builder.push(legacy_request_kind_predicate_sql.to_string());
        builder.push(" AND ");
        builder.push(legacy_request_kind_sql.to_string());
        builder.push(" IN (");
        {
            let mut separated = builder.separated(", ");
            for request_kind in request_kinds {
                separated.push_bind(request_kind.clone());
            }
            separated.push_unseparated(")");
        }
        builder.push("))");
    }

    fn push_operational_class_filter_clause<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        operational_class: &'a str,
        legacy_request_kind_predicate_sql: &str,
        stored_operational_class_sql: &str,
        legacy_operational_class_sql: &str,
    ) {
        builder.push("(");
        builder.push("((NOT ");
        builder.push(legacy_request_kind_predicate_sql.to_string());
        builder.push(") AND ");
        builder.push(stored_operational_class_sql.to_string());
        builder.push(" = ");
        builder.push_bind(operational_class);
        builder.push(") OR (");
        builder.push(legacy_request_kind_predicate_sql.to_string());
        builder.push(" AND ");
        builder.push(legacy_operational_class_sql.to_string());
        builder.push(" = ");
        builder.push_bind(operational_class);
        builder.push("))");
    }

    fn push_result_bucket_filter_clause<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        result_bucket: &'a str,
        legacy_request_kind_predicate_sql: &str,
        stored_result_bucket_sql: &str,
        legacy_result_bucket_sql: &str,
    ) {
        builder.push("(");
        builder.push("((NOT ");
        builder.push(legacy_request_kind_predicate_sql.to_string());
        builder.push(") AND ");
        builder.push(stored_result_bucket_sql.to_string());
        builder.push(" = ");
        builder.push_bind(result_bucket);
        builder.push(") OR (");
        builder.push(legacy_request_kind_predicate_sql.to_string());
        builder.push(" AND ");
        builder.push(legacy_result_bucket_sql.to_string());
        builder.push(" = ");
        builder.push_bind(result_bucket);
        builder.push("))");
    }

    fn map_token_log_row(row: sqlx::sqlite::SqliteRow) -> Result<TokenLogRecord, sqlx::Error> {
        let key_id: Option<String> = row.try_get("api_key_id")?;
        let method: String = row.try_get("method")?;
        let path: String = row.try_get("path")?;
        let query: Option<String> = row.try_get("query")?;
        let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
        let stored_request_kind_label: Option<String> = row.try_get("request_kind_label")?;
        let stored_request_kind_detail: Option<String> = row.try_get("request_kind_detail")?;
        let request_kind = finalize_token_request_kind(
            method.as_str(),
            path.as_str(),
            query.as_deref(),
            stored_request_kind_key.clone(),
            stored_request_kind_label.clone(),
            stored_request_kind_detail.clone(),
        );

        Ok(TokenLogRecord {
            id: row.try_get("id")?,
            key_id,
            method,
            path,
            query,
            http_status: row.try_get("http_status")?,
            mcp_status: row.try_get("mcp_status")?,
            business_credits: row.try_get("business_credits")?,
            request_kind_key: request_kind.key,
            request_kind_label: request_kind.label,
            request_kind_detail: request_kind.detail,
            counts_business_quota: row.try_get::<i64, _>("counts_business_quota")? != 0,
            result_status: row.try_get("result_status")?,
            error_message: row.try_get("error_message")?,
            failure_kind: row.try_get("failure_kind")?,
            key_effect_code: row.try_get("key_effect_code")?,
            key_effect_summary: row.try_get("key_effect_summary")?,
            binding_effect_code: row.try_get("binding_effect_code")?,
            binding_effect_summary: row.try_get("binding_effect_summary")?,
            selection_effect_code: row.try_get("selection_effect_code")?,
            selection_effect_summary: row.try_get("selection_effect_summary")?,
            gateway_mode: row.try_get("gateway_mode")?,
            experiment_variant: row.try_get("experiment_variant")?,
            proxy_session_id: row.try_get("proxy_session_id")?,
            routing_subject_hash: row.try_get("routing_subject_hash")?,
            upstream_operation: row.try_get("upstream_operation")?,
            fallback_reason: row.try_get("fallback_reason")?,
            created_at: row.try_get("created_at")?,
        })
    }

    pub async fn fetch_token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let end_exclusive = until.unwrap_or(now_ts);
        if end_exclusive <= since {
            return Ok(TokenSummary {
                total_requests: 0,
                success_count: 0,
                error_count: 0,
                quota_exhausted_count: 0,
                last_activity: None,
            });
        }

        let rows = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count,
                quota_exhausted_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(since)
        .bind(end_exclusive)
        .fetch_all(&self.pool)
        .await?;

        let mut total_requests = 0;
        let mut success_count = 0;
        let mut system_failure_count = 0;
        let mut external_failure_count = 0;
        let mut quota_exhausted_count = 0;
        let mut last_activity: Option<i64> = None;

        for (bucket_start, success, system_failure, external_failure, quota_exhausted) in rows {
            success_count += success;
            system_failure_count += system_failure;
            external_failure_count += external_failure;
            quota_exhausted_count += quota_exhausted;
            total_requests += success + system_failure + external_failure + quota_exhausted;
            let bucket_end = bucket_start + TOKEN_USAGE_STATS_BUCKET_SECS;
            last_activity = Some(match last_activity {
                Some(prev) if prev > bucket_end => prev,
                _ => bucket_end,
            });
        }

        let error_count = system_failure_count + external_failure_count;

        Ok(TokenSummary {
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            last_activity,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_token_logs_catalog(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<RequestLogsCatalog, ProxyError> {
        let cache_key = Self::token_logs_catalog_filters_are_empty(filters)
            .then(|| Self::token_logs_catalog_cache_key(token_id, since, until));
        if let Some(cache_key) = cache_key.as_deref()
            && let Some(cached) = self.cached_request_logs_catalog(cache_key).await
        {
            return Ok(cached);
        }

        let request_kind_options = self
            .fetch_token_log_request_kind_options(token_id, since, until, filters)
            .await?;
        let results = self
            .fetch_token_log_result_facet_options(token_id, since, until, filters)
            .await?;
        let key_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "key_effect_code",
                false,
                filters,
            )
            .await?;
        let binding_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "binding_effect_code",
                false,
                filters,
            )
            .await?;
        let selection_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "selection_effect_code",
                false,
                filters,
            )
            .await?;
        let keys = self
            .fetch_token_log_facet_options(token_id, since, until, "api_key_id", true, filters)
            .await?;
        let catalog = RequestLogsCatalog {
            retention_days: effective_auth_token_log_retention_days(),
            request_kind_options,
            facets: RequestLogPageFacets {
                results,
                key_effects,
                binding_effects,
                selection_effects,
                tokens: Vec::new(),
                keys,
            },
        };
        if let Some(cache_key) = cache_key {
            self.cache_request_logs_catalog(cache_key, &catalog).await;
        }
        Ok(catalog)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_token_logs_cursor_page(
        &self,
        token_id: &str,
        page_size: i64,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
    ) -> Result<TokenLogsCursorPage, ProxyError> {
        let page_size = page_size.clamp(1, 200);
        let query_limit = page_size + 1;
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql("path", "request_kind_key");
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");

        let mut rows_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT id, api_key_id, method, path, query, http_status, mcp_status,
                   CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END AS business_credits,
                   request_kind_key,
                   request_kind_label,
                   request_kind_detail,
                   counts_business_quota,
                   result_status, error_message, failure_kind, key_effect_code,
                   key_effect_summary, binding_effect_code, binding_effect_summary,
                   selection_effect_code, selection_effect_summary,
                   gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                   upstream_operation, fallback_reason, created_at
            FROM auth_token_logs
            WHERE token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND created_at < ");
            rows_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            rows_query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut rows_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(key_effect_code) = key_effect_code {
            rows_query.push(" AND key_effect_code = ");
            rows_query.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = binding_effect_code {
            rows_query.push(" AND binding_effect_code = ");
            rows_query.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = selection_effect_code {
            rows_query.push(" AND selection_effect_code = ");
            rows_query.push_bind(selection_effect_code);
        }
        if let Some(key_id) = key_id {
            rows_query.push(" AND api_key_id = ");
            rows_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            rows_query.push(" AND ");
            Self::push_request_kind_filter_clause(
                &mut rows_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            rows_query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut rows_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        Self::push_desc_cursor_clause(&mut rows_query, "created_at", "id", cursor, direction, true);
        match direction {
            RequestLogsCursorDirection::Older => {
                rows_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
            }
            RequestLogsCursorDirection::Newer => {
                rows_query.push(" ORDER BY created_at ASC, id ASC LIMIT ");
            }
        }
        rows_query.push_bind(query_limit);

        let mut rows = rows_query.build().fetch_all(&self.pool).await?;
        let has_more = rows.len() as i64 > page_size;
        if has_more {
            rows.truncate(page_size as usize);
        }
        if matches!(direction, RequestLogsCursorDirection::Newer) {
            rows.reverse();
        }

        let items = rows
            .into_iter()
            .map(Self::map_token_log_row)
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

        Ok(TokenLogsCursorPage {
            next_cursor: has_older
                .then(|| {
                    items
                        .last()
                        .map(Self::request_logs_cursor_for_token_record)
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
                        .map(Self::request_logs_cursor_for_token_record)
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
    pub async fn fetch_token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<TokenLogsPage, ProxyError> {
        let per_page = per_page.clamp(1, 200) as i64;
        let page = page.max(1) as i64;
        let offset = (page - 1) * per_page;
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql("path", "request_kind_key");
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");

        let mut total_query =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ");
        total_query.push_bind(token_id);
        total_query.push(" AND created_at >= ");
        total_query.push_bind(since);
        if let Some(until) = until {
            total_query.push(" AND created_at < ");
            total_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            total_query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut total_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(key_effect_code) = key_effect_code {
            total_query.push(" AND key_effect_code = ");
            total_query.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = binding_effect_code {
            total_query.push(" AND binding_effect_code = ");
            total_query.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = selection_effect_code {
            total_query.push(" AND selection_effect_code = ");
            total_query.push_bind(selection_effect_code);
        }
        if let Some(key_id) = key_id {
            total_query.push(" AND api_key_id = ");
            total_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            total_query.push(" AND ");
            Self::push_request_kind_filter_clause(
                &mut total_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            total_query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut total_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        let total: i64 = total_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        let mut rows_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT id, api_key_id, method, path, query, http_status, mcp_status,
                   CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END AS business_credits,
                   request_kind_key,
                   request_kind_label,
                   request_kind_detail,
                   counts_business_quota,
                   result_status, error_message, failure_kind, key_effect_code,
                   key_effect_summary, binding_effect_code, binding_effect_summary,
                   selection_effect_code, selection_effect_summary,
                   gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                   upstream_operation, fallback_reason, created_at
            FROM auth_token_logs
            WHERE token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND created_at < ");
            rows_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            rows_query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut rows_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(key_effect_code) = key_effect_code {
            rows_query.push(" AND key_effect_code = ");
            rows_query.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = binding_effect_code {
            rows_query.push(" AND binding_effect_code = ");
            rows_query.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = selection_effect_code {
            rows_query.push(" AND selection_effect_code = ");
            rows_query.push_bind(selection_effect_code);
        }
        if let Some(key_id) = key_id {
            rows_query.push(" AND api_key_id = ");
            rows_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            rows_query.push(" AND ");
            Self::push_request_kind_filter_clause(
                &mut rows_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            rows_query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut rows_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        rows_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        rows_query.push_bind(per_page);
        rows_query.push(" OFFSET ");
        rows_query.push_bind(offset);
        let rows = rows_query.build().fetch_all(&self.pool).await?;

        let items = rows
            .into_iter()
            .map(Self::map_token_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        let empty_filters = TokenLogsCatalogFilters {
            request_kinds: &[],
            result_status: None,
            key_effect_code: None,
            binding_effect_code: None,
            selection_effect_code: None,
            key_id: None,
            operational_class: None,
        };
        let request_kind_options = self
            .fetch_token_log_request_kind_options(token_id, since, until, empty_filters)
            .await?;
        let results = self
            .fetch_token_log_result_facet_options(token_id, since, until, empty_filters)
            .await?;
        let key_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "key_effect_code",
                false,
                empty_filters,
            )
            .await?;
        let binding_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "binding_effect_code",
                false,
                empty_filters,
            )
            .await?;
        let selection_effects = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "selection_effect_code",
                false,
                empty_filters,
            )
            .await?;
        let keys = self
            .fetch_token_log_facet_options(
                token_id,
                since,
                until,
                "api_key_id",
                true,
                empty_filters,
            )
            .await?;

        Ok(TokenLogsPage {
            items,
            total,
            request_kind_options,
            facets: RequestLogPageFacets {
                results,
                key_effects,
                binding_effects,
                selection_effects,
                tokens: Vec::new(),
                keys,
            },
        })
    }

    async fn fetch_token_log_facet_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        column_expr: &str,
        require_non_empty: bool,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql("path", "request_kind_key");
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {column_expr} AS value, COUNT(*) AS count FROM auth_token_logs"
        ));
        Self::push_token_logs_catalog_filters(
            &mut query,
            token_id,
            since,
            until,
            filters,
            stored_request_kind_sql,
            &legacy_request_kind_predicate_sql,
            &legacy_request_kind_sql,
            &stored_operational_class_case_sql,
            &legacy_operational_class_case_sql,
            &stored_result_bucket_sql,
            &legacy_result_bucket_sql,
        );
        if require_non_empty {
            query.push(" AND ");
            query.push(format!(
                "{column_expr} IS NOT NULL AND TRIM({column_expr}) <> ''"
            ));
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

    async fn fetch_token_log_result_facet_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql("path", "request_kind_key");
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");

        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "
            SELECT
                CASE
                    WHEN {legacy_request_kind_predicate_sql} THEN {legacy_result_bucket_sql}
                    ELSE {stored_result_bucket_sql}
                END AS value,
                COUNT(*) AS count
            FROM auth_token_logs
            "
        ));
        Self::push_token_logs_catalog_filters(
            &mut query,
            token_id,
            since,
            until,
            filters,
            stored_request_kind_sql,
            &legacy_request_kind_predicate_sql,
            &legacy_request_kind_sql,
            &stored_operational_class_case_sql,
            &legacy_operational_class_case_sql,
            &stored_result_bucket_sql,
            &legacy_result_bucket_sql,
        );
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

    pub async fn fetch_token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        type RequestKindOptionRow = (String, String, i64, i64, i64);
        let stored_request_kind_sql = "request_kind_key";
        let canonical_request_kind_predicate_sql =
            canonical_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let stored_label_sql = canonical_request_kind_label_sql(stored_request_kind_sql);
        let mut stored_query = QueryBuilder::<Sqlite>::new(format!(
            "
            SELECT
                {stored_request_kind_sql} AS request_kind_key,
                {stored_label_sql} AS request_kind_label,
                COUNT(*) AS request_count,
                MAX(CASE WHEN counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                MAX(CASE WHEN counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
            FROM auth_token_logs
            "
        ));
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_request_kind_sql = token_log_request_kind_key_sql("path", "request_kind_key");
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "counts_business_quota",
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");
        Self::push_token_logs_catalog_filters(
            &mut stored_query,
            token_id,
            since,
            until,
            filters,
            stored_request_kind_sql,
            &legacy_request_kind_predicate_sql,
            &legacy_request_kind_sql,
            &stored_operational_class_case_sql,
            &legacy_operational_class_case_sql,
            &stored_result_bucket_sql,
            &legacy_result_bucket_sql,
        );
        stored_query.push(" AND ");
        stored_query.push(canonical_request_kind_predicate_sql.clone());
        stored_query.push(" GROUP BY 1, 2");

        let stored_options = stored_query
            .build_query_as::<RequestKindOptionRow>()
            .fetch_all(&self.pool)
            .await?;
        let legacy_label_sql = canonical_request_kind_label_sql(&legacy_request_kind_sql);
        let mut legacy_query = QueryBuilder::<Sqlite>::new(format!(
            "
            SELECT
                {legacy_request_kind_sql} AS request_kind_key,
                {legacy_label_sql} AS request_kind_label,
                COUNT(*) AS request_count,
                MAX(CASE WHEN counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                MAX(CASE WHEN counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
            FROM auth_token_logs
            "
        ));
        Self::push_token_logs_catalog_filters(
            &mut legacy_query,
            token_id,
            since,
            until,
            filters,
            stored_request_kind_sql,
            &legacy_request_kind_predicate_sql,
            &legacy_request_kind_sql,
            &stored_operational_class_case_sql,
            &legacy_operational_class_case_sql,
            &stored_result_bucket_sql,
            &legacy_result_bucket_sql,
        );
        legacy_query.push(" AND ");
        legacy_query.push(legacy_request_kind_predicate_sql.as_str());
        legacy_query.push(" GROUP BY 1, 2");

        let legacy_options = legacy_query
            .build_query_as::<RequestKindOptionRow>()
            .fetch_all(&self.pool)
            .await?;
        let mut options_by_key = BTreeMap::<String, (String, bool, bool, i64)>::new();
        for (key, label, request_count, has_billable, has_non_billable) in
            stored_options.into_iter().chain(legacy_options)
        {
            match options_by_key.get_mut(&key) {
                Some((
                    current_label,
                    current_has_billable,
                    current_has_non_billable,
                    current_count,
                )) if prefer_request_kind_label(current_label, &label) => {
                    *current_label = label;
                    *current_has_billable |= has_billable != 0;
                    *current_has_non_billable |= has_non_billable != 0;
                    *current_count += request_count;
                }
                Some((_, current_has_billable, current_has_non_billable, current_count)) => {
                    *current_has_billable |= has_billable != 0;
                    *current_has_non_billable |= has_non_billable != 0;
                    *current_count += request_count;
                }
                None => {
                    options_by_key.insert(
                        key,
                        (
                            label,
                            has_billable != 0,
                            has_non_billable != 0,
                            request_count,
                        ),
                    );
                }
            }
        }

        let mut normalized_options = options_by_key
            .into_iter()
            .map(
                |(key, (label, has_billable, has_non_billable, count))| TokenRequestKindOption {
                    protocol_group: token_request_kind_protocol_group(&key).to_string(),
                    billing_group: token_request_kind_option_billing_group(
                        &key,
                        has_billable,
                        has_non_billable,
                    )
                    .to_string(),
                    key,
                    label,
                    count,
                },
            )
            .collect::<Vec<_>>();
        normalized_options.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.key.cmp(&right.key))
        });

        Ok(normalized_options)
    }

    pub async fn fetch_token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        let hours = hours.clamp(1, 168); // up to 7 days
        let now_ts = Utc::now().timestamp();
        let current_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let window_start = current_bucket - (hours - 1) * SECS_PER_HOUR;
        let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(window_start)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(bucket_start, success_count, system_failure_count, external_failure_count)| {
                    TokenHourlyBucket {
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    }
                },
            )
            .collect())
    }

    pub async fn fetch_token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        if until <= since {
            return Err(ProxyError::Other("invalid usage window".into()));
        }
        if bucket_secs <= 0 {
            return Err(ProxyError::Other("bucket_secs must be positive".into()));
        }
        let bucket_secs = match bucket_secs {
            s if s == SECS_PER_HOUR => SECS_PER_HOUR,
            s if s == SECS_PER_DAY => SECS_PER_DAY,
            _ => {
                return Err(ProxyError::Other(
                    "bucket_secs must be either 3600 (hour) or 86400 (day)".into(),
                ));
            }
        };
        let span = until - since;
        let mut bucket_count = span / bucket_secs;
        if span % bucket_secs != 0 {
            bucket_count += 1;
        }
        if bucket_count > 1000 {
            return Err(ProxyError::Other(
                "requested usage series is too large".into(),
            ));
        }
        if bucket_secs == SECS_PER_HOUR {
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            Ok(rows
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect())
        } else {
            // Aggregate hourly stats into daily buckets.
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            let mut by_day: HashMap<i64, (i64, i64, i64)> = HashMap::new();
            for (bucket_start, success, system_failure, external_failure) in rows {
                let day_start = bucket_start - (bucket_start % SECS_PER_DAY);
                let entry = by_day.entry(day_start).or_insert((0, 0, 0));
                entry.0 += success;
                entry.1 += system_failure;
                entry.2 += external_failure;
            }

            let mut buckets: Vec<TokenUsageBucket> = by_day
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        (success_count, system_failure_count, external_failure_count),
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect();
            buckets.sort_by_key(|b| b.bucket_start);
            Ok(buckets)
        }
    }

    pub(crate) async fn reset_monthly(&self) -> Result<(), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        let now_ts = now.timestamp();

        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE status = ?
              AND status_changed_at IS NOT NULL
              AND status_changed_at < ?
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now_ts)
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn mark_quota_exhausted(&self, key: &str) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?, last_used_at = ?
            WHERE api_key = ? AND status NOT IN (?, ?) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(now)
        .bind(now)
        .bind(key)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn restore_active_status(&self, key: &str) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE api_key = ? AND status = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn quarantine_key_by_id(
        &self,
        key_id: &str,
        source: &str,
        reason_code: &str,
        reason_summary: &str,
        reason_detail: &str,
    ) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let quarantine_id = nanoid!(12);
        let insert_result = sqlx::query(
            r#"
            INSERT INTO api_key_quarantines (
                id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
            ON CONFLICT(key_id) WHERE cleared_at IS NULL DO NOTHING
            "#,
        )
        .bind(quarantine_id)
        .bind(key_id)
        .bind(source)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(reason_detail)
        .bind(now)
        .execute(&self.pool)
        .await?;

        if insert_result.rows_affected() == 0 {
            sqlx::query(
                r#"
                UPDATE api_key_quarantines
                SET source = ?, reason_code = ?, reason_summary = ?, reason_detail = ?
                WHERE key_id = ? AND cleared_at IS NULL
                "#,
            )
            .bind(source)
            .bind(reason_code)
            .bind(reason_summary)
            .bind(reason_detail)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
            return Ok(false);
        }

        Ok(true)
    }

    pub(crate) async fn clear_key_quarantine_by_id(
        &self,
        key_id: &str,
    ) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_key_quarantines
            SET cleared_at = ?
            WHERE key_id = ? AND cleared_at IS NULL
            "#,
        )
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    // Admin ops: add/undelete key by secret
    pub(crate) async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.add_or_undelete_key_in_group(api_key, None).await
    }

    // Admin ops: add/undelete key by secret and optionally assign a group.
    pub(crate) async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        let (id, _) = self
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key, group, None, None, None, false,
            )
            .await?;
        Ok(id)
    }

    // Admin ops: add/undelete key by secret with status
    pub(crate) async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration(
            api_key, None, None, None, None, false,
        )
        .await
    }

    // Admin ops: add/undelete key by secret with status and optional group assignment.
    //
    // Behavior:
    // - created / undeleted: set group_name when group is provided and non-empty
    // - existed: set group_name only if the stored group is empty (do not override)
    pub(crate) async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration(
            api_key, group, None, None, None, false,
        )
        .await
    }

    pub(crate) async fn add_or_undelete_key_with_status_in_group_and_registration(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        proxy_affinity: Option<&forward_proxy::ForwardProxyAffinityRecord>,
        hint_only_proxy_affinity: bool,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let normalized_group = group
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string);
        let normalized_registration_ip = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_registration_region = registration_region
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mut retry_idx = 0usize;

        loop {
            match self
                .add_or_undelete_key_with_status_in_group_once(
                    api_key,
                    normalized_group.as_deref(),
                    normalized_registration_ip.as_deref(),
                    normalized_registration_region.as_deref(),
                    proxy_affinity,
                    hint_only_proxy_affinity,
                )
                .await
            {
                Ok(result) => return Ok(result),
                Err(err)
                    if is_transient_sqlite_write_error(&err)
                        && retry_idx < API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS.len() =>
                {
                    let backoff_ms = API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS[retry_idx];
                    retry_idx += 1;
                    let key_preview = preview_key(api_key);
                    eprintln!(
                        "api key upsert transient sqlite error (api_key_preview={}, attempt={}, backoff={}ms): {}",
                        key_preview, retry_idx, backoff_ms, err
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub(crate) async fn add_or_undelete_key_with_status_in_group_once(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        proxy_affinity: Option<&forward_proxy::ForwardProxyAffinityRecord>,
        hint_only_proxy_affinity: bool,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().timestamp();

        let operation_result: Result<(String, ApiKeyUpsertStatus), ProxyError> = async {
            if let Some((id, deleted_at, existing_group, existing_registration_ip, existing_registration_region)) =
                sqlx::query_as::<_, (String, Option<i64>, Option<String>, Option<String>, Option<String>)>(
                    "SELECT id, deleted_at, group_name, registration_ip, registration_region FROM api_keys WHERE api_key = ? LIMIT 1",
                )
                .bind(api_key)
                .fetch_optional(&mut *tx)
                .await?
            {
                let existing_empty = existing_group
                    .as_deref()
                    .map(str::trim)
                    .map(|g| g.is_empty())
                    .unwrap_or(true);
                let existing_has_registration_metadata =
                    existing_registration_ip.is_some() || existing_registration_region.is_some();
                let should_refresh_registration =
                    registration_ip.is_some() || registration_region.is_some();
                let should_persist_proxy_affinity =
                    !hint_only_proxy_affinity || !existing_has_registration_metadata;

                let mut assignments = Vec::new();
                if deleted_at.is_some() {
                    assignments.push("deleted_at = NULL".to_string());
                }
                if group.is_some() && existing_empty {
                    assignments.push("group_name = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_ip = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_region = ?".to_string());
                }

                if !assignments.is_empty() {
                    let mut query = String::from("UPDATE api_keys SET ");
                    query.push_str(&assignments.join(", "));
                    query.push_str(" WHERE id = ?");
                    let mut sql = sqlx::query(&query);
                    if let Some(group) = group
                        && existing_empty
                    {
                        sql = sql.bind(group);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(registration_ip);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(registration_region);
                    }
                    sql.bind(&id).execute(&mut *tx).await?;
                }
                if should_persist_proxy_affinity
                    && let Some(proxy_affinity) = proxy_affinity
                {
                    sqlx::query(
                        r#"
                        INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                        VALUES (?1, ?2, ?3, strftime('%s', 'now'))
                        ON CONFLICT(key_id) DO UPDATE SET
                            primary_proxy_key = excluded.primary_proxy_key,
                            secondary_proxy_key = excluded.secondary_proxy_key,
                            updated_at = strftime('%s', 'now')
                        "#,
                    )
                    .bind(&id)
                    .bind(proxy_affinity.primary_proxy_key.as_deref())
                    .bind(proxy_affinity.secondary_proxy_key.as_deref())
                    .execute(&mut *tx)
                    .await?;
                }

                if deleted_at.is_some() {
                    return Ok((id, ApiKeyUpsertStatus::Undeleted));
                }

                return Ok((id, ApiKeyUpsertStatus::Existed));
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (
                    id,
                    api_key,
                    group_name,
                    registration_ip,
                    registration_region,
                    status,
                    created_at,
                    status_changed_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(api_key)
            .bind(group)
            .bind(registration_ip)
            .bind(registration_region)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if let Some(proxy_affinity) = proxy_affinity {
                sqlx::query(
                    r#"
                    INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                    VALUES (?1, ?2, ?3, strftime('%s', 'now'))
                    ON CONFLICT(key_id) DO UPDATE SET
                        primary_proxy_key = excluded.primary_proxy_key,
                        secondary_proxy_key = excluded.secondary_proxy_key,
                        updated_at = strftime('%s', 'now')
                    "#,
                )
                .bind(&id)
                .bind(proxy_affinity.primary_proxy_key.as_deref())
                .bind(proxy_affinity.secondary_proxy_key.as_deref())
                .execute(&mut *tx)
                .await?;
            }
            Ok((id, ApiKeyUpsertStatus::Created))
        }
        .await;

        match operation_result {
            Ok(result) => {
                tx.commit().await?;
                Ok(result)
            }
            Err(err) => {
                tx.rollback().await.ok();
                Err(err)
            }
        }
    }

    // Admin ops: soft-delete by ID (mark deleted_at)
    pub(crate) async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND status IN (?, ?) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key_id)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn touch_key(&self, key: &str, timestamp: i64) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = ?
            WHERE api_key = ?
            "#,
        )
        .bind(timestamp)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn log_attempt(&self, entry: AttemptLog<'_>) -> Result<i64, ProxyError> {
        let created_at = Utc::now().timestamp();
        let status_code = entry.status.map(|code| code.as_u16() as i64);
        let failure_kind = entry.failure_kind.map(str::to_string).or_else(|| {
            if entry.outcome == OUTCOME_ERROR {
                classify_failure_kind(
                    entry.path,
                    status_code,
                    entry.tavily_status_code,
                    entry.error,
                    entry.response_body,
                )
            } else {
                None
            }
        });
        let key_effect_summary = entry.key_effect_summary.map(str::to_string);
        let binding_effect_summary = entry.binding_effect_summary.map(str::to_string);
        let selection_effect_summary = entry.selection_effect_summary.map(str::to_string);
        let request_kind = normalize_request_kind_for_response_context(
            classify_token_request_kind(entry.path, Some(entry.request_body)),
            ResponseRequestKindContext {
                method: entry.method.as_str(),
                path: entry.path,
                http_status: status_code,
                tavily_status: entry.tavily_status_code,
                failure_kind: failure_kind.as_deref(),
                error_message: entry.error,
                response_body: entry.response_body,
            },
        );

        let forwarded_json =
            serde_json::to_string(entry.forwarded_headers).unwrap_or_else(|_| "[]".to_string());
        let dropped_json =
            serde_json::to_string(entry.dropped_headers).unwrap_or_else(|_| "[]".to_string());

        let bucket_start = local_day_bucket_start_utc_ts(created_at);
        let (bucket_success, bucket_error, bucket_quota_exhausted) = match entry.outcome {
            OUTCOME_SUCCESS => (1_i64, 0_i64, 0_i64),
            OUTCOME_ERROR => (0_i64, 1_i64, 0_i64),
            OUTCOME_QUOTA_EXHAUSTED => (0_i64, 0_i64, 1_i64),
            _ => (0_i64, 0_i64, 0_i64),
        };
        let request_value_bucket =
            request_value_bucket_for_request_log(&request_kind.key, Some(entry.request_body));
        let (
            bucket_valuable_success,
            bucket_valuable_failure,
            bucket_other_success,
            bucket_other_failure,
            bucket_unknown,
        ) = match request_value_bucket {
            RequestValueBucket::Valuable => match entry.outcome {
                OUTCOME_SUCCESS => (1_i64, 0_i64, 0_i64, 0_i64, 0_i64),
                OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => (0_i64, 1_i64, 0_i64, 0_i64, 0_i64),
                _ => (0_i64, 0_i64, 0_i64, 0_i64, 0_i64),
            },
            RequestValueBucket::Other => match entry.outcome {
                OUTCOME_SUCCESS => (0_i64, 0_i64, 1_i64, 0_i64, 0_i64),
                OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => (0_i64, 0_i64, 0_i64, 1_i64, 0_i64),
                _ => (0_i64, 0_i64, 0_i64, 0_i64, 0_i64),
            },
            RequestValueBucket::Unknown => (0_i64, 0_i64, 0_i64, 0_i64, 1_i64),
        };
        let dashboard_rollup_counts = Self::dashboard_rollup_counts_for_request(
            &request_kind.key,
            Some(entry.request_body),
            entry.outcome,
            failure_kind.as_deref(),
            0,
        );
        let minute_bucket_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;

        let mut tx = self.pool.begin().await?;

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_kind_key,
                request_kind_label,
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
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(entry.key_id)
        .bind(entry.auth_token_id)
        .bind(entry.method.as_str())
        .bind(entry.path)
        .bind(entry.query)
        .bind(status_code)
        .bind(entry.tavily_status_code)
        .bind(entry.error)
        .bind(entry.outcome)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(None::<i64>)
        .bind(failure_kind)
        .bind(entry.key_effect_code)
        .bind(key_effect_summary)
        .bind(entry.binding_effect_code)
        .bind(binding_effect_summary)
        .bind(entry.selection_effect_code)
        .bind(selection_effect_summary)
        .bind(entry.gateway_mode)
        .bind(entry.experiment_variant)
        .bind(entry.proxy_session_id)
        .bind(entry.routing_subject_hash)
        .bind(entry.upstream_operation)
        .bind(entry.fallback_reason)
        .bind(entry.request_body)
        .bind(entry.response_body)
        .bind(forwarded_json)
        .bind(dropped_json)
        .bind(created_at)
        .fetch_one(&mut *tx)
        .await?;

        // Daily API-key rollup bucket (bucket_secs=86400, aligned to local midnight).
        if let Some(key_id) = entry.key_id {
            sqlx::query(
                r#"
                INSERT INTO api_key_usage_buckets (
                    api_key_id,
                    bucket_start,
                    bucket_secs,
                    total_requests,
                    success_count,
                    error_count,
                    quota_exhausted_count,
                    valuable_success_count,
                    valuable_failure_count,
                    other_success_count,
                    other_failure_count,
                    unknown_count,
                    updated_at
                ) VALUES (?, ?, 86400, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(api_key_id, bucket_start, bucket_secs)
                DO UPDATE SET
                    total_requests = total_requests + 1,
                    success_count = success_count + excluded.success_count,
                    error_count = error_count + excluded.error_count,
                    quota_exhausted_count = quota_exhausted_count + excluded.quota_exhausted_count,
                    valuable_success_count =
                        valuable_success_count + excluded.valuable_success_count,
                    valuable_failure_count =
                        valuable_failure_count + excluded.valuable_failure_count,
                    other_success_count = other_success_count + excluded.other_success_count,
                    other_failure_count = other_failure_count + excluded.other_failure_count,
                    unknown_count = unknown_count + excluded.unknown_count,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(key_id)
            .bind(bucket_start)
            .bind(bucket_success)
            .bind(bucket_error)
            .bind(bucket_quota_exhausted)
            .bind(bucket_valuable_success)
            .bind(bucket_valuable_failure)
            .bind(bucket_other_success)
            .bind(bucket_other_failure)
            .bind(bucket_unknown)
            .bind(created_at)
            .execute(&mut *tx)
            .await?;
        }

        Self::upsert_dashboard_request_rollup_bucket(
            &mut tx,
            minute_bucket_start,
            SECS_PER_MINUTE,
            dashboard_rollup_counts,
            created_at,
        )
        .await?;
        Self::upsert_dashboard_request_rollup_bucket(
            &mut tx,
            bucket_start,
            SECS_PER_DAY,
            dashboard_rollup_counts,
            created_at,
        )
        .await?;

        tx.commit().await?;
        self.invalidate_request_logs_catalog_cache().await;
        Ok(request_log_id)
    }

    pub(crate) fn api_key_metrics_from_clause() -> &'static str {
        r#"
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
        "#
    }

    pub(crate) fn api_key_metrics_query(include_quarantine_detail: bool) -> String {
        let quarantine_detail_sql = if include_quarantine_detail {
            "aq.reason_detail AS quarantine_reason_detail,"
        } else {
            "NULL AS quarantine_reason_detail,"
        };
        format!(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.registration_ip,
                ak.registration_region,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                {quarantine_detail_sql}
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            {}
            "#,
            Self::api_key_metrics_from_clause(),
        )
    }

    pub(crate) fn map_api_key_metrics_row(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<ApiKeyMetrics, sqlx::Error> {
        let id: String = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let group_name: Option<String> = row.try_get("group_name")?;
        let registration_ip: Option<String> = row.try_get("registration_ip")?;
        let registration_region: Option<String> = row.try_get("registration_region")?;
        let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
        let last_used_at: i64 = row.try_get("last_used_at")?;
        let deleted_at: Option<i64> = row.try_get("deleted_at")?;
        let quota_limit: Option<i64> = row.try_get("quota_limit")?;
        let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
        let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
        let total_requests: i64 = row.try_get("total_requests")?;
        let success_count: i64 = row.try_get("success_count")?;
        let error_count: i64 = row.try_get("error_count")?;
        let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
        let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
        let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
        let quarantine_reason_summary: Option<String> = row.try_get("quarantine_reason_summary")?;
        let quarantine_reason_detail: Option<String> = row.try_get("quarantine_reason_detail")?;
        let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

        Ok(ApiKeyMetrics {
            id,
            status,
            group_name: normalize_optional_api_key_field(group_name),
            registration_ip: normalize_optional_api_key_field(registration_ip),
            registration_region: normalize_optional_api_key_field(registration_region),
            status_changed_at: status_changed_at.and_then(normalize_timestamp),
            last_used_at: normalize_timestamp(last_used_at),
            deleted_at: deleted_at.and_then(normalize_timestamp),
            quota_limit,
            quota_remaining,
            quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                source,
                reason_code: quarantine_reason_code.unwrap_or_default(),
                reason_summary: quarantine_reason_summary.unwrap_or_default(),
                reason_detail: quarantine_reason_detail.unwrap_or_default(),
                created_at: quarantine_created_at.unwrap_or_default(),
            }),
        })
    }

    pub(crate) fn normalize_api_key_groups(groups: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for group in groups {
            let value = group.trim().to_string();
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_regions(regions: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for region in regions {
            let value = region.trim().to_string();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_statuses(statuses: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for status in statuses {
            let value = status.trim().to_ascii_lowercase();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn push_api_key_group_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        groups: &'a [String],
    ) {
        if groups.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if group.is_empty() {
                builder.push("(TRIM(COALESCE(ak.group_name, '')) = '')");
            } else {
                builder
                    .push("(TRIM(COALESCE(ak.group_name, '')) = ")
                    .push_bind(group)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_status_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        statuses: &'a [String],
    ) {
        if statuses.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, status) in statuses.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if status == "quarantined" {
                builder.push("(aq.key_id IS NOT NULL)");
            } else {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_registration_ip_filter<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        registration_ip: Option<&'a str>,
    ) {
        let Some(registration_ip) = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        builder
            .push(" AND TRIM(COALESCE(ak.registration_ip, '')) = ")
            .push_bind(registration_ip);
    }

    pub(crate) fn push_api_key_region_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        regions: &'a [String],
    ) {
        if regions.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, region) in regions.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            builder
                .push("(TRIM(COALESCE(ak.registration_region, '')) = ")
                .push_bind(region)
                .push(")");
        }
        builder.push(")");
    }

    pub(crate) async fn fetch_api_key_group_facets(
        &self,
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT TRIM(COALESCE(ak.group_name, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_status_filters(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        Self::push_api_key_region_filters(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_status_facets(
        &self,
        groups: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT CASE WHEN aq.key_id IS NOT NULL THEN 'quarantined' ELSE ak.status END AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut builder, groups);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        Self::push_api_key_region_filters(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_region_facets(
        &self,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT TRIM(COALESCE(ak.registration_region, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut builder, groups);
        Self::push_api_key_status_filters(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        builder.push(" AND TRIM(COALESCE(ak.registration_region, '')) != ''");
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_metrics(
        &self,
        include_quarantine_detail: bool,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let query = format!(
            "{} ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
            Self::api_key_metrics_query(include_quarantine_detail),
        );
        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_dashboard_exhausted_api_key_metrics(
        &self,
        limit: usize,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let limit = limit.clamp(1, 50) as i64;
        let statuses = vec![STATUS_EXHAUSTED.to_string()];
        let mut items_builder = QueryBuilder::<Sqlite>::new(Self::api_key_metrics_query(false));
        Self::push_api_key_status_filters(&mut items_builder, &statuses);
        items_builder.push(
            " ORDER BY COALESCE(ak.last_used_at, 0) DESC, COALESCE(ak.status_changed_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(limit);
        items_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_dashboard_exhausted_api_key_ids(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        let limit = limit.clamp(1, 50) as i64;
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT ak.id
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
              AND ak.status = ?
            ORDER BY COALESCE(ak.last_used_at, 0) DESC, COALESCE(ak.status_changed_at, 0) DESC, ak.id ASC
            LIMIT ?
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_metrics_page(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        let requested_page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let groups = Self::normalize_api_key_groups(groups);
        let statuses = Self::normalize_api_key_statuses(statuses);
        let registration_ip = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let regions = Self::normalize_api_key_regions(regions);

        let mut count_builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
        count_builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut count_builder, &groups);
        Self::push_api_key_status_filters(&mut count_builder, &statuses);
        Self::push_api_key_registration_ip_filter(&mut count_builder, registration_ip);
        Self::push_api_key_region_filters(&mut count_builder, &regions);
        let total = count_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;
        let total_pages = ((total + per_page - 1) / per_page).max(1);
        let page = requested_page.min(total_pages);
        let offset = (page - 1) * per_page;

        let mut items_builder = QueryBuilder::<Sqlite>::new(Self::api_key_metrics_query(false));
        Self::push_api_key_group_filters(&mut items_builder, &groups);
        Self::push_api_key_status_filters(&mut items_builder, &statuses);
        Self::push_api_key_registration_ip_filter(&mut items_builder, registration_ip);
        Self::push_api_key_region_filters(&mut items_builder, &regions);
        items_builder.push(
            " ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(per_page);
        items_builder.push(" OFFSET ").push_bind(offset);
        let items = items_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()?;

        let group_counts = self
            .fetch_api_key_group_facets(&statuses, registration_ip, &regions)
            .await?;
        let status_counts = self
            .fetch_api_key_status_facets(&groups, registration_ip, &regions)
            .await?;
        let region_counts = self
            .fetch_api_key_region_facets(&groups, &statuses, registration_ip)
            .await?;

        Ok(PaginatedApiKeyMetrics {
            items,
            total,
            page,
            per_page,
            facets: ApiKeyListFacets {
                groups: group_counts,
                statuses: status_counts,
                regions: region_counts,
            },
        })
    }

    pub(crate) async fn fetch_api_key_metric_by_id(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.registration_ip,
                ak.registration_region,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                aq.reason_detail AS quarantine_reason_detail,
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL AND ak.id = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| -> Result<ApiKeyMetrics, sqlx::Error> {
            let id: String = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            let group_name: Option<String> = row.try_get("group_name")?;
            let registration_ip: Option<String> = row.try_get("registration_ip")?;
            let registration_region: Option<String> = row.try_get("registration_region")?;
            let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
            let last_used_at: i64 = row.try_get("last_used_at")?;
            let deleted_at: Option<i64> = row.try_get("deleted_at")?;
            let quota_limit: Option<i64> = row.try_get("quota_limit")?;
            let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
            let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
            let total_requests: i64 = row.try_get("total_requests")?;
            let success_count: i64 = row.try_get("success_count")?;
            let error_count: i64 = row.try_get("error_count")?;
            let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
            let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
            let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
            let quarantine_reason_summary: Option<String> =
                row.try_get("quarantine_reason_summary")?;
            let quarantine_reason_detail: Option<String> =
                row.try_get("quarantine_reason_detail")?;
            let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

            Ok(ApiKeyMetrics {
                id,
                status,
                group_name: normalize_optional_api_key_field(group_name),
                registration_ip: normalize_optional_api_key_field(registration_ip),
                registration_region: normalize_optional_api_key_field(registration_region),
                status_changed_at: status_changed_at.and_then(normalize_timestamp),
                last_used_at: normalize_timestamp(last_used_at),
                deleted_at: deleted_at.and_then(normalize_timestamp),
                quota_limit,
                quota_remaining,
                quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                    source,
                    reason_code: quarantine_reason_code.unwrap_or_default(),
                    reason_summary: quarantine_reason_summary.unwrap_or_default(),
                    reason_detail: quarantine_reason_detail.unwrap_or_default(),
                    created_at: quarantine_created_at.unwrap_or_default(),
                }),
            })
        })
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;

        let rows = sqlx::query(
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
                request_kind_key,
                request_kind_label,
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
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            FROM request_logs
            WHERE visibility = ?
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let records = rows
            .into_iter()
            .map(Self::map_request_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub(crate) async fn fetch_latest_visible_request_log_id(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT id
            FROM request_logs
            WHERE visibility = ?
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_optional(&self.pool)
        .await
        .map(|value| value.flatten())
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_logs_page(
        &self,
        result_status: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        let request_kinds: Vec<String> = Vec::new();
        let result = self
            .fetch_request_logs_page(
                None,
                None,
                &request_kinds,
                result_status,
                None,
                None,
                None,
                None,
                None,
                operational_class,
                page,
                per_page,
                true,
                true,
            )
            .await?;
        Ok((result.items, result.total))
    }

    fn map_request_log_row(row: sqlx::sqlite::SqliteRow) -> Result<RequestLogRecord, sqlx::Error> {
        let forwarded = parse_header_list(row.try_get::<Option<String>, _>("forwarded_headers")?);
        let dropped = parse_header_list(row.try_get::<Option<String>, _>("dropped_headers")?);
        let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
        let response_body: Option<Vec<u8>> = row.try_get("response_body")?;
        let method: String = row.try_get("method")?;
        let path: String = row.try_get("path")?;
        let query: Option<String> = row.try_get("query")?;
        let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
        let stored_request_kind_label: Option<String> = row.try_get("request_kind_label")?;
        let stored_request_kind_detail: Option<String> = row.try_get("request_kind_detail")?;
        let request_kind = canonicalize_request_log_request_kind(
            path.as_str(),
            request_body.as_deref(),
            stored_request_kind_key.clone(),
            stored_request_kind_label.clone(),
            stored_request_kind_detail.clone(),
        );
        let result_status: String = row.try_get("result_status")?;
        let failure_kind: Option<String> = row.try_get("failure_kind")?;
        let request_kind_protocol_group =
            match row.try_get::<Option<String>, _>("request_kind_protocol_group") {
                Ok(Some(value)) => value,
                _ => token_request_kind_protocol_group(&request_kind.key).to_string(),
            };
        let request_kind_billing_group =
            match row.try_get::<Option<String>, _>("request_kind_billing_group") {
                Ok(Some(value)) => value,
                _ => token_request_kind_billing_group_for_request_log(
                    &request_kind.key,
                    request_body.as_deref(),
                )
                .to_string(),
            };
        let operational_class = match row.try_get::<Option<String>, _>("operational_class") {
            Ok(Some(value)) => value,
            _ => operational_class_for_request_log(
                &request_kind.key,
                request_body.as_deref(),
                result_status.as_str(),
                failure_kind.as_deref(),
            )
            .to_string(),
        };

        Ok(RequestLogRecord {
            id: row.try_get("id")?,
            key_id: row.try_get("api_key_id")?,
            auth_token_id: row.try_get("auth_token_id")?,
            method,
            path,
            query,
            status_code: row.try_get("status_code")?,
            tavily_status_code: row.try_get("tavily_status_code")?,
            error_message: row.try_get("error_message")?,
            business_credits: row.try_get("business_credits")?,
            request_kind_key: request_kind.key,
            request_kind_label: request_kind.label,
            request_kind_detail: request_kind.detail,
            request_kind_protocol_group,
            request_kind_billing_group,
            result_status,
            failure_kind,
            key_effect_code: row.try_get("key_effect_code")?,
            key_effect_summary: row.try_get("key_effect_summary")?,
            binding_effect_code: row.try_get("binding_effect_code")?,
            binding_effect_summary: row.try_get("binding_effect_summary")?,
            selection_effect_code: row.try_get("selection_effect_code")?,
            selection_effect_summary: row.try_get("selection_effect_summary")?,
            gateway_mode: row.try_get("gateway_mode")?,
            experiment_variant: row.try_get("experiment_variant")?,
            proxy_session_id: row.try_get("proxy_session_id")?,
            routing_subject_hash: row.try_get("routing_subject_hash")?,
            upstream_operation: row.try_get("upstream_operation")?,
            fallback_reason: row.try_get("fallback_reason")?,
            operational_class,
            request_body: request_body.unwrap_or_default(),
            response_body: response_body.unwrap_or_default(),
            created_at: row.try_get("created_at")?,
            forwarded_headers: forwarded,
            dropped_headers: dropped,
        })
    }

    fn map_request_log_bodies_row(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<RequestLogBodiesRecord, sqlx::Error> {
        Ok(RequestLogBodiesRecord {
            request_body: row.try_get("request_body")?,
            response_body: row.try_get("response_body")?,
        })
    }

    pub(crate) async fn fetch_request_log_bodies(
        &self,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT request_body, response_body
            FROM request_logs
            WHERE id = ? AND visibility = ?
            LIMIT 1
            "#,
        )
        .bind(log_id)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_optional(&self.pool)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_request_log_bodies(
        &self,
        key_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT request_body, response_body
            FROM request_logs
            WHERE id = ? AND api_key_id = ? AND visibility = ?
            LIMIT 1
            "#,
        )
        .bind(log_id)
        .bind(key_id)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_optional(&self.pool)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_token_log_bodies(
        &self,
        token_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT rl.request_body, rl.response_body
            FROM auth_token_logs atl
            LEFT JOIN request_logs rl
              ON rl.id = atl.request_log_id
             AND rl.visibility = ?
            WHERE atl.id = ? AND atl.token_id = ?
            LIMIT 1
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(log_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    fn request_logs_cursor(created_at: i64, id: i64) -> RequestLogsCursor {
        RequestLogsCursor { created_at, id }
    }

    fn request_logs_cursor_for_record(record: &RequestLogRecord) -> RequestLogsCursor {
        Self::request_logs_cursor(record.created_at, record.id)
    }

    fn request_logs_cursor_for_token_record(record: &TokenLogRecord) -> RequestLogsCursor {
        Self::request_logs_cursor(record.created_at, record.id)
    }

    fn push_desc_cursor_clause<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        created_at_column: &str,
        id_column: &str,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        has_where: bool,
    ) -> bool {
        let Some(cursor) = cursor else {
            return has_where;
        };

        builder.push(if has_where { " AND (" } else { " WHERE (" });
        builder.push(created_at_column);
        builder.push(match direction {
            RequestLogsCursorDirection::Older => " < ",
            RequestLogsCursorDirection::Newer => " > ",
        });
        builder.push_bind(cursor.created_at);
        builder.push(" OR (");
        builder.push(created_at_column);
        builder.push(" = ");
        builder.push_bind(cursor.created_at);
        builder.push(" AND ");
        builder.push(id_column);
        builder.push(match direction {
            RequestLogsCursorDirection::Older => " < ",
            RequestLogsCursorDirection::Newer => " > ",
        });
        builder.push_bind(cursor.id);
        builder.push("))");
        true
    }

    fn request_logs_catalog_cache_key(
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        include_token_facets: bool,
        include_key_facets: bool,
    ) -> String {
        let scope = scoped_key_id
            .map(|key_id| format!("key:{key_id}"))
            .unwrap_or_else(|| "global".to_string());
        format!(
            "request_logs:{scope}:since={}:tokens={include_token_facets}:keys={include_key_facets}",
            since.unwrap_or_default()
        )
    }

    fn token_logs_catalog_cache_key(token_id: &str, since: i64, until: Option<i64>) -> String {
        format!(
            "token_logs:{token_id}:since={since}:until={}",
            until.unwrap_or_default()
        )
    }

    fn request_logs_catalog_filters_are_empty(filters: RequestLogsCatalogFilters<'_>) -> bool {
        filters.request_kinds.is_empty()
            && filters.result_status.is_none()
            && filters.key_effect_code.is_none()
            && filters.binding_effect_code.is_none()
            && filters.selection_effect_code.is_none()
            && filters.auth_token_id.is_none()
            && filters.key_id.is_none()
            && filters.operational_class.is_none()
    }

    fn token_logs_catalog_filters_are_empty(filters: TokenLogsCatalogFilters<'_>) -> bool {
        filters.request_kinds.is_empty()
            && filters.result_status.is_none()
            && filters.key_effect_code.is_none()
            && filters.binding_effect_code.is_none()
            && filters.selection_effect_code.is_none()
            && filters.key_id.is_none()
            && filters.operational_class.is_none()
    }

}
