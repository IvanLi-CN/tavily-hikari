use crate::store::*;
use crate::*;
use chrono::Timelike;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub(crate) struct ApiKeyLease {
    pub(crate) id: String,
    pub(crate) secret: String,
}

pub(crate) struct AttemptLog<'a> {
    pub(crate) key_id: Option<&'a str>,
    pub(crate) auth_token_id: Option<&'a str>,
    pub(crate) method: &'a Method,
    pub(crate) path: &'a str,
    pub(crate) query: Option<&'a str>,
    pub(crate) status: Option<StatusCode>,
    pub(crate) tavily_status_code: Option<i64>,
    pub(crate) error: Option<&'a str>,
    pub(crate) request_body: &'a [u8],
    pub(crate) response_body: &'a [u8],
    pub(crate) outcome: &'a str,
    pub(crate) failure_kind: Option<&'a str>,
    pub(crate) key_effect_code: &'a str,
    pub(crate) key_effect_summary: Option<&'a str>,
    pub(crate) binding_effect_code: &'a str,
    pub(crate) binding_effect_summary: Option<&'a str>,
    pub(crate) selection_effect_code: &'a str,
    pub(crate) selection_effect_summary: Option<&'a str>,
    pub(crate) gateway_mode: Option<&'a str>,
    pub(crate) experiment_variant: Option<&'a str>,
    pub(crate) proxy_session_id: Option<&'a str>,
    pub(crate) routing_subject_hash: Option<&'a str>,
    pub(crate) upstream_operation: Option<&'a str>,
    pub(crate) fallback_reason: Option<&'a str>,
    pub(crate) forwarded_headers: &'a [String],
    pub(crate) dropped_headers: &'a [String],
}

/// 透传请求描述。
#[derive(Debug, Clone)]
pub struct ProxyRequest {
    pub method: Method,
    pub path: String,
    pub query: Option<String>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub auth_token_id: Option<String>,
    pub pinned_api_key_id: Option<String>,
    pub prefer_mcp_session_affinity: bool,
    pub gateway_mode: Option<String>,
    pub experiment_variant: Option<String>,
    pub proxy_session_id: Option<String>,
    pub routing_subject_hash: Option<String>,
    pub upstream_operation: Option<String>,
    pub fallback_reason: Option<String>,
}

/// 透传响应。
#[derive(Debug, Clone)]
pub struct ProxyResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub api_key_id: Option<String>,
    pub request_log_id: Option<i64>,
    pub key_effect_code: String,
    pub key_effect_summary: Option<String>,
    pub binding_effect_code: String,
    pub binding_effect_summary: Option<String>,
    pub selection_effect_code: String,
    pub selection_effect_summary: Option<String>,
}

/// Token quota verdict used by the HTTP layer to decide whether to forward.
#[derive(Debug, Clone)]
pub struct TokenQuotaVerdict {
    pub allowed: bool,
    pub exceeded_window: Option<QuotaWindow>,
    pub hourly_used: i64,
    pub hourly_limit: i64,
    pub daily_used: i64,
    pub daily_limit: i64,
    pub monthly_used: i64,
    pub monthly_limit: i64,
}

impl TokenQuotaVerdict {
    pub fn new(
        hourly_used_raw: i64,
        hourly_limit: i64,
        daily_used_raw: i64,
        daily_limit: i64,
        monthly_used_raw: i64,
        monthly_limit: i64,
    ) -> Self {
        let hourly_limit = hourly_limit.max(0);
        let daily_limit = daily_limit.max(0);
        let monthly_limit = monthly_limit.max(0);
        let hourly_used_raw = hourly_used_raw.max(0);
        let daily_used_raw = daily_used_raw.max(0);
        let monthly_used_raw = monthly_used_raw.max(0);

        let mut exceeded_window = None;
        let mut allowed = true;
        if hourly_limit == 0 || hourly_used_raw > hourly_limit {
            exceeded_window = Some(QuotaWindow::Hour);
            allowed = false;
        }
        if daily_limit == 0 || daily_used_raw > daily_limit {
            exceeded_window = Some(QuotaWindow::Day);
            allowed = false;
        }
        if monthly_limit == 0 || monthly_used_raw > monthly_limit {
            exceeded_window = Some(QuotaWindow::Month);
            allowed = false;
        }

        let hourly_used = min(hourly_used_raw, hourly_limit);
        let daily_used = min(daily_used_raw, daily_limit);
        let monthly_used = min(monthly_used_raw, monthly_limit);
        Self {
            allowed,
            exceeded_window,
            hourly_used,
            hourly_limit,
            daily_used,
            daily_limit,
            monthly_used,
            monthly_limit,
        }
    }

    pub(crate) fn effective_window(&self) -> Option<QuotaWindow> {
        if let Some(window) = self.exceeded_window {
            return Some(window);
        }

        // Snapshot-based enforcement blocks when counters are *at* the limit (>=),
        // so expose the same "exhausted window" for reporting/UI consistency.
        if self.monthly_used >= self.monthly_limit {
            return Some(QuotaWindow::Month);
        }
        if self.daily_used >= self.daily_limit {
            return Some(QuotaWindow::Day);
        }
        if self.hourly_used >= self.hourly_limit {
            return Some(QuotaWindow::Hour);
        }
        None
    }

    pub(crate) fn projected_window(&self, delta: i64) -> Option<QuotaWindow> {
        if let Some(window) = self.effective_window() {
            return Some(window);
        }
        if delta > 0 {
            if self.monthly_used.saturating_add(delta) > self.monthly_limit {
                return Some(QuotaWindow::Month);
            }
            if self.daily_used.saturating_add(delta) > self.daily_limit {
                return Some(QuotaWindow::Day);
            }
            if self.hourly_used.saturating_add(delta) > self.hourly_limit {
                return Some(QuotaWindow::Hour);
            }
        }
        None
    }

    pub fn window_name(&self) -> Option<&'static str> {
        self.effective_window().map(|w| w.as_str())
    }

    pub fn window_name_for_delta(&self, delta: i64) -> Option<&'static str> {
        self.projected_window(delta).map(|w| w.as_str())
    }

    pub fn state_key(&self) -> &'static str {
        self.window_name().unwrap_or("normal")
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillingLedgerWindowSnapshot {
    pub ledger_credits: i64,
    pub quota_credits: i64,
    pub diff_credits: i64,
}

impl BillingLedgerWindowSnapshot {
    pub(crate) fn new(ledger_credits: i64, quota_credits: i64) -> Self {
        Self {
            ledger_credits,
            quota_credits,
            diff_credits: quota_credits - ledger_credits,
        }
    }

