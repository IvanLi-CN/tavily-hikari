#[cfg(test)]
#[path = "key_store_alerts_tests.rs"]
mod alert_grouping_tests;

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
        job: latest_event.job.clone(),
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
        job: latest_event.job.clone(),
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
                job: latest_child.job.clone(),
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

#[allow(clippy::too_many_arguments)]
fn build_alert_job_ref(
    id: Option<i64>,
    job_type: Option<String>,
    trigger_source: Option<String>,
    status: Option<String>,
    attempt: Option<i64>,
    message: Option<String>,
    queued_at: Option<i64>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
) -> Option<AlertJobRef> {
    Some(AlertJobRef {
        id: id?,
        job_type: job_type?,
        trigger_source: trigger_source.unwrap_or_else(|| "scheduler".to_string()),
        status: status?,
        attempt: attempt.unwrap_or(1),
        message,
        queued_at: queued_at.unwrap_or_default(),
        started_at,
        finished_at,
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
    job: Option<&AlertJobRef>,
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

    let job_subject = || {
        job.map(|job| {
            (
                ALERT_SUBJECT_JOB.to_string(),
                job.id.to_string(),
                format!("{} #{}", job.job_type, job.id),
            )
        })
    };

    let preferred_subject = match alert_type {
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429
        | ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432
        | ALERT_TYPE_UPSTREAM_KEY_BLOCKED
        | ALERT_TYPE_API_KEY_EXHAUSTED => key_subject()
            .or_else(user_subject)
            .or_else(token_subject),
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED | ALERT_TYPE_USER_QUOTA_EXHAUSTED => user_subject()
            .or_else(token_subject)
            .or_else(key_subject),
        ALERT_TYPE_JOB_FAILED => job_subject()
            .or_else(key_subject)
            .or_else(user_subject)
            .or_else(token_subject),
        _ => user_subject()
            .or_else(token_subject)
            .or_else(key_subject)
            .or_else(job_subject),
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

#[derive(Clone, Copy)]
struct AlertTitleSummaryContext<'a> {
    subject_label: &'a str,
    token: Option<&'a AlertEntityRef>,
    key: Option<&'a AlertEntityRef>,
    job: Option<&'a AlertJobRef>,
    request_kind: Option<&'a TokenRequestKind>,
    error_message: Option<&'a str>,
    reason_summary: Option<&'a str>,
}

fn build_alert_title_and_summary(
    alert_type: &str,
    context: AlertTitleSummaryContext<'_>,
) -> (String, String) {
    let AlertTitleSummaryContext {
        subject_label,
        token,
        key,
        job,
        request_kind,
        error_message,
        reason_summary,
    } = context;
    let token_label = token.map(|value| value.label.as_str()).unwrap_or("unknown");
    let key_label = key.map(|value| value.label.as_str()).unwrap_or("unknown");
    let job_type = job.map(|value| value.job_type.as_str()).unwrap_or("job");
    let job_status = job.map(|value| value.status.as_str()).unwrap_or("failed");
    let job_attempt = job.map(|value| value.attempt).unwrap_or(1);
    let job_message_suffix = job
        .and_then(|value| value.message.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(" Message: {value}."))
        .unwrap_or_default();
    let request_kind_label = request_kind
        .map(|value| value.label.as_str())
        .unwrap_or("Unknown request");
    let request_rate_window_minutes = parse_request_rate_window_metadata(error_message)
        .unwrap_or_else(request_rate_limit_window_minutes);
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
        ALERT_TYPE_API_KEY_EXHAUSTED => (
            format!("Upstream key {key_label} was exhausted"),
            format!("System maintenance marked key {key_label} as exhausted because upstream quota was exhausted.{reason_suffix}"),
        ),
        ALERT_TYPE_JOB_FAILED => (
            format!("{subject_label} failed"),
            format!("{job_type} finished with status {job_status} on attempt {job_attempt}.{job_message_suffix}"),
        ),
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED => (
            format!("{subject_label} hit the local request-rate limit"),
            format!(
                "Token {token_label} was rate limited by the local rolling {request_rate_window_minutes}m request-rate window for {request_kind_label}."
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
    pub(crate) async fn ensure_auth_token_logs_alert_time_index(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_auth_token_logs_alert_time
               ON auth_token_logs(created_at DESC, id DESC)
               WHERE failure_kind = 'upstream_rate_limited_429'
                  OR result_status = 'quota_exhausted'"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
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

    fn push_alert_events_for_source_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
        source: AlertReadSource,
    ) {
        match source {
            AlertReadSource::Raw => Self::push_alert_events_cte(query, filters),
            AlertReadSource::Projected => Self::push_projected_alert_events_cte(query, filters),
        }
    }

    async fn alert_projection_is_complete(&self) -> Result<bool, ProxyError> {
        let status = self.alert_projection_status().await?;
        Ok(status.coverage == "ok" && status.stale_reason.is_none())
    }

    async fn fetch_alert_query_rows(
        &self,
        query: QueryBuilder<'_, Sqlite>,
        source: AlertReadSource,
    ) -> Result<Vec<sqlx::sqlite::SqliteRow>, ProxyError> {
        self.fetch_alert_query_rows_for_operation(query, source, SqliteOperation::AdminAlertsRead)
            .await
    }

    async fn fetch_alert_query_rows_for_operation(
        &self,
        mut query: QueryBuilder<'_, Sqlite>,
        _source: AlertReadSource,
        operation: SqliteOperation,
    ) -> Result<Vec<sqlx::sqlite::SqliteRow>, ProxyError> {
        let mut conn = self.sqlite_runtime.acquire_operation_connection(operation).await?;
        let result = query.build().fetch_all(&mut *conn).await;
        conn.complete_query(result).await
    }

    async fn fetch_projected_alert_query_rows_in_admin_session(
        &self,
        session: &mut AdminAlertsReadSession,
        mut query: QueryBuilder<'_, Sqlite>,
    ) -> Result<Vec<sqlx::sqlite::SqliteRow>, ProxyError> {
        let result = query.build().fetch_all(&mut **session).await;
        session.query(result).await
    }

    async fn ensure_admin_alert_projection_coverage(
        &self,
        session: &mut AdminAlertsReadSession,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let result = sqlx::query_as::<_, (i64, i64, i64, Option<String>)>(
            r#"SELECT COUNT(*),
                      SUM(CASE WHEN phase = 'idle' THEN 1 ELSE 0 END),
                      SUM(CASE WHEN observed_at IS NOT NULL AND observed_at >= ? THEN 1 ELSE 0 END),
                      MAX(stale_reason)
                 FROM observability.dashboard_alert_projection_state"#,
        )
        .bind(now.saturating_sub(ALERT_PROJECTION_STALE_SECS))
        .fetch_one(&mut **session)
        .await;
        let (sources, idle_sources, fresh_sources, stale_reason) = session.query(result).await?;
        if sources == ALERT_PROJECTION_SOURCES.len() as i64
            && idle_sources == sources
            && fresh_sources == sources
            && stale_reason.is_none()
        {
            return Ok(());
        }
        session
            .defer(stale_reason.unwrap_or_else(|| "coverage_projecting".to_string()))
            .await
    }

    async fn effective_auth_token_log_retention_days_in_admin_session(
        &self,
        session: &mut AdminAlertsReadSession,
    ) -> Result<i64, ProxyError> {
        let result = sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_AUTH_TOKEN_LOG_RETENTION_DAYS_V1)
            .fetch_optional(&mut **session)
            .await;
        let value = session.query(result).await?;
        if let Some(retention_days) = value
            .as_deref()
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(normalize_auth_token_log_retention_days)
        {
            return Ok(retention_days);
        }
        effective_auth_token_log_retention_days()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_admin_alert_events_page(
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
        let filters = AlertEventFilters {
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            request_kinds,
        };
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let mut session = self.begin_admin_alerts_read_session().await?;
        let result = async {
            self.ensure_admin_alert_projection_coverage(&mut session).await?;

            let mut count_query = QueryBuilder::new("");
            Self::push_projected_alert_events_cte(&mut count_query, filters);
            count_query.push(" SELECT COUNT(*) FROM alerts WHERE 1 = 1");
            Self::push_alert_request_kind_filter(
                &mut count_query,
                "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
                filters.request_kinds,
            );
            let count_result = count_query.build_query_scalar().fetch_one(&mut *session).await;
            let total = session.query(count_result).await?;

            let mut query = QueryBuilder::new("");
            Self::push_projected_alert_events_cte(&mut query, filters);
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
            let query_result = query.build().fetch_all(&mut *session).await;
            let rows = session.query(query_result).await?;
            let items = rows
                .into_iter()
                .map(Self::decode_alert_event_projection_row)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(Self::build_alert_event_from_projection)
                .collect();
            Ok::<_, ProxyError>(PaginatedAlertEvents {
                items,
                total,
                page,
                per_page,
            })
        }
        .await;
        let finish_result = session.finish().await;
        finish_result?;
        result
    }

    async fn fetch_projected_alert_events_page(
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
        Self::push_projected_alert_events_cte(&mut count_query, filters);
        count_query.push(" SELECT COUNT(*) FROM alerts WHERE 1 = 1");
        Self::push_alert_request_kind_filter(
            &mut count_query,
            "COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown')",
            filters.request_kinds,
        );
        let mut count_conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let count_result = count_query.build_query_scalar().fetch_one(&mut *count_conn).await;
        let total = count_conn.complete_query(count_result).await?;

        let mut query = QueryBuilder::new("");
        Self::push_projected_alert_events_cte(&mut query, filters);
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
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let result = query.build().fetch_all(&mut *conn).await;
        let rows = conn.complete_query(result).await?;
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
            "alerts_projection_sidecar",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/alerts/events"),
                scope: Some("alerts"),
                phase: Some("projection_sidecar"),
                page_size: Some(per_page),
                row_count: Some(items.len()),
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
        let mut count_conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let count_result = count_query.build_query_scalar().fetch_one(&mut *count_conn).await;
        let total = count_conn.complete_query(count_result).await?;

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

        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let query_result = query.build().fetch_all(&mut *conn).await;
        let rows = conn.complete_query(query_result).await?;
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
        Self::push_alert_events_cte(query, filters);
        Self::push_alert_grouping_cte_tail(query, filters);
    }

    fn push_projected_alert_groups_cte<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        Self::push_projected_alert_events_cte(query, filters);
        Self::push_alert_grouping_cte_tail(query, filters);
    }

    fn push_alert_grouping_cte_tail<'a>(
        query: &mut QueryBuilder<'a, Sqlite>,
        filters: AlertEventFilters<'a>,
    ) {
        let subject_kind_sql = Self::alert_subject_kind_sql("alerts");
        let subject_id_sql = Self::alert_subject_id_sql("alerts");
        let request_kind_sql = Self::alert_group_request_kind_sql("alerts");
        let request_rate_minutes = request_rate_limit_window_minutes();
        let request_rate_seconds = request_rate_minutes * 60;
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
        let mut count_conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let count_result = count_query.build_query_scalar().fetch_one(&mut *count_conn).await;
        let total = count_conn.complete_query(count_result).await?;

        let mut query = QueryBuilder::new("");
        Self::push_alert_groups_cte(&mut query, filters);
        query.push(" SELECT * FROM grouped_alerts");
        query.push(
            " ORDER BY last_seen DESC, total_count DESC, alert_type DESC, row_sort_id DESC LIMIT ",
        );
        query.push_bind(per_page);
        query.push(" OFFSET ");
        query.push_bind(offset);

        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminAlertsRead)
            .await?;
        let query_result = query.build().fetch_all(&mut *conn).await;
        let rows = conn.complete_query(query_result).await?;
        let groups = rows
            .iter()
            .map(Self::decode_alert_group_projection_row)
            .collect::<Result<Vec<_>, _>>()?;
        let latest_events_by_row_sort_id = self
            .fetch_group_latest_events(filters, &groups, AlertReadSource::Raw)
            .await?;
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

    async fn fetch_projected_alert_group_page(
        &self,
        filters: AlertEventFilters<'_>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AlertGroupRecord>, i64), ProxyError> {
        self.fetch_projected_alert_group_page_for_operation(
            filters,
            page,
            per_page,
            SqliteOperation::AdminAlertsRead,
        )
        .await
    }

    async fn fetch_projected_alert_group_page_for_operation(
        &self,
        filters: AlertEventFilters<'_>,
        page: i64,
        per_page: i64,
        operation: SqliteOperation,
    ) -> Result<(Vec<AlertGroupRecord>, i64), ProxyError> {
        let started = Instant::now();
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;

        let mut count_query = QueryBuilder::new("");
        Self::push_projected_alert_groups_cte(&mut count_query, filters);
        count_query.push(" SELECT COUNT(*) FROM grouped_alerts");
        let mut count_conn = self
            .sqlite_runtime
            .acquire_operation_connection(operation)
            .await?;
        let count_result = count_query.build_query_scalar().fetch_one(&mut *count_conn).await;
        let total = count_conn.complete_query(count_result).await?;

        let mut query = QueryBuilder::new("");
        Self::push_projected_alert_groups_cte(&mut query, filters);
        query.push(" SELECT * FROM grouped_alerts");
        query.push(
            " ORDER BY last_seen DESC, total_count DESC, alert_type DESC, row_sort_id DESC LIMIT ",
        );
        query.push_bind(per_page);
        query.push(" OFFSET ");
        query.push_bind(offset);
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(operation)
            .await?;
        let result = query.build().fetch_all(&mut *conn).await;
        let rows = conn.complete_query(result).await?;
        let groups = rows
            .iter()
            .map(Self::decode_alert_group_projection_row)
            .collect::<Result<Vec<_>, _>>()?;
        let latest_events_by_row_sort_id = self
            .fetch_group_latest_events_for_operation(
                filters,
                &groups,
                AlertReadSource::Projected,
                operation,
            )
            .await?;
        let items = Self::build_alert_group_records(groups, latest_events_by_row_sort_id);
        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "alerts_grouping_sidecar",
            started.elapsed(),
            PerfLogScope {
                route: Some("/api/alerts/groups"),
                scope: Some("alerts"),
                phase: Some("projection_sidecar"),
                page_size: Some(per_page),
                row_count: Some(items.len()),
                ..Default::default()
            },
        );
        Ok((items, total))
    }

    async fn fetch_projected_alert_group_count_for_operation(
        &self,
        filters: AlertEventFilters<'_>,
        operation: SqliteOperation,
    ) -> Result<i64, ProxyError> {
        let mut query = QueryBuilder::new("");
        Self::push_projected_alert_groups_cte(&mut query, filters);
        query.push(" SELECT COUNT(*) FROM grouped_alerts");
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(operation)
            .await?;
        let result = query.build_query_scalar().fetch_one(&mut *conn).await;
        conn.complete_query(result).await
    }

    async fn populate_selected_mother_groups(
        &self,
        filters: AlertEventFilters<'_>,
        groups: Vec<AlertGroupRecord>,
        source: AlertReadSource,
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
        Self::push_alert_events_for_source_cte(&mut query, filters, source);
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

        let rows = self.fetch_alert_query_rows(query, source).await?;
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

    async fn populate_selected_projected_mother_groups_in_admin_session(
        &self,
        filters: AlertEventFilters<'_>,
        groups: Vec<AlertGroupRecord>,
        session: &mut AdminAlertsReadSession,
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
        let selected_subjects = selected_mother_keys.iter().cloned().collect::<Vec<_>>();
        if selected_subjects.is_empty() {
            return Ok(groups);
        }

        let mut query = QueryBuilder::new("");
        let subject_kind_sql = Self::alert_subject_kind_sql("alerts");
        let subject_id_sql = Self::alert_subject_id_sql("alerts");
        Self::push_projected_alert_events_cte(&mut query, filters);
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
                separated
                    .push_unseparated(subject_kind_sql.as_str())
                    .push_unseparated(" = ")
                    .push_bind_unseparated(subject_kind);
                separated.push_unseparated(" AND ");
                separated
                    .push_unseparated(subject_id_sql.as_str())
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
        query.push(") ORDER BY occurred_at DESC, row_sort_id DESC");

        let rows = self
            .fetch_projected_alert_query_rows_in_admin_session(session, query)
            .await?;
        let all_events = rows
            .into_iter()
            .map(Self::decode_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect::<Vec<_>>();
        let mut groups_by_mother_key = build_group_records_from_events(all_events)
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
            job_id,
            job_type,
            job_trigger_source,
            job_status,
            job_attempt,
            job_message,
            job_queued_at,
            job_started_at,
            job_finished_at,
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
        let job = build_alert_job_ref(
            job_id,
            job_type,
            job_trigger_source,
            job_status,
            job_attempt,
            job_message,
            job_queued_at,
            job_started_at,
            job_finished_at,
        );
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
            job.as_ref(),
        );
        let (title, summary) = build_alert_title_and_summary(
            resolved_alert_type.as_str(),
            AlertTitleSummaryContext {
                subject_label: subject_label.as_str(),
                token: token.as_ref(),
                key: key.as_ref(),
                job: job.as_ref(),
                request_kind: request_kind.as_ref(),
                error_message: error_message.as_deref(),
                reason_summary: reason_summary.as_deref(),
            },
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
            job,
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
        let filters = AlertEventFilters {
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            request_kinds,
        };
        if self.alert_projection_is_complete().await? {
            return self
                .fetch_projected_alert_events_page(filters, page, per_page)
                .await;
        }
        self.fetch_alert_event_projection_page(filters, page, per_page).await
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
        let source = if self.alert_projection_is_complete().await? {
            AlertReadSource::Projected
        } else {
            AlertReadSource::Raw
        };
        let (page_items, total) = match source {
            AlertReadSource::Raw => {
                self.fetch_alert_group_projection_page(filters, page, per_page).await?
            }
            AlertReadSource::Projected => {
                self.fetch_projected_alert_group_page(filters, page, per_page)
                    .await?
            }
        };
        let items = self
            .populate_selected_mother_groups(filters, page_items, source)
            .await?;
        Ok(PaginatedAlertGroups {
            items,
            total,
            page,
            per_page,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_admin_alert_groups_page(
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
        let filters = AlertEventFilters {
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            request_kinds,
        };
        let mut session = self.begin_admin_alerts_read_session().await?;
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let result = async {
            self.ensure_admin_alert_projection_coverage(&mut session).await?;
            let mut count_query = QueryBuilder::new("");
            Self::push_projected_alert_groups_cte(&mut count_query, filters);
            count_query.push(" SELECT COUNT(*) FROM grouped_alerts");
            let count_result = count_query.build_query_scalar().fetch_one(&mut *session).await;
            let total = session.query(count_result).await?;
            let mut query = QueryBuilder::new("");
            Self::push_projected_alert_groups_cte(&mut query, filters);
            query.push(" SELECT * FROM grouped_alerts");
            query.push(
                " ORDER BY last_seen DESC, total_count DESC, alert_type DESC, row_sort_id DESC LIMIT ",
            );
            query.push_bind(per_page);
            query.push(" OFFSET ");
            query.push_bind(offset);
            let query_result = query.build().fetch_all(&mut *session).await;
            let rows = session.query(query_result).await?;
            let groups = rows
                .iter()
                .map(Self::decode_alert_group_projection_row)
                .collect::<Result<Vec<_>, _>>()?;
            let latest_events = self
                .fetch_projected_group_latest_events_in_admin_session(filters, &groups, &mut session)
                .await?;
            let page_items = Self::build_alert_group_records(groups, latest_events);
            let items = self
                .populate_selected_projected_mother_groups_in_admin_session(
                    filters,
                    page_items,
                    &mut session,
                )
                .await?;
            Ok::<_, ProxyError>(PaginatedAlertGroups {
                items,
                total,
                page,
                per_page,
            })
        }
        .await;
        let finish_result = session.finish().await;
        finish_result?;
        result
    }

    pub(crate) async fn fetch_alert_catalog(&self) -> Result<AlertCatalog, ProxyError> {
        let source = if self.alert_projection_is_complete().await? {
            AlertReadSource::Projected
        } else {
            AlertReadSource::Raw
        };
        self.fetch_alert_catalog_from_source(source, None).await
    }

    pub(crate) async fn fetch_admin_alert_catalog(&self) -> Result<AlertCatalog, ProxyError> {
        let mut session = self.begin_admin_alerts_read_session().await?;
        self.ensure_admin_alert_projection_coverage(&mut session).await?;
        let result = self
            .fetch_alert_catalog_from_source(AlertReadSource::Projected, Some(&mut session))
            .await;
        let finish_result = session.finish().await;
        finish_result?;
        result
    }

    async fn fetch_alert_catalog_from_source(
        &self,
        source: AlertReadSource,
        mut session: Option<&mut AdminAlertsReadSession>,
    ) -> Result<AlertCatalog, ProxyError> {
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
        Self::push_alert_events_for_source_cte(&mut request_kind_query, filters, source);
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
        let request_kind_rows = match session.as_deref_mut() {
            Some(session) => self
                .fetch_projected_alert_query_rows_in_admin_session(session, request_kind_query)
                .await?,
            None => self.fetch_alert_query_rows(request_kind_query, source).await?,
        };
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
        Self::push_alert_events_for_source_cte(&mut users_query, filters, source);
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
        let users = match session.as_deref_mut() {
            Some(session) => self
                .fetch_projected_alert_query_rows_in_admin_session(session, users_query)
                .await?,
            None => self.fetch_alert_query_rows(users_query, source).await?,
        }
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
        Self::push_alert_events_for_source_cte(&mut tokens_query, filters, source);
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
        let tokens = match session.as_deref_mut() {
            Some(session) => self
                .fetch_projected_alert_query_rows_in_admin_session(session, tokens_query)
                .await?,
            None => self.fetch_alert_query_rows(tokens_query, source).await?,
        }
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
        Self::push_alert_events_for_source_cte(&mut keys_query, filters, source);
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
        let keys = match session.as_deref_mut() {
            Some(session) => self
                .fetch_projected_alert_query_rows_in_admin_session(session, keys_query)
                .await?,
            None => self.fetch_alert_query_rows(keys_query, source).await?,
        }
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
        Self::push_alert_events_for_source_cte(&mut types_query, filters, source);
        types_query.push(
            " SELECT alert_type, COUNT(*) AS count \
              FROM alerts \
              WHERE COALESCE(NULLIF(TRIM(alert_type), ''), '') <> '' \
              GROUP BY alert_type",
        );
        let types_rows = match session.as_deref_mut() {
            Some(session) => self
                .fetch_projected_alert_query_rows_in_admin_session(session, types_query)
                .await?,
            None => self.fetch_alert_query_rows(types_query, source).await?,
        };
        let types = Self::summarize_alert_type_count_rows(types_rows)
        .into_iter()
        .map(|value| LogFacetOption {
            value: value.alert_type,
            count: value.count,
        })
        .collect::<Vec<_>>();

        let retention_days = match session {
            Some(session) => self
                .effective_auth_token_log_retention_days_in_admin_session(session)
                .await?,
            None => self.effective_auth_token_log_retention_days().await?,
        };
        Ok(AlertCatalog {
            retention_days,
            types,
            request_kind_options,
            users,
            tokens,
            keys,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
        let (top_groups, grouped_count) = self.fetch_alert_group_projection_page(filters, 1, 10).await?;

        let mut grouped_count_windows = Vec::with_capacity(3);
        for grouped_window_hours in [1_i64, 24_i64, 24_i64 * 7] {
            let grouped_since = self
                .backend_time
                .now_ts()
                .saturating_sub(grouped_window_hours.saturating_mul(3600));
            let grouped_filters = AlertEventFilters {
                alert_type: None,
                since: Some(grouped_since),
                until: None,
                user_id: None,
                token_id: None,
                key_id: None,
                request_kinds: &[],
            };
            let mut grouped_count_query = QueryBuilder::new("");
            Self::push_alert_groups_cte(&mut grouped_count_query, grouped_filters);
            grouped_count_query.push(" SELECT COUNT(*) FROM grouped_alerts");
            grouped_count_windows.push(RecentAlertsGroupedWindowCount {
                window_hours: grouped_window_hours,
                grouped_count: grouped_count_query
                    .build_query_scalar()
                    .fetch_one(&self.pool)
                    .await?,
            });
        }

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
            grouped_count_windows,
            counts_by_type,
            top_groups,
            coverage: "ok".to_string(),
            stale: false,
            error: None,
        })
    }
}
