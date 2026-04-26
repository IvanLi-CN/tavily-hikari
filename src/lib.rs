mod analysis;
mod forward_proxy;
mod models;
mod store;
mod tavily_proxy;
#[cfg(test)]
mod tests;

pub use analysis::{
    analyze_http_attempt, analyze_mcp_attempt, canonical_request_kind_key_for_filter,
    canonicalize_request_log_request_kind, classify_token_request_kind,
    display_result_status_for_request_kind, extract_mcp_has_error_by_id_from_bytes,
    extract_mcp_usage_credits_by_id_from_bytes, extract_usage_credits_from_json_bytes,
    extract_usage_credits_total_from_json_bytes, failure_kind_solution_guidance,
    finalize_token_request_kind, is_canonical_request_kind_key, mcp_response_has_any_error,
    mcp_response_has_any_success, normalize_operational_class_filter,
    operational_class_for_request_kind, operational_class_for_request_log,
    operational_class_for_request_path, operational_class_for_token_log,
    should_append_solution_guidance, token_request_kind_billing_group,
    token_request_kind_billing_group_for_request, token_request_kind_billing_group_for_request_log,
    token_request_kind_billing_group_for_token_log, token_request_kind_protocol_group,
};
pub use forward_proxy::{
    ForwardProxyHourlyBucketResponse, ForwardProxyLiveNodeResponse, ForwardProxyLiveStatsResponse,
    ForwardProxySettings, ForwardProxySettingsResponse, ForwardProxyStatsResponse,
    ForwardProxyValidationError, ForwardProxyValidationNodeResult,
    ForwardProxyValidationProbeResult, ForwardProxyValidationResponse,
    ForwardProxyWeightHourlyBucketResponse,
};
pub use models::*;
pub use tavily_proxy::*;

use std::{
    cmp::min,
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use chrono::{Datelike, Local, TimeZone, Utc};
use futures_util::{StreamExt, TryStreamExt};
use nanoid::nanoid;
use rand::Rng;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{CONTENT_LENGTH, HOST, HeaderMap, HeaderValue},
};
use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Executor, QueryBuilder, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use tokio::sync::{Mutex, Notify, RwLock};
use url::form_urlencoded;

pub type ForwardProxyProgressCallback = dyn Fn(ForwardProxyProgressEvent) + Send + Sync;

