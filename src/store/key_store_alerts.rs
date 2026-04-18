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
struct RawAuthTokenAlertRow {
    id: i64,
    token_id: String,
    key_id: Option<String>,
    request_log_id: Option<i64>,
    method: String,
    path: String,
    query: Option<String>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    result_status: String,
    failure_kind: Option<String>,
    error_message: Option<String>,
    counts_business_quota: bool,
    created_at: i64,
    user_id: Option<String>,
    user_display_name: Option<String>,
    user_username: Option<String>,
}

#[derive(Debug, Clone)]
struct RawMaintenanceAlertRow {
    id: String,
    key_id: String,
    token_id: Option<String>,
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
    created_at: i64,
    user_id: Option<String>,
    user_display_name: Option<String>,
    user_username: Option<String>,
    reason_code: Option<String>,
    reason_summary: Option<String>,
    reason_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct AlertGroupAccumulator {
    alert_type: String,
    subject_kind: String,
    subject_id: String,
    subject_label: String,
    user: Option<AlertUserRef>,
    token: Option<AlertEntityRef>,
    key: Option<AlertEntityRef>,
    request_kind: Option<TokenRequestKind>,
    count: i64,
    first_seen: i64,
    last_seen: i64,
    latest_event: AlertEventRecord,
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
    async fn fetch_alert_auth_token_events(
        &self,
        filters: AlertEventFilters<'_>,
    ) -> Result<Vec<AlertEventRecord>, ProxyError> {
        if filters.alert_type == Some(ALERT_TYPE_UPSTREAM_KEY_BLOCKED) {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::new(
            r#"
            SELECT
                atl.id,
                atl.token_id,
                atl.api_key_id,
                atl.request_log_id,
                atl.method,
                atl.path,
                atl.query,
                atl.request_kind_key,
                atl.request_kind_label,
                atl.request_kind_detail,
                atl.result_status,
                atl.failure_kind,
                atl.error_message,
                atl.counts_business_quota,
                atl.created_at,
                u.id AS user_id,
                u.display_name AS user_display_name,
                u.username AS user_username
            FROM auth_token_logs atl
            LEFT JOIN user_token_bindings b ON b.token_id = atl.token_id
            LEFT JOIN users u ON u.id = b.user_id
            WHERE (
                atl.failure_kind = 'upstream_rate_limited_429'
                OR atl.result_status = 'quota_exhausted'
            )
            "#,
        );

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
            query.push(" AND atl.api_key_id = ").push_bind(key_id);
        }
        query.push(" ORDER BY atl.created_at DESC, atl.id DESC");

        let rows = query.build().fetch_all(&self.pool).await?;
        let normalized_request_kinds = normalize_alert_request_kind_filters(filters.request_kinds);

        let events = rows
            .into_iter()
            .map(|row| {
                Ok(RawAuthTokenAlertRow {
                    id: row.try_get("id")?,
                    token_id: row.try_get("token_id")?,
                    key_id: row.try_get("api_key_id")?,
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
                    counts_business_quota: row.try_get::<i64, _>("counts_business_quota")? != 0,
                    created_at: row.try_get("created_at")?,
                    user_id: row.try_get("user_id")?,
                    user_display_name: row.try_get("user_display_name")?,
                    user_username: row.try_get("user_username")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?
            .into_iter()
            .filter_map(|row| {
                let alert_type =
                    if row.failure_kind.as_deref() == Some(ALERT_TYPE_UPSTREAM_RATE_LIMITED_429) {
                        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429.to_string()
                    } else if row.result_status == "quota_exhausted" && !row.counts_business_quota {
                        ALERT_TYPE_USER_REQUEST_RATE_LIMITED.to_string()
                    } else if row.result_status == "quota_exhausted" {
                        ALERT_TYPE_USER_QUOTA_EXHAUSTED.to_string()
                    } else {
                        return None;
                    };

                if let Some(expected_type) = filters.alert_type
                    && expected_type != alert_type
                {
                    return None;
                }

                let user =
                    build_alert_user_ref(row.user_id, row.user_display_name, row.user_username);
                let token = Some(AlertEntityRef {
                    id: row.token_id.clone(),
                    label: row.token_id.clone(),
                });
                let key = build_alert_entity_ref(row.key_id);
                let request_kind = build_alert_request_kind(
                    Some(row.method.as_str()),
                    Some(row.path.as_str()),
                    row.query.as_deref(),
                    row.request_kind_key,
                    row.request_kind_label,
                    row.request_kind_detail,
                );

                if !normalized_request_kinds.is_empty()
                    && request_kind
                        .as_ref()
                        .map(|value| !normalized_request_kinds.contains(&value.key))
                        .unwrap_or(true)
                {
                    return None;
                }

                let request = row.request_log_id.map(|request_log_id| AlertRequestRef {
                    id: request_log_id,
                    method: row.method.clone(),
                    path: row.path.clone(),
                    query: row.query.clone(),
                });
                let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
                    alert_type.as_str(),
                    user.as_ref(),
                    token.as_ref(),
                    key.as_ref(),
                );
                let (title, summary) = build_alert_title_and_summary(
                    alert_type.as_str(),
                    subject_label.as_str(),
                    token.as_ref(),
                    key.as_ref(),
                    request_kind.as_ref(),
                    None,
                );

                Some(AlertEventRecord {
                    id: format!("atl:{}", row.id),
                    alert_type,
                    title,
                    summary,
                    occurred_at: row.created_at,
                    subject_kind,
                    subject_id,
                    subject_label,
                    user,
                    token,
                    key,
                    request,
                    request_kind,
                    failure_kind: row.failure_kind,
                    result_status: Some(row.result_status),
                    error_message: row.error_message,
                    reason_code: None,
                    reason_summary: None,
                    reason_detail: None,
                    source: AlertSourceRef {
                        kind: ALERT_SOURCE_AUTH_TOKEN_LOG.to_string(),
                        id: row.id.to_string(),
                    },
                })
            })
            .collect::<Vec<_>>();
        Ok(events)
    }

    async fn fetch_alert_maintenance_events(
        &self,
        filters: AlertEventFilters<'_>,
    ) -> Result<Vec<AlertEventRecord>, ProxyError> {
        if let Some(alert_type) = filters.alert_type
            && alert_type != ALERT_TYPE_UPSTREAM_KEY_BLOCKED
        {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::new(
            r#"
            SELECT
                m.id,
                m.key_id,
                COALESCE(m.auth_token_id, atl.token_id) AS token_id,
                COALESCE(m.request_log_id, atl.request_log_id) AS request_log_id,
                COALESCE(atl.method, rl.method) AS method,
                COALESCE(atl.path, rl.path) AS path,
                COALESCE(atl.query, rl.query) AS query,
                atl.request_kind_key,
                atl.request_kind_label,
                atl.request_kind_detail,
                atl.result_status,
                atl.failure_kind,
                atl.error_message,
                m.created_at,
                u.id AS user_id,
                u.display_name AS user_display_name,
                u.username AS user_username,
                m.reason_code,
                m.reason_summary,
                m.reason_detail
            FROM api_key_maintenance_records m
            LEFT JOIN auth_token_logs atl ON atl.id = m.auth_token_log_id
            LEFT JOIN request_logs rl ON rl.id = COALESCE(m.request_log_id, atl.request_log_id)
            LEFT JOIN user_token_bindings b ON b.token_id = COALESCE(m.auth_token_id, atl.token_id)
            LEFT JOIN users u ON u.id = b.user_id
            WHERE COALESCE(m.reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
            "#,
        );

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
        query.push(" ORDER BY m.created_at DESC, m.id DESC");

        let rows = query.build().fetch_all(&self.pool).await?;
        let normalized_request_kinds = normalize_alert_request_kind_filters(filters.request_kinds);

        let events = rows
            .into_iter()
            .map(|row| {
                Ok(RawMaintenanceAlertRow {
                    id: row.try_get("id")?,
                    key_id: row.try_get("key_id")?,
                    token_id: row.try_get("token_id")?,
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
                    created_at: row.try_get("created_at")?,
                    user_id: row.try_get("user_id")?,
                    user_display_name: row.try_get("user_display_name")?,
                    user_username: row.try_get("user_username")?,
                    reason_code: row.try_get("reason_code")?,
                    reason_summary: row.try_get("reason_summary")?,
                    reason_detail: row.try_get("reason_detail")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?
            .into_iter()
            .filter_map(|row| {
                let request_kind = build_alert_request_kind(
                    row.method.as_deref(),
                    row.path.as_deref(),
                    row.query.as_deref(),
                    row.request_kind_key,
                    row.request_kind_label,
                    row.request_kind_detail,
                );

                if !normalized_request_kinds.is_empty()
                    && request_kind
                        .as_ref()
                        .map(|value| !normalized_request_kinds.contains(&value.key))
                        .unwrap_or(true)
                {
                    return None;
                }

                let user =
                    build_alert_user_ref(row.user_id, row.user_display_name, row.user_username);
                let token = build_alert_entity_ref(row.token_id);
                let key = Some(AlertEntityRef {
                    id: row.key_id.clone(),
                    label: row.key_id.clone(),
                });
                let request = row.request_log_id.map(|request_log_id| AlertRequestRef {
                    id: request_log_id,
                    method: row.method.clone().unwrap_or_else(|| "POST".to_string()),
                    path: row.path.clone().unwrap_or_else(|| "/unknown".to_string()),
                    query: row.query.clone(),
                });
                let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
                    ALERT_TYPE_UPSTREAM_KEY_BLOCKED,
                    user.as_ref(),
                    token.as_ref(),
                    key.as_ref(),
                );
                let (title, summary) = build_alert_title_and_summary(
                    ALERT_TYPE_UPSTREAM_KEY_BLOCKED,
                    subject_label.as_str(),
                    token.as_ref(),
                    key.as_ref(),
                    request_kind.as_ref(),
                    row.reason_summary.as_deref(),
                );

                Some(AlertEventRecord {
                    id: format!("maint:{}", row.id),
                    alert_type: ALERT_TYPE_UPSTREAM_KEY_BLOCKED.to_string(),
                    title,
                    summary,
                    occurred_at: row.created_at,
                    subject_kind,
                    subject_id,
                    subject_label,
                    user,
                    token,
                    key,
                    request,
                    request_kind,
                    failure_kind: row.failure_kind,
                    result_status: row.result_status,
                    error_message: row.error_message,
                    reason_code: row.reason_code,
                    reason_summary: row.reason_summary,
                    reason_detail: row.reason_detail,
                    source: AlertSourceRef {
                        kind: ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD.to_string(),
                        id: row.id,
                    },
                })
            })
            .collect::<Vec<_>>();
        Ok(events)
    }

    async fn fetch_alert_events_filtered(
        &self,
        filters: AlertEventFilters<'_>,
    ) -> Result<Vec<AlertEventRecord>, ProxyError> {
        let (auth_token_events, maintenance_events) = tokio::join!(
            self.fetch_alert_auth_token_events(filters),
            self.fetch_alert_maintenance_events(filters)
        );
        let mut events = Vec::new();
        events.extend(auth_token_events?);
        events.extend(maintenance_events?);
        events.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(events)
    }

    fn paginate_alert_events(
        events: Vec<AlertEventRecord>,
        page: i64,
        per_page: i64,
    ) -> PaginatedAlertEvents {
        let total = events.len() as i64;
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let start = ((page - 1) * per_page) as usize;
        let end = (start + per_page as usize).min(events.len());
        let items = if start >= events.len() {
            Vec::new()
        } else {
            events[start..end].to_vec()
        };
        PaginatedAlertEvents {
            items,
            total,
            page,
            per_page,
        }
    }

    fn group_alert_events(events: &[AlertEventRecord]) -> Vec<AlertGroupRecord> {
        let mut groups: HashMap<String, AlertGroupAccumulator> = HashMap::new();
        for event in events {
            let group_id = alert_group_id(event);
            let entry = groups
                .entry(group_id.clone())
                .or_insert_with(|| AlertGroupAccumulator {
                    alert_type: event.alert_type.clone(),
                    subject_kind: event.subject_kind.clone(),
                    subject_id: event.subject_id.clone(),
                    subject_label: event.subject_label.clone(),
                    user: event.user.clone(),
                    token: event.token.clone(),
                    key: event.key.clone(),
                    request_kind: event.request_kind.clone(),
                    count: 0,
                    first_seen: event.occurred_at,
                    last_seen: event.occurred_at,
                    latest_event: event.clone(),
                });
            entry.count += 1;
            entry.first_seen = entry.first_seen.min(event.occurred_at);
            if event.occurred_at >= entry.last_seen {
                entry.last_seen = event.occurred_at;
                if event.occurred_at > entry.latest_event.occurred_at
                    || (event.occurred_at == entry.latest_event.occurred_at
                        && event.id > entry.latest_event.id)
                {
                    entry.latest_event = event.clone();
                    entry.token = event.token.clone();
                    entry.key = event.key.clone();
                    entry.user = event.user.clone();
                }
            }
        }

        let mut grouped = groups
            .into_iter()
            .map(|(id, value)| AlertGroupRecord {
                id,
                alert_type: value.alert_type,
                subject_kind: value.subject_kind,
                subject_id: value.subject_id,
                subject_label: value.subject_label,
                user: value.user,
                token: value.token,
                key: value.key,
                request_kind: value.request_kind,
                count: value.count,
                first_seen: value.first_seen,
                last_seen: value.last_seen,
                latest_event: value.latest_event,
            })
            .collect::<Vec<_>>();
        grouped.sort_by(|left, right| {
            right
                .last_seen
                .cmp(&left.last_seen)
                .then_with(|| right.count.cmp(&left.count))
                .then_with(|| right.id.cmp(&left.id))
        });
        grouped
    }

    fn paginate_alert_groups(
        groups: Vec<AlertGroupRecord>,
        page: i64,
        per_page: i64,
    ) -> PaginatedAlertGroups {
        let total = groups.len() as i64;
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let start = ((page - 1) * per_page) as usize;
        let end = (start + per_page as usize).min(groups.len());
        let items = if start >= groups.len() {
            Vec::new()
        } else {
            groups[start..end].to_vec()
        };
        PaginatedAlertGroups {
            items,
            total,
            page,
            per_page,
        }
    }

    fn summarize_alert_type_counts(events: &[AlertEventRecord]) -> Vec<AlertTypeCount> {
        let mut counts = default_alert_type_counts();
        let mut index_by_type = HashMap::new();
        for (index, item) in counts.iter().enumerate() {
            index_by_type.insert(item.alert_type.clone(), index);
        }
        for event in events {
            if let Some(index) = index_by_type.get(&event.alert_type).copied() {
                counts[index].count += 1;
            }
        }
        counts
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
        let events = self
            .fetch_alert_events_filtered(AlertEventFilters {
                alert_type,
                since,
                until,
                user_id,
                token_id,
                key_id,
                request_kinds,
            })
            .await?;
        Ok(Self::paginate_alert_events(events, page, per_page))
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
        let events = self
            .fetch_alert_events_filtered(AlertEventFilters {
                alert_type,
                since,
                until,
                user_id,
                token_id,
                key_id,
                request_kinds,
            })
            .await?;
        Ok(Self::paginate_alert_groups(
            Self::group_alert_events(&events),
            page,
            per_page,
        ))
    }

    pub(crate) async fn fetch_alert_catalog(&self) -> Result<AlertCatalog, ProxyError> {
        let events = self
            .fetch_alert_events_filtered(AlertEventFilters {
                alert_type: None,
                since: None,
                until: None,
                user_id: None,
                token_id: None,
                key_id: None,
                request_kinds: &[],
            })
            .await?;

        let types = Self::summarize_alert_type_counts(&events)
            .into_iter()
            .map(|value| LogFacetOption {
                value: value.alert_type,
                count: value.count,
            })
            .collect::<Vec<_>>();

        let mut request_kind_map: HashMap<String, TokenRequestKindOption> = HashMap::new();
        let mut users_map: HashMap<String, AlertFacetOption> = HashMap::new();
        let mut tokens_map: HashMap<String, AlertFacetOption> = HashMap::new();
        let mut keys_map: HashMap<String, AlertFacetOption> = HashMap::new();

        for event in &events {
            if let Some(request_kind) = event.request_kind.as_ref() {
                request_kind_map
                    .entry(request_kind.key.clone())
                    .and_modify(|entry| entry.count += 1)
                    .or_insert_with(|| TokenRequestKindOption {
                        key: request_kind.key.clone(),
                        label: request_kind.label.clone(),
                        protocol_group: token_request_kind_protocol_group(&request_kind.key)
                            .to_string(),
                        billing_group: token_request_kind_billing_group(&request_kind.key)
                            .to_string(),
                        count: 1,
                    });
            }

            if let Some(user) = event.user.as_ref() {
                users_map
                    .entry(user.user_id.clone())
                    .and_modify(|entry| entry.count += 1)
                    .or_insert_with(|| AlertFacetOption {
                        value: user.user_id.clone(),
                        label: alert_user_label(user),
                        count: 1,
                    });
            }
            if let Some(token) = event.token.as_ref() {
                tokens_map
                    .entry(token.id.clone())
                    .and_modify(|entry| entry.count += 1)
                    .or_insert_with(|| AlertFacetOption {
                        value: token.id.clone(),
                        label: token.label.clone(),
                        count: 1,
                    });
            }
            if let Some(key) = event.key.as_ref() {
                keys_map
                    .entry(key.id.clone())
                    .and_modify(|entry| entry.count += 1)
                    .or_insert_with(|| AlertFacetOption {
                        value: key.id.clone(),
                        label: key.label.clone(),
                        count: 1,
                    });
            }
        }

        let mut request_kind_options = request_kind_map.into_values().collect::<Vec<_>>();
        request_kind_options.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.key.cmp(&right.key))
        });

        let mut users = users_map.into_values().collect::<Vec<_>>();
        users.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.value.cmp(&right.value))
        });

        let mut tokens = tokens_map.into_values().collect::<Vec<_>>();
        tokens.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.value.cmp(&right.value))
        });

        let mut keys = keys_map.into_values().collect::<Vec<_>>();
        keys.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.value.cmp(&right.value))
        });

        Ok(AlertCatalog {
            retention_days: effective_auth_token_log_retention_days(),
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
        let since = Utc::now()
            .timestamp()
            .saturating_sub(clamped_window_hours.saturating_mul(3600));
        let events = self
            .fetch_alert_events_filtered(AlertEventFilters {
                alert_type: None,
                since: Some(since),
                until: None,
                user_id: None,
                token_id: None,
                key_id: None,
                request_kinds: &[],
            })
            .await?;
        let grouped = Self::group_alert_events(&events);
        Ok(RecentAlertsSummary {
            window_hours: clamped_window_hours,
            total_events: events.len() as i64,
            grouped_count: grouped.len() as i64,
            counts_by_type: Self::summarize_alert_type_counts(&events),
            top_groups: grouped.into_iter().take(5).collect(),
        })
    }
}
