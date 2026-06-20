#[derive(Clone, Copy)]
struct AlertEventFilters<'a> {
    alert_type: Option<&'a str>,
    since: Option<i64>,
    until: Option<i64>,
    user_id: Option<&'a str>,
    token_id: Option<&'a str>,
    key_id: Option<&'a str>,
    request_kinds: &'a [String],
}

#[derive(Debug, Clone)]
struct AlertEventProjectionRow {
    source_kind: String,
    source_id: String,
    row_sort_id: String,
    alert_type: String,
    occurred_at: i64,
    token_id: Option<String>,
    key_id: Option<String>,
    request_log_id: Option<i64>,
    method: Option<String>,
    path: Option<String>,
    query: Option<String>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    result_status: Option<String>,
    failure_kind: Option<String>,
    error_message: Option<String>,
    counts_business_quota: Option<bool>,
    user_id: Option<String>,
    user_display_name: Option<String>,
    user_username: Option<String>,
    reason_code: Option<String>,
    reason_summary: Option<String>,
    reason_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct AlertGroupProjectionRow {
    row_sort_id: String,
    alert_type: String,
    subject_kind: String,
    subject_id: String,
    total_count: i64,
    first_seen: i64,
    last_seen: i64,
}

fn normalize_alert_request_kind_filters(request_kinds: &[String]) -> Vec<String> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for request_kind in request_kinds {
        let normalized = canonical_request_kind_key_for_filter(request_kind);
        if seen.insert(normalized.clone()) {
            deduped.push(normalized);
        }
    }
    deduped
}

fn build_alert_user_ref(
    user_id: Option<String>,
    display_name: Option<String>,
    username: Option<String>,
) -> Option<AlertUserRef> {
    user_id.map(|user_id| AlertUserRef {
        user_id,
        display_name,
        username,
    })
}

fn build_alert_entity_ref(id: Option<String>) -> Option<AlertEntityRef> {
    id.map(|id| AlertEntityRef {
        label: id.clone(),
        id,
    })
}

fn build_alert_request_kind(
    method: Option<&str>,
    path: Option<&str>,
    query: Option<&str>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
) -> Option<TokenRequestKind> {
    match (method, path) {
        (Some(method), Some(path)) => Some(finalize_token_request_kind(
            method,
            path,
            query,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
        )),
        _ => request_kind_key.map(|key| {
            let label = request_kind_label.unwrap_or_else(|| key.clone());
            TokenRequestKind::new(key, label, request_kind_detail)
        }),
    }
}

fn alert_user_label(user: &AlertUserRef) -> String {
    user.display_name
        .clone()
        .or_else(|| user.username.clone())
        .unwrap_or_else(|| user.user_id.clone())
}

fn alert_subject_tuple(
    alert_type: &str,
    user: Option<&AlertUserRef>,
    token: Option<&AlertEntityRef>,
    key: Option<&AlertEntityRef>,
) -> (String, String, String) {
    if alert_type == ALERT_TYPE_UPSTREAM_KEY_BLOCKED
        && let Some(key) = key
    {
        return (
            ALERT_SUBJECT_KEY.to_string(),
            key.id.clone(),
            key.label.clone(),
        );
    }

    if let Some(user) = user {
        return (
            ALERT_SUBJECT_USER.to_string(),
            user.user_id.clone(),
            alert_user_label(user),
        );
    }

    if let Some(token) = token {
        return (
            ALERT_SUBJECT_TOKEN.to_string(),
            token.id.clone(),
            token.label.clone(),
        );
    }

    if let Some(key) = key {
        return (
            ALERT_SUBJECT_KEY.to_string(),
            key.id.clone(),
            key.label.clone(),
        );
    }

    (
        ALERT_SUBJECT_TOKEN.to_string(),
        "unknown".to_string(),
        "Unknown".to_string(),
    )
}