#[derive(Debug, Clone, Default)]
pub struct ForwardProxyCancellation {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl ForwardProxyCancellation {
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, AtomicOrdering::SeqCst) {
            self.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(AtomicOrdering::SeqCst)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            self.notify.notified().await;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ForwardProxyProgressEvent {
    Phase {
        operation: &'static str,
        #[serde(rename = "phaseKey")]
        phase_key: &'static str,
        label: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        current: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Complete {
        operation: &'static str,
        payload: Value,
    },
    Nodes {
        operation: &'static str,
        nodes: Vec<ForwardProxyProgressNodeState>,
    },
    Node {
        operation: &'static str,
        node: ForwardProxyProgressNodeState,
    },
    Error {
        operation: &'static str,
        message: String,
        #[serde(rename = "phaseKey")]
        #[serde(skip_serializing_if = "Option::is_none")]
        phase_key: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        current: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyProgressNodeState {
    pub node_key: String,
    pub display_name: String,
    pub protocol: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestKindCanonicalBackfillTableReport {
    pub table: &'static str,
    pub meta_key: &'static str,
    pub dry_run: bool,
    pub batch_size: i64,
    pub cursor_before: i64,
    pub cursor_after: i64,
    pub rows_scanned: i64,
    pub rows_updated: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestKindCanonicalBackfillReport {
    pub dry_run: bool,
    pub batch_size: i64,
    pub request_logs: RequestKindCanonicalBackfillTableReport,
    pub auth_token_logs: RequestKindCanonicalBackfillTableReport,
}

impl ForwardProxyProgressEvent {
    pub fn phase(operation: &'static str, phase_key: &'static str, label: &'static str) -> Self {
        Self::Phase {
            operation,
            phase_key,
            label,
            current: None,
            total: None,
            detail: None,
        }
    }

    pub fn phase_with_progress(
        operation: &'static str,
        phase_key: &'static str,
        label: &'static str,
        current: usize,
        total: usize,
        detail: Option<String>,
    ) -> Self {
        Self::Phase {
            operation,
            phase_key,
            label,
            current: Some(current),
            total: Some(total),
            detail,
        }
    }

    pub fn complete(operation: &'static str, payload: Value) -> Self {
        Self::Complete { operation, payload }
    }

    pub fn nodes(operation: &'static str, nodes: Vec<ForwardProxyProgressNodeState>) -> Self {
        Self::Nodes { operation, nodes }
    }

    pub fn node(operation: &'static str, node: ForwardProxyProgressNodeState) -> Self {
        Self::Node { operation, node }
    }

    pub fn error(
        operation: &'static str,
        message: impl Into<String>,
        phase_key: Option<&'static str>,
        label: Option<&'static str>,
        current: Option<usize>,
        total: Option<usize>,
        detail: Option<String>,
    ) -> Self {
        Self::Error {
            operation,
            message: message.into(),
            phase_key,
            label,
            current,
            total,
            detail,
        }
    }
}

fn emit_forward_proxy_progress(
    progress: Option<&ForwardProxyProgressCallback>,
    event: ForwardProxyProgressEvent,
) {
    if let Some(progress) = progress {
        progress(event);
    }
}

fn forward_proxy_cancelled_error() -> ProxyError {
    ProxyError::Other("forward proxy validation cancelled".to_string())
}

fn ensure_forward_proxy_not_cancelled(
    cancellation: Option<&ForwardProxyCancellation>,
) -> Result<(), ProxyError> {
    if cancellation.is_some_and(ForwardProxyCancellation::is_cancelled) {
        return Err(forward_proxy_cancelled_error());
    }
    Ok(())
}

async fn run_forward_proxy_future_with_cancel<T, Fut>(
    cancellation: Option<&ForwardProxyCancellation>,
    future: Fut,
) -> Result<T, ProxyError>
where
    Fut: Future<Output = T>,
{
    if let Some(cancellation) = cancellation {
        tokio::select! {
            _ = cancellation.cancelled() => Err(forward_proxy_cancelled_error()),
            value = future => Ok(value),
        }
    } else {
        Ok(future.await)
    }
}

fn compute_latency_median(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[middle])
    } else {
        Some((sorted[middle - 1] + sorted[middle]) / 2.0)
    }
}

/// Tavily MCP upstream默认端点。
pub const DEFAULT_UPSTREAM: &str = "https://mcp.tavily.com/mcp";

fn join_url_paths(prefix_path: &str, appended_path: &str) -> String {
    let prefix = prefix_path.trim_matches('/');
    let appended = appended_path.trim_matches('/');
    match (prefix.is_empty(), appended.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{appended}"),
        (false, true) => format!("/{prefix}"),
        (false, false) => format!("/{prefix}/{appended}"),
    }
}

pub(crate) fn build_path_prefixed_url(base: &Url, appended_path: &str) -> Url {
    let mut url = base.clone();
    url.set_path(&join_url_paths(base.path(), appended_path));
    url
}

pub(crate) fn build_mcp_upstream_url(base: &Url, request_path: &str) -> Url {
    if matches!(base.path(), "" | "/") {
        return build_path_prefixed_url(base, request_path);
    }

    let appended_path = if request_path == "/mcp" {
        ""
    } else if let Some(relative) = request_path.strip_prefix("/mcp/") {
        relative
    } else {
        request_path
    };

    build_path_prefixed_url(base, appended_path)
}

const STATUS_ACTIVE: &str = "active";
const STATUS_EXHAUSTED: &str = "exhausted";
const STATUS_DISABLED: &str = "disabled";
const QUARANTINE_REASON_DETAIL_MAX_LEN: usize = 1024;

const OUTCOME_SUCCESS: &str = "success";
const OUTCOME_ERROR: &str = "error";
const OUTCOME_QUOTA_EXHAUSTED: &str = "quota_exhausted";
const OUTCOME_UNKNOWN: &str = "unknown";
pub const REQUEST_LOG_VISIBILITY_VISIBLE: &str = "visible";
pub const REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW: &str = "suppressed_retry_shadow";
pub const REQUEST_KIND_CANONICAL_BACKFILL_BATCH_SIZE: i64 = 500;
const REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS: u64 = 200;
const REQUEST_KIND_CANONICAL_MIGRATION_STALE_SECS: i64 = 300;
const FAILURE_KIND_UPSTREAM_GATEWAY_5XX: &str = "upstream_gateway_5xx";
const FAILURE_KIND_UPSTREAM_RATE_LIMITED_429: &str = "upstream_rate_limited_429";
const FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401: &str = "upstream_account_deactivated_401";
const FAILURE_KIND_TRANSPORT_SEND_ERROR: &str = "transport_send_error";
const FAILURE_KIND_MCP_ACCEPT_406: &str = "mcp_accept_406";
const FAILURE_KIND_MCP_METHOD_405: &str = "mcp_method_405";
const FAILURE_KIND_MCP_PATH_404: &str = "mcp_path_404";
const FAILURE_KIND_TOOL_ARGUMENT_VALIDATION: &str = "tool_argument_validation";
const FAILURE_KIND_UNKNOWN_TOOL_NAME: &str = "unknown_tool_name";
const FAILURE_KIND_INVALID_SEARCH_DEPTH: &str = "invalid_search_depth";
const FAILURE_KIND_INVALID_COUNTRY_SEARCH_DEPTH_COMBO: &str = "invalid_country_search_depth_combo";
const FAILURE_KIND_RESEARCH_PAYLOAD_422: &str = "research_payload_422";
const FAILURE_KIND_QUERY_TOO_LONG: &str = "query_too_long";
const FAILURE_KIND_OTHER: &str = "other";
const KEY_EFFECT_NONE: &str = "none";
const KEY_EFFECT_QUARANTINED: &str = "quarantined";
const KEY_EFFECT_MARKED_EXHAUSTED: &str = "marked_exhausted";
const KEY_EFFECT_RESTORED_ACTIVE: &str = "restored_active";
const KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET: &str = "mcp_session_init_backoff_set";
const KEY_EFFECT_MCP_SESSION_RETRY_WAITED: &str = "mcp_session_retry_waited";
const KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED: &str = "mcp_session_retry_scheduled";
const KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED: &str = "mcp_session_init_cooldown_avoided";
const KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED: &str = "mcp_session_init_rate_limit_avoided";
const KEY_EFFECT_MCP_SESSION_INIT_PRESSURE_AVOIDED: &str = "mcp_session_init_pressure_avoided";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED: &str = "http_project_affinity_reused";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND: &str = "http_project_affinity_bound";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND: &str = "http_project_affinity_rebound";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED: &str =
    "http_project_affinity_cooldown_avoided";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED: &str =
    "http_project_affinity_rate_limit_avoided";
const KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED: &str =
    "http_project_affinity_pressure_avoided";
const MAINTENANCE_SOURCE_SYSTEM: &str = "system";
const MAINTENANCE_SOURCE_ADMIN: &str = "admin";
const MAINTENANCE_OP_AUTO_QUARANTINE: &str = "auto_quarantine";
const MAINTENANCE_OP_AUTO_MARK_EXHAUSTED: &str = "auto_mark_exhausted";
const MAINTENANCE_OP_AUTO_RESTORE_ACTIVE: &str = "auto_restore_active";
const MAINTENANCE_OP_MANUAL_CLEAR_QUARANTINE: &str = "manual_clear_quarantine";
const MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED: &str = "manual_mark_exhausted";
const API_KEY_IP_GEO_BATCH_FIELDS: &str = "?fields=city,subdivision,asn";
const API_KEY_IP_GEO_BATCH_SIZE: usize = 100;
const API_KEY_IP_GEO_HTTP_TIMEOUT_SECS: u64 = 10;
const API_KEY_IP_GEO_CONNECT_TIMEOUT_SECS: u64 = 5;

// dev-open-admin mode uses a synthetic token id ("dev") for request attribution.
// Keep a placeholder row in auth_tokens so SQLite FOREIGN KEY constraints in
// token_usage_buckets / auth_token_quota / token_usage_stats never fail.
const DEV_OPEN_ADMIN_TOKEN_ID: &str = "dev";
const DEV_OPEN_ADMIN_TOKEN_SECRET: &str = "dev-open-admin";
const DEV_OPEN_ADMIN_TOKEN_NOTE: &str = "[system] dev-open-admin placeholder";
pub const USER_MONTHLY_BROKEN_LIMIT_DEFAULT: i64 = 5;
pub const UNBOUND_TOKEN_MONTHLY_BROKEN_LIMIT_DEFAULT: i64 = 2;
const BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED: &str = "account_deactivated";
const BLOCKED_KEY_REASON_KEY_REVOKED: &str = "key_revoked";
const BLOCKED_KEY_REASON_INVALID_API_KEY: &str = "invalid_api_key";
const MCP_SESSION_INIT_BACKOFF_SCOPE: &str = "mcp_session_init";
const HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE: &str = "http_project_affinity";
const MCP_SESSION_INIT_BACKOFF_DEFAULT_SECS: i64 = 60;
const MCP_SESSION_INIT_BACKOFF_MIN_SECS: i64 = 30;
const MCP_SESSION_INIT_BACKOFF_MAX_SECS: i64 = 300;
const MCP_SESSION_INIT_RECENT_PRESSURE_WINDOW_SECS: i64 = 60;
const HTTP_PROJECT_AFFINITY_RECENT_PRESSURE_WINDOW_SECS: i64 = 60;
const BROKEN_KEY_SUBJECT_USER: &str = "user";
const BROKEN_KEY_SUBJECT_TOKEN: &str = "token";
const BROKEN_KEY_SOURCE_AUTO: &str = "auto";
const BROKEN_KEY_SOURCE_MANUAL: &str = "manual";

const BLOCKED_HEADERS: &[&str] = &[
    "forwarded",
    "via",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-forwarded-port",
    "x-forwarded-server",
    "x-original-forwarded-for",
    "x-forwarded-protocol",
    "x-real-ip",
    "true-client-ip",
    "cf-connecting-ip",
    "cf-true-client-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "x-cluster-client-ip",
    "x-proxy-user-ip",
    "fastly-client-ip",
    "proxy-authorization",
    "proxy-connection",
    "akamai-origin-hop",
    "x-akamai-edgescape",
    "x-akamai-forwarded-for",
    "cdn-loop",
];

pub(crate) fn is_binding_effect_code(code: &str) -> bool {
    matches!(
        code,
        KEY_EFFECT_NONE
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND
    )
}

pub(crate) fn is_selection_effect_code(code: &str) -> bool {
    matches!(
        code,
        KEY_EFFECT_NONE
            | KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED
            | KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED
            | KEY_EFFECT_MCP_SESSION_INIT_PRESSURE_AVOIDED
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED
            | KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED
    )
}

pub(crate) fn is_key_effect_code(code: &str) -> bool {
    matches!(
        code,
        KEY_EFFECT_NONE
            | KEY_EFFECT_QUARANTINED
            | KEY_EFFECT_MARKED_EXHAUSTED
            | KEY_EFFECT_RESTORED_ACTIVE
            | "cleared_quarantine"
            | KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET
            | KEY_EFFECT_MCP_SESSION_RETRY_WAITED
            | KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED
    )
}

const ALLOWED_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "content-type",
    "last-event-id",
    "mcp-protocol-version",
    "mcp-session-id",
    "pragma",
    "user-agent",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-dest",
    "sec-fetch-user",
    "origin",
    "referer",
];

const ALLOWED_PREFIXES: &[&str] = &["x-mcp-", "x-tavily-", "tavily-"];

// Default per-token quota limits. These are used when no environment override is provided.
pub const TOKEN_HOURLY_LIMIT: i64 = 100;
pub const TOKEN_DAILY_LIMIT: i64 = 500;
pub const TOKEN_MONTHLY_LIMIT: i64 = 5000;
// Legacy per-token raw request limit defaults that still back stored quota rows and
// deprecated read aliases. The active runtime request-rate limiter now uses a fixed
// rolling 5-minute window and no longer reads these values.
pub const TOKEN_HOURLY_REQUEST_LIMIT: i64 = 500;
pub const REQUEST_RATE_LIMIT_WINDOW_MINUTES: i64 = 5;
pub const REQUEST_RATE_LIMIT_WINDOW_SECS: i64 = REQUEST_RATE_LIMIT_WINDOW_MINUTES * SECS_PER_MINUTE;
pub const REQUEST_RATE_LIMIT: i64 = 100;
pub const REQUEST_RATE_LIMIT_MIN: i64 = 1;
// Keep a request_id -> key affinity for Tavily research result polling.
// This avoids switching keys between POST /research and GET /research/{request_id}.
const RESEARCH_REQUEST_AFFINITY_TTL_SECS: i64 = 24 * 60 * 60;
const MCP_SESSION_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;
const MCP_PROXY_USER_AGENT: &str = "tavily-hikari-mcp-proxy/1.0";
pub const MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT: i64 = 5;
pub const MCP_SESSION_AFFINITY_KEY_COUNT_MIN: i64 = 1;
pub const MCP_SESSION_AFFINITY_KEY_COUNT_MAX: i64 = 1_000;
pub const REBALANCE_MCP_ENABLED_DEFAULT: bool = false;
pub const REBALANCE_MCP_SESSION_PERCENT_DEFAULT: i64 = 100;
pub const REBALANCE_MCP_SESSION_PERCENT_MIN: i64 = 0;
pub const REBALANCE_MCP_SESSION_PERCENT_MAX: i64 = 100;
pub const MCP_GATEWAY_MODE_UPSTREAM: &str = "upstream_mcp";
pub const MCP_GATEWAY_MODE_REBALANCE: &str = "rebalance_http";
pub const MCP_EXPERIMENT_VARIANT_CONTROL: &str = "control";
pub const MCP_EXPERIMENT_VARIANT_REBALANCE: &str = "rebalance";
pub const REBALANCE_MCP_HTTP_BACKOFF_SCOPE: &str = "rebalance_mcp_http";
// Hard cap on the number of token→key affinity entries kept in memory to prevent
// unbounded growth under churny traffic (many distinct tokens).
const TOKEN_AFFINITY_MAX_ENTRIES: usize = 10_000;
const USER_API_KEY_BINDING_RECENT_LIMIT: i64 = 3;
const TOKEN_API_KEY_BINDING_RECENT_LIMIT: i64 = 3;
// Cache token -> user binding to avoid repeated DB lookups on hot request paths.
const TOKEN_BINDING_CACHE_TTL_SECS: u64 = 30;
const TOKEN_BINDING_CACHE_MAX_ENTRIES: usize = 10_000;
const ACCOUNT_QUOTA_RESOLUTION_CACHE_TTL_SECS: u64 = 5;
const ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES: usize = 10_000;
const ADMIN_REQUEST_LOGS_CATALOG_CACHE_TTL_SECS: i64 = 30;
// Keep the lease TTL below the acquisition wait so a crashed holder can be recovered
// by the next in-flight request instead of blocking the subject for minutes.
const QUOTA_SUBJECT_LOCK_TTL_SECS: u64 = 20;
const QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS: u64 = 30;
const QUOTA_SUBJECT_LOCK_REFRESH_SECS: u64 = 5;
const QUOTA_SUBJECT_LOCK_REFRESH_RETRY_SECS: u64 = 1;

const REQUEST_LOGS_MIN_RETENTION_DAYS: i64 = 32;

const BILLING_STATE_NONE: &str = "none";
const BILLING_STATE_PENDING: &str = "pending";
const BILLING_STATE_CHARGED: &str = "charged";

static QUOTA_SUBJECT_LOCK_OWNER_SEQ: AtomicU64 = AtomicU64::new(1);

const GRANULARITY_MINUTE: &str = "minute";
const GRANULARITY_HOUR: &str = "hour";
const GRANULARITY_DAY: &str = "day";
// Per-token raw request counter (any request type), aggregated per minute.
#[allow(dead_code)]
const GRANULARITY_REQUEST_MINUTE: &str = "request_minute";
const BUCKET_RETENTION_SECS: i64 = 2 * 24 * 3600; // 48h，足够覆盖 24h 窗口
const CLEANUP_INTERVAL_SECS: i64 = 600;
const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_FIVE_MINUTES: i64 = 5 * SECS_PER_MINUTE;
const SECS_PER_HOUR: i64 = 3600;
const SECS_PER_DAY: i64 = 24 * SECS_PER_HOUR;
const TOKEN_USAGE_STATS_BUCKET_SECS: i64 = SECS_PER_HOUR;
const USAGE_PROBE_TIMEOUT_SECS: u64 = 8;
const USAGE_PROBE_RETRY_ATTEMPTS: usize = 3;
const USAGE_PROBE_RETRY_DELAY_MS: u64 = 200;

// Time-based retention for per-token access logs (auth_token_logs).
// This is purely time-driven and must not depend on access token enable/disable/delete status,
// to preserve auditability.
const AUTH_TOKEN_LOG_RETENTION_SECS: i64 = 90 * SECS_PER_DAY;

const META_KEY_DATA_CONSISTENCY_DONE: &str = "data_consistency_v1_done";
const META_KEY_TOKEN_USAGE_ROLLUP_TS: &str = "token_usage_rollup_last_ts";
const META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2: &str = "token_usage_rollup_last_log_id_v2";
const META_KEY_HEAL_ORPHAN_TOKENS_V1: &str = "heal_orphan_auth_tokens_from_logs_v1";
const META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE: &str = "api_key_usage_buckets_v1_done";
const META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE: &str =
    "api_key_usage_buckets_request_value_v2_done";
const META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE: &str =
    "dashboard_request_rollup_buckets_v1_done";
const META_KEY_ACCOUNT_QUOTA_BACKFILL_V1: &str = "account_quota_backfill_v1";
const META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1: &str =
    "account_quota_inherits_defaults_backfill_v1";
const META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1: &str = "account_quota_zero_base_cutover_v1";
const META_KEY_FORCE_USER_RELOGIN_V1: &str = "force_user_relogin_v1";
const META_KEY_ACCOUNT_USAGE_ROLLUP_V1_DONE: &str = "account_usage_rollup_v1_done";
const META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START: &str =
    "account_usage_rollup_rate5m_coverage_start";
const META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START: &str =
    "account_usage_rollup_quota1h_coverage_start";
const META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA24H_COVERAGE_START: &str =
    "account_usage_rollup_quota24h_coverage_start";
const META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA_MONTH_COVERAGE_START: &str =
    "account_usage_rollup_quota_month_coverage_start";
const META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1: &str = "account_limit_snapshot_backfill_v1";
const META_KEY_ALLOW_REGISTRATION_V1: &str = "allow_registration_v1";
const META_KEY_REQUEST_RATE_LIMIT_V1: &str = "request_rate_limit_v1";
const META_KEY_MCP_SESSION_AFFINITY_KEY_COUNT_V1: &str = "mcp_session_affinity_key_count_v1";
const META_KEY_REBALANCE_MCP_ENABLED_V1: &str = "rebalance_mcp_enabled_v1";
const META_KEY_REBALANCE_MCP_SESSION_PERCENT_V1: &str = "rebalance_mcp_session_percent_v1";
const META_KEY_USER_BLOCKED_KEY_BASE_LIMIT_V1: &str = "user_blocked_key_base_limit_v1";
const META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1: &str = "linuxdo_system_tag_defaults_v1";
const META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1: &str = "linuxdo_system_tag_defaults_tuple_v1";
const META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE: &str =
    "request_kind_canonical_migration_v1_state";
const META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE: &str =
    "request_kind_canonical_migration_v1_done";
const META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND: &str =
    "request_kind_canonical_migration_v1_request_logs_upper_bound";
const META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND: &str =
    "request_kind_canonical_migration_v1_auth_token_logs_upper_bound";
const META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1: &str =
    "request_kind_canonical_backfill_request_logs_v1";
const META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1: &str =
    "request_kind_canonical_backfill_auth_token_logs_v1";
const META_KEY_API_KEY_CREATED_AT_BACKFILL_V1: &str = "api_key_created_at_backfill_v1";
// Cutover marker for switching business quota counters from "requests" to "credits".
// We cannot retroactively convert legacy request counts into credits, so we reset the
// lightweight counters once and start charging by upstream credits going forward.
const META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1: &str = "business_quota_credits_cutover_v1";
const META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1: &str = "business_quota_monthly_rebase_v1";
const ACCOUNT_USAGE_ROLLUP_REQUEST_BACKFILL_SECS: i64 = SECS_PER_DAY;
const ACCOUNT_USAGE_ROLLUP_BUSINESS_BACKFILL_SECS: i64 = 90 * SECS_PER_DAY;
const ACCOUNT_USAGE_ROLLUP_MONTH_CHART_MONTHS: i32 = 12;
const ACCOUNT_USAGE_ROLLUP_FIVE_MINUTE_RETENTION_SECS: i64 = 2 * SECS_PER_DAY;
const ACCOUNT_USAGE_ROLLUP_HOUR_RETENTION_SECS: i64 = 8 * SECS_PER_DAY;
const ACCOUNT_USAGE_ROLLUP_DAY_RETENTION_SECS: i64 = 400 * SECS_PER_DAY;
const ACCOUNT_USAGE_ROLLUP_MONTH_RETENTION_MONTHS: i32 = 24;
const API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS: [u64; 2] = [20, 50];
const TOKEN_USAGE_ROLLUP_TRANSIENT_RETRY_BACKOFF_MS: [u64; 3] = [20, 50, 100];

pub async fn run_request_kind_canonical_backfill(
    database_path: &str,
    batch_size: i64,
    dry_run: bool,
) -> Result<RequestKindCanonicalBackfillReport, ProxyError> {
    let pool = store::open_sqlite_pool(database_path, true, false).await?;
    store::run_request_kind_canonical_backfill_with_pool(&pool, batch_size, dry_run, None, None)
        .await
}

fn token_limit_from_env(var: &str, default: i64) -> i64 {
    match std::env::var(var) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return default;
            }
            match trimmed.parse::<i64>() {
                Ok(v) if v > 0 => v,
                _ => default,
            }
        }
        Err(_) => default,
    }
}

fn parse_hhmm(raw: &str) -> Option<(u32, u32)> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let hh = parts.next()?;
    let mm = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if hh.len() != 2 || mm.len() != 2 {
        return None;
    }
    let hour = hh.parse::<u32>().ok()?;
    let minute = mm.parse::<u32>().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

/// Effective request log GC run time (local server time), including environment overrides.
///
/// Environment variable: `REQUEST_LOGS_GC_AT` (format `HH:mm`).
pub fn effective_request_logs_gc_at() -> (u32, u32) {
    match std::env::var("REQUEST_LOGS_GC_AT") {
        Ok(raw) => parse_hhmm(&raw).unwrap_or((7, 0)),
        Err(_) => (7, 0),
    }
}

/// Effective request log retention days (minimum enforced), including environment overrides.
///
/// Environment variable: `REQUEST_LOGS_RETENTION_DAYS` (positive integer; min 32).
pub fn effective_request_logs_retention_days() -> i64 {
    let days = token_limit_from_env(
        "REQUEST_LOGS_RETENTION_DAYS",
        REQUEST_LOGS_MIN_RETENTION_DAYS,
    );
    days.max(REQUEST_LOGS_MIN_RETENTION_DAYS)
}

pub fn effective_auth_token_log_retention_days() -> i64 {
    AUTH_TOKEN_LOG_RETENTION_SECS / SECS_PER_DAY
}

/// Effective hourly quota limit per access token, including environment overrides.
///
/// Environment variable: `TOKEN_HOURLY_LIMIT` (must be a positive integer).
pub fn effective_token_hourly_limit() -> i64 {
    token_limit_from_env("TOKEN_HOURLY_LIMIT", TOKEN_HOURLY_LIMIT)
}

/// Effective daily quota limit per access token, including environment overrides.
///
/// Environment variable: `TOKEN_DAILY_LIMIT` (must be a positive integer).
pub fn effective_token_daily_limit() -> i64 {
    token_limit_from_env("TOKEN_DAILY_LIMIT", TOKEN_DAILY_LIMIT)
}

/// Effective monthly quota limit per access token, including environment overrides.
///
/// Environment variable: `TOKEN_MONTHLY_LIMIT` (must be a positive integer).
pub fn effective_token_monthly_limit() -> i64 {
    token_limit_from_env("TOKEN_MONTHLY_LIMIT", TOKEN_MONTHLY_LIMIT)
}

/// Effective hourly raw request limit per access token, including environment overrides.
///
/// Environment variable: `TOKEN_HOURLY_REQUEST_LIMIT` (must be a positive integer).
pub fn effective_token_hourly_request_limit() -> i64 {
    token_limit_from_env("TOKEN_HOURLY_REQUEST_LIMIT", TOKEN_HOURLY_REQUEST_LIMIT)
}

pub fn request_rate_limit_window_minutes() -> i64 {
    REQUEST_RATE_LIMIT_WINDOW_MINUTES
}

pub fn request_rate_limit_window_secs() -> i64 {
    REQUEST_RATE_LIMIT_WINDOW_SECS
}

pub fn request_rate_limit() -> i64 {
    REQUEST_RATE_LIMIT
}

#[derive(Debug, Clone)]
struct SanitizedHeaders {
    headers: HeaderMap,
    forwarded: Vec<String>,
    dropped: Vec<String>,
}

#[derive(Debug, Clone)]
struct TokenAffinity {
    key_id: String,
    expires_at: i64,
}

#[derive(Debug)]
struct TokenAffinityState {
    ttl_secs: i64,
    mappings: HashMap<String, TokenAffinity>,
}

impl TokenAffinityState {
    fn new(ttl_secs: i64) -> Self {
        Self {
            ttl_secs,
            mappings: HashMap::new(),
        }
    }

