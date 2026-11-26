#[allow(dead_code)]
enum AlertSql<'a> {
    Sqlite(QueryBuilder<'a, Sqlite>),
    Postgres(QueryBuilder<'a, sqlx::Postgres>),
}


/// Rewrites the SQLite spellings used by the alert CTE text into their PostgreSQL equivalents.
///
/// Only whole-token constructs are replaced, and the replacements are idempotent, so a fragment
/// may be pushed more than once without changing the emitted SQL.
/// Rewrites the SQLite spellings used by the alert CTE text into their PostgreSQL equivalents.
///
/// Only whole constructs are replaced and every replacement is idempotent: already-qualified
/// `core.` relations are left intact because the pattern requires the bare name.
fn postgres_alert_sql(sql: &str) -> String {
    sql.replace(
        "json_extract(payload_json, '$.alert_type')",
        "payload_json ->> 'alert_type'",
    )
    .replace(
        "json_extract(payload_json, '$.occurred_at')",
        "(payload_json ->> 'occurred_at')::bigint",
    )
    .replace("NULL AS job_attempt", "CAST(NULL AS bigint) AS job_attempt")
    .replace("NULL AS job_queued_at", "CAST(NULL AS bigint) AS job_queued_at")
    .replace("NULL AS job_started_at", "CAST(NULL AS bigint) AS job_started_at")
    .replace("NULL AS job_finished_at", "CAST(NULL AS bigint) AS job_finished_at")
    .replace("NULL AS request_log_id", "CAST(NULL AS bigint) AS request_log_id")
    .replace("NULL AS counts_business_quota", "CAST(NULL AS bigint) AS counts_business_quota")
    .replace(
        "printf('atl:%020lld', atl.id)",
        "('atl:' || lpad(CAST(atl.id AS text), 20, '0'))",
    )
    .replace(
        "printf('job:%020lld', j.id)",
        "('job:' || lpad(CAST(j.id AS text), 20, '0'))",
    )
    .replace("printf('maint:%s', m.id)", "('maint:' || CAST(m.id AS text))")
    .replace(
        "printf('maint:%s', atl.api_key_id)",
        "('maint:' || CAST(atl.api_key_id AS text))",
    )
    .replace("CAST(atl.id AS TEXT)", "CAST(atl.id AS text)")
    .replace("CAST(m.id AS TEXT)", "CAST(m.id AS text)")
    .replace("CAST(j.id AS TEXT)", "CAST(j.id AS text)")
        .replace("FROM api_key_maintenance_records", "FROM core.api_key_maintenance_records")
        .replace("FROM api_key_transient_backoffs", "FROM core.api_key_transient_backoffs")
        .replace("FROM auth_token_logs", "FROM core.auth_token_logs")
        .replace("FROM request_logs", "FROM core.request_logs")
        .replace("FROM scheduled_jobs", "FROM core.scheduled_jobs")
        .replace("FROM upstream_reconciliation_usage", "FROM core.upstream_reconciliation_usage")
        .replace("FROM user_token_bindings", "FROM core.user_token_bindings")
        .replace("FROM users", "FROM core.users")
        .replace("FROM meta", "FROM core.meta")
        .replace("JOIN api_key_maintenance_records", "JOIN core.api_key_maintenance_records")
        .replace("JOIN api_key_transient_backoffs", "JOIN core.api_key_transient_backoffs")
        .replace("JOIN auth_token_logs", "JOIN core.auth_token_logs")
        .replace("JOIN request_logs", "JOIN core.request_logs")
    .replace("core.observability.request_logs", "observability.request_logs")
        .replace("JOIN scheduled_jobs", "JOIN core.scheduled_jobs")
        .replace("JOIN upstream_reconciliation_usage", "JOIN core.upstream_reconciliation_usage")
        .replace("JOIN user_token_bindings", "JOIN core.user_token_bindings")
        .replace("JOIN users", "JOIN core.users")
    .replace("LEFT JOIN user_token_bindings", "LEFT JOIN core.user_token_bindings")
    .replace("LEFT JOIN users", "LEFT JOIN core.users")
    .replace("LEFT JOIN auth_token_logs", "LEFT JOIN core.auth_token_logs")
    .replace("LEFT JOIN api_key_maintenance_records", "LEFT JOIN core.api_key_maintenance_records")
    .replace("LEFT JOIN scheduled_jobs", "LEFT JOIN core.scheduled_jobs")
        .replace("JOIN meta", "JOIN core.meta")
}

/// Rewrites `json_extract(payload_json, '$.x')` into the PostgreSQL extraction, typing the
/// members the projection decoder reads as integers. A regex pass avoids depending on how the
/// surrounding statement is split across `push` calls.
fn rewrite_json_members(sql: &str) -> String {
    const INT_MEMBERS: [&str; 7] = [
        "occurred_at",
        "request_log_id",
        "counts_business_quota",
        "job_attempt",
        "job_queued_at",
        "job_started_at",
        "job_finished_at",
    ];
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(start) = rest.find("json_extract(payload_json, '$.") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "json_extract(payload_json, '$.".len()..];
        let end = after.find('\'').expect("json member is quoted");
        let member = &after[..end];
        let consumed = start + "json_extract(payload_json, '$.".len() + end + 1;
        if INT_MEMBERS.contains(&member) {
            out.push_str(&format!("(payload_json ->> '{member}')::bigint"));
        } else {
            out.push_str(&format!("(payload_json ->> '{member}')"));
        }
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    out
}

