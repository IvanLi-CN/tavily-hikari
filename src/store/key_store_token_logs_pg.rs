/// PostgreSQL twins of the modules in `key_store_token_logs.rs`.
///
/// The SQLite implementations stay in the source module; these variants are reachable
/// only through the attached PostgreSQL pool.
impl KeyStore {
    pub async fn fetch_token_logs_by_billing_pg(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
        billing_filter: TokenLogBillingFilter,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let mut builder = QueryBuilder::new(
            r#"
            SELECT auth_token_logs.id, auth_token_logs.api_key_id, auth_token_logs.method, auth_token_logs.path, auth_token_logs.query, auth_token_logs.http_status, auth_token_logs.mcp_status,
                   CASE
                       WHEN COALESCE(bl.billing_state, auth_token_logs.billing_state) = 'charged'
                       THEN COALESCE(bl.business_credits, auth_token_logs.business_credits)
                       ELSE NULL
                   END AS business_credits,
                   auth_token_logs.request_kind_key, auth_token_logs.request_kind_label, auth_token_logs.request_kind_detail,
                   auth_token_logs.counts_business_quota, auth_token_logs.result_status, auth_token_logs.error_message, auth_token_logs.failure_kind, auth_token_logs.key_effect_code,
                   auth_token_logs.key_effect_summary, auth_token_logs.binding_effect_code, auth_token_logs.binding_effect_summary,
                   auth_token_logs.selection_effect_code, auth_token_logs.selection_effect_summary,
                   auth_token_logs.gateway_mode, auth_token_logs.experiment_variant, auth_token_logs.proxy_session_id, auth_token_logs.routing_subject_hash,
                   auth_token_logs.upstream_operation, auth_token_logs.fallback_reason, auth_token_logs.created_at
            FROM core.auth_token_logs
            LEFT JOIN core.billing_ledger bl ON bl.auth_token_log_id = auth_token_logs.id
            WHERE auth_token_logs.token_id =
            "#,
        );
        builder.push_bind(token_id);
        if let Some(bid) = before_id {
            builder.push(" AND auth_token_logs.id < ");
            builder.push_bind(bid);
        }
        if billing_filter == TokenLogBillingFilter::Billable {
            builder.push(" AND auth_token_logs.counts_business_quota = 1");
        }
        builder.push(" ORDER BY auth_token_logs.created_at DESC, auth_token_logs.id DESC LIMIT ");
        builder.push_bind(limit);
        let rows = builder.build().fetch_all(self.pg()?).await?;

        Ok(rows
            .into_iter()
            .map(Self::map_token_log_row)
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn fetch_token_summary_since_pg(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        let now_ts = self.backend_time.now_ts();
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
            FROM core.token_usage_stats
            WHERE token_id = $1 AND bucket_secs = $2 AND bucket_start >= $3 AND bucket_start < $4
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(since)
        .bind(end_exclusive)
        .fetch_all(self.pg()?)
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
    pub async fn fetch_token_logs_cursor_page_pg(
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
        let started = Instant::now();
        let page_size = page_size.clamp(1, 200);
        let query_limit = page_size + 1;
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "auth_token_logs.request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql(
            "auth_token_logs.path",
            "auth_token_logs.request_kind_key",
        );
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let stored_result_bucket_sql = result_bucket_case_sql(
            &stored_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let legacy_result_bucket_sql = result_bucket_case_sql(
            &legacy_operational_class_case_sql,
            "auth_token_logs.result_status",
        );

        let mut rows_query = QueryBuilder::<sqlx::Postgres>::new(
            r#"
            SELECT auth_token_logs.id, auth_token_logs.api_key_id, auth_token_logs.method, auth_token_logs.path, auth_token_logs.query, auth_token_logs.http_status, auth_token_logs.mcp_status,
                   CASE
                       WHEN COALESCE(bl.billing_state, auth_token_logs.billing_state) = 'charged'
                       THEN COALESCE(bl.business_credits, auth_token_logs.business_credits)
                       ELSE NULL
                   END AS business_credits,
                   auth_token_logs.request_kind_key,
                   auth_token_logs.request_kind_label,
                   auth_token_logs.request_kind_detail,
                   auth_token_logs.counts_business_quota,
                   auth_token_logs.result_status, auth_token_logs.error_message, auth_token_logs.failure_kind, auth_token_logs.key_effect_code,
                   auth_token_logs.key_effect_summary, auth_token_logs.binding_effect_code, auth_token_logs.binding_effect_summary,
                   auth_token_logs.selection_effect_code, auth_token_logs.selection_effect_summary,
                   auth_token_logs.gateway_mode, auth_token_logs.experiment_variant, auth_token_logs.proxy_session_id, auth_token_logs.routing_subject_hash,
                   auth_token_logs.upstream_operation, auth_token_logs.fallback_reason, auth_token_logs.created_at
            FROM core.auth_token_logs
            LEFT JOIN core.billing_ledger bl ON bl.auth_token_log_id = auth_token_logs.id
            WHERE auth_token_logs.token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND auth_token_logs.created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND auth_token_logs.created_at < ");
            rows_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            rows_query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
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
            rows_query.push(" AND auth_token_logs.api_key_id = ");
            rows_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            rows_query.push(" AND ");
            Self::push_request_kind_filter_clause_pg(
                &mut rows_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            rows_query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                &mut rows_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        Self::push_desc_cursor_clause_pg(
            &mut rows_query,
            "auth_token_logs.created_at",
            "auth_token_logs.id",
            cursor,
            direction,
            true,
        );
        match direction {
            RequestLogsCursorDirection::Older => {
                rows_query.push(
                    " ORDER BY auth_token_logs.created_at DESC, auth_token_logs.id DESC LIMIT ",
                );
            }
            RequestLogsCursorDirection::Newer => {
                rows_query.push(
                    " ORDER BY auth_token_logs.created_at ASC, auth_token_logs.id ASC LIMIT ",
                );
            }
        }
        rows_query.push_bind(query_limit);

        let mut rows = rows_query.build().fetch_all(self.pg()?).await?;
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

        let result = TokenLogsCursorPage {
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
        };
        emit_low_memory_protection_decision(
            "admin_read",
            PerfLogScope {
                route: Some("/api/tokens/:id/logs/list"),
                scope: Some("token"),
                page_size: Some(page_size),
                degraded: Some("full"),
                ..Default::default()
            },
        );
        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "token_logs_list_completed",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/tokens/:id/logs/list"),
                scope: Some("token"),
                page_size: Some(page_size),
                row_count: Some(result.items.len()),
                degraded: Some("full"),
                ..Default::default()
            },
        );
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_token_logs_page_pg(
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
        let stored_request_kind_sql = "auth_token_logs.request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql(
            "auth_token_logs.path",
            "auth_token_logs.request_kind_key",
        );
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let stored_result_bucket_sql = result_bucket_case_sql(
            &stored_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let legacy_result_bucket_sql = result_bucket_case_sql(
            &legacy_operational_class_case_sql,
            "auth_token_logs.result_status",
        );

        let mut total_query = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COUNT(*) FROM core.auth_token_logs WHERE auth_token_logs.token_id = ",
        );
        total_query.push_bind(token_id);
        total_query.push(" AND auth_token_logs.created_at >= ");
        total_query.push_bind(since);
        if let Some(until) = until {
            total_query.push(" AND auth_token_logs.created_at < ");
            total_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            total_query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                &mut total_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(key_effect_code) = key_effect_code {
            total_query.push(" AND auth_token_logs.key_effect_code = ");
            total_query.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = binding_effect_code {
            total_query.push(" AND auth_token_logs.binding_effect_code = ");
            total_query.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = selection_effect_code {
            total_query.push(" AND auth_token_logs.selection_effect_code = ");
            total_query.push_bind(selection_effect_code);
        }
        if let Some(key_id) = key_id {
            total_query.push(" AND auth_token_logs.api_key_id = ");
            total_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            total_query.push(" AND ");
            Self::push_request_kind_filter_clause_pg(
                &mut total_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            total_query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                &mut total_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        let total: i64 = total_query
            .build_query_scalar()
            .fetch_one(self.pg()?)
            .await?;

        let mut rows_query = QueryBuilder::<sqlx::Postgres>::new(
            r#"
            SELECT auth_token_logs.id, auth_token_logs.api_key_id, auth_token_logs.method, auth_token_logs.path, auth_token_logs.query, auth_token_logs.http_status, auth_token_logs.mcp_status,
                   CASE
                       WHEN COALESCE(bl.billing_state, auth_token_logs.billing_state) = 'charged'
                       THEN COALESCE(bl.business_credits, auth_token_logs.business_credits)
                       ELSE NULL
                   END AS business_credits,
                   auth_token_logs.request_kind_key,
                   auth_token_logs.request_kind_label,
                   auth_token_logs.request_kind_detail,
                   auth_token_logs.counts_business_quota,
                   auth_token_logs.result_status, auth_token_logs.error_message, auth_token_logs.failure_kind, auth_token_logs.key_effect_code,
                   auth_token_logs.key_effect_summary, auth_token_logs.binding_effect_code, auth_token_logs.binding_effect_summary,
                   auth_token_logs.selection_effect_code, auth_token_logs.selection_effect_summary,
                   auth_token_logs.gateway_mode, auth_token_logs.experiment_variant, auth_token_logs.proxy_session_id, auth_token_logs.routing_subject_hash,
                   auth_token_logs.upstream_operation, auth_token_logs.fallback_reason, auth_token_logs.created_at
            FROM core.auth_token_logs
            LEFT JOIN core.billing_ledger bl ON bl.auth_token_log_id = auth_token_logs.id
            WHERE auth_token_logs.token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND auth_token_logs.created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND auth_token_logs.created_at < ");
            rows_query.push_bind(until);
        }
        if let Some(result_status) = result_status {
            rows_query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                &mut rows_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(key_effect_code) = key_effect_code {
            rows_query.push(" AND auth_token_logs.key_effect_code = ");
            rows_query.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = binding_effect_code {
            rows_query.push(" AND auth_token_logs.binding_effect_code = ");
            rows_query.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = selection_effect_code {
            rows_query.push(" AND auth_token_logs.selection_effect_code = ");
            rows_query.push_bind(selection_effect_code);
        }
        if let Some(key_id) = key_id {
            rows_query.push(" AND auth_token_logs.api_key_id = ");
            rows_query.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            rows_query.push(" AND ");
            Self::push_request_kind_filter_clause_pg(
                &mut rows_query,
                stored_request_kind_sql,
                &legacy_request_kind_predicate_sql,
                &legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(operational_class) = operational_class {
            rows_query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                &mut rows_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        rows_query
            .push(" ORDER BY auth_token_logs.created_at DESC, auth_token_logs.id DESC LIMIT ");
        rows_query.push_bind(per_page);
        rows_query.push(" OFFSET ");
        rows_query.push_bind(offset);
        let rows = rows_query.build().fetch_all(self.pg()?).await?;

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

    async fn fetch_token_log_facet_options_pg(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        column_expr: &str,
        require_non_empty: bool,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let column_expr = match column_expr {
            "api_key_id" => "auth_token_logs.api_key_id",
            "key_effect_code" => "auth_token_logs.key_effect_code",
            "binding_effect_code" => "auth_token_logs.binding_effect_code",
            "selection_effect_code" => "auth_token_logs.selection_effect_code",
            _ => column_expr,
        };
        let stored_request_kind_sql = "auth_token_logs.request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql(
            "auth_token_logs.path",
            "auth_token_logs.request_kind_key",
        );
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let stored_result_bucket_sql = result_bucket_case_sql(
            &stored_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let legacy_result_bucket_sql = result_bucket_case_sql(
            &legacy_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let mut query = QueryBuilder::<sqlx::Postgres>::new(format!(
            "SELECT {column_expr} AS value, COUNT(*) AS count FROM core.auth_token_logs"
        ));
        Self::push_token_logs_catalog_filters_pg(
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

        let rows = query.build().fetch_all(self.pg()?).await?;
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

    async fn fetch_token_log_result_facet_options_pg(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let stored_request_kind_sql = "auth_token_logs.request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql = token_log_request_kind_key_sql(
            "auth_token_logs.path",
            "auth_token_logs.request_kind_key",
        );
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let stored_result_bucket_sql = result_bucket_case_sql(
            &stored_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let legacy_result_bucket_sql = result_bucket_case_sql(
            &legacy_operational_class_case_sql,
            "auth_token_logs.result_status",
        );

        let mut query = QueryBuilder::<sqlx::Postgres>::new(format!(
            "
            SELECT
                CASE
                    WHEN {legacy_request_kind_predicate_sql} THEN {legacy_result_bucket_sql}
                    ELSE {stored_result_bucket_sql}
                END AS value,
                COUNT(*) AS count
            FROM core.auth_token_logs
            "
        ));
        Self::push_token_logs_catalog_filters_pg(
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

        let rows = query.build().fetch_all(self.pg()?).await?;
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

    pub async fn fetch_token_log_request_kind_options_pg(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
        filters: TokenLogsCatalogFilters<'_>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        type RequestKindOptionRow = (String, String, i64, i64, i64);
        let stored_request_kind_sql = "auth_token_logs.request_kind_key";
        let canonical_request_kind_predicate_sql =
            canonical_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let stored_label_sql = canonical_request_kind_label_sql(stored_request_kind_sql);
        let mut stored_query = QueryBuilder::<sqlx::Postgres>::new(format!(
            "
            SELECT
                {stored_request_kind_sql} AS request_kind_key,
                {stored_label_sql} AS request_kind_label,
                COUNT(*) AS request_count,
                MAX(CASE WHEN auth_token_logs.counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                MAX(CASE WHEN auth_token_logs.counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
            FROM core.auth_token_logs
            "
        ));
        let stored_operational_class_case_sql = token_log_operational_class_case_sql(
            stored_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let legacy_request_kind_sql = token_log_request_kind_key_sql(
            "auth_token_logs.path",
            "auth_token_logs.request_kind_key",
        );
        let legacy_operational_class_case_sql = token_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            "auth_token_logs.counts_business_quota",
            "auth_token_logs.result_status",
            "COALESCE(auth_token_logs.failure_kind, '')",
        );
        let stored_result_bucket_sql = result_bucket_case_sql(
            &stored_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        let legacy_result_bucket_sql = result_bucket_case_sql(
            &legacy_operational_class_case_sql,
            "auth_token_logs.result_status",
        );
        Self::push_token_logs_catalog_filters_pg(
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
            .fetch_all(self.pg()?)
            .await?;
        let legacy_label_sql = canonical_request_kind_label_sql(&legacy_request_kind_sql);
        let mut legacy_query = QueryBuilder::<sqlx::Postgres>::new(format!(
            "
            SELECT
                {legacy_request_kind_sql} AS request_kind_key,
                {legacy_label_sql} AS request_kind_label,
                COUNT(*) AS request_count,
                MAX(CASE WHEN auth_token_logs.counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                MAX(CASE WHEN auth_token_logs.counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
            FROM core.auth_token_logs
            "
        ));
        Self::push_token_logs_catalog_filters_pg(
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
            .fetch_all(self.pg()?)
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

    pub async fn fetch_token_hourly_breakdown_pg(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        let hours = hours.clamp(1, 168); // up to 7 days
        let now_ts = self.backend_time.now_ts();
        let current_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let window_start = current_bucket - (hours - 1) * SECS_PER_HOUR;
        let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count
            FROM core.token_usage_stats
            WHERE token_id = $1 AND bucket_secs = $2 AND bucket_start >= $3
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(window_start)
        .fetch_all(self.pg()?)
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

    pub async fn fetch_token_usage_series_pg(
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
                FROM core.token_usage_stats
                WHERE token_id = $1 AND bucket_secs = $2 AND bucket_start >= $3 AND bucket_start < $4
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(self.pg()?)
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
                FROM core.token_usage_stats
                WHERE token_id = $1 AND bucket_secs = $2 AND bucket_start >= $3 AND bucket_start < $4
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(self.pg()?)
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

    pub(crate) async fn reset_monthly_pg(&self) -> Result<(), ProxyError> {
        let now = self.backend_time.now_utc();
        let month_start = start_of_month(now).timestamp();

        let now_ts = now.timestamp();

        sqlx::query(
            r#"
            UPDATE core.api_keys
            SET status = $1, status_changed_at = $2
            WHERE status = $3
              AND status_changed_at IS NOT NULL
              AND status_changed_at < $4
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now_ts)
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .execute(self.pg()?)
        .await?;

        Ok(())
    }

    pub(crate) async fn mark_quota_exhausted_pg(&self, key: &str) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let res = sqlx::query(
            r#"
            UPDATE core.api_keys
            SET status = $1, status_changed_at = $2, last_used_at = $3
            WHERE api_key = $4 AND status NOT IN ($5, $6) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(now)
        .bind(now)
        .bind(key)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(self.pg()?)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn restore_active_status_pg(&self, key: &str) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let res = sqlx::query(
            r#"
            UPDATE core.api_keys
            SET status = $1, status_changed_at = $2
            WHERE api_key = $3 AND status = $4 AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key)
        .bind(STATUS_EXHAUSTED)
        .execute(self.pg()?)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn quarantine_key_by_id_pg(
        &self,
        key_id: &str,
        source: &str,
        reason_code: &str,
        reason_summary: &str,
        reason_detail: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let quarantine_id = nanoid!(12);
        let insert_result = sqlx::query(
            r#"
            INSERT INTO core.api_key_quarantines (
                id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)
            ON CONFLICT (key_id) WHERE cleared_at IS NULL DO NOTHING
            "#,
        )
        .bind(quarantine_id)
        .bind(key_id)
        .bind(source)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(reason_detail)
        .bind(now)
        .execute(self.pg()?)
        .await?;

        if insert_result.rows_affected() == 0 {
            sqlx::query(
                r#"
                UPDATE core.api_key_quarantines
                SET source = $1, reason_code = $2, reason_summary = $3, reason_detail = $4
                WHERE key_id = $5 AND cleared_at IS NULL
                "#,
            )
            .bind(source)
            .bind(reason_code)
            .bind(reason_summary)
            .bind(reason_detail)
            .bind(key_id)
            .execute(self.pg()?)
            .await?;
            return Ok(false);
        }

        Ok(true)
    }

    pub(crate) async fn clear_key_quarantine_by_id_pg(
        &self,
        key_id: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let res = sqlx::query(
            r#"
            UPDATE core.api_key_quarantines
            SET cleared_at = $1
            WHERE key_id = $2 AND cleared_at IS NULL
            "#,
        )
        .bind(now)
        .bind(key_id)
        .execute(self.pg()?)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn fetch_active_existing_api_keys_pg(
        &self,
        api_keys: &[String],
    ) -> Result<HashSet<String>, ProxyError> {
        if api_keys.is_empty() {
            return Ok(HashSet::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT api_key FROM core.api_keys WHERE deleted_at IS NULL AND api_key IN (",
        );
        let mut separated = builder.separated(", ");
        for api_key in api_keys {
            separated.push_bind(api_key);
        }
        separated.push_unseparated(")");

        let rows = builder
            .build_query_scalar::<String>()
            .fetch_all(self.pg()?)
            .await?;
        Ok(rows.into_iter().collect())
    }

    async fn add_or_undelete_key_with_status_in_group_once_pg(
        &self,
        api_key: &str,
        input: ApiKeyUpsertInput<'_>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let mut tx = self.pg()?.begin().await?;
        let now = self.backend_time.now_ts();

        let operation_result: Result<(String, ApiKeyUpsertStatus), ProxyError> = async {
            if let Some((id, deleted_at, existing_group, existing_registration_ip, existing_registration_region)) =
                sqlx::query_as::<_, (String, Option<i64>, Option<String>, Option<String>, Option<String>)>(
                    "SELECT id, deleted_at, group_name, registration_ip, registration_region FROM core.api_keys WHERE api_key = $1 LIMIT 1",
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
                    input.registration_ip.is_some() || input.registration_region.is_some();
                let should_persist_proxy_affinity =
                    !input.hint_only_proxy_affinity || !existing_has_registration_metadata;

                let mut assignments = Vec::new();
                if deleted_at.is_some() {
                    assignments.push("deleted_at = NULL".to_string());
                }
                if input.group.is_some() && existing_empty {
                    assignments.push("group_name = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_ip = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_region = ?".to_string());
                }

                if !assignments.is_empty() {
                    let mut query = String::from("UPDATE core.api_keys SET ");
                    query.push_str(&assignments.join(", "));
                    query.push_str(" WHERE id = ?");
                    // `assignments` grows at runtime, so the positional parameters are numbered
                    // as the statement is assembled instead of being hard-coded per branch.
                    let mut numbered = String::with_capacity(query.len() + 8);
                    let mut param_index = 0usize;
                    let mut rest = query.as_str();
                    while let Some(position) = rest.find('?') {
                        numbered.push_str(&rest[..position]);
                        param_index += 1;
                        numbered.push('$');
                        numbered.push_str(&param_index.to_string());
                        rest = &rest[position + 1..];
                    }
                    numbered.push_str(rest);
                    let mut sql = sqlx::query(&numbered);
                    if let Some(group) = input.group
                        && existing_empty
                    {
                        sql = sql.bind(group);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(input.registration_ip);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(input.registration_region);
                    }
                    sql.bind(&id).execute(&mut *tx).await?;
                }
                if deleted_at.is_some() {
                    sqlx::query(
                        "INSERT INTO core.api_key_membership_intervals (key_id, active_from) VALUES ($1, $2)",
                    )
                    .bind(&id)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
                }
                if should_persist_proxy_affinity
                    && let Some(proxy_affinity) = input.proxy_affinity
                {
                    sqlx::query(
                        r#"
                        INSERT INTO core.forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                        VALUES ($1, $2, $3, EXTRACT(EPOCH FROM now())::bigint)
                        ON CONFLICT (key_id) DO UPDATE SET
                            primary_proxy_key = excluded.primary_proxy_key,
                            secondary_proxy_key = excluded.secondary_proxy_key,
                            updated_at = EXTRACT(EPOCH FROM now())::bigint
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

            // The SQLite helper takes a SQLite connection; the PostgreSQL twin probes
            // core.api_keys through the same transaction.
            let id = Self::generate_unique_key_id_pg(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO core.api_keys (
                    id,
                    api_key,
                    group_name,
                    registration_ip,
                    registration_region,
                    status,
                    created_at,
                    status_changed_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
            )
            .bind(&id)
            .bind(api_key)
            .bind(input.group)
            .bind(input.registration_ip)
            .bind(input.registration_region)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO core.api_key_membership_intervals (key_id, active_from) VALUES ($1, $2)",
            )
            .bind(&id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if let Some(proxy_affinity) = input.proxy_affinity {
                sqlx::query(
                    r#"
                    INSERT INTO core.forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                    VALUES ($1, $2, $3, EXTRACT(EPOCH FROM now())::bigint)
                    ON CONFLICT (key_id) DO UPDATE SET
                        primary_proxy_key = excluded.primary_proxy_key,
                        secondary_proxy_key = excluded.secondary_proxy_key,
                        updated_at = EXTRACT(EPOCH FROM now())::bigint
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

    pub(crate) async fn soft_delete_key_by_id_pg(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self.pg()?.begin().await?;
        sqlx::query("UPDATE core.api_keys SET deleted_at = $1 WHERE id = $2")
            .bind(now)
            .bind(key_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE core.api_key_membership_intervals SET active_until = $1 WHERE key_id = $2 AND active_until IS NULL",
        )
        .bind(now)
        .bind(key_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn disable_key_by_id_pg(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            UPDATE core.api_keys
            SET status = $1, status_changed_at = $2
            WHERE id = $3 AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(key_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn enable_key_by_id_pg(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            UPDATE core.api_keys
            SET status = $1, status_changed_at = $2
            WHERE id = $3 AND status IN ($4, $5) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key_id)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn touch_key_pg(&self, key: &str, timestamp: i64) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE core.api_keys
            SET last_used_at = $1
            WHERE api_key = $2
            "#,
        )
        .bind(timestamp)
        .bind(key)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn fetch_api_key_group_facets_pg(
        &self,
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT TRIM(COALESCE(ak.group_name, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause_pg());
        Self::push_api_key_status_filters_pg(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter_pg(&mut builder, registration_ip);
        Self::push_api_key_region_filters_pg(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(self.pg()?).await?;
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

    pub(crate) async fn fetch_api_key_status_facets_pg(
        &self,
        groups: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            r#"
            SELECT CASE
                WHEN aq.key_id IS NOT NULL THEN 'quarantined'
                WHEN ak.status = 'active' AND tb.key_id IS NOT NULL THEN 'temporary_isolated'
                ELSE ak.status
            END AS value, COUNT(*) AS count
            "#,
        );
        builder.push(Self::api_key_metrics_from_clause_pg());
        Self::push_api_key_group_filters_pg(&mut builder, groups);
        Self::push_api_key_registration_ip_filter_pg(&mut builder, registration_ip);
        Self::push_api_key_region_filters_pg(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(self.pg()?).await?;
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

    pub(crate) async fn fetch_api_key_region_facets_pg(
        &self,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT TRIM(COALESCE(ak.registration_region, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause_pg());
        Self::push_api_key_group_filters_pg(&mut builder, groups);
        Self::push_api_key_status_filters_pg(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter_pg(&mut builder, registration_ip);
        builder.push(" AND TRIM(COALESCE(ak.registration_region, '')) != ''");
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(self.pg()?).await?;
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

    pub(crate) async fn fetch_api_key_metrics_pg(
        &self,
        include_quarantine_detail: bool,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let query = format!(
            "{} ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
            Self::api_key_metrics_query_pg(include_quarantine_detail),
        );
        let rows = sqlx::query(&query).fetch_all(self.pg()?).await?;
        rows.into_iter()
            .map(Self::map_api_key_metrics_row_pg)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_dashboard_exhausted_api_key_metrics_pg(
        &self,
        limit: usize,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let limit = limit.clamp(1, 50) as i64;
        let statuses = vec![STATUS_EXHAUSTED.to_string()];
        let mut items_builder =
            QueryBuilder::<sqlx::Postgres>::new(Self::api_key_metrics_query_pg(false));
        Self::push_api_key_status_filters_pg(&mut items_builder, &statuses);
        items_builder.push(
            " ORDER BY COALESCE(ak.last_used_at, 0) DESC, COALESCE(ak.status_changed_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(limit);
        items_builder
            .build()
            .fetch_all(self.pg()?)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row_pg)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_dashboard_exhausted_api_key_ids_pg(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        let limit = limit.clamp(1, 50) as i64;
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT ak.id
            FROM core.api_keys ak
            LEFT JOIN core.api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
              AND ak.status = $1
            ORDER BY COALESCE(ak.last_used_at, 0) DESC, COALESCE(ak.status_changed_at, 0) DESC, ak.id ASC
            LIMIT $2
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(limit)
        .fetch_all(self.pg()?)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_metrics_page_pg(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let requested_page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let groups = Self::normalize_api_key_groups(groups);
        let statuses = Self::normalize_api_key_statuses(statuses);
        let registration_ip = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let regions = Self::normalize_api_key_regions(regions);

        let mut count_builder = QueryBuilder::<sqlx::Postgres>::new("SELECT COUNT(*)");
        count_builder.push(Self::api_key_metrics_from_clause_pg());
        Self::push_api_key_group_filters_pg(&mut count_builder, &groups);
        Self::push_api_key_status_filters_pg(&mut count_builder, &statuses);
        Self::push_api_key_registration_ip_filter_pg(&mut count_builder, registration_ip);
        Self::push_api_key_region_filters_pg(&mut count_builder, &regions);
        let total = count_builder
            .build_query_scalar::<i64>()
            .fetch_one(self.pg()?)
            .await?;
        let total_pages = ((total + per_page - 1) / per_page).max(1);
        let page = requested_page.min(total_pages);
        let offset = (page - 1) * per_page;

        let mut items_builder =
            QueryBuilder::<sqlx::Postgres>::new(Self::api_key_metrics_query_pg(false));
        Self::push_api_key_group_filters_pg(&mut items_builder, &groups);
        Self::push_api_key_status_filters_pg(&mut items_builder, &statuses);
        Self::push_api_key_registration_ip_filter_pg(&mut items_builder, registration_ip);
        Self::push_api_key_region_filters_pg(&mut items_builder, &regions);
        items_builder.push(
            " ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(per_page);
        items_builder.push(" OFFSET ").push_bind(offset);
        let items = items_builder
            .build()
            .fetch_all(self.pg()?)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row_pg)
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

    pub(crate) async fn fetch_api_key_metric_by_id_pg(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        let mut builder = QueryBuilder::<sqlx::Postgres>::new(Self::api_key_metrics_query_pg(true));
        builder.push(" AND ak.id = ");
        builder.push_bind(key_id);
        builder.push(" LIMIT 1");

        let row = builder.build().fetch_optional(self.pg()?).await?;

        row.map(Self::map_api_key_metrics_row_pg)
            .transpose()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_logs_pg(
        &self,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;

        let rows = if let Some(since) = since {
            sqlx::query(
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
                counts_business_quota,
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
                request_body_bytes,
                response_body_bytes,
                request_body_sha256,
                response_body_sha256,
                body_cleaned_reason,
                body_cleaned_at,
                forwarded_headers,
                dropped_headers,
                remote_addr,
                client_ip,
                client_ip_source,
                client_ip_trusted,
                ip_headers,
                created_at
            FROM observability.request_logs
            WHERE visibility = $1 AND created_at >= $2
            ORDER BY created_at DESC, id DESC
            LIMIT $3
            "#,
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(since)
            .bind(limit)
            .fetch_all(self.pg()?)
            .await?
        } else {
            sqlx::query(
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
                counts_business_quota,
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
                request_body_bytes,
                response_body_bytes,
                request_body_sha256,
                response_body_sha256,
                body_cleaned_reason,
                body_cleaned_at,
                forwarded_headers,
                dropped_headers,
                remote_addr,
                client_ip,
                client_ip_source,
                client_ip_trusted,
                ip_headers,
                created_at
            FROM observability.request_logs
            WHERE visibility = $1
            ORDER BY created_at DESC, id DESC
            LIMIT $2
            "#,
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(limit)
            .fetch_all(self.pg()?)
            .await?
        };

        let records = rows
            .into_iter()
            .map(Self::map_request_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub(crate) async fn fetch_latest_visible_request_log_id_pg(
        &self,
        since: Option<i64>,
    ) -> Result<Option<i64>, ProxyError> {
        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id FROM observability.request_logs WHERE visibility = ",
        );
        builder.push_bind(REQUEST_LOG_VISIBILITY_VISIBLE);
        if let Some(since) = since {
            builder.push(" AND created_at >= ").push_bind(since);
        }
        builder.push(" ORDER BY created_at DESC, id DESC LIMIT 1");

        builder
            .build_query_scalar::<Option<i64>>()
            .fetch_optional(self.pg()?)
            .await
            .map(|value| value.flatten())
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_visible_request_log_signature_pg(
        &self,
        limit: usize,
        since: i64,
    ) -> Result<Vec<(i64, i64)>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT id, created_at
            FROM observability.request_logs
            WHERE visibility = $1 AND created_at >= $2
            ORDER BY created_at DESC, id DESC
            LIMIT $3
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(since)
        .bind(limit)
        .fetch_all(self.pg()?)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_client_ip_counts_by_user_pg(
        &self,
        user_ids: &[String],
        since: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT request_user_id, COUNT(DISTINCT client_ip) AS ip_count FROM observability.request_logs WHERE visibility = ",
        );
        builder.push_bind(REQUEST_LOG_VISIBILITY_VISIBLE);
        builder.push(" AND request_user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
            separated.push_unseparated(")");
        }
        builder.push(" AND created_at >= ");
        builder.push_bind(since);
        builder
            .push(" AND client_ip IS NOT NULL AND TRIM(client_ip) != '' GROUP BY request_user_id");

        let rows = builder.build().fetch_all(self.pg()?).await?;
        let mut result = HashMap::new();
        for row in rows {
            let user_id: String = row.try_get("request_user_id")?;
            let ip_count: i64 = row.try_get("ip_count")?;
            result.insert(user_id, ip_count);
        }
        Ok(result)
    }

    pub(crate) async fn fetch_recent_client_ip_addresses_for_user_pg(
        &self,
        user_id: &str,
        since: i64,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        let row_limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            "SELECT client_ip, MAX(created_at) AS latest_seen_at FROM observability.request_logs \
             WHERE request_user_id = $1 AND created_at >= $2 AND visibility = $3 \
             AND client_ip IS NOT NULL AND TRIM(client_ip) != '' \
             GROUP BY client_ip ORDER BY latest_seen_at DESC, client_ip ASC LIMIT $4",
        )
        .bind(user_id)
        .bind(since)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(row_limit)
        .fetch_all(self.pg()?)
        .await?;

        rows.into_iter()
            .map(|row| row.try_get("client_ip").map_err(ProxyError::from))
            .collect()
    }

    pub(crate) async fn fetch_recent_client_ip_timeline_for_user_pg(
        &self,
        user_id: &str,
        since: i64,
        limit: usize,
    ) -> Result<Vec<AdminUserIpTimelineEntry>, ProxyError> {
        let row_limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            "SELECT client_ip, MIN(created_at) AS first_seen_at, MAX(created_at) AS last_seen_at, \
             COUNT(*) AS request_count FROM observability.request_logs \
             WHERE request_user_id = $1 AND created_at >= $2 AND visibility = $3 \
             AND client_ip IS NOT NULL AND TRIM(client_ip) != '' \
             GROUP BY client_ip ORDER BY last_seen_at DESC, client_ip ASC LIMIT $4",
        )
        .bind(user_id)
        .bind(since)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(row_limit)
        .fetch_all(self.pg()?)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(AdminUserIpTimelineEntry {
                    ip_address: row.try_get("client_ip")?,
                    first_seen_at: row.try_get("first_seen_at")?,
                    last_seen_at: row.try_get("last_seen_at")?,
                    request_count: row.try_get("request_count")?,
                })
            })
            .collect()
    }

    pub(crate) async fn fetch_recent_client_ip_requests_pg(
        &self,
        limit: usize,
    ) -> Result<Vec<ObservedClientIpRequest>, ProxyError> {
        let row_limit = limit.clamp(1, 100) as i64;
        let rows = sqlx::query(
            r#"
            SELECT id, created_at, remote_addr, client_ip, client_ip_source, client_ip_trusted, ip_headers
            FROM observability.request_logs
            WHERE visibility = $1
              AND NOT (
                gateway_mode IS NOT NULL
                AND upstream_operation IS NOT NULL
                AND gateway_mode = $2
                AND upstream_operation = $3
                AND remote_addr IS NULL
                AND client_ip IS NULL
                AND (
                  ip_headers IS NULL
                  OR TRIM(ip_headers) = ''
                  OR TRIM(ip_headers) = '[]'
                )
              )
            ORDER BY created_at DESC, id DESC
            LIMIT $4
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(MCP_GATEWAY_MODE_REBALANCE)
        .bind("mcp")
        .bind(row_limit)
        .fetch_all(self.pg()?)
        .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(ObservedClientIpRequest {
                id: row.try_get("id")?,
                created_at: row.try_get("created_at")?,
                remote_addr: row.try_get("remote_addr")?,
                client_ip: row.try_get("client_ip")?,
                client_ip_source: row.try_get("client_ip_source")?,
                client_ip_trusted: row.try_get::<i64, _>("client_ip_trusted")? != 0,
                ip_headers: parse_client_ip_header_values(
                    row.try_get::<Option<String>, _>("ip_headers")?,
                ),
            });
        }
        Ok(items)
    }

    pub(crate) async fn fetch_request_log_bodies_pg(
        &self,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT request_body, response_body,
                   request_body_bytes, response_body_bytes,
                   request_body_sha256, response_body_sha256,
                   body_cleaned_reason, body_cleaned_at
            FROM observability.request_logs
            WHERE id = $1 AND visibility = $2
            LIMIT 1
            "#,
        )
        .bind(log_id)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_optional(self.pg()?)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_request_log_bodies_pg(
        &self,
        key_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT request_body, response_body,
                   request_body_bytes, response_body_bytes,
                   request_body_sha256, response_body_sha256,
                   body_cleaned_reason, body_cleaned_at
            FROM observability.request_logs
            WHERE id = $1 AND api_key_id = $2 AND visibility = $3
            LIMIT 1
            "#,
        )
        .bind(log_id)
        .bind(key_id)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_optional(self.pg()?)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_token_log_bodies_pg(
        &self,
        token_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        sqlx::query(
            r#"
            SELECT rl.request_body, rl.response_body,
                   rl.request_body_bytes, rl.response_body_bytes,
                   rl.request_body_sha256, rl.response_body_sha256,
                   rl.body_cleaned_reason, rl.body_cleaned_at
            FROM core.auth_token_logs atl
            LEFT JOIN observability.request_logs rl
              ON rl.id = atl.request_log_id
             AND rl.visibility = $1
            WHERE atl.id = $2 AND atl.token_id = $3
            LIMIT 1
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(log_id)
        .bind(token_id)
        .fetch_optional(self.pg()?)
        .await?
        .map(Self::map_request_log_bodies_row)
        .transpose()
        .map_err(ProxyError::from)
    }

    fn push_request_kind_filter_clause_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        stored_request_kind_sql: &str,
        legacy_request_kind_predicate_sql: &str,
        legacy_request_kind_sql: &str,
        request_kinds: &[String],
    ) {
        push_request_kind_filter_clause_body!(
            builder,
            stored_request_kind_sql,
            legacy_request_kind_predicate_sql,
            legacy_request_kind_sql,
            request_kinds
        );
    }

    fn push_operational_class_filter_clause_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        operational_class: &'a str,
        legacy_request_kind_predicate_sql: &str,
        stored_operational_class_sql: &str,
        legacy_operational_class_sql: &str,
    ) {
        push_class_filter_clause_body!(
            builder,
            operational_class,
            legacy_request_kind_predicate_sql,
            stored_operational_class_sql,
            legacy_operational_class_sql
        );
    }

    fn push_result_bucket_filter_clause_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        result_bucket: &'a str,
        legacy_request_kind_predicate_sql: &str,
        stored_result_bucket_sql: &str,
        legacy_result_bucket_sql: &str,
    ) {
        push_class_filter_clause_body!(
            builder,
            result_bucket,
            legacy_request_kind_predicate_sql,
            stored_result_bucket_sql,
            legacy_result_bucket_sql
        );
    }

    /// PostgreSQL twin of `KeyStore::api_key_metrics_from_clause`; the SQLite original keeps
    /// serving the SQLite path while this slice reads PostgreSQL.
    fn api_key_metrics_from_clause_pg() -> &'static str {
        r#"
            FROM core.api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0)::bigint AS total_requests,
                    COALESCE(SUM(success_count), 0)::bigint AS success_count,
                    COALESCE(SUM(error_count), 0)::bigint AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0)::bigint AS quota_exhausted_count
                FROM observability.api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN core.api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            LEFT JOIN (
                SELECT
                    key_id,
                    MAX(cooldown_until) AS transient_backoff_cooldown_until,
                    MAX(retry_after_secs) AS transient_backoff_retry_after_secs,
                    STRING_AGG(scope, ',') AS transient_backoff_scopes
                FROM core.api_key_transient_backoffs
                WHERE cooldown_until > EXTRACT(EPOCH FROM now())::bigint
                  AND reason_code = 'upstream_unknown_403'
                GROUP BY key_id
            ) AS tb
            ON tb.key_id = ak.id
            WHERE ak.deleted_at IS NULL
        "#
    }

    fn api_key_metrics_query_pg(include_quarantine_detail: bool) -> String {
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
                tb.transient_backoff_cooldown_until,
                tb.transient_backoff_retry_after_secs,
                tb.transient_backoff_scopes,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            {}
            "#,
            Self::api_key_metrics_from_clause_pg(),
        )
    }

    fn map_api_key_metrics_row_pg(
        row: sqlx::postgres::PgRow,
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
        let transient_backoff_cooldown_until: Option<i64> =
            row.try_get("transient_backoff_cooldown_until")?;
        let transient_backoff_retry_after_secs: Option<i64> =
            row.try_get("transient_backoff_retry_after_secs")?;
        let transient_backoff_scopes: Option<String> = row.try_get("transient_backoff_scopes")?;
        let is_temporary_isolated = status == STATUS_ACTIVE
            && quarantine_source.is_none()
            && transient_backoff_cooldown_until.is_some();

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
            transient_backoff: is_temporary_isolated.then(|| ApiKeyTransientBackoff {
                reason_code: FAILURE_KIND_UPSTREAM_UNKNOWN_403.to_string(),
                cooldown_until: transient_backoff_cooldown_until.unwrap_or_default(),
                retry_after_secs: transient_backoff_retry_after_secs.unwrap_or_default(),
                scopes: transient_backoff_scopes
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|scope| !scope.is_empty())
                    .map(str::to_string)
                    .collect(),
            }),
        })
    }

    fn push_api_key_group_filters_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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
                    .push_bind(group.as_str())
                    .push(")");
            }
        }
        builder.push(")");
    }

    fn push_api_key_status_filters_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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
            } else if status == "temporary_isolated" {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = 'active' AND tb.key_id IS NOT NULL)");
            } else if status == STATUS_ACTIVE {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status.as_str())
                    .push(" AND tb.key_id IS NULL)");
            } else {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status.as_str())
                    .push(")");
            }
        }
        builder.push(")");
    }

    fn push_api_key_registration_ip_filter_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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

    fn push_api_key_region_filters_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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
                .push_bind(region.as_str())
                .push(")");
        }
        builder.push(")");
    }

    #[allow(clippy::too_many_arguments)]
    fn push_token_logs_catalog_filters_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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
        builder.push(" WHERE auth_token_logs.token_id = ");
        builder.push_bind(token_id);
        builder.push(" AND auth_token_logs.created_at >= ");
        builder.push_bind(since);
        if let Some(until) = until {
            builder.push(" AND auth_token_logs.created_at < ");
            builder.push_bind(until);
        }
        if let Some(key_effect_code) = filters.key_effect_code {
            builder.push(" AND auth_token_logs.key_effect_code = ");
            builder.push_bind(key_effect_code);
        }
        if let Some(binding_effect_code) = filters.binding_effect_code {
            builder.push(" AND auth_token_logs.binding_effect_code = ");
            builder.push_bind(binding_effect_code);
        }
        if let Some(selection_effect_code) = filters.selection_effect_code {
            builder.push(" AND auth_token_logs.selection_effect_code = ");
            builder.push_bind(selection_effect_code);
        }
        if let Some(key_id) = filters.key_id {
            builder.push(" AND auth_token_logs.api_key_id = ");
            builder.push_bind(key_id);
        }
        if !normalized_request_kinds.is_empty() {
            builder.push(" AND ");
            Self::push_request_kind_filter_clause_pg(
                builder,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql,
                legacy_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = filters.result_status {
            builder.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                builder,
                result_status,
                legacy_request_kind_predicate_sql,
                stored_result_bucket_sql,
                legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = filters.operational_class {
            builder.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                builder,
                operational_class,
                legacy_request_kind_predicate_sql,
                stored_operational_class_case_sql,
                legacy_operational_class_case_sql,
            );
        }
    }

    fn push_desc_cursor_clause_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        created_at_column: &str,
        id_column: &str,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        has_where: bool,
    ) -> bool {
        push_desc_cursor_clause_body!(
            builder,
            created_at_column,
            id_column,
            cursor,
            direction,
            has_where
        )
    }
}