    /// 返回给定 token 当前的亲和 key（若存在且未过期），并在过期时清理映射。
    fn get_candidate(&mut self, token_id: &str, now_ts: i64) -> Option<String> {
        if let Some(entry) = self.mappings.get(token_id) {
            if entry.expires_at > now_ts {
                return Some(entry.key_id.clone());
            }
            // 亲和已过期，删除旧映射
            self.mappings.remove(token_id);
        }
        None
    }

    /// 记录或更新 token 的亲和 key，并从 now_ts 起应用 TTL。
    fn record_mapping(&mut self, token_id: &str, key_id: &str, now_ts: i64) {
        // 先在写入前进行一次轻量清理，防止在高基数 token 场景下无限增长。
        if self.mappings.len() >= TOKEN_AFFINITY_MAX_ENTRIES {
            self.prune(now_ts);
        }

        let expires_at = now_ts + self.ttl_secs;
        self.mappings.insert(
            token_id.to_owned(),
            TokenAffinity {
                key_id: key_id.to_owned(),
                expires_at,
            },
        );
    }

    /// 清理过期条目，并在必要时进一步驱逐部分条目以控制总体大小。
    fn prune(&mut self, now_ts: i64) {
        // 先移除所有已经过期的亲和关系。
        self.mappings.retain(|_, v| v.expires_at > now_ts);

        if self.mappings.len() <= TOKEN_AFFINITY_MAX_ENTRIES {
            return;
        }

        // 如果仍然超过上限，则按过期时间从最近到最远排序，优先淘汰“最接近过期”的条目。
        // 目标是把大小收缩到上限的一半，避免每次触顶都全量排序。
        let mut entries: Vec<(String, i64)> = self
            .mappings
            .iter()
            .map(|(k, v)| (k.clone(), v.expires_at))
            .collect();

        entries.sort_by_key(|(_, expires_at)| *expires_at);

        let target_len = TOKEN_AFFINITY_MAX_ENTRIES / 2;
        let to_remove = self.mappings.len().saturating_sub(target_len.max(1));

        for (key, _) in entries.into_iter().take(to_remove) {
            self.mappings.remove(&key);
        }
    }
}

#[cfg(test)]
mod affinity_tests {
    use super::*;