fn build_alert_title_and_summary(
    alert_type: &str,
    subject_label: &str,
    token: Option<&AlertEntityRef>,
    key: Option<&AlertEntityRef>,
    request_kind: Option<&TokenRequestKind>,
    reason_summary: Option<&str>,
) -> (String, String) {
    let token_label = token.map(|value| value.label.as_str()).unwrap_or("unknown");
    let key_label = key.map(|value| value.label.as_str()).unwrap_or("unknown");
    let request_kind_label = request_kind
        .map(|value| value.label.as_str())
        .unwrap_or("Unknown request");
    let reason_suffix = reason_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(" Reason: {value}."))
        .unwrap_or_default();

    match alert_type {
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429 => (
            format!("{subject_label} hit upstream 429"),
            format!(
                "Token {token_label} received an upstream 429 response for {request_kind_label}."
            ),
        ),
        ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432 => (
            format!("{subject_label} hit Tavily usage limit"),
            format!(
                "Token {token_label} received Tavily usage-limit 432 for {request_kind_label} via key {key_label}."
            ),
        ),
        ALERT_TYPE_UPSTREAM_KEY_BLOCKED => (
            format!("Upstream key {key_label} was blocked"),
            format!("Maintenance evidence marked key {key_label} as blocked.{reason_suffix}"),
        ),
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED => (
            format!("{subject_label} hit the local request-rate limit"),
            format!(
                "Token {token_label} was rate limited by the local rolling window for {request_kind_label}."
            ),
        ),
        ALERT_TYPE_USER_QUOTA_EXHAUSTED => (
            format!("{subject_label} exhausted business quota"),
            format!(
                "Token {token_label} exhausted the business quota allowance for {request_kind_label}."
            ),
        ),
        _ => (
            format!("{subject_label} emitted an alert"),
            "Alert details are available in the related request and source records.".to_string(),
        ),
    }
}

fn alert_group_id(event: &AlertEventRecord) -> String {
    let request_kind_key = event
        .request_kind
        .as_ref()
        .map(|value| value.key.as_str())
        .unwrap_or("unknown");
    format!(
        "{}:{}:{}:{}",
        event.alert_type, event.subject_kind, event.subject_id, request_kind_key
    )
}

