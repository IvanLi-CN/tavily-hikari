/// PostgreSQL twins of the request-log read-model helpers.
///
/// The SQLite implementations stay in `key_store_request_logs_and_dashboard.rs` and keep
/// serving the request path until the cutover wave; these variants are reachable only
/// through the attached PostgreSQL pool.
impl KeyStore {
    fn push_request_logs_scope_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        scoped_key_id: Option<&'a str>,
        since: Option<i64>,
        until: Option<i64>,
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
        if let Some(until) = until {
            builder.push(if has_where {
                " AND created_at < "
            } else {
                " WHERE created_at < "
            });
            builder.push_bind(until);
            has_where = true;
        }
        has_where
    }

    fn push_request_logs_filters_pg<'a, 'b>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        filters: RequestLogFilterParams<'a, 'b>,
    ) {
        let RequestLogFilterParams {
            request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            request_user_id,
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
        if let Some(request_user_id) = request_user_id {
            builder.push(if has_where {
                " AND request_user_id = "
            } else {
                " WHERE request_user_id = "
            });
            builder.push_bind(request_user_id.to_string());
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
            Self::push_request_kind_filter_clause_pg(
                builder,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql,
                legacy_request_kind_sql,
                request_kinds,
            );
        }
    }

    fn push_effective_request_kind_filter_clause_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        effective_request_kind_sql: &str,
        request_kinds: &[String],
    ) {
        builder.push("(");
        builder.push(effective_request_kind_sql.to_string());
        builder.push(" IN (");
        {
            let mut separated = builder.separated(", ");
            for request_kind in request_kinds {
                separated.push_bind(request_kind.clone());
            }
            separated.push_unseparated(")");
        }
        builder.push(")");
    }

    fn push_request_log_catalog_rollup_filters_pg<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
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

    async fn fetch_dashboard_rollup_success_count_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        bucket_secs: i64,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        if bucket_start_at_least >= bucket_start_before {
            return Ok(0);
        }

        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(success_count), 0)::bigint
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_secs = $1
              AND bucket_start >= $2
              AND bucket_start < $3
            "#,
        )
        .bind(bucket_secs)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&mut **tx)
        .await
        .map_err(ProxyError::Database)
    }

    async fn fetch_dashboard_rollup_success_count_for_range_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
            success_count += Self::fetch_dashboard_rollup_success_count_tx_pg(
                tx,
                SECS_PER_MINUTE,
                cursor,
                leading_minute_end,
            )
            .await?;
            cursor = leading_minute_end;
        }

        if cursor < full_day_end {
            success_count += Self::fetch_dashboard_rollup_success_count_tx_pg(
                tx,
                SECS_PER_DAY,
                cursor,
                full_day_end,
            )
            .await?;
            cursor = full_day_end;
        }

        if cursor < end {
            success_count +=
                Self::fetch_dashboard_rollup_success_count_tx_pg(tx, SECS_PER_MINUTE, cursor, end)
                    .await?;
        }

        Ok(success_count)
    }

    async fn fetch_visible_request_log_success_count_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        start: i64,
        end: i64,
    ) -> Result<i64, ProxyError> {
        if start >= end {
            return Ok(0);
        }

        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(CASE WHEN result_status = $1 THEN 1 ELSE 0 END), 0)::bigint
            FROM observability.request_logs
            WHERE visibility = $2
              AND created_at >= $3
              AND created_at < $4
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

    async fn fetch_dashboard_rollup_month_metrics_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
                &Self::fetch_dashboard_rollup_window_metrics_tx_pg(
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
                &Self::fetch_dashboard_rollup_window_metrics_tx_pg(
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
            &Self::fetch_dashboard_rollup_window_metrics_tx_pg(
                tx,
                SECS_PER_MINUTE,
                today_start,
                Some(today_end),
            )
            .await?,
        );

        Ok(month_metrics)
    }

    async fn upsert_request_log_catalog_rollup_delta_pg(
        tx: &mut sqlx::PgConnection,
        key: &RequestLogCatalogRollupKey,
        request_count_delta: i64,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        if request_count_delta == 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO observability.request_log_catalog_rollups (
                bucket_start,
                request_kind_key,
                request_kind_label,
                result_bucket,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                api_key_id,
                operational_class,
                request_count,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT(
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
            ) DO UPDATE SET
                request_count = request_log_catalog_rollups.request_count + excluded.request_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key.bucket_start)
        .bind(&key.request_kind_key)
        .bind(&key.request_kind_label)
        .bind(&key.result_bucket)
        .bind(&key.key_effect_code)
        .bind(&key.binding_effect_code)
        .bind(&key.selection_effect_code)
        .bind(&key.auth_token_id)
        .bind(&key.api_key_id)
        .bind(&key.operational_class)
        .bind(request_count_delta)
        .bind(updated_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM observability.request_log_catalog_rollups
            WHERE bucket_start = $1
              AND request_kind_key = $2
              AND request_kind_label = $3
              AND result_bucket = $4
              AND key_effect_code = $5
              AND binding_effect_code = $6
              AND selection_effect_code = $7
              AND auth_token_id = $8
              AND api_key_id = $9
              AND operational_class = $10
              AND request_count <= 0
            "#,
        )
        .bind(key.bucket_start)
        .bind(&key.request_kind_key)
        .bind(&key.request_kind_label)
        .bind(&key.result_bucket)
        .bind(&key.key_effect_code)
        .bind(&key.binding_effect_code)
        .bind(&key.selection_effect_code)
        .bind(&key.auth_token_id)
        .bind(&key.api_key_id)
        .bind(&key.operational_class)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
                DELETE FROM observability.request_log_catalog_rollups
                WHERE bucket_start = $1
                  AND request_kind_key = $2
                  AND request_kind_label = $3
                  AND result_bucket = $4
                  AND key_effect_code = $5
                  AND binding_effect_code = $6
                  AND selection_effect_code = $7
                  AND auth_token_id = $8
                  AND api_key_id = $9
                  AND operational_class = $10
                  AND request_count <= 0
                "#,
        )
        .bind(key.bucket_start)
        .bind(&key.request_kind_key)
        .bind(&key.request_kind_label)
        .bind(&key.result_bucket)
        .bind(&key.key_effect_code)
        .bind(&key.binding_effect_code)
        .bind(&key.selection_effect_code)
        .bind(&key.auth_token_id)
        .bind(&key.api_key_id)
        .bind(&key.operational_class)
        .execute(tx)
        .await?;
        Ok(())
    }

    async fn upsert_api_key_usage_bucket_delta_pg(
        tx: &mut sqlx::PgConnection,
        key_id: &str,
        bucket_start: i64,
        delta: ApiKeyUsageBucketDelta,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        if delta.is_zero() {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO observability.api_key_usage_buckets (
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
            ) VALUES ($1, $2, 86400, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT(api_key_id, bucket_start, bucket_secs)
            DO UPDATE SET
                total_requests = api_key_usage_buckets.total_requests + excluded.total_requests,
                success_count = api_key_usage_buckets.success_count + excluded.success_count,
                error_count = api_key_usage_buckets.error_count + excluded.error_count,
                quota_exhausted_count = api_key_usage_buckets.quota_exhausted_count + excluded.quota_exhausted_count,
                valuable_success_count = api_key_usage_buckets.valuable_success_count + excluded.valuable_success_count,
                valuable_failure_count = api_key_usage_buckets.valuable_failure_count + excluded.valuable_failure_count,
                other_success_count = api_key_usage_buckets.other_success_count + excluded.other_success_count,
                other_failure_count = api_key_usage_buckets.other_failure_count + excluded.other_failure_count,
                unknown_count = api_key_usage_buckets.unknown_count + excluded.unknown_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key_id)
        .bind(bucket_start)
        .bind(delta.total_requests)
        .bind(delta.success_count)
        .bind(delta.error_count)
        .bind(delta.quota_exhausted_count)
        .bind(delta.valuable_success_count)
        .bind(delta.valuable_failure_count)
        .bind(delta.other_success_count)
        .bind(delta.other_failure_count)
        .bind(delta.unknown_count)
        .bind(updated_at)
        .execute(tx)
        .await?;
        Ok(())
    }

    async fn upsert_auth_token_activity_delta_pg(
        tx: &mut sqlx::PgConnection,
        token_id: &str,
        delta: AuthTokenActivityDelta,
    ) -> Result<(), ProxyError> {
        if delta.is_zero() {
            return Ok(());
        }

        sqlx::query(
            r#"
            UPDATE core.auth_tokens
            SET total_requests = total_requests + $1,
                last_used_at = CASE
                    WHEN $2 IS NULL THEN last_used_at
                    WHEN last_used_at IS NULL OR last_used_at < $3 THEN $4
                    ELSE last_used_at
                END
            WHERE id = $5 AND deleted_at IS NULL
            "#,
        )
        .bind(delta.total_requests_delta)
        .bind(delta.last_used_at)
        .bind(delta.last_used_at)
        .bind(delta.last_used_at)
        .bind(token_id)
        .execute(tx)
        .await?;
        Ok(())
    }

    async fn log_effect_bucket_migration_needed_for_table_pg(
        &self,
        table: &str,
        target_column: &str,
        legacy_effect_codes: &[&str],
    ) -> Result<bool, ProxyError> {
        let placeholders = (1..=legacy_effect_codes.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT 1 FROM {table}
             WHERE key_effect_code IN ({placeholders})
               AND ({target_column} IS NULL OR TRIM({target_column}) = '' OR {target_column} = 'none')
             LIMIT 1"
        );
        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        for code in legacy_effect_codes {
            query = query.bind(*code);
        }
        Ok(query.fetch_optional(self.pg()?).await?.is_some())
    }

    async fn migrate_log_effect_buckets_pg(&self) -> Result<(), ProxyError> {
        if self
            .get_meta_i64(META_KEY_REQUEST_LOG_EFFECT_BUCKET_MIGRATION_V1_DONE)
            .await?
            == Some(1)
        {
            return Ok(());
        }
        let request_logs_table = "observability.request_logs";
        let binding_codes = [
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND,
            KEY_EFFECT_API_REBALANCE_ROUTE_BOUND,
            KEY_EFFECT_API_REBALANCE_ROUTE_REUSED,
            KEY_EFFECT_API_REBALANCE_ROUTE_REBOUND,
        ];
        let selection_codes = [
            KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED,
            KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED,
            KEY_EFFECT_MCP_SESSION_INIT_PRESSURE_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED,
            KEY_EFFECT_API_REBALANCE_COOLDOWN_AVOIDED,
            KEY_EFFECT_API_REBALANCE_RATE_LIMIT_AVOIDED,
            KEY_EFFECT_API_REBALANCE_PRESSURE_AVOIDED,
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
        let needs_migration = self
            .log_effect_bucket_migration_needed_for_table(
                request_logs_table,
                "binding_effect_code",
                &binding_codes,
            )
            .await?
            || self
                .log_effect_bucket_migration_needed_for_table(
                    request_logs_table,
                    "selection_effect_code",
                    &selection_codes,
                )
                .await?
            || self
                .log_effect_bucket_migration_needed_for_table(
                    "core.auth_token_logs",
                    "binding_effect_code",
                    &binding_codes,
                )
                .await?
            || self
                .log_effect_bucket_migration_needed_for_table(
                    "core.auth_token_logs",
                    "selection_effect_code",
                    &selection_codes,
                )
                .await?;
        if !needs_migration {
            self.set_meta_i64(META_KEY_REQUEST_LOG_EFFECT_BUCKET_MIGRATION_V1_DONE, 1)
                .await?;
            return Ok(());
        }

        let mut tx = self.pg()?.begin().await?;
        for table in [request_logs_table, "core.auth_token_logs"] {
            let binding_sql = format!(
                "UPDATE {table}
                 SET binding_effect_code = key_effect_code,
                     binding_effect_summary = key_effect_summary,
                     key_effect_code = 'none',
                     key_effect_summary = NULL
                 WHERE key_effect_code IN ($1, $2, $3, $4, $5, $6)
                   AND (binding_effect_code IS NULL OR TRIM(binding_effect_code) = '' OR binding_effect_code = 'none')"
            );
            sqlx::query(&binding_sql)
                .bind(binding_codes[0])
                .bind(binding_codes[1])
                .bind(binding_codes[2])
                .bind(binding_codes[3])
                .bind(binding_codes[4])
                .bind(binding_codes[5])
                .execute(&mut *tx)
                .await?;

            let selection_sql = format!(
                "UPDATE {table}
                 SET selection_effect_code = key_effect_code,
                     selection_effect_summary = key_effect_summary,
                     key_effect_code = 'none',
                     key_effect_summary = NULL
                 WHERE key_effect_code IN ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                   AND (selection_effect_code IS NULL OR TRIM(selection_effect_code) = '' OR selection_effect_code = 'none')"
            );
            sqlx::query(&selection_sql)
                .bind(selection_codes[0])
                .bind(selection_codes[1])
                .bind(selection_codes[2])
                .bind(selection_codes[3])
                .bind(selection_codes[4])
                .bind(selection_codes[5])
                .bind(selection_codes[6])
                .bind(selection_codes[7])
                .bind(selection_codes[8])
                .execute(&mut *tx)
                .await?;
        }
        set_meta_i64_executor_pg(
            &mut *tx,
            META_KEY_REQUEST_LOG_EFFECT_BUCKET_MIGRATION_V1_DONE,
            1,
        )
        .await?;
        tx.commit().await?;

        Ok(())
    }

    async fn fetch_request_logs_rollup_total_pg(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<i64, ProxyError> {
        let mut query = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT COALESCE(SUM(request_count), 0)::bigint FROM observability.request_log_catalog_rollups",
        );
        Self::push_request_log_catalog_rollup_filters_pg(&mut query, scoped_key_id, since, filters);
        query
            .build_query_scalar::<i64>()
            .fetch_one(self.pg()?)
            .await
            .map_err(ProxyError::from)
    }

    #[allow(clippy::too_many_arguments)]
    async fn request_logs_exist_for_filters_pg(
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
    ) -> Result<bool, ProxyError> {
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key", crate::analysis::SqlDialect::Postgres);
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let stored_counts_business_quota_sql = format!(
            "COALESCE(counts_business_quota, {})",
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres)
        );
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres);
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

        let mut query =
            QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM observability.request_logs");
        let has_where = Self::push_request_logs_scope_pg(&mut query, scoped_key_id, since, None);
        let mut has_where = has_where;
        if let Some(key_effect_code) = key_effect_code {
            query.push(if has_where {
                " AND key_effect_code = "
            } else {
                " WHERE key_effect_code = "
            });
            query.push_bind(key_effect_code.to_string());
            has_where = true;
        }
        if let Some(binding_effect_code) = binding_effect_code {
            query.push(if has_where {
                " AND binding_effect_code = "
            } else {
                " WHERE binding_effect_code = "
            });
            query.push_bind(binding_effect_code.to_string());
            has_where = true;
        }
        if let Some(selection_effect_code) = selection_effect_code {
            query.push(if has_where {
                " AND selection_effect_code = "
            } else {
                " WHERE selection_effect_code = "
            });
            query.push_bind(selection_effect_code.to_string());
            has_where = true;
        }
        if let Some(auth_token_id) = auth_token_id {
            query.push(if has_where {
                " AND auth_token_id = "
            } else {
                " WHERE auth_token_id = "
            });
            query.push_bind(auth_token_id.to_string());
            has_where = true;
        }
        if let Some(key_id) = key_id {
            query.push(if has_where {
                " AND api_key_id = "
            } else {
                " WHERE api_key_id = "
            });
            query.push_bind(key_id.to_string());
            has_where = true;
        }
        if !normalized_request_kinds.is_empty() {
            query.push(if has_where { " AND " } else { " WHERE " });
            Self::push_effective_request_kind_filter_clause_pg(
                &mut query,
                &effective_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = result_status {
            query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                &mut query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                &mut query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        query.push(" LIMIT 1");
        Ok(query.build().fetch_optional(self.pg()?).await?.is_some())
    }

    async fn fetch_request_log_request_kind_options_pg(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        type RequestKindOptionRow = (String, String, i64);
        let mut legacy_query = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT request_kind_key, request_kind_label, SUM(request_count)::bigint AS request_count FROM observability.request_log_catalog_rollups",
        );
        Self::push_request_log_catalog_rollup_filters_pg(
            &mut legacy_query,
            scoped_key_id,
            since,
            filters,
        );
        legacy_query.push(" GROUP BY 1, 2");

        let rows = legacy_query
            .build_query_as::<RequestKindOptionRow>()
            .fetch_all(self.pg()?)
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

    async fn fetch_request_log_facet_options_pg(
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
        let mut query = QueryBuilder::<sqlx::Postgres>::new(format!(
            "SELECT {column_expr} AS value, SUM(request_count)::bigint AS count FROM observability.request_log_catalog_rollups"
        ));
        Self::push_request_log_catalog_rollup_filters_pg(&mut query, scoped_key_id, since, filters);
        if require_non_empty {
            query.push(" AND TRIM(");
            query.push(column_expr);
            query.push(") <> ''");
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

    async fn fetch_request_log_result_facet_options_pg(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        filters: RequestLogsCatalogFilters<'_>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        let mut query = QueryBuilder::<sqlx::Postgres>::new(
            "
            SELECT
                result_bucket AS value,
                SUM(request_count)::bigint AS count
            FROM observability.request_log_catalog_rollups
            ",
        );
        Self::push_request_log_catalog_rollup_filters_pg(&mut query, scoped_key_id, since, filters);
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_request_logs_cursor_page_pg(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        request_user_id: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        page_size: i64,
    ) -> Result<RequestLogsCursorPage, ProxyError> {
        let started = Instant::now();
        let retention_days = self
            .get_system_settings()
            .await?
            .request_log_retention
            .max_log_retention_days;
        let since = Self::clamp_request_logs_rollup_since_at(
            since,
            retention_days,
            self.backend_time.local_now(),
        );
        let page_size = page_size.clamp(1, 200);
        let query_limit = page_size + 1;
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key", crate::analysis::SqlDialect::Postgres);
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let stored_counts_business_quota_sql = format!(
            "COALESCE(counts_business_quota, {})",
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres)
        );
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres);
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

        let mut items_query = QueryBuilder::<sqlx::Postgres>::new(format!(
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
                {effective_operational_class_sql} AS operational_class,
                {effective_request_kind_protocol_group_sql} AS request_kind_protocol_group,
                {effective_request_kind_billing_group_sql} AS request_kind_billing_group,
                created_at
            FROM observability.request_logs
            "#
        ));
        let has_where =
            Self::push_request_logs_scope_pg(&mut items_query, scoped_key_id, since, until);
        Self::push_request_logs_filters_pg(
            &mut items_query,
            RequestLogFilterParams {
                request_kinds: &[],
                result_status: None,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                request_user_id,
                auth_token_id,
                key_id,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql: &legacy_request_kind_predicate_sql,
                legacy_request_kind_sql: &legacy_request_kind_sql,
                has_where,
            },
        );
        if !normalized_request_kinds.is_empty() {
            items_query.push(" AND ");
            Self::push_effective_request_kind_filter_clause_pg(
                &mut items_query,
                &effective_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = result_status {
            items_query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                &mut items_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            items_query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
                &mut items_query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        Self::push_desc_cursor_clause_pg(
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

        let mut rows = items_query.build().fetch_all(self.pg()?).await?;
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

        let result = RequestLogsCursorPage {
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
        };
        emit_low_memory_protection_decision(
            "admin_read",
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some(if scoped_key_id.is_some() {
                    "key"
                } else {
                    "global"
                }),
                page_size: Some(page_size),
                degraded: Some("full"),
                ..Default::default()
            },
        );
        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "request_logs_list_completed",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some(if scoped_key_id.is_some() {
                    "key"
                } else {
                    "global"
                }),
                page_size: Some(page_size),
                row_count: Some(result.items.len()),
                degraded: Some("full"),
                ..Default::default()
            },
        );
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_request_logs_page_pg(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        request_user_id: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
        include_token_facets: bool,
        include_key_facets: bool,
        include_bodies: bool,
    ) -> Result<RequestLogsPage, ProxyError> {
        let retention_days = self
            .get_system_settings()
            .await?
            .request_log_retention
            .max_log_retention_days;
        let since = Self::clamp_request_logs_rollup_since_at(
            since,
            retention_days,
            self.backend_time.local_now(),
        );
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
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key", crate::analysis::SqlDialect::Postgres);
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let stored_counts_business_quota_sql = format!(
            "COALESCE(counts_business_quota, {})",
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres)
        );
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres);
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
        let mut items_query = QueryBuilder::<sqlx::Postgres>::new(format!(
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
                {effective_operational_class_sql} AS operational_class,
                {effective_request_kind_protocol_group_sql} AS request_kind_protocol_group,
                {effective_request_kind_billing_group_sql} AS request_kind_billing_group,
                created_at
            FROM observability.request_logs
            "#
        ));
        let has_where = Self::push_request_logs_scope_pg(&mut items_query, scoped_key_id, since, None);
        Self::push_request_logs_filters_pg(
            &mut items_query,
            RequestLogFilterParams {
                request_kinds: &[],
                result_status: None,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                request_user_id,
                auth_token_id,
                key_id,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql: &legacy_request_kind_predicate_sql,
                legacy_request_kind_sql: &legacy_request_kind_sql,
                has_where,
            },
        );
        if !normalized_request_kinds.is_empty() {
            items_query.push(" AND ");
            Self::push_effective_request_kind_filter_clause_pg(
                &mut items_query,
                &effective_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = result_status {
            items_query.push(" AND ");
            Self::push_result_bucket_filter_clause_pg(
                &mut items_query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            items_query.push(" AND ");
            Self::push_operational_class_filter_clause_pg(
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
        let rows = items_query.build().fetch_all(self.pg()?).await?;
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

    pub(crate) async fn ensure_request_log_catalog_rollup_schema_pg(&self) -> Result<(), ProxyError> {
        let missing: Option<String> = sqlx::query_scalar(
            r#"
            SELECT required.rel
            FROM unnest($1::text[]) AS required(rel)
            WHERE to_regclass(required.rel) IS NULL
            ORDER BY required.rel
            LIMIT 1
            "#,
        )
        .bind(vec![
            "observability.request_log_catalog_rollups",
            "observability.idx_request_log_catalog_rollups_kind_time",
            "observability.idx_request_log_catalog_rollups_result_time",
            "observability.idx_request_log_catalog_rollups_token_time",
            "observability.idx_request_log_catalog_rollups_key_time",
            "observability.idx_request_log_catalog_rollups_operational_time",
        ])
        .fetch_optional(self.pg()?)
        .await?;
        match missing {
            None => Ok(()),
            Some(relation) => Err(ProxyError::Other(format!(
                "postgresql baseline is missing {relation}; apply src/store/pg/migrations first"
            ))),
        }
    }

    pub(crate) async fn rebuild_request_log_catalog_rollups_pg(&self) -> Result<(), ProxyError> {
        let retention_days = self
            .get_system_settings()
            .await?
            .request_log_retention
            .max_log_retention_days;
        let since = configured_request_logs_retention_threshold_utc_ts_at(
            retention_days,
            self.backend_time.local_now(),
        );
        let exprs = Self::request_log_catalog_rollup_exprs("");
        let canonical_request_kind_predicate_sql =
            canonical_request_kind_stored_predicate_sql("request_kind_key");
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql("request_kind_key");
        let canonicalize_legacy_rows_sql = format!(
            r#"
            UPDATE observability.request_logs
            SET request_kind_key = request_kind_key
            WHERE visibility = 'visible'
              AND created_at >= $1
              AND {legacy_request_kind_predicate_sql}
            "#
        );
        sqlx::query(&canonicalize_legacy_rows_sql)
            .bind(since)
            .execute(self.pg()?)
            .await?;
        sqlx::query("DELETE FROM observability.request_log_catalog_rollups")
            .execute(self.pg()?)
            .await?;
        let rebuild_sql = format!(
            r#"
            INSERT INTO observability.request_log_catalog_rollups (
                {},
                request_count,
                updated_at
            )
            SELECT
                {},
                COUNT(*) AS request_count,
                (EXTRACT(EPOCH FROM now()))::bigint AS updated_at
            FROM observability.request_logs
            WHERE visibility = 'visible'
              AND created_at >= $1
              AND {canonical_request_kind_predicate_sql}
            GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
            "#,
            Self::request_log_catalog_rollup_columns(),
            exprs.join(", "),
        );
        sqlx::query(&rebuild_sql)
            .bind(since)
            .execute(self.pg()?)
            .await?;
        self.invalidate_request_logs_catalog_cache().await;
        Ok(())
    }

    pub(crate) async fn fetch_api_key_secret_pg(
        &self,
        key_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let secret = sqlx::query_scalar::<_, String>(
            "SELECT api_key FROM core.api_keys WHERE id = $1 LIMIT 1",
        )
        .bind(key_id)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(secret.filter(|value| !value.trim().is_empty()))
    }

    pub(crate) async fn fetch_api_key_id_by_secret_pg(
        &self,
        secret: &str,
    ) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM core.api_keys WHERE api_key = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(secret)
        .fetch_optional(self.pg()?)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_state_snapshot_pg(
        &self,
        key_id: &str,
    ) -> Result<KeyStateSnapshot, ProxyError> {
        let status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT status FROM core.api_keys WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(key_id)
        .fetch_optional(self.pg()?)
        .await?
        .flatten();
        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM core.api_key_quarantines
            WHERE key_id = $1 AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(self.pg()?)
        .await?
        .is_some();
        Ok(KeyStateSnapshot {
            status,
            quarantined,
        })
    }

    pub(crate) async fn insert_api_key_maintenance_record_pg(
        &self,
        record: ApiKeyMaintenanceRecord,
    ) -> Result<(), ProxyError> {
        let auth_token_id = if let Some(auth_token_id) = record.auth_token_id.as_deref() {
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM core.auth_tokens WHERE id = $1 LIMIT 1")
                .bind(auth_token_id)
                .fetch_optional(self.pg()?)
                .await?
                .map(|_| auth_token_id.to_string())
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO core.api_key_maintenance_records (
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
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
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_quota_for_key_pg(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE core.api_keys
               SET quota_limit = $1, quota_remaining = $2, quota_synced_at = $3
             WHERE id = $4"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(self.pg()?)
        .await?;
        self.request_stats_coalescer
            .mark_dashboard_read_dirty()
            .await;
        Ok(())
    }

    pub(crate) async fn record_quota_sync_sample_pg(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
        source: &str,
    ) -> Result<(), ProxyError> {
        let mut tx = self.pg()?.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO core.api_key_quota_sync_samples (
                key_id,
                quota_limit,
                quota_remaining,
                captured_at,
                source
            ) VALUES ($1, $2, $3, $4, $5)
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
            r#"UPDATE core.api_keys
               SET quota_limit = $1, quota_remaining = $2, quota_synced_at = $3
             WHERE id = $4"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.request_stats_coalescer
            .mark_dashboard_read_dirty()
            .await;
        Ok(())
    }

    async fn fetch_dashboard_quota_source_id_pg(&self, today_end: i64) -> Result<i64, ProxyError> {
        let mut before_id = None;
        for _ in 0..DASHBOARD_QUOTA_SOURCE_MAX_PAGES {
            let query_result = match before_id {
                Some(before_id) => {
                    sqlx::query(
                        r#"
                    SELECT id, captured_at
                    FROM core.api_key_quota_sync_samples
                    WHERE id > 0 AND id < $1
                    ORDER BY id DESC
                    LIMIT $2
                    "#,
                    )
                    .bind(before_id)
                    .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                    .fetch_all(self.pg()?)
                    .await
                }
                None => {
                    sqlx::query(
                        r#"
                    SELECT id, captured_at
                    FROM core.api_key_quota_sync_samples
                    WHERE id > 0
                    ORDER BY id DESC
                    LIMIT $1
                    "#,
                    )
                    .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                    .fetch_all(self.pg()?)
                    .await
                }
            };
            let samples = query_result?
                .into_iter()
                .map(|row| Ok::<_, sqlx::Error>((row.try_get("id")?, row.try_get("captured_at")?)))
                .collect::<Result<Vec<(i64, i64)>, _>>()?;

            if let Some((id, _)) = samples
                .iter()
                .copied()
                .find(|(_, captured_at)| *captured_at < today_end)
            {
                return Ok(id);
            }
            let Some((last_id, _)) = samples.last().copied() else {
                return Ok(0);
            };
            if samples.len() < DASHBOARD_QUOTA_SOURCE_PAGE_SIZE as usize {
                return Ok(0);
            }
            before_id = Some(last_id);
        }
        Err(self.dashboard_quota_read_budget_deferred())
    }

    async fn fetch_dashboard_quota_source_captured_at_pg(
        &self,
        today_end: i64,
    ) -> Result<i64, ProxyError> {
        let query_result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT captured_at
            FROM core.api_key_quota_sync_samples
            WHERE captured_at < $1
            ORDER BY captured_at DESC, key_id ASC, id ASC
            LIMIT $2
            "#,
        )
        .bind(today_end)
        .bind(1_i64)
        .fetch_optional(self.pg()?)
        .await;
        Ok(query_result?.unwrap_or_default())
    }

    pub(crate) async fn fetch_dashboard_quota_incremental_samples_pg(
        &self,
        after: DashboardQuotaSampleWatermark,
        today_end: i64,
        limit: i64,
    ) -> Result<(DashboardQuotaSampleWatermark, Vec<DashboardQuotaSample>), ProxyError> {
        let watermark = self
            .fetch_dashboard_quota_sample_watermark(today_end)
            .await?;
        if watermark == after || watermark.source_id <= after.source_id {
            return Ok((watermark, Vec::new()));
        }

        let query_result = sqlx::query(
            r#"
            WITH pending AS (
                SELECT id, key_id, quota_remaining, captured_at
                FROM core.api_key_quota_sync_samples
                WHERE id > $1
                  AND captured_at < $2
                ORDER BY id ASC
                LIMIT $3
            )
            SELECT
                pending.id,
                pending.key_id,
                pending.quota_remaining,
                pending.captured_at,
                previous.quota_remaining AS previous_quota_remaining
            FROM pending
            LEFT JOIN core.api_key_quota_sync_samples previous
                ON previous.id = (
                    SELECT id
                    FROM core.api_key_quota_sync_samples candidate
                    WHERE candidate.key_id = pending.key_id
                      AND candidate.id < pending.id
                      AND candidate.captured_at < $4
                    ORDER BY candidate.captured_at DESC, candidate.id DESC
                    LIMIT 1
                )
            ORDER BY pending.id ASC
            "#,
        )
        .bind(after.source_id)
        .bind(today_end)
        .bind(limit.max(1))
        .bind(today_end)
        .fetch_all(self.pg()?)
        .await;
        let samples = query_result?
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(DashboardQuotaSample {
                    id: row.try_get("id")?,
                    key_id: row.try_get("key_id")?,
                    quota_remaining: row.try_get("quota_remaining")?,
                    captured_at: row.try_get("captured_at")?,
                    previous_quota_remaining: row.try_get("previous_quota_remaining")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((watermark, samples))
    }

    async fn fetch_visible_request_log_floor_since_pg(
        &self,
        since: i64,
    ) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT MIN(created_at)
            FROM observability.request_logs
            WHERE visibility = $1
              AND created_at >= $2
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(since)
        .fetch_one(self.pg()?)
        .await
        .map_err(ProxyError::Database)
    }

    #[cfg(test)]
    async fn fetch_visible_request_log_window_metrics_pg(
        &self,
        start: i64,
        end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        if start >= end {
            return Ok(SummaryWindowMetrics::default());
        }

        let request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key", crate::analysis::SqlDialect::Postgres);
        let request_value_bucket_case_sql =
            request_value_bucket_sql(&request_kind_sql, "request_body", crate::analysis::SqlDialect::Postgres);
        let query = format!(
            r#"
            WITH scoped_logs AS (
                SELECT
                    result_status,
                    ({request_value_bucket_case_sql}) AS request_value_bucket
                FROM observability.request_logs
                WHERE visibility = $1
                  AND created_at >= $2
                  AND created_at < $3
            )
            SELECT
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = $4 THEN 1 ELSE 0 END), 0)::bigint AS success_count,
                COALESCE(SUM(CASE WHEN result_status = $5 THEN 1 ELSE 0 END), 0)::bigint AS error_count,
                COALESCE(SUM(CASE WHEN result_status = $6 THEN 1 ELSE 0 END), 0)::bigint AS quota_exhausted_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'valuable' AND result_status = $7 THEN 1 ELSE 0 END), 0)::bigint AS valuable_success_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'valuable' AND result_status IN ($8, $9) THEN 1 ELSE 0 END), 0)::bigint AS valuable_failure_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'other' AND result_status = $10 THEN 1 ELSE 0 END), 0)::bigint AS other_success_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'other' AND result_status IN ($11, $12) THEN 1 ELSE 0 END), 0)::bigint AS other_failure_count,
                COALESCE(SUM(CASE WHEN request_value_bucket = 'unknown' THEN 1 ELSE 0 END), 0)::bigint AS unknown_count
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
            .fetch_one(self.pg()?)
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

    #[cfg(test)]
    async fn fetch_api_key_usage_bucket_window_metrics_pg(
        &self,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let row = if let Some(bucket_start_before) = bucket_start_before {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0)::bigint AS total_requests,
                    COALESCE(SUM(success_count), 0)::bigint AS success_count,
                    COALESCE(SUM(error_count), 0)::bigint AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0)::bigint AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0)::bigint AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0)::bigint AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0)::bigint AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0)::bigint AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0)::bigint AS unknown_count
                FROM observability.api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= $1
                  AND bucket_start < $2
                "#,
            )
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(self.pg()?)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0)::bigint AS total_requests,
                    COALESCE(SUM(success_count), 0)::bigint AS success_count,
                    COALESCE(SUM(error_count), 0)::bigint AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0)::bigint AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0)::bigint AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0)::bigint AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0)::bigint AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0)::bigint AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0)::bigint AS unknown_count
                FROM observability.api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= $1
                "#,
            )
            .bind(bucket_start_at_least)
            .fetch_one(self.pg()?)
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

    pub(crate) async fn fetch_success_breakdown_from_dashboard_rollups_pg(
        &self,
        month_start: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let now = self.backend_time.now_ts();
        let month_request_log_floor = self
            .fetch_visible_request_log_floor_since(month_start)
            .await?;
        let historical_month_success = self
            .fetch_utc_month_gap_success_count(month_start, month_request_log_floor, now)
            .await?;
        let mut tx = self.pg()?.begin().await?;
        let (retained_partial_minute_success, dashboard_month_start) = match month_request_log_floor
        {
            Some(floor) if floor > month_start => {
                let minute_start = floor.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
                if floor == minute_start {
                    (0, floor)
                } else {
                    let next_minute_start = minute_start.saturating_add(SECS_PER_MINUTE);
                    let partial_minute_success = Self::fetch_visible_request_log_success_count_tx_pg(
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
        let dashboard_month_success = Self::fetch_dashboard_rollup_success_count_for_range_tx_pg(
            &mut tx,
            dashboard_month_start,
            now.saturating_add(1),
        )
        .await?;
        let daily_success =
            Self::fetch_dashboard_rollup_success_count_for_range_tx_pg(&mut tx, day_start, day_end)
                .await?;
        tx.commit().await?;

        Ok(SuccessBreakdown {
            monthly_success: historical_month_success
                + retained_partial_minute_success
                + dashboard_month_success,
            daily_success,
        })
    }
}