    #[test]
    fn no_mapping_returns_none() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;
        assert!(state.get_candidate("token-a", now).is_none());
    }

    #[test]
    fn mapping_is_returned_before_ttl() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;
        state.record_mapping("token-a", "key-1", now);

        let cand = state.get_candidate("token-a", now + 30);
        assert_eq!(cand.as_deref(), Some("key-1"));
    }

    #[test]
    fn mapping_expires_after_ttl_and_is_cleaned() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;
        state.record_mapping("token-a", "key-1", now);

        // 超过 TTL 之后应返回 None
        let cand = state.get_candidate("token-a", now + 61);
        assert!(cand.is_none());

        // 再次查询应仍为 None（确认映射已被删除）
        let cand2 = state.get_candidate("token-a", now + 62);
        assert!(cand2.is_none());
    }

    #[test]
    fn record_mapping_overwrites_existing_entry() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;
        state.record_mapping("token-a", "key-1", now);
        state.record_mapping("token-a", "key-2", now + 10);

        let cand = state.get_candidate("token-a", now + 20);
        assert_eq!(cand.as_deref(), Some("key-2"));
    }

    #[test]
    fn prune_keeps_map_bounded() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;

        // 填充超过上限的条目，验证内部会触发收缩。
        let over = TOKEN_AFFINITY_MAX_ENTRIES + 100;
        for i in 0..over {
            let token_id = format!("token-{i}");
            let key_id = format!("key-{i}");
            state.record_mapping(&token_id, &key_id, now);
        }

        assert!(
            state.mappings.len() <= TOKEN_AFFINITY_MAX_ENTRIES,
            "mappings.len()={} should be <= {}",
            state.mappings.len(),
            TOKEN_AFFINITY_MAX_ENTRIES
        );
    }
}