#[allow(dead_code)]
impl<'a> AlertSql<'a> {
    fn new(dialect: SqlDialect) -> Self {
        match dialect {
            SqlDialect::Sqlite => Self::Sqlite(QueryBuilder::new("")),
            SqlDialect::Postgres => Self::Postgres(QueryBuilder::new("")),
        }
    }

    fn push(&mut self, sql: impl std::fmt::Display) -> &mut Self {
        match self {
            Self::Sqlite(query) => {
                query.push(sql);
            }
            Self::Postgres(query) => {
                // The alert CTE text is written in SQLite's dialect because that is the historical
                // implementation. Rewrite the constructs PostgreSQL spells differently here, once,
                // instead of at every call site.
                query.push(rewrite_json_members(&postgres_alert_sql(&sql.to_string())));
            }
        }
        self
    }

    fn push_bind<T>(&mut self, value: T) -> &mut Self
    where
        T: 'a
            + Send
            + Sync
            + sqlx::Encode<'a, Sqlite>
            + sqlx::Type<Sqlite>
            + sqlx::Encode<'a, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
    {
        match self {
            Self::Sqlite(query) => {
                query.push_bind(value);
            }
            Self::Postgres(query) => {
                query.push_bind(value);
            }
        }
        self
    }

    /// Separator helper for `IN (...)` lists; the dialect-erased port of `QueryBuilder::separated`.
    fn separated(&mut self, separator: &'static str) -> Separated<'_, 'a> {
        Separated {
            query: self,
            separator,
            first: true,
        }
    }

    fn sql(&self) -> &str {
        match self {
            Self::Sqlite(query) => query.sql(),
            Self::Postgres(query) => query.sql(),
        }
    }

    /// Takes the PostgreSQL builder out of the dialect wrapper so the caller can execute it
    /// with the compiled statement type that `sqlx` expects.
    fn into_postgres(self) -> Option<QueryBuilder<'a, sqlx::Postgres>> {
        match self {
            Self::Postgres(query) => Some(query),
            Self::Sqlite(_) => None,
        }
    }

    /// SQLite counterpart of [`Self::into_postgres`].
    fn into_sqlite(self) -> Option<QueryBuilder<'a, Sqlite>> {
        match self {
            Self::Sqlite(query) => Some(query),
            Self::Postgres(_) => None,
        }
    }
}

/// Separator writer over [`AlertSql`], mirroring `sqlx::QueryBuilder::separated`.
pub(crate) struct Separated<'q, 'a> {
    query: &'q mut AlertSql<'a>,
    separator: &'static str,
    first: bool,
}

impl<'q, 'a> Separated<'q, 'a> {
    fn next(&mut self) {
        if self.first {
            self.first = false;
        } else {
            self.query.push(self.separator);
        }
    }

    pub(crate) fn push(&mut self, sql: impl std::fmt::Display) -> &mut Self {
        self.next();
        self.query.push(sql);
        self
    }

    pub(crate) fn push_unseparated(&mut self, sql: impl std::fmt::Display) -> &mut Self {
        self.query.push(sql);
        self
    }

    pub(crate) fn push_bind<T>(&mut self, value: T) -> &mut Self
    where
        T: 'a
            + Send
            + Sync
            + sqlx::Encode<'a, Sqlite>
            + sqlx::Type<Sqlite>
            + sqlx::Encode<'a, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
    {
        self.next();
        self.query.push_bind(value);
        self
    }

