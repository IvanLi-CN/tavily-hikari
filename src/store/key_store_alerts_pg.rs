/// PostgreSQL branches of the alert readers.
///
/// The SQLite implementations stay in `key_store_alerts.rs` and keep serving the request path
/// until the cutover wave; these variants are reachable only through the attached PostgreSQL
/// pool. Both branches build their SQL from the same dialect-aware CTE helpers, so the query
/// text stays single-sourced.
impl KeyStore {
    /// PostgreSQL branch of the filtered projection page. The dialect-aware CTE builder supplies
    /// the JSONB rewrite and `$n` placeholders, so the SQL body stays single-sourced with SQLite.
    ///
    /// The SQLite path additionally refreshes projection coverage before reading; that step
    /// maintains SQLite projection state and has no PostgreSQL counterpart yet, so this branch
    /// reads whatever the projection writer has already produced.
    #[allow(clippy::too_many_arguments)]
    async fn fetch_admin_alert_events_page_pg(
        &self,
        alert_type: Option<&str>,
        since: Option<i64>,
        until: Option<i64>,
        user_id: Option<&str>,
        token_id: Option<&str>,
        key_id: Option<&str>,
        request_kinds: &[String],
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedAlertEvents, ProxyError> {
        let filters = self.retain_alert_filters(AlertEventFilters {
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            request_kinds,
        });
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let request_kind_expr = "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')";
        let mut session = self.begin_pg_read_session().await?;

        let mut count = AlertSql::new(SqlDialect::Postgres);
        Self::push_projected_alert_events_cte_any(&mut count, filters, SqlDialect::Postgres);
        count.push(" SELECT COUNT(*) FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter_any(&mut count, request_kind_expr, filters.request_kinds);
        let total = count
            .into_postgres()
            .expect("postgres alert builder")
            .build_query_scalar::<i64>()
            .fetch_one(&mut *session.connection())
            .await?;

        let mut query = AlertSql::new(SqlDialect::Postgres);
        Self::push_projected_alert_events_cte_any(&mut query, filters, SqlDialect::Postgres);
        query.push(" SELECT * FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter_any(&mut query, request_kind_expr, filters.request_kinds);
        query.push(" ORDER BY occurred_at DESC, row_sort_id DESC LIMIT ");
        query.push_bind(per_page);
        query.push(" OFFSET ");
        query.push_bind(offset);
        let rows = query
            .into_postgres()
            .expect("postgres alert builder")
            .build()
            .fetch_all(&mut *session.connection())
            .await?;
        session.finish().await?;
        let items = rows
            .into_iter()
            .map(Self::decode_alert_event_projection_row_pg)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect();
        Ok(PaginatedAlertEvents {
            items,
            total,
            page,
            per_page,
        })
    }

    /// PostgreSQL branch of the canonical-snapshot page. One repeatable-read transaction keeps
    /// the total and the page on the same canonical build, matching the SQLite read session.
    async fn fetch_admin_alert_events_page_for_canonical_snapshot_pg(
        &self,
        build_generation: i64,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedAlertEvents, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let retention_since = self.alert_projection_retention_since();
        let mut session = self.begin_pg_read_session().await?;
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = $1 AND occurred_at >= $2",
        )
        .bind(build_generation)
        .bind(retention_since)
        .fetch_one(&mut *session.connection())
        .await?;
        let rows = sqlx::query(
            "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
             FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = $1 AND occurred_at >= $2 \
             ORDER BY occurred_at DESC, row_sort_id DESC LIMIT $3 OFFSET $4",
        )
        .bind(build_generation)
        .bind(retention_since)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&mut *session.connection())
        .await?;
        session.finish().await?;
        let items = rows
            .into_iter()
            .map(Self::decode_default_alert_event_projection_row_pg)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect();
        Ok(PaginatedAlertEvents {
            items,
            total,
            page,
            per_page,
        })
    }

    /// PostgreSQL twin of [`Self::decode_default_alert_event_projection_row`]; the canonical
    /// relations keep the same column names, so only the row type changes.
    fn decode_default_alert_event_projection_row_pg(
        row: sqlx::postgres::PgRow,
    ) -> Result<AlertEventProjectionRow, ProxyError> {
        let payload_json = row.try_get::<String, _>("payload_json")?;
        let mut projection = serde_json::from_str::<AlertEventProjectionRow>(&payload_json)
            .map_err(|_| ProxyError::Other("invalid alert projection payload".to_string()))?;
        projection.source_kind = row.try_get("source_kind")?;
        projection.source_id = row.try_get("source_id")?;
        projection.row_sort_id = row.try_get("row_sort_id")?;
        projection.occurred_at = row.try_get("occurred_at")?;
        Ok(projection)
    }

    /// PostgreSQL twin of [`Self::decode_alert_event_projection_row`]. The projection relation
    /// keeps the SQLite column aliases, so only the row type and the boolean encoding differ:
    /// `counts_business_quota` is stored as `bigint` in the PG baseline.
    fn decode_alert_event_projection_row_pg(
        row: sqlx::postgres::PgRow,
    ) -> Result<AlertEventProjectionRow, sqlx::Error> {
        Ok(AlertEventProjectionRow {
            source_kind: row.try_get("source_kind")?,
            source_id: row.try_get("source_id")?,
            row_sort_id: row.try_get("row_sort_id")?,
            alert_type: row.try_get("alert_type")?,
            occurred_at: row.try_get("occurred_at")?,
            token_id: row.try_get("token_id")?,
            key_id: row.try_get("key_id")?,
            request_log_id: row.try_get("request_log_id")?,
            method: row.try_get("method")?,
            path: row.try_get("path")?,
            query: row.try_get("query")?,
            request_kind_key: row.try_get("request_kind_key")?,
            request_kind_label: row.try_get("request_kind_label")?,
            request_kind_detail: row.try_get("request_kind_detail")?,
            result_status: row.try_get("result_status")?,
            failure_kind: row.try_get("failure_kind")?,
            error_message: row.try_get("error_message")?,
            counts_business_quota: row
                .try_get::<Option<i64>, _>("counts_business_quota")?
                .map(|value| value != 0),
            user_id: row.try_get("user_id")?,
            user_display_name: row.try_get("user_display_name")?,
            user_username: row.try_get("user_username")?,
            reason_code: row.try_get("reason_code")?,
            reason_summary: row.try_get("reason_summary")?,
            reason_detail: row.try_get("reason_detail")?,
            job_id: row.try_get("job_id")?,
            job_type: row.try_get("job_type")?,
            job_trigger_source: row.try_get("job_trigger_source")?,
            job_status: row.try_get("job_status")?,
            job_attempt: row.try_get("job_attempt")?,
            job_message: row.try_get("job_message")?,
            job_queued_at: row.try_get("job_queued_at")?,
            job_started_at: row.try_get("job_started_at")?,
            job_finished_at: row.try_get("job_finished_at")?,
        })
    }
}