#[derive(Default, Debug)]
struct CleanupState {
    last_pruned: i64,
}

const USER_TAG_EFFECT_QUOTA_DELTA: &str = "quota_delta";
const USER_TAG_EFFECT_BLOCK_ALL: &str = "block_all";
const USER_TAG_SOURCE_MANUAL: &str = "manual";
const USER_TAG_SOURCE_SYSTEM_LINUXDO: &str = "system_linuxdo";
const USER_TAG_SYSTEM_KEY_LINUXDO_PREFIX: &str = "linuxdo_l";
const USER_TAG_ICON_LINUXDO: &str = "linuxdo";

// LinuxDo trust tiers intentionally ship with the legacy token quota tuple as their
// additive delta, so auto-bound LinuxDo users receive that uplift on top of the
// account base quota unless an admin edits the system tag effect later.
fn linuxdo_system_tag_default_deltas() -> (i64, i64, i64, i64) {
    (
        effective_token_hourly_request_limit(),
        effective_token_hourly_limit(),
        effective_token_daily_limit(),
        effective_token_monthly_limit(),
    )
}

fn format_linuxdo_system_tag_default_deltas(value: (i64, i64, i64, i64)) -> String {
    format!("{},{},{},{}", value.0, value.1, value.2, value.3)
}

fn parse_linuxdo_system_tag_default_deltas(raw: &str) -> Option<(i64, i64, i64, i64)> {
    let mut parts = raw.split(',').map(str::trim);
    let hourly_any = parts.next()?.parse().ok()?;
    let hourly = parts.next()?.parse().ok()?;
    let daily = parts.next()?.parse().ok()?;
    let monthly = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((hourly_any, hourly, daily, monthly))
}

