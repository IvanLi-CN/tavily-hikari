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
    grouping_kind: String,
    row_sort_id: String,
    alert_type: String,
    subject_kind: String,
    subject_id: String,
    count: i64,
    first_seen: i64,
    last_seen: i64,
    semantic_window_kind: Option<String>,
    semantic_window_minutes: Option<i64>,
    semantic_window_start: Option<i64>,
    semantic_window_end: Option<i64>,
    child_count: i64,
}

#[derive(Debug, Clone)]
struct AlertGroupingEnvelope {
    top_level_items: Vec<AlertGroupRecord>,
}

#[derive(Debug, Clone)]
struct AlertChildWindowAccumulator {
    key: String,
    kind: AlertSemanticWindowKind,
    window_minutes: Option<i64>,
    semantic_window_start: Option<i64>,
    semantic_window_end: Option<i64>,
    events: Vec<AlertEventRecord>,
}


fn parse_request_rate_window_metadata(error_message: Option<&str>) -> Option<i64> {
    let message = error_message?.trim();
    let marker = "rolling ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find('m')?;
    rest[..end].trim().parse::<i64>().ok().filter(|value| *value > 0)
}

fn quota_window_kind_from_error_message(error_message: Option<&str>) -> Option<AlertSemanticWindowKind> {
    let message = error_message?.trim();
    if message.contains("quota exceeded on month window") {
        return Some(AlertSemanticWindowKind::Month);
    }
    if message.contains("quota exceeded on day window") {
        return Some(AlertSemanticWindowKind::Day);
    }
    if message.contains("quota exceeded on hour window") {
        return Some(AlertSemanticWindowKind::RollingHour);
    }
    None
}

fn local_hour_window_bounds(ts: i64) -> Option<(i64, i64)> {
    let now = chrono::DateTime::from_timestamp(ts, 0)?.with_timezone(&chrono::Local);
    let end = now.timestamp();
    let start = end.saturating_sub(59 * 60);
    Some((start, end))
}

fn local_day_window_bounds(ts: i64) -> Option<(i64, i64)> {
    let now = chrono::DateTime::from_timestamp(ts, 0)?.with_timezone(&chrono::Local);
    let start = start_of_local_day_utc_ts(now);
    let end = next_local_day_start_utc_ts(start).saturating_sub(1);
    Some((start, end))
}

fn utc_month_window_bounds(ts: i64) -> Option<(i64, i64)> {
    let now = chrono::DateTime::from_timestamp(ts, 0)?.with_timezone(&chrono::Utc);
    let start = start_of_month(now).timestamp();
    let end = shift_month_start_utc_ts(start, 1).saturating_sub(1);
    Some((start, end))
}

fn event_semantic_window(event: &AlertEventRecord) -> Option<AlertSemanticWindow> {
    match event.alert_type.as_str() {
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED => {
            let window_minutes = parse_request_rate_window_metadata(event.error_message.as_deref())
                .or(Some(request_rate_limit_window_minutes()));
            let rolling_window_secs = window_minutes.unwrap_or(5) * 60;
            Some(AlertSemanticWindow {
                kind: AlertSemanticWindowKind::RequestRate,
                window_minutes,
                window_start: Some(event.occurred_at.saturating_sub(rolling_window_secs)),
                window_end: Some(event.occurred_at),
                window_key: None,
            })
        }
        ALERT_TYPE_USER_QUOTA_EXHAUSTED => {
            let kind = quota_window_kind_from_error_message(event.error_message.as_deref())?;
            let (window_start, window_end, window_key) = match kind {
                AlertSemanticWindowKind::RollingHour => {
                    let (start, end) = local_hour_window_bounds(event.occurred_at)?;
                    (
                        Some(start),
                        Some(end),
                        Some(format!("hour:{start}")),
                    )
                }
                AlertSemanticWindowKind::Day => {
                    let (start, end) = local_day_window_bounds(event.occurred_at)?;
                    (
                        Some(start),
                        Some(end),
                        Some(format!("day:{start}")),
                    )
                }
                AlertSemanticWindowKind::Month => {
                    let (start, end) = utc_month_window_bounds(event.occurred_at)?;
                    (
                        Some(start),
                        Some(end),
                        Some(format!("month:{start}")),
                    )
                }
                AlertSemanticWindowKind::RequestRate => return None,
            };
            Some(AlertSemanticWindow {
                kind,
                window_minutes: if kind == AlertSemanticWindowKind::RollingHour {
                    Some(60)
                } else {
                    None
                },
                window_start,
                window_end,
                window_key,
            })
        }
        _ => None,
    }
}

fn group_request_kind_key(event: &AlertEventRecord) -> &str {
    event
        .request_kind
        .as_ref()
        .map(|value| value.key.as_str())
        .unwrap_or("unknown")
}

fn build_compat_group_record(events: &[AlertEventRecord]) -> Option<AlertGroupRecord> {
    let latest_event = events.first()?.clone();
    let earliest_event = events.last()?;
    Some(AlertGroupRecord {
        id: alert_group_id(&latest_event),
        alert_type: latest_event.alert_type.clone(),
        subject_kind: latest_event.subject_kind.clone(),
        subject_id: latest_event.subject_id.clone(),
        subject_label: latest_event.subject_label.clone(),
        user: latest_event.user.clone(),
        token: latest_event.token.clone(),
        key: latest_event.key.clone(),
        request_kind: latest_event.request_kind.clone(),
        count: events.len() as i64,
        first_seen: earliest_event.occurred_at,
        last_seen: latest_event.occurred_at,
        latest_event,
        grouping_kind: "compat".to_string(),
        semantic_window_kind: None,
        semantic_window_minutes: None,
        semantic_window_start: None,
        semantic_window_end: None,
        semantic_window_key: None,
        child_count: 0,
        event_count: events.len() as i64,
        children: Vec::new(),
        child_events: Vec::new(),
    })
}

fn semantic_group_base_id(event: &AlertEventRecord) -> String {
    let semantic = event.semantic_window.as_ref();
    let semantic_key = semantic
        .and_then(|value| value.window_key.clone())
        .unwrap_or_else(|| semantic.map(|value| value.kind.as_str().to_string()).unwrap_or_default());
    format!(
        "{}:{}:{}:{}",
        event.alert_type, event.subject_kind, event.subject_id, semantic_key
    )
}

fn child_group_id(parent_id: &str, index: usize) -> String {
    format!("{parent_id}:child:{index}")
}

fn build_child_group_record(id: String, events: Vec<AlertEventRecord>) -> Option<AlertGroupRecord> {
    let latest_event = events.first()?.clone();
    let earliest_event = events.last()?;
    let semantic = latest_event.semantic_window.clone();
    Some(AlertGroupRecord {
        id,
        alert_type: latest_event.alert_type.clone(),
        subject_kind: latest_event.subject_kind.clone(),
        subject_id: latest_event.subject_id.clone(),
        subject_label: latest_event.subject_label.clone(),
        user: latest_event.user.clone(),
        token: latest_event.token.clone(),
        key: latest_event.key.clone(),
        request_kind: None,
        count: events.len() as i64,
        first_seen: earliest_event.occurred_at,
        last_seen: latest_event.occurred_at,
        latest_event,
        grouping_kind: "child".to_string(),
        semantic_window_kind: semantic
            .as_ref()
            .map(|value| value.kind.as_str().to_string()),
        semantic_window_minutes: semantic.as_ref().and_then(|value| value.window_minutes),
        semantic_window_start: semantic.as_ref().and_then(|value| value.window_start),
        semantic_window_end: semantic.as_ref().and_then(|value| value.window_end),
        semantic_window_key: semantic.and_then(|value| value.window_key),
        child_count: 0,
        event_count: events.len() as i64,
        children: Vec::new(),
        child_events: events,
    })
}

