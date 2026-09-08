impl KeyStore {
    fn push_alert_projection_source_selection<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        selected_source: Option<(&str, &'a [String])>,
        branch_source: &str,
        source_id_expr: &str,
    ) {
        let Some((source_kind, source_ids)) = selected_source else {
            return;
        };
        if source_kind != branch_source || source_ids.is_empty() {
            query.push(" AND 1 = 0");
            return;
        }
        query.push(" AND ").push(source_id_expr).push(" IN (");
        {
            let mut separated = query.separated(", ");
            for source_id in source_ids {
                separated.push_bind(source_id);
            }
        }
        query.push(")");
    }

    fn alert_subject_kind_sql(alias: &str) -> String {
        format!(
            "CASE \
                WHEN {alias}.alert_type = 'job_failed' AND {alias}.job_id IS NOT NULL THEN 'job' \
                WHEN {alias}.alert_type IN ('upstream_rate_limited_429', 'upstream_usage_limit_432', 'upstream_key_blocked', 'api_key_exhausted') AND {alias}.key_id IS NOT NULL THEN 'key' \
                WHEN {alias}.user_id IS NOT NULL THEN 'user' \
                WHEN {alias}.token_id IS NOT NULL THEN 'token' \
                WHEN {alias}.key_id IS NOT NULL THEN 'key' \
                ELSE 'token' \
            END"
        )
    }

    fn alert_subject_id_sql(alias: &str) -> String {
        format!(
            "CASE \
                WHEN {alias}.alert_type = 'job_failed' AND {alias}.job_id IS NOT NULL THEN CAST({alias}.job_id AS TEXT) \
                WHEN {alias}.alert_type IN ('upstream_rate_limited_429', 'upstream_usage_limit_432', 'upstream_key_blocked', 'api_key_exhausted') AND {alias}.key_id IS NOT NULL THEN {alias}.key_id \
                WHEN {alias}.user_id IS NOT NULL THEN {alias}.user_id \
                WHEN {alias}.token_id IS NOT NULL THEN {alias}.token_id \
                WHEN {alias}.key_id IS NOT NULL THEN {alias}.key_id \
                ELSE 'unknown' \
            END"
        )
    }

    fn push_alert_request_kind_filter(
        query: &mut QueryBuilder<'_, Sqlite>,
        request_kind_expr: &str,
        request_kinds: &[String],
    ) {
        let normalized = normalize_alert_request_kind_filters(request_kinds);
        if normalized.is_empty() {
            return;
        }
        query.push(" AND ");
        query.push(request_kind_expr);
        query.push(" IN (");
        {
            let mut separated = query.separated(", ");
            for request_kind in normalized {
                separated.push_bind(request_kind);
            }
        }
        query.push(")");
    }

    fn push_auth_alert_filters<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        key_expr: &str,
    ) {
        if let Some(alert_type) = filters.alert_type {
            match alert_type {
                ALERT_TYPE_UPSTREAM_RATE_LIMITED_429 => {
                    query.push(" AND atl.failure_kind = ").push_bind(alert_type);
                }
                ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432 => {
                    query.push(
                        " AND atl.result_status = 'quota_exhausted' AND COALESCE(atl.http_status, rl.tavily_status_code) = 432",
                    );
                }
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED => {
                    query.push(
                        " AND atl.result_status = 'quota_exhausted' AND atl.counts_business_quota = 0",
                    );
                }
                ALERT_TYPE_USER_QUOTA_EXHAUSTED => {
                    query.push(
                        " AND atl.result_status = 'quota_exhausted' AND COALESCE(atl.http_status, rl.tavily_status_code, 0) <> 432 AND atl.counts_business_quota <> 0",
                    );
                }
                ALERT_TYPE_UPSTREAM_KEY_BLOCKED => {
                    query.push(" AND 1 = 0");
                }
                _ => {
                    query.push(" AND 1 = 0");
                }
            };
        }
        if let Some(since) = filters.since {
            query.push(" AND atl.created_at >= ").push_bind(since);
        }
        if let Some(until) = filters.until {
            query.push(" AND atl.created_at <= ").push_bind(until);
        }
        if let Some(user_id) = filters.user_id {
            query.push(" AND u.id = ").push_bind(user_id);
        }
        if let Some(token_id) = filters.token_id {
            query.push(" AND atl.token_id = ").push_bind(token_id);
        }
        if let Some(key_id) = filters.key_id {
            query.push(" AND ").push(key_expr).push(" = ").push_bind(key_id);
        }
    }

    fn push_maintenance_alert_filters<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        branch_alert_type: &str,
    ) {
        if let Some(alert_type) = filters.alert_type
            && alert_type != branch_alert_type
        {
            query.push(" AND 1 = 0");
        }
        if let Some(since) = filters.since {
            query.push(" AND m.created_at >= ").push_bind(since);
        }
        if let Some(until) = filters.until {
            query.push(" AND m.created_at <= ").push_bind(until);
        }
        if let Some(user_id) = filters.user_id {
            query.push(" AND u.id = ").push_bind(user_id);
        }
        if let Some(token_id) = filters.token_id {
            query
                .push(" AND COALESCE(m.auth_token_id, atl.token_id) = ")
                .push_bind(token_id);
        }
        if let Some(key_id) = filters.key_id {
            query.push(" AND m.key_id = ").push_bind(key_id);
        }
    }

    fn push_job_alert_filters<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        if let Some(alert_type) = filters.alert_type
            && alert_type != ALERT_TYPE_JOB_FAILED
        {
            query.push(" AND 1 = 0");
        }
        let occurred_at_expr = "COALESCE(j.finished_at, j.started_at, j.queued_at)";
        if let Some(since) = filters.since {
            query.push(" AND ").push(occurred_at_expr).push(" >= ").push_bind(since);
        }
        if let Some(until) = filters.until {
            query.push(" AND ").push(occurred_at_expr).push(" <= ").push_bind(until);
        }
        if filters.user_id.is_some() || filters.token_id.is_some() {
            query.push(" AND 1 = 0");
        }
        if let Some(key_id) = filters.key_id {
            query.push(" AND j.key_id = ").push_bind(key_id);
        }
    }

    fn push_alert_events_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        Self::push_alert_events_cte_for_selected_source(query, filters, None);
    }

    // Projection first obtains a small source-key page through an indexed
    // watermark seek. Restricting this CTE to those keys keeps its joins from
    // becoming a full source-table scan during catch-up.
    fn push_alert_events_cte_for_projection_source_keys<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        source_kind: &'a str,
        source_ids: &'a [String],
    ) {
        Self::push_alert_events_cte_for_selected_source(
            query,
            filters,
            Some((source_kind, source_ids)),
        );
    }

    fn push_alert_events_cte_for_selected_source<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        selected_source: Option<(&'a str, &'a [String])>,
    ) {
        let effective_request_kind_sql = token_log_request_kind_key_sql(
            "COALESCE(rl.path, atl.path)",
            "COALESCE(atl.request_kind_key, rl.request_kind_key)",
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let maintenance_request_kind_sql = token_log_request_kind_key_sql(
            "COALESCE(atl.path, rl.path)",
            "COALESCE(atl.request_kind_key, rl.request_kind_key)",
        );
        let maintenance_request_kind_label_sql =
            canonical_request_kind_label_sql(&maintenance_request_kind_sql);

        query.push("WITH alerts AS (");
        query.push(" SELECT ");
        query.push_bind(ALERT_SOURCE_AUTH_TOKEN_LOG);
        query.push(format!(
            r#" AS source_kind,
                CAST(atl.id AS TEXT) AS source_id,
                printf('atl:%020lld', atl.id) AS row_sort_id,
                CASE
                    WHEN atl.failure_kind = 'upstream_rate_limited_429' THEN 'upstream_rate_limited_429'
                    WHEN atl.result_status = 'quota_exhausted' AND COALESCE(atl.http_status, rl.tavily_status_code) = 432 THEN 'upstream_usage_limit_432'
                    WHEN atl.result_status = 'quota_exhausted' AND atl.counts_business_quota = 0 THEN 'user_request_rate_limited'
                    WHEN atl.result_status = 'quota_exhausted' THEN 'user_quota_exhausted'
                    ELSE ''
                END AS alert_type,
                atl.created_at AS occurred_at,
                atl.token_id AS token_id,
                COALESCE(atl.api_key_id, rl.api_key_id) AS key_id,
                atl.request_log_id AS request_log_id,
                COALESCE(NULLIF(TRIM(atl.method), ''), rl.method) AS method,
                COALESCE(NULLIF(TRIM(atl.path), ''), rl.path) AS path,
                COALESCE(atl.query, rl.query) AS query,
                {effective_request_kind_sql} AS request_kind_key,
                {effective_request_kind_label_sql} AS request_kind_label,
                atl.request_kind_detail AS request_kind_detail,
                atl.result_status AS result_status,
                atl.failure_kind AS failure_kind,
                atl.error_message AS error_message,
                atl.counts_business_quota AS counts_business_quota,
                u.id AS user_id,
                u.display_name AS user_display_name,
                u.username AS user_username,
                NULL AS reason_code,
                NULL AS reason_summary,
                NULL AS reason_detail,
                NULL AS job_id,
                NULL AS job_type,
                NULL AS job_trigger_source,
                NULL AS job_status,
                NULL AS job_attempt,
                NULL AS job_message,
                NULL AS job_queued_at,
                NULL AS job_started_at,
                NULL AS job_finished_at
            FROM auth_token_logs atl
            LEFT JOIN observability.request_logs rl
              ON rl.id = atl.request_log_id
             AND (
                    atl.method IS NULL
                 OR atl.method = ''
                 OR atl.path IS NULL
                 OR atl.path = ''
                 OR atl.request_kind_key IS NULL
                 OR atl.request_kind_key = ''
                 OR atl.request_kind_label IS NULL
                 OR atl.request_kind_label = ''
                 OR atl.api_key_id IS NULL
                 OR atl.query IS NULL
                )
            LEFT JOIN user_token_bindings b ON b.token_id = atl.token_id
            LEFT JOIN users u ON u.id = b.user_id
            WHERE (
                atl.failure_kind = 'upstream_rate_limited_429'
                OR atl.result_status = 'quota_exhausted'
            )
            "#
        ));
        Self::push_auth_alert_filters(
            query,
            filters,
            "COALESCE(atl.api_key_id, rl.api_key_id)",
        );
        Self::push_alert_projection_source_selection(
            query,
            selected_source,
            ALERT_SOURCE_AUTH_TOKEN_LOG,
            "CAST(atl.id AS TEXT)",
        );

        query.push(
            r#"
            UNION ALL
            SELECT
            "#,
        );
        query.push_bind(ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD);
        query.push(format!(
            r#" AS source_kind,
                m.id AS source_id,
                printf('maint:%s', m.id) AS row_sort_id,
                'upstream_key_blocked' AS alert_type,
                m.created_at AS occurred_at,
                COALESCE(m.auth_token_id, atl.token_id) AS token_id,
                m.key_id AS key_id,
                COALESCE(m.request_log_id, atl.request_log_id) AS request_log_id,
                COALESCE(NULLIF(TRIM(atl.method), ''), rl.method) AS method,
                COALESCE(NULLIF(TRIM(atl.path), ''), rl.path) AS path,
                COALESCE(atl.query, rl.query) AS query,
                {maintenance_request_kind_sql} AS request_kind_key,
                {maintenance_request_kind_label_sql} AS request_kind_label,
                atl.request_kind_detail AS request_kind_detail,
                atl.result_status AS result_status,
                atl.failure_kind AS failure_kind,
                atl.error_message AS error_message,
                NULL AS counts_business_quota,
                u.id AS user_id,
                u.display_name AS user_display_name,
                u.username AS user_username,
                m.reason_code AS reason_code,
                m.reason_summary AS reason_summary,
                m.reason_detail AS reason_detail,
                NULL AS job_id,
                NULL AS job_type,
                NULL AS job_trigger_source,
                NULL AS job_status,
                NULL AS job_attempt,
                NULL AS job_message,
                NULL AS job_queued_at,
                NULL AS job_started_at,
                NULL AS job_finished_at
            FROM api_key_maintenance_records m
            LEFT JOIN auth_token_logs atl ON atl.id = m.auth_token_log_id
            LEFT JOIN observability.request_logs rl
              ON rl.id = COALESCE(m.request_log_id, atl.request_log_id)
             AND (
                    atl.id IS NULL
                 OR atl.method IS NULL
                 OR atl.method = ''
                 OR atl.path IS NULL
                 OR atl.path = ''
                 OR atl.request_kind_key IS NULL
                 OR atl.request_kind_key = ''
                 OR atl.request_kind_label IS NULL
                 OR atl.request_kind_label = ''
                 OR atl.query IS NULL
                )
            LEFT JOIN user_token_bindings b ON b.token_id = COALESCE(m.auth_token_id, atl.token_id)
            LEFT JOIN users u ON u.id = b.user_id
            WHERE COALESCE(m.reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
            "#
        ));
        Self::push_maintenance_alert_filters(query, filters, ALERT_TYPE_UPSTREAM_KEY_BLOCKED);
        Self::push_alert_projection_source_selection(
            query,
            selected_source,
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD,
            "m.id",
        );
        query.push(
            r#"
            UNION ALL
            SELECT
            "#,
        );
        query.push_bind(ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD);
        query.push(format!(
            r#" AS source_kind,
                m.id AS source_id,
                printf('maint:%s', m.id) AS row_sort_id,
                'api_key_exhausted' AS alert_type,
                m.created_at AS occurred_at,
                COALESCE(m.auth_token_id, atl.token_id) AS token_id,
                m.key_id AS key_id,
                COALESCE(m.request_log_id, atl.request_log_id) AS request_log_id,
                COALESCE(NULLIF(TRIM(atl.method), ''), rl.method) AS method,
                COALESCE(NULLIF(TRIM(atl.path), ''), rl.path) AS path,
                COALESCE(atl.query, rl.query) AS query,
                {maintenance_request_kind_sql} AS request_kind_key,
                {maintenance_request_kind_label_sql} AS request_kind_label,
                atl.request_kind_detail AS request_kind_detail,
                atl.result_status AS result_status,
                atl.failure_kind AS failure_kind,
                atl.error_message AS error_message,
                NULL AS counts_business_quota,
                u.id AS user_id,
                u.display_name AS user_display_name,
                u.username AS user_username,
                m.reason_code AS reason_code,
                m.reason_summary AS reason_summary,
                m.reason_detail AS reason_detail,
                NULL AS job_id,
                NULL AS job_type,
                NULL AS job_trigger_source,
                NULL AS job_status,
                NULL AS job_attempt,
                NULL AS job_message,
                NULL AS job_queued_at,
                NULL AS job_started_at,
                NULL AS job_finished_at
            FROM api_key_maintenance_records m
            LEFT JOIN auth_token_logs atl ON atl.id = m.auth_token_log_id
            LEFT JOIN observability.request_logs rl
              ON rl.id = COALESCE(m.request_log_id, atl.request_log_id)
             AND (
                    atl.id IS NULL
                 OR atl.method IS NULL
                 OR atl.method = ''
                 OR atl.path IS NULL
                 OR atl.path = ''
                 OR atl.request_kind_key IS NULL
                 OR atl.request_kind_key = ''
                 OR atl.request_kind_label IS NULL
                 OR atl.request_kind_label = ''
                 OR atl.query IS NULL
                )
            LEFT JOIN user_token_bindings b ON b.token_id = COALESCE(m.auth_token_id, atl.token_id)
            LEFT JOIN users u ON u.id = b.user_id
            WHERE m.source = 'system'
              AND m.operation_code = 'auto_mark_exhausted'
              AND m.reason_code = 'quota_exhausted'
            "#
        ));
        Self::push_maintenance_alert_filters(query, filters, ALERT_TYPE_API_KEY_EXHAUSTED);
        Self::push_alert_projection_source_selection(
            query,
            selected_source,
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD,
            "m.id",
        );
        query.push(
            r#"
            UNION ALL
            SELECT
            "#,
        );
        query.push_bind(ALERT_SOURCE_SCHEDULED_JOB);
        query.push(
            r#" AS source_kind,
                CAST(j.id AS TEXT) AS source_id,
                printf('job:%020lld', j.id) AS row_sort_id,
                'job_failed' AS alert_type,
                COALESCE(j.finished_at, j.started_at, j.queued_at) AS occurred_at,
                NULL AS token_id,
                j.key_id AS key_id,
                NULL AS request_log_id,
                NULL AS method,
                NULL AS path,
                NULL AS query,
                NULL AS request_kind_key,
                NULL AS request_kind_label,
                NULL AS request_kind_detail,
                j.status AS result_status,
                'job_failed' AS failure_kind,
                j.message AS error_message,
                NULL AS counts_business_quota,
                NULL AS user_id,
                NULL AS user_display_name,
                NULL AS user_username,
                'job_failed' AS reason_code,
                j.message AS reason_summary,
                NULL AS reason_detail,
                j.id AS job_id,
                j.job_type AS job_type,
                j.trigger_source AS job_trigger_source,
                j.status AS job_status,
                j.attempt AS job_attempt,
                j.message AS job_message,
                j.queued_at AS job_queued_at,
                j.started_at AS job_started_at,
                j.finished_at AS job_finished_at
            FROM scheduled_jobs j
            WHERE LOWER(TRIM(j.status)) IN ('error', 'failed')
            "#,
        );
        Self::push_job_alert_filters(query, filters);
        Self::push_alert_projection_source_selection(
            query,
            selected_source,
            ALERT_SOURCE_SCHEDULED_JOB,
            "CAST(j.id AS TEXT)",
        );
        query.push(")");
    }

    // The read model stores the normalized source row as JSON. It deliberately
    // has the same column contract as the raw CTE above so callers can retain
    // grouping and pagination behavior while avoiding joins and scans of the
    // live log tables once the projection has complete coverage.
    fn push_projected_alert_events_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        Self::push_projected_alert_events_cte_from_relation(
            query,
            filters,
            "observability.dashboard_alert_projection_events",
        );
    }

    fn push_projected_alert_events_cte_from_relation<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        relation: &str,
    ) {
        query.push(
            r#"WITH alerts AS (
                SELECT source_kind,
                       source_id,
                       row_sort_id,
                       COALESCE(json_extract(payload_json, '$.alert_type'), '') AS alert_type,
                       occurred_at,
                       json_extract(payload_json, '$.token_id') AS token_id,
                       json_extract(payload_json, '$.key_id') AS key_id,
                       json_extract(payload_json, '$.request_log_id') AS request_log_id,
                       json_extract(payload_json, '$.method') AS method,
                       json_extract(payload_json, '$.path') AS path,
                       json_extract(payload_json, '$.query') AS query,
                       json_extract(payload_json, '$.request_kind_key') AS request_kind_key,
                       json_extract(payload_json, '$.request_kind_label') AS request_kind_label,
                       json_extract(payload_json, '$.request_kind_detail') AS request_kind_detail,
                       json_extract(payload_json, '$.result_status') AS result_status,
                       json_extract(payload_json, '$.failure_kind') AS failure_kind,
                       json_extract(payload_json, '$.error_message') AS error_message,
                       json_extract(payload_json, '$.counts_business_quota') AS counts_business_quota,
                       json_extract(payload_json, '$.user_id') AS user_id,
                       json_extract(payload_json, '$.user_display_name') AS user_display_name,
                       json_extract(payload_json, '$.user_username') AS user_username,
                       json_extract(payload_json, '$.reason_code') AS reason_code,
                       json_extract(payload_json, '$.reason_summary') AS reason_summary,
                       json_extract(payload_json, '$.reason_detail') AS reason_detail,
                       json_extract(payload_json, '$.job_id') AS job_id,
                       json_extract(payload_json, '$.job_type') AS job_type,
                       json_extract(payload_json, '$.job_trigger_source') AS job_trigger_source,
                       json_extract(payload_json, '$.job_status') AS job_status,
                       json_extract(payload_json, '$.job_attempt') AS job_attempt,
                       json_extract(payload_json, '$.job_message') AS job_message,
                       json_extract(payload_json, '$.job_queued_at') AS job_queued_at,
                       json_extract(payload_json, '$.job_started_at') AS job_started_at,
                       json_extract(payload_json, '$.job_finished_at') AS job_finished_at
                  FROM "#,
        );
        query.push(relation);
        query.push(
            r#"
                 WHERE 1 = 1"#,
        );
        if let Some(alert_type) = filters.alert_type {
            query
                .push(" AND json_extract(payload_json, '$.alert_type') = ")
                .push_bind(alert_type);
        }
        if let Some(since) = filters.since {
            query.push(" AND occurred_at >= ").push_bind(since);
        }
        if let Some(until) = filters.until {
            query.push(" AND occurred_at <= ").push_bind(until);
        }
        if let Some(user_id) = filters.user_id {
            query
                .push(" AND json_extract(payload_json, '$.user_id') = ")
                .push_bind(user_id);
        }
        if let Some(token_id) = filters.token_id {
            query
                .push(" AND json_extract(payload_json, '$.token_id') = ")
                .push_bind(token_id);
        }
        if let Some(key_id) = filters.key_id {
            query
                .push(" AND json_extract(payload_json, '$.key_id') = ")
                .push_bind(key_id);
        }
        query.push(")");
    }
}