#[derive(Debug, Clone)]
struct AccountQuotaLimits {
    hourly_any_limit: i64,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
    inherits_defaults: bool,
}

impl AccountQuotaLimits {
    fn zero_base() -> Self {
        Self {
            hourly_any_limit: 0,
            hourly_limit: 0,
            daily_limit: 0,
            monthly_limit: 0,
            inherits_defaults: false,
        }
    }

    fn legacy_defaults() -> Self {
        Self {
            hourly_any_limit: effective_token_hourly_request_limit(),
            hourly_limit: effective_token_hourly_limit(),
            daily_limit: effective_token_daily_limit(),
            monthly_limit: effective_token_monthly_limit(),
            inherits_defaults: true,
        }
    }

    fn clamped_non_negative(&self) -> Self {
        Self {
            hourly_any_limit: self.hourly_any_limit.max(0),
            hourly_limit: self.hourly_limit.max(0),
            daily_limit: self.daily_limit.max(0),
            monthly_limit: self.monthly_limit.max(0),
            inherits_defaults: self.inherits_defaults,
        }
    }

    fn same_limits_as(&self, other: &Self) -> bool {
        self.hourly_any_limit == other.hourly_any_limit
            && self.hourly_limit == other.hourly_limit
            && self.daily_limit == other.daily_limit
            && self.monthly_limit == other.monthly_limit
    }
}

fn account_quota_limits_from_row(
    hourly_any_limit: i64,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
    inherits_defaults: i64,
) -> AccountQuotaLimits {
    AccountQuotaLimits {
        hourly_any_limit,
        hourly_limit,
        daily_limit,
        monthly_limit,
        inherits_defaults: inherits_defaults == 1,
    }
}

fn normalize_optional_api_key_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

#[derive(Debug, serde::Deserialize)]
struct CountryIsBatchEntry {
    ip: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    subdivision: Option<String>,
}

