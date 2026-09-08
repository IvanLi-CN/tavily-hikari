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

impl AlertEventFilters<'_> {
    fn is_unfiltered(self) -> bool {
        self.alert_type.is_none()
            && self.since.is_none()
            && self.until.is_none()
            && self.user_id.is_none()
            && self.token_id.is_none()
            && self.key_id.is_none()
            && self.request_kinds.is_empty()
    }
}

#[derive(Clone)]
enum AlertReadSource {
    Raw,
    Projected,
    CanonicalSnapshot(i64),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    job_id: Option<i64>,
    job_type: Option<String>,
    job_trigger_source: Option<String>,
    job_status: Option<String>,
    job_attempt: Option<i64>,
    job_message: Option<String>,
    job_queued_at: Option<i64>,
    job_started_at: Option<i64>,
    job_finished_at: Option<i64>,
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