fn sort_alert_events_desc(events: &mut [AlertEventRecord]) {
    events.sort_by(|left, right| {
        right
            .occurred_at
            .cmp(&left.occurred_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn semantic_subject_key(event: &AlertEventRecord) -> String {
    let semantic = event.semantic_window.as_ref();
    let kind = semantic
        .map(|value| value.kind.as_str())
        .unwrap_or("compat");
    let minutes = semantic
        .and_then(|value| value.window_minutes)
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!(
        "{}:{}:{}:{}:{}",
        event.alert_type, event.subject_kind, event.subject_id, kind, minutes
    )
}

fn compat_subject_key(event: &AlertEventRecord) -> String {
    format!(
        "{}:{}:{}:{}",
        event.alert_type,
        event.subject_kind,
        event.subject_id,
        group_request_kind_key(event)
    )
}

fn build_semantic_child_windows(events: Vec<AlertEventRecord>) -> Vec<AlertGroupRecord> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut asc_events = events;
    asc_events.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut windows: Vec<AlertChildWindowAccumulator> = Vec::new();
    for event in asc_events {
        let Some(semantic) = event.semantic_window.clone() else {
            continue;
        };
        let should_start_new = match windows.last() {
            Some(current) if current.kind == AlertSemanticWindowKind::RequestRate => {
                let threshold = current.window_minutes.unwrap_or(5) * 60;
                current
                    .events
                    .last()
                    .map(|last| event.occurred_at.saturating_sub(last.occurred_at) > threshold)
                    .unwrap_or(true)
            }
            Some(current) => current.key != semantic.window_key.clone().unwrap_or_default(),
            None => true,
        };
        if should_start_new {
            let key = semantic
                .window_key
                .clone()
                .unwrap_or_else(|| format!("{}:{}", semantic.kind.as_str(), event.occurred_at));
            windows.push(AlertChildWindowAccumulator {
                key,
                kind: semantic.kind,
                window_minutes: semantic.window_minutes,
                semantic_window_start: semantic.window_start,
                semantic_window_end: semantic.window_end,
                events: vec![event],
            });
            continue;
        }
        if let Some(current) = windows.last_mut() {
            current.events.push(event);
            if current.kind == AlertSemanticWindowKind::RequestRate {
                current.semantic_window_start = current
                    .semantic_window_start
                    .zip(current.events.last().and_then(|value| value.semantic_window.as_ref().and_then(|semantic| semantic.window_start)))
                    .map(|(left, right)| left.min(right))
                    .or(current.semantic_window_start)
                    .or_else(|| current.events.last().and_then(|value| value.semantic_window.as_ref().and_then(|semantic| semantic.window_start)));
                current.semantic_window_end = current
                    .semantic_window_end
                    .zip(current.events.last().and_then(|value| value.semantic_window.as_ref().and_then(|semantic| semantic.window_end)))
                    .map(|(left, right)| left.max(right))
                    .or(current.semantic_window_end)
                    .or_else(|| current.events.last().and_then(|value| value.semantic_window.as_ref().and_then(|semantic| semantic.window_end)));
            }
        }
    }

    windows
        .into_iter()
        .enumerate()
        .filter_map(|(index, mut window)| {
            sort_alert_events_desc(&mut window.events);
            if let Some(latest) = window.events.first_mut()
                && let Some(semantic) = latest.semantic_window.as_mut()
                && window.kind == AlertSemanticWindowKind::RequestRate
            {
                semantic.window_start = window.semantic_window_start;
                semantic.window_end = window.semantic_window_end;
                semantic.window_key = Some(window.key.clone());
            }
            let parent_id = semantic_group_base_id(window.events.first()?);
            build_child_group_record(child_group_id(&parent_id, index), window.events)
        })
        .collect()
}

fn child_group_chain_boundary(left: &AlertGroupRecord, right: &AlertGroupRecord) -> bool {
    let left_kind = left.semantic_window_kind.as_deref().unwrap_or_default();
    let right_kind = right.semantic_window_kind.as_deref().unwrap_or_default();
    if left_kind != right_kind {
        return true;
    }
    match left_kind {
        "request_rate" => {
            let threshold = left.semantic_window_minutes.unwrap_or(5) * 60;
            let left_end = left.semantic_window_end.unwrap_or(left.last_seen);
            let right_start = right.semantic_window_start.unwrap_or(right.first_seen);
            right_start.saturating_sub(left_end) > threshold
        }
        "rolling_hour" => {
            let left_end = left.semantic_window_end.unwrap_or(left.last_seen);
            let right_start = right.semantic_window_start.unwrap_or(right.first_seen);
            right_start.saturating_sub(left_end) > 60 * 60
        }
        "day" | "month" => {
            let left_end = left.semantic_window_end.unwrap_or(left.last_seen);
            let right_start = right.semantic_window_start.unwrap_or(right.first_seen);
            right_start.saturating_sub(left_end) > 1
        }
        _ => true,
    }
}

fn semantic_mother_id_from_child(latest_event: &AlertEventRecord, ordinal: usize) -> String {
    let base = semantic_group_base_id(latest_event);
    format!("{base}:mother:{ordinal}")
}

fn build_semantic_mother_groups(children: Vec<AlertGroupRecord>) -> Vec<AlertGroupRecord> {
    if children.is_empty() {
        return Vec::new();
    }
    let mut asc_children = children;
    asc_children.sort_by(|left, right| {
        left.first_seen
            .cmp(&right.first_seen)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut chains: Vec<Vec<AlertGroupRecord>> = Vec::new();
    for child in asc_children {
        let should_start_new = match chains.last().and_then(|items| items.last()) {
            Some(previous) => child_group_chain_boundary(previous, &child),
            None => true,
        };
        if should_start_new {
            chains.push(vec![child]);
        } else if let Some(current) = chains.last_mut() {
            current.push(child);
        }
    }

    chains
        .into_iter()
        .enumerate()
        .filter_map(|(index, mut chain)| {
            chain.sort_by(|left, right| {
                right
                    .last_seen
                    .cmp(&left.last_seen)
                    .then_with(|| right.id.cmp(&left.id))
            });
            let latest_child = chain.first()?.clone();
            let first_seen = chain.iter().map(|value| value.first_seen).min()?;
            let last_seen = chain.iter().map(|value| value.last_seen).max()?;
            let event_count = chain.iter().map(|value| value.event_count).sum::<i64>();
            let parent_id = format!("{}:mother:{index}", semantic_group_base_id(&latest_child.latest_event));
            Some(AlertGroupRecord {
                id: parent_id,
                alert_type: latest_child.alert_type.clone(),
                subject_kind: latest_child.subject_kind.clone(),
                subject_id: latest_child.subject_id.clone(),
                subject_label: latest_child.subject_label.clone(),
                user: latest_child.user.clone(),
                token: latest_child.token.clone(),
                key: latest_child.key.clone(),
                request_kind: None,
                count: event_count,
                first_seen,
                last_seen,
                latest_event: latest_child.latest_event.clone(),
                grouping_kind: "mother".to_string(),
                semantic_window_kind: latest_child.semantic_window_kind.clone(),
                semantic_window_minutes: latest_child.semantic_window_minutes,
                semantic_window_start: chain
                    .iter()
                    .filter_map(|value| value.semantic_window_start)
                    .min(),
                semantic_window_end: chain
                    .iter()
                    .filter_map(|value| value.semantic_window_end)
                    .max(),
                semantic_window_key: None,
                child_count: chain.len() as i64,
                event_count,
                children: chain,
                child_events: Vec::new(),
            })
        })
        .collect()
}

fn build_group_records_from_events(events: Vec<AlertEventRecord>) -> AlertGroupingEnvelope {
    let mut compat_groups: HashMap<String, Vec<AlertEventRecord>> = HashMap::new();
    let mut semantic_groups: HashMap<String, Vec<AlertEventRecord>> = HashMap::new();

    for event in events {
        if matches!(
            event.alert_type.as_str(),
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED | ALERT_TYPE_USER_QUOTA_EXHAUSTED
        ) && event.semantic_window.is_some()
        {
            semantic_groups
                .entry(semantic_subject_key(&event))
                .or_default()
                .push(event);
        } else {
            compat_groups
                .entry(compat_subject_key(&event))
                .or_default()
                .push(event);
        }
    }

    let mut items = Vec::new();

    for (_key, mut grouped_events) in compat_groups {
        sort_alert_events_desc(&mut grouped_events);
        if let Some(group) = build_compat_group_record(&grouped_events) {
            items.push(group);
        }
    }

    for (_key, grouped_events) in semantic_groups {
        let children = build_semantic_child_windows(grouped_events);
        if children.is_empty() {
            continue;
        }
        items.extend(build_semantic_mother_groups(children));
    }

    items.sort_by(|left, right| {
        right
            .last_seen
            .cmp(&left.last_seen)
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| right.alert_type.cmp(&left.alert_type))
            .then_with(|| right.id.cmp(&left.id))
    });

    AlertGroupingEnvelope {
        top_level_items: items,
    }
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
    let user_subject = || {
        user.map(|user| {
            (
                ALERT_SUBJECT_USER.to_string(),
                user.user_id.clone(),
                alert_user_label(user),
            )
        })
    };

    let token_subject = || {
        token.map(|token| {
            (
                ALERT_SUBJECT_TOKEN.to_string(),
                token.id.clone(),
                token.label.clone(),
            )
        })
    };

    let key_subject = || {
        key.map(|key| {
            (
                ALERT_SUBJECT_KEY.to_string(),
                key.id.clone(),
                key.label.clone(),
            )
        })
    };

    let preferred_subject = match alert_type {
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429
        | ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432
        | ALERT_TYPE_UPSTREAM_KEY_BLOCKED => key_subject()
            .or_else(user_subject)
            .or_else(token_subject),
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED | ALERT_TYPE_USER_QUOTA_EXHAUSTED => user_subject()
            .or_else(token_subject)
            .or_else(key_subject),
        _ => user_subject()
            .or_else(token_subject)
            .or_else(key_subject),
    };

    if let Some(subject) = preferred_subject {
        return subject;
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
    pub(crate) async fn fetch_recent_alerts_summary_token(
        &self,
        window_hours: i64,
    ) -> Result<[i64; 4], ProxyError> {
        let clamped_window_hours = window_hours.clamp(1, 24 * 30);
        let since = self
            .backend_time
            .now_ts()
            .saturating_sub(clamped_window_hours.saturating_mul(3600));
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS row_count,
                COALESCE(MAX(created_at), 0) AS max_created_at,
                COALESCE(SUM(created_at), 0) AS created_at_sum,
                COALESCE(SUM(COALESCE(request_log_id, 0)), 0) AS request_log_id_sum
            FROM auth_token_logs
            WHERE created_at >= ?
              AND (
                    failure_kind = 'upstream_rate_limited_429'
                 OR result_status = 'quota_exhausted'
              )
            "#,
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            row.try_get("row_count")?,
            row.try_get("max_created_at")?,
            row.try_get("created_at_sum")?,
            row.try_get("request_log_id_sum")?,
        ])
    }

    fn alert_subject_kind_sql(alias: &str) -> String {
        format!(
            "CASE \
                WHEN {alias}.alert_type IN ('upstream_rate_limited_429', 'upstream_usage_limit_432', 'upstream_key_blocked') AND {alias}.key_id IS NOT NULL THEN 'key' \
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
                WHEN {alias}.alert_type IN ('upstream_rate_limited_429', 'upstream_usage_limit_432', 'upstream_key_blocked') AND {alias}.key_id IS NOT NULL THEN {alias}.key_id \
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
                    WHEN atl.result_status = 'quota_exhausted' AND rl.tavily_status_code = 432 THEN 'upstream_usage_limit_432'
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
                NULL AS reason_detail
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
                m.reason_detail AS reason_detail
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
        Self::push_maintenance_alert_filters(query, filters);
        query.push(")");
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
        let started = Instant::now();
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

        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "alerts_projection",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/alerts/events"),
                scope: Some("alerts"),
                phase: Some("alerts_projection"),
                page_size: Some(per_page),
                row_count: Some(items.len()),
                degraded: Some("auth_token_logs_first"),
                ..Default::default()
            },
        );

        Ok(PaginatedAlertEvents {
            items,
            total,
            page,
            per_page,
        })
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
        let request_rate_minutes = request_rate_limit_window_minutes();
        let request_rate_seconds = request_rate_minutes * 60;
        Self::push_alert_events_cte(query, filters);
        query.push(format!(
            r#",
            classified_alerts AS (
                SELECT
                    alerts.*,
                    {subject_kind_sql} AS subject_kind,
                    {subject_id_sql} AS subject_id,
                    {request_kind_sql} AS request_kind_key_normalized,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN 'mother'
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND (
                                 instr(COALESCE(alerts.error_message, ''), 'month window') > 0
                                 OR instr(COALESCE(alerts.error_message, ''), 'day window') > 0
                                 OR instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                             )
                            THEN 'mother'
                        ELSE 'compat'
                    END AS grouping_kind,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN 'request_rate'
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'month window') > 0
                            THEN 'month'
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'day window') > 0
                            THEN 'day'
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                            THEN 'rolling_hour'
                        ELSE NULL
                    END AS semantic_window_kind,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN {request_rate_minutes}
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                            THEN 60
                        ELSE NULL
                    END AS semantic_window_minutes,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN alerts.occurred_at - {request_rate_seconds}
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                            THEN alerts.occurred_at - 3540
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'day window') > 0
                            THEN CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'localtime', 'start of day', 'utc')) AS INTEGER)
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'month window') > 0
                            THEN CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'start of month')) AS INTEGER)
                        ELSE NULL
                    END AS semantic_window_start,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN alerts.occurred_at
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                            THEN alerts.occurred_at
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'day window') > 0
                            THEN CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'localtime', 'start of day', '+1 day', 'utc')) AS INTEGER) - 1
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'month window') > 0
                            THEN CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'start of month', '+1 month')) AS INTEGER) - 1
                        ELSE NULL
                    END AS semantic_window_end,
                    CASE
                        WHEN alerts.alert_type = 'user_request_rate_limited'
                            THEN NULL
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'hour window') > 0
                            THEN printf('hour:%lld', alerts.occurred_at - 3540)
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'day window') > 0
                            THEN printf('day:%lld', CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'localtime', 'start of day', 'utc')) AS INTEGER))
                        WHEN alerts.alert_type = 'user_quota_exhausted'
                             AND instr(COALESCE(alerts.error_message, ''), 'month window') > 0
                            THEN printf('month:%lld', CAST(strftime('%s', datetime(alerts.occurred_at, 'unixepoch', 'start of month')) AS INTEGER))
                        ELSE NULL
                    END AS semantic_window_key
                FROM alerts
            ),
            filtered_alerts AS (
                SELECT * FROM classified_alerts
                WHERE 1 = 1
            "#
        ));
        Self::push_alert_request_kind_filter(
            query,
            "COALESCE(NULLIF(TRIM(request_kind_key_normalized), ''), 'unknown')",
            filters.request_kinds,
        );
        query.push(format!(
            r#"
            ),
            semantic_events AS (
                SELECT
                    filtered_alerts.*,
                    LAG(filtered_alerts.semantic_window_key) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.semantic_window_kind,
                                     COALESCE(filtered_alerts.semantic_window_minutes, 0)
                        ORDER BY filtered_alerts.occurred_at ASC, filtered_alerts.row_sort_id ASC
                    ) AS prev_window_key,
                    LAG(filtered_alerts.semantic_window_kind) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.semantic_window_kind,
                                     COALESCE(filtered_alerts.semantic_window_minutes, 0)
                        ORDER BY filtered_alerts.occurred_at ASC, filtered_alerts.row_sort_id ASC
                    ) AS prev_window_kind,
                    LAG(filtered_alerts.semantic_window_minutes) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.semantic_window_kind,
                                     COALESCE(filtered_alerts.semantic_window_minutes, 0)
                        ORDER BY filtered_alerts.occurred_at ASC, filtered_alerts.row_sort_id ASC
                    ) AS prev_window_minutes,
                    LAG(filtered_alerts.semantic_window_end) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.semantic_window_kind,
                                     COALESCE(filtered_alerts.semantic_window_minutes, 0)
                        ORDER BY filtered_alerts.occurred_at ASC, filtered_alerts.row_sort_id ASC
                    ) AS prev_window_end,
                    LAG(filtered_alerts.occurred_at) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.semantic_window_kind,
                                     COALESCE(filtered_alerts.semantic_window_minutes, 0)
                        ORDER BY filtered_alerts.occurred_at ASC, filtered_alerts.row_sort_id ASC
                    ) AS prev_occurred_at
                FROM filtered_alerts
                WHERE filtered_alerts.grouping_kind = 'mother'
                  AND filtered_alerts.semantic_window_kind IS NOT NULL
            ),
            semantic_children AS (
                SELECT
                    semantic_events.*,
                    SUM(
                        CASE
                            WHEN semantic_events.semantic_window_kind = 'request_rate' THEN
                                CASE
                                    WHEN semantic_events.prev_occurred_at IS NULL THEN 1
                                    WHEN semantic_events.occurred_at - semantic_events.prev_occurred_at
                                         > COALESCE(semantic_events.prev_window_minutes, semantic_events.semantic_window_minutes, {request_rate_minutes}) * 60
                                        THEN 1
                                    ELSE 0
                                END
                            ELSE
                                CASE
                                    WHEN semantic_events.prev_window_key IS NULL THEN 1
                                    WHEN semantic_events.prev_window_kind != semantic_events.semantic_window_kind THEN 1
                                    WHEN COALESCE(semantic_events.prev_window_key, '') != COALESCE(semantic_events.semantic_window_key, '') THEN 1
                                    ELSE 0
                                END
                        END
                    ) OVER (
                        PARTITION BY semantic_events.alert_type,
                                     semantic_events.subject_kind,
                                     semantic_events.subject_id,
                                     semantic_events.semantic_window_kind,
                                     COALESCE(semantic_events.semantic_window_minutes, 0)
                        ORDER BY semantic_events.occurred_at ASC, semantic_events.row_sort_id ASC
                        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                    ) AS child_ordinal
                FROM semantic_events
            ),
            semantic_child_ranges AS (
                SELECT
                    semantic_children.*,
                    MIN(semantic_children.occurred_at) OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                    ) AS child_first_seen,
                    MAX(semantic_children.occurred_at) OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                    ) AS child_last_seen,
                    COUNT(*) OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                    ) AS child_event_count,
                    MIN(semantic_children.semantic_window_start) OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                    ) AS child_window_start,
                    MAX(semantic_children.semantic_window_end) OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                    ) AS child_window_end,
                    ROW_NUMBER() OVER (
                        PARTITION BY semantic_children.alert_type,
                                     semantic_children.subject_kind,
                                     semantic_children.subject_id,
                                     semantic_children.semantic_window_kind,
                                     COALESCE(semantic_children.semantic_window_minutes, 0),
                                     semantic_children.child_ordinal
                        ORDER BY semantic_children.occurred_at DESC, semantic_children.row_sort_id DESC
                    ) AS child_latest_rank
                FROM semantic_children
            ),
            semantic_child_heads AS (
                SELECT
                    semantic_child_ranges.alert_type,
                    semantic_child_ranges.subject_kind,
                    semantic_child_ranges.subject_id,
                    semantic_child_ranges.semantic_window_kind,
                    semantic_child_ranges.semantic_window_minutes,
                    semantic_child_ranges.child_ordinal,
                    semantic_child_ranges.row_sort_id,
                    semantic_child_ranges.child_first_seen,
                    semantic_child_ranges.child_last_seen,
                    semantic_child_ranges.child_event_count,
                    semantic_child_ranges.child_window_start,
                    semantic_child_ranges.child_window_end,
                    LAG(semantic_child_ranges.child_window_end) OVER (
                        PARTITION BY semantic_child_ranges.alert_type,
                                     semantic_child_ranges.subject_kind,
                                     semantic_child_ranges.subject_id,
                                     semantic_child_ranges.semantic_window_kind,
                                     COALESCE(semantic_child_ranges.semantic_window_minutes, 0)
                        ORDER BY semantic_child_ranges.child_first_seen ASC,
                                 semantic_child_ranges.child_ordinal ASC
                    ) AS prev_child_window_end,
                    LAG(semantic_child_ranges.child_last_seen) OVER (
                        PARTITION BY semantic_child_ranges.alert_type,
                                     semantic_child_ranges.subject_kind,
                                     semantic_child_ranges.subject_id,
                                     semantic_child_ranges.semantic_window_kind,
                                     COALESCE(semantic_child_ranges.semantic_window_minutes, 0)
                        ORDER BY semantic_child_ranges.child_first_seen ASC,
                                 semantic_child_ranges.child_ordinal ASC
                    ) AS prev_child_last_seen
                FROM semantic_child_ranges
                WHERE semantic_child_ranges.child_latest_rank = 1
            ),
            semantic_mother_heads AS (
                SELECT
                    semantic_child_heads.*,
                    SUM(
                        CASE
                            WHEN semantic_child_heads.prev_child_last_seen IS NULL THEN 1
                            WHEN semantic_child_heads.semantic_window_kind = 'request_rate'
                                 AND semantic_child_heads.child_window_start
                                     - COALESCE(semantic_child_heads.prev_child_window_end, semantic_child_heads.prev_child_last_seen)
                                     > COALESCE(semantic_child_heads.semantic_window_minutes, {request_rate_minutes}) * 60
                                THEN 1
                            WHEN semantic_child_heads.semantic_window_kind = 'rolling_hour'
                                 AND semantic_child_heads.child_window_start
                                     - COALESCE(semantic_child_heads.prev_child_window_end, semantic_child_heads.prev_child_last_seen)
                                     > 3600
                                THEN 1
                            WHEN semantic_child_heads.semantic_window_kind IN ('day', 'month')
                                 AND semantic_child_heads.child_window_start
                                     - COALESCE(semantic_child_heads.prev_child_window_end, semantic_child_heads.prev_child_last_seen)
                                     > 1
                                THEN 1
                            ELSE 0
                        END
                    ) OVER (
                        PARTITION BY semantic_child_heads.alert_type,
                                     semantic_child_heads.subject_kind,
                                     semantic_child_heads.subject_id,
                                     semantic_child_heads.semantic_window_kind,
                                     COALESCE(semantic_child_heads.semantic_window_minutes, 0)
                        ORDER BY semantic_child_heads.child_first_seen ASC,
                                 semantic_child_heads.child_ordinal ASC
                        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                        ) AS mother_ordinal
                FROM semantic_child_heads
            ),
            semantic_grouped_alerts AS (
                SELECT
                    'mother' AS grouping_kind,
                    semantic_mother_heads.row_sort_id AS row_sort_id,
                    semantic_mother_heads.alert_type AS alert_type,
                    semantic_mother_heads.subject_kind AS subject_kind,
                    semantic_mother_heads.subject_id AS subject_id,
                    NULL AS request_kind_key_normalized,
                    SUM(semantic_mother_heads.child_event_count) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS total_count,
                    MIN(semantic_mother_heads.child_first_seen) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS first_seen,
                    MAX(semantic_mother_heads.child_last_seen) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS last_seen,
                    semantic_mother_heads.semantic_window_kind AS semantic_window_kind,
                    semantic_mother_heads.semantic_window_minutes AS semantic_window_minutes,
                    MIN(semantic_mother_heads.child_window_start) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS semantic_window_start,
                    MAX(semantic_mother_heads.child_window_end) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS semantic_window_end,
                    COUNT(*) OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                    ) AS child_count,
                    ROW_NUMBER() OVER (
                        PARTITION BY semantic_mother_heads.alert_type,
                                     semantic_mother_heads.subject_kind,
                                     semantic_mother_heads.subject_id,
                                     semantic_mother_heads.semantic_window_kind,
                                     COALESCE(semantic_mother_heads.semantic_window_minutes, 0),
                                     semantic_mother_heads.mother_ordinal
                        ORDER BY semantic_mother_heads.child_last_seen DESC,
                                 semantic_mother_heads.row_sort_id DESC
                    ) AS group_rank
                FROM semantic_mother_heads
            ),
            compat_grouped_alerts AS (
                SELECT
                    'compat' AS grouping_kind,
                    filtered_alerts.row_sort_id AS row_sort_id,
                    filtered_alerts.alert_type AS alert_type,
                    filtered_alerts.subject_kind AS subject_kind,
                    filtered_alerts.subject_id AS subject_id,
                    filtered_alerts.request_kind_key_normalized AS request_kind_key_normalized,
                    COUNT(*) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.request_kind_key_normalized
                    ) AS total_count,
                    MIN(filtered_alerts.occurred_at) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.request_kind_key_normalized
                    ) AS first_seen,
                    MAX(filtered_alerts.occurred_at) OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.request_kind_key_normalized
                    ) AS last_seen,
                    NULL AS semantic_window_kind,
                    NULL AS semantic_window_minutes,
                    NULL AS semantic_window_start,
                    NULL AS semantic_window_end,
                    0 AS child_count,
                    ROW_NUMBER() OVER (
                        PARTITION BY filtered_alerts.alert_type,
                                     filtered_alerts.subject_kind,
                                     filtered_alerts.subject_id,
                                     filtered_alerts.request_kind_key_normalized
                        ORDER BY filtered_alerts.occurred_at DESC, filtered_alerts.row_sort_id DESC
                    ) AS group_rank
                FROM filtered_alerts
                WHERE filtered_alerts.grouping_kind = 'compat'
            ),
            grouped_alerts AS (
                SELECT * FROM semantic_grouped_alerts WHERE group_rank = 1
                UNION ALL
                SELECT * FROM compat_grouped_alerts WHERE group_rank = 1
            )"#
        ));
    }

    fn decode_alert_group_projection_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<AlertGroupProjectionRow, sqlx::Error> {
        Ok(AlertGroupProjectionRow {
            grouping_kind: row.try_get("grouping_kind")?,
            row_sort_id: row.try_get("row_sort_id")?,
            alert_type: row.try_get("alert_type")?,
            subject_kind: row.try_get("subject_kind")?,
            subject_id: row.try_get("subject_id")?,
            count: row.try_get("total_count")?,
            first_seen: row.try_get("first_seen")?,
            last_seen: row.try_get("last_seen")?,
            semantic_window_kind: row.try_get("semantic_window_kind")?,
            semantic_window_minutes: row.try_get("semantic_window_minutes")?,
            semantic_window_start: row.try_get("semantic_window_start")?,
            semantic_window_end: row.try_get("semantic_window_end")?,
            child_count: row.try_get("child_count")?,
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
                    id: if group.grouping_kind == "mother" {
                        semantic_mother_id_from_child(&latest_event, 0)
                    } else {
                        alert_group_id(&latest_event)
                    },
                    alert_type: group.alert_type,
                    subject_kind: group.subject_kind,
                    subject_id: group.subject_id,
                    subject_label: latest_event.subject_label.clone(),
                    user: latest_event.user.clone(),
                    token: latest_event.token.clone(),
                    key: latest_event.key.clone(),
                    request_kind: (group.grouping_kind == "compat")
                        .then(|| latest_event.request_kind.clone())
                        .flatten(),
                    count: group.count,
                    first_seen: group.first_seen,
                    last_seen: group.last_seen,
                    latest_event,
                    grouping_kind: group.grouping_kind,
                    semantic_window_kind: group.semantic_window_kind,
                    semantic_window_minutes: group.semantic_window_minutes,
                    semantic_window_start: group.semantic_window_start,
                    semantic_window_end: group.semantic_window_end,
                    semantic_window_key: None,
                    child_count: group.child_count,
                    event_count: group.count,
                    children: Vec::new(),
                    child_events: Vec::new(),
                })
            })
            .collect()
    }

    async fn fetch_alert_group_projection_page(
        &self,
        filters: AlertEventFilters<'_>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AlertGroupRecord>, i64), ProxyError> {
        let started = Instant::now();
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let mut count_query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut count_query, filters);
        count_query.push(" SELECT COUNT(*) FROM grouped_alerts");
        let total: i64 = count_query.build_query_scalar().fetch_one(&self.pool).await?;

        let mut query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut query, filters);
        query.push(" SELECT * FROM grouped_alerts");
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
        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "alerts_grouping",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/alerts/groups"),
                scope: Some("alerts"),
                phase: Some("alerts_grouping"),
                page_size: Some(per_page),
                row_count: Some(items.len()),
                degraded: Some("auth_token_logs_first"),
                ..Default::default()
            },
        );
        Ok((items, total))
    }

    async fn populate_selected_mother_groups(
        &self,
        filters: AlertEventFilters<'_>,
        groups: Vec<AlertGroupRecord>,
    ) -> Result<Vec<AlertGroupRecord>, ProxyError> {
        if groups.is_empty() {
            return Ok(groups);
        }
        let selected_mother_keys = groups
            .iter()
            .filter(|group| group.grouping_kind == "mother")
            .map(|group| {
                (
                    group.alert_type.clone(),
                    group.subject_kind.clone(),
                    group.subject_id.clone(),
                    group.first_seen,
                    group.last_seen,
                )
            })
            .collect::<HashSet<_>>();
        let selected_subjects = groups
            .iter()
            .filter(|group| group.grouping_kind == "mother")
            .map(|group| {
                (
                    group.alert_type.clone(),
                    group.subject_kind.clone(),
                    group.subject_id.clone(),
                    group.first_seen,
                    group.last_seen,
                )
            })
            .collect::<Vec<_>>();
        if selected_subjects.is_empty() {
            return Ok(groups);
        }
        let mut query = QueryBuilder::new("");
        let subject_kind_sql = Self::alert_subject_kind_sql("alerts");
        let subject_id_sql = Self::alert_subject_id_sql("alerts");
        Self::push_alert_events_cte(&mut query, filters);
        query.push(" SELECT * FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter(
            &mut query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        query.push(" AND (");
        {
            let mut separated = query.separated(" OR ");
            for (alert_type, subject_kind, subject_id, first_seen, last_seen) in &selected_subjects {
                separated.push("(");
                separated
                    .push_unseparated("alerts.alert_type = ")
                    .push_bind_unseparated(alert_type);
                separated.push_unseparated(" AND ");
                separated.push_unseparated(subject_kind_sql.as_str());
                separated
                    .push_unseparated(" = ")
                    .push_bind_unseparated(subject_kind);
                separated.push_unseparated(" AND ");
                separated.push_unseparated(subject_id_sql.as_str());
                separated
                    .push_unseparated(" = ")
                    .push_bind_unseparated(subject_id);
                separated
                    .push_unseparated(" AND alerts.occurred_at >= ")
                    .push_bind_unseparated(*first_seen);
                separated
                    .push_unseparated(" AND alerts.occurred_at <= ")
                    .push_bind_unseparated(*last_seen);
                separated.push_unseparated(")");
            }
        }
        query.push(")");
        query.push(" ORDER BY occurred_at DESC, row_sort_id DESC");

        let rows = query.build().fetch_all(&self.pool).await?;
        let all_events = rows
            .into_iter()
            .map(Self::decode_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect::<Vec<_>>();
        let grouped = build_group_records_from_events(all_events);
        let mut groups_by_mother_key = grouped
            .top_level_items
            .into_iter()
            .filter_map(|group| {
                if group.grouping_kind != "mother" {
                    return None;
                }
                let key = (
                    group.alert_type.clone(),
                    group.subject_kind.clone(),
                    group.subject_id.clone(),
                    group.first_seen,
                    group.last_seen,
                );
                selected_mother_keys.contains(&key).then_some((key, group))
            })
            .collect::<HashMap<_, _>>();

        Ok(groups
            .into_iter()
            .map(|group| {
                if group.grouping_kind != "mother" {
                    return group;
                }
                let key = (
                    group.alert_type.clone(),
                    group.subject_kind.clone(),
                    group.subject_id.clone(),
                    group.first_seen,
                    group.last_seen,
                );
                groups_by_mother_key.remove(&key).unwrap_or(group)
            })
            .collect())
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

        let mut event = AlertEventRecord {
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
            semantic_window: None,
        };
        event.semantic_window = event_semantic_window(&event);
        Some(event)
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
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let filters = AlertEventFilters {
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            request_kinds,
        };
        let (page_items, total) =
            self.fetch_alert_group_projection_page(filters, page, per_page).await?;
        let items = self.populate_selected_mother_groups(filters, page_items).await?;
        Ok(PaginatedAlertGroups {
            items,
            total,
            page,
            per_page,
        })
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
        let mut total_query = QueryBuilder::new("");
        Self::push_alert_events_cte(&mut total_query, filters);
        total_query.push(" SELECT COUNT(*) FROM alerts");
        let total_events: i64 = total_query.build_query_scalar().fetch_one(&self.pool).await?;
        let (top_groups, grouped_count) = self.fetch_alert_group_projection_page(filters, 1, 5).await?;

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

        Ok(RecentAlertsSummary {
            window_hours: clamped_window_hours,
            total_events,
            grouped_count,
            counts_by_type,
            top_groups,
            coverage: "ok".to_string(),
            stale: false,
            error: None,
        })
    }
}

#[cfg(test)]
mod alert_grouping_tests {
    use super::*;
    use crate::BackendTime;
    use tempfile::tempdir;

    async fn seed_bound_user_and_token(
        store: &KeyStore,
        user_id: &str,
        token_id: &str,
        display_name: &str,
        username: &str,
        created_at: i64,
    ) {
        sqlx::query(
            "INSERT INTO users (id, display_name, username, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(display_name)
        .bind(username)
        .bind(created_at)
        .bind(created_at)
        .execute(&store.pool)
        .await
        .expect("insert user");

        sqlx::query("INSERT INTO auth_tokens (id, secret, created_at) VALUES (?, ?, ?)")
            .bind(token_id)
            .bind(format!("secret-{token_id}"))
            .bind(created_at)
            .execute(&store.pool)
            .await
            .expect("insert auth token");

        sqlx::query(
            "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token_id)
        .bind(created_at)
        .bind(created_at)
        .execute(&store.pool)
        .await
        .expect("insert user token binding");
    }

    async fn insert_request_log(
        store: &KeyStore,
        token_id: &str,
        key_id: &str,
        created_at: i64,
    ) -> i64 {
        sqlx::query_scalar(
            r#"
            INSERT INTO observability.request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                tavily_status_code,
                result_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                counts_business_quota,
                created_at
            ) VALUES (?, ?, 'POST', '/api/tavily/search', 'max_results=5', 432, 'quota_exhausted', 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 1, ?)
            RETURNING id
            "#,
        )
        .bind(key_id)
        .bind(token_id)
        .bind(created_at)
        .fetch_one(&store.pool)
        .await
        .expect("insert request log")
    }

    async fn insert_request_rate_alert(
        store: &KeyStore,
        token_id: &str,
        created_at: i64,
        request_kind_key: &str,
        request_kind_label: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                created_at
            ) VALUES (?, 'POST', '/mcp', ?, ?, ?, 'quota_exhausted', 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)', 'none', 'none', 'none', 0, ?)
            "#,
        )
        .bind(token_id)
        .bind(request_kind_key)
        .bind(request_kind_label)
        .bind(request_kind_label)
        .bind(created_at)
        .execute(&store.pool)
        .await
        .expect("insert request-rate alert");
    }

    async fn insert_upstream_usage_limit_alert(
        store: &KeyStore,
        token_id: &str,
        key_id: &str,
        created_at: i64,
    ) {
        let request_log_id = insert_request_log(store, token_id, key_id, created_at).await;
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', 'max_results=5', 432, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'quota_exhausted', 'This request exceeds your plan''s set usage limit.', 'none', 'none', 'none', 1, ?, ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(key_id)
        .bind(request_log_id)
        .bind(created_at)
        .execute(&store.pool)
        .await
        .expect("insert upstream usage-limit alert");
    }

    fn make_alert_event(
        id: &str,
        alert_type: &str,
        occurred_at: i64,
        request_kind_key: &str,
        request_kind_label: &str,
        error_message: Option<&str>,
    ) -> AlertEventRecord {
        let mut event = AlertEventRecord {
            id: id.to_string(),
            alert_type: alert_type.to_string(),
            title: format!("title-{id}"),
            summary: format!("summary-{id}"),
            occurred_at,
            subject_kind: "user".to_string(),
            subject_id: "usr_test".to_string(),
            subject_label: "Test User".to_string(),
            user: Some(AlertUserRef {
                user_id: "usr_test".to_string(),
                display_name: Some("Test User".to_string()),
                username: Some("tester".to_string()),
            }),
            token: Some(AlertEntityRef {
                id: "tok_test".to_string(),
                label: "tok_test".to_string(),
            }),
            key: None,
            request: Some(AlertRequestRef {
                id: occurred_at,
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                query: None,
            }),
            request_kind: Some(TokenRequestKind::new(
                request_kind_key,
                request_kind_label,
                Some(request_kind_label.to_string()),
            )),
            failure_kind: None,
            result_status: Some("quota_exhausted".to_string()),
            error_message: error_message.map(str::to_string),
            reason_code: None,
            reason_summary: None,
            reason_detail: None,
            source: AlertSourceRef {
                kind: "auth_token_log".to_string(),
                id: format!("source-{id}"),
            },
            semantic_window: None,
        };
        event.semantic_window = event_semantic_window(&event);
        event
    }

    #[test]
    fn request_rate_events_merge_request_kinds_into_one_child_window() {
        let grouped = build_group_records_from_events(vec![
            make_alert_event(
                "evt-1",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_000,
                "mcp_initialize",
                "MCP initialize",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-2",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_060,
                "mcp_tools_list",
                "MCP tools/list",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-3",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_120,
                "mcp_notifications_initialized",
                "MCP notifications/initialized",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-4",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_180,
                "mcp_resources_list",
                "MCP resources/list",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
        ]);

        assert_eq!(grouped.top_level_items.len(), 1);
        let mother = &grouped.top_level_items[0];
        assert_eq!(mother.grouping_kind, "mother");
        assert_eq!(mother.child_count, 1);
        assert_eq!(mother.event_count, 4);
        assert!(mother.request_kind.is_none());

        let child = &mother.children[0];
        assert_eq!(child.grouping_kind, "child");
        assert_eq!(child.child_events.len(), 4);
        assert!(child.request_kind.is_none());
        assert_eq!(
            child.child_events[0]
                .request_kind
                .as_ref()
                .map(|value| value.key.as_str()),
            Some("mcp_resources_list")
        );
        assert_eq!(
            child.child_events[3]
                .request_kind
                .as_ref()
                .map(|value| value.key.as_str()),
            Some("mcp_initialize")
        );
    }

    #[test]
    fn upstream_alerts_prefer_key_subject_over_token() {
        let user = AlertUserRef {
            user_id: "usr_test".to_string(),
            display_name: Some("Test User".to_string()),
            username: Some("tester".to_string()),
        };
        let token = AlertEntityRef {
            id: "tok_test".to_string(),
            label: "tok_test".to_string(),
        };
        let key = AlertEntityRef {
            id: "key_test".to_string(),
            label: "key_test".to_string(),
        };

        let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
            ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432,
            Some(&user),
            Some(&token),
            Some(&key),
        );

        assert_eq!(subject_kind, ALERT_SUBJECT_KEY);
        assert_eq!(subject_id, "key_test");
        assert_eq!(subject_label, "key_test");
    }

    #[test]
    fn local_limit_alerts_prefer_user_subject_over_token() {
        let user = AlertUserRef {
            user_id: "usr_test".to_string(),
            display_name: Some("Test User".to_string()),
            username: Some("tester".to_string()),
        };
        let token = AlertEntityRef {
            id: "tok_test".to_string(),
            label: "tok_test".to_string(),
        };
        let key = AlertEntityRef {
            id: "key_test".to_string(),
            label: "key_test".to_string(),
        };

        let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            Some(&user),
            Some(&token),
            Some(&key),
        );

        assert_eq!(subject_kind, ALERT_SUBJECT_USER);
        assert_eq!(subject_id, "usr_test");
        assert_eq!(subject_label, "Test User");
    }

    #[test]
    fn request_rate_contiguous_children_roll_up_into_one_mother_range() {
        let grouped = build_group_records_from_events(vec![
            make_alert_event(
                "evt-1",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_000,
                "mcp_initialize",
                "MCP initialize",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-2",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_060,
                "mcp_tools_list",
                "MCP tools/list",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-3",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_420,
                "mcp_notifications_initialized",
                "MCP notifications/initialized",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-4",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_000_480,
                "mcp_resources_list",
                "MCP resources/list",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
            make_alert_event(
                "evt-5",
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
                1_700_001_500,
                "mcp_resources_list",
                "MCP resources/list",
                Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
            ),
        ]);

        assert_eq!(grouped.top_level_items.len(), 2);
        let merged = grouped
            .top_level_items
            .iter()
            .find(|item| item.child_count == 2)
            .expect("merged mother range");
        assert_eq!(merged.grouping_kind, "mother");
        assert_eq!(merged.event_count, 4);
        assert_eq!(merged.children.len(), 2);

        let split = grouped
            .top_level_items
            .iter()
            .find(|item| item.child_count == 1)
            .expect("split mother range");
        assert_eq!(split.event_count, 1);
    }

    #[test]
    fn quota_window_parser_recovers_hour_day_and_month_semantics() {
        let hour = make_alert_event(
            "hour",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_100_000,
            "tavily_search",
            "Tavily Search",
            Some("token quota exceeded on hour window (limit 100, used 100)"),
        );
        let day = make_alert_event(
            "day",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_100_000,
            "tavily_search",
            "Tavily Search",
            Some("token quota exceeded on day window (limit 500, used 500)"),
        );
        let month = make_alert_event(
            "month",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_100_000,
            "tavily_search",
            "Tavily Search",
            Some("token quota exceeded on month window (limit 5000, used 5000)"),
        );

        assert_eq!(
            hour.semantic_window.as_ref().map(|value| value.kind),
            Some(AlertSemanticWindowKind::RollingHour)
        );
        assert_eq!(
            day.semantic_window.as_ref().map(|value| value.kind),
            Some(AlertSemanticWindowKind::Day)
        );
        assert_eq!(
            month.semantic_window.as_ref().map(|value| value.kind),
            Some(AlertSemanticWindowKind::Month)
        );
        assert!(hour
            .semantic_window
            .as_ref()
            .and_then(|value| value.window_key.as_ref())
            .is_some_and(|value| value.starts_with("hour:")));
        assert!(day
            .semantic_window
            .as_ref()
            .and_then(|value| value.window_key.as_ref())
            .is_some_and(|value| value.starts_with("day:")));
        assert!(month
            .semantic_window
            .as_ref()
            .and_then(|value| value.window_key.as_ref())
            .is_some_and(|value| value.starts_with("month:")));
    }

    #[test]
    fn quota_hour_windows_form_distinct_children_under_one_mother() {
        let grouped = build_group_records_from_events(vec![
            make_alert_event(
                "hour-1",
                ALERT_TYPE_USER_QUOTA_EXHAUSTED,
                1_700_200_000,
                "tavily_search",
                "Tavily Search",
                Some("token quota exceeded on hour window (limit 100, used 100)"),
            ),
            make_alert_event(
                "hour-2",
                ALERT_TYPE_USER_QUOTA_EXHAUSTED,
                1_700_200_010,
                "tavily_extract",
                "Tavily Extract",
                Some("token quota exceeded on hour window (limit 100, used 100)"),
            ),
        ]);

        assert_eq!(grouped.top_level_items.len(), 1);
        let mother = &grouped.top_level_items[0];
        assert_eq!(mother.grouping_kind, "mother");
        assert_eq!(mother.semantic_window_kind.as_deref(), Some("rolling_hour"));
        assert_eq!(mother.child_count, 2);
        assert_eq!(mother.event_count, 2);
    }

    #[test]
    fn unrecoverable_quota_events_fall_back_to_compat_groups() {
        let grouped = build_group_records_from_events(vec![make_alert_event(
            "quota-compat",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_300_000,
            "tavily_search",
            "Tavily Search",
            Some("quota exhausted"),
        )]);

        assert_eq!(grouped.top_level_items.len(), 1);
        let group = &grouped.top_level_items[0];
        assert_eq!(group.grouping_kind, "compat");
        assert_eq!(
            group.request_kind.as_ref().map(|value| value.key.as_str()),
            Some("tavily_search")
        );
    }

    #[tokio::test]
    async fn fetch_alert_groups_page_executes_sqlite_grouped_query_for_mother_and_compat_groups() {
        let temp_dir = tempdir().expect("create temp dir");
        let db_path = temp_dir.path().join("alerts-groups.db");
        let db_str = db_path.to_string_lossy().to_string();
        let store = KeyStore::new_with_time(&db_str, BackendTime::system())
            .await
            .expect("create key store");

        let user_id = "usr_alerts_sql";
        let token_id = "tok_alerts_sql";
        let key_id = "key_alerts_sql";
        seed_bound_user_and_token(
            &store,
            user_id,
            token_id,
            "SQLite Alerts",
            "sqlite-alerts",
            1_700_000_000,
        )
        .await;

        for (created_at, request_kind_key, request_kind_label) in [
            (1_700_000_000_i64, "mcp_initialize", "MCP initialize"),
            (1_700_000_060_i64, "mcp_tools_list", "MCP tools/list"),
            (
                1_700_000_420_i64,
                "mcp_notifications_initialized",
                "MCP notifications/initialized",
            ),
            (1_700_000_480_i64, "mcp_resources_list", "MCP resources/list"),
        ] {
            insert_request_rate_alert(
                &store,
                token_id,
                created_at,
                request_kind_key,
                request_kind_label,
            )
            .await;
        }

        insert_upstream_usage_limit_alert(&store, token_id, key_id, 1_700_100_000).await;
        insert_upstream_usage_limit_alert(&store, token_id, key_id, 1_700_100_120).await;

        let page = store
            .fetch_alert_groups_page(None, None, None, None, None, None, &[], 1, 20)
            .await
            .expect("fetch grouped alerts page");

        assert_eq!(page.total, 2);

        let mother = page
            .items
            .iter()
            .find(|item| item.grouping_kind == "mother")
            .expect("semantic mother group");
        assert_eq!(mother.alert_type, ALERT_TYPE_USER_REQUEST_RATE_LIMITED);
        assert_eq!(mother.subject_kind, ALERT_SUBJECT_USER);
        assert_eq!(mother.child_count, 2);
        assert_eq!(mother.event_count, 4);
        assert_eq!(mother.children.len(), 2);
        assert!(mother.request_kind.is_none());

        let compat = page
            .items
            .iter()
            .find(|item| item.grouping_kind == "compat")
            .expect("compat group");
        assert_eq!(compat.alert_type, ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432);
        assert_eq!(compat.subject_kind, ALERT_SUBJECT_KEY);
        assert_eq!(compat.count, 2);
        assert_eq!(compat.event_count, 2);
        assert_eq!(
            compat.request_kind.as_ref().map(|value| value.key.as_str()),
            Some("api:search")
        );
    }

    #[tokio::test]
    async fn fetch_alert_groups_page_supports_multiple_mother_groups_without_sqlite_syntax_errors() {
        let temp_dir = tempdir().expect("create temp dir");
        let db_path = temp_dir.path().join("alerts-groups-multi-mother.db");
        let db_str = db_path.to_string_lossy().to_string();
        let store = KeyStore::new_with_time(&db_str, BackendTime::system())
            .await
            .expect("create key store");

        for (user_id, token_id, created_at) in [
            ("usr_alerts_multi_a", "tok_alerts_multi_a", 1_700_000_000_i64),
            ("usr_alerts_multi_b", "tok_alerts_multi_b", 1_700_010_000_i64),
        ] {
            seed_bound_user_and_token(
                &store,
                user_id,
                token_id,
                "SQLite Alerts",
                "sqlite-alerts",
                created_at.saturating_sub(120),
            )
            .await;
        }

        for (token_id, created_at, request_kind_key, request_kind_label) in [
            (
                "tok_alerts_multi_a",
                1_700_000_000_i64,
                "mcp_initialize",
                "MCP initialize",
            ),
            (
                "tok_alerts_multi_a",
                1_700_000_060_i64,
                "mcp_tools_list",
                "MCP tools/list",
            ),
            (
                "tok_alerts_multi_b",
                1_700_010_000_i64,
                "mcp_initialize",
                "MCP initialize",
            ),
            (
                "tok_alerts_multi_b",
                1_700_010_060_i64,
                "mcp_tools_list",
                "MCP tools/list",
            ),
        ] {
            insert_request_rate_alert(
                &store,
                token_id,
                created_at,
                request_kind_key,
                request_kind_label,
            )
            .await;
        }

        let page = store
            .fetch_alert_groups_page(
                Some(ALERT_TYPE_USER_REQUEST_RATE_LIMITED),
                None,
                None,
                None,
                None,
                None,
                &[],
                1,
                20,
            )
            .await
            .expect("fetch grouped alerts page with multiple mother groups");

        let mother_groups = page
            .items
            .iter()
            .filter(|item| item.grouping_kind == "mother")
            .collect::<Vec<_>>();
        assert_eq!(mother_groups.len(), 2);
        assert!(mother_groups.iter().all(|group| group.children.len() == 1));
    }
}