impl KeyStore {
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
                        " AND atl.result_status = 'quota_exhausted' AND rl.tavily_status_code = 432",
                    );
                }
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED => {
                    query.push(
                        " AND atl.result_status = 'quota_exhausted' AND atl.counts_business_quota = 0",
                    );
                }
                ALERT_TYPE_USER_QUOTA_EXHAUSTED => {
                    query.push(
                        " AND atl.result_status = 'quota_exhausted' AND COALESCE(rl.tavily_status_code, 0) <> 432 AND atl.counts_business_quota <> 0",
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
    ) {
        if let Some(alert_type) = filters.alert_type
            && alert_type != ALERT_TYPE_UPSTREAM_KEY_BLOCKED
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

    fn push_alert_events_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        let stored_request_kind_sql = "atl.request_kind_key";
        let effective_request_kind_sql = request_log_request_kind_key_sql(
            "COALESCE(rl.path, atl.path)",
            "rl.request_body",
            stored_request_kind_sql,
        );
        let effective_request_kind_label_sql =
            canonical_request_kind_label_sql(&effective_request_kind_sql);
        let maintenance_request_kind_sql = request_log_request_kind_key_sql(
            "COALESCE(atl.path, rl.path)",
            "rl.request_body",
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
                    WHEN atl.result_status = 'quota_exhausted' AND rl.tavily_status_code = 432 THEN 'upstream_usage_limit_432'
                    WHEN atl.result_status = 'quota_exhausted' AND atl.counts_business_quota = 0 THEN 'user_request_rate_limited'
                    WHEN atl.result_status = 'quota_exhausted' THEN 'user_quota_exhausted'
                    ELSE ''
                END AS alert_type,
                atl.created_at AS occurred_at,
                atl.token_id AS token_id,
                COALESCE(atl.api_key_id, rl.api_key_id) AS key_id,
                atl.request_log_id AS request_log_id,
                atl.method AS method,
                atl.path AS path,
                atl.query AS query,
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
                NULL AS reason_detail
            FROM auth_token_logs atl
            LEFT JOIN observability.request_logs rl ON rl.id = atl.request_log_id
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
                COALESCE(atl.method, rl.method) AS method,
                COALESCE(atl.path, rl.path) AS path,
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
                m.reason_detail AS reason_detail
            FROM api_key_maintenance_records m
            LEFT JOIN auth_token_logs atl ON atl.id = m.auth_token_log_id
            LEFT JOIN observability.request_logs rl ON rl.id = COALESCE(m.request_log_id, atl.request_log_id)
            LEFT JOIN user_token_bindings b ON b.token_id = COALESCE(m.auth_token_id, atl.token_id)
            LEFT JOIN users u ON u.id = b.user_id
            WHERE COALESCE(m.reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
            "#
        ));
        Self::push_maintenance_alert_filters(query, filters);
        query.push(")");
    }

    fn alert_subject_kind_sql(alias: &str) -> String {
        format!(
            "CASE \
                WHEN {alias}.alert_type = 'upstream_key_blocked' AND {alias}.key_id IS NOT NULL THEN 'key' \
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
                WHEN {alias}.alert_type = 'upstream_key_blocked' AND {alias}.key_id IS NOT NULL THEN {alias}.key_id \
                WHEN {alias}.user_id IS NOT NULL THEN {alias}.user_id \
                WHEN {alias}.token_id IS NOT NULL THEN {alias}.token_id \
                WHEN {alias}.key_id IS NOT NULL THEN {alias}.key_id \
                ELSE 'unknown' \
            END"
        )
    }

    fn alert_group_request_kind_sql(alias: &str) -> String {
        format!("COALESCE(NULLIF(TRIM({alias}.request_kind_key), ''), 'unknown')")
    }

    fn push_alert_groups_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        let subject_kind_sql = Self::alert_subject_kind_sql("alerts");
        let subject_id_sql = Self::alert_subject_id_sql("alerts");
        let request_kind_sql = Self::alert_group_request_kind_sql("alerts");
        Self::push_alert_events_cte(query, filters);
        query.push(format!(
            r#",
            grouped_alerts AS (
                SELECT
                    alerts.row_sort_id AS row_sort_id,
                    alerts.alert_type AS alert_type,
                    {subject_kind_sql} AS subject_kind,
                    {subject_id_sql} AS subject_id,
                    {request_kind_sql} AS request_kind_key,
                    COUNT(*) OVER (
                        PARTITION BY alerts.alert_type, {subject_kind_sql}, {subject_id_sql}, {request_kind_sql}
                    ) AS total_count,
                    MIN(alerts.occurred_at) OVER (
                        PARTITION BY alerts.alert_type, {subject_kind_sql}, {subject_id_sql}, {request_kind_sql}
                    ) AS first_seen,
                    MAX(alerts.occurred_at) OVER (
                        PARTITION BY alerts.alert_type, {subject_kind_sql}, {subject_id_sql}, {request_kind_sql}
                    ) AS last_seen,
                    ROW_NUMBER() OVER (
                        PARTITION BY alerts.alert_type, {subject_kind_sql}, {subject_id_sql}, {request_kind_sql}
                        ORDER BY alerts.occurred_at DESC, alerts.row_sort_id DESC
                    ) AS group_rank
                FROM alerts
            )"#
        ));
    }

    fn decode_alert_group_projection_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<AlertGroupProjectionRow, sqlx::Error> {
        Ok(AlertGroupProjectionRow {
            row_sort_id: row.try_get("row_sort_id")?,
            alert_type: row.try_get("alert_type")?,
            subject_kind: row.try_get("subject_kind")?,
            subject_id: row.try_get("subject_id")?,
            total_count: row.try_get("total_count")?,
            first_seen: row.try_get("first_seen")?,
            last_seen: row.try_get("last_seen")?,
        })
    }

    fn summarize_alert_type_count_rows(rows: Vec<sqlx::sqlite::SqliteRow>) -> Vec<AlertTypeCount> {
        let mut counts = default_alert_type_counts();
        let mut index_by_type = HashMap::new();
        for (index, item) in counts.iter().enumerate() {
            index_by_type.insert(item.alert_type.clone(), index);
        }
        for row in rows {
            let Ok(alert_type) = row.try_get::<String, _>("alert_type") else {
                continue;
            };
            let Ok(count) = row.try_get::<i64, _>("count") else {
                continue;
            };
            if let Some(index) = index_by_type.get(&alert_type).copied() {
                counts[index].count = count;
            }
        }
        counts
    }

    async fn fetch_alert_event_projection_page(
        &self,
        filters: AlertEventFilters<'_>,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedAlertEvents, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let mut count_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut count_query, filters);
        count_query.push(" SELECT COUNT(*) FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter(
            &mut count_query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        let total: i64 = count_query.build_query_scalar().fetch_one(&self.pool).await?;

        let mut query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut query, filters);
        query.push(" SELECT * FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter(
            &mut query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        query.push(" ORDER BY occurred_at DESC, row_sort_id DESC LIMIT ");
        query.push_bind(per_page);
        query.push(" OFFSET ");
        query.push_bind(offset);

        let rows = query.build().fetch_all(&self.pool).await?;
        let items = rows
            .into_iter()
            .map(Self::decode_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect::<Vec<_>>();

        Ok(PaginatedAlertEvents {
            items,
            total,
            page,
            per_page,
        })
    }

    async fn fetch_group_latest_events_by_projection(
        &self,
        filters: AlertEventFilters<'_>,
        groups: &[AlertGroupProjectionRow],
    ) -> Result<HashMap<String, AlertEventRecord>, ProxyError> {
        if groups.is_empty() {
            return Ok(HashMap::new());
        }

        let mut query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut query, filters);
        query.push(" SELECT * FROM alerts WHERE row_sort_id IN (");
        {
            let mut separated = query.separated(", ");
            for group in groups {
                separated.push_bind(group.row_sort_id.as_str());
            }
        }
        query.push(")");

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut events_by_row_sort_id = HashMap::new();
        for row in rows {
            let decoded = Self::decode_alert_event_projection_row(row)?;
            let row_sort_id = decoded.row_sort_id.clone();
            if let Some(event) = Self::build_alert_event_from_projection(decoded) {
                events_by_row_sort_id.insert(row_sort_id, event);
            }
        }
        Ok(events_by_row_sort_id)
    }

    fn build_alert_group_records(
        groups: Vec<AlertGroupProjectionRow>,
        latest_events_by_row_sort_id: HashMap<String, AlertEventRecord>,
    ) -> Vec<AlertGroupRecord> {
        groups
            .into_iter()
            .filter_map(|group| {
                let latest_event = latest_events_by_row_sort_id.get(&group.row_sort_id)?.clone();
                Some(AlertGroupRecord {
                    id: alert_group_id(&latest_event),
                    alert_type: group.alert_type,
                    subject_kind: group.subject_kind,
                    subject_id: group.subject_id,
                    subject_label: latest_event.subject_label.clone(),
                    user: latest_event.user.clone(),
                    token: latest_event.token.clone(),
                    key: latest_event.key.clone(),
                    request_kind: latest_event.request_kind.clone(),
                    count: group.total_count,
                    first_seen: group.first_seen,
                    last_seen: group.last_seen,
                    latest_event,
                })
            })
            .collect()
    }

    async fn fetch_alert_group_projection_page(
        &self,
        filters: AlertEventFilters<'_>,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;

        let mut count_query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut count_query, filters);
        count_query.push(" SELECT COUNT(*) FROM grouped_alerts WHERE group_rank = 1");
        Self::push_alert_request_kind_filter(
            &mut count_query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        let total: i64 = count_query.build_query_scalar().fetch_one(&self.pool).await?;

        let mut query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut query, filters);
        query.push(" SELECT * FROM grouped_alerts WHERE group_rank = 1");
        Self::push_alert_request_kind_filter(
            &mut query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        query.push(
            " ORDER BY last_seen DESC, total_count DESC, alert_type DESC, row_sort_id DESC LIMIT ",
        );
        query.push_bind(per_page);
        query.push(" OFFSET ");
        query.push_bind(offset);

        let rows = query.build().fetch_all(&self.pool).await?;
        let groups = rows
            .iter()
            .map(Self::decode_alert_group_projection_row)
            .collect::<Result<Vec<_>, _>>()?;
        let latest_events_by_row_sort_id =
            self.fetch_group_latest_events_by_projection(filters, &groups).await?;
        let items = Self::build_alert_group_records(groups, latest_events_by_row_sort_id);

        Ok(PaginatedAlertGroups {
            items,
            total,
            page,
            per_page,
        })
    }

    fn decode_alert_event_projection_row(
        row: sqlx::sqlite::SqliteRow,
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
        })
    }

    fn build_alert_event_from_projection(row: AlertEventProjectionRow) -> Option<AlertEventRecord> {
        let AlertEventProjectionRow {
            source_kind,
            source_id,
            row_sort_id: _,
            alert_type,
            occurred_at,
            token_id,
            key_id,
            request_log_id,
            method,
            path,
            query,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            result_status,
            failure_kind,
            error_message,
            counts_business_quota,
            user_id,
            user_display_name,
            user_username,
            reason_code,
            reason_summary,
            reason_detail,
        } = row;

        let resolved_alert_type = if !alert_type.trim().is_empty() {
            alert_type
        } else if failure_kind.as_deref() == Some(ALERT_TYPE_UPSTREAM_RATE_LIMITED_429) {
            ALERT_TYPE_UPSTREAM_RATE_LIMITED_429.to_string()
        } else if result_status.as_deref() == Some("quota_exhausted")
            && counts_business_quota == Some(false)
        {
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED.to_string()
        } else if result_status.as_deref() == Some("quota_exhausted") {
            ALERT_TYPE_USER_QUOTA_EXHAUSTED.to_string()
        } else {
            return None;
        };

        let user = build_alert_user_ref(user_id, user_display_name, user_username);
        let token = build_alert_entity_ref(token_id);
        let key = build_alert_entity_ref(key_id);
        let request_kind = build_alert_request_kind(
            method.as_deref(),
            path.as_deref(),
            query.as_deref(),
            request_kind_key,
            request_kind_label,
            request_kind_detail,
        );
        let request = request_log_id.map(|request_log_id| AlertRequestRef {
            id: request_log_id,
            method: method.clone().unwrap_or_else(|| "POST".to_string()),
            path: path.clone().unwrap_or_else(|| "/unknown".to_string()),
            query: query.clone(),
        });
        let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
            resolved_alert_type.as_str(),
            user.as_ref(),
            token.as_ref(),
            key.as_ref(),
        );
        let (title, summary) = build_alert_title_and_summary(
            resolved_alert_type.as_str(),
            subject_label.as_str(),
            token.as_ref(),
            key.as_ref(),
            request_kind.as_ref(),
            reason_summary.as_deref(),
        );

        Some(AlertEventRecord {
            id: format!("{source_kind}:{source_id}"),
            alert_type: resolved_alert_type,
            title,
            summary,
            occurred_at,
            subject_kind,
            subject_id,
            subject_label,
            user,
            token,
            key,
            request,
            request_kind,
            failure_kind,
            result_status,
            error_message,
            reason_code,
            reason_summary,
            reason_detail,
            source: AlertSourceRef {
                kind: source_kind,
                id: source_id,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_alert_events_page(
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
        self.fetch_alert_event_projection_page(
            AlertEventFilters {
                alert_type,
                since,
                until,
                user_id,
                token_id,
                key_id,
                request_kinds,
            },
            page,
            per_page,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_alert_groups_page(
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
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        self.fetch_alert_group_projection_page(
            AlertEventFilters {
                alert_type,
                since,
                until,
                user_id,
                token_id,
                key_id,
                request_kinds,
            },
            page,
            per_page,
        )
        .await
    }

    pub(crate) async fn fetch_alert_catalog(&self) -> Result<AlertCatalog, ProxyError> {
        let filters = AlertEventFilters {
            alert_type: None,
            since: None,
            until: None,
            user_id: None,
            token_id: None,
            key_id: None,
            request_kinds: &[],
        };
        let mut request_kind_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut request_kind_query, filters);
        request_kind_query.push(
            " SELECT \
                request_kind_key, \
                MIN(request_kind_label) AS request_kind_label, \
                COUNT(*) AS count \
              FROM alerts \
              WHERE COALESCE(NULLIF(TRIM(request_kind_key), ''), '') <> '' \
                AND COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown') <> 'unknown' \
              GROUP BY request_kind_key \
              ORDER BY count DESC, request_kind_label ASC, request_kind_key ASC",
        );
        let request_kind_rows = request_kind_query.build().fetch_all(&self.pool).await?;
        let request_kind_options = request_kind_rows
            .into_iter()
            .map(|row| -> Result<TokenRequestKindOption, sqlx::Error> {
                let key: String = row.try_get("request_kind_key")?;
                let label = row
                    .try_get::<Option<String>, _>("request_kind_label")?
                    .unwrap_or_else(|| key.clone());
                let count: i64 = row.try_get("count")?;
                Ok(TokenRequestKindOption {
                    key: key.clone(),
                    label,
                    protocol_group: token_request_kind_protocol_group(&key).to_string(),
                    billing_group: token_request_kind_billing_group(&key).to_string(),
                    count,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut users_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut users_query, filters);
        users_query.push(
            " SELECT \
                user_id AS value, \
                COALESCE(NULLIF(TRIM(user_display_name), ''), NULLIF(TRIM(user_username), ''), user_id) AS label, \
                COUNT(*) AS count \
              FROM alerts \
              WHERE user_id IS NOT NULL \
              GROUP BY user_id, label \
              ORDER BY count DESC, label ASC, value ASC",
        );
        let users = users_query
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| -> Result<AlertFacetOption, sqlx::Error> {
                Ok(AlertFacetOption {
                    value: row.try_get("value")?,
                    label: row.try_get("label")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut tokens_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut tokens_query, filters);
        tokens_query.push(
            " SELECT \
                token_id AS value, \
                token_id AS label, \
                COUNT(*) AS count \
              FROM alerts \
              WHERE token_id IS NOT NULL \
              GROUP BY token_id \
              ORDER BY count DESC, label ASC, value ASC",
        );
        let tokens = tokens_query
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| -> Result<AlertFacetOption, sqlx::Error> {
                Ok(AlertFacetOption {
                    value: row.try_get("value")?,
                    label: row.try_get("label")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut keys_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut keys_query, filters);
        keys_query.push(
            " SELECT \
                key_id AS value, \
                key_id AS label, \
                COUNT(*) AS count \
              FROM alerts \
              WHERE key_id IS NOT NULL \
              GROUP BY key_id \
              ORDER BY count DESC, label ASC, value ASC",
        );
        let keys = keys_query
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| -> Result<AlertFacetOption, sqlx::Error> {
                Ok(AlertFacetOption {
                    value: row.try_get("value")?,
                    label: row.try_get("label")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut types_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut types_query, filters);
        types_query.push(
            " SELECT alert_type, COUNT(*) AS count \
              FROM alerts \
              WHERE COALESCE(NULLIF(TRIM(alert_type), ''), '') <> '' \
              GROUP BY alert_type",
        );
        let types = Self::summarize_alert_type_count_rows(
            types_query.build().fetch_all(&self.pool).await?,
        )
        .into_iter()
        .map(|value| LogFacetOption {
            value: value.alert_type,
            count: value.count,
        })
        .collect::<Vec<_>>();

        Ok(AlertCatalog {
            retention_days: self.effective_auth_token_log_retention_days().await?,
            types,
            request_kind_options,
            users,
            tokens,
            keys,
        })
    }

    pub(crate) async fn fetch_recent_alerts_summary(
        &self,
        window_hours: i64,
    ) -> Result<RecentAlertsSummary, ProxyError> {
        let clamped_window_hours = window_hours.clamp(1, 24 * 30);
        let since = self
            .backend_time
            .now_ts()
            .saturating_sub(clamped_window_hours.saturating_mul(3600));
        let filters = AlertEventFilters {
            alert_type: None,
            since: Some(since),
            until: None,
            user_id: None,
            token_id: None,
            key_id: None,
            request_kinds: &[],
        };
        let mut summary_query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut summary_query, filters);
        summary_query.push(
            " SELECT \
                (SELECT COUNT(*) FROM alerts) AS total_events, \
                (SELECT COUNT(*) FROM grouped_alerts WHERE group_rank = 1) AS grouped_count",
        );
        let summary_row = summary_query.build().fetch_one(&self.pool).await?;
        let total_events: i64 = summary_row.try_get("total_events")?;
        let grouped_count: i64 = summary_row.try_get("grouped_count")?;

        let mut counts_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut counts_query, filters);
        counts_query.push(
            " SELECT alert_type, COUNT(*) AS count \
              FROM alerts \
              WHERE COALESCE(NULLIF(TRIM(alert_type), ''), '') <> '' \
              GROUP BY alert_type",
        );
        let counts_by_type = Self::summarize_alert_type_count_rows(
            counts_query.build().fetch_all(&self.pool).await?,
        );

        let top_groups = self.fetch_alert_group_projection_page(filters, 1, 5).await?.items;
        Ok(RecentAlertsSummary {
            window_hours: clamped_window_hours,
            total_events,
            grouped_count,
            counts_by_type,
            top_groups,
        })
    }
}