#[derive(Debug, Clone)]
struct ForwardProxyGeoCandidate {
    endpoint: forward_proxy::ForwardProxyEndpoint,
    host_ips: Vec<String>,
    regions: Vec<String>,
    source: ForwardProxyGeoSource,
    geo_refreshed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForwardProxyGeoSource {
    Unknown,
    Trace,
    Negative,
}

impl ForwardProxyGeoSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "",
            Self::Trace => "trace",
            Self::Negative => "negative",
        }
    }

    fn from_runtime(value: &str) -> Self {
        match value.trim() {
            "trace" => Self::Trace,
            "negative" => Self::Negative,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForwardProxyGeoRefreshMode {
    LazyFillMissing,
    ForceRefreshAll,
}

#[derive(Clone, Copy)]
struct RegistrationAffinityContext<'a> {
    geo_origin: &'a str,
    registration_ip: Option<&'a str>,
    registration_region: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignedProxyMatchKind {
    RegistrationIp,
    SameRegion,
    Other,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyAssignmentPreview {
    pub key: String,
    pub label: String,
    pub match_kind: AssignedProxyMatchKind,
}

#[derive(Debug, Clone)]
pub struct ApiKeyStickyNode {
    pub role: &'static str,
    pub node: forward_proxy::ForwardProxyLiveNodeResponse,
}

#[derive(Debug, Clone)]
pub struct ApiKeyStickyNodesResponse {
    pub range_start: String,
    pub range_end: String,
    pub bucket_seconds: i64,
    pub nodes: Vec<ApiKeyStickyNode>,
}

fn normalize_ip_string(raw: &str) -> Option<String> {
    raw.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
    {
        return false;
    }

    let [a, b, c, _d] = ip.octets();
    if a == 0 {
        return false;
    }
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }
    if a == 192 && b == 0 && c == 0 {
        return false;
    }
    if a == 198 && (b == 18 || b == 19) {
        return false;
    }
    if a >= 240 {
        return false;
    }

    true
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4() {
        return is_public_ipv4(v4);
    }

    let segments = ip.segments();
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
        && !is_documentation
}

fn is_global_geo_ip(raw: &str) -> bool {
    match raw.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_public_ipv4(ip),
        Ok(IpAddr::V6(ip)) => is_public_ipv6(ip),
        Err(_) => false,
    }
}

fn build_registration_geo_batch_url(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    if origin.contains('?') {
        format!(
            "{origin}&{}",
            API_KEY_IP_GEO_BATCH_FIELDS.trim_start_matches('?')
        )
    } else {
        format!("{origin}{API_KEY_IP_GEO_BATCH_FIELDS}")
    }
}

fn trim_or_empty(value: Option<String>) -> String {
    value
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn looks_like_subdivision_code(raw: &str) -> bool {
    let raw = raw.trim();
    let len = raw.len();
    if !(2..=3).contains(&len) {
        return false;
    }
    raw.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn format_registration_region(country: &str, subdivision: &str, city: &str) -> Option<String> {
    let mut parts = Vec::new();
    if !country.is_empty() {
        parts.push(country.to_string());
    }
    if !subdivision.is_empty() {
        if looks_like_subdivision_code(subdivision) && !city.is_empty() {
            parts.push(format!("{city} ({subdivision})"));
        } else {
            parts.push(subdivision.to_string());
        }
    } else if parts.is_empty() && !city.is_empty() {
        parts.push(city.to_string());
    }
    let result = parts.join(" ").trim().to_string();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

async fn resolve_registration_regions(origin: &str, ips: &[String]) -> HashMap<String, String> {
    let pending = ips
        .iter()
        .filter_map(|ip| normalize_ip_string(ip))
        .filter(|ip| is_global_geo_ip(ip))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return HashMap::new();
    }

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(API_KEY_IP_GEO_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(API_KEY_IP_GEO_HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            eprintln!("build api key geo resolver client error: {err}");
            return HashMap::new();
        }
    };
    let batch_url = build_registration_geo_batch_url(origin);
    let mut resolved = HashMap::new();

    'batch_lookup: for batch in pending.chunks(API_KEY_IP_GEO_BATCH_SIZE) {
        let mut attempt = 0usize;
        let response = loop {
            match client.post(&batch_url).json(batch).send().await {
                Ok(response)
                    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                        && attempt == 0 =>
                {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                Ok(response) => break response,
                Err(err) if attempt == 0 => {
                    attempt += 1;
                    eprintln!("api key geo lookup request error, retrying once: {err}");
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(err) => {
                    eprintln!("api key geo lookup request error: {err}");
                    continue 'batch_lookup;
                }
            }
        };

        if !response.status().is_success() {
            eprintln!("api key geo lookup returned status: {}", response.status());
            continue;
        }

        let entries = match response.json::<Vec<CountryIsBatchEntry>>().await {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("api key geo lookup decode error: {err}");
                continue;
            }
        };

        for entry in entries {
            let Some(ip) = normalize_ip_string(&entry.ip) else {
                continue;
            };
            let region = format_registration_region(
                trim_or_empty(entry.country).as_str(),
                trim_or_empty(entry.subdivision).as_str(),
                trim_or_empty(entry.city).as_str(),
            );
            if let Some(region) = region {
                resolved.insert(ip, region);
            }
        }
    }

    resolved
}

fn default_account_quota_limits_for_created_at(
    user_created_at: i64,
    zero_base_cutover_at: i64,
) -> AccountQuotaLimits {
    if user_created_at >= zero_base_cutover_at {
        AccountQuotaLimits::zero_base()
    } else {
        AccountQuotaLimits::legacy_defaults()
    }
}

#[derive(Debug, Clone)]
struct UserTagRecord {
    id: String,
    name: String,
    display_name: String,
    icon: Option<String>,
    system_key: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
    user_count: i64,
}

impl UserTagRecord {
    fn is_system(&self) -> bool {
        self.system_key.is_some()
    }

    fn is_block_all(&self) -> bool {
        self.effect_kind == USER_TAG_EFFECT_BLOCK_ALL
    }
}

#[derive(Debug, Clone)]
struct UserTagBindingRecord {
    source: String,
    tag: UserTagRecord,
}

#[derive(Debug, Clone)]
struct AccountQuotaBreakdownRecord {
    kind: String,
    label: String,
    tag_id: Option<String>,
    tag_name: Option<String>,
    source: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
}

#[derive(Debug, Clone)]
struct AccountQuotaResolution {
    base: AccountQuotaLimits,
    effective: AccountQuotaLimits,
    breakdown: Vec<AccountQuotaBreakdownRecord>,
    tags: Vec<UserTagBindingRecord>,
}

fn clamp_i128_to_i64(value: i128) -> i64 {
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn apply_quota_delta(value: i64, delta: i64) -> i64 {
    clamp_i128_to_i64(i128::from(value) + i128::from(delta))
}

fn normalize_linuxdo_trust_level(trust_level: Option<i64>) -> Option<i64> {
    trust_level.filter(|level| (0..=4).contains(level))
}

fn linuxdo_system_key_for_level(level: i64) -> String {
    format!("{USER_TAG_SYSTEM_KEY_LINUXDO_PREFIX}{level}")
}

fn to_admin_quota_limit_set(limits: &AccountQuotaLimits) -> AdminQuotaLimitSet {
    AdminQuotaLimitSet {
        hourly_any_limit: limits.hourly_any_limit,
        hourly_limit: limits.hourly_limit,
        daily_limit: limits.daily_limit,
        monthly_limit: limits.monthly_limit,
        inherits_defaults: limits.inherits_defaults,
    }
}

fn to_admin_user_tag(tag: &UserTagRecord) -> AdminUserTag {
    AdminUserTag {
        id: tag.id.clone(),
        name: tag.name.clone(),
        display_name: tag.display_name.clone(),
        icon: tag.icon.clone(),
        system_key: tag.system_key.clone(),
        effect_kind: tag.effect_kind.clone(),
        hourly_any_delta: tag.hourly_any_delta,
        hourly_delta: tag.hourly_delta,
        daily_delta: tag.daily_delta,
        monthly_delta: tag.monthly_delta,
        user_count: tag.user_count,
    }
}

fn to_admin_user_tag_binding(binding: &UserTagBindingRecord) -> AdminUserTagBinding {
    AdminUserTagBinding {
        tag_id: binding.tag.id.clone(),
        name: binding.tag.name.clone(),
        display_name: binding.tag.display_name.clone(),
        icon: binding.tag.icon.clone(),
        system_key: binding.tag.system_key.clone(),
        effect_kind: binding.tag.effect_kind.clone(),
        hourly_any_delta: binding.tag.hourly_any_delta,
        hourly_delta: binding.tag.hourly_delta,
        daily_delta: binding.tag.daily_delta,
        monthly_delta: binding.tag.monthly_delta,
        source: binding.source.clone(),
    }
}

fn to_admin_quota_breakdown_entry(
    entry: &AccountQuotaBreakdownRecord,
) -> AdminUserQuotaBreakdownEntry {
    AdminUserQuotaBreakdownEntry {
        kind: entry.kind.clone(),
        label: entry.label.clone(),
        tag_id: entry.tag_id.clone(),
        tag_name: entry.tag_name.clone(),
        source: entry.source.clone(),
        effect_kind: entry.effect_kind.clone(),
        hourly_any_delta: entry.hourly_any_delta,
        hourly_delta: entry.hourly_delta,
        daily_delta: entry.daily_delta,
        monthly_delta: entry.monthly_delta,
    }
}

fn build_account_quota_resolution(
    base: AccountQuotaLimits,
    tags: Vec<UserTagBindingRecord>,
) -> AccountQuotaResolution {
    let mut effective = base.clone();
    let mut breakdown = vec![AccountQuotaBreakdownRecord {
        kind: "base".to_string(),
        label: "base".to_string(),
        tag_id: None,
        tag_name: None,
        source: None,
        effect_kind: "base".to_string(),
        hourly_any_delta: base.hourly_any_limit,
        hourly_delta: base.hourly_limit,
        daily_delta: base.daily_limit,
        monthly_delta: base.monthly_limit,
    }];
    let mut block_all = false;

    for binding in &tags {
        breakdown.push(AccountQuotaBreakdownRecord {
            kind: "tag".to_string(),
            label: binding.tag.display_name.clone(),
            tag_id: Some(binding.tag.id.clone()),
            tag_name: Some(binding.tag.name.clone()),
            source: Some(binding.source.clone()),
            effect_kind: binding.tag.effect_kind.clone(),
            hourly_any_delta: binding.tag.hourly_any_delta,
            hourly_delta: binding.tag.hourly_delta,
            daily_delta: binding.tag.daily_delta,
            monthly_delta: binding.tag.monthly_delta,
        });

        if binding.tag.is_block_all() {
            block_all = true;
            continue;
        }

        effective.hourly_any_limit =
            apply_quota_delta(effective.hourly_any_limit, binding.tag.hourly_any_delta);
        effective.hourly_limit =
            apply_quota_delta(effective.hourly_limit, binding.tag.hourly_delta);
        effective.daily_limit = apply_quota_delta(effective.daily_limit, binding.tag.daily_delta);
        effective.monthly_limit =
            apply_quota_delta(effective.monthly_limit, binding.tag.monthly_delta);
    }

    effective = if block_all {
        AccountQuotaLimits {
            hourly_any_limit: 0,
            hourly_limit: 0,
            daily_limit: 0,
            monthly_limit: 0,
            inherits_defaults: base.inherits_defaults,
        }
    } else {
        effective.clamped_non_negative()
    };

    breakdown.push(AccountQuotaBreakdownRecord {
        kind: "effective".to_string(),
        label: "effective".to_string(),
        tag_id: None,
        tag_name: None,
        source: None,
        effect_kind: if block_all {
            USER_TAG_EFFECT_BLOCK_ALL.to_string()
        } else {
            "effective".to_string()
        },
        hourly_any_delta: effective.hourly_any_limit,
        hourly_delta: effective.hourly_limit,
        daily_delta: effective.daily_limit,
        monthly_delta: effective.monthly_limit,
    });

    AccountQuotaResolution {
        base,
        effective,
        breakdown,
        tags,
    }
}

#[derive(Debug, Clone)]
enum QuotaSubject {
    Token(String),
    Account(String),
}

impl QuotaSubject {
    fn billing_subject(&self) -> String {
        match self {
            Self::Token(token_id) => format!("token:{token_id}"),
            Self::Account(user_id) => format!("account:{user_id}"),
        }
    }

    fn from_billing_subject(subject: &str) -> Result<Self, ProxyError> {
        if let Some(user_id) = subject.strip_prefix("account:") {
            Ok(Self::Account(user_id.to_string()))
        } else if let Some(token_id) = subject.strip_prefix("token:") {
            Ok(Self::Token(token_id.to_string()))
        } else {
            Err(ProxyError::QuotaDataMissing {
                reason: format!("invalid billing subject: {subject}"),
            })
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TokenBindingCacheEntry {
    user_id: Option<String>,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct AccountQuotaResolutionCacheEntry {
    resolution: AccountQuotaResolution,
    expires_at: Instant,
}
