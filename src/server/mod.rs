use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, OnceLock},
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordVerifier},
};
use async_compression::tokio::{bufread::ZstdDecoder, write::ZstdEncoder};
use async_stream::stream;
use axum::http::header::{
    CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, SET_COOKIE, TRANSFER_ENCODING,
};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Router,
    body::{self, Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Form, Path, Query, RawQuery, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode},
    response::{Json, Redirect},
    routing::{any, delete, get, patch, post, put},
};
use base64::Engine as _;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate, TimeZone, Utc};
use futures_util::stream as futures_stream;
use futures_util::{Stream, StreamExt};
use nanoid::nanoid;
use rand::Rng;
use reqwest::Client;
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Component;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use url::form_urlencoded;
#[derive(Debug, Clone, PartialEq, Eq)]
struct SummarySig {
    freshness: DashboardOverviewFreshness,
}
use std::time::Duration;
use tavily_hikari::{
    AdminTokenEnabledFilter, AdminTokenListFilters, AdminTokenOwnerFilter, AdminUserIdentity,
    AdminUserSortedPageRequest, AdminUserUsageSeriesKind, AnalysisPressureSnapshot, ApiKeyMetrics,
    ApiKeyStickyNode, ApiKeyStickyUser, ApiKeyUserUsageBucket, AuthToken, ClientIpInfo,
    DB_COMPACTION_COOLDOWN_SECS, DB_COMPACTION_MIN_RECLAIMABLE_BYTES,
    DB_COMPACTION_MIN_RECLAIMABLE_RATIO, ForwardProxyHourlyBucketResponse,
    ForwardProxyStatsResponse, ForwardProxyWeightHourlyBucketResponse, JobLog, LogFacetOption,
    OAuthAccountProfile, PendingBillingSettleOutcome, ProxyError, ProxyRequest, ProxyResponse,
    ProxySummary, QUOTA_SYNC_JOB_TIMEOUT_SECS, RequestLogBodiesRecord, RequestLogRecord,
    RequestLogsCatalog, RequestLogsCursor, RequestLogsCursorDirection, RequestLogsCursorPage,
    RequestLogsGcOptions, StickyCreditsWindow, TavilyProxy, TokenHourlyBucket,
    TokenHourlyRequestVerdict, TokenLogBillingFilter, TokenLogRecord, TokenLogsCursorPage,
    TokenQuotaVerdict, TokenRequestKindOption, TokenSummary, TokenUsageBucket,
    TrustedClientIpSettings, UNBOUND_TOKEN_MONTHLY_BROKEN_LIMIT_DEFAULT,
    USER_MONTHLY_BROKEN_LIMIT_DEFAULT, UserTokenLookup, analyze_mcp_attempt,
    canonical_request_kind_key_for_filter, classify_token_request_kind,
    display_result_status_for_request_kind, effective_request_logs_gc_at,
    effective_token_daily_limit, effective_token_hourly_limit, effective_token_monthly_limit,
    extract_mcp_has_error_by_id_from_bytes, extract_mcp_usage_credits_by_id_from_bytes,
    extract_usage_credits_from_json_bytes, extract_usage_credits_total_from_json_bytes,
    format_request_logs_gc_report_message, mcp_response_has_any_error,
    mcp_response_has_any_success, normalize_operational_class_filter,
    operational_class_for_token_log, request_rate_limit, request_rate_limit_window_minutes,
    resolve_client_ip_info, run_db_compaction_once, token_request_kind_billing_group_for_token_log,
    token_request_kind_protocol_group,
};
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, Semaphore};
use tokio_util::io::{ReaderStream, StreamReader};
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