    pub(crate) fn is_match(&self) -> bool {
        self.diff_credits == 0
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillingLedgerAuditEntry {
    pub billing_subject: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub hour: BillingLedgerWindowSnapshot,
    pub day: BillingLedgerWindowSnapshot,
    pub month: BillingLedgerWindowSnapshot,
}

impl BillingLedgerAuditEntry {
    pub(crate) fn has_mismatch(&self) -> bool {
        !(self.hour.is_match() && self.day.is_match() && self.month.is_match())
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillingLedgerAuditSummary {
    pub generated_at: i64,
    pub minute_bucket_start: i64,
    pub hour_bucket_start: i64,
    pub hour_window_start: i64,
    pub day_window_start: i64,
    pub month_window_start: i64,
    pub current_month_charged_rows: i64,
    pub current_month_charged_credits: i64,
    pub subject_count: usize,
    pub mismatched_subjects: usize,
    pub hour_only_mismatches: usize,
    pub day_only_mismatches: usize,
    pub month_only_mismatches: usize,
    pub mixed_mismatches: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BillingLedgerAuditReport {
    pub summary: BillingLedgerAuditSummary,
    pub entries: Vec<BillingLedgerAuditEntry>,
}

impl BillingLedgerAuditReport {
    pub fn has_mismatches(&self) -> bool {
        self.summary.mismatched_subjects > 0
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyQuotaRebaseReport {
    pub current_month_start: i64,
    pub previous_rebase_month_start: Option<i64>,
    pub current_month_charged_rows: i64,
    pub current_month_charged_credits: i64,
    pub rebased_subject_count: usize,
    pub rebased_token_subjects: usize,
    pub rebased_account_subjects: usize,
    pub cleared_token_rows: i64,
    pub cleared_account_rows: i64,
    pub meta_updated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestRateScope {
    User,
    Token,
}

impl RequestRateScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Token => "token",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRateView {
    pub used: i64,
    pub limit: i64,
    pub window_minutes: i64,
    pub scope: RequestRateScope,
}

/// Lightweight verdict for the rolling request-rate limiter that counts all
/// authenticated requests for a bound user or unbound token subject.
#[derive(Debug, Clone)]
pub struct TokenHourlyRequestVerdict {
    pub allowed: bool,
    pub hourly_used: i64,
    pub hourly_limit: i64,
    pub window_minutes: i64,
    pub scope: RequestRateScope,
    pub retry_after_seconds: i64,
}

impl TokenHourlyRequestVerdict {
    pub fn new(
        hourly_used_raw: i64,
        hourly_limit: i64,
        window_minutes: i64,
        scope: RequestRateScope,
        retry_after_seconds: i64,
    ) -> Self {
        let hourly_limit = hourly_limit.max(0);
        let hourly_used_raw = hourly_used_raw.max(0);
        let allowed = hourly_limit > 0 && hourly_used_raw <= hourly_limit;
        let hourly_used = std::cmp::min(hourly_used_raw, hourly_limit);
        Self {
            allowed,
            hourly_used,
            hourly_limit,
            window_minutes: window_minutes.max(1),
            scope,
            retry_after_seconds: retry_after_seconds.max(0),
        }
    }

    pub fn request_rate(&self) -> RequestRateView {
        RequestRateView {
            used: self.hourly_used,
            limit: self.hourly_limit,
            window_minutes: self.window_minutes,
            scope: self.scope,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaWindow {
    Hour,
    Day,
    Month,
}

impl QuotaWindow {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuotaWindow::Hour => "hour",
            QuotaWindow::Day => "day",
            QuotaWindow::Month => "month",
        }
    }
}

/// 每个 API key 的聚合统计信息。
#[derive(Debug, Clone)]
pub struct ApiKeyMetrics {
    pub id: String,
    pub status: String,
    pub group_name: Option<String>,
    pub registration_ip: Option<String>,
    pub registration_region: Option<String>,
    pub status_changed_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub deleted_at: Option<i64>,
    pub quota_limit: Option<i64>,
    pub quota_remaining: Option<i64>,
    pub quota_synced_at: Option<i64>,
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub quota_exhausted_count: i64,
    pub quarantine: Option<ApiKeyQuarantine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyFacetCount {
    pub value: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyListFacets {
    pub groups: Vec<ApiKeyFacetCount>,
    pub statuses: Vec<ApiKeyFacetCount>,
    pub regions: Vec<ApiKeyFacetCount>,
}

#[derive(Debug, Clone)]
pub struct PaginatedApiKeyMetrics {
    pub items: Vec<ApiKeyMetrics>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub facets: ApiKeyListFacets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyQuarantine {
    pub source: String,
    pub reason_code: String,
    pub reason_summary: String,
    pub reason_detail: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEffect {
    pub code: String,
    pub summary: Option<String>,
}

impl KeyEffect {
    pub(crate) fn none() -> Self {
        Self {
            code: KEY_EFFECT_NONE.to_string(),
            summary: None,
        }
    }

    pub(crate) fn new(code: &str, summary: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            summary: Some(summary.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiKeyMaintenanceRecord {
    pub id: String,
    pub key_id: String,
    pub source: String,
    pub operation_code: String,
    pub operation_summary: String,
    pub reason_code: Option<String>,
    pub reason_summary: Option<String>,
    pub reason_detail: Option<String>,
    pub request_log_id: Option<i64>,
    pub auth_token_log_id: Option<i64>,
    pub auth_token_id: Option<String>,
    pub actor_user_id: Option<String>,
    pub actor_display_name: Option<String>,
    pub status_before: Option<String>,
    pub status_after: Option<String>,
    pub quarantine_before: bool,
    pub quarantine_after: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceActor {
    pub auth_token_id: Option<String>,
    pub actor_user_id: Option<String>,
    pub actor_display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyStateSnapshot {
    pub status: Option<String>,
    pub quarantined: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenPrimaryApiKeyAffinity {
    pub token_id: String,
    pub user_id: Option<String>,
    pub api_key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpProjectAffinityBinding {
    pub owner_subject: String,
    pub project_id_hash: String,
    pub api_key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpProjectAffinityContext {
    pub owner_subject: String,
    pub project_id_hash: String,
    pub affinity_subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSessionBinding {
    pub proxy_session_id: String,
    pub upstream_session_id: Option<String>,
    pub upstream_key_id: Option<String>,
    pub auth_token_id: Option<String>,
    pub user_id: Option<String>,
    pub protocol_version: Option<String>,
    pub last_event_id: Option<String>,
    pub gateway_mode: String,
    pub experiment_variant: String,
    pub ab_bucket: Option<i64>,
    pub routing_subject_hash: Option<String>,
    pub fallback_reason: Option<String>,
    pub rate_limited_until: Option<i64>,
    pub last_rate_limited_at: Option<i64>,
    pub last_rate_limit_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub revoke_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettings {
    pub request_rate_limit: i64,
    pub mcp_session_affinity_key_count: i64,
    pub rebalance_mcp_enabled: bool,
    pub rebalance_mcp_session_percent: i64,
}

/// 单条请求日志记录的关键信息。
#[derive(Debug, Clone)]
pub struct RequestLogRecord {
    pub id: i64,
    pub key_id: Option<String>,
    pub auth_token_id: Option<String>,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub status_code: Option<i64>,
    pub tavily_status_code: Option<i64>,
    pub error_message: Option<String>,
    pub business_credits: Option<i64>,
    pub request_kind_key: String,
    pub request_kind_label: String,
    pub request_kind_detail: Option<String>,
    pub request_kind_protocol_group: String,
    pub request_kind_billing_group: String,
    pub result_status: String,
    pub failure_kind: Option<String>,
    pub key_effect_code: String,
    pub key_effect_summary: Option<String>,
    pub binding_effect_code: String,
    pub binding_effect_summary: Option<String>,
    pub selection_effect_code: String,
    pub selection_effect_summary: Option<String>,
    pub gateway_mode: Option<String>,
    pub experiment_variant: Option<String>,
    pub proxy_session_id: Option<String>,
    pub routing_subject_hash: Option<String>,
    pub upstream_operation: Option<String>,
    pub fallback_reason: Option<String>,
    pub operational_class: String,
    pub request_body: Vec<u8>,
    pub response_body: Vec<u8>,
    pub created_at: i64,
    pub forwarded_headers: Vec<String>,
    pub dropped_headers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestLogBodiesRecord {
    pub request_body: Option<Vec<u8>>,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFacetOption {
    pub value: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestLogPageFacets {
    pub results: Vec<LogFacetOption>,
    pub key_effects: Vec<LogFacetOption>,
    pub binding_effects: Vec<LogFacetOption>,
    pub selection_effects: Vec<LogFacetOption>,
    pub tokens: Vec<LogFacetOption>,
    pub keys: Vec<LogFacetOption>,
}

#[derive(Debug, Clone)]
pub struct RequestLogsPage {
    pub items: Vec<RequestLogRecord>,
    pub total: i64,
    pub request_kind_options: Vec<TokenRequestKindOption>,
    pub facets: RequestLogPageFacets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestLogsCursorDirection {
    Older,
    Newer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLogsCursor {
    pub created_at: i64,
    pub id: i64,
}

#[derive(Debug, Clone)]
pub struct RequestLogsCursorPage {
    pub items: Vec<RequestLogRecord>,
    pub page_size: i64,
    pub next_cursor: Option<RequestLogsCursor>,
    pub prev_cursor: Option<RequestLogsCursor>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, Clone)]
pub struct TokenLogsCursorPage {
    pub items: Vec<TokenLogRecord>,
    pub page_size: i64,
    pub next_cursor: Option<RequestLogsCursor>,
    pub prev_cursor: Option<RequestLogsCursor>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, Clone)]
pub struct RequestLogsCatalog {
    pub retention_days: i64,
    pub request_kind_options: Vec<TokenRequestKindOption>,
    pub facets: RequestLogPageFacets,
}

pub const ALERT_TYPE_UPSTREAM_RATE_LIMITED_429: &str = "upstream_rate_limited_429";
pub const ALERT_TYPE_UPSTREAM_KEY_BLOCKED: &str = "upstream_key_blocked";
pub const ALERT_TYPE_USER_REQUEST_RATE_LIMITED: &str = "user_request_rate_limited";
pub const ALERT_TYPE_USER_QUOTA_EXHAUSTED: &str = "user_quota_exhausted";

pub const ALERT_SOURCE_AUTH_TOKEN_LOG: &str = "auth_token_log";
pub const ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD: &str = "api_key_maintenance_record";

pub const ALERT_SUBJECT_USER: &str = "user";
pub const ALERT_SUBJECT_TOKEN: &str = "token";
pub const ALERT_SUBJECT_KEY: &str = "key";

pub fn is_supported_alert_type(value: &str) -> bool {
    matches!(
        value,
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429
            | ALERT_TYPE_UPSTREAM_KEY_BLOCKED
            | ALERT_TYPE_USER_REQUEST_RATE_LIMITED
            | ALERT_TYPE_USER_QUOTA_EXHAUSTED
    )
}

pub fn default_alert_type_counts() -> Vec<AlertTypeCount> {
    [
        ALERT_TYPE_UPSTREAM_RATE_LIMITED_429,
        ALERT_TYPE_UPSTREAM_KEY_BLOCKED,
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
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
    pub request: Option<AlertRequestRef>,
    pub request_kind: Option<TokenRequestKind>,
    pub failure_kind: Option<String>,
    pub result_status: Option<String>,
    pub error_message: Option<String>,
    pub reason_code: Option<String>,
    pub reason_summary: Option<String>,
    pub reason_detail: Option<String>,
    pub source: AlertSourceRef,
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
    pub request_kind: Option<TokenRequestKind>,
    pub count: i64,
    pub first_seen: i64,
    pub last_seen: i64,
    pub latest_event: AlertEventRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaginatedAlertGroups {
    pub items: Vec<AlertGroupRecord>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertFacetOption {
    pub value: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertCatalog {
    pub retention_days: i64,
    pub types: Vec<LogFacetOption>,
    pub request_kind_options: Vec<TokenRequestKindOption>,
    pub users: Vec<AlertFacetOption>,
    pub tokens: Vec<AlertFacetOption>,
    pub keys: Vec<AlertFacetOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertTypeCount {
    pub alert_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentAlertsSummary {
    pub window_hours: i64,
    pub total_events: i64,
    pub grouped_count: i64,
    pub counts_by_type: Vec<AlertTypeCount>,
    pub top_groups: Vec<AlertGroupRecord>,
}

/// 汇总统计信息，用于展示整体代理运行状况。
#[derive(Debug, Clone)]
pub struct ProxySummary {
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub quota_exhausted_count: i64,
    pub active_keys: i64,
    pub exhausted_keys: i64,
    pub quarantined_keys: i64,
    pub last_activity: Option<i64>,
    pub total_quota_limit: i64,
    pub total_quota_remaining: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryQuotaCharge {
    pub local_estimated_credits: i64,
    pub upstream_actual_credits: i64,
    pub sampled_key_count: i64,
    pub stale_key_count: i64,
    pub latest_sync_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryWindowMetrics {
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub quota_exhausted_count: i64,
    pub valuable_success_count: i64,
    pub valuable_failure_count: i64,
    pub other_success_count: i64,
    pub other_failure_count: i64,
    pub unknown_count: i64,
    pub upstream_exhausted_key_count: i64,
    pub new_keys: i64,
    pub new_quarantines: i64,
    pub quota_charge: SummaryQuotaCharge,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryWindows {
    pub today: SummaryWindowMetrics,
    pub yesterday: SummaryWindowMetrics,
    pub month: SummaryWindowMetrics,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardHourlyRequestBucket {
    pub bucket_start: i64,
    pub secondary_success: i64,
    pub primary_success: i64,
    pub secondary_failure: i64,
    pub primary_failure_429: i64,
    pub primary_failure_other: i64,
    pub unknown: i64,
    pub mcp_non_billable: i64,
    pub mcp_billable: i64,
    pub api_non_billable: i64,
    pub api_billable: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardHourlyRequestWindow {
    pub bucket_seconds: i64,
    pub visible_buckets: i64,
    pub retained_buckets: i64,
    pub buckets: Vec<DashboardHourlyRequestBucket>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardProxyDashboardSummary {
    pub available_nodes: i64,
    pub total_nodes: i64,
}

/// Successful request counters for public metrics.
#[derive(Debug, Clone)]
pub struct SuccessBreakdown {
    pub monthly_success: i64,
    pub daily_success: i64,
}

/// Background job log record for scheduled tasks
#[derive(Debug, Clone)]
pub struct JobLog {
    pub id: i64,
    pub job_type: String,
    pub key_id: Option<String>,
    pub key_group: Option<String>,
    pub status: String,
    pub attempt: i64,
    pub message: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobGroupCounts {
    pub all: i64,
    pub quota: i64,
    pub usage: i64,
    pub logs: i64,
    pub geo: i64,
    pub linuxdo: i64,
}

pub(crate) fn random_string(alphabet: &[u8], len: usize) -> String {
    let mut s = String::with_capacity(len);
    let mut rng = rand::thread_rng();
    for _ in 0..len {
        let idx = rng.gen_range(0..alphabet.len());
        s.push(alphabet[idx] as char);
    }
    s
}

/// Token list record for management UI
#[derive(Debug, Clone)]
pub struct AuthToken {
    pub id: String, // 4-char id code
    pub enabled: bool,
    pub note: Option<String>,
    pub group_name: Option<String>,
    pub total_requests: i64,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub quota: Option<TokenQuotaVerdict>,
    pub quota_hourly_reset_at: Option<i64>,
    pub quota_daily_reset_at: Option<i64>,
    pub quota_monthly_reset_at: Option<i64>,
}

/// Full token for copy (never store prefix-only here)
#[derive(Debug, Clone)]
pub struct AuthTokenSecret {
    pub id: String,
    pub token: String, // th-<id>-<secret>
}

#[derive(Debug, Clone)]
pub struct AdminQuotaLimitSet {
    pub hourly_any_limit: i64,
    pub hourly_limit: i64,
    pub daily_limit: i64,
    pub monthly_limit: i64,
    pub inherits_defaults: bool,
}

#[derive(Debug, Clone)]
pub struct AdminUserTag {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub system_key: Option<String>,
    pub effect_kind: String,
    pub hourly_any_delta: i64,
    pub hourly_delta: i64,
    pub daily_delta: i64,
    pub monthly_delta: i64,
    pub user_count: i64,
}

#[derive(Debug, Clone)]
pub struct AdminUserTagBinding {
    pub tag_id: String,
    pub name: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub system_key: Option<String>,
    pub effect_kind: String,
    pub hourly_any_delta: i64,
    pub hourly_delta: i64,
    pub daily_delta: i64,
    pub monthly_delta: i64,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct AdminUserQuotaBreakdownEntry {
    pub kind: String,
    pub label: String,
    pub tag_id: Option<String>,
    pub tag_name: Option<String>,
    pub source: Option<String>,
    pub effect_kind: String,
    pub hourly_any_delta: i64,
    pub hourly_delta: i64,
    pub daily_delta: i64,
    pub monthly_delta: i64,
}

#[derive(Debug, Clone)]
pub struct AdminUserQuotaDetails {
    pub base: AdminQuotaLimitSet,
    pub effective: AdminQuotaLimitSet,
    pub breakdown: Vec<AdminUserQuotaBreakdownEntry>,
    pub tags: Vec<AdminUserTagBinding>,
}

#[derive(Debug, Clone)]
pub struct UserDashboardSummary {
    pub request_rate: RequestRateView,
    pub hourly_any_used: i64,
    pub hourly_any_limit: i64,
    pub quota_hourly_used: i64,
    pub quota_hourly_limit: i64,
    pub quota_daily_used: i64,
    pub quota_daily_limit: i64,
    pub quota_monthly_used: i64,
    pub quota_monthly_limit: i64,
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct UserLogMetricsSummary {
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenLogMetricsSummary {
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRangeUtc {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone)]
pub struct AdminUserIdentity {
    pub user_id: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub active: bool,
    pub last_login_at: Option<i64>,
    pub token_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyBrokenKeyRelatedUser {
    pub user_id: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonthlyBrokenKeyDetail {
    pub key_id: String,
    pub current_status: String,
    pub reason_code: Option<String>,
    pub reason_summary: Option<String>,
    pub latest_break_at: i64,
    pub source: String,
    pub breaker_token_id: Option<String>,
    pub breaker_user_id: Option<String>,
    pub breaker_user_display_name: Option<String>,
    pub manual_actor_display_name: Option<String>,
    pub related_users: Vec<MonthlyBrokenKeyRelatedUser>,
}

#[derive(Debug, Clone)]
pub struct PaginatedMonthlyBrokenKeys {
    pub items: Vec<MonthlyBrokenKeyDetail>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StickyCreditsWindow {
    pub success_credits: i64,
    pub failure_credits: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyUserUsageBucket {
    pub bucket_start: i64,
    pub bucket_end: i64,
    pub success_credits: i64,
    pub failure_credits: i64,
}

#[derive(Debug, Clone)]
pub struct ApiKeyStickyUser {
    pub user: AdminUserIdentity,
    pub last_success_at: i64,
    pub yesterday: StickyCreditsWindow,
    pub today: StickyCreditsWindow,
    pub month: StickyCreditsWindow,
    pub daily_buckets: Vec<ApiKeyUserUsageBucket>,
}

#[derive(Debug, Clone)]
pub struct PaginatedApiKeyStickyUsers {
    pub items: Vec<ApiKeyStickyUser>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone)]
pub struct UserTokenSummary {
    pub token_id: String,
    pub enabled: bool,
    pub note: Option<String>,
    pub last_used_at: Option<i64>,
    pub request_rate: RequestRateView,
    pub hourly_any_used: i64,
    pub hourly_any_limit: i64,
    pub quota_hourly_used: i64,
    pub quota_hourly_limit: i64,
    pub quota_daily_used: i64,
    pub quota_daily_limit: i64,
    pub quota_monthly_used: i64,
    pub quota_monthly_limit: i64,
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
}

/// Third-party profile normalized for local account upsert.
#[derive(Debug, Clone)]
pub struct OAuthAccountProfile {
    pub provider: String,
    pub provider_user_id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub avatar_template: Option<String>,
    pub active: bool,
    pub trust_level: Option<i64>,
    pub raw_payload_json: Option<String>,
}

/// OAuth account record that is eligible for refresh-token based profile sync.
#[derive(Debug, Clone)]
pub struct OAuthAccountRefreshTokenRecord {
    pub provider: String,
    pub provider_user_id: String,
    pub user_id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub refresh_token_ciphertext: String,
    pub refresh_token_nonce: String,
}

/// Local user identity resolved from oauth_accounts/users.
#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub user_id: String,
    pub provider: String,
    pub provider_user_id: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub avatar_template: Option<String>,
}

/// Persisted user session record.
#[derive(Debug, Clone)]
pub struct UserSession {
    pub token: String,
    pub user: UserIdentity,
    pub expires_at: i64,
}

/// User-facing token lookup status for `/api/user/token`.
#[derive(Debug, Clone)]
pub enum UserTokenLookup {
    Found(AuthTokenSecret),
    MissingBinding,
    Unavailable,
}

/// Payload returned from OAuth state consume operation.
#[derive(Debug, Clone)]
pub struct OAuthLoginStatePayload {
    pub redirect_to: Option<String>,
    pub bind_token_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRequestKind {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
}

impl TokenRequestKind {
    pub(crate) fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        detail: Option<String>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            detail: detail.and_then(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRequestKindOption {
    pub key: String,
    pub label: String,
    pub protocol_group: String,
    pub billing_group: String,
    pub count: i64,
}

/// Per-token log for detail UI
#[derive(Debug, Clone)]
pub struct TokenLogRecord {
    pub id: i64,
    pub key_id: Option<String>,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub http_status: Option<i64>,
    pub mcp_status: Option<i64>,
    pub business_credits: Option<i64>,
    pub request_kind_key: String,
    pub request_kind_label: String,
    pub request_kind_detail: Option<String>,
    pub counts_business_quota: bool,
    pub result_status: String,
    pub error_message: Option<String>,
    pub failure_kind: Option<String>,
    pub key_effect_code: String,
    pub key_effect_summary: Option<String>,
    pub binding_effect_code: String,
    pub binding_effect_summary: Option<String>,
    pub selection_effect_code: String,
    pub selection_effect_summary: Option<String>,
    pub gateway_mode: Option<String>,
    pub experiment_variant: Option<String>,
    pub proxy_session_id: Option<String>,
    pub routing_subject_hash: Option<String>,
    pub upstream_operation: Option<String>,
    pub fallback_reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct TokenLogsPage {
    pub items: Vec<TokenLogRecord>,
    pub total: i64,
    pub request_kind_options: Vec<TokenRequestKindOption>,
    pub facets: RequestLogPageFacets,
}

/// Token summary for period view
#[derive(Debug, Clone)]
pub struct TokenSummary {
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub quota_exhausted_count: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TokenUsageBucket {
    pub bucket_start: i64,
    pub success_count: i64,
    pub system_failure_count: i64,
    pub external_failure_count: i64,
}

/// Hourly aggregated counts for charting.
#[derive(Debug, Clone)]
pub struct TokenHourlyBucket {
    pub bucket_start: i64,
    pub success_count: i64,
    pub system_failure_count: i64,
    pub external_failure_count: i64,
}

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("invalid upstream endpoint '{endpoint}': {source}")]
    InvalidEndpoint {
        endpoint: String,
        #[source]
        source: url::ParseError,
    },
    #[error("no API keys available in the store")]
    NoAvailableKeys,
    #[error("pinned MCP session key is unavailable")]
    PinnedMcpSessionUnavailable,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("http error: {0}")]
    Http(reqwest::Error),
    #[error("missing usage data: {reason}")]
    QuotaDataMissing { reason: String },
    #[error("usage http error {status}: {body}")]
    UsageHttp {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("other error: {0}")]
    Other(String),
}

pub async fn audit_business_quota_ledger(
    database_path: &str,
    now: chrono::DateTime<Utc>,
) -> Result<BillingLedgerAuditReport, ProxyError> {
    let pool = open_sqlite_pool(database_path, false, true).await?;
    audit_business_quota_ledger_with_pool(&pool, now).await
}

pub async fn rebase_current_month_business_quota(
    database_path: &str,
    now: chrono::DateTime<Utc>,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    let pool = open_sqlite_pool(database_path, false, false).await?;
    rebase_current_month_business_quota_with_pool(
        &pool,
        now,
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        true,
    )
    .await
}

pub(crate) fn map_forward_proxy_validation_error_code(error: &ProxyError) -> String {
    match error {
        ProxyError::Http(err) => {
            if err.is_timeout() {
                "proxy_timeout".to_string()
            } else {
                "proxy_unreachable".to_string()
            }
        }
        ProxyError::Other(message) => {
            if message.contains("xray") {
                "xray_missing".to_string()
            } else if message.contains("subscription resolved zero proxy entries")
                || message.contains("subscription contains no supported proxy entries")
            {
                "subscription_invalid".to_string()
            } else if message.contains("subscription") {
                "subscription_unreachable".to_string()
            } else if message.contains("timeout") {
                "proxy_timeout".to_string()
            } else {
                "validation_failed".to_string()
            }
        }
        _ => "validation_failed".to_string(),
    }
}

pub(crate) fn parse_forward_proxy_trace_response(body: &str) -> Option<(String, String)> {
    let mut ip: Option<String> = None;
    let mut country: Option<String> = None;
    let mut colo: Option<String> = None;
    for line in body.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let normalized = value.trim();
        if normalized.is_empty() {
            continue;
        }
        match key.trim() {
            "ip" => ip = Some(normalized.to_string()),
            "loc" => country = Some(normalized.to_string()),
            "colo" => colo = Some(normalized.to_string()),
            _ => {}
        }
    }

    let ip = normalize_ip_string(&ip?)?;
    let location = match (country, colo) {
        (Some(country), Some(colo)) => format!("{country} / {colo}"),
        (Some(country), None) => country,
        (None, Some(colo)) => colo,
        (None, None) => return None,
    };

    Some((ip, location))
}

pub(crate) fn start_of_month(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
}

pub(crate) fn start_of_local_month_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let first_day = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .expect("valid start of month date");
    let naive = first_day
        .and_hms_opt(0, 0, 0)
        .expect("valid start of month time");
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::None => {
            // Extremely unlikely at midnight; fall back to current timestamp.
            now.with_timezone(&Utc).timestamp()
        }
    }
}

pub(crate) fn start_of_next_month(
    current_month_start: chrono::DateTime<Utc>,
) -> chrono::DateTime<Utc> {
    let (year, month) = if current_month_start.month() == 12 {
        (current_month_start.year() + 1, 1)
    } else {
        (current_month_start.year(), current_month_start.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .expect("valid start of next month")
}

#[derive(Debug, Clone, Copy)]
struct BillingLedgerWindows {
    generated_at: i64,
    minute_bucket_start: i64,
    hour_bucket_start: i64,
    hour_window_start: i64,
    day_window_start: i64,
    day_window_end: i64,
    month_window_start: i64,
}

impl BillingLedgerWindows {
    pub(crate) fn from_now(now: chrono::DateTime<Utc>) -> Self {
        let generated_at = now.timestamp();
        let minute_bucket_start = generated_at - (generated_at % SECS_PER_MINUTE);
        let hour_bucket_start = generated_at - (generated_at % SECS_PER_HOUR);
        let day_window = server_local_day_window_utc(now.with_timezone(&Local));
        Self {
            generated_at,
            minute_bucket_start,
            hour_bucket_start,
            hour_window_start: minute_bucket_start - 59 * SECS_PER_MINUTE,
            day_window_start: day_window.start,
            day_window_end: day_window.end,
            month_window_start: start_of_month(now).timestamp(),
        }
    }
}

#[derive(Debug, Default)]
struct BillingLedgerAccumulator {
    hour_ledger: i64,
    day_ledger: i64,
    month_ledger: i64,
    hour_quota: i64,
    day_quota: i64,
    month_quota: i64,
}

impl BillingLedgerAccumulator {
    pub(crate) fn has_any_value(&self) -> bool {
        self.hour_ledger != 0
            || self.day_ledger != 0
            || self.month_ledger != 0
            || self.hour_quota != 0
            || self.day_quota != 0
            || self.month_quota != 0
    }
}

pub(crate) fn billing_subject_parts(subject: &str) -> Result<(&'static str, &str), ProxyError> {
    if let Some(user_id) = subject.strip_prefix("account:") {
        Ok(("account", user_id))
    } else if let Some(token_id) = subject.strip_prefix("token:") {
        Ok(("token", token_id))
    } else {
        Err(ProxyError::QuotaDataMissing {
            reason: format!("invalid billing subject: {subject}"),
        })
    }
}

pub(crate) async fn get_meta_i64_executor<'e, E>(
    executor: E,
    key: &str,
) -> Result<Option<i64>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let value = sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(key)
        .fetch_optional(executor)
        .await?;
    Ok(value.and_then(|raw| raw.parse::<i64>().ok()))
}

pub(crate) async fn set_meta_i64_executor<'e, E>(
    executor: E,
    key: &str,
    value: i64,
) -> Result<(), ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value.to_string())
    .execute(executor)
    .await?;
    Ok(())
}

pub(crate) async fn ensure_charged_subjects_are_valid<'e, E>(
    executor: E,
    window_start: i64,
    generated_at: i64,
) -> Result<(), ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let invalid_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM auth_token_logs
        WHERE billing_state = ?
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
          AND (
            billing_subject IS NULL
            OR (
                billing_subject NOT LIKE 'token:%'
                AND billing_subject NOT LIKE 'account:%'
            )
          )
        "#,
    )
    .bind(BILLING_STATE_CHARGED)
    .bind(window_start)
    .bind(generated_at)
    .fetch_one(executor)
    .await?;

    if invalid_count > 0 {
        return Err(ProxyError::QuotaDataMissing {
            reason: format!(
                "found {invalid_count} charged auth_token_logs rows with invalid billing_subject between {window_start} and {generated_at}"
            ),
        });
    }

    Ok(())
}

pub(crate) async fn fetch_current_month_charged_totals<'e, E>(
    executor: E,
    current_month_start: i64,
    generated_at: i64,
) -> Result<(i64, i64), ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*) AS charged_rows,
            COALESCE(SUM(business_credits), 0) AS charged_credits
        FROM auth_token_logs
        WHERE billing_state = ?
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        "#,
    )
    .bind(BILLING_STATE_CHARGED)
    .bind(current_month_start)
    .bind(generated_at)
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub(crate) async fn fetch_charged_ledger_window<'e, E>(
    executor: E,
    window_start: i64,
    generated_at: i64,
) -> Result<Vec<(String, i64, i64)>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT
            billing_subject,
            COALESCE(SUM(business_credits), 0) AS total_credits,
            COUNT(*) AS charged_rows
        FROM auth_token_logs
        WHERE billing_state = ?
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        GROUP BY billing_subject
        ORDER BY billing_subject ASC
        "#,
    )
    .bind(BILLING_STATE_CHARGED)
    .bind(window_start)
    .bind(generated_at)
    .fetch_all(executor)
    .await
    .map_err(ProxyError::Database)
}

pub(crate) async fn fetch_token_quota_window<'e, E>(
    executor: E,
    granularity: &str,
    window_start: i64,
    window_end: i64,
) -> Result<Vec<(String, i64)>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT
            token_id,
            COALESCE(SUM(count), 0) AS total_credits
        FROM token_usage_buckets
        WHERE granularity = ?
          AND bucket_start >= ?
          AND bucket_start <= ?
        GROUP BY token_id
        ORDER BY token_id ASC
        "#,
    )
    .bind(granularity)
    .bind(window_start)
    .bind(window_end)
    .fetch_all(executor)
    .await
    .map_err(ProxyError::Database)
}

pub(crate) async fn fetch_account_quota_window<'e, E>(
    executor: E,
    granularity: &str,
    window_start: i64,
    window_end: i64,
) -> Result<Vec<(String, i64)>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT
            user_id,
            COALESCE(SUM(count), 0) AS total_credits
        FROM account_usage_buckets
        WHERE granularity = ?
          AND bucket_start >= ?
          AND bucket_start <= ?
        GROUP BY user_id
        ORDER BY user_id ASC
        "#,
    )
    .bind(granularity)
    .bind(window_start)
    .bind(window_end)
    .fetch_all(executor)
    .await
    .map_err(ProxyError::Database)
}

pub(crate) async fn fetch_token_monthly_quota_rows<'e, E>(
    executor: E,
) -> Result<Vec<(String, i64, i64)>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT token_id, month_start, month_count
        FROM auth_token_quota
        ORDER BY token_id ASC
        "#,
    )
    .fetch_all(executor)
    .await
    .map_err(ProxyError::Database)
}

pub(crate) async fn fetch_account_monthly_quota_rows<'e, E>(
    executor: E,
) -> Result<Vec<(String, i64, i64)>, ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT user_id, month_start, month_count
        FROM account_monthly_quota
        ORDER BY user_id ASC
        "#,
    )
    .fetch_all(executor)
    .await
    .map_err(ProxyError::Database)
}

pub(crate) async fn audit_business_quota_ledger_with_pool(
    pool: &SqlitePool,
    now: chrono::DateTime<Utc>,
) -> Result<BillingLedgerAuditReport, ProxyError> {
    let mut conn = begin_read_snapshot_sqlite_connection(pool).await?;
    let windows = BillingLedgerWindows::from_now(now);
    let result = async {
        ensure_charged_subjects_are_valid(
            &mut *conn,
            std::cmp::min(windows.day_window_start, windows.month_window_start),
            windows.generated_at,
        )
        .await?;

        let mut subjects: BTreeMap<String, BillingLedgerAccumulator> = BTreeMap::new();
        let (current_month_charged_rows, current_month_charged_credits) =
            fetch_current_month_charged_totals(
                &mut *conn,
                windows.month_window_start,
                windows.generated_at,
            )
            .await?;

        for (subject, total_credits, _row_count) in
            fetch_charged_ledger_window(&mut *conn, windows.hour_window_start, windows.generated_at)
                .await?
        {
            subjects.entry(subject).or_default().hour_ledger = total_credits;
        }
        for (subject, total_credits, _row_count) in
            fetch_charged_ledger_window(&mut *conn, windows.day_window_start, windows.generated_at)
                .await?
        {
            subjects.entry(subject).or_default().day_ledger = total_credits;
        }
        for (subject, total_credits, _row_count) in fetch_charged_ledger_window(
            &mut *conn,
            windows.month_window_start,
            windows.generated_at,
        )
        .await?
        {
            subjects.entry(subject).or_default().month_ledger = total_credits;
        }

        for (token_id, total_credits) in fetch_token_quota_window(
            &mut *conn,
            GRANULARITY_MINUTE,
            windows.hour_window_start,
            windows.minute_bucket_start,
        )
        .await?
        {
            subjects
                .entry(format!("token:{token_id}"))
                .or_default()
                .hour_quota = total_credits;
        }
        for (user_id, total_credits) in fetch_account_quota_window(
            &mut *conn,
            GRANULARITY_MINUTE,
            windows.hour_window_start,
            windows.minute_bucket_start,
        )
        .await?
        {
            subjects
                .entry(format!("account:{user_id}"))
                .or_default()
                .hour_quota = total_credits;
        }

        for (token_id, total_credits) in fetch_token_quota_window(
            &mut *conn,
            GRANULARITY_DAY,
            windows.day_window_start,
            windows.day_window_start,
        )
        .await?
        {
            subjects
                .entry(format!("token:{token_id}"))
                .or_default()
                .day_quota = total_credits;
        }
        for (token_id, total_credits) in fetch_token_quota_window(
            &mut *conn,
            GRANULARITY_HOUR,
            windows.day_window_start,
            windows.day_window_end.saturating_sub(1),
        )
        .await?
        {
            subjects
                .entry(format!("token:{token_id}"))
                .or_default()
                .day_quota += total_credits;
        }
        for (user_id, total_credits) in fetch_account_quota_window(
            &mut *conn,
            GRANULARITY_DAY,
            windows.day_window_start,
            windows.day_window_start,
        )
        .await?
        {
            subjects
                .entry(format!("account:{user_id}"))
                .or_default()
                .day_quota = total_credits;
        }
        for (user_id, total_credits) in fetch_account_quota_window(
            &mut *conn,
            GRANULARITY_HOUR,
            windows.day_window_start,
            windows.day_window_end.saturating_sub(1),
        )
        .await?
        {
            subjects
                .entry(format!("account:{user_id}"))
                .or_default()
                .day_quota += total_credits;
        }

        for (token_id, stored_month_start, month_count) in
            fetch_token_monthly_quota_rows(&mut *conn).await?
        {
            let effective_count = if stored_month_start >= windows.month_window_start {
                month_count
            } else {
                0
            };
            if effective_count != 0 {
                subjects
                    .entry(format!("token:{token_id}"))
                    .or_default()
                    .month_quota = effective_count;
            }
        }
        for (user_id, stored_month_start, month_count) in
            fetch_account_monthly_quota_rows(&mut *conn).await?
        {
            let effective_count = if stored_month_start >= windows.month_window_start {
                month_count
            } else {
                0
            };
            if effective_count != 0 {
                subjects
                    .entry(format!("account:{user_id}"))
                    .or_default()
                    .month_quota = effective_count;
            }
        }

        let mut entries = Vec::new();
        let mut mismatched_subjects = 0_usize;
        let mut hour_only_mismatches = 0_usize;
        let mut day_only_mismatches = 0_usize;
        let mut month_only_mismatches = 0_usize;
        let mut mixed_mismatches = 0_usize;

        for (billing_subject, totals) in subjects {
            if !totals.has_any_value() {
                continue;
            }

            let (subject_kind, subject_id) = billing_subject_parts(&billing_subject)?;
            let subject_kind = subject_kind.to_string();
            let subject_id = subject_id.to_string();
            let entry = BillingLedgerAuditEntry {
                billing_subject,
                subject_kind,
                subject_id,
                hour: BillingLedgerWindowSnapshot::new(totals.hour_ledger, totals.hour_quota),
                day: BillingLedgerWindowSnapshot::new(totals.day_ledger, totals.day_quota),
                month: BillingLedgerWindowSnapshot::new(totals.month_ledger, totals.month_quota),
            };

            let hour_mismatch = !entry.hour.is_match();
            let day_mismatch = !entry.day.is_match();
            let month_mismatch = !entry.month.is_match();
            if entry.has_mismatch() {
                mismatched_subjects += 1;
                match (hour_mismatch, day_mismatch, month_mismatch) {
                    (true, false, false) => hour_only_mismatches += 1,
                    (false, true, false) => day_only_mismatches += 1,
                    (false, false, true) => month_only_mismatches += 1,
                    _ => mixed_mismatches += 1,
                }
            }
            entries.push(entry);
        }

        let subject_count = entries.len();

        Ok(BillingLedgerAuditReport {
            summary: BillingLedgerAuditSummary {
                generated_at: windows.generated_at,
                minute_bucket_start: windows.minute_bucket_start,
                hour_bucket_start: windows.hour_bucket_start,
                hour_window_start: windows.hour_window_start,
                day_window_start: windows.day_window_start,
                month_window_start: windows.month_window_start,
                current_month_charged_rows,
                current_month_charged_credits,
                subject_count,
                mismatched_subjects,
                hour_only_mismatches,
                day_only_mismatches,
                month_only_mismatches,
                mixed_mismatches,
            },
            entries,
        })
    }
    .await;

    let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;

    result
}

pub(crate) async fn rebase_current_month_business_quota_with_pool(
    pool: &SqlitePool,
    now: chrono::DateTime<Utc>,
    meta_key: &str,
    update_meta: bool,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    let mut conn = begin_immediate_sqlite_connection(pool).await?;
    let locked_now = Utc::now();
    let windows = BillingLedgerWindows::from_now(if locked_now > now { locked_now } else { now });

    let result = async {
        ensure_charged_subjects_are_valid(
            &mut *conn,
            windows.month_window_start,
            windows.generated_at,
        )
        .await?;

        let previous_rebase_month_start = get_meta_i64_executor(&mut *conn, meta_key).await?;
        let (current_month_charged_rows, current_month_charged_credits) =
            fetch_current_month_charged_totals(
                &mut *conn,
                windows.month_window_start,
                windows.generated_at,
            )
            .await?;
        let rebased_subjects = fetch_charged_ledger_window(
            &mut *conn,
            windows.month_window_start,
            windows.generated_at,
        )
        .await?;

        let cleared_token_rows =
            sqlx::query("UPDATE auth_token_quota SET month_start = ?, month_count = 0")
                .bind(windows.month_window_start)
                .execute(&mut *conn)
                .await?
                .rows_affected() as i64;
        let cleared_account_rows =
            sqlx::query("UPDATE account_monthly_quota SET month_start = ?, month_count = 0")
                .bind(windows.month_window_start)
                .execute(&mut *conn)
                .await?
                .rows_affected() as i64;

        let mut rebased_token_subjects = 0_usize;
        let mut rebased_account_subjects = 0_usize;
        for (billing_subject, total_credits, _row_count) in rebased_subjects.iter() {
            match QuotaSubject::from_billing_subject(billing_subject)? {
                QuotaSubject::Token(token_id) => {
                    sqlx::query(
                        r#"
                        INSERT INTO auth_token_quota (token_id, month_start, month_count)
                        VALUES (?, ?, ?)
                        ON CONFLICT(token_id) DO UPDATE SET
                            month_start = excluded.month_start,
                            month_count = excluded.month_count
                        "#,
                    )
                    .bind(&token_id)
                    .bind(windows.month_window_start)
                    .bind(*total_credits)
                    .execute(&mut *conn)
                    .await?;
                    rebased_token_subjects += 1;
                }
                QuotaSubject::Account(user_id) => {
                    sqlx::query(
                        r#"
                        INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                        VALUES (?, ?, ?)
                        ON CONFLICT(user_id) DO UPDATE SET
                            month_start = excluded.month_start,
                            month_count = excluded.month_count
                        "#,
                    )
                    .bind(&user_id)
                    .bind(windows.month_window_start)
                    .bind(*total_credits)
                    .execute(&mut *conn)
                    .await?;
                    rebased_account_subjects += 1;
                }
            }
        }

        let meta_updated =
            update_meta && previous_rebase_month_start != Some(windows.month_window_start);
        if update_meta {
            set_meta_i64_executor(&mut *conn, meta_key, windows.month_window_start).await?;
        }

        Ok(MonthlyQuotaRebaseReport {
            current_month_start: windows.month_window_start,
            previous_rebase_month_start,
            current_month_charged_rows,
            current_month_charged_credits,
            rebased_subject_count: rebased_subjects.len(),
            rebased_token_subjects,
            rebased_account_subjects,
            cleared_token_rows,
            cleared_account_rows,
            meta_updated,
        })
    }
    .await;

    let report = match result {
        Ok(report) => report,
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            return Err(err);
        }
    };

    if let Err(err) = sqlx::query("COMMIT").execute(&mut *conn).await {
        let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        return Err(ProxyError::Database(err));
    }

    Ok(report)
}

pub(crate) fn local_date_start_utc_ts(
    date: chrono::NaiveDate,
    fallback_now: chrono::DateTime<Local>,
) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("valid start of local day");
    local_naive_datetime_utc_ts(naive, fallback_now)
}

pub(crate) fn local_naive_datetime_utc_ts(
    naive: chrono::NaiveDateTime,
    fallback_now: chrono::DateTime<Local>,
) -> i64 {
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::None => {
            // Extremely unlikely for the local datetimes we use here; fall back to current timestamp.
            fallback_now.with_timezone(&Utc).timestamp()
        }
    }
}

pub(crate) fn start_of_local_day_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    local_date_start_utc_ts(now.date_naive(), now)
}

pub(crate) fn start_of_local_hour_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let naive = now
        .date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .expect("valid start of local hour");
    local_naive_datetime_utc_ts(naive, now)
}

pub(crate) fn previous_local_day_start_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let previous_date = now
        .date_naive()
        .pred_opt()
        .unwrap_or_else(|| now.date_naive());
    local_date_start_utc_ts(previous_date, now)
}

pub(crate) fn previous_local_same_time_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let previous_date = now
        .date_naive()
        .pred_opt()
        .unwrap_or_else(|| now.date_naive());
    let naive = previous_date.and_time(now.time());
    local_naive_datetime_utc_ts(naive, now)
}

pub(crate) fn local_day_bucket_start_utc_ts(created_at_utc_ts: i64) -> i64 {
    let Some(utc_dt) = Utc.timestamp_opt(created_at_utc_ts, 0).single() else {
        return 0;
    };
    start_of_local_day_utc_ts(utc_dt.with_timezone(&Local))
}

pub(crate) fn next_local_day_start_utc_ts(current_day_start_utc_ts: i64) -> i64 {
    let Some(utc_dt) = Utc.timestamp_opt(current_day_start_utc_ts, 0).single() else {
        return current_day_start_utc_ts.saturating_add(SECS_PER_DAY);
    };
    let local_dt = utc_dt.with_timezone(&Local);
    let next_date = local_dt
        .date_naive()
        .succ_opt()
        .unwrap_or_else(|| local_dt.date_naive());
    local_date_start_utc_ts(next_date, local_dt)
}

pub(crate) fn server_local_day_window_utc(now: chrono::DateTime<Local>) -> TimeRangeUtc {
    let start = start_of_local_day_utc_ts(now);
    let end = next_local_day_start_utc_ts(start);
    TimeRangeUtc { start, end }
}

pub fn parse_explicit_today_window(
    today_start: Option<&str>,
    today_end: Option<&str>,
) -> Result<Option<TimeRangeUtc>, String> {
    let normalized_start = today_start.map(str::trim).filter(|value| !value.is_empty());
    let normalized_end = today_end.map(str::trim).filter(|value| !value.is_empty());
    match (normalized_start, normalized_end) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            Err("today_start and today_end must be provided together".to_string())
        }
        (Some(raw_start), Some(raw_end)) => {
            let start = chrono::DateTime::parse_from_rfc3339(raw_start).map_err(|_| {
                "today_start must be a valid ISO8601 datetime with offset".to_string()
            })?;
            let end = chrono::DateTime::parse_from_rfc3339(raw_end).map_err(|_| {
                "today_end must be a valid ISO8601 datetime with offset".to_string()
            })?;
            if end <= start {
                return Err("today_end must be later than today_start".to_string());
            }
            if start.time() != chrono::NaiveTime::MIN || end.time() != chrono::NaiveTime::MIN {
                return Err("today_start and today_end must align to local midnight".to_string());
            }
            let duration = end.signed_duration_since(start);
            if duration < chrono::Duration::hours(23) || duration > chrono::Duration::hours(25) {
                return Err(
                    "today_start and today_end must describe exactly one natural-day window"
                        .to_string(),
                );
            }
            let next_date = start
                .date_naive()
                .succ_opt()
                .ok_or_else(|| "today_start must be a single natural-day window".to_string())?;
            if end.date_naive() != next_date {
                return Err(
                    "today_start and today_end must describe exactly one natural-day window"
                        .to_string(),
                );
            }
            Ok(Some(TimeRangeUtc {
                start: start.with_timezone(&Utc).timestamp(),
                end: end.with_timezone(&Utc).timestamp(),
            }))
        }
    }
}

pub(crate) fn request_logs_retention_threshold_utc_ts(retention_days: i64) -> i64 {
    let days = retention_days.max(REQUEST_LOGS_MIN_RETENTION_DAYS);
    let today = Local::now().date_naive();
    let keep_from_date = today
        .checked_sub_days(chrono::Days::new((days - 1) as u64))
        .unwrap_or(today);
    let naive = keep_from_date
        .and_hms_opt(0, 0, 0)
        .expect("valid local midnight");
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::None => Local::now().with_timezone(&Utc).timestamp(),
    }
}

pub(crate) fn normalize_timestamp(timestamp: i64) -> Option<i64> {
    if timestamp <= 0 {
        None
    } else {
        Some(timestamp)
    }
}

pub(crate) fn preview_key(key: &str) -> String {
    let shown = min(6, key.len());
    format!("{}…", &key[..shown])
}

pub(crate) fn compose_path(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{}?{}", path, q),
        _ => path.to_owned(),
    }
}

pub(crate) fn log_success(
    key: &str,
    method: &Method,
    path: &str,
    query: Option<&str>,
    status: StatusCode,
) {
    let key_preview = preview_key(key);
    let full_path = compose_path(path, query);
    println!("[{key_preview}] {method} {full_path} -> {status}");
}

pub(crate) fn log_error(
    key: &str,
    method: &Method,
    path: &str,
    query: Option<&str>,
    err: &reqwest::Error,
) {
    let key_preview = preview_key(key);
    let full_path = compose_path(path, query);
    eprintln!("[{key_preview}] {method} {full_path} !! {err}");
}

pub(crate) fn log_proxy_error(
    key: &str,
    method: &Method,
    path: &str,
    query: Option<&str>,
    err: &ProxyError,
) {
    match err {
        ProxyError::Http(source) => log_error(key, method, path, query, source),
        _ => {
            let key_preview = preview_key(key);
            let full_path = compose_path(path, query);
            eprintln!("[{key_preview}] {method} {full_path} !! {err}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineDecision {
    pub reason_code: String,
    pub reason_summary: String,
    pub reason_detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyHealthAction {
    None,
    MarkExhausted,
    Quarantine(QuarantineDecision),
}

#[derive(Debug, Clone)]
pub struct AttemptAnalysis {
    pub status: &'static str,
    pub tavily_status_code: Option<i64>,
    pub key_health_action: KeyHealthAction,
    pub failure_kind: Option<String>,
    pub key_effect: KeyEffect,
    pub api_key_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageOutcome {
    Success,
    Error,
    QuotaExhausted,
}
