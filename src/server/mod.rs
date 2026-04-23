use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordVerifier},
};
use async_stream::stream;
use axum::http::header::{
    CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, SET_COOKIE, TRANSFER_ENCODING,
};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Router,
    body::{self, Body, Bytes},
    extract::{Form, Path, Query, RawQuery, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode},
    response::{Json, Redirect},
    routing::{any, delete, get, patch, post, put},
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate, TimeZone, Utc};
use futures_util::stream as futures_stream;
use futures_util::{Stream, StreamExt};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::form_urlencoded;
#[derive(Debug, Clone, PartialEq, Eq)]
struct SummarySig {
    summary: [i64; 9],
    summary_last_activity: Option<i64>,
    today: [i64; 15],
    yesterday: [i64; 15],
    month: [i64; 17],
    proxy: Option<(i64, i64)>,
    exhausted_keys: Vec<String>,
    disabled_tokens: Vec<String>,
    disabled_tokens_error: bool,
    disabled_tokens_truncated: bool,
    recent_jobs: Vec<(i64, String, Option<i64>)>,
    hourly_window_anchor: i64,
    recent_alerts_total_events: i64,
    recent_alerts_grouped_count: i64,
    recent_alerts_counts: Vec<(String, i64)>,
    recent_alerts_top_groups: Vec<(String, i64, i64)>,
}
use std::time::{Duration, Instant};
use tavily_hikari::{
    AdminUserIdentity, AdminUserUsageSeriesKind, ApiKeyMetrics, ApiKeyStickyNode, ApiKeyStickyUser,
    ApiKeyUserUsageBucket, AuthToken, ForwardProxyHourlyBucketResponse, ForwardProxyStatsResponse,
    ForwardProxyWeightHourlyBucketResponse, JobLog, LogFacetOption, OAuthAccountProfile,
    PendingBillingSettleOutcome, ProxyError, ProxyRequest, ProxyResponse, ProxySummary,
    RequestLogBodiesRecord, RequestLogRecord, RequestLogsCatalog, RequestLogsCursor,
    RequestLogsCursorDirection, RequestLogsCursorPage, StickyCreditsWindow, TavilyProxy,
    TokenHourlyBucket, TokenHourlyRequestVerdict, TokenLogRecord, TokenLogsCursorPage,
    TokenQuotaVerdict, TokenRequestKindOption, TokenSummary, TokenUsageBucket,
    UNBOUND_TOKEN_MONTHLY_BROKEN_LIMIT_DEFAULT, USER_MONTHLY_BROKEN_LIMIT_DEFAULT, UserTokenLookup,
    analyze_mcp_attempt, canonical_request_kind_key_for_filter, classify_token_request_kind,
    display_result_status_for_request_kind, effective_request_logs_gc_at,
    effective_request_logs_retention_days, effective_token_daily_limit,
    effective_token_hourly_limit, effective_token_monthly_limit,
    extract_mcp_has_error_by_id_from_bytes, extract_mcp_usage_credits_by_id_from_bytes,
    extract_usage_credits_from_json_bytes, extract_usage_credits_total_from_json_bytes,
    mcp_response_has_any_error, mcp_response_has_any_success, normalize_operational_class_filter,
    operational_class_for_token_log, request_rate_limit, request_rate_limit_window_minutes,
    token_request_kind_billing_group_for_token_log, token_request_kind_protocol_group,
};
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tower_http::services::{ServeDir, ServeFile};

include!("state.rs");
include!("schedulers.rs");
include!("spa.rs");
include!("handlers/tavily.rs");
include!("handlers/public.rs");
include!("handlers/admin_auth.rs");
include!("handlers/user.rs");
include!("handlers/admin_resources.rs");
include!("serve.rs");
include!("dto.rs");
include!("proxy.rs");
include!("tests.rs");
