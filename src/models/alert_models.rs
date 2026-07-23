use super::TokenRequestKind;

pub const ALERT_TYPE_UPSTREAM_RATE_LIMITED_429: &str = "upstream_rate_limited_429";
pub const ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432: &str = "upstream_usage_limit_432";
pub const ALERT_TYPE_UPSTREAM_KEY_BLOCKED: &str = "upstream_key_blocked";
pub const ALERT_TYPE_USER_REQUEST_RATE_LIMITED: &str = "user_request_rate_limited";
pub const ALERT_TYPE_USER_QUOTA_EXHAUSTED: &str = "user_quota_exhausted";
pub const ALERT_TYPE_API_KEY_EXHAUSTED: &str = "api_key_exhausted";
pub const ALERT_TYPE_JOB_FAILED: &str = "job_failed";

pub const ALERT_SOURCE_AUTH_TOKEN_LOG: &str = "auth_token_log";
pub const ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD: &str = "api_key_maintenance_record";
pub const ALERT_SOURCE_SCHEDULED_JOB: &str = "scheduled_job";

pub const ALERT_SUBJECT_USER: &str = "user";
pub const ALERT_SUBJECT_TOKEN: &str = "token";
pub const ALERT_SUBJECT_KEY: &str = "key";
pub const ALERT_SUBJECT_JOB: &str = "job";

pub fn is_supported_alert_type(value: &str) -> bool {
    matches!(
        value,
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429
            | ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432
            | ALERT_TYPE_UPSTREAM_KEY_BLOCKED
            | ALERT_TYPE_USER_REQUEST_RATE_LIMITED
            | ALERT_TYPE_USER_QUOTA_EXHAUSTED
            | ALERT_TYPE_API_KEY_EXHAUSTED
            | ALERT_TYPE_JOB_FAILED
    )
}

pub fn default_alert_type_counts() -> Vec<AlertTypeCount> {
    [
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429,
        ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432,
        ALERT_TYPE_UPSTREAM_KEY_BLOCKED,
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
        ALERT_TYPE_API_KEY_EXHAUSTED,
        ALERT_TYPE_JOB_FAILED,
    ]
    .into_iter()
    .map(|alert_type| AlertTypeCount {
        alert_type: alert_type.to_string(),
        count: 0,
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertEntityRef {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertUserRef {
    pub user_id: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRequestRef {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertSourceRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertJobRef {
    pub id: i64,
    pub job_type: String,
    pub trigger_source: String,
    pub status: String,
    pub attempt: i64,
    pub message: Option<String>,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSemanticWindowKind {
    RequestRate,
    RollingHour,
    Day,
    Month,
}

impl AlertSemanticWindowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RequestRate => "request_rate",
            Self::RollingHour => "rolling_hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertSemanticWindow {
    pub kind: AlertSemanticWindowKind,
    pub window_minutes: Option<i64>,
    pub window_start: Option<i64>,
    pub window_end: Option<i64>,
    pub window_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertEventRecord {
    pub id: String,
    pub alert_type: String,
    pub title: String,
    pub summary: String,
    pub occurred_at: i64,
    pub subject_kind: String,
    pub subject_id: String,
    pub subject_label: String,
    pub user: Option<AlertUserRef>,
    pub token: Option<AlertEntityRef>,
    pub key: Option<AlertEntityRef>,
    pub job: Option<AlertJobRef>,
    pub request: Option<AlertRequestRef>,
    pub request_kind: Option<TokenRequestKind>,
    pub failure_kind: Option<String>,
    pub result_status: Option<String>,
    pub error_message: Option<String>,
    pub reason_code: Option<String>,
    pub reason_summary: Option<String>,
    pub reason_detail: Option<String>,
    pub source: AlertSourceRef,
    pub semantic_window: Option<AlertSemanticWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaginatedAlertEvents {
    pub items: Vec<AlertEventRecord>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertGroupRecord {
    pub id: String,
    pub alert_type: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub subject_label: String,
    pub user: Option<AlertUserRef>,
    pub token: Option<AlertEntityRef>,
    pub key: Option<AlertEntityRef>,
    pub job: Option<AlertJobRef>,
    pub request_kind: Option<TokenRequestKind>,
    pub count: i64,
    pub first_seen: i64,
    pub last_seen: i64,
    pub latest_event: AlertEventRecord,
    pub grouping_kind: String,
    pub semantic_window_kind: Option<String>,
    pub semantic_window_minutes: Option<i64>,
    pub semantic_window_start: Option<i64>,
    pub semantic_window_end: Option<i64>,
    pub semantic_window_key: Option<String>,
    pub child_count: i64,
    pub event_count: i64,
    pub children: Vec<AlertGroupRecord>,
    pub child_events: Vec<AlertEventRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaginatedAlertGroups {
    pub items: Vec<AlertGroupRecord>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertTypeCount {
    pub alert_type: String,
    pub count: i64,
}
