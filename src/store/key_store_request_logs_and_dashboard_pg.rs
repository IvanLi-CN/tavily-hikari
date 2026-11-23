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

}