    pub(crate) fn push_bind_unseparated<T>(&mut self, value: T) -> &mut Self
    where
        T: 'a
            + Send
            + Sync
            + sqlx::Encode<'a, Sqlite>
            + sqlx::Type<Sqlite>
            + sqlx::Encode<'a, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
    {
        self.query.push_bind(value);
        self
    }
}

#[allow(dead_code)]
impl KeyStore {
    #[allow(dead_code)]
    fn push_alert_projection_source_selection_any<'a>(
        query: &mut AlertSql<'a>,
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
        for (i, source_id) in source_ids.iter().enumerate() {
            if i > 0 {
                query.push(", ");
            }
            query.push_bind(source_id);
        }
        query.push(")");
    }

    #[allow(dead_code)]
    fn push_alert_request_kind_filter_any(
        query: &mut AlertSql<'_>,
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
        for (i, request_kind) in normalized.into_iter().enumerate() {
            if i > 0 {
                query.push(", ");
            }
            query.push_bind(request_kind);
        }
        query.push(")");
    }

    #[allow(dead_code)]
    fn push_auth_alert_filters_any<'a>(
        query: &mut AlertSql<'a>,
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
            query
                .push(" AND ")
                .push(key_expr)
                .push(" = ")
                .push_bind(key_id);
        }
    }

    #[allow(dead_code)]
    fn push_maintenance_alert_filters_any<'a>(
        query: &mut AlertSql<'a>,
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

    #[allow(dead_code)]
    fn push_job_alert_filters_any<'a>(query: &mut AlertSql<'a>, filters: AlertEventFilters<'a>) {
        if let Some(alert_type) = filters.alert_type
            && alert_type != ALERT_TYPE_JOB_FAILED
        {
            query.push(" AND 1 = 0");
        }
        let occurred_at_expr = "COALESCE(j.finished_at, j.started_at, j.queued_at)";
        if let Some(since) = filters.since {
            query
                .push(" AND ")
                .push(occurred_at_expr)
                .push(" >= ")
                .push_bind(since);
        }
        if let Some(until) = filters.until {
            query
                .push(" AND ")
                .push(occurred_at_expr)
                .push(" <= ")
                .push_bind(until);
        }
        if filters.user_id.is_some() || filters.token_id.is_some() {
            query.push(" AND 1 = 0");
        }
        if let Some(key_id) = filters.key_id {
            query.push(" AND j.key_id = ").push_bind(key_id);
        }
    }

    #[allow(dead_code)]
    fn push_alert_events_cte_any<'a>(query: &mut AlertSql<'a>, filters: AlertEventFilters<'a>) {
        Self::push_alert_events_cte_for_selected_source_any(query, filters, None);
    }

    #[allow(dead_code)]
    fn push_alert_events_cte_for_projection_source_keys_any<'a>(
        query: &mut AlertSql<'a>,
        filters: AlertEventFilters<'a>,
        source_kind: &'a str,
        source_ids: &'a [String],
    ) {
        Self::push_alert_events_cte_for_selected_source_any(
            query,
            filters,
            Some((source_kind, source_ids)),
        );
    }

    #[allow(dead_code)]
    fn push_alert_events_cte_for_selected_source_any<'a>(
        query: &mut AlertSql<'a>,
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
        Self::push_auth_alert_filters_any(
            query,
            filters,
            "COALESCE(atl.api_key_id, rl.api_key_id)",
        );
        Self::push_alert_projection_source_selection_any(
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
                CAST(m.id AS TEXT) AS source_id,
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
        Self::push_maintenance_alert_filters_any(query, filters, ALERT_TYPE_UPSTREAM_KEY_BLOCKED);
        Self::push_alert_projection_source_selection_any(
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
                CAST(m.id AS TEXT) AS source_id,
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
        Self::push_maintenance_alert_filters_any(query, filters, ALERT_TYPE_API_KEY_EXHAUSTED);
        Self::push_alert_projection_source_selection_any(
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
                CAST(j.id AS TEXT) AS job_id,
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
        Self::push_job_alert_filters_any(query, filters);
        Self::push_alert_projection_source_selection_any(
            query,
            selected_source,
            ALERT_SOURCE_SCHEDULED_JOB,
            "CAST(j.id AS TEXT)",
        );
        query.push(")");
    }

    #[allow(dead_code)]
    fn push_projected_alert_events_cte_any<'a>(
        query: &mut AlertSql<'a>,
        filters: AlertEventFilters<'a>,
        dialect: SqlDialect,
    ) {
        Self::push_projected_alert_events_cte_from_relation_any(
            query,
            filters,
            "observability.dashboard_alert_projection_events",
            dialect,
        );
    }

    #[allow(dead_code)]
    fn push_projected_alert_events_cte_from_relation_any<'a>(
        query: &mut AlertSql<'a>,
        filters: AlertEventFilters<'a>,
        relation: &str,
        dialect: SqlDialect,
    ) {
        query.push(Self::alert_payload_projection_sql(
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
            dialect,
        ));
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
        for (i, source_id) in source_ids.iter().enumerate() {
            if i > 0 {
                query.push(", ");
            }
            query.push_bind(source_id);
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
        for (i, request_kind) in normalized.into_iter().enumerate() {
            if i > 0 {
                query.push(", ");
            }
            query.push_bind(request_kind);
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
            query
                .push(" AND ")
                .push(key_expr)
                .push(" = ")
                .push_bind(key_id);
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
            query
                .push(" AND ")
                .push(occurred_at_expr)
                .push(" >= ")
                .push_bind(since);
        }
        if let Some(until) = filters.until {
            query
                .push(" AND ")
                .push(occurred_at_expr)
                .push(" <= ")
                .push_bind(until);
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
        Self::push_auth_alert_filters(query, filters, "COALESCE(atl.api_key_id, rl.api_key_id)");
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
                CAST(m.id AS TEXT) AS source_id,
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
                CAST(m.id AS TEXT) AS source_id,
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
                CAST(j.id AS TEXT) AS job_id,
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
        dialect: SqlDialect,
    ) {
        Self::push_projected_alert_events_cte_from_relation(
            query,
            filters,
            "observability.dashboard_alert_projection_events",
            dialect,
        );
    }

    /// Rewrites the SQLite `json_extract(payload_json, '$.field')` projection for PostgreSQL.
    /// The payload column is text on both engines, so the PostgreSQL form needs an explicit
    /// `jsonb` cast; the SQLite statement itself stays untouched.
    pub(crate) fn alert_payload_projection_sql(sql: &str, dialect: SqlDialect) -> String {
        if dialect == SqlDialect::Sqlite {
            return sql.to_string();
        }
        let marker = "json_extract(payload_json, '$.";
        let mut out = String::with_capacity(sql.len());
        let mut rest = sql;
        while let Some(index) = rest.find(marker) {
            out.push_str(&rest[..index]);
            let after = &rest[index + marker.len()..];
            let Some(end) = after.find('\'') else {
                out.push_str(rest);
                return out;
            };
            let field = &after[..end];
            // The projection decoder reads these members as `i64`; `->>` always yields text, so
            // cast the integer members back to bigint.
            const INT_MEMBERS: [&str; 6] = [
                "occurred_at",
                "request_log_id",
                "job_attempt",
                "job_queued_at",
                "job_started_at",
                "job_finished_at",
            ];
            if field == "counts_business_quota" {
                // The payload stores this member as a JSON boolean; SQLite returns 1/0 while
                // PostgreSQL would reject the text `true` for a bigint column.
                out.push_str("(CASE WHEN (payload_json::jsonb ->> 'counts_business_quota') = 'true' THEN 1 ELSE 0 END)::bigint");
            } else if INT_MEMBERS.contains(&field) {
                out.push_str("((payload_json::jsonb ->> '");
                out.push_str(field);
                out.push_str("'))::bigint");
            } else {
                out.push_str("(payload_json::jsonb ->> '");
                out.push_str(field);
                out.push_str("')");
            }
            rest = &after[end + 1..];
            if let Some(tail) = rest.strip_prefix(')') {
                rest = tail;
            }
        }
        out.push_str(rest);
        out
    }

    fn push_projected_alert_events_cte_from_relation<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        relation: &str,
        dialect: SqlDialect,
    ) {
        query.push(Self::alert_payload_projection_sql(
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
            dialect,
        ));
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

#[cfg(test)]
mod alert_sql_tests {
    use super::{AlertSql, SqlDialect};

    #[test]
    fn emits_placeholders_for_each_dialect() {
        let mut postgres = AlertSql::new(SqlDialect::Postgres);
        postgres.push("SELECT ").push_bind(1_i64);
        assert_eq!(postgres.sql(), "SELECT $1");

        let mut sqlite = AlertSql::new(SqlDialect::Sqlite);
        sqlite.push("SELECT ").push_bind(1_i64);
        assert_eq!(sqlite.sql(), "SELECT ?");
    }
}
