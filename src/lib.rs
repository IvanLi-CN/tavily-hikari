mod forward_proxy;

use std::{
    cmp::min,
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use chrono::{Datelike, Local, TimeZone, Utc};
use futures_util::TryStreamExt;
use nanoid::nanoid;
use rand::Rng;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{CONTENT_LENGTH, HOST, HeaderMap, HeaderValue},
};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use url::form_urlencoded;

pub use forward_proxy::{
    ForwardProxyLiveStatsResponse, ForwardProxySettings, ForwardProxySettingsResponse,
    ForwardProxyValidationError, ForwardProxyValidationProbeResult, ForwardProxyValidationResponse,
};

/// Tavily MCP upstream默认端点。
pub const DEFAULT_UPSTREAM: &str = "https://mcp.tavily.com/mcp";

const STATUS_ACTIVE: &str = "active";
const STATUS_EXHAUSTED: &str = "exhausted";
const STATUS_DISABLED: &str = "disabled";
const QUARANTINE_REASON_DETAIL_MAX_LEN: usize = 1024;

const OUTCOME_SUCCESS: &str = "success";
const OUTCOME_ERROR: &str = "error";
const OUTCOME_QUOTA_EXHAUSTED: &str = "quota_exhausted";
const OUTCOME_UNKNOWN: &str = "unknown";

// dev-open-admin mode uses a synthetic token id ("dev") for request attribution.
// Keep a placeholder row in auth_tokens so SQLite FOREIGN KEY constraints in
// token_usage_buckets / auth_token_quota / token_usage_stats never fail.
const DEV_OPEN_ADMIN_TOKEN_ID: &str = "dev";
const DEV_OPEN_ADMIN_TOKEN_SECRET: &str = "dev-open-admin";
const DEV_OPEN_ADMIN_TOKEN_NOTE: &str = "[system] dev-open-admin placeholder";

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

const ALLOWED_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "content-type",
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
// Default per-token raw request limit (any request type) per hour.
// This is enforced separately from the business quota above, and counts every
// successful token-authenticated request regardless of MCP method.
pub const TOKEN_HOURLY_REQUEST_LIMIT: i64 = 500;
// Soft affinity window for mapping access tokens to API keys (in seconds).
// Within this window, a token will try to reuse the same API key if it is still active.
const TOKEN_AFFINITY_TTL_SECS: i64 = 15 * 60;
// Keep a request_id -> key affinity for Tavily research result polling.
// This avoids switching keys between POST /research and GET /research/{request_id}.
const RESEARCH_REQUEST_AFFINITY_TTL_SECS: i64 = 24 * 60 * 60;
// Hard cap on the number of token→key affinity entries kept in memory to prevent
// unbounded growth under churny traffic (many distinct tokens).
const TOKEN_AFFINITY_MAX_ENTRIES: usize = 10_000;
// Cache token -> user binding to avoid repeated DB lookups on hot request paths.
const TOKEN_BINDING_CACHE_TTL_SECS: u64 = 30;
const TOKEN_BINDING_CACHE_MAX_ENTRIES: usize = 10_000;
const ACCOUNT_QUOTA_RESOLUTION_CACHE_TTL_SECS: u64 = 5;
const ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES: usize = 10_000;
// Keep the lease TTL below the acquisition wait so a crashed holder can be recovered
// by the next in-flight request instead of blocking the subject for minutes.
const QUOTA_SUBJECT_LOCK_TTL_SECS: u64 = 20;
const QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS: u64 = 30;
const QUOTA_SUBJECT_LOCK_REFRESH_SECS: u64 = 5;
const QUOTA_SUBJECT_LOCK_REFRESH_RETRY_SECS: u64 = 1;

const REQUEST_LOGS_MIN_RETENTION_DAYS: i64 = 7;

const BILLING_STATE_PENDING: &str = "pending";
const BILLING_STATE_CHARGED: &str = "charged";

static QUOTA_SUBJECT_LOCK_OWNER_SEQ: AtomicU64 = AtomicU64::new(1);

const GRANULARITY_MINUTE: &str = "minute";
const GRANULARITY_HOUR: &str = "hour";
// Per-token raw request counter (any request type), aggregated per minute.
const GRANULARITY_REQUEST_MINUTE: &str = "request_minute";
const BUCKET_RETENTION_SECS: i64 = 2 * 24 * 3600; // 48h，足够覆盖 24h 窗口
const CLEANUP_INTERVAL_SECS: i64 = 600;
const SECS_PER_MINUTE: i64 = 60;
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
const META_KEY_ACCOUNT_QUOTA_BACKFILL_V1: &str = "account_quota_backfill_v1";
const META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1: &str =
    "account_quota_inherits_defaults_backfill_v1";
const META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1: &str = "account_quota_zero_base_cutover_v1";
const META_KEY_FORCE_USER_RELOGIN_V1: &str = "force_user_relogin_v1";
const META_KEY_ALLOW_REGISTRATION_V1: &str = "allow_registration_v1";
const META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1: &str = "linuxdo_system_tag_defaults_v1";
const META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1: &str = "linuxdo_system_tag_defaults_tuple_v1";
const META_KEY_AUTH_TOKEN_LOG_REQUEST_KIND_BACKFILL_V1: &str =
    "auth_token_log_request_kind_backfill_v1";
// Cutover marker for switching business quota counters from "requests" to "credits".
// We cannot retroactively convert legacy request counts into credits, so we reset the
// lightweight counters once and start charging by upstream credits going forward.
const META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1: &str = "business_quota_credits_cutover_v1";
const API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS: [u64; 2] = [20, 50];

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
/// Environment variable: `REQUEST_LOGS_RETENTION_DAYS` (positive integer; min 7).
pub fn effective_request_logs_retention_days() -> i64 {
    let days = token_limit_from_env(
        "REQUEST_LOGS_RETENTION_DAYS",
        REQUEST_LOGS_MIN_RETENTION_DAYS,
    );
    days.max(REQUEST_LOGS_MIN_RETENTION_DAYS)
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

    /// 显式删除 token 的亲和关系。
    fn drop_mapping(&mut self, token_id: &str) {
        self.mappings.remove(token_id);
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
    fn drop_mapping_removes_affinity() {
        let mut state = TokenAffinityState::new(60);
        let now = 1_000;
        state.record_mapping("token-a", "key-1", now);
        state.drop_mapping("token-a");

        let cand = state.get_candidate("token-a", now + 10);
        assert!(cand.is_none());
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
struct AccountQuotaSnapshot {
    hourly_any_used: i64,
    hourly_any_limit: i64,
    hourly_used: i64,
    hourly_limit: i64,
    daily_used: i64,
    daily_limit: i64,
    monthly_used: i64,
    monthly_limit: i64,
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

#[derive(Clone, Debug)]
struct TokenQuota {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
}

/// Lightweight per-token hourly request limiter that counts *all* authenticated
/// requests, regardless of MCP method or HTTP endpoint.
#[derive(Clone, Debug)]
struct TokenRequestLimit {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
}

/// 负责均衡 Tavily API key 并透传请求的代理。
#[derive(Clone, Debug)]
pub struct TavilyProxy {
    client: Client,
    forward_proxy_clients: forward_proxy::ForwardProxyClientPool,
    forward_proxy: Arc<Mutex<forward_proxy::ForwardProxyManager>>,
    forward_proxy_affinity: Arc<Mutex<HashMap<String, forward_proxy::ForwardProxyAffinityRecord>>>,
    xray_supervisor: Arc<Mutex<forward_proxy::XraySupervisor>>,
    upstream: Url,
    key_store: Arc<KeyStore>,
    upstream_origin: String,
    token_quota: TokenQuota,
    token_request_limit: TokenRequestLimit,
    affinity: Arc<Mutex<TokenAffinityState>>,
    research_request_affinity: Arc<Mutex<TokenAffinityState>>,
    research_request_owner_affinity: Arc<Mutex<TokenAffinityState>>,
    // Fast in-process lock to collapse duplicate work within one instance. Cross-instance
    // serialization is provided by quota_subject_locks in SQLite.
    token_billing_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    research_key_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
}

#[derive(Clone, Debug)]
pub struct TavilyProxyOptions {
    pub xray_binary: String,
    pub xray_runtime_dir: std::path::PathBuf,
}

impl TavilyProxyOptions {
    pub fn from_database_path(database_path: &str) -> Self {
        Self {
            xray_binary: forward_proxy::default_xray_binary(),
            xray_runtime_dir: forward_proxy::default_xray_runtime_dir(database_path),
        }
    }
}

#[derive(Debug, Clone)]
struct QuotaSubjectDbLease {
    subject: String,
    owner: String,
    ttl: Duration,
}

#[derive(Debug)]
struct QuotaSubjectLockGuard {
    store: Arc<KeyStore>,
    lease: QuotaSubjectDbLease,
    refresh_stop: Arc<AtomicBool>,
    lease_lost: Arc<AtomicBool>,
    refresh_task: tokio::task::JoinHandle<()>,
}

impl QuotaSubjectLockGuard {
    fn new(store: Arc<KeyStore>, lease: QuotaSubjectDbLease) -> Self {
        let refresh_stop = Arc::new(AtomicBool::new(false));
        let lease_lost = Arc::new(AtomicBool::new(false));
        let refresh_task = {
            let store = Arc::clone(&store);
            let lease = lease.clone();
            let refresh_stop = Arc::clone(&refresh_stop);
            let lease_lost = Arc::clone(&lease_lost);
            tokio::spawn(async move {
                let refresh_every = Duration::from_secs(QUOTA_SUBJECT_LOCK_REFRESH_SECS);
                let retry_every = Duration::from_secs(QUOTA_SUBJECT_LOCK_REFRESH_RETRY_SECS);
                while !refresh_stop.load(AtomicOrdering::Relaxed) {
                    tokio::time::sleep(refresh_every).await;
                    if refresh_stop.load(AtomicOrdering::Relaxed) {
                        break;
                    }

                    let retry_budget = lease.ttl.saturating_sub(refresh_every);
                    let retry_deadline = Instant::now() + retry_budget.max(retry_every);
                    loop {
                        match store.refresh_quota_subject_lock(&lease).await {
                            Ok(()) => break,
                            Err(err) => {
                                if refresh_stop.load(AtomicOrdering::Relaxed) {
                                    return;
                                }
                                if Instant::now() >= retry_deadline {
                                    lease_lost.store(true, AtomicOrdering::Relaxed);
                                    eprintln!(
                                        "quota subject lock refresh exhausted retries (subject={} owner={}): {}",
                                        lease.subject, lease.owner, err
                                    );
                                    return;
                                }
                                eprintln!(
                                    "quota subject lock refresh failed (subject={} owner={}): {}; retrying",
                                    lease.subject, lease.owner, err
                                );
                                tokio::time::sleep(retry_every).await;
                            }
                        }
                    }
                }
            })
        };

        Self {
            store,
            lease,
            refresh_stop,
            lease_lost,
            refresh_task,
        }
    }

    fn ensure_live(&self) -> Result<(), ProxyError> {
        if self.lease_lost.load(AtomicOrdering::Relaxed) {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                self.lease.subject,
            )));
        }
        let mut forced = self
            .store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        if forced.remove(&self.lease.subject) {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                self.lease.subject,
            )));
        }
        Ok(())
    }
}

impl Drop for QuotaSubjectLockGuard {
    fn drop(&mut self) {
        self.refresh_stop.store(true, AtomicOrdering::Relaxed);
        self.refresh_task.abort();

        let store = Arc::clone(&self.store);
        let lease = self.lease.clone();
        tokio::spawn(async move {
            if let Err(err) = store.release_quota_subject_lock(&lease).await {
                eprintln!(
                    "quota subject lock release failed (subject={} owner={}): {}",
                    lease.subject, lease.owner, err
                );
            }
        });
    }
}

#[derive(Debug)]
pub struct TokenBillingGuard {
    billing_subject: String,
    _local: tokio::sync::OwnedMutexGuard<()>,
    _subject_lock: QuotaSubjectLockGuard,
}

impl TokenBillingGuard {
    pub fn billing_subject(&self) -> &str {
        &self.billing_subject
    }

    pub fn ensure_live(&self) -> Result<(), ProxyError> {
        self._subject_lock.ensure_live()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingBillingSettleOutcome {
    Charged,
    AlreadySettled,
    RetryLater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyUpsertStatus {
    Created,
    Undeleted,
    Existed,
}

impl ApiKeyUpsertStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Undeleted => "undeleted",
            Self::Existed => "existed",
        }
    }
}

impl TavilyProxy {
    pub async fn new<I, S>(keys: I, database_path: &str) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_options(
            keys,
            DEFAULT_UPSTREAM,
            database_path,
            TavilyProxyOptions::from_database_path(database_path),
        )
        .await
    }

    pub async fn with_endpoint<I, S>(
        keys: I,
        upstream: &str,
        database_path: &str,
    ) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_options(
            keys,
            upstream,
            database_path,
            TavilyProxyOptions::from_database_path(database_path),
        )
        .await
    }

    pub async fn with_options<I, S>(
        keys: I,
        upstream: &str,
        database_path: &str,
        options: TavilyProxyOptions,
    ) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let sanitized: Vec<String> = keys
            .into_iter()
            .map(|k| k.into().trim().to_owned())
            .filter(|k| !k.is_empty())
            .collect();

        let key_store = KeyStore::new(database_path).await?;
        if !sanitized.is_empty() {
            key_store.sync_keys(&sanitized).await?;
        }
        let upstream = Url::parse(upstream).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: upstream.to_owned(),
            source,
        })?;
        let upstream_origin = origin_from_url(&upstream);
        let forward_proxy_settings =
            forward_proxy::load_forward_proxy_settings(&key_store.pool).await?;
        let forward_proxy_runtime =
            forward_proxy::load_forward_proxy_runtime_states(&key_store.pool).await?;
        let forward_proxy = Arc::new(Mutex::new(forward_proxy::ForwardProxyManager::new(
            forward_proxy_settings,
            forward_proxy_runtime,
        )));
        let key_store = Arc::new(key_store);
        let token_quota = TokenQuota::new(key_store.clone());
        let token_request_limit = TokenRequestLimit::new(key_store.clone());
        let forward_proxy_clients = forward_proxy::ForwardProxyClientPool::new()?;
        let mut proxy = Self {
            client: forward_proxy_clients.direct_client(),
            forward_proxy_clients,
            forward_proxy,
            forward_proxy_affinity: Arc::new(Mutex::new(HashMap::new())),
            xray_supervisor: Arc::new(Mutex::new(forward_proxy::XraySupervisor::new(
                options.xray_binary,
                options.xray_runtime_dir,
            ))),
            upstream,
            key_store,
            upstream_origin,
            token_quota,
            token_request_limit,
            affinity: Arc::new(Mutex::new(TokenAffinityState::new(TOKEN_AFFINITY_TTL_SECS))),
            research_request_affinity: Arc::new(Mutex::new(TokenAffinityState::new(
                RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            ))),
            research_request_owner_affinity: Arc::new(Mutex::new(TokenAffinityState::new(
                RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            ))),
            token_billing_locks: Arc::new(Mutex::new(HashMap::new())),
            research_key_locks: Arc::new(Mutex::new(HashMap::new())),
        };
        proxy.initialize_forward_proxy_runtime().await?;
        Ok(proxy)
    }

    async fn initialize_forward_proxy_runtime(&mut self) -> Result<(), ProxyError> {
        if let Err(err) = self.refresh_forward_proxy_subscriptions().await {
            eprintln!("forward-proxy startup subscription refresh error: {err}");
        }
        let manager = self.forward_proxy.lock().await;
        forward_proxy::sync_manager_runtime_to_store(&self.key_store, &manager).await
    }

    pub async fn get_forward_proxy_settings(
        &self,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        let manager = self.forward_proxy.lock().await;
        forward_proxy::build_forward_proxy_settings_response(&self.key_store.pool, &manager).await
    }

    pub async fn get_forward_proxy_live_stats(
        &self,
    ) -> Result<ForwardProxyLiveStatsResponse, ProxyError> {
        let manager = self.forward_proxy.lock().await;
        forward_proxy::build_forward_proxy_live_stats_response(&self.key_store.pool, &manager).await
    }

    pub async fn update_forward_proxy_settings(
        &self,
        settings: ForwardProxySettings,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        let normalized = settings.normalized();
        let previous_manager = {
            let manager = self.forward_proxy.lock().await;
            manager.clone()
        };
        let previous_subscription_urls = previous_manager
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.source == forward_proxy::FORWARD_PROXY_SOURCE_SUBSCRIPTION)
            .filter_map(|endpoint| endpoint.raw_url.clone())
            .collect::<Vec<_>>();
        forward_proxy::save_forward_proxy_settings(&self.key_store.pool, normalized.clone())
            .await?;
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.apply_settings(normalized.clone());
        }
        if let Err(err) = self.refresh_forward_proxy_subscriptions().await {
            eprintln!("forward-proxy subscription refresh after settings update error: {err}");
            if normalized.subscription_urls == previous_manager.settings.subscription_urls
                && !previous_subscription_urls.is_empty()
            {
                let mut manager = self.forward_proxy.lock().await;
                manager.apply_subscription_urls(previous_subscription_urls);
                let mut xray = self.xray_supervisor.lock().await;
                xray.sync_endpoints(&mut manager.endpoints).await?;
            }
        }
        let mut targets = {
            let manager = self.forward_proxy.lock().await;
            manager
                .endpoints
                .iter()
                .filter(|endpoint| !endpoint.is_direct())
                .cloned()
                .collect::<Vec<_>>()
        };
        for endpoint in targets.drain(..) {
            let _ = self
                .probe_and_record_forward_proxy_endpoint(
                    &endpoint,
                    "settings_update",
                    None,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                )
                .await;
        }
        self.get_forward_proxy_settings().await
    }

    pub async fn validate_forward_proxy_candidates(
        &self,
        proxy_urls: Vec<String>,
        subscription_urls: Vec<String>,
    ) -> Result<ForwardProxyValidationResponse, ProxyError> {
        let mut results = Vec::new();
        let mut normalized_values = Vec::new();
        let mut discovered_nodes = 0usize;
        let mut best_latency: Option<f64> = None;
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);

        for raw in forward_proxy::normalize_proxy_url_entries(proxy_urls) {
            let Some(parsed) = forward_proxy::parse_forward_proxy_entry(&raw) else {
                results.push(ForwardProxyValidationProbeResult {
                    value: raw.clone(),
                    normalized_value: None,
                    ok: false,
                    discovered_nodes: Some(0),
                    latency_ms: None,
                    error_code: Some("proxy_invalid".to_string()),
                    message: "unsupported proxy url or unsupported scheme".to_string(),
                });
                continue;
            };
            let endpoint = forward_proxy::ForwardProxyEndpoint {
                key: format!(
                    "__validate_proxy__{:016x}",
                    forward_proxy::stable_hash_u64(&parsed.normalized)
                ),
                source: forward_proxy::FORWARD_PROXY_SOURCE_MANUAL.to_string(),
                display_name: parsed.display_name.clone(),
                protocol: parsed.protocol,
                endpoint_url: parsed.endpoint_url.clone(),
                raw_url: Some(parsed.normalized.clone()),
            };
            match self
                .probe_forward_proxy_endpoint(
                    &endpoint,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                    &probe_url,
                )
                .await
            {
                Ok(latency_ms) => {
                    normalized_values.push(parsed.normalized.clone());
                    discovered_nodes += 1;
                    best_latency =
                        Some(best_latency.map_or(latency_ms, |current| current.min(latency_ms)));
                    results.push(ForwardProxyValidationProbeResult {
                        value: raw,
                        normalized_value: Some(parsed.normalized),
                        ok: true,
                        discovered_nodes: Some(1),
                        latency_ms: Some(latency_ms),
                        error_code: None,
                        message: "proxy validation succeeded".to_string(),
                    });
                }
                Err(err) => {
                    results.push(ForwardProxyValidationProbeResult {
                        value: raw,
                        normalized_value: Some(parsed.normalized),
                        ok: false,
                        discovered_nodes: Some(1),
                        latency_ms: None,
                        error_code: Some(map_forward_proxy_validation_error_code(&err)),
                        message: err.to_string(),
                    });
                }
            }
        }

        for subscription_url in forward_proxy::normalize_subscription_entries(subscription_urls) {
            match self
                .validate_forward_proxy_subscription(&subscription_url)
                .await
            {
                Ok((count, latency_ms, mut normalized)) => {
                    discovered_nodes += count;
                    best_latency =
                        Some(best_latency.map_or(latency_ms, |current| current.min(latency_ms)));
                    normalized_values.push(subscription_url.clone());
                    normalized_values.append(&mut normalized);
                    results.push(ForwardProxyValidationProbeResult {
                        value: subscription_url.clone(),
                        normalized_value: Some(subscription_url),
                        ok: true,
                        discovered_nodes: Some(count),
                        latency_ms: Some(latency_ms),
                        error_code: None,
                        message: "subscription validation succeeded".to_string(),
                    });
                }
                Err(err) => {
                    results.push(ForwardProxyValidationProbeResult {
                        value: subscription_url.clone(),
                        normalized_value: Some(subscription_url),
                        ok: false,
                        discovered_nodes: Some(0),
                        latency_ms: None,
                        error_code: Some(map_forward_proxy_validation_error_code(&err)),
                        message: err.to_string(),
                    });
                }
            }
        }

        normalized_values.sort();
        normalized_values.dedup();
        let ok = results.iter().any(|result| result.ok);
        let first_error =
            results
                .iter()
                .find(|result| !result.ok)
                .map(|result| ForwardProxyValidationError {
                    code: result
                        .error_code
                        .clone()
                        .unwrap_or_else(|| "validation_failed".to_string()),
                    message: result.message.clone(),
                });

        Ok(ForwardProxyValidationResponse {
            ok,
            normalized_values,
            discovered_nodes,
            latency_ms: best_latency,
            results,
            first_error,
        })
    }

    async fn validate_forward_proxy_subscription(
        &self,
        subscription_url: &str,
    ) -> Result<(usize, f64, Vec<String>), ProxyError> {
        let validation_timeout =
            Duration::from_secs(forward_proxy::FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS);
        let validation_started = Instant::now();
        let normalized_subscription =
            forward_proxy::normalize_subscription_entries(vec![subscription_url.to_string()])
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ProxyError::Other("subscription url must be a valid http/https url".to_string())
                })?;
        let urls = forward_proxy::fetch_subscription_proxy_urls_with_validation_budget(
            &self.forward_proxy_clients.direct_client(),
            &normalized_subscription,
            validation_timeout,
            validation_started,
        )
        .await
        .map_err(|err| {
            ProxyError::Other(format!(
                "failed to fetch or decode subscription payload: {err}"
            ))
        })?;
        if urls.is_empty() {
            return Err(ProxyError::Other(
                "subscription resolved zero proxy entries".to_string(),
            ));
        }
        let endpoints = forward_proxy::normalize_proxy_endpoints_from_urls(
            &urls,
            forward_proxy::FORWARD_PROXY_SOURCE_SUBSCRIPTION,
        );
        if endpoints.is_empty() {
            return Err(ProxyError::Other(
                "subscription contains no supported proxy entries".to_string(),
            ));
        }
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let timeout_error = || {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                validation_timeout.as_millis()
            ))
        };
        let mut last_error: Option<ProxyError> = None;
        let mut best_latency: Option<f64> = None;
        for endpoint in endpoints.iter().take(3) {
            let Some(remaining_timeout) =
                validation_timeout.checked_sub(validation_started.elapsed())
            else {
                last_error = Some(timeout_error());
                break;
            };
            if remaining_timeout.is_zero() {
                last_error = Some(timeout_error());
                break;
            }
            match self
                .probe_forward_proxy_endpoint(endpoint, remaining_timeout, &probe_url)
                .await
            {
                Ok(latency_ms) => {
                    best_latency = Some(latency_ms);
                    break;
                }
                Err(err) => {
                    if validation_started.elapsed() >= validation_timeout {
                        last_error = Some(timeout_error());
                        break;
                    }
                    last_error = Some(err);
                }
            }
        }
        let Some(latency_ms) = best_latency else {
            if let Some(err) = last_error {
                return Err(ProxyError::Other(format!(
                    "subscription proxy probe failed: {err}; no entry passed validation"
                )));
            }
            return Err(ProxyError::Other(
                "no subscription proxy entry passed validation".to_string(),
            ));
        };
        Ok((
            endpoints.len(),
            latency_ms,
            endpoints
                .into_iter()
                .filter_map(|endpoint| endpoint.raw_url)
                .collect(),
        ))
    }

    pub async fn refresh_forward_proxy_subscriptions(&self) -> Result<(), ProxyError> {
        let settings = {
            let manager = self.forward_proxy.lock().await;
            manager.settings.clone()
        };

        let mut subscription_urls = Vec::new();
        let mut fetched_any_subscription = false;
        for subscription_url in &settings.subscription_urls {
            match forward_proxy::fetch_subscription_proxy_urls(
                &self.forward_proxy_clients.direct_client(),
                subscription_url,
                Duration::from_secs(
                    forward_proxy::FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS,
                ),
            )
            .await
            {
                Ok(urls) => {
                    fetched_any_subscription = true;
                    subscription_urls.extend(urls);
                }
                Err(err) => {
                    eprintln!(
                        "failed to refresh forward proxy subscription {subscription_url}: {err}"
                    );
                }
            }
        }

        if !settings.subscription_urls.is_empty() && !fetched_any_subscription {
            return Err(ProxyError::Other(
                "all forward proxy subscriptions failed to refresh".to_string(),
            ));
        }

        let mut manager = self.forward_proxy.lock().await;
        manager.apply_subscription_urls(subscription_urls);
        {
            let mut xray = self.xray_supervisor.lock().await;
            xray.sync_endpoints(&mut manager.endpoints).await?;
        }
        let endpoints = manager.endpoints.clone();
        for endpoint in &endpoints {
            if let Some(runtime) = manager.runtime.get_mut(&endpoint.key) {
                runtime.kind = endpoint.protocol.as_str().to_string();
                runtime.endpoint_url = endpoint
                    .endpoint_url
                    .as_ref()
                    .map(Url::to_string)
                    .or_else(|| endpoint.raw_url.clone());
                runtime.available = endpoint.is_selectable();
                if endpoint.is_direct() || endpoint.is_selectable() {
                    runtime.last_error = None;
                } else if !endpoint.is_selectable() {
                    runtime.last_error = Some("xray_missing".to_string());
                }
            }
        }
        forward_proxy::sync_manager_runtime_to_store(&self.key_store, &manager).await
    }

    pub async fn maybe_run_forward_proxy_maintenance(&self) -> Result<(), ProxyError> {
        let should_refresh = {
            let manager = self.forward_proxy.lock().await;
            manager.should_refresh_subscriptions()
        };
        if should_refresh {
            self.refresh_forward_proxy_subscriptions().await?;
        }
        let probe_candidate = {
            let mut manager = self.forward_proxy.lock().await;
            manager
                .mark_probe_started()
                .and_then(|selected| manager.endpoint_by_key(&selected.key))
        };
        if let Some(endpoint) = probe_candidate {
            let probe_url = forward_proxy::derive_probe_url(&self.upstream);
            let probe_result = self
                .probe_forward_proxy_endpoint(
                    &endpoint,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                    &probe_url,
                )
                .await;
            match probe_result {
                Ok(latency_ms) => {
                    let _ = self
                        .record_forward_proxy_attempt_inner(
                            &endpoint.key,
                            true,
                            Some(latency_ms),
                            None,
                            true,
                        )
                        .await;
                }
                Err(err) => {
                    let failure_kind = map_forward_proxy_validation_error_code(&err);
                    let _ = self
                        .record_forward_proxy_attempt_inner(
                            &endpoint.key,
                            false,
                            None,
                            Some(failure_kind.as_str()),
                            true,
                        )
                        .await;
                }
            }
            let mut manager = self.forward_proxy.lock().await;
            manager.mark_probe_finished();
        }
        Ok(())
    }

    async fn probe_forward_proxy_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        timeout: Duration,
        probe_url: &Url,
    ) -> Result<f64, ProxyError> {
        let mut temporary_xray_key = None;
        let resolved = if endpoint.requires_xray() && endpoint.endpoint_url.is_none() {
            let raw_url = endpoint.raw_url.as_deref().ok_or_else(|| {
                ProxyError::Other("xray proxy validation requires raw proxy url".to_string())
            })?;
            let validate_key = format!(
                "__validate_xray__{:016x}",
                forward_proxy::stable_hash_u64(raw_url)
            );
            let validate_endpoint = forward_proxy::ForwardProxyEndpoint {
                key: validate_key.clone(),
                source: endpoint.source.clone(),
                display_name: endpoint.display_name.clone(),
                protocol: endpoint.protocol,
                endpoint_url: None,
                raw_url: Some(raw_url.to_string()),
            };
            let route_url = self
                .xray_supervisor
                .lock()
                .await
                .ensure_instance(&validate_endpoint)
                .await?;
            temporary_xray_key = Some(validate_key);
            forward_proxy::ForwardProxyEndpoint {
                endpoint_url: Some(route_url),
                ..validate_endpoint
            }
        } else {
            endpoint.clone()
        };
        let result = forward_proxy::probe_forward_proxy_endpoint(
            &self.forward_proxy_clients,
            &resolved,
            probe_url,
            timeout,
        )
        .await;
        if let Some(temp_key) = temporary_xray_key {
            self.xray_supervisor
                .lock()
                .await
                .remove_instance(&temp_key)
                .await;
        }
        result
    }

    async fn probe_and_record_forward_proxy_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        request_kind: &str,
        api_key_id: Option<&str>,
        timeout: Duration,
    ) -> Result<f64, ProxyError> {
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let result = self
            .probe_forward_proxy_endpoint(endpoint, timeout, &probe_url)
            .await;
        match result {
            Ok(latency_ms) => {
                self.record_forward_proxy_attempt(
                    endpoint.key.as_str(),
                    api_key_id,
                    request_kind,
                    true,
                    Some(latency_ms),
                    None,
                )
                .await?;
                Ok(latency_ms)
            }
            Err(err) => {
                let error_code = map_forward_proxy_validation_error_code(&err);
                self.record_forward_proxy_attempt(
                    endpoint.key.as_str(),
                    api_key_id,
                    request_kind,
                    false,
                    None,
                    Some(error_code.as_str()),
                )
                .await?;
                Err(err)
            }
        }
    }

    async fn load_proxy_affinity_record(
        &self,
        api_key_id: &str,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        {
            let cache = self.forward_proxy_affinity.lock().await;
            if let Some(record) = cache.get(api_key_id) {
                return Ok(record.clone());
            }
        }
        let record =
            forward_proxy::load_forward_proxy_key_affinity(&self.key_store.pool, api_key_id)
                .await?
                .unwrap_or_default();
        let mut cache = self.forward_proxy_affinity.lock().await;
        cache.insert(api_key_id.to_string(), record.clone());
        Ok(record)
    }

    async fn store_proxy_affinity_record(
        &self,
        api_key_id: &str,
        record: forward_proxy::ForwardProxyAffinityRecord,
    ) -> Result<(), ProxyError> {
        forward_proxy::save_forward_proxy_key_affinity(&self.key_store.pool, api_key_id, &record)
            .await?;
        let mut cache = self.forward_proxy_affinity.lock().await;
        cache.insert(api_key_id.to_string(), record);
        Ok(())
    }

    async fn reconcile_proxy_affinity_record(
        &self,
        api_key_id: &str,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        let mut record = self.load_proxy_affinity_record(api_key_id).await?;
        let now = Utc::now().timestamp();
        let mut manager = self.forward_proxy.lock().await;
        manager.ensure_non_zero_weight();

        let is_valid = |proxy_key: &str,
                        manager: &forward_proxy::ForwardProxyManager,
                        allow_direct_primary: bool| {
            let Some(endpoint) = manager.endpoint(proxy_key) else {
                return false;
            };
            if endpoint.is_direct() && !allow_direct_primary {
                return false;
            }
            manager.runtime(proxy_key).is_some_and(|runtime| {
                runtime.available && runtime.weight > 0.0 && endpoint.is_selectable()
            })
        };

        if let Some(primary) = record.primary_proxy_key.as_deref()
            && !is_valid(primary, &manager, true)
        {
            record.primary_proxy_key = None;
        }
        if let Some(secondary) = record.secondary_proxy_key.as_deref()
            && !is_valid(secondary, &manager, true)
        {
            record.secondary_proxy_key = None;
        }
        if record.primary_proxy_key == record.secondary_proxy_key {
            record.secondary_proxy_key = None;
        }

        if record.primary_proxy_key.is_none() {
            let exclude = HashSet::new();
            if let Some(primary) = manager
                .rank_candidates_for_subject(
                    &format!("{api_key_id}:primary"),
                    &exclude,
                    false,
                    forward_proxy::FORWARD_PROXY_DEFAULT_PRIMARY_CANDIDATE_COUNT,
                )
                .into_iter()
                .next()
                .or_else(|| {
                    manager
                        .rank_candidates_for_subject(
                            &format!("{api_key_id}:primary"),
                            &exclude,
                            true,
                            forward_proxy::FORWARD_PROXY_DEFAULT_PRIMARY_CANDIDATE_COUNT,
                        )
                        .into_iter()
                        .next()
                })
            {
                record.primary_proxy_key = Some(primary.key.clone());
            }
        }

        if record.secondary_proxy_key.is_none() {
            let mut exclude = HashSet::new();
            if let Some(primary) = record.primary_proxy_key.as_ref() {
                exclude.insert(primary.clone());
            }
            if let Some(secondary) = manager
                .rank_candidates_for_subject(
                    &format!("{api_key_id}:secondary"),
                    &exclude,
                    false,
                    forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                )
                .into_iter()
                .next()
                .or_else(|| {
                    manager
                        .rank_candidates_for_subject(
                            &format!("{api_key_id}:secondary"),
                            &exclude,
                            true,
                            forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                        )
                        .into_iter()
                        .next()
                })
            {
                record.secondary_proxy_key = Some(secondary.key.clone());
            }
        }

        if record.primary_proxy_key.is_none() && record.secondary_proxy_key.is_some() {
            record.primary_proxy_key = record.secondary_proxy_key.take();
        }
        record.updated_at = now;
        drop(manager);
        self.store_proxy_affinity_record(api_key_id, record.clone())
            .await?;
        Ok(record)
    }

    async fn promote_proxy_affinity_secondary(
        &self,
        api_key_id: &str,
        succeeded_proxy_key: &str,
    ) -> Result<(), ProxyError> {
        let mut record = self.reconcile_proxy_affinity_record(api_key_id).await?;
        if record.primary_proxy_key.as_deref() == Some(succeeded_proxy_key) {
            return Ok(());
        }
        if record.secondary_proxy_key.as_deref() == Some(succeeded_proxy_key) {
            record.primary_proxy_key = Some(succeeded_proxy_key.to_string());
            record.secondary_proxy_key = None;
            let manager = self.forward_proxy.lock().await;
            let mut exclude = HashSet::new();
            exclude.insert(succeeded_proxy_key.to_string());
            if let Some(next_secondary) = manager
                .rank_candidates_for_subject(
                    &format!("{api_key_id}:secondary"),
                    &exclude,
                    false,
                    forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                )
                .into_iter()
                .next()
                .or_else(|| {
                    manager
                        .rank_candidates_for_subject(
                            &format!("{api_key_id}:secondary"),
                            &exclude,
                            true,
                            forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                        )
                        .into_iter()
                        .next()
                })
            {
                record.secondary_proxy_key = Some(next_secondary.key.clone());
            }
            drop(manager);
            record.updated_at = Utc::now().timestamp();
            self.store_proxy_affinity_record(api_key_id, record).await?;
        }
        Ok(())
    }

    async fn build_proxy_attempt_plan(
        &self,
        api_key_id: &str,
    ) -> Result<Vec<forward_proxy::SelectedForwardProxy>, ProxyError> {
        let record = self.reconcile_proxy_affinity_record(api_key_id).await?;
        let manager = self.forward_proxy.lock().await;
        let mut plan = Vec::new();
        let mut seen = HashSet::new();
        for key in [
            record.primary_proxy_key.as_ref(),
            record.secondary_proxy_key.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if seen.insert(key.clone())
                && let Some(endpoint) = manager.endpoint(key)
                && endpoint.is_selectable()
            {
                plan.push(forward_proxy::SelectedForwardProxy::from_endpoint(endpoint));
            }
        }
        if plan.is_empty() {
            let exclude = HashSet::new();
            if let Some(endpoint) = manager
                .rank_candidates_for_subject(
                    &format!("{api_key_id}:fallback"),
                    &exclude,
                    true,
                    forward_proxy::FORWARD_PROXY_DEFAULT_PRIMARY_CANDIDATE_COUNT,
                )
                .into_iter()
                .next()
            {
                plan.push(forward_proxy::SelectedForwardProxy::from_endpoint(
                    &endpoint,
                ));
            }
        }
        Ok(plan)
    }

    async fn record_forward_proxy_attempt(
        &self,
        proxy_key: &str,
        _api_key_id: Option<&str>,
        _request_kind: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_forward_proxy_attempt_inner(proxy_key, success, latency_ms, failure_kind, false)
            .await
    }

    async fn record_forward_proxy_attempt_inner(
        &self,
        proxy_key: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
        is_probe: bool,
    ) -> Result<(), ProxyError> {
        forward_proxy::insert_forward_proxy_attempt(
            &self.key_store.pool,
            proxy_key,
            success,
            latency_ms,
            failure_kind,
            is_probe,
        )
        .await?;
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.record_attempt(proxy_key, success, latency_ms, failure_kind);
            if let Some(runtime) = manager.runtime(proxy_key).cloned() {
                let bucket_start = (Utc::now().timestamp() / 3600) * 3600;
                let sample_epoch_us = Utc::now().timestamp_nanos_opt().unwrap_or_default() / 1_000;
                forward_proxy::persist_forward_proxy_runtime_state(&self.key_store.pool, &runtime)
                    .await?;
                forward_proxy::upsert_forward_proxy_weight_hourly_bucket(
                    &self.key_store.pool,
                    proxy_key,
                    bucket_start,
                    runtime.weight,
                    sample_epoch_us,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn send_with_forward_proxy<F>(
        &self,
        api_key_id: &str,
        request_kind: &str,
        mut build: F,
    ) -> Result<(reqwest::Response, forward_proxy::SelectedForwardProxy), ProxyError>
    where
        F: FnMut(Client) -> reqwest::RequestBuilder,
    {
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.note_request();
        }
        if let Err(err) = self.maybe_run_forward_proxy_maintenance().await {
            eprintln!("forward-proxy maintenance error: {err}");
        }
        let plan: Vec<forward_proxy::SelectedForwardProxy> = self
            .build_proxy_attempt_plan(api_key_id)
            .await
            .unwrap_or_default();
        let mut last_error: Option<ProxyError> = None;
        for candidate in plan {
            let client = match self
                .forward_proxy_clients
                .client_for(candidate.endpoint_url.as_ref())
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let error_code = map_forward_proxy_validation_error_code(&err);
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            Some(api_key_id),
                            request_kind,
                            false,
                            None,
                            Some(error_code.as_str()),
                        )
                        .await;
                    last_error = Some(err);
                    continue;
                }
            };
            let started = Instant::now();
            match build(client).send().await {
                Ok(response) => {
                    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            Some(api_key_id),
                            request_kind,
                            true,
                            Some(latency_ms),
                            None,
                        )
                        .await;
                    let _ = self
                        .promote_proxy_affinity_secondary(api_key_id, &candidate.key)
                        .await;
                    return Ok((response, candidate));
                }
                Err(err) => {
                    let failure_kind = forward_proxy::failure_kind_from_http_error(&err);
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            Some(api_key_id),
                            request_kind,
                            false,
                            None,
                            Some(failure_kind),
                        )
                        .await;
                    last_error = Some(ProxyError::Http(err));
                }
            }
        }

        let direct = {
            let manager = self.forward_proxy.lock().await;
            manager
                .endpoint_by_key(forward_proxy::FORWARD_PROXY_DIRECT_KEY)
                .filter(|endpoint| endpoint.is_selectable())
                .map(|endpoint| forward_proxy::SelectedForwardProxy::from_endpoint(&endpoint))
        };
        let Some(direct) = direct else {
            return Err(last_error.unwrap_or_else(|| {
                ProxyError::Other("no selectable forward proxy endpoints available".to_string())
            }));
        };
        let client = self.forward_proxy_clients.direct_client();
        let started = Instant::now();
        match build(client).send().await {
            Ok(response) => {
                let _ = self
                    .record_forward_proxy_attempt(
                        &direct.key,
                        Some(api_key_id),
                        request_kind,
                        true,
                        Some(started.elapsed().as_secs_f64() * 1000.0),
                        None,
                    )
                    .await;
                Ok((response, direct))
            }
            Err(err) => {
                let _ = self
                    .record_forward_proxy_attempt(
                        &direct.key,
                        Some(api_key_id),
                        request_kind,
                        false,
                        None,
                        Some(forward_proxy::failure_kind_from_http_error(&err)),
                    )
                    .await;
                Err(last_error.unwrap_or(ProxyError::Http(err)))
            }
        }
    }

    async fn billing_subject_for_token(&self, token_id: &str) -> Result<String, ProxyError> {
        Ok(
            match self.key_store.find_user_id_by_token_fresh(token_id).await? {
                Some(user_id) => QuotaSubject::Account(user_id).billing_subject(),
                None => QuotaSubject::Token(token_id.to_string()).billing_subject(),
            },
        )
    }

    async fn reconcile_pending_billing_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<(), ProxyError> {
        let pending = self
            .key_store
            .list_pending_billing_log_ids(billing_subject)
            .await?;
        for log_id in pending {
            // `lock_token_billing()` already holds the per-subject lock at this point, so a
            // retry-later miss here is unexpected. We retry once to tolerate edge timing around
            // SQLite statement visibility, then fail closed so stale pending charges cannot bypass
            // the quota precheck for the current request.
            let mut retry_later_attempts = 0;
            loop {
                match self.key_store.apply_pending_billing_log(log_id).await? {
                    PendingBillingSettleOutcome::Charged
                    | PendingBillingSettleOutcome::AlreadySettled => break,
                    PendingBillingSettleOutcome::RetryLater => {
                        retry_later_attempts += 1;
                        if retry_later_attempts >= 2 {
                            let msg = format!(
                                "pending billing claim miss for auth_token_logs.id={log_id}; blocking request until replay succeeds",
                            );
                            eprintln!("{msg}");
                            let _ = self.annotate_pending_billing_attempt(log_id, &msg).await;
                            return Err(ProxyError::Other(msg));
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        }
        Ok(())
    }

    async fn lock_billing_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let lock = {
            let mut locks = self.token_billing_locks.lock().await;
            if locks.len() > 1024 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(billing_subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(billing_subject.to_string(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                billing_subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;
        Ok(TokenBillingGuard {
            billing_subject: billing_subject.to_string(),
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    /// Serialize quota/billing work per effective quota subject across both the local process
    /// and any other instances sharing the same SQLite database.
    pub async fn lock_token_billing(
        &self,
        token_id: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let current_subject = self.billing_subject_for_token(token_id).await?;
        let mut subjects = self
            .key_store
            .list_pending_billing_subjects_for_token(token_id)
            .await?;
        subjects.push(current_subject.clone());
        subjects.sort();
        subjects.dedup();

        let mut current_guard: Option<TokenBillingGuard> = None;
        let mut extra_guards: Vec<TokenBillingGuard> = Vec::new();
        for subject in subjects {
            let guard = self.lock_billing_subject(&subject).await?;
            self.reconcile_pending_billing_for_subject(guard.billing_subject())
                .await?;
            if subject == current_subject {
                current_guard = Some(guard);
            } else {
                extra_guards.push(guard);
            }
        }
        drop(extra_guards);

        current_guard.ok_or_else(|| {
            ProxyError::Other(format!(
                "failed to acquire billing guard for current subject {current_subject}",
            ))
        })
    }

    async fn lock_research_key_usage(&self, key_id: &str) -> Result<TokenBillingGuard, ProxyError> {
        let subject = format!("research-key:{key_id}");
        let lock = {
            let mut locks = self.research_key_locks.lock().await;
            if locks.len() > 256 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(&subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(subject.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                &subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;

        Ok(TokenBillingGuard {
            billing_subject: subject,
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    async fn acquire_key_for(
        &self,
        auth_token_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let now = Utc::now().timestamp();

        let Some(token_id) = auth_token_id else {
            // No token id (e.g. certain internal or dev flows) → plain global scheduling.
            return self.key_store.acquire_key().await;
        };

        // Step 1: 尝试使用当前有效的亲和 key（仅在 TTL 窗口内且未过期）。
        let candidate_key_id = {
            let mut state = self.affinity.lock().await;
            state.get_candidate(token_id, now)
        };

        if let Some(key_id) = candidate_key_id {
            if let Some(lease) = self.key_store.try_acquire_specific_key(&key_id).await? {
                return Ok(lease);
            }
            // 底层认为该 key 不再可用（禁用、删除等），清除亲和映射。
            let mut state = self.affinity.lock().await;
            state.drop_mapping(token_id);
        }

        // Step 2: 没有可用亲和 key → 使用全局 LRU 选取一把新 key，并建立新的亲和关系。
        let lease = self.key_store.acquire_key().await?;
        {
            let mut state = self.affinity.lock().await;
            state.record_mapping(token_id, &lease.id, now);
        }
        Ok(lease)
    }

    async fn acquire_key_for_research_request(
        &self,
        auth_token_id: Option<&str>,
        research_request_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let now = Utc::now().timestamp();

        if let Some(request_id) = research_request_id {
            let mut candidate_key_id = {
                let mut state = self.research_request_affinity.lock().await;
                state.get_candidate(request_id, now)
            };

            if candidate_key_id.is_none()
                && let Some((key_id, owner_token_id)) = self
                    .key_store
                    .get_research_request_affinity(request_id, now)
                    .await?
            {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                candidate_key_id = Some(key_id);
            }

            if let Some(key_id) = candidate_key_id {
                if let Some(lease) = self.key_store.try_acquire_specific_key(&key_id).await? {
                    if let Some(token_id) = auth_token_id {
                        let mut state = self.affinity.lock().await;
                        state.record_mapping(token_id, &lease.id, now);
                    }
                    return Ok(lease);
                }
                return Err(ProxyError::NoAvailableKeys);
            }
        }

        self.acquire_key_for(auth_token_id).await
    }

    async fn populate_research_request_affinity_caches(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        now: i64,
    ) {
        {
            let mut state = self.research_request_affinity.lock().await;
            state.record_mapping(request_id, key_id, now);
        }
        let mut owner_state = self.research_request_owner_affinity.lock().await;
        owner_state.record_mapping(request_id, token_id, now);
    }

    async fn record_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.populate_research_request_affinity_caches(request_id, key_id, token_id, now)
            .await;
        self.key_store
            .save_research_request_affinity(
                request_id,
                key_id,
                token_id,
                now + RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            )
            .await
    }

    pub async fn is_research_request_owned_by(
        &self,
        request_id: &str,
        token_id: Option<&str>,
    ) -> Result<bool, ProxyError> {
        let Some(token_id) = token_id else {
            return Ok(false);
        };

        let now = Utc::now().timestamp();
        if let Some(owner) = {
            let mut state = self.research_request_owner_affinity.lock().await;
            state.get_candidate(request_id, now)
        } {
            return Ok(owner == token_id);
        }

        match self
            .key_store
            .get_research_request_affinity(request_id, now)
            .await
        {
            Ok(Some((key_id, owner_token_id))) => {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                Ok(owner_token_id == token_id)
            }
            Ok(None) => Ok(false),
            Err(err) => Err(err),
        }
    }

    async fn reconcile_key_health(
        &self,
        lease: &ApiKeyLease,
        source: &str,
        analysis: &AttemptAnalysis,
    ) -> Result<(), ProxyError> {
        match &analysis.key_health_action {
            KeyHealthAction::None => self.key_store.restore_active_status(&lease.secret).await,
            KeyHealthAction::MarkExhausted => {
                let _changed = self.key_store.mark_quota_exhausted(&lease.secret).await?;
                Ok(())
            }
            KeyHealthAction::Quarantine(decision) => {
                self.key_store
                    .quarantine_key_by_id(
                        &lease.id,
                        source,
                        &decision.reason_code,
                        &decision.reason_summary,
                        &decision.reason_detail,
                    )
                    .await
            }
        }
    }

    async fn maybe_quarantine_usage_error(
        &self,
        key_id: &str,
        source: &str,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        let ProxyError::UsageHttp { status, body } = err else {
            return Ok(());
        };
        let Some(decision) =
            classify_quarantine_reason(Some(status.as_u16() as i64), body.as_bytes())
        else {
            return Ok(());
        };
        self.key_store
            .quarantine_key_by_id(
                key_id,
                source,
                &decision.reason_code,
                &decision.reason_summary,
                &decision.reason_detail,
            )
            .await
    }

    /// 将请求透传到 Tavily upstream 并记录日志。
    pub async fn proxy_request(&self, request: ProxyRequest) -> Result<ProxyResponse, ProxyError> {
        let lease = self
            .acquire_key_for(request.auth_token_id.as_deref())
            .await?;

        let mut url = self.upstream.clone();
        url.set_path(request.path.as_str());

        {
            let mut pairs = url.query_pairs_mut();
            if let Some(existing) = request.query.as_ref() {
                for (key, value) in form_urlencoded::parse(existing.as_bytes()) {
                    pairs.append_pair(&key, &value);
                }
            }
            pairs.append_pair("tavilyApiKey", lease.secret.as_str());
        }

        drop(url.query_pairs_mut());

        let sanitized_headers = self.sanitize_headers(&request.headers);
        let request_method = request.method.clone();
        let request_body = request.body.clone();
        let request_url = url.clone();
        let tavily_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "mcp", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
                    .header("Tavily-Api-Key", tavily_secret.as_str())
                    .body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                let outcome = analyze_attempt(status, &body_bytes);

                log_success(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    status,
                );

                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: Some(status),
                        tavily_status_code: outcome.tavily_status_code,
                        error: None,
                        request_body: &request.body,
                        response_body: &body_bytes,
                        outcome: outcome.status,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;

                self.reconcile_key_health(&lease, request.path.as_str(), &outcome)
                    .await?;

                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                })
            }
            Err(err) => {
                log_proxy_error(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    &err,
                );
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &request.body,
                        response_body: &[],
                        outcome: OUTCOME_ERROR,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP JSON endpoint (e.g. `/search`, `/extract`).
    /// It injects the Tavily key into the `api_key` field, performs header sanitization,
    /// records request logs with sensitive fields redacted, and updates key quota state.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_json_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let lease = self.acquire_key_for(auth_token_id).await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path(upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            // Remove any existing api_key field (case-insensitive) before inserting the Tavily key.
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            // Unexpected payload shape; wrap it so we still send a valid JSON object upstream.
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        // Force Tavily to return usage for predictable endpoints so we can charge credits 1:1.
        // Tavily does not document/support this on `/research` (we use /usage diff for that).
        if matches!(upstream_path, "/search" | "/extract" | "/crawl" | "/map")
            && let Value::Object(ref mut map) = upstream_options
        {
            map.insert("include_usage".to_string(), Value::Bool(true));
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_path.trim_start_matches('/'), |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let analysis = analyze_http_attempt(status, &body_bytes);
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && upstream_path == "/research"
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;

                self.reconcile_key_health(&lease, display_path, &analysis)
                    .await?;

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Proxy Tavily `/research` while charging credits via `/usage` (research_usage) diff.
    ///
    /// Tavily research responses do not include `usage.credits`, so we probe
    /// `GET {usage_base}/usage` before and after the call using the *same* upstream key.
    ///
    /// Returns the usage delta when both probes succeed; otherwise `None`.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_research_with_usage_diff(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis, Option<i64>), ProxyError> {
        let lease = self.acquire_key_for(auth_token_id).await?;
        // Research billing uses /usage diff of a key-scoped counter; protect it from concurrent
        // research calls sharing the same upstream key, otherwise deltas can be misattributed.
        let _key_guard = self.lock_research_key_usage(&lease.id).await?;

        let before_usage = match self
            .fetch_research_usage_for_secret_with_retries(
                &lease.secret,
                usage_base,
                Some(&lease.id),
                "research_usage_before",
            )
            .await
        {
            Ok(usage) => usage,
            Err(err) => {
                self.maybe_quarantine_usage_error(&lease.id, "/api/tavily/research#usage", &err)
                    .await?;
                return Err(err);
            }
        };

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path("/research");

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let analysis = analyze_http_attempt(status, &body_bytes);
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;

                self.reconcile_key_health(&lease, display_path, &analysis)
                    .await?;

                let after_usage = match self
                    .fetch_research_usage_for_secret_with_retries(
                        &lease.secret,
                        usage_base,
                        Some(&lease.id),
                        "research_usage_after",
                    )
                    .await
                {
                    Ok(usage) => Some(usage),
                    Err(err) => {
                        self.maybe_quarantine_usage_error(
                            &lease.id,
                            "/api/tavily/research#usage_after",
                            &err,
                        )
                        .await?;
                        None
                    }
                };
                let delta = match after_usage {
                    Some(after) if after >= before_usage => Some(after - before_usage),
                    _ => None,
                };

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                    },
                    analysis,
                    delta,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP endpoint with no request body
    /// (for example `GET /research/{request_id}`).
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_get_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let research_request_id = extract_research_request_id_from_path(upstream_path);
        let lease = self
            .acquire_key_for_research_request(auth_token_id, research_request_id.as_deref())
            .await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path(upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        let redacted_request_body: Vec<u8> = Vec::new();
        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research_result", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let analysis = analyze_http_attempt(status, &body_bytes);
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = research_request_id.as_deref()
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(request_id, &lease.id, token_id)
                        .await?;
                }

                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;

                self.reconcile_key_health(&lease, display_path, &analysis)
                    .await?;

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: &lease.id,
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Proxy a Tavily HTTP `/search` call via the usage base URL, performing key rotation
    /// and recording request logs with sensitive fields redacted.
    pub async fn proxy_http_search(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        self.proxy_http_json_endpoint(
            usage_base,
            "/search",
            auth_token_id,
            method,
            display_path,
            options,
            original_headers,
            true,
        )
        .await
    }

    /// 获取全部 API key 的统计信息，按状态与最近使用时间排序。
    pub async fn list_api_key_metrics(&self) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metrics(false).await
    }

    /// Admin: list API key metrics with pagination and optional filters.
    pub async fn list_api_key_metrics_paged(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        self.key_store
            .fetch_api_key_metrics_page(page, per_page, groups, statuses)
            .await
    }

    /// 获取单个 API key 的完整统计信息，包含隔离详情。
    pub async fn get_api_key_metric(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metric_by_id(key_id).await
    }

    /// 获取最近的请求日志，按时间倒序排列。
    pub async fn recent_request_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_recent_logs(limit).await
    }

    /// Admin: recent request logs with simple pagination and optional result_status filter.
    pub async fn recent_request_logs_page(
        &self,
        result_status: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        self.key_store
            .fetch_recent_logs_page(result_status, page, per_page)
            .await
    }

    /// 获取指定 key 在起始时间以来的汇总。
    pub async fn key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_key_summary_since(key_id, since).await
    }

    /// 获取指定 key 的最近日志（可选起始时间过滤）。
    pub async fn key_recent_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_key_logs(key_id, limit, since).await
    }

    // ----- Public auth token management API -----

    /// Validate an access token in format `th-<id>-<secret>` and record usage.
    /// Returns true if valid and enabled.
    pub async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        self.key_store.validate_access_token(token).await
    }

    /// Admin: create a new access token with optional note.
    pub async fn create_access_token(
        &self,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.create_access_token(note).await
    }

    /// Admin: batch create access tokens with required group name.
    pub async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        self.key_store
            .create_access_tokens_batch(group, count, note)
            .await
    }

    /// Admin: list tokens for management.
    pub async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_access_tokens().await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    /// Admin: list tokens paginated.
    pub async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let (mut tokens, total) = self
            .key_store
            .list_access_tokens_paged(page, per_page)
            .await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok((tokens, total))
    }

    async fn populate_token_quota(&self, tokens: &mut [AuthToken]) -> Result<(), ProxyError> {
        if tokens.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
        let verdicts = self.token_quota.snapshot_many(&ids).await?;
        let token_bindings = self.key_store.list_user_bindings_for_tokens(&ids).await?;
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % 60);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * 60;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let token_hourly_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let token_daily_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();
        let account_hourly_oldest = self
            .key_store
            .earliest_account_usage_bucket_since_bulk(
                &user_ids,
                GRANULARITY_MINUTE,
                hour_window_start,
            )
            .await?;
        let account_daily_oldest = self
            .key_store
            .earliest_account_usage_bucket_since_bulk(&user_ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let month_start = start_of_month(now);
        let next_month_reset = start_of_next_month(month_start).timestamp();
        for token in tokens.iter_mut() {
            if let Some(verdict) = verdicts.get(&token.id) {
                let hourly_oldest = if let Some(user_id) = token_bindings.get(&token.id) {
                    account_hourly_oldest.get(user_id).copied()
                } else {
                    token_hourly_oldest.get(&token.id).copied()
                };
                let daily_oldest = if let Some(user_id) = token_bindings.get(&token.id) {
                    account_daily_oldest.get(user_id).copied()
                } else {
                    token_daily_oldest.get(&token.id).copied()
                };
                token.quota_hourly_reset_at = if verdict.hourly_used > 0 {
                    hourly_oldest.map(|bucket| bucket + SECS_PER_HOUR)
                } else {
                    None
                };
                token.quota_daily_reset_at = if verdict.daily_used > 0 {
                    daily_oldest.map(|bucket| bucket + SECS_PER_DAY)
                } else {
                    None
                };
                token.quota_monthly_reset_at = if verdict.monthly_used > 0 {
                    Some(next_month_reset)
                } else {
                    None
                };
                token.quota = Some(verdict.clone());
            }
        }
        Ok(())
    }

    /// Admin: delete a token by id code.
    pub async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        self.key_store.delete_access_token(id).await
    }

    /// Admin: set token enabled/disabled.
    pub async fn set_access_token_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        self.key_store.set_access_token_enabled(id, enabled).await
    }

    /// Admin: update token note.
    pub async fn update_access_token_note(&self, id: &str, note: &str) -> Result<(), ProxyError> {
        self.key_store.update_access_token_note(id, note).await
    }

    /// Admin: get full token string for copy.
    pub async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store.get_access_token_secret(id).await
    }

    /// Admin: rotate token secret while keeping the same token id.
    /// Returns the new full token string (th-<id>-<secret>).
    pub async fn rotate_access_token_secret(
        &self,
        id: &str,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.rotate_access_token_secret(id).await
    }

    /// Create a one-time OAuth login state with TTL for CSRF/replay protection.
    pub async fn create_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            None,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash.
    pub async fn create_oauth_login_state_with_binding(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            binding_hash,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash and token id.
    pub async fn create_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .insert_oauth_login_state(provider, redirect_to, ttl_secs, binding_hash, bind_token_id)
            .await
    }

    /// Consume and invalidate an OAuth login state. Returns redirect target when valid.
    pub async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, None)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state bound to optional browser context hash.
    pub async fn consume_oauth_login_state_with_binding(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, binding_hash)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state and return all payload fields.
    pub async fn consume_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        self.key_store
            .consume_oauth_login_state(provider, state, binding_hash)
            .await
    }

    /// Upsert local user identity from third-party OAuth profile.
    pub async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.key_store.upsert_oauth_account(profile).await
    }

    /// Check whether a third-party account already exists locally.
    pub async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .oauth_account_exists(provider, provider_user_id)
            .await
    }

    /// Read whether first-time third-party registration is enabled.
    pub async fn allow_registration(&self) -> Result<bool, ProxyError> {
        self.key_store.allow_registration().await
    }

    /// Persist whether first-time third-party registration is enabled.
    pub async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.key_store.set_allow_registration(allow).await
    }

    /// Ensure one-to-one user token binding exists, creating a token only when missing.
    pub async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding(user_id, note)
            .await
    }

    /// Ensure binding with an optional preferred token id. Falls back to default behavior.
    pub async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding_with_preferred(user_id, note, preferred_token_id)
            .await
    }

    /// Fetch current user token by user_id. Does not auto-recreate when unavailable.
    pub async fn get_user_token(&self, user_id: &str) -> Result<UserTokenLookup, ProxyError> {
        self.key_store.get_user_token(user_id).await
    }

    /// List tokens bound to the specified user.
    pub async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_user_tokens(user_id).await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    /// Verify whether a token belongs to the specified user.
    pub async fn is_user_token_bound(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.is_user_token_bound(user_id, token_id).await
    }

    /// Get a token secret only when the token belongs to the specified user.
    pub async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store
            .get_user_token_secret(user_id, token_id)
            .await
    }

    /// User-level quota and usage summary for dashboard.
    pub async fn user_dashboard_summary(
        &self,
        user_id: &str,
    ) -> Result<UserDashboardSummary, ProxyError> {
        let account = self
            .token_quota
            .snapshot_for_user(user_id)
            .await?
            .unwrap_or(AccountQuotaSnapshot {
                hourly_any_used: 0,
                hourly_any_limit: 0,
                hourly_used: 0,
                hourly_limit: 0,
                daily_used: 0,
                daily_limit: 0,
                monthly_used: 0,
                monthly_limit: 0,
            });
        let (monthly_success, daily_success, daily_failure) =
            self.key_store.fetch_user_success_failure(user_id).await?;
        let last_activity = self.key_store.fetch_user_last_activity(user_id).await?;
        Ok(UserDashboardSummary {
            hourly_any_used: account.hourly_any_used,
            hourly_any_limit: account.hourly_any_limit,
            quota_hourly_used: account.hourly_used,
            quota_hourly_limit: account.hourly_limit,
            quota_daily_used: account.daily_used,
            quota_daily_limit: account.daily_limit,
            quota_monthly_used: account.monthly_used,
            quota_monthly_limit: account.monthly_limit,
            daily_success,
            daily_failure,
            monthly_success,
            last_activity,
        })
    }

    /// Admin: list users with pagination and optional fuzzy query.
    pub async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        self.key_store
            .list_admin_users_paged(page, per_page, query, tag_id)
            .await
    }

    /// Admin: get a single user identity by id.
    pub async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        self.key_store.get_admin_user_identity(user_id).await
    }

    /// Admin: resolve token owners in bulk for management views.
    pub async fn get_admin_token_owners(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let token_bindings = self
            .key_store
            .list_user_bindings_for_tokens(token_ids)
            .await?;
        if token_bindings.is_empty() {
            return Ok(HashMap::new());
        }

        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();

        let user_map = self.key_store.get_admin_user_identities(&user_ids).await?;
        let mut owners = HashMap::with_capacity(token_bindings.len());
        for (token_id, user_id) in token_bindings {
            if let Some(identity) = user_map.get(&user_id) {
                owners.insert(token_id, identity.clone());
            }
        }
        Ok(owners)
    }

    /// Admin: upsert account quota limits for a user.
    pub async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_quota_limits(
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
            )
            .await
    }

    /// Admin: list all user tag definitions.
    pub async fn list_user_tags(&self) -> Result<Vec<AdminUserTag>, ProxyError> {
        Ok(self
            .key_store
            .list_user_tags()
            .await?
            .into_iter()
            .map(|tag| to_admin_user_tag(&tag))
            .collect())
    }

    /// Admin: create a custom user tag.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<AdminUserTag, ProxyError> {
        self.key_store
            .create_user_tag(
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| to_admin_user_tag(&tag))
    }

    /// Admin: update an existing user tag definition.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_user_tag(
        &self,
        tag_id: &str,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<Option<AdminUserTag>, ProxyError> {
        self.key_store
            .update_user_tag(
                tag_id,
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| tag.map(|it| to_admin_user_tag(&it)))
    }

    /// Admin: delete a custom user tag definition.
    pub async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        self.key_store.delete_user_tag(tag_id).await
    }

    /// Admin: bind a custom tag to a user.
    pub async fn bind_user_tag_to_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.bind_user_tag_to_user(user_id, tag_id).await
    }

    /// Admin: unbind a tag from a user.
    pub async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .unbind_user_tag_from_user(user_id, tag_id)
            .await
    }

    /// Admin: list tag bindings for a set of users.
    pub async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<AdminUserTagBinding>>, ProxyError> {
        let bindings = self
            .key_store
            .list_user_tag_bindings_for_users(user_ids)
            .await?;
        Ok(bindings
            .into_iter()
            .map(|(user_id, items)| {
                (
                    user_id,
                    items
                        .into_iter()
                        .map(|binding| to_admin_user_tag_binding(&binding))
                        .collect(),
                )
            })
            .collect())
    }

    /// Admin: resolve base/effective quota and breakdown for a user.
    pub async fn get_admin_user_quota_details(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserQuotaDetails>, ProxyError> {
        let Some(_) = self.key_store.get_admin_user_identity(user_id).await? else {
            return Ok(None);
        };
        let resolution = self
            .key_store
            .resolve_account_quota_resolution(user_id)
            .await?;
        Ok(Some(AdminUserQuotaDetails {
            base: to_admin_quota_limit_set(&resolution.base),
            effective: to_admin_quota_limit_set(&resolution.effective),
            breakdown: resolution
                .breakdown
                .iter()
                .map(to_admin_quota_breakdown_entry)
                .collect(),
            tags: resolution
                .tags
                .iter()
                .map(to_admin_user_tag_binding)
                .collect(),
        }))
    }

    /// Create persisted user session.
    pub async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        self.key_store
            .create_user_session(user, session_max_age_secs)
            .await
    }

    /// Lookup valid user session from cookie token.
    pub async fn get_user_session(&self, token: &str) -> Result<Option<UserSession>, ProxyError> {
        self.key_store.get_user_session(token).await
    }

    /// Revoke persisted user session token.
    pub async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        self.key_store.revoke_user_session(token).await
    }

    /// Record a token usage log. Intended for /mcp proxy handler.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_token_attempt_with_kind(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            &request_kind,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_token_log(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                request_kind,
            )
            .await
    }

    /// Persist a billable attempt before quota counters are charged, so it can be replayed if the
    /// process crashes after the upstream call succeeds.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_with_kind(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &request_kind,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
    ) -> Result<i64, ProxyError> {
        let billing_subject = self.billing_subject_for_token(token_id).await?;
        self.record_pending_billing_attempt_for_subject_with_kind(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &billing_subject,
            request_kind,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_for_subject_with_kind(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            &request_kind,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .insert_token_log_pending_billing(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                business_credits,
                billing_subject,
                request_kind,
            )
            .await
    }

    pub async fn settle_pending_billing_attempt(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        self.key_store.apply_pending_billing_log(log_id).await
    }

    pub async fn annotate_pending_billing_attempt(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .annotate_pending_billing_log(log_id, message)
            .await
    }

    #[cfg(test)]
    async fn force_pending_billing_claim_miss_once(&self, log_id: i64) {
        let mut forced = self
            .key_store
            .forced_pending_claim_miss_log_ids
            .lock()
            .await;
        forced.insert(log_id);
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn force_quota_subject_lock_loss_once_for_subject(&self, billing_subject: &str) {
        let mut forced = self
            .key_store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        forced.insert(billing_subject.to_string());
    }

    /// Token summary since a timestamp
    pub async fn token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        self.key_store
            .fetch_token_summary_since(token_id, since, until)
            .await
    }

    /// Token recent logs with optional before-id pagination
    pub async fn token_recent_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        self.key_store
            .fetch_token_logs(token_id, limit, before_id)
            .await
    }

    /// Check and update quota usage for a token. Returns the latest counts and verdict.
    pub async fn check_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        self.token_quota.check(token_id).await
    }

    /// Read-only snapshot of the current business quota usage for a token (hour/day/month).
    /// This does NOT increment any counters.
    pub async fn peek_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota.snapshot_for_token(token_id, now).await
    }

    /// Read-only snapshot for a locked billing subject. Use this when a request must keep the
    /// same quota subject from precheck through charge even if token bindings change mid-flight.
    pub async fn peek_token_quota_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota
            .snapshot_for_billing_subject(billing_subject, now)
            .await
    }

    /// Charge business quota usage for a token by Tavily credits (1:1).
    /// `credits <= 0` is treated as a no-op.
    pub async fn charge_token_quota(&self, token_id: &str, credits: i64) -> Result<(), ProxyError> {
        self.token_quota.charge(token_id, credits).await
    }

    /// Check and update the hourly *raw request* usage for a token.
    /// This limiter counts every authenticated request (regardless of MCP method)
    /// within the last rolling hour and enforces `TOKEN_HOURLY_REQUEST_LIMIT`.
    pub async fn check_token_hourly_requests(
        &self,
        token_id: &str,
    ) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        self.token_request_limit.check(token_id).await
    }

    /// Read-only snapshot of hourly raw request usage for a set of tokens.
    /// Used by dashboards / leaderboards; does not increment counters.
    pub async fn token_hourly_any_snapshot(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        self.token_request_limit.snapshot_many(token_ids).await
    }

    /// Read-only snapshot of current token quota usage (hour / day / month).
    pub async fn token_quota_snapshot(
        &self,
        token_id: &str,
    ) -> Result<Option<TokenQuotaVerdict>, ProxyError> {
        let now = Utc::now();
        let verdict = self.token_quota.snapshot_for_token(token_id, now).await?;
        Ok(Some(verdict))
    }

    /// Token logs (page-based pagination)
    pub async fn token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
    ) -> Result<(Vec<TokenLogRecord>, i64), ProxyError> {
        self.key_store
            .fetch_token_logs_page(token_id, page, per_page, since, until, request_kinds)
            .await
    }

    pub async fn token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        self.key_store
            .fetch_token_log_request_kind_options(token_id, since, until)
            .await
    }

    /// Hourly breakdown for recent N hours (success + non-success aggregated as error).
    pub async fn token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        self.key_store
            .fetch_token_hourly_breakdown(token_id, hours)
            .await
    }

    /// Generic usage series for arbitrary window and granularity.
    pub async fn token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        self.key_store
            .fetch_token_usage_series(token_id, since, until, bucket_secs)
            .await
    }

    /// 根据 ID 获取真实 API key，仅供管理员调用。
    pub async fn get_api_key_secret(&self, key_id: &str) -> Result<Option<String>, ProxyError> {
        self.key_store.fetch_api_key_secret(key_id).await
    }

    /// Admin: add or undelete an API key. Returns the key ID.
    pub async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.key_store.add_or_undelete_key(api_key).await
    }

    /// Admin: add or undelete an API key and optionally assign it to a group.
    pub async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .add_or_undelete_key_in_group(api_key, group)
            .await
    }

    /// Admin: add/undelete an API key and return the upsert status.
    pub async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status(api_key)
            .await
    }

    /// Admin: add/undelete an API key in the provided group and return the upsert status.
    pub async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status_in_group(api_key, group)
            .await
    }

    /// Admin: soft delete a key by ID.
    pub async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.soft_delete_key_by_id(key_id).await
    }

    /// Admin: disable a key by ID.
    pub async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.disable_key_by_id(key_id).await
    }

    /// Admin: enable a key by ID (from disabled/exhausted -> active).
    pub async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.enable_key_by_id(key_id).await
    }

    /// Admin: clear the active quarantine record for a key.
    pub async fn clear_key_quarantine_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.clear_key_quarantine_by_id(key_id).await
    }

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
    }

    /// Public metrics: successful requests today and this month.
    pub async fn success_breakdown(&self) -> Result<SuccessBreakdown, ProxyError> {
        let now = Local::now();
        let month_start = start_of_local_month_utc_ts(now);
        let day_start = start_of_local_day_utc_ts(now);
        self.key_store
            .fetch_success_breakdown(month_start, day_start)
            .await
    }

    /// Token-scoped success/failure breakdown.
    pub async fn token_success_breakdown(
        &self,
        token_id: &str,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let day_start = start_of_day(now).timestamp();
        self.key_store
            .fetch_token_success_failure(token_id, month_start, day_start)
            .await
    }

    fn sanitize_headers(&self, headers: &HeaderMap) -> SanitizedHeaders {
        sanitize_headers_inner(headers, &self.upstream, &self.upstream_origin)
    }
}

impl TokenQuota {
    fn new(store: Arc<KeyStore>) -> Self {
        Self {
            store,
            cleanup: Arc::new(Mutex::new(CleanupState::default())),
            hourly_limit: effective_token_hourly_limit(),
            daily_limit: effective_token_daily_limit(),
            monthly_limit: effective_token_monthly_limit(),
        }
    }

    async fn resolve_subject(&self, token_id: &str) -> Result<QuotaSubject, ProxyError> {
        if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
            Ok(QuotaSubject::Account(user_id))
        } else {
            Ok(QuotaSubject::Token(token_id.to_string()))
        }
    }

    async fn check(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);

        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let month_start = start_of_month(now).timestamp();

        let verdict = match self.resolve_subject(token_id).await? {
            QuotaSubject::Account(user_id) => {
                let resolution = self
                    .store
                    .resolve_account_quota_resolution(&user_id)
                    .await?;
                let limits = resolution.effective;
                if limits.hourly_limit <= 0 || limits.daily_limit <= 0 || limits.monthly_limit <= 0
                {
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_MINUTE, hour_window_start)
                        .await?;
                    let daily_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_HOUR, day_window_start)
                        .await?;
                    let monthly_used = self
                        .store
                        .fetch_account_monthly_count(&user_id, month_start)
                        .await?;
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    )
                } else {
                    self.store
                        .increment_account_usage_bucket(&user_id, minute_bucket, GRANULARITY_MINUTE)
                        .await?;
                    self.store
                        .increment_account_usage_bucket(&user_id, hour_bucket, GRANULARITY_HOUR)
                        .await?;
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_MINUTE, hour_window_start)
                        .await?;
                    let daily_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_HOUR, day_window_start)
                        .await?;
                    let monthly_used = self
                        .store
                        .increment_account_monthly_quota(&user_id, month_start)
                        .await?;
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    )
                }
            }
            QuotaSubject::Token(token_id) => {
                // Increment usage buckets and monthly quota as an approximate, cheap counter
                // for *business* quota decisions. This path is allowed to drift slightly
                // from the detailed logs in exchange for lower per-request overhead.
                self.store
                    .increment_usage_bucket(&token_id, minute_bucket, GRANULARITY_MINUTE)
                    .await?;
                self.store
                    .increment_usage_bucket(&token_id, hour_bucket, GRANULARITY_HOUR)
                    .await?;

                let hourly_used = self
                    .store
                    .sum_usage_buckets(&token_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .store
                    .sum_usage_buckets(&token_id, GRANULARITY_HOUR, day_window_start)
                    .await?;
                let monthly_used = self
                    .store
                    .increment_monthly_quota(&token_id, month_start)
                    .await?;

                TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                )
            }
        };

        self.maybe_cleanup(now_ts).await?;
        Ok(verdict)
    }

    async fn charge(&self, token_id: &str, credits: i64) -> Result<(), ProxyError> {
        if credits <= 0 {
            return Ok(());
        }

        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let month_start = start_of_month(now).timestamp();

        match self.resolve_subject(token_id).await? {
            QuotaSubject::Account(user_id) => {
                self.store
                    .increment_account_usage_bucket_by(
                        &user_id,
                        minute_bucket,
                        GRANULARITY_MINUTE,
                        credits,
                    )
                    .await?;
                self.store
                    .increment_account_usage_bucket_by(
                        &user_id,
                        hour_bucket,
                        GRANULARITY_HOUR,
                        credits,
                    )
                    .await?;
                let _ = self
                    .store
                    .increment_account_monthly_quota_by(&user_id, month_start, credits)
                    .await?;
            }
            QuotaSubject::Token(token_id) => {
                self.store
                    .increment_usage_bucket_by(
                        &token_id,
                        minute_bucket,
                        GRANULARITY_MINUTE,
                        credits,
                    )
                    .await?;
                self.store
                    .increment_usage_bucket_by(&token_id, hour_bucket, GRANULARITY_HOUR, credits)
                    .await?;
                let _ = self
                    .store
                    .increment_monthly_quota_by(&token_id, month_start, credits)
                    .await?;
            }
        }

        self.maybe_cleanup(now_ts).await?;
        Ok(())
    }

    async fn snapshot_for_token(
        &self,
        token_id: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let subject = self.resolve_subject(token_id).await?;
        self.snapshot_for_subject(&subject, now).await
    }

    async fn snapshot_for_billing_subject(
        &self,
        billing_subject: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let subject = QuotaSubject::from_billing_subject(billing_subject)?;
        self.snapshot_for_subject(&subject, now).await
    }

    async fn snapshot_for_subject(
        &self,
        subject: &QuotaSubject,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let month_start = start_of_month(now).timestamp();
        match subject {
            QuotaSubject::Account(user_id) => {
                let limits = self
                    .store
                    .resolve_account_quota_resolution(user_id)
                    .await?
                    .effective;
                let hourly_used = self
                    .store
                    .sum_account_usage_buckets(user_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .store
                    .sum_account_usage_buckets(user_id, GRANULARITY_HOUR, day_window_start)
                    .await?;
                let monthly_used = self
                    .store
                    .fetch_account_monthly_count(user_id, month_start)
                    .await?;
                Ok(TokenQuotaVerdict::new(
                    hourly_used,
                    limits.hourly_limit,
                    daily_used,
                    limits.daily_limit,
                    monthly_used,
                    limits.monthly_limit,
                ))
            }
            QuotaSubject::Token(token_id) => {
                let hourly_used = self
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_HOUR, day_window_start)
                    .await?;
                let monthly_used = self
                    .store
                    .fetch_monthly_count(token_id, month_start)
                    .await?;
                Ok(TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                ))
            }
        }
    }

    async fn snapshot_for_user(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountQuotaSnapshot>, ProxyError> {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let month_start = start_of_month(now).timestamp();
        let limits = self
            .store
            .resolve_account_quota_resolution(user_id)
            .await?
            .effective;
        let hourly_any_used = self
            .store
            .sum_account_usage_buckets(user_id, GRANULARITY_REQUEST_MINUTE, hour_window_start)
            .await?;
        let hourly_used = self
            .store
            .sum_account_usage_buckets(user_id, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let daily_used = self
            .store
            .sum_account_usage_buckets(user_id, GRANULARITY_HOUR, day_window_start)
            .await?;
        let monthly_used = self
            .store
            .fetch_account_monthly_count(user_id, month_start)
            .await?;
        Ok(Some(AccountQuotaSnapshot {
            hourly_any_used,
            hourly_any_limit: limits.hourly_any_limit,
            hourly_used,
            hourly_limit: limits.hourly_limit,
            daily_used,
            daily_limit: limits.daily_limit,
            monthly_used,
            monthly_limit: limits.monthly_limit,
        }))
    }

    async fn snapshot_many(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenQuotaVerdict>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let month_start = start_of_month(now).timestamp();

        let token_bindings = self.store.list_user_bindings_for_tokens(token_ids).await?;
        let mut token_subjects: Vec<String> = Vec::new();
        let mut account_subjects: Vec<(String, String)> = Vec::new();
        let mut account_user_ids: Vec<String> = Vec::new();
        for token_id in token_ids {
            if let Some(user_id) = token_bindings.get(token_id) {
                account_subjects.push((token_id.clone(), user_id.clone()));
                account_user_ids.push(user_id.clone());
            } else {
                token_subjects.push(token_id.clone());
            }
        }
        account_user_ids.sort_unstable();
        account_user_ids.dedup();

        let token_hourly_totals = self
            .store
            .sum_usage_buckets_bulk(&token_subjects, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let token_daily_totals = self
            .store
            .sum_usage_buckets_bulk(&token_subjects, GRANULARITY_HOUR, day_window_start)
            .await?;
        let token_monthly_totals = self
            .store
            .fetch_monthly_counts(&token_subjects, month_start)
            .await?;

        let mut verdicts = HashMap::new();
        for token_id in token_subjects {
            let hourly_used = token_hourly_totals.get(&token_id).copied().unwrap_or(0);
            let daily_used = token_daily_totals.get(&token_id).copied().unwrap_or(0);
            let monthly_used = token_monthly_totals.get(&token_id).copied().unwrap_or(0);
            verdicts.insert(
                token_id,
                TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                ),
            );
        }
        if !account_user_ids.is_empty() {
            let account_limits = self
                .store
                .resolve_account_quota_limits_bulk(&account_user_ids)
                .await?;
            let account_hourly_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_MINUTE,
                    hour_window_start,
                )
                .await?;
            let account_daily_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_HOUR,
                    day_window_start,
                )
                .await?;
            let account_monthly_totals = self
                .store
                .fetch_account_monthly_counts(&account_user_ids, month_start)
                .await?;
            let default_limits = AccountQuotaLimits::zero_base();

            for (token_id, user_id) in account_subjects {
                let limits = account_limits
                    .get(&user_id)
                    .cloned()
                    .unwrap_or_else(|| default_limits.clone());
                let hourly_used = account_hourly_totals.get(&user_id).copied().unwrap_or(0);
                let daily_used = account_daily_totals.get(&user_id).copied().unwrap_or(0);
                let monthly_used = account_monthly_totals.get(&user_id).copied().unwrap_or(0);
                verdicts.insert(
                    token_id,
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    ),
                );
            }
        }
        Ok(verdicts)
    }

    async fn maybe_cleanup(&self, now_ts: i64) -> Result<(), ProxyError> {
        let should_prune = {
            let mut guard = self.cleanup.lock().await;
            if now_ts - guard.last_pruned < CLEANUP_INTERVAL_SECS {
                false
            } else {
                guard.last_pruned = now_ts;
                true
            }
        };

        if should_prune {
            let threshold = now_ts - BUCKET_RETENTION_SECS;
            self.store
                .delete_old_usage_buckets(GRANULARITY_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_usage_buckets(GRANULARITY_HOUR, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_HOUR, threshold)
                .await?;
        }

        Ok(())
    }
}

impl TokenRequestLimit {
    fn new(store: Arc<KeyStore>) -> Self {
        Self {
            store,
            cleanup: Arc::new(Mutex::new(CleanupState::default())),
            hourly_limit: effective_token_hourly_request_limit(),
        }
    }

    async fn check(&self, token_id: &str) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let verdict =
            if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
                let limits = self
                    .store
                    .resolve_account_quota_resolution(&user_id)
                    .await?
                    .effective;
                if limits.hourly_any_limit <= 0 {
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(
                            &user_id,
                            GRANULARITY_REQUEST_MINUTE,
                            hour_window_start,
                        )
                        .await?;
                    TokenHourlyRequestVerdict::new(hourly_used, limits.hourly_any_limit)
                } else {
                    self.store
                        .increment_account_usage_bucket(
                            &user_id,
                            minute_bucket,
                            GRANULARITY_REQUEST_MINUTE,
                        )
                        .await?;
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(
                            &user_id,
                            GRANULARITY_REQUEST_MINUTE,
                            hour_window_start,
                        )
                        .await?;
                    TokenHourlyRequestVerdict::new(hourly_used, limits.hourly_any_limit)
                }
            } else {
                // Increment per-minute raw request bucket for this token.
                self.store
                    .increment_usage_bucket(token_id, minute_bucket, GRANULARITY_REQUEST_MINUTE)
                    .await?;

                let hourly_used = self
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_REQUEST_MINUTE, hour_window_start)
                    .await?;
                TokenHourlyRequestVerdict::new(hourly_used, self.hourly_limit)
            };

        self.maybe_cleanup(now_ts).await?;
        Ok(verdict)
    }

    /// Read-only snapshot of hourly raw request usage for a set of tokens.
    /// This does NOT increment counters and is intended for dashboards / leaderboards.
    async fn snapshot_many(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now_ts = Utc::now().timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;

        let token_bindings = self.store.list_user_bindings_for_tokens(token_ids).await?;
        let mut token_subjects: Vec<String> = Vec::new();
        let mut account_subjects: Vec<(String, String)> = Vec::new();
        let mut account_user_ids: Vec<String> = Vec::new();
        for token_id in token_ids {
            if let Some(user_id) = token_bindings.get(token_id) {
                account_subjects.push((token_id.clone(), user_id.clone()));
                account_user_ids.push(user_id.clone());
            } else {
                token_subjects.push(token_id.clone());
            }
        }
        account_user_ids.sort_unstable();
        account_user_ids.dedup();

        let mut map = HashMap::new();
        let token_totals = self
            .store
            .sum_usage_buckets_bulk(
                &token_subjects,
                GRANULARITY_REQUEST_MINUTE,
                hour_window_start,
            )
            .await?;
        for token_id in token_subjects {
            let used = token_totals.get(&token_id).copied().unwrap_or(0);
            map.insert(
                token_id,
                TokenHourlyRequestVerdict::new(used, self.hourly_limit),
            );
        }

        if !account_user_ids.is_empty() {
            let account_limits = self
                .store
                .resolve_account_quota_limits_bulk(&account_user_ids)
                .await?;
            let account_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_REQUEST_MINUTE,
                    hour_window_start,
                )
                .await?;
            let default_hourly_any_limit = AccountQuotaLimits::zero_base().hourly_any_limit;
            for (token_id, user_id) in account_subjects {
                let used = account_totals.get(&user_id).copied().unwrap_or(0);
                let limit = account_limits
                    .get(&user_id)
                    .map(|limits| limits.hourly_any_limit)
                    .unwrap_or(default_hourly_any_limit);
                map.insert(token_id, TokenHourlyRequestVerdict::new(used, limit));
            }
        }
        Ok(map)
    }

    async fn maybe_cleanup(&self, now_ts: i64) -> Result<(), ProxyError> {
        let should_prune = {
            let mut guard = self.cleanup.lock().await;
            if now_ts - guard.last_pruned < CLEANUP_INTERVAL_SECS {
                false
            } else {
                guard.last_pruned = now_ts;
                true
            }
        };

        if should_prune {
            let threshold = now_ts - BUCKET_RETENTION_SECS;
            self.store
                .delete_old_usage_buckets(GRANULARITY_REQUEST_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_REQUEST_MINUTE, threshold)
                .await?;
        }

        Ok(())
    }
}

impl TavilyProxy {
    /// List keys whose quota hasn't been synced within `older_than_secs` seconds (or never).
    pub async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .list_keys_pending_quota_sync(older_than_secs)
            .await
    }

    /// Sync usage/quota for specific key via Tavily Usage API base (e.g., https://api.tavily.com).
    pub async fn sync_key_quota(
        &self,
        key_id: &str,
        usage_base: &str,
    ) -> Result<(i64, i64), ProxyError> {
        let Some(secret) = self.key_store.fetch_api_key_secret(key_id).await? else {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        };
        let (limit, remaining) = match self
            .fetch_usage_quota_for_secret(&secret, usage_base, None, Some(key_id), "quota_sync")
            .await
        {
            Ok(quota) => quota,
            Err(err) => {
                self.maybe_quarantine_usage_error(key_id, "/api/tavily/usage", &err)
                    .await?;
                return Err(err);
            }
        };
        let now = Utc::now().timestamp();
        self.key_store
            .update_quota_for_key(key_id, limit, remaining, now)
            .await?;
        Ok((limit, remaining))
    }

    /// Probe usage/quota for an API key secret via Tavily Usage API base (e.g., https://api.tavily.com).
    /// This performs *no* database mutation and is safe to use for admin validation flows.
    pub async fn probe_api_key_quota(
        &self,
        api_key: &str,
        usage_base: &str,
    ) -> Result<(i64, i64), ProxyError> {
        self.fetch_usage_quota_for_secret(
            api_key,
            usage_base,
            Some(Duration::from_secs(USAGE_PROBE_TIMEOUT_SECS)),
            None,
            "quota_probe",
        )
        .await
    }

    /// Admin: mark a key as quota-exhausted by its secret string.
    pub async fn mark_key_quota_exhausted_by_secret(
        &self,
        api_key: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.mark_quota_exhausted(api_key).await
    }

    async fn fetch_usage_quota_for_secret(
        &self,
        secret: &str,
        usage_base: &str,
        timeout: Option<Duration>,
        api_key_id: Option<&str>,
        request_kind: &str,
    ) -> Result<(i64, i64), ProxyError> {
        let base = Url::parse(usage_base).map_err(|e| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_string(),
            source: e,
        })?;
        let mut url = base.clone();
        url.set_path("/usage");

        let secret_header = secret.to_string();
        let request_url = url.clone();
        let resp = match api_key_id {
            Some(api_key_id) => self
                .send_with_forward_proxy(api_key_id, request_kind, |client| {
                    let mut req = client
                        .get(request_url.clone())
                        .header("Authorization", format!("Bearer {}", secret_header));
                    if let Some(timeout) = timeout {
                        req = req.timeout(timeout);
                    }
                    req
                })
                .await
                .map(|(response, _)| response)?,
            None => {
                let mut req = self
                    .client
                    .get(request_url.clone())
                    .header("Authorization", format!("Bearer {}", secret_header));
                if let Some(timeout) = timeout {
                    req = req.timeout(timeout);
                }
                req.send().await.map_err(ProxyError::Http)?
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(ProxyError::Http)?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ProxyError::UsageHttp { status, body });
        }
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProxyError::Other(format!("invalid usage json: {}", e)))?;
        let key_limit = json
            .get("key")
            .and_then(|k| k.get("limit"))
            .and_then(|v| v.as_i64());
        let key_usage = json
            .get("key")
            .and_then(|k| k.get("usage"))
            .and_then(|v| v.as_i64());
        let acc_limit = json
            .get("account")
            .and_then(|a| a.get("plan_limit"))
            .and_then(|v| v.as_i64());
        let acc_usage = json
            .get("account")
            .and_then(|a| a.get("plan_usage"))
            .and_then(|v| v.as_i64());
        let limit = key_limit.or(acc_limit).unwrap_or(0);
        let used = key_usage.or(acc_usage).unwrap_or(0);
        if limit <= 0 && used <= 0 {
            return Err(ProxyError::QuotaDataMissing {
                reason: "missing key/account usage fields".to_owned(),
            });
        }
        let remaining = (limit - used).max(0);
        Ok((limit, remaining))
    }

    async fn fetch_research_usage_for_secret(
        &self,
        secret: &str,
        usage_base: &str,
        timeout: Option<Duration>,
        api_key_id: Option<&str>,
        request_kind: &str,
    ) -> Result<i64, ProxyError> {
        let base = Url::parse(usage_base).map_err(|e| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_string(),
            source: e,
        })?;
        let mut url = base.clone();
        url.set_path("/usage");

        let secret_header = secret.to_string();
        let request_url = url.clone();
        let resp = match api_key_id {
            Some(api_key_id) => self
                .send_with_forward_proxy(api_key_id, request_kind, |client| {
                    let mut req = client
                        .get(request_url.clone())
                        .header("Authorization", format!("Bearer {}", secret_header));
                    if let Some(timeout) = timeout {
                        req = req.timeout(timeout);
                    }
                    req
                })
                .await
                .map(|(response, _)| response)?,
            None => {
                let mut req = self
                    .client
                    .get(request_url.clone())
                    .header("Authorization", format!("Bearer {}", secret_header));
                if let Some(timeout) = timeout {
                    req = req.timeout(timeout);
                }
                req.send().await.map_err(ProxyError::Http)?
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(ProxyError::Http)?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ProxyError::UsageHttp { status, body });
        }

        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProxyError::Other(format!("invalid usage json: {}", e)))?;
        let usage = json
            .get("key")
            .and_then(|k| k.get("research_usage"))
            .and_then(parse_credits_value);
        usage.ok_or_else(|| ProxyError::QuotaDataMissing {
            reason: "missing key.research_usage field".to_owned(),
        })
    }

    async fn fetch_research_usage_for_secret_with_retries(
        &self,
        secret: &str,
        usage_base: &str,
        api_key_id: Option<&str>,
        request_kind: &str,
    ) -> Result<i64, ProxyError> {
        let mut last_error: Option<ProxyError> = None;
        for attempt in 0..USAGE_PROBE_RETRY_ATTEMPTS {
            match self
                .fetch_research_usage_for_secret(
                    secret,
                    usage_base,
                    Some(Duration::from_secs(USAGE_PROBE_TIMEOUT_SECS)),
                    api_key_id,
                    request_kind,
                )
                .await
            {
                Ok(usage) => return Ok(usage),
                Err(err) => last_error = Some(err),
            }

            if attempt + 1 < USAGE_PROBE_RETRY_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(USAGE_PROBE_RETRY_DELAY_MS)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProxyError::Other("research usage probe failed without error".to_owned())
        }))
    }

    /// Aggregate per-token usage logs into token_usage_stats for UI metrics.
    /// Used by background schedulers to keep usage charts up to date.
    pub async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        self.key_store.rollup_token_usage_stats().await
    }

    /// Time-based garbage collection for per-token access logs.
    /// This uses a fixed retention window and never looks at token status,
    /// to avoid impacting auditability.
    pub async fn gc_auth_token_logs(&self) -> Result<i64, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let threshold = now_ts - AUTH_TOKEN_LOG_RETENTION_SECS;
        self.key_store.delete_old_auth_token_logs(threshold).await
    }

    /// Time-based garbage collection for request_logs (online recent logs only).
    /// Retention is defined by local-day boundaries and enforced via environment variables.
    pub async fn gc_request_logs(&self) -> Result<i64, ProxyError> {
        let retention_days = effective_request_logs_retention_days();
        let threshold = request_logs_retention_threshold_utc_ts(retention_days);
        self.key_store.delete_old_request_logs(threshold).await
    }

    /// Job logging helpers
    pub async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .scheduled_job_start(job_type, key_id, attempt)
            .await
    }

    pub async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .scheduled_job_finish(job_id, status, message)
            .await
    }

    pub async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        self.key_store.list_recent_jobs(limit).await
    }

    pub async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64), ProxyError> {
        self.key_store
            .list_recent_jobs_paginated(group, page, per_page)
            .await
    }
}

fn is_transient_sqlite_write_error(err: &ProxyError) -> bool {
    let ProxyError::Database(db_err) = err else {
        return false;
    };
    let sqlx::Error::Database(db_err) = db_err else {
        return false;
    };

    if let Some(code) = db_err.code() {
        match code.as_ref() {
            // SQLite primary and extended codes for lock/busy states.
            "5" | "6" | "261" | "262" | "517" | "518" | "SQLITE_BUSY" | "SQLITE_LOCKED" => {
                return true;
            }
            _ => {}
        }
    }

    let message = db_err.message().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("database is busy")
}

#[derive(Debug)]
struct KeyStore {
    pool: SqlitePool,
    token_binding_cache: RwLock<HashMap<String, TokenBindingCacheEntry>>,
    account_quota_resolution_cache: RwLock<HashMap<String, AccountQuotaResolutionCacheEntry>>,
    #[cfg(test)]
    forced_pending_claim_miss_log_ids: Mutex<HashSet<i64>>,
    // Lightweight failpoint registry used by integration tests to simulate a lost quota
    // subject lease after precheck but before settlement.
    forced_quota_subject_lock_loss_subjects: std::sync::Mutex<HashSet<String>>,
}

impl KeyStore {
    async fn new(database_path: &str) -> Result<Self, ProxyError> {
        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await?;

        let store = Self {
            pool,
            token_binding_cache: RwLock::new(HashMap::new()),
            account_quota_resolution_cache: RwLock::new(HashMap::new()),
            #[cfg(test)]
            forced_pending_claim_miss_log_ids: Mutex::new(HashSet::new()),
            forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(HashSet::new()),
        };
        store.initialize_schema().await?;
        Ok(store)
    }

    async fn initialize_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                group_name TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_api_keys_schema().await?;
        self.ensure_api_key_quarantines_schema().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT NOT NULL,
                auth_token_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                request_body BLOB,
                response_body BLOB,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_request_logs_schema().await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_auth_token_time
               ON request_logs(auth_token_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // API key usage rollups (for statistics that must not depend on request_logs retention).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_usage_buckets (
                api_key_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                total_requests INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                error_count INTEGER NOT NULL,
                quota_exhausted_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (api_key_id, bucket_start, bucket_secs),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_usage_buckets_time
               ON api_key_usage_buckets(bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // Access tokens for /mcp authentication
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_tokens (
                id TEXT PRIMARY KEY,           -- 4-char id code
                secret TEXT NOT NULL,          -- 12-char secret
                enabled INTEGER NOT NULL DEFAULT 1,
                note TEXT,
                group_name TEXT,
                total_requests INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_auth_tokens_schema().await?;

        // Persist research request ownership/key affinity so result polling survives
        // process restarts and multi-instance routing.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS research_requests (
                request_id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                token_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_research_requests_expires_at
               ON research_requests(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        forward_proxy::ensure_forward_proxy_schema(&self.pool).await?;

        // User identity model (separated from admin auth):
        // - users: local user records
        // - oauth_accounts: third-party account bindings (provider + provider_user_id unique)
        // - user_sessions: persisted user sessions for browser auth
        // - user_token_bindings: one user may bind multiple auth tokens
        // - oauth_login_states: one-time OAuth state tokens for CSRF/replay protection
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                username TEXT,
                avatar_template TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_login_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS oauth_accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                provider_user_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                username TEXT,
                name TEXT,
                avatar_template TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                trust_level INTEGER,
                raw_payload TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(provider, provider_user_id),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_oauth_accounts_user ON oauth_accounts(user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_sessions (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked_at INTEGER,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_sessions_user ON user_sessions(user_id, expires_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_token_bindings (
                user_id TEXT NOT NULL,
                token_id TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, token_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.migrate_user_token_bindings_to_multi_binding().await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_token_bindings_user_updated
               ON user_token_bindings(user_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS oauth_login_states (
                state TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                redirect_to TEXT,
                binding_hash TEXT,
                bind_token_id TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                consumed_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_oauth_login_states_expire ON oauth_login_states(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("oauth_login_states", "binding_hash")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_login_states ADD COLUMN binding_hash TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_column_exists("oauth_login_states", "bind_token_id")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_login_states ADD COLUMN bind_token_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        self.ensure_dev_open_admin_token().await?;

        // Ensure per-token usage logs table exists BEFORE running data consistency migration
        // because the migration queries auth_token_logs.
        // Per-token usage logs for detail page (auth_token_logs)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_token_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_id TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                http_status INTEGER,
                mcp_status INTEGER,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                result_status TEXT NOT NULL,
                error_message TEXT,
                counts_business_quota INTEGER NOT NULL DEFAULT 1,
                business_credits INTEGER,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_token_time ON auth_token_logs(token_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // Upgrade: add mcp_status column if missing
        if !self
            .table_column_exists("auth_token_logs", "mcp_status")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN mcp_status INTEGER")
                .execute(&self.pool)
                .await?;
        }

        let mut request_kind_schema_changed = false;
        if !self
            .table_column_exists("auth_token_logs", "request_kind_key")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_key TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .table_column_exists("auth_token_logs", "request_kind_label")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_label TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .table_column_exists("auth_token_logs", "request_kind_detail")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_detail TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        // Upgrade: add counts_business_quota column if missing
        if !self
            .table_column_exists("auth_token_logs", "counts_business_quota")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN counts_business_quota INTEGER NOT NULL DEFAULT 1",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_billable_id
               ON auth_token_logs(counts_business_quota, id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_token_request_kind_time
               ON auth_token_logs(token_id, request_kind_key, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("auth_token_logs", "business_credits")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN business_credits INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "billing_subject")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN billing_subject TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "billing_state")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN billing_state TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_billing_pending
               ON auth_token_logs(billing_state, billing_subject, id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS quota_subject_locks (
                subject TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_quota_subject_locks_expires_at
               ON quota_subject_locks(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_usage_buckets (
                token_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                granularity TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (token_id, bucket_start, granularity),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_usage_lookup ON token_usage_buckets(token_id, granularity, bucket_start)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_token_quota (
                token_id TEXT PRIMARY KEY,
                month_start INTEGER NOT NULL,
                month_count INTEGER NOT NULL,
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_quota_limits (
                user_id TEXT PRIMARY KEY,
                hourly_any_limit INTEGER NOT NULL,
                hourly_limit INTEGER NOT NULL,
                daily_limit INTEGER NOT NULL,
                monthly_limit INTEGER NOT NULL,
                inherits_defaults INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("account_quota_limits", "inherits_defaults")
            .await?
        {
            sqlx::query(
                "ALTER TABLE account_quota_limits ADD COLUMN inherits_defaults INTEGER NOT NULL DEFAULT 1",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                icon TEXT,
                system_key TEXT UNIQUE,
                effect_kind TEXT NOT NULL DEFAULT 'quota_delta',
                hourly_any_delta INTEGER NOT NULL DEFAULT 0,
                hourly_delta INTEGER NOT NULL DEFAULT 0,
                daily_delta INTEGER NOT NULL DEFAULT 0,
                monthly_delta INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_tag_bindings (
                user_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, tag_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (tag_id) REFERENCES user_tags(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_tag_bindings_user_updated
               ON user_tag_bindings(user_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_tag_bindings_tag_user
               ON user_tag_bindings(tag_id, user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_usage_buckets (
                user_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                granularity TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (user_id, bucket_start, granularity),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_account_usage_lookup
               ON account_usage_buckets(user_id, granularity, bucket_start)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_monthly_quota (
                user_id TEXT PRIMARY KEY,
                month_start INTEGER NOT NULL,
                month_count INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_usage_stats (
                token_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                system_failure_count INTEGER NOT NULL,
                external_failure_count INTEGER NOT NULL,
                quota_exhausted_count INTEGER NOT NULL,
                PRIMARY KEY (token_id, bucket_start, bucket_secs),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_usage_stats_token_time
               ON token_usage_stats(token_id, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // Scheduled jobs table for background tasks (e.g., quota/usage sync)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS scheduled_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_type TEXT NOT NULL,
                key_id TEXT,
                status TEXT NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 1,
                message TEXT,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Meta table for lightweight global key/value settings (e.g., migrations, rollup state)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        if request_kind_schema_changed
            || self
                .get_meta_i64(META_KEY_AUTH_TOKEN_LOG_REQUEST_KIND_BACKFILL_V1)
                .await?
                .is_none()
        {
            self.backfill_auth_token_log_request_kinds().await?;
            self.set_meta_i64(META_KEY_AUTH_TOKEN_LOG_REQUEST_KIND_BACKFILL_V1, 1)
                .await?;
        }

        // Backfill API key usage buckets exactly once. This enables safe request_logs retention
        // without changing the meaning of cumulative statistics.
        if self
            .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
            .await?
            .is_none()
        {
            self.migrate_api_key_usage_buckets_v1().await?;
            self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE, 1)
                .await?;
        }

        // After ensuring schemas, run the data consistency migration at most once.
        // Older versions incremented auth_tokens.total_requests during validation; this
        // migration reconciles those counters using auth_token_logs, then marks itself
        // as completed in the meta table so that future startups do not depend on
        // potentially truncated logs.
        if self
            .get_meta_i64(META_KEY_DATA_CONSISTENCY_DONE)
            .await?
            .is_none()
        {
            self.migrate_data_consistency().await?;
            self.set_meta_i64(META_KEY_DATA_CONSISTENCY_DONE, 1).await?;
        }

        // One-time healer: backfill soft-deleted auth_tokens rows for any token_id
        // that only exists in auth_token_logs. This ensures that downstream usage
        // rollups into token_usage_stats (which reference auth_tokens via FOREIGN KEY)
        // will not fail with constraint errors for legacy data.
        if self
            .get_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1)
            .await?
            .is_none()
        {
            self.heal_orphan_auth_tokens_from_logs().await?;
        }

        // Cut over business quota counters from legacy "requests" units to "credits".
        // Historical request counts cannot be converted safely, but clearing them would silently
        // grant fresh quota to every active subject on upgrade. Preserve existing windows and let
        // them age out naturally; new charges written after the cutover are already credits-based.
        if self
            .get_meta_i64(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
            .await?
            .is_none()
        {
            self.set_meta_i64(
                META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1,
                Utc::now().timestamp(),
            )
            .await?;
        }

        if self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1)
            .await?
            .is_none()
        {
            self.backfill_account_quota_v1().await?;
            self.set_meta_i64(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1, 1)
                .await?;
        }
        if self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
            .await?
            .is_none()
        {
            self.backfill_account_quota_inherits_defaults_v1().await?;
            self.set_meta_i64(
                META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1,
                Utc::now().timestamp(),
            )
            .await?;
        }
        if self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1)
            .await?
            .is_none()
        {
            self.set_meta_i64(
                META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1,
                Utc::now().timestamp(),
            )
            .await?;
        }
        if self
            .get_meta_i64(META_KEY_FORCE_USER_RELOGIN_V1)
            .await?
            .is_none()
        {
            self.force_user_relogin_v1().await?;
            self.set_meta_i64(META_KEY_FORCE_USER_RELOGIN_V1, Utc::now().timestamp())
                .await?;
        }
        self.seed_linuxdo_system_tags().await?;
        if self
            .get_meta_i64(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
            .await?
            .is_none()
        {
            self.backfill_linuxdo_system_tag_default_deltas_v1().await?;
            self.set_meta_i64(
                META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1,
                Utc::now().timestamp(),
            )
            .await?;
        }
        self.sync_linuxdo_system_tag_default_deltas_with_env()
            .await?;
        self.backfill_linuxdo_user_tag_bindings().await?;
        self.sync_account_quota_limits_with_defaults().await?;

        Ok(())
    }

    async fn ensure_dev_open_admin_token(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            ) VALUES (?, ?, 0, ?, NULL, 0, ?, NULL, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(DEV_OPEN_ADMIN_TOKEN_ID)
        .bind(DEV_OPEN_ADMIN_TOKEN_SECRET)
        .bind(DEV_OPEN_ADMIN_TOKEN_NOTE)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn user_token_bindings_uses_single_binding_primary_key(
        &self,
    ) -> Result<bool, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT name, pk FROM pragma_table_info('user_token_bindings')",
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(false);
        }

        let mut user_id_pk = 0;
        let mut token_id_pk = 0;
        for (name, pk) in rows {
            if name == "user_id" {
                user_id_pk = pk;
            } else if name == "token_id" {
                token_id_pk = pk;
            }
        }

        Ok(user_id_pk == 1 && token_id_pk == 0)
    }

    async fn migrate_user_token_bindings_to_multi_binding(&self) -> Result<(), ProxyError> {
        if !self
            .user_token_bindings_uses_single_binding_primary_key()
            .await?
        {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            CREATE TABLE user_token_bindings_v2 (
                user_id TEXT NOT NULL,
                token_id TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, token_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_token_bindings_v2 (user_id, token_id, created_at, updated_at)
               SELECT user_id, token_id, created_at, updated_at
               FROM user_token_bindings"#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE user_token_bindings")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE user_token_bindings_v2 RENAME TO user_token_bindings")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn force_user_relogin_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE user_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn migrate_api_key_usage_buckets_v1(&self) -> Result<(), ProxyError> {
        // Rebuild buckets from request_logs to preserve cumulative statistics after retention.
        // This is safe to rerun because we clear and recompute deterministically.
        let now_ts = Utc::now().timestamp();
        let mut read_conn = self.pool.acquire().await?;
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM api_key_usage_buckets")
            .execute(&mut *tx)
            .await?;

        let mut rows = sqlx::query(
            r#"
            SELECT api_key_id, created_at, result_status
            FROM request_logs
            ORDER BY api_key_id ASC, created_at ASC, id ASC
            "#,
        )
        .fetch(&mut *read_conn);

        #[derive(Clone, Copy, Default)]
        struct BucketCounts {
            total_requests: i64,
            success_count: i64,
            error_count: i64,
            quota_exhausted_count: i64,
        }

        async fn flush_bucket(
            tx: &mut Transaction<'_, Sqlite>,
            now_ts: i64,
            key: &str,
            bucket_start: i64,
            counts: BucketCounts,
        ) -> Result<(), ProxyError> {
            if counts.total_requests <= 0 {
                return Ok(());
            }
            sqlx::query(
                r#"
                INSERT INTO api_key_usage_buckets (
                    api_key_id,
                    bucket_start,
                    bucket_secs,
                    total_requests,
                    success_count,
                    error_count,
                    quota_exhausted_count,
                    updated_at
                ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(key)
            .bind(bucket_start)
            .bind(counts.total_requests)
            .bind(counts.success_count)
            .bind(counts.error_count)
            .bind(counts.quota_exhausted_count)
            .bind(now_ts)
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut current_key: Option<String> = None;
        let mut current_bucket_start: i64 = 0;
        let mut counts = BucketCounts::default();

        while let Some(row) = rows.try_next().await? {
            let key_id: String = row.try_get("api_key_id")?;
            let created_at: i64 = row.try_get("created_at")?;
            let status: String = row.try_get("result_status")?;

            let bucket_start = local_day_bucket_start_utc_ts(created_at);

            let needs_flush = match current_key.as_deref() {
                None => false,
                Some(k) if k != key_id.as_str() => true,
                Some(_) if current_bucket_start != bucket_start => true,
                _ => false,
            };

            if needs_flush {
                let key = current_key.as_deref().expect("flush key present");
                flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;

                counts = BucketCounts::default();
            }

            current_key = Some(key_id);
            current_bucket_start = bucket_start;

            counts.total_requests += 1;
            match status.as_str() {
                OUTCOME_SUCCESS => counts.success_count += 1,
                OUTCOME_ERROR => counts.error_count += 1,
                OUTCOME_QUOTA_EXHAUSTED => counts.quota_exhausted_count += 1,
                _ => {}
            }
        }

        if let Some(key) = current_key.as_deref() {
            flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Reconcile derived fields to ensure cross-table consistency.
    /// This migration is idempotent and safe to run on every startup.
    async fn migrate_data_consistency(&self) -> Result<(), ProxyError> {
        // 1) Access tokens: recompute total_requests and last_used_at from auth_token_logs
        //    Older versions incremented total_requests during validation, which
        //    inflated counters. The canonical source of truth is auth_token_logs.
        sqlx::query(
            r#"
            UPDATE auth_tokens
            SET total_requests = COALESCE((
                    SELECT COUNT(*) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                ), 0),
                last_used_at = (
                    SELECT MAX(created_at) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 2) API keys: refresh last_used_at from request_logs to avoid stale values
        //    (This is a best-effort consistency update; it's safe and general.)
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = COALESCE((
                SELECT MAX(created_at) FROM request_logs r WHERE r.api_key_id = api_keys.id
            ), last_used_at)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Ensure that every token_id referenced in auth_token_logs has a corresponding
    /// auth_tokens row. Missing rows are backfilled as disabled, soft-deleted tokens
    /// so that downstream usage aggregation into token_usage_stats (with FOREIGN KEYs)
    /// does not fail for legacy data.
    async fn heal_orphan_auth_tokens_from_logs(&self) -> Result<(), ProxyError> {
        // Skip if auth_token_logs table does not exist (very old databases).
        let has_logs_table = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'auth_token_logs' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        if has_logs_table.is_none() {
            self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, 0).await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            )
            SELECT
                l.token_id,
                'restored-from-logs',
                0,
                '[auto-restored from logs]',
                NULL,
                COUNT(*) AS total_requests,
                MIN(l.created_at) AS created_at,
                MAX(l.created_at) AS last_used_at,
                ?
            FROM auth_token_logs l
            LEFT JOIN auth_tokens t ON t.id = l.token_id
            WHERE t.id IS NULL
            GROUP BY l.token_id
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Record completion so this healer is only ever run once per database.
        self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, now)
            .await?;

        Ok(())
    }

    async fn backfill_account_quota_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let hourly_any_limit = effective_token_hourly_request_limit();
        let hourly_limit = effective_token_hourly_limit();
        let daily_limit = effective_token_daily_limit();
        let monthly_limit = effective_token_monthly_limit();

        // Ensure every bound account has a default limits row.
        sqlx::query(
            r#"
            INSERT INTO account_quota_limits (
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
                created_at,
                updated_at
            )
            SELECT
                b.user_id,
                ?,
                ?,
                ?,
                ?,
                ?,
                ?
            FROM user_token_bindings b
            GROUP BY b.user_id
            ON CONFLICT(user_id) DO NOTHING
            "#,
        )
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Copy existing token rolling buckets to account scope.
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            SELECT
                b.user_id,
                u.bucket_start,
                u.granularity,
                SUM(u.count) AS count
            FROM user_token_bindings b
            JOIN token_usage_buckets u ON u.token_id = b.token_id
            GROUP BY b.user_id, u.bucket_start, u.granularity
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = excluded.count
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Copy monthly counters to account scope. If multiple tokens map to one account,
        // keep the latest month_start and aggregate counts in that month.
        sqlx::query(
            r#"
            WITH mapped AS (
                SELECT b.user_id AS user_id, q.month_start AS month_start, q.month_count AS month_count
                FROM user_token_bindings b
                JOIN auth_token_quota q ON q.token_id = b.token_id
            ),
            latest AS (
                SELECT user_id, MAX(month_start) AS latest_month_start
                FROM mapped
                GROUP BY user_id
            )
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            SELECT
                l.user_id,
                l.latest_month_start,
                COALESCE(SUM(CASE WHEN m.month_start = l.latest_month_start THEN m.month_count ELSE 0 END), 0)
            FROM latest l
            LEFT JOIN mapped m ON m.user_id = l.user_id
            GROUP BY l.user_id, l.latest_month_start
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn increment_usage_bucket_by(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(token_id, bucket_start, granularity)
            DO UPDATE SET count = token_usage_buckets.count + excluded.count
            "#,
        )
        .bind(token_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_usage_bucket(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_usage_bucket_by(token_id, bucket_start, granularity, 1)
            .await
    }

    async fn increment_account_usage_bucket_by(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = account_usage_buckets.count + excluded.count
            "#,
        )
        .bind(user_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_account_usage_bucket(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_account_usage_bucket_by(user_id, bucket_start, granularity, 1)
            .await
    }

    async fn sum_usage_buckets(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    async fn sum_account_usage_buckets(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    async fn sum_account_usage_buckets_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    async fn sum_usage_buckets_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, SUM(count) as total FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, total) in rows {
            map.insert(token_id, total);
        }
        Ok(map)
    }

    async fn earliest_usage_bucket_since_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, MIN(bucket_start) as earliest FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, bucket_start) in rows {
            map.insert(token_id, bucket_start);
        }
        Ok(map)
    }

    async fn earliest_account_usage_bucket_since_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, MIN(bucket_start) as earliest FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, bucket_start) in rows {
            map.insert(user_id, bucket_start);
        }
        Ok(map)
    }

    async fn fetch_monthly_counts(
        &self,
        token_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT token_id, month_start, month_count FROM auth_token_quota WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (token_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(token_id.clone(), 0);
                stale_ids.push(token_id);
            } else {
                map.insert(token_id, stored_count);
            }
        }

        for token_id in stale_ids {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(&token_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    async fn fetch_monthly_count(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(token_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    async fn fetch_account_monthly_count(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    async fn fetch_account_monthly_counts(
        &self,
        user_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT user_id, month_start, month_count FROM account_monthly_quota WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (user_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(user_id.clone(), 0);
                stale_ids.push(user_id);
            } else {
                map.insert(user_id, stored_count);
            }
        }

        for user_id in stale_ids {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(&user_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    async fn delete_old_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM token_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_old_account_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM account_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete per-token usage logs older than the given threshold.
    /// This is strictly time-based and deliberately independent of token status,
    /// so that audit trails are not coupled to enable/disable/delete operations.
    async fn delete_old_auth_token_logs(&self, threshold: i64) -> Result<i64, ProxyError> {
        let result = sqlx::query(
            r#"
            DELETE FROM auth_token_logs
            WHERE created_at < ?
            "#,
        )
        .bind(threshold)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    async fn delete_old_request_logs(&self, threshold: i64) -> Result<i64, ProxyError> {
        // Batched deletes reduce long-running write locks on large tables.
        const BATCH_SIZE: i64 = 5_000;
        let mut total_deleted = 0_i64;
        loop {
            let result = sqlx::query(
                r#"
                DELETE FROM request_logs
                WHERE id IN (
                    SELECT id
                    FROM request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            )
            .bind(threshold)
            .bind(BATCH_SIZE)
            .execute(&self.pool)
            .await?;
            let deleted = result.rows_affected() as i64;
            total_deleted += deleted;
            if deleted == 0 {
                break;
            }
        }
        Ok(total_deleted)
    }

    /// Aggregate per-token usage logs into hourly buckets in token_usage_stats.
    /// Returns (rows_affected, new_last_rollup_ts). When there are no new logs,
    /// rows_affected is 0 and new_last_rollup_ts is None.
    async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        async fn read_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
        ) -> Result<Option<i64>, ProxyError> {
            let value =
                sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
                    .bind(key)
                    .fetch_optional(&mut **tx)
                    .await?;
            Ok(value.and_then(|v| v.parse::<i64>().ok()))
        }

        async fn write_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
            value: i64,
        ) -> Result<(), ProxyError> {
            sqlx::query(
                r#"
                INSERT INTO meta (key, value)
                VALUES (?, ?)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(key)
            .bind(value.to_string())
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut tx = self.pool.begin().await?;

        // v2 cursor: strictly monotonic auth_token_logs.id to guarantee idempotent rollup.
        // Backward compatibility: on first v2 run, legacy timestamp is used only to filter
        // the migration batch, then the cursor permanently switches to id-based mode.
        let v2_cursor = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2).await?;
        let (last_log_id, migration_legacy_ts) = if let Some(id) = v2_cursor {
            (id, None)
        } else {
            (
                0,
                read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?,
            )
        };

        let (max_log_id, max_created_at): (Option<i64>, Option<i64>) =
            if let Some(legacy_ts) = migration_legacy_ts {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(CASE WHEN created_at >= ? THEN created_at END) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                    "#,
                )
                .bind(legacy_ts)
                .fetch_one(&mut *tx)
                .await?
            } else {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(created_at) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                      AND id > ?
                    "#,
                )
                .bind(last_log_id)
                .fetch_one(&mut *tx)
                .await?
            };

        let Some(max_log_id) = max_log_id else {
            if migration_legacy_ts.is_some() {
                // No billable logs yet: initialize v2 cursor to complete migration.
                write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, 0).await?;
            }
            tx.commit().await?;
            return Ok((0, None));
        };

        let bucket_secs = TOKEN_USAGE_STATS_BUCKET_SECS;

        let result = if let Some(legacy_ts) = migration_legacy_ts {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND created_at >= ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(legacy_ts)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND id > ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(last_log_id)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        };

        let affected = result.rows_affected() as i64;
        let mut new_last_rollup_ts = max_created_at;

        write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, max_log_id).await?;
        if let Some(ts) = max_created_at {
            // Keep legacy timestamp cursor monotonic for observability and downgrade compatibility.
            // This prevents accidental timestamp regression when newer log ids carry older created_at.
            let legacy_ts = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?;
            let clamped_ts = legacy_ts.map_or(ts, |old| old.max(ts));
            write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS, clamped_ts).await?;
            new_last_rollup_ts = Some(clamped_ts);
        }

        tx.commit().await?;
        Ok((affected, new_last_rollup_ts))
    }

    async fn increment_monthly_quota_by(
        &self,
        token_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_monthly_count(token_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                    ELSE auth_token_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                    ELSE auth_token_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(token_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;

        Ok(month_count)
    }

    async fn increment_monthly_quota(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_monthly_quota_by(token_id, current_month_start, 1)
            .await
    }

    async fn increment_account_monthly_quota_by(
        &self,
        user_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_account_monthly_count(user_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                    ELSE account_monthly_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                    ELSE account_monthly_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(user_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;
        Ok(month_count)
    }

    async fn increment_account_monthly_quota(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_account_monthly_quota_by(user_id, current_month_start, 1)
            .await
    }

    async fn upgrade_auth_tokens_schema(&self) -> Result<(), ProxyError> {
        // Future-proof placeholder for migrations
        // Ensure required columns exist if table is from older version
        // enabled
        if !self.auth_tokens_column_exists("enabled").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1")
                .execute(&self.pool)
                .await?;
        }

        if !self.auth_tokens_column_exists("note").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN note TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("total_requests").await? {
            sqlx::query(
                "ALTER TABLE auth_tokens ADD COLUMN total_requests INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self.auth_tokens_column_exists("created_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("last_used_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN last_used_at INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("group_name").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN group_name TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("deleted_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN deleted_at INTEGER")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn auth_tokens_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('auth_tokens') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn table_column_exists(&self, table: &str, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
        )
        .bind(table)
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn upgrade_api_keys_schema(&self) -> Result<(), ProxyError> {
        // Track whether legacy column existed to gate one-time migration logic
        let had_disabled_at = self.api_keys_column_exists("disabled_at").await?;
        if had_disabled_at {
            sqlx::query("ALTER TABLE api_keys RENAME COLUMN disabled_at TO status_changed_at")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("status").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN status TEXT NOT NULL DEFAULT 'active'")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("status_changed_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN status_changed_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("group_name").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN group_name TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Add deleted_at for soft delete marker (timestamp)
        if !self.api_keys_column_exists("deleted_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN deleted_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        // Quota tracking columns for Tavily usage
        if !self.api_keys_column_exists("quota_limit").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_limit INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.api_keys_column_exists("quota_remaining").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_remaining INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.api_keys_column_exists("quota_synced_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_synced_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        // Migrate legacy status='deleted' into deleted_at and normalize status
        let legacy_deleted = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM api_keys WHERE status = 'deleted' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        if legacy_deleted.is_some() {
            let now = Utc::now().timestamp();
            sqlx::query(
                r#"UPDATE api_keys
                   SET deleted_at = COALESCE(status_changed_at, ?)
                   WHERE status = 'deleted' AND (deleted_at IS NULL OR deleted_at = 0)"#,
            )
            .bind(now)
            .execute(&self.pool)
            .await?;

            sqlx::query("UPDATE api_keys SET status = 'active' WHERE status = 'deleted'")
                .execute(&self.pool)
                .await?;
        }

        // Only when migrating from legacy 'disabled_at' do we mark keys as exhausted.
        if had_disabled_at {
            sqlx::query(
                r#"
                UPDATE api_keys
                SET status = ?
                WHERE status_changed_at IS NOT NULL
                  AND status_changed_at != 0
                  AND status <> ?
                "#,
            )
            .bind(STATUS_EXHAUSTED)
            .bind(STATUS_EXHAUSTED)
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?
            WHERE status IS NULL
               OR status = ''
            "#,
        )
        .bind(STATUS_ACTIVE)
        .execute(&self.pool)
        .await?;

        self.ensure_api_key_ids().await?;
        self.ensure_api_keys_primary_key().await?;

        Ok(())
    }

    async fn ensure_api_key_quarantines_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_quarantines (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                source TEXT NOT NULL,
                reason_code TEXT NOT NULL,
                reason_summary TEXT NOT NULL,
                reason_detail TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                cleared_at INTEGER,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_quarantines_active ON api_key_quarantines(key_id) WHERE cleared_at IS NULL",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_api_key_quarantines_key_created ON api_key_quarantines(key_id, created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn ensure_api_key_ids(&self) -> Result<(), ProxyError> {
        if !self.api_keys_column_exists("id").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN id TEXT")
                .execute(&self.pool)
                .await?;
        }

        let mut tx = self.pool.begin().await?;
        let keys = sqlx::query_scalar::<_, String>(
            "SELECT api_key FROM api_keys WHERE id IS NULL OR id = ''",
        )
        .fetch_all(&mut *tx)
        .await?;

        for api_key in keys {
            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query("UPDATE api_keys SET id = ? WHERE api_key = ?")
                .bind(&id)
                .bind(&api_key)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn ensure_api_keys_primary_key(&self) -> Result<(), ProxyError> {
        if self.api_keys_primary_key_is_id().await? {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        // Ensure the temp table schema is up-to-date even if a previous migration attempt left it behind.
        sqlx::query("DROP TABLE IF EXISTS api_keys_new")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE api_keys_new (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                group_name TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO api_keys_new (
                id,
                api_key,
                group_name,
                status,
                status_changed_at,
                last_used_at,
                quota_limit,
                quota_remaining,
                quota_synced_at,
                deleted_at
            )
            SELECT
                id,
                api_key,
                group_name,
                status,
                status_changed_at,
                last_used_at,
                quota_limit,
                quota_remaining,
                quota_synced_at,
                deleted_at
            FROM api_keys
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("DROP TABLE api_keys").execute(&mut *tx).await?;
        sqlx::query("ALTER TABLE api_keys_new RENAME TO api_keys")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn api_keys_primary_key_is_id(&self) -> Result<bool, ProxyError> {
        let rows = sqlx::query("SELECT name, pk FROM pragma_table_info('api_keys')")
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let name: String = row.try_get("name")?;
            let pk: i64 = row.try_get("pk")?;
            if name == "id" {
                return Ok(pk > 0);
            }
        }

        Ok(false)
    }

    async fn generate_unique_key_id(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<String, ProxyError> {
        loop {
            let candidate = nanoid!(4);
            let exists = sqlx::query_scalar::<_, Option<String>>(
                "SELECT id FROM api_keys WHERE id = ? LIMIT 1",
            )
            .bind(&candidate)
            .fetch_optional(&mut **tx)
            .await?;

            if exists.is_none() {
                return Ok(candidate);
            }
        }
    }

    async fn api_keys_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('api_keys') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;

        Ok(exists.is_some())
    }

    async fn upgrade_request_logs_schema(&self) -> Result<(), ProxyError> {
        if !self.request_logs_column_exists("result_status").await? {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN result_status TEXT NOT NULL DEFAULT 'unknown'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .request_logs_column_exists("tavily_status_code")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN tavily_status_code INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("forwarded_headers").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN forwarded_headers TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("dropped_headers").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN dropped_headers TEXT")
                .execute(&self.pool)
                .await?;
        }

        self.ensure_request_logs_key_ids().await?;

        Ok(())
    }

    async fn ensure_request_logs_key_ids(&self) -> Result<(), ProxyError> {
        if !self.request_logs_column_exists("api_key_id").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN api_key_id TEXT")
                .execute(&self.pool)
                .await?;

            sqlx::query(
                r#"
                UPDATE request_logs
                SET api_key_id = (
                    SELECT id FROM api_keys WHERE api_keys.api_key = request_logs.api_key
                )
                "#,
            )
            .execute(&self.pool)
            .await?;
        }

        if self.request_logs_column_exists("api_key").await? {
            let mut tx = self.pool.begin().await?;

            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS request_logs_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    api_key_id TEXT NOT NULL,
                    auth_token_id TEXT,
                    method TEXT NOT NULL,
                    path TEXT NOT NULL,
                    query TEXT,
                    status_code INTEGER,
                    tavily_status_code INTEGER,
                    error_message TEXT,
                    result_status TEXT NOT NULL DEFAULT 'unknown',
                    request_body BLOB,
                    response_body BLOB,
                    forwarded_headers TEXT,
                    dropped_headers TEXT,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
                )
                "#,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO request_logs_new (
                    id,
                    api_key_id,
                    auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                )
                SELECT
                    id,
                    api_key_id,
                    NULL as auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                "#,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query("DROP TABLE request_logs")
                .execute(&mut *tx)
                .await?;
            sqlx::query("ALTER TABLE request_logs_new RENAME TO request_logs")
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
        }

        if !self.request_logs_column_exists("request_body").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN request_body BLOB")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("auth_token_id").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN auth_token_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    async fn request_logs_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('request_logs') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;

        Ok(exists.is_some())
    }

    pub async fn fetch_key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        // `api_key_usage_buckets.bucket_start` is aligned to *server-local midnight* (stored as UTC ts).
        // Callers might pass `since` aligned to UTC midnight (e.g. from browser). Normalize so daily
        // bucket queries remain correct under non-UTC server timezones.
        let since_bucket_start = local_day_bucket_start_utc_ts(since);

        let totals_row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(total_requests), 0) AS total_requests,
              COALESCE(SUM(success_count), 0) AS success_count,
              COALESCE(SUM(error_count), 0) AS error_count,
              COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE api_key_id = ? AND bucket_secs = 86400 AND bucket_start >= ?
            "#,
        )
        .bind(key_id)
        .bind(since_bucket_start)
        .fetch_one(&self.pool)
        .await?;

        // Active/exhausted counts in this scope are not meaningful per single key; expose 1/0 for convenience
        // We will compute based on current key status
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        let key_last_used_at: Option<i64> =
            sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;
        let last_activity = key_last_used_at
            .and_then(normalize_timestamp)
            .filter(|ts| *ts >= since_bucket_start);

        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_quarantines
            WHERE key_id = ? AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        let (active_keys, exhausted_keys, quarantined_keys) = if quarantined {
            (0, 0, 1)
        } else {
            match status.as_deref() {
                Some(STATUS_EXHAUSTED) => (0, 1, 0),
                _ => (1, 0, 0),
            }
        };

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys,
            exhausted_keys,
            quarantined_keys,
            last_activity,
            total_quota_limit: 0,
            total_quota_remaining: 0,
        })
    }

    pub async fn fetch_key_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = if let Some(since_ts) = since {
            sqlx::query_as::<_, (
                i64,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                Option<String>,
                String,
                Vec<u8>,
                Vec<u8>,
                i64,
                String,
                String,
            )>(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ? AND created_at >= ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(since_ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (
                i64,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                Option<String>,
                String,
                Vec<u8>,
                Vec<u8>,
                i64,
                String,
                String,
            )>(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    key_id,
                    auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    created_at,
                    forwarded_headers,
                    dropped_headers,
                )| RequestLogRecord {
                    id,
                    key_id,
                    auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    created_at,
                    forwarded_headers: forwarded_headers
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    dropped_headers: dropped_headers
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                },
            )
            .collect())
    }

    async fn sync_keys(&self, keys: &[String]) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;

        let now = Utc::now().timestamp();

        for key in keys {
            // If key exists, undelete by clearing deleted_at
            if let Some((id, deleted_at)) = sqlx::query_as::<_, (String, Option<i64>)>(
                "SELECT id, deleted_at FROM api_keys WHERE api_key = ? LIMIT 1",
            )
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            {
                if deleted_at.is_some() {
                    sqlx::query("UPDATE api_keys SET deleted_at = NULL WHERE id = ?")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (id, api_key, status, status_changed_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(key)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        // Soft delete any keys not present in the provided set
        if keys.is_empty() {
            sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE deleted_at IS NULL")
                .bind(now)
                .execute(&mut *tx)
                .await?;
        } else {
            let mut builder = QueryBuilder::new("UPDATE api_keys SET deleted_at = ");
            builder.push_bind(now);
            builder.push(" WHERE deleted_at IS NULL AND api_key NOT IN (");
            {
                let mut separated = builder.separated(", ");
                for key in keys {
                    separated.push_bind(key);
                }
            }
            builder.push(")");
            builder.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn acquire_key(&self) -> Result<ApiKeyLease, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY last_used_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        Err(ProxyError::NoAvailableKeys)
    }

    async fn try_acquire_specific_key(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyLease>, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE id = ? AND status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(Some(ApiKeyLease {
                id,
                secret: api_key,
            }));
        }

        Ok(None)
    }

    async fn save_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO research_requests (
                request_id,
                key_id,
                token_id,
                expires_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(request_id) DO UPDATE SET
                key_id = excluded.key_id,
                token_id = excluded.token_id,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(request_id)
        .bind(key_id)
        .bind(token_id)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Opportunistic cleanup to keep this small over time.
        sqlx::query("DELETE FROM research_requests WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn get_research_request_affinity(
        &self,
        request_id: &str,
        now: i64,
    ) -> Result<Option<(String, String)>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT key_id, token_id
            FROM research_requests
            WHERE request_id = ? AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(request_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        if row.is_none() {
            sqlx::query(
                r#"
                DELETE FROM research_requests
                WHERE request_id = ? AND expires_at <= ?
                "#,
            )
            .bind(request_id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(row)
    }

    // ----- Access token helpers -----

    fn compose_full_token(id: &str, secret: &str) -> String {
        format!("th-{}-{}", id, secret)
    }

    async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        // Expect format th-<id>-<secret>
        let Some(rest) = token.strip_prefix("th-") else {
            return Ok(false);
        };
        let parts: Vec<&str> = rest.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Ok(false);
        }
        let id = parts[0];
        let secret = parts[1];
        // Keep short, human-friendly id; strengthen total entropy by lengthening secret.
        // Backward-compatible: accept legacy 12-char secrets and new longer secrets.
        const LEGACY_SECRET_LEN: usize = 12;
        const NEW_SECRET_LEN: usize = 24; // chosen to significantly raise entropy
        let secret_len_ok = secret.len() == LEGACY_SECRET_LEN || secret.len() == NEW_SECRET_LEN;
        if id.len() != 4 || !secret_len_ok {
            return Ok(false);
        }

        // Validation should be a pure check. Do NOT mutate usage counters here,
        // otherwise the token's total_requests will be double-counted (once here,
        // and once when we actually record the attempt). Only return whether the
        // token exists and is enabled.
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COUNT(1) as cnt, enabled FROM auth_tokens WHERE id = ? AND secret = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .bind(secret)
        .fetch_optional(&self.pool)
        .await?;

        Ok(matches!(row, Some((cnt, enabled)) if cnt > 0 && enabled == 1))
    }

    async fn create_access_token(&self, note: Option<&str>) -> Result<AuthTokenSecret, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        loop {
            let id = random_string(ALPHABET, 4);
            // Increase secret length to strengthen token entropy while keeping id short.
            let secret = random_string(ALPHABET, 24);
            let res = sqlx::query(
                r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                   VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)"#,
            )
            .bind(&id)
            .bind(&secret)
            .bind(note.unwrap_or(""))
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    let token_str = Self::compose_full_token(&id, &secret);
                    return Ok(AuthTokenSecret {
                        id,
                        token: token_str,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    // Retry on rare id collision
                    continue;
                }
                Err(e) => return Err(ProxyError::Database(e)),
            }
        }
    }

    /// Batch-create access tokens with required group name. Optional note applied to each row.
    async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut tx = self.pool.begin().await?;
        let mut out: Vec<AuthTokenSecret> = Vec::with_capacity(count);
        for _ in 0..count {
            loop {
                let id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);
                let res = sqlx::query(
                    r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES (?, ?, 1, ?, ?, 0, ?, NULL, NULL)"#,
                )
                .bind(&id)
                .bind(&secret)
                .bind(note.unwrap_or(""))
                .bind(group)
                .bind(Utc::now().timestamp())
                .execute(&mut *tx)
                .await;

                match res {
                    Ok(_) => {
                        let token = Self::compose_full_token(&id, &secret);
                        out.push(AuthTokenSecret { id, token });
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        continue;
                    }
                    Err(e) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(e));
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(out)
    }
    // Generate random string of given length from provided alphabet
    // Alphabet is a byte slice of ASCII alphanumerics
    // Using ThreadRng for simplicity

    async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    /// Paginated list of access tokens ordered by created_at desc. Returns (items, total)
    async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;

        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM auth_tokens WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC
               LIMIT ? OFFSET ?"#,
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect();
        Ok((items, total))
    }

    async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE auth_tokens SET enabled = 0, deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_access_token_enabled(&self, id: &str, enabled: bool) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET enabled = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(if enabled { 1 } else { 0 })
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_access_token_note(&self, id: &str, note: &str) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET note = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(note)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT secret FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &secret),
        }))
    }

    /// Update the secret for an existing token id and return the new full token string.
    async fn rotate_access_token_secret(&self, id: &str) -> Result<AuthTokenSecret, ProxyError> {
        // Ensure token exists first to provide a clearer error on missing id
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        }

        // Generate a new secret with the current strong length
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let new_secret = random_string(ALPHABET, 24);

        sqlx::query("UPDATE auth_tokens SET secret = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(&new_secret)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &new_secret),
        })
    }

    async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT t.id, t.enabled, t.note, t.group_name, t.total_requests, t.created_at, t.last_used_at
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND t.deleted_at IS NULL
               ORDER BY b.updated_at DESC, b.created_at DESC, t.id DESC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used_at)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    async fn is_user_token_bound(&self, user_id: &str, token_id: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT 1
               FROM user_token_bindings
               WHERE user_id = ? AND token_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn list_user_bindings_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, String>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, user_id FROM user_token_bindings WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");
        let rows = builder
            .build_query_as::<(String, String)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, user_id) in rows {
            map.insert(token_id, user_id);
        }
        Ok(map)
    }

    async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT t.secret
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND b.token_id = ? AND t.deleted_at IS NULL AND t.enabled = 1
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    #[allow(dead_code)]
    async fn find_user_id_by_token(&self, token_id: &str) -> Result<Option<String>, ProxyError> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.token_binding_cache.read().await;
            cache.get(token_id).cloned()
        } && cached.expires_at > now
        {
            return Ok(cached.user_id);
        }

        self.find_user_id_by_token_fresh(token_id).await
    }

    async fn find_user_id_by_token_fresh(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let user_id = row.map(|(id,)| id);
        self.cache_token_binding(token_id, user_id.as_deref()).await;
        Ok(user_id)
    }

    async fn cache_token_binding(&self, token_id: &str, user_id: Option<&str>) {
        let mut cache = self.token_binding_cache.write().await;
        cache.insert(
            token_id.to_string(),
            TokenBindingCacheEntry {
                user_id: user_id.map(str::to_string),
                expires_at: Instant::now() + Duration::from_secs(TOKEN_BINDING_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - TOKEN_BINDING_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    async fn cached_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Option<AccountQuotaResolution> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.account_quota_resolution_cache.read().await;
            cache.get(user_id).cloned()
        } && cached.expires_at > now
        {
            return Some(cached.resolution);
        }
        None
    }

    async fn cache_account_quota_resolution(
        &self,
        user_id: &str,
        resolution: &AccountQuotaResolution,
    ) {
        let mut cache = self.account_quota_resolution_cache.write().await;
        cache.insert(
            user_id.to_string(),
            AccountQuotaResolutionCacheEntry {
                resolution: resolution.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(ACCOUNT_QUOTA_RESOLUTION_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    async fn invalidate_account_quota_resolution(&self, user_id: &str) {
        self.account_quota_resolution_cache
            .write()
            .await
            .remove(user_id);
    }

    async fn invalidate_account_quota_resolutions<I, S>(&self, user_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cache = self.account_quota_resolution_cache.write().await;
        for user_id in user_ids {
            cache.remove(user_id.as_ref());
        }
    }

    async fn invalidate_all_account_quota_resolutions(&self) {
        self.account_quota_resolution_cache.write().await.clear();
    }

    async fn list_user_ids_for_tag(&self, tag_id: &str) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT user_id FROM user_tag_bindings WHERE tag_id = ?",
        )
        .bind(tag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let search = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tag_id = tag_id.map(str::trim).filter(|value| !value.is_empty());

        let total = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )"#,
                )
                .bind(tag_id)
                .fetch_one(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
                    .fetch_one(&self.pool)
                    .await?
            }
        };

        let rows = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let items = rows
            .into_iter()
            .map(
                |(user_id, display_name, username, active, last_login_at, token_count)| {
                    AdminUserIdentity {
                        user_id,
                        display_name,
                        username,
                        active: active == 1,
                        last_login_at,
                        token_count,
                    }
                },
            )
            .collect();
        Ok((items, total))
    }

    async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            ),
        >(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id = ?
               GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(user_id, display_name, username, active, last_login_at, token_count)| {
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                }
            },
        ))
    }

    async fn get_admin_user_identities(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at");

        let rows = builder
            .build_query_as::<(
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;

        let mut items = HashMap::with_capacity(rows.len());
        for (user_id, display_name, username, active, last_login_at, token_count) in rows {
            items.insert(
                user_id.clone(),
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                },
            );
        }
        Ok(items)
    }

    async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Ok(false);
        }

        let defaults = AccountQuotaLimits::legacy_defaults();
        let current = self.ensure_account_quota_limits(user_id).await?;
        let requested = AccountQuotaLimits {
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults: false,
        };
        let inherits_defaults = if current.inherits_defaults && requested.same_limits_as(&defaults)
        {
            1
        } else {
            0
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    hourly_any_limit = excluded.hourly_any_limit,
                    hourly_limit = excluded.hourly_limit,
                    daily_limit = excluded.daily_limit,
                    monthly_limit = excluded.monthly_limit,
                    inherits_defaults = excluded.inherits_defaults,
                    updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(inherits_defaults)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    async fn backfill_account_quota_inherits_defaults_v1(&self) -> Result<(), ProxyError> {
        let defaults = AccountQuotaLimits::legacy_defaults();
        // Legacy rows do not record whether they were following defaults or manually customized.
        // Only rows that already match the current env tuple are safe to keep on the default-track;
        // every other tuple is conservatively treated as a custom baseline so upgrades never clobber
        // admin-set quotas.
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET inherits_defaults = CASE
                   WHEN hourly_any_limit = ?
                    AND hourly_limit = ?
                    AND daily_limit = ?
                    AND monthly_limit = ?
                   THEN 1
                   ELSE 0
               END"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn sync_account_quota_limits_with_defaults(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let defaults = AccountQuotaLimits::legacy_defaults();
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = ?,
                   hourly_limit = ?,
                   daily_limit = ?,
                   monthly_limit = ?,
                   updated_at = ?
               WHERE inherits_defaults = 1"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_all_account_quota_resolutions().await;
        Ok(())
    }

    async fn account_quota_zero_base_cutover_at(&self) -> Result<i64, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1)
            .await?
            .unwrap_or(i64::MAX))
    }

    async fn default_account_quota_limits_for_user(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        let user_created_at =
            sqlx::query_scalar::<_, i64>("SELECT created_at FROM users WHERE id = ? LIMIT 1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        Ok(default_account_quota_limits_for_created_at(
            user_created_at,
            cutover_at,
        ))
    }

    async fn default_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        let mut builder = QueryBuilder::new("SELECT id, created_at FROM users WHERE id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(user_id, created_at)| {
                (
                    user_id,
                    default_account_quota_limits_for_created_at(created_at, cutover_at),
                )
            })
            .collect())
    }

    async fn ensure_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        if let Some(existing) = self.fetch_account_quota_limits(user_id).await? {
            return Ok(existing);
        }

        let now = Utc::now().timestamp();
        let defaults = self.default_account_quota_limits_for_user(user_id).await?;
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO NOTHING"#,
        )
        .bind(user_id)
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(if defaults.inherits_defaults { 1 } else { 0 })
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.fetch_account_quota_limits(user_id)
            .await?
            .ok_or_else(|| {
                ProxyError::Other(format!(
                    "account quota limits missing after ensure for user {user_id}"
                ))
            })
    }

    async fn ensure_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<(), ProxyError> {
        if user_ids.is_empty() {
            return Ok(());
        }

        let existing = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let missing_user_ids: Vec<String> = user_ids
            .iter()
            .filter(|user_id| !existing.contains_key(*user_id))
            .cloned()
            .collect();
        if missing_user_ids.is_empty() {
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let defaults_by_user = self
            .default_account_quota_limits_for_users(&missing_user_ids)
            .await?;

        let mut builder = QueryBuilder::new(
            "INSERT INTO account_quota_limits (user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults, created_at, updated_at) ",
        );
        builder.push_values(&missing_user_ids, |mut b, user_id| {
            let defaults = defaults_by_user
                .get(user_id)
                .cloned()
                .unwrap_or_else(AccountQuotaLimits::zero_base);
            b.push_bind(user_id)
                .push_bind(defaults.hourly_any_limit)
                .push_bind(defaults.hourly_limit)
                .push_bind(defaults.daily_limit)
                .push_bind(defaults.monthly_limit)
                .push_bind(if defaults.inherits_defaults { 1 } else { 0 })
                .push_bind(now)
                .push_bind(now);
        });
        builder.push(" ON CONFLICT(user_id) DO NOTHING");
        builder.build().execute(&self.pool).await?;
        Ok(())
    }

    async fn fetch_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountQuotaLimits>, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit,
                      COALESCE(inherits_defaults, 1)
               FROM account_quota_limits
               WHERE user_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults)| {
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                )
            },
        ))
    }

    async fn fetch_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, COALESCE(inherits_defaults, 1) FROM account_quota_limits WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (
            user_id,
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults,
        ) in rows
        {
            map.insert(
                user_id,
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                ),
            );
        }
        Ok(map)
    }

    async fn seed_linuxdo_system_tags(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        for level in 0..=4 {
            let system_key = linuxdo_system_key_for_level(level);
            let display_name = format!("L{level}");
            sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(system_key) DO UPDATE SET
                        name = excluded.name,
                        display_name = excluded.display_name,
                        icon = excluded.icon,
                        updated_at = excluded.updated_at"#,
            )
            .bind(&system_key)
            .bind(&system_key)
            .bind(display_name)
            .bind(USER_TAG_ICON_LINUXDO)
            .bind(&system_key)
            .bind(USER_TAG_EFFECT_QUOTA_DELTA)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn infer_linuxdo_system_tag_default_deltas_from_rows(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.len() != 5 {
            return Ok(None);
        }
        let mut expected: Option<(i64, i64, i64, i64)> = None;
        for (effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta) in rows {
            if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA {
                return Ok(None);
            }
            let current = (hourly_any_delta, hourly_delta, daily_delta, monthly_delta);
            match expected {
                Some(previous) if previous != current => return Ok(None),
                Some(_) => {}
                None => expected = Some(current),
            }
        }
        Ok(expected)
    }

    async fn get_linuxdo_system_tag_default_deltas_meta(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let Some(raw) = self
            .get_meta_string(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
            .await?
        else {
            return Ok(None);
        };
        Ok(parse_linuxdo_system_tag_default_deltas(&raw))
    }

    async fn set_linuxdo_system_tag_default_deltas_meta(
        &self,
        value: (i64, i64, i64, i64),
    ) -> Result<(), ProxyError> {
        self.set_meta_string(
            META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1,
            &format_linuxdo_system_tag_default_deltas(value),
        )
        .await
    }

    async fn allow_registration(&self) -> Result<bool, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ALLOW_REGISTRATION_V1)
            .await?
            .unwrap_or(1)
            != 0)
    }

    async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.set_meta_i64(META_KEY_ALLOW_REGISTRATION_V1, if allow { 1 } else { 0 })
            .await?;
        Ok(allow)
    }

    async fn sync_linuxdo_system_tag_default_deltas_with_env(&self) -> Result<(), ProxyError> {
        let current = linuxdo_system_tag_default_deltas();
        let previous = match self.get_linuxdo_system_tag_default_deltas_meta().await? {
            Some(value) => value,
            None => self
                .infer_linuxdo_system_tag_default_deltas_from_rows()
                .await?
                .unwrap_or(current),
        };
        if previous == current {
            self.set_linuxdo_system_tag_default_deltas_meta(current)
                .await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = ?
                 AND hourly_delta = ?
                 AND daily_delta = ?
                 AND monthly_delta = ?"#,
        )
        .bind(current.0)
        .bind(current.1)
        .bind(current.2)
        .bind(current.3)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .bind(previous.0)
        .bind(previous.1)
        .bind(previous.2)
        .bind(previous.3)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
        }
        self.set_linuxdo_system_tag_default_deltas_meta(current)
            .await?;
        Ok(())
    }

    async fn backfill_linuxdo_system_tag_default_deltas_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = 0
                 AND hourly_delta = 0
                 AND daily_delta = 0
                 AND monthly_delta = 0"#,
        )
        .bind(hourly_any_delta)
        .bind(hourly_delta)
        .bind(daily_delta)
        .bind(monthly_delta)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
        }
        Ok(())
    }

    async fn sync_linuxdo_system_tag_binding(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;
        self.sync_linuxdo_system_tag_binding_in_tx(&mut tx, user_id, trust_level)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(())
    }

    async fn sync_linuxdo_system_tag_binding_in_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let Some(level) = normalize_linuxdo_trust_level(trust_level) else {
            return Ok(());
        };
        let desired_key = linuxdo_system_key_for_level(level);
        let Some((tag_id,)) =
            sqlx::query_as::<_, (String,)>("SELECT id FROM user_tags WHERE system_key = ? LIMIT 1")
                .bind(&desired_key)
                .fetch_optional(&mut **tx)
                .await?
        else {
            eprintln!(
                "linuxdo system tag sync skipped for user {} trust_level {:?}: missing system tag for LinuxDo trust level {}",
                user_id, trust_level, level
            );
            return Ok(());
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"DELETE FROM user_tag_bindings
               WHERE user_id = ?
                 AND tag_id IN (
                     SELECT id FROM user_tags WHERE system_key LIKE 'linuxdo_l%'
                 )
                 AND tag_id <> ?"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .bind(USER_TAG_SOURCE_SYSTEM_LINUXDO)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn sync_linuxdo_system_tag_binding_best_effort(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) {
        if let Err(err) = self
            .sync_linuxdo_system_tag_binding(user_id, trust_level)
            .await
        {
            eprintln!(
                "linuxdo system tag sync error for user {} trust_level {:?}: {}",
                user_id, trust_level, err
            );
        }
    }

    async fn backfill_linuxdo_user_tag_bindings(&self) -> Result<(), ProxyError> {
        let rows = sqlx::query_as::<_, (String, Option<i64>)>(
            r#"SELECT user_id, trust_level
               FROM oauth_accounts
               WHERE provider = 'linuxdo'"#,
        )
        .fetch_all(&self.pool)
        .await?;
        for (user_id, trust_level) in rows {
            self.sync_linuxdo_system_tag_binding(&user_id, trust_level)
                .await?;
        }
        Ok(())
    }

    async fn fetch_user_tag_by_id(
        &self,
        tag_id: &str,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
            ),
        >(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               WHERE t.id = ?
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               LIMIT 1"#,
        )
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            )| UserTagRecord {
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            },
        ))
    }

    async fn list_user_tags(&self) -> Result<Vec<UserTagRecord>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, String, i64, i64, i64, i64, i64)>(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                )| UserTagRecord {
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                },
            )
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<UserTagRecord, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        for _ in 0..8 {
            let id = random_string(ALPHABET, 8);
            let inserted = sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(_) => {
                    return self
                        .fetch_user_tag_by_id(&id)
                        .await?
                        .ok_or_else(|| ProxyError::Other("created user tag missing".to_string()));
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    if db_err.message().contains("user_tags.name") {
                        return Err(ProxyError::Other(
                            "user tag name already exists".to_string(),
                        ));
                    }
                    continue;
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        Err(ProxyError::Other(
            "failed to allocate unique user tag id".to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_user_tag(
        &self,
        tag_id: &str,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(None);
        };
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let now = Utc::now().timestamp();
        if existing.is_system() {
            if existing.name != name
                || existing.display_name != display_name
                || existing.icon.as_deref() != icon
            {
                return Err(ProxyError::Other(
                    "system user tags only allow effect updates".to_string(),
                ));
            }
            sqlx::query(
                r#"UPDATE user_tags
                   SET effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        } else {
            let updated = sqlx::query(
                r#"UPDATE user_tags
                   SET name = ?,
                       display_name = ?,
                       icon = ?,
                       effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await;
            match updated {
                Ok(_) => {}
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    return Err(ProxyError::Other(
                        "user tag name already exists".to_string(),
                    ));
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        self.fetch_user_tag_by_id(tag_id).await
    }

    async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if existing.is_system() {
            return Err(ProxyError::Other(
                "system user tags cannot be deleted".to_string(),
            ));
        }
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_tag_bindings WHERE tag_id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM user_tags WHERE id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        Ok(true)
    }

    async fn bind_user_tag_to_user(&self, user_id: &str, tag_id: &str) -> Result<bool, ProxyError> {
        let user_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if user_exists == 0 {
            return Ok(false);
        }
        let Some(tag) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if tag.is_system() {
            return Err(ProxyError::Other(
                "system user tags are managed by the server".to_string(),
            ));
        }
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .bind(USER_TAG_SOURCE_MANUAL)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        let binding = sqlx::query_as::<_, (String, Option<String>)>(
            r#"SELECT b.source, t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND b.tag_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((source, system_key)) = binding else {
            return Ok(false);
        };
        if source != USER_TAG_SOURCE_MANUAL || system_key.is_some() {
            return Err(ProxyError::Other(
                "system-managed user tag bindings are read-only".to_string(),
            ));
        }
        sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ? AND tag_id = ?")
            .bind(user_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<UserTagBindingRecord>>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            r#"SELECT
                 b.user_id,
                 b.source,
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC");

        let rows = builder
            .build_query_as::<(
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;
        let mut map: HashMap<String, Vec<UserTagBindingRecord>> = HashMap::new();
        for (
            user_id,
            source,
            tag_id,
            name,
            display_name,
            icon,
            system_key,
            effect_kind,
            hourly_any_delta,
            hourly_delta,
            daily_delta,
            monthly_delta,
        ) in rows
        {
            map.entry(user_id.clone())
                .or_default()
                .push(UserTagBindingRecord {
                    source,
                    tag: UserTagRecord {
                        id: tag_id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        user_count: 0,
                    },
                });
        }
        Ok(map)
    }

    async fn list_user_tag_bindings_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserTagBindingRecord>, ProxyError> {
        Ok(self
            .list_user_tag_bindings_for_users(&[user_id.to_string()])
            .await?
            .remove(user_id)
            .unwrap_or_default())
    }

    async fn resolve_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.ensure_account_quota_limits_for_users(user_ids).await?;
        let base_limits = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let tag_bindings = self.list_user_tag_bindings_for_users(user_ids).await?;
        let defaults = AccountQuotaLimits::zero_base();
        let mut map = HashMap::new();
        for user_id in user_ids {
            let base = base_limits
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| defaults.clone());
            let tags = tag_bindings.get(user_id).cloned().unwrap_or_default();
            map.insert(
                user_id.clone(),
                build_account_quota_resolution(base, tags).effective,
            );
        }
        Ok(map)
    }

    async fn resolve_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaResolution, ProxyError> {
        if let Some(cached) = self.cached_account_quota_resolution(user_id).await {
            return Ok(cached);
        }

        let base = self.ensure_account_quota_limits(user_id).await?;
        let tags = self.list_user_tag_bindings_for_user(user_id).await?;
        let resolution = build_account_quota_resolution(base, tags);
        self.cache_account_quota_resolution(user_id, &resolution)
            .await;
        Ok(resolution)
    }

    async fn fetch_user_success_failure(
        &self,
        user_id: &str,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let day_start = start_of_day(now).timestamp();
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs l
            JOIN user_token_bindings b ON b.token_id = l.token_id
            WHERE b.user_id = ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_start)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(OUTCOME_ERROR)
        .bind(day_start)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }

    async fn fetch_user_last_activity(&self, user_id: &str) -> Result<Option<i64>, ProxyError> {
        let row = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT MAX(l.created_at)
               FROM auth_token_logs l
               JOIN user_token_bindings b ON b.token_id = l.token_id
               WHERE b.user_id = ?"#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn insert_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let state = random_string(ALPHABET, 48);
            let res = sqlx::query(
                r#"INSERT INTO oauth_login_states
                   (state, provider, redirect_to, binding_hash, bind_token_id, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&state)
            .bind(provider)
            .bind(redirect_to.map(str::trim).filter(|value| !value.is_empty()))
            .bind(
                binding_hash
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
            .bind(bind_token_id.map(str::trim).filter(|value| !value.is_empty()))
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => return Ok(state),
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = if let Some(hash) = binding_hash
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash = ?
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash IS NULL
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((redirect_to, bind_token_id)) = row else {
            tx.rollback().await.ok();
            return Ok(None);
        };

        let updated = sqlx::query(
            r#"UPDATE oauth_login_states
               SET consumed_at = ?
               WHERE state = ? AND provider = ? AND consumed_at IS NULL"#,
        )
        .bind(now)
        .bind(state)
        .bind(provider)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(None);
        }

        tx.commit().await?;
        Ok(Some(OAuthLoginStatePayload {
            redirect_to,
            bind_token_id,
        }))
    }

    async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        let display_name = profile
            .name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| {
                profile
                    .username
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        let username = profile
            .username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let avatar = profile
            .avatar_template
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let active = if profile.active { 1 } else { 0 };
        let now = Utc::now().timestamp();

        for _ in 0..4 {
            let mut tx = self.pool.begin().await?;

            let existing = sqlx::query_as::<_, (String,)>(
                r#"SELECT user_id
                   FROM oauth_accounts
                   WHERE provider = ? AND provider_user_id = ?
                   LIMIT 1"#,
            )
            .bind(&profile.provider)
            .bind(&profile.provider_user_id)
            .fetch_optional(&mut *tx)
            .await?;

            let user_id = if let Some((user_id,)) = existing {
                user_id
            } else {
                const ALPHABET: &[u8] =
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
                let mut created_user_id = None;
                for _ in 0..8 {
                    let candidate = random_string(ALPHABET, 12);
                    let inserted = sqlx::query(
                        r#"INSERT INTO users
                           (id, display_name, username, avatar_template, active, created_at, updated_at, last_login_at)
                           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(&candidate)
                    .bind(display_name.clone())
                    .bind(username.clone())
                    .bind(avatar.clone())
                    .bind(active)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await;

                    match inserted {
                        Ok(_) => {
                            created_user_id = Some(candidate);
                            break;
                        }
                        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                            continue;
                        }
                        Err(err) => {
                            tx.rollback().await.ok();
                            return Err(ProxyError::Database(err));
                        }
                    }
                }

                let Some(user_id) = created_user_id else {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(
                        "failed to allocate unique local user id".to_string(),
                    ));
                };

                let zero_base = AccountQuotaLimits::zero_base();
                sqlx::query(
                    r#"INSERT INTO account_quota_limits (
                           user_id,
                           hourly_any_limit,
                           hourly_limit,
                           daily_limit,
                           monthly_limit,
                           inherits_defaults,
                           created_at,
                           updated_at
                       )
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
                )
                .bind(&user_id)
                .bind(zero_base.hourly_any_limit)
                .bind(zero_base.hourly_limit)
                .bind(zero_base.daily_limit)
                .bind(zero_base.monthly_limit)
                .bind(0)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;

                let inserted_account = sqlx::query(
                    r#"INSERT INTO oauth_accounts
                       (provider, provider_user_id, user_id, username, name, avatar_template, active, trust_level, raw_payload, created_at, updated_at)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                )
                .bind(&profile.provider)
                .bind(&profile.provider_user_id)
                .bind(&user_id)
                .bind(username.clone())
                .bind(display_name.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(profile.trust_level)
                .bind(profile.raw_payload_json.clone())
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_account {
                    Ok(_) => user_id,
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        tx.rollback().await.ok();
                        continue;
                    }
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            };

            sqlx::query(
                r#"UPDATE users
                   SET display_name = ?, username = ?, avatar_template = ?, active = ?, updated_at = ?, last_login_at = ?
                   WHERE id = ?"#,
            )
            .bind(display_name.clone())
            .bind(username.clone())
            .bind(avatar.clone())
            .bind(active)
            .bind(now)
            .bind(now)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"UPDATE oauth_accounts
                   SET username = ?, name = ?, avatar_template = ?, active = ?, trust_level = ?, raw_payload = ?, updated_at = ?
                   WHERE provider = ? AND provider_user_id = ?"#,
            )
            .bind(username.clone())
            .bind(display_name.clone())
            .bind(avatar.clone())
            .bind(active)
            .bind(profile.trust_level)
            .bind(profile.raw_payload_json.clone())
            .bind(now)
            .bind(&profile.provider)
            .bind(&profile.provider_user_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            if profile.provider == "linuxdo" {
                self.sync_linuxdo_system_tag_binding_best_effort(&user_id, profile.trust_level)
                    .await;
            }
            return Ok(UserIdentity {
                user_id,
                provider: profile.provider.clone(),
                provider_user_id: profile.provider_user_id.clone(),
                display_name,
                username,
                avatar_template: avatar,
            });
        }

        Err(ProxyError::Other(
            "failed to upsert oauth account after retries".to_string(),
        ))
    }

    async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        let row = sqlx::query_scalar::<_, i64>(
            r#"SELECT 1
               FROM oauth_accounts
               WHERE provider = ? AND provider_user_id = ?
               LIMIT 1"#,
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.ensure_user_token_binding_with_preferred(user_id, note, None)
            .await
    }

    async fn fetch_active_token_secret_by_id(
        &self,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT secret
               FROM auth_tokens
               WHERE id = ? AND enabled = 1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        let preferred_token_id = preferred_token_id
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(preferred_token_id) = preferred_token_id
            && let Some(preferred_secret) = self
                .fetch_active_token_secret_by_id(preferred_token_id)
                .await?
        {
            for _ in 0..4 {
                let now = Utc::now().timestamp();
                let mut tx = self.pool.begin().await?;

                let owner = sqlx::query_as::<_, (String,)>(
                    r#"SELECT user_id
                       FROM user_token_bindings
                       WHERE token_id = ?
                       LIMIT 1"#,
                )
                .bind(preferred_token_id)
                .fetch_optional(&mut *tx)
                .await?;

                match owner {
                    Some((owner_user_id,)) if owner_user_id != user_id => {
                        tx.rollback().await.ok();
                        break;
                    }
                    Some(_) => {
                        let touch = sqlx::query(
                            r#"UPDATE user_token_bindings
                               SET updated_at = ?
                               WHERE user_id = ? AND token_id = ?"#,
                        )
                        .bind(now)
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .execute(&mut *tx)
                        .await;
                        match touch {
                            Ok(_) => {
                                tx.commit().await?;
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                    None => {
                        let result = sqlx::query(
                            r#"INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at)
                               VALUES (?, ?, ?, ?)
                               ON CONFLICT(user_id, token_id) DO UPDATE SET
                                   updated_at = excluded.updated_at"#,
                        )
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .bind(now)
                        .bind(now)
                        .execute(&mut *tx)
                        .await;

                        match result {
                            Ok(_) => {
                                tx.commit().await?;
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                                tx.rollback().await.ok();
                                continue;
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                }
            }
        }

        if let Some(existing) = self.fetch_user_token_any_status(user_id).await? {
            self.cache_token_binding(&existing.id, Some(user_id)).await;
            return Ok(existing);
        }

        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        let note = note.unwrap_or("").trim().to_string();

        for _ in 0..4 {
            let mut tx = self.pool.begin().await?;
            if let Some((token_id, secret)) = sqlx::query_as::<_, (String, String)>(
                r#"SELECT b.token_id, t.secret
                   FROM user_token_bindings b
                   JOIN auth_tokens t ON t.id = b.token_id
                   WHERE b.user_id = ?
                   ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
                   LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            {
                tx.rollback().await.ok();
                return Ok(AuthTokenSecret {
                    id: token_id.clone(),
                    token: Self::compose_full_token(&token_id, &secret),
                });
            }

            let mut created: Option<(String, String)> = None;
            for _ in 0..8 {
                let token_id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);

                let inserted_token = sqlx::query(
                    r#"INSERT INTO auth_tokens
                       (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)"#,
                )
                .bind(&token_id)
                .bind(&secret)
                .bind(&note)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_token {
                    Ok(_) => {
                        created = Some((token_id, secret));
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            }

            let Some((token_id, secret)) = created else {
                tx.rollback().await.ok();
                return Err(ProxyError::Other(
                    "failed to create auth token for user binding".to_string(),
                ));
            };

            let inserted_binding = sqlx::query(
                r#"INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at)
                   VALUES (?, ?, ?, ?)"#,
            )
            .bind(user_id)
            .bind(&token_id)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;

            match inserted_binding {
                Ok(_) => {
                    tx.commit().await?;
                    self.cache_token_binding(&token_id, Some(user_id)).await;
                    return Ok(AuthTokenSecret {
                        id: token_id.clone(),
                        token: Self::compose_full_token(&token_id, &secret),
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    tx.rollback().await.ok();
                    continue;
                }
                Err(err) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Database(err));
                }
            }
        }

        Err(ProxyError::Other(
            "failed to ensure user token binding after retries".to_string(),
        ))
    }

    async fn fetch_user_token_any_status(
        &self,
        user_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"SELECT b.token_id, t.secret
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ?
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(token_id, secret)| AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    async fn get_user_token(&self, user_id: &str) -> Result<UserTokenLookup, ProxyError> {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<i64>, Option<i64>)>(
            r#"SELECT b.token_id, t.secret, t.enabled, t.deleted_at
               FROM user_token_bindings b
               LEFT JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ?
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((token_id, maybe_secret, maybe_enabled, maybe_deleted_at)) = row else {
            return Ok(UserTokenLookup::MissingBinding);
        };
        let Some(secret) = maybe_secret else {
            return Ok(UserTokenLookup::Unavailable);
        };
        let enabled = maybe_enabled.unwrap_or(0);
        if enabled != 1 || maybe_deleted_at.is_some() {
            return Ok(UserTokenLookup::Unavailable);
        }

        Ok(UserTokenLookup::Found(AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        let now = Utc::now().timestamp();
        let expires_at = now + session_max_age_secs.max(60);

        sqlx::query("DELETE FROM user_sessions WHERE expires_at < ? OR revoked_at IS NOT NULL")
            .bind(now)
            .execute(&self.pool)
            .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let inserted = sqlx::query(
                r#"INSERT INTO user_sessions (token, user_id, provider, created_at, expires_at, revoked_at)
                   VALUES (?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&token)
            .bind(&user.user_id)
            .bind(&user.provider)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(_) => {
                    return Ok(UserSession {
                        token,
                        user: user.clone(),
                        expires_at,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    async fn get_user_session(&self, token: &str) -> Result<Option<UserSession>, ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("DELETE FROM user_sessions WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await?;

        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
            ),
        >(
            r#"SELECT
                    s.token,
                    s.user_id,
                    s.provider,
                    oa.provider_user_id,
                    u.display_name,
                    u.username,
                    u.avatar_template,
                    s.expires_at
               FROM user_sessions s
               JOIN users u ON u.id = s.user_id
               LEFT JOIN oauth_accounts oa ON oa.user_id = u.id AND oa.provider = s.provider
               WHERE s.token = ? AND s.revoked_at IS NULL AND s.expires_at > ?
               LIMIT 1"#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                token,
                user_id,
                provider,
                provider_user_id,
                display_name,
                username,
                avatar_template,
                expires_at,
            )| UserSession {
                token,
                user: UserIdentity {
                    user_id,
                    provider,
                    provider_user_id: provider_user_id.unwrap_or_default(),
                    display_name,
                    username,
                    avatar_template,
                },
                expires_at,
            },
        ))
    }

    async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE user_sessions SET revoked_at = ? WHERE token = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ----- Token usage logs & metrics -----
    async fn backfill_auth_token_log_request_kinds(&self) -> Result<(), ProxyError> {
        let fallback_key_sql = token_request_kind_fallback_key_sql();
        let fallback_label_sql = token_request_kind_fallback_label_sql();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        // Normalize legacy rows once at startup so read paths can filter directly on stored request kind columns.
        let query = format!(
            r#"
            UPDATE auth_token_logs
            SET
                request_kind_key = {fallback_key_sql},
                request_kind_label = {fallback_label_sql}
            WHERE {needs_fallback_sql}
            "#,
        );
        sqlx::query(query.as_str()).execute(&self.pool).await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_token_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
    ) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        let counts_business_quota = if counts_business_quota { 1i64 } else { 0i64 };
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status,
                request_kind_key, request_kind_label, request_kind_detail,
                result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(counts_business_quota)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(created_at)
        .bind(token_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_token_log_pending_billing(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
    ) -> Result<i64, ProxyError> {
        let created_at = Utc::now().timestamp();
        let counts_business_quota = if counts_business_quota { 1i64 } else { 0i64 };
        let log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(counts_business_quota)
        .bind(business_credits)
        .bind(billing_subject)
        .bind(BILLING_STATE_PENDING)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(created_at)
        .bind(token_id)
        .execute(&self.pool)
        .await?;

        Ok(log_id)
    }

    async fn list_pending_billing_log_ids(
        &self,
        billing_subject: &str,
    ) -> Result<Vec<i64>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT id
            FROM auth_token_logs
            WHERE billing_state = ? AND billing_subject = ? AND COALESCE(business_credits, 0) > 0
            ORDER BY id ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(billing_subject)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    async fn list_pending_billing_subjects_for_token(
        &self,
        token_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT DISTINCT billing_subject
            FROM auth_token_logs
            WHERE billing_state = ?
              AND token_id = ?
              AND billing_subject IS NOT NULL
              AND COALESCE(business_credits, 0) > 0
            ORDER BY billing_subject ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(token_id)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    async fn apply_pending_billing_log(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        let mut tx = self.pool.begin().await?;
        #[cfg(test)]
        let force_claim_miss = {
            let mut forced = self.forced_pending_claim_miss_log_ids.lock().await;
            forced.remove(&log_id)
        };
        #[cfg(not(test))]
        let force_claim_miss = false;

        let claimed = if force_claim_miss {
            None
        } else {
            sqlx::query_as::<_, (i64, Option<String>, i64)>(
                r#"
                UPDATE auth_token_logs
                SET billing_state = ?
                WHERE id = ? AND billing_state = ?
                RETURNING COALESCE(business_credits, 0), billing_subject, created_at
                "#,
            )
            .bind(BILLING_STATE_CHARGED)
            .bind(log_id)
            .bind(BILLING_STATE_PENDING)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((credits, billing_subject, created_at)) = claimed else {
            let billing_state = sqlx::query_scalar::<_, String>(
                "SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1",
            )
            .bind(log_id)
            .fetch_optional(&mut *tx)
            .await?;
            match billing_state.as_deref() {
                Some(BILLING_STATE_CHARGED) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::AlreadySettled);
                }
                Some(BILLING_STATE_PENDING) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::RetryLater);
                }
                Some(other) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::QuotaDataMissing {
                        reason: format!(
                            "invalid billing_state for auth_token_logs.id={log_id}: {other}",
                        ),
                    });
                }
                None => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(format!(
                        "pending billing log not found: {log_id}",
                    )));
                }
            }
        };

        if credits <= 0 {
            tx.commit().await?;
            return Ok(PendingBillingSettleOutcome::Charged);
        }

        let Some(billing_subject) = billing_subject else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!("missing billing_subject for auth_token_logs.id={log_id}"),
            });
        };

        let charge_time = Utc
            .timestamp_opt(created_at, 0)
            .single()
            .unwrap_or_else(Utc::now);
        let charge_ts = charge_time.timestamp();
        let minute_bucket = charge_ts - (charge_ts % SECS_PER_MINUTE);
        let hour_bucket = charge_ts - (charge_ts % SECS_PER_HOUR);
        let month_start = start_of_month(charge_time).timestamp();

        if let Some(user_id) = billing_subject.strip_prefix("account:") {
            sqlx::query(
                r#"
                INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id, bucket_start, granularity)
                DO UPDATE SET count = account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id, bucket_start, granularity)
                DO UPDATE SET count = account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(hour_bucket)
            .bind(GRANULARITY_HOUR)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                        ELSE account_monthly_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                        ELSE account_monthly_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(user_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;
        } else if let Some(token_id) = billing_subject.strip_prefix("token:") {
            sqlx::query(
                r#"
                INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(token_id, bucket_start, granularity)
                DO UPDATE SET count = token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(token_id, bucket_start, granularity)
                DO UPDATE SET count = token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(hour_bucket)
            .bind(GRANULARITY_HOUR)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO auth_token_quota (token_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(token_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                        ELSE auth_token_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                        ELSE auth_token_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(token_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;
        } else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!(
                    "invalid billing_subject for auth_token_logs.id={log_id}: {billing_subject}",
                ),
            });
        }

        tx.commit().await?;
        Ok(PendingBillingSettleOutcome::Charged)
    }

    async fn annotate_pending_billing_log(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET error_message = CASE
                WHEN error_message IS NULL OR error_message = '' THEN ?
                WHEN error_message = ? THEN error_message
                ELSE error_message || ' | ' || ?
            END
            WHERE id = ?
            "#,
        )
        .bind(message)
        .bind(message)
        .bind(message)
        .bind(log_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn acquire_quota_subject_lock(
        &self,
        subject: &str,
        ttl: Duration,
        wait_timeout: Duration,
    ) -> Result<QuotaSubjectDbLease, ProxyError> {
        let owner = format!(
            "{}:{}",
            std::process::id(),
            QUOTA_SUBJECT_LOCK_OWNER_SEQ.fetch_add(1, AtomicOrdering::Relaxed)
        );
        let deadline = Instant::now() + wait_timeout;
        let ttl_secs = ttl.as_secs().max(1) as i64;

        loop {
            let now = Utc::now().timestamp();
            let expires_at = now + ttl_secs;
            let mut tx = self.pool.begin().await?;
            sqlx::query("DELETE FROM quota_subject_locks WHERE subject = ? AND expires_at <= ?")
                .bind(subject)
                .bind(now)
                .execute(&mut *tx)
                .await?;

            let inserted = sqlx::query(
                r#"
                INSERT OR IGNORE INTO quota_subject_locks (subject, owner, expires_at, updated_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(subject)
            .bind(&owner)
            .bind(expires_at)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            if inserted.rows_affected() == 1 {
                tx.commit().await?;
                return Ok(QuotaSubjectDbLease {
                    subject: subject.to_string(),
                    owner,
                    ttl,
                });
            }

            tx.rollback().await.ok();
            if Instant::now() >= deadline {
                return Err(ProxyError::Other(format!(
                    "timed out acquiring quota subject lock for {subject}",
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn refresh_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let expires_at = now + lease.ttl.as_secs().max(1) as i64;
        let rows = sqlx::query(
            "UPDATE quota_subject_locks SET expires_at = ?, updated_at = ? WHERE subject = ? AND owner = ?",
        )
        .bind(expires_at)
        .bind(now)
        .bind(&lease.subject)
        .bind(&lease.owner)
        .execute(&self.pool)
        .await?;
        if rows.rows_affected() == 0 {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                lease.subject,
            )));
        }
        Ok(())
    }

    async fn release_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        sqlx::query("DELETE FROM quota_subject_locks WHERE subject = ? AND owner = ?")
            .bind(&lease.subject)
            .bind(&lease.owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fetch_token_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        type TokenLogRow = (
            i64,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            i64,
        );
        let rows = if let Some(bid) = before_id {
            sqlx::query_as::<_, TokenLogRow>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                       request_kind_key, request_kind_label, request_kind_detail,
                       result_status, error_message, created_at
                FROM auth_token_logs
                WHERE token_id = ? AND id < ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(bid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, TokenLogRow>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                       request_kind_key, request_kind_label, request_kind_detail,
                       result_status, error_message, created_at
                FROM auth_token_logs
                WHERE token_id = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    business_credits,
                    request_kind_key,
                    request_kind_label,
                    request_kind_detail,
                    result_status,
                    error_message,
                    created_at,
                )| {
                    let request_kind = finalize_token_request_kind(
                        method.as_str(),
                        path.as_str(),
                        query.as_deref(),
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                    );
                    TokenLogRecord {
                        id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        business_credits,
                        request_kind_key: request_kind.key,
                        request_kind_label: request_kind.label,
                        request_kind_detail: request_kind.detail,
                        result_status,
                        error_message,
                        created_at,
                    }
                },
            )
            .collect())
    }

    pub async fn fetch_token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let end_exclusive = until.unwrap_or(now_ts);
        if end_exclusive <= since {
            return Ok(TokenSummary {
                total_requests: 0,
                success_count: 0,
                error_count: 0,
                quota_exhausted_count: 0,
                last_activity: None,
            });
        }

        let rows = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count,
                quota_exhausted_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(since)
        .bind(end_exclusive)
        .fetch_all(&self.pool)
        .await?;

        let mut total_requests = 0;
        let mut success_count = 0;
        let mut system_failure_count = 0;
        let mut external_failure_count = 0;
        let mut quota_exhausted_count = 0;
        let mut last_activity: Option<i64> = None;

        for (bucket_start, success, system_failure, external_failure, quota_exhausted) in rows {
            success_count += success;
            system_failure_count += system_failure;
            external_failure_count += external_failure;
            quota_exhausted_count += quota_exhausted;
            total_requests += success + system_failure + external_failure + quota_exhausted;
            let bucket_end = bucket_start + TOKEN_USAGE_STATS_BUCKET_SECS;
            last_activity = Some(match last_activity {
                Some(prev) if prev > bucket_end => prev,
                _ => bucket_end,
            });
        }

        let error_count = system_failure_count + external_failure_count;

        Ok(TokenSummary {
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            last_activity,
        })
    }

    pub async fn fetch_token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
    ) -> Result<(Vec<TokenLogRecord>, i64), ProxyError> {
        let per_page = per_page.clamp(1, 200) as i64;
        let page = page.max(1) as i64;
        let offset = (page - 1) * per_page;
        type TokenLogRow = (
            i64,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            i64,
        );
        let filtered_request_kinds: Vec<&str> = request_kinds
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        let fallback_key_sql = token_request_kind_fallback_key_sql();

        let mut total_query =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ");
        total_query.push_bind(token_id);
        total_query.push(" AND created_at >= ");
        total_query.push_bind(since);
        if let Some(until) = until {
            total_query.push(" AND created_at < ");
            total_query.push_bind(until);
        }
        if !filtered_request_kinds.is_empty() {
            total_query.push(" AND (request_kind_key IN (");
            {
                let mut separated = total_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            total_query.push(" OR (");
            total_query.push(needs_fallback_sql);
            total_query.push(" AND ");
            total_query.push(fallback_key_sql);
            total_query.push(" IN (");
            {
                let mut separated = total_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            total_query.push("))");
        }
        let total: i64 = total_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        let mut rows_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT id, method, path, query, http_status, mcp_status,
                   CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                   request_kind_key,
                   request_kind_label,
                   request_kind_detail,
                   result_status, error_message, created_at
            FROM auth_token_logs
            WHERE token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND created_at < ");
            rows_query.push_bind(until);
        }
        if !filtered_request_kinds.is_empty() {
            rows_query.push(" AND (request_kind_key IN (");
            {
                let mut separated = rows_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            rows_query.push(" OR (");
            rows_query.push(needs_fallback_sql);
            rows_query.push(" AND ");
            rows_query.push(fallback_key_sql);
            rows_query.push(" IN (");
            {
                let mut separated = rows_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            rows_query.push("))");
        }
        rows_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        rows_query.push_bind(per_page);
        rows_query.push(" OFFSET ");
        rows_query.push_bind(offset);
        let rows = rows_query
            .build_query_as::<TokenLogRow>()
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(
                |(
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    business_credits,
                    request_kind_key,
                    request_kind_label,
                    request_kind_detail,
                    result_status,
                    error_message,
                    created_at,
                )| {
                    let request_kind = finalize_token_request_kind(
                        method.as_str(),
                        path.as_str(),
                        query.as_deref(),
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                    );
                    TokenLogRecord {
                        id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        business_credits,
                        request_kind_key: request_kind.key,
                        request_kind_label: request_kind.label,
                        request_kind_detail: request_kind.detail,
                        result_status,
                        error_message,
                        created_at,
                    }
                },
            )
            .collect();

        Ok((items, total))
    }

    pub async fn fetch_token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        let fallback_key_sql = token_request_kind_fallback_key_sql();
        let fallback_label_sql = token_request_kind_fallback_label_sql();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        let mut stored_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT DISTINCT request_kind_key, request_kind_label
            FROM auth_token_logs
            WHERE token_id =
            "#,
        );
        stored_query.push_bind(token_id);
        stored_query.push(" AND created_at >= ");
        stored_query.push_bind(since);
        if let Some(until) = until {
            stored_query.push(" AND created_at < ");
            stored_query.push_bind(until);
        }
        stored_query.push(
            r#"
              AND request_kind_key IS NOT NULL
              AND TRIM(request_kind_key) <> ''
              AND request_kind_label IS NOT NULL
              AND TRIM(request_kind_label) <> ''
              AND NOT (
            "#,
        );
        stored_query.push(token_request_kind_needs_fallback_sql());
        stored_query.push(")");

        let options = stored_query
            .build_query_as::<(String, String)>()
            .fetch_all(&self.pool)
            .await?;

        let legacy_fallback_query = if until.is_some() {
            format!(
                r#"
                SELECT DISTINCT
                    {fallback_key_sql} AS request_kind_key,
                    {fallback_label_sql} AS request_kind_label
                FROM auth_token_logs
                WHERE token_id = ?
                  AND created_at >= ?
                  AND created_at < ?
                  AND {needs_fallback_sql}
                "#
            )
        } else {
            format!(
                r#"
                SELECT DISTINCT
                    {fallback_key_sql} AS request_kind_key,
                    {fallback_label_sql} AS request_kind_label
                FROM auth_token_logs
                WHERE token_id = ?
                  AND created_at >= ?
                  AND {needs_fallback_sql}
                "#
            )
        };
        let legacy_options = if let Some(until) = until {
            sqlx::query_as::<_, (String, String)>(legacy_fallback_query.as_str())
                .bind(token_id)
                .bind(since)
                .bind(until)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_as::<_, (String, String)>(legacy_fallback_query.as_str())
                .bind(token_id)
                .bind(since)
                .fetch_all(&self.pool)
                .await?
        };
        let mut options_by_key = BTreeMap::<String, String>::new();
        for (key, label) in options.into_iter().chain(legacy_options.into_iter()) {
            match options_by_key.get_mut(&key) {
                Some(current_label) if prefer_request_kind_label(current_label, &label) => {
                    *current_label = label;
                }
                Some(_) => {}
                None => {
                    options_by_key.insert(key, label);
                }
            }
        }

        let mut normalized_options = options_by_key
            .into_iter()
            .map(|(key, label)| TokenRequestKindOption { key, label })
            .collect::<Vec<_>>();
        normalized_options.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.key.cmp(&right.key))
        });

        Ok(normalized_options)
    }

    pub async fn fetch_token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        let hours = hours.clamp(1, 168); // up to 7 days
        let now_ts = Utc::now().timestamp();
        let current_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let window_start = current_bucket - (hours - 1) * SECS_PER_HOUR;
        let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(window_start)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(bucket_start, success_count, system_failure_count, external_failure_count)| {
                    TokenHourlyBucket {
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    }
                },
            )
            .collect())
    }

    pub async fn fetch_token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        if until <= since {
            return Err(ProxyError::Other("invalid usage window".into()));
        }
        if bucket_secs <= 0 {
            return Err(ProxyError::Other("bucket_secs must be positive".into()));
        }
        let bucket_secs = match bucket_secs {
            s if s == SECS_PER_HOUR => SECS_PER_HOUR,
            s if s == SECS_PER_DAY => SECS_PER_DAY,
            _ => {
                return Err(ProxyError::Other(
                    "bucket_secs must be either 3600 (hour) or 86400 (day)".into(),
                ));
            }
        };
        let span = until - since;
        let mut bucket_count = span / bucket_secs;
        if span % bucket_secs != 0 {
            bucket_count += 1;
        }
        if bucket_count > 1000 {
            return Err(ProxyError::Other(
                "requested usage series is too large".into(),
            ));
        }
        if bucket_secs == SECS_PER_HOUR {
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            Ok(rows
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect())
        } else {
            // Aggregate hourly stats into daily buckets.
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            let mut by_day: HashMap<i64, (i64, i64, i64)> = HashMap::new();
            for (bucket_start, success, system_failure, external_failure) in rows {
                let day_start = bucket_start - (bucket_start % SECS_PER_DAY);
                let entry = by_day.entry(day_start).or_insert((0, 0, 0));
                entry.0 += success;
                entry.1 += system_failure;
                entry.2 += external_failure;
            }

            let mut buckets: Vec<TokenUsageBucket> = by_day
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        (success_count, system_failure_count, external_failure_count),
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect();
            buckets.sort_by_key(|b| b.bucket_start);
            Ok(buckets)
        }
    }

    async fn reset_monthly(&self) -> Result<(), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        let now_ts = now.timestamp();

        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE status = ?
              AND status_changed_at IS NOT NULL
              AND status_changed_at < ?
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now_ts)
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn mark_quota_exhausted(&self, key: &str) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?, last_used_at = ?
            WHERE api_key = ? AND status <> ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(now)
        .bind(now)
        .bind(key)
        .bind(STATUS_DISABLED)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn restore_active_status(&self, key: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE api_key = ? AND status = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn quarantine_key_by_id(
        &self,
        key_id: &str,
        source: &str,
        reason_code: &str,
        reason_summary: &str,
        reason_detail: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let quarantine_id = nanoid!(12);
        let insert_result = sqlx::query(
            r#"
            INSERT INTO api_key_quarantines (
                id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
            ON CONFLICT(key_id) WHERE cleared_at IS NULL DO NOTHING
            "#,
        )
        .bind(quarantine_id)
        .bind(key_id)
        .bind(source)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(reason_detail)
        .bind(now)
        .execute(&self.pool)
        .await?;

        if insert_result.rows_affected() == 0 {
            sqlx::query(
                r#"
                UPDATE api_key_quarantines
                SET source = ?, reason_code = ?, reason_summary = ?, reason_detail = ?
                WHERE key_id = ? AND cleared_at IS NULL
                "#,
            )
            .bind(source)
            .bind(reason_code)
            .bind(reason_summary)
            .bind(reason_detail)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    async fn clear_key_quarantine_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_key_quarantines
            SET cleared_at = ?
            WHERE key_id = ? AND cleared_at IS NULL
            "#,
        )
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // Admin ops: add/undelete key by secret
    async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.add_or_undelete_key_in_group(api_key, None).await
    }

    // Admin ops: add/undelete key by secret and optionally assign a group.
    async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        let (id, _) = self
            .add_or_undelete_key_with_status_in_group(api_key, group)
            .await?;
        Ok(id)
    }

    // Admin ops: add/undelete key by secret with status
    async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group(api_key, None)
            .await
    }

    // Admin ops: add/undelete key by secret with status and optional group assignment.
    //
    // Behavior:
    // - created / undeleted: set group_name when group is provided and non-empty
    // - existed: set group_name only if the stored group is empty (do not override)
    async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let normalized_group = group
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string);
        let mut retry_idx = 0usize;

        loop {
            match self
                .add_or_undelete_key_with_status_in_group_once(api_key, normalized_group.as_deref())
                .await
            {
                Ok(result) => return Ok(result),
                Err(err)
                    if is_transient_sqlite_write_error(&err)
                        && retry_idx < API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS.len() =>
                {
                    let backoff_ms = API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS[retry_idx];
                    retry_idx += 1;
                    let key_preview = preview_key(api_key);
                    eprintln!(
                        "api key upsert transient sqlite error (api_key_preview={}, attempt={}, backoff={}ms): {}",
                        key_preview, retry_idx, backoff_ms, err
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn add_or_undelete_key_with_status_in_group_once(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().timestamp();

        let operation_result: Result<(String, ApiKeyUpsertStatus), ProxyError> = async {
            if let Some((id, deleted_at, existing_group)) =
                sqlx::query_as::<_, (String, Option<i64>, Option<String>)>(
                    "SELECT id, deleted_at, group_name FROM api_keys WHERE api_key = ? LIMIT 1",
                )
                .bind(api_key)
                .fetch_optional(&mut *tx)
                .await?
            {
                let existing_empty = existing_group
                    .as_deref()
                    .map(str::trim)
                    .map(|g| g.is_empty())
                    .unwrap_or(true);

                if deleted_at.is_some() {
                    if let Some(group) = group {
                        sqlx::query(
                            "UPDATE api_keys SET deleted_at = NULL, group_name = ? WHERE id = ?",
                        )
                        .bind(group)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await?;
                    } else {
                        sqlx::query("UPDATE api_keys SET deleted_at = NULL WHERE id = ?")
                            .bind(&id)
                            .execute(&mut *tx)
                            .await?;
                    }

                    return Ok((id, ApiKeyUpsertStatus::Undeleted));
                }

                if let Some(group) = group
                    && existing_empty
                {
                    sqlx::query("UPDATE api_keys SET group_name = ? WHERE id = ?")
                        .bind(group)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await?;
                }

                return Ok((id, ApiKeyUpsertStatus::Existed));
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (id, api_key, group_name, status, status_changed_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(api_key)
            .bind(group)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            Ok((id, ApiKeyUpsertStatus::Created))
        }
        .await;

        match operation_result {
            Ok(result) => {
                tx.commit().await?;
                Ok(result)
            }
            Err(err) => {
                tx.rollback().await.ok();
                Err(err)
            }
        }
    }

    // Admin ops: soft-delete by ID (mark deleted_at)
    async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND status IN (?, ?) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key_id)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn touch_key(&self, key: &str, timestamp: i64) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = ?
            WHERE api_key = ?
            "#,
        )
        .bind(timestamp)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn log_attempt(&self, entry: AttemptLog<'_>) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        let status_code = entry.status.map(|code| code.as_u16() as i64);

        let forwarded_json =
            serde_json::to_string(entry.forwarded_headers).unwrap_or_else(|_| "[]".to_string());
        let dropped_json =
            serde_json::to_string(entry.dropped_headers).unwrap_or_else(|_| "[]".to_string());

        let bucket_start = local_day_bucket_start_utc_ts(created_at);
        let (bucket_success, bucket_error, bucket_quota_exhausted) = match entry.outcome {
            OUTCOME_SUCCESS => (1_i64, 0_i64, 0_i64),
            OUTCOME_ERROR => (0_i64, 1_i64, 0_i64),
            OUTCOME_QUOTA_EXHAUSTED => (0_i64, 0_i64, 1_i64),
            _ => (0_i64, 0_i64, 0_i64),
        };

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(entry.key_id)
        .bind(entry.auth_token_id)
        .bind(entry.method.as_str())
        .bind(entry.path)
        .bind(entry.query)
        .bind(status_code)
        .bind(entry.tavily_status_code)
        .bind(entry.error)
        .bind(entry.outcome)
        .bind(entry.request_body)
        .bind(entry.response_body)
        .bind(forwarded_json)
        .bind(dropped_json)
        .bind(created_at)
        .execute(&mut *tx)
        .await?;

        // Daily API-key rollup bucket (bucket_secs=86400, aligned to local midnight).
        sqlx::query(
            r#"
            INSERT INTO api_key_usage_buckets (
                api_key_id,
                bucket_start,
                bucket_secs,
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                updated_at
            ) VALUES (?, ?, 86400, 1, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, bucket_start, bucket_secs)
            DO UPDATE SET
                total_requests = total_requests + 1,
                success_count = success_count + excluded.success_count,
                error_count = error_count + excluded.error_count,
                quota_exhausted_count = quota_exhausted_count + excluded.quota_exhausted_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(entry.key_id)
        .bind(bucket_start)
        .bind(bucket_success)
        .bind(bucket_error)
        .bind(bucket_quota_exhausted)
        .bind(created_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    fn api_key_metrics_from_clause() -> &'static str {
        r#"
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
        "#
    }

    fn api_key_metrics_query(include_quarantine_detail: bool) -> String {
        let quarantine_detail_sql = if include_quarantine_detail {
            "aq.reason_detail AS quarantine_reason_detail,"
        } else {
            "NULL AS quarantine_reason_detail,"
        };
        format!(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                {quarantine_detail_sql}
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            {}
            "#,
            Self::api_key_metrics_from_clause(),
        )
    }

    fn map_api_key_metrics_row(row: sqlx::sqlite::SqliteRow) -> Result<ApiKeyMetrics, sqlx::Error> {
        let id: String = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let group_name: Option<String> = row.try_get("group_name")?;
        let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
        let last_used_at: i64 = row.try_get("last_used_at")?;
        let deleted_at: Option<i64> = row.try_get("deleted_at")?;
        let quota_limit: Option<i64> = row.try_get("quota_limit")?;
        let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
        let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
        let total_requests: i64 = row.try_get("total_requests")?;
        let success_count: i64 = row.try_get("success_count")?;
        let error_count: i64 = row.try_get("error_count")?;
        let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
        let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
        let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
        let quarantine_reason_summary: Option<String> = row.try_get("quarantine_reason_summary")?;
        let quarantine_reason_detail: Option<String> = row.try_get("quarantine_reason_detail")?;
        let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

        Ok(ApiKeyMetrics {
            id,
            status,
            group_name: group_name.and_then(|name| {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            }),
            status_changed_at: status_changed_at.and_then(normalize_timestamp),
            last_used_at: normalize_timestamp(last_used_at),
            deleted_at: deleted_at.and_then(normalize_timestamp),
            quota_limit,
            quota_remaining,
            quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                source,
                reason_code: quarantine_reason_code.unwrap_or_default(),
                reason_summary: quarantine_reason_summary.unwrap_or_default(),
                reason_detail: quarantine_reason_detail.unwrap_or_default(),
                created_at: quarantine_created_at.unwrap_or_default(),
            }),
        })
    }

    fn normalize_api_key_groups(groups: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for group in groups {
            let value = group.trim().to_string();
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    fn normalize_api_key_statuses(statuses: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for status in statuses {
            let value = status.trim().to_ascii_lowercase();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    fn push_api_key_group_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        groups: &'a [String],
    ) {
        if groups.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if group.is_empty() {
                builder.push("(TRIM(COALESCE(ak.group_name, '')) = '')");
            } else {
                builder
                    .push("(TRIM(COALESCE(ak.group_name, '')) = ")
                    .push_bind(group)
                    .push(")");
            }
        }
        builder.push(")");
    }

    fn push_api_key_status_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        statuses: &'a [String],
    ) {
        if statuses.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, status) in statuses.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if status == "quarantined" {
                builder.push("(aq.key_id IS NOT NULL)");
            } else {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status)
                    .push(")");
            }
        }
        builder.push(")");
    }

    async fn fetch_api_key_group_facets(
        &self,
        statuses: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT TRIM(COALESCE(ak.group_name, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_status_filters(&mut builder, statuses);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    async fn fetch_api_key_status_facets(
        &self,
        groups: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT CASE WHEN aq.key_id IS NOT NULL THEN 'quarantined' ELSE ak.status END AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut builder, groups);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    async fn fetch_api_key_metrics(
        &self,
        include_quarantine_detail: bool,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let query = format!(
            "{} ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
            Self::api_key_metrics_query(include_quarantine_detail),
        );
        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    async fn fetch_api_key_metrics_page(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        let requested_page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let groups = Self::normalize_api_key_groups(groups);
        let statuses = Self::normalize_api_key_statuses(statuses);

        let mut count_builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
        count_builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut count_builder, &groups);
        Self::push_api_key_status_filters(&mut count_builder, &statuses);
        let total = count_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;
        let total_pages = ((total + per_page - 1) / per_page).max(1);
        let page = requested_page.min(total_pages);
        let offset = (page - 1) * per_page;

        let mut items_builder = QueryBuilder::<Sqlite>::new(Self::api_key_metrics_query(false));
        Self::push_api_key_group_filters(&mut items_builder, &groups);
        Self::push_api_key_status_filters(&mut items_builder, &statuses);
        items_builder.push(
            " ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(per_page);
        items_builder.push(" OFFSET ").push_bind(offset);
        let items = items_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()?;

        let group_counts = self.fetch_api_key_group_facets(&statuses).await?;
        let status_counts = self.fetch_api_key_status_facets(&groups).await?;

        Ok(PaginatedApiKeyMetrics {
            items,
            total,
            page,
            per_page,
            facets: ApiKeyListFacets {
                groups: group_counts,
                statuses: status_counts,
            },
        })
    }

    async fn fetch_api_key_metric_by_id(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                aq.reason_detail AS quarantine_reason_detail,
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL AND ak.id = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| -> Result<ApiKeyMetrics, sqlx::Error> {
            let id: String = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            let group_name: Option<String> = row.try_get("group_name")?;
            let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
            let last_used_at: i64 = row.try_get("last_used_at")?;
            let deleted_at: Option<i64> = row.try_get("deleted_at")?;
            let quota_limit: Option<i64> = row.try_get("quota_limit")?;
            let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
            let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
            let total_requests: i64 = row.try_get("total_requests")?;
            let success_count: i64 = row.try_get("success_count")?;
            let error_count: i64 = row.try_get("error_count")?;
            let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
            let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
            let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
            let quarantine_reason_summary: Option<String> =
                row.try_get("quarantine_reason_summary")?;
            let quarantine_reason_detail: Option<String> =
                row.try_get("quarantine_reason_detail")?;
            let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

            Ok(ApiKeyMetrics {
                id,
                status,
                group_name: group_name.and_then(|name| {
                    let trimmed = name.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_owned())
                    }
                }),
                status_changed_at: status_changed_at.and_then(normalize_timestamp),
                last_used_at: normalize_timestamp(last_used_at),
                deleted_at: deleted_at.and_then(normalize_timestamp),
                quota_limit,
                quota_remaining,
                quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                    source,
                    reason_code: quarantine_reason_code.unwrap_or_default(),
                    reason_summary: quarantine_reason_summary.unwrap_or_default(),
                    reason_detail: quarantine_reason_detail.unwrap_or_default(),
                    created_at: quarantine_created_at.unwrap_or_default(),
                }),
            })
        })
        .transpose()
        .map_err(ProxyError::from)
    }

    async fn fetch_recent_logs(&self, limit: usize) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;

        let rows = sqlx::query(
            r#"
            SELECT
                id,
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            FROM request_logs
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let records = rows
            .into_iter()
            .map(|row| -> Result<RequestLogRecord, sqlx::Error> {
                let forwarded =
                    parse_header_list(row.try_get::<Option<String>, _>("forwarded_headers")?);
                let dropped =
                    parse_header_list(row.try_get::<Option<String>, _>("dropped_headers")?);
                let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                let response_body: Option<Vec<u8>> = row.try_get("response_body")?;
                Ok(RequestLogRecord {
                    id: row.try_get("id")?,
                    key_id: row.try_get("api_key_id")?,
                    auth_token_id: row.try_get("auth_token_id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    status_code: row.try_get("status_code")?,
                    tavily_status_code: row.try_get("tavily_status_code")?,
                    error_message: row.try_get("error_message")?,
                    result_status: row.try_get("result_status")?,
                    created_at: row.try_get("created_at")?,
                    request_body: request_body.unwrap_or_default(),
                    response_body: response_body.unwrap_or_default(),
                    forwarded_headers: forwarded,
                    dropped_headers: dropped,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    async fn fetch_recent_logs_page(
        &self,
        result_status: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;

        let (rows, total) = if let Some(status) = result_status {
            let total: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*) AS count
                FROM request_logs
                WHERE result_status = ?
                "#,
            )
            .bind(status)
            .fetch_one(&self.pool)
            .await?;

            let rows = sqlx::query(
                r#"
                SELECT
                    id,
                    api_key_id,
                    auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                WHERE result_status = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(status)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

            (rows, total)
        } else {
            let total: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*) AS count
                FROM request_logs
                "#,
            )
            .fetch_one(&self.pool)
            .await?;

            let rows = sqlx::query(
                r#"
                SELECT
                    id,
                    api_key_id,
                    auth_token_id,
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                ORDER BY created_at DESC, id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

            (rows, total)
        };

        let records = rows
            .into_iter()
            .map(|row| -> Result<RequestLogRecord, sqlx::Error> {
                let forwarded =
                    parse_header_list(row.try_get::<Option<String>, _>("forwarded_headers")?);
                let dropped =
                    parse_header_list(row.try_get::<Option<String>, _>("dropped_headers")?);
                let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                let response_body: Option<Vec<u8>> = row.try_get("response_body")?;
                Ok(RequestLogRecord {
                    id: row.try_get("id")?,
                    key_id: row.try_get("api_key_id")?,
                    auth_token_id: row.try_get("auth_token_id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    status_code: row.try_get("status_code")?,
                    tavily_status_code: row.try_get("tavily_status_code")?,
                    error_message: row.try_get("error_message")?,
                    result_status: row.try_get("result_status")?,
                    created_at: row.try_get("created_at")?,
                    request_body: request_body.unwrap_or_default(),
                    response_body: response_body.unwrap_or_default(),
                    forwarded_headers: forwarded,
                    dropped_headers: dropped,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((records, total))
    }

    async fn fetch_api_key_secret(&self, key_id: &str) -> Result<Option<String>, ProxyError> {
        let secret =
            sqlx::query_scalar::<_, String>("SELECT api_key FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(secret)
    }

    async fn update_quota_for_key(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE api_keys
               SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ?
             WHERE id = ?"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = Utc::now().timestamp();
        let threshold = now - older_than_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
            )
            ORDER BY CASE WHEN quota_synced_at IS NULL OR quota_synced_at = 0 THEN 0 ELSE 1 END, quota_synced_at ASC
            "#,
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        let started_at = Utc::now().timestamp();
        let res = sqlx::query(
            r#"INSERT INTO scheduled_jobs (job_type, key_id, status, attempt, started_at)
               VALUES (?, ?, 'running', ?, ?)"#,
        )
        .bind(job_type)
        .bind(key_id)
        .bind(attempt)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE scheduled_jobs SET status = ?, message = ?, finished_at = ? WHERE id = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            r#"SELECT
                    j.id,
                    j.job_type,
                    j.key_id,
                    k.group_name AS key_group,
                    j.status,
                    j.attempt,
                    j.message,
                    j.started_at,
                    j.finished_at
                FROM scheduled_jobs j
                LEFT JOIN api_keys k ON k.id = j.key_id
                ORDER BY j.started_at DESC, j.id DESC
                LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100) as i64;
        let offset = ((page - 1) as i64).saturating_mul(per_page);

        let where_clause = match group {
            "quota" => "WHERE j.job_type = 'quota_sync' OR j.job_type = 'quota_sync/manual'",
            "usage" => "WHERE j.job_type = 'token_usage_rollup'",
            "logs" => "WHERE j.job_type = 'auth_token_logs_gc' OR j.job_type = 'request_logs_gc'",
            _ => "",
        };

        let count_where_clause = match group {
            "quota" => "WHERE job_type = 'quota_sync' OR job_type = 'quota_sync/manual'",
            "usage" => "WHERE job_type = 'token_usage_rollup'",
            "logs" => "WHERE job_type = 'auth_token_logs_gc' OR job_type = 'request_logs_gc'",
            _ => "",
        };

        let count_query = format!("SELECT COUNT(*) FROM scheduled_jobs {}", count_where_clause);
        let total: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(&self.pool)
            .await?;

        let select_query = format!(
            r#"
            SELECT
                j.id,
                j.job_type,
                j.key_id,
                k.group_name AS key_group,
                j.status,
                j.attempt,
                j.message,
                j.started_at,
                j.finished_at
            FROM scheduled_jobs j
            LEFT JOIN api_keys k ON k.id = j.key_id
            {}
            ORDER BY j.started_at DESC, j.id DESC
            LIMIT ? OFFSET ?
            "#,
            where_clause
        );

        let rows = sqlx::query(&select_query)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((items, total))
    }

    async fn get_meta_string(&self, key: &str) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    async fn get_meta_i64(&self, key: &str) -> Result<Option<i64>, ProxyError> {
        let value = self.get_meta_string(key).await?;

        if let Some(v) = value {
            match v.parse::<i64>() {
                Ok(parsed) => Ok(Some(parsed)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    async fn set_meta_string(&self, key: &str, value: &str) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_meta_i64(&self, key: &str, value: i64) -> Result<(), ProxyError> {
        let v = value.to_string();
        self.set_meta_string(key, &v).await
    }

    async fn fetch_summary(&self) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(total_requests), 0) AS total_requests,
                COALESCE(SUM(success_count), 0) AS success_count,
                COALESCE(SUM(error_count), 0) AS error_count,
                COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS active_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS exhausted_keys,
                COALESCE(SUM(CASE WHEN aq.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS quarantined_keys
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .fetch_one(&self.pool)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(last_used_at) FROM api_keys WHERE deleted_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?
        .and_then(normalize_timestamp);

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0) AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0) AS total_quota_remaining
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            quarantined_keys: key_counts_row.try_get("quarantined_keys")?,
            last_activity,
            total_quota_limit: quotas_row.try_get("total_quota_limit")?,
            total_quota_remaining: quotas_row.try_get("total_quota_remaining")?,
        })
    }

    async fn fetch_success_breakdown(
        &self,
        month_since: i64,
        day_since: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN bucket_start >= ? THEN success_count ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN bucket_start >= ? THEN success_count ELSE 0 END), 0) AS daily_success
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .bind(month_since)
        .bind(day_since)
        .fetch_one(&self.pool)
        .await?;

        Ok(SuccessBreakdown {
            monthly_success: row.try_get("monthly_success")?,
            daily_success: row.try_get("daily_success")?,
        })
    }

    async fn fetch_token_success_failure(
        &self,
        token_id: &str,
        month_since: i64,
        day_since: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs
            WHERE token_id = ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_since)
        .bind(OUTCOME_ERROR)
        .bind(day_since)
        .bind(token_id)
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }
}

#[derive(Debug)]
struct ApiKeyLease {
    id: String,
    secret: String,
}

struct AttemptLog<'a> {
    key_id: &'a str,
    auth_token_id: Option<&'a str>,
    method: &'a Method,
    path: &'a str,
    query: Option<&'a str>,
    status: Option<StatusCode>,
    tavily_status_code: Option<i64>,
    error: Option<&'a str>,
    request_body: &'a [u8],
    response_body: &'a [u8],
    outcome: &'a str,
    forwarded_headers: &'a [String],
    dropped_headers: &'a [String],
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
}

/// 透传响应。
#[derive(Debug, Clone)]
pub struct ProxyResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
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
    fn new(
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

    fn effective_window(&self) -> Option<QuotaWindow> {
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

    fn projected_window(&self, delta: i64) -> Option<QuotaWindow> {
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

/// Lightweight verdict for the per-token hourly raw request limiter.
#[derive(Debug, Clone)]
pub struct TokenHourlyRequestVerdict {
    pub allowed: bool,
    pub hourly_used: i64,
    pub hourly_limit: i64,
}

impl TokenHourlyRequestVerdict {
    fn new(hourly_used_raw: i64, hourly_limit: i64) -> Self {
        let hourly_limit = hourly_limit.max(0);
        let hourly_used_raw = hourly_used_raw.max(0);
        let allowed = hourly_limit > 0 && hourly_used_raw <= hourly_limit;
        let hourly_used = std::cmp::min(hourly_used_raw, hourly_limit);
        Self {
            allowed,
            hourly_used,
            hourly_limit,
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

/// 单条请求日志记录的关键信息。
#[derive(Debug, Clone)]
pub struct RequestLogRecord {
    pub id: i64,
    pub key_id: String,
    pub auth_token_id: Option<String>,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub status_code: Option<i64>,
    pub tavily_status_code: Option<i64>,
    pub error_message: Option<String>,
    pub result_status: String,
    pub request_body: Vec<u8>,
    pub response_body: Vec<u8>,
    pub created_at: i64,
    pub forwarded_headers: Vec<String>,
    pub dropped_headers: Vec<String>,
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

fn random_string(alphabet: &[u8], len: usize) -> String {
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
    pub last_activity: Option<i64>,
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

#[derive(Debug, Clone)]
pub struct UserTokenSummary {
    pub token_id: String,
    pub enabled: bool,
    pub note: Option<String>,
    pub last_used_at: Option<i64>,
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
    fn new(key: impl Into<String>, label: impl Into<String>, detail: Option<String>) -> Self {
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
}

/// Per-token log for detail UI
#[derive(Debug, Clone)]
pub struct TokenLogRecord {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub http_status: Option<i64>,
    pub mcp_status: Option<i64>,
    pub business_credits: Option<i64>,
    pub request_kind_key: String,
    pub request_kind_label: String,
    pub request_kind_detail: Option<String>,
    pub result_status: String,
    pub error_message: Option<String>,
    pub created_at: i64,
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

fn map_forward_proxy_validation_error_code(error: &ProxyError) -> String {
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

fn start_of_month(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
}

fn start_of_local_month_utc_ts(now: chrono::DateTime<Local>) -> i64 {
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

fn start_of_next_month(current_month_start: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    let (year, month) = if current_month_start.month() == 12 {
        (current_month_start.year() + 1, 1)
    } else {
        (current_month_start.year(), current_month_start.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .expect("valid start of next month")
}

fn start_of_day(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid start of day")
        .and_utc()
}

fn start_of_local_day_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let naive = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid start of local day");
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::None => {
            // Extremely unlikely at midnight; fall back to current timestamp.
            now.with_timezone(&Utc).timestamp()
        }
    }
}

fn local_day_bucket_start_utc_ts(created_at_utc_ts: i64) -> i64 {
    let Some(utc_dt) = Utc.timestamp_opt(created_at_utc_ts, 0).single() else {
        return 0;
    };
    start_of_local_day_utc_ts(utc_dt.with_timezone(&Local))
}

fn request_logs_retention_threshold_utc_ts(retention_days: i64) -> i64 {
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

fn normalize_timestamp(timestamp: i64) -> Option<i64> {
    if timestamp <= 0 {
        None
    } else {
        Some(timestamp)
    }
}

fn preview_key(key: &str) -> String {
    let shown = min(6, key.len());
    format!("{}…", &key[..shown])
}

fn compose_path(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{}?{}", path, q),
        _ => path.to_owned(),
    }
}

fn log_success(key: &str, method: &Method, path: &str, query: Option<&str>, status: StatusCode) {
    let key_preview = preview_key(key);
    let full_path = compose_path(path, query);
    println!("[{key_preview}] {method} {full_path} -> {status}");
}

fn log_error(key: &str, method: &Method, path: &str, query: Option<&str>, err: &reqwest::Error) {
    let key_preview = preview_key(key);
    let full_path = compose_path(path, query);
    eprintln!("[{key_preview}] {method} {full_path} !! {err}");
}

fn log_proxy_error(key: &str, method: &Method, path: &str, query: Option<&str>, err: &ProxyError) {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageOutcome {
    Success,
    Error,
    QuotaExhausted,
}

fn analyze_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    if !status.is_success() {
        return AttemptAnalysis {
            status: OUTCOME_ERROR,
            tavily_status_code: Some(status.as_u16() as i64),
            key_health_action: classify_quarantine_reason(Some(status.as_u16() as i64), body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
        };
    }

    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return AttemptAnalysis {
                status: OUTCOME_UNKNOWN,
                tavily_status_code: None,
                key_health_action: KeyHealthAction::None,
            };
        }
    };

    let mut any_success = false;
    let mut any_error = false;
    let mut detected_code = None;
    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            // JSON-RPC batch responses return an array of message envelopes. Treat each element
            // as its own message so we can correctly detect success/error and enforce billing.
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    for message in messages {
        if let Some((outcome, code)) = analyze_json_message(&message) {
            if detected_code.is_none() {
                detected_code = code;
            }
            match outcome {
                MessageOutcome::QuotaExhausted => {
                    return AttemptAnalysis {
                        status: OUTCOME_QUOTA_EXHAUSTED,
                        tavily_status_code: code.or(detected_code),
                        key_health_action: KeyHealthAction::MarkExhausted,
                    };
                }
                MessageOutcome::Error => {
                    any_error = true;
                }
                MessageOutcome::Success => any_success = true,
            }
        }
    }

    if any_error {
        return AttemptAnalysis {
            status: OUTCOME_ERROR,
            tavily_status_code: detected_code,
            key_health_action: classify_quarantine_reason(detected_code, body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
        };
    }

    if any_success {
        return AttemptAnalysis {
            status: OUTCOME_SUCCESS,
            tavily_status_code: detected_code,
            key_health_action: KeyHealthAction::None,
        };
    }

    AttemptAnalysis {
        status: OUTCOME_UNKNOWN,
        tavily_status_code: detected_code,
        key_health_action: KeyHealthAction::None,
    }
}

/// Analyze a single Tavily HTTP JSON response (e.g. `/search`) using HTTP status and
/// optional structured `status` field from the body.
pub fn analyze_http_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    let http_code = status.as_u16() as i64;

    let parsed = serde_json::from_slice::<Value>(body).ok();
    let structured = parsed.as_ref().and_then(extract_status_code);
    let structured_outcome = parsed
        .as_ref()
        .and_then(extract_status_text)
        .and_then(classify_status_text);

    let effective = structured.unwrap_or(http_code);
    let mut outcome = if let Some(code) = structured {
        let code_outcome = classify_status_code(code);
        if matches!(code_outcome, MessageOutcome::Success) {
            structured_outcome.unwrap_or(code_outcome)
        } else {
            code_outcome
        }
    } else {
        structured_outcome.unwrap_or_else(|| classify_status_code(effective))
    };

    // If HTTP status itself is an error, never treat the outcome as success.
    if !status.is_success() && matches!(outcome, MessageOutcome::Success) {
        outcome = if effective == 432 {
            MessageOutcome::QuotaExhausted
        } else {
            MessageOutcome::Error
        };
    }

    let (status_str, key_health_action) = match outcome {
        MessageOutcome::Success => (OUTCOME_SUCCESS, KeyHealthAction::None),
        MessageOutcome::Error => (
            OUTCOME_ERROR,
            classify_quarantine_reason(Some(effective), body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
        ),
        MessageOutcome::QuotaExhausted => (OUTCOME_QUOTA_EXHAUSTED, KeyHealthAction::MarkExhausted),
    };

    AttemptAnalysis {
        status: status_str,
        tavily_status_code: Some(effective),
        key_health_action,
    }
}

/// Analyze a Tavily MCP JSON-RPC response (e.g. `/mcp tools/call`) using the same heuristics
/// as the core proxy request logger (supports JSON-RPC envelopes and SSE message streams).
pub fn analyze_mcp_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    analyze_attempt(status, body)
}

/// Best-effort detection of whether a Tavily MCP response contains *any* error.
///
/// This is used by downstream billing code to avoid over-charging when a JSON-RPC batch
/// contains partial failures (e.g. some items succeed but others error/quota-exhaust).
///
/// Conservative behavior: if we cannot confidently parse the response, treat it as "has error"
/// so we never apply the "expected credits" billing fallback on ambiguous payloads.
pub fn mcp_response_has_any_error(body: &[u8]) -> bool {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => return true,
    };

    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    if messages.is_empty() {
        return true;
    }

    for message in messages {
        let Some((outcome, _code)) = analyze_json_message(&message) else {
            return true;
        };
        if outcome != MessageOutcome::Success {
            return true;
        }
    }

    false
}

/// Best-effort detection of whether a Tavily MCP response contains at least one successful item.
pub fn mcp_response_has_any_success(body: &[u8]) -> bool {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => return false,
    };

    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    if messages.is_empty() {
        return false;
    }

    for message in messages {
        if let Some((outcome, _code)) = analyze_json_message(&message)
            && outcome == MessageOutcome::Success
        {
            return true;
        }
    }

    false
}

fn sanitize_headers_inner(
    headers: &HeaderMap,
    upstream: &Url,
    upstream_origin: &str,
) -> SanitizedHeaders {
    let mut sanitized = HeaderMap::new();
    let mut forwarded = Vec::new();
    let mut dropped = Vec::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        if !should_forward_header(name) {
            dropped.push(key);
            continue;
        }
        if let Some(transformed) = transform_header_value(name, value, upstream, upstream_origin) {
            sanitized.insert(name.clone(), transformed);
            forwarded.push(key);
        } else {
            dropped.push(key);
        }
    }
    SanitizedHeaders {
        headers: sanitized,
        forwarded,
        dropped,
    }
}

fn should_forward_header(name: &reqwest::header::HeaderName) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    if BLOCKED_HEADERS.iter().any(|blocked| lower == *blocked) {
        return false;
    }
    if ALLOWED_HEADERS.iter().any(|allowed| lower == *allowed) {
        return true;
    }
    if ALLOWED_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }
    if lower.starts_with("x-") && !lower.starts_with("x-forwarded-") && lower != "x-real-ip" {
        return true;
    }
    false
}

fn transform_header_value(
    name: &reqwest::header::HeaderName,
    value: &HeaderValue,
    upstream: &Url,
    upstream_origin: &str,
) -> Option<HeaderValue> {
    let lower = name.as_str().to_ascii_lowercase();
    match lower.as_str() {
        "origin" => HeaderValue::from_str(upstream_origin).ok(),
        "referer" => match value.to_str() {
            Ok(raw) => {
                if let Ok(mut url) = Url::parse(raw) {
                    url.set_scheme(upstream.scheme()).ok()?;
                    url.set_host(upstream.host_str()).ok()?;
                    if let Some(port) = upstream.port() {
                        url.set_port(Some(port)).ok()?;
                    } else {
                        url.set_port(None).ok()?;
                    }
                    if url.path().is_empty() {
                        url.set_path("/");
                    }
                    HeaderValue::from_str(url.as_str()).ok()
                } else {
                    HeaderValue::from_str(upstream_origin).ok()
                }
            }
            Err(_) => HeaderValue::from_str(upstream_origin).ok(),
        },
        "sec-fetch-site" => Some(HeaderValue::from_static("same-origin")),
        _ => Some(value.clone()),
    }
}

fn origin_from_url(url: &Url) -> String {
    let mut origin = match url.host_str() {
        Some(host) => format!("{}://{}", url.scheme(), host),
        None => url.as_str().to_string(),
    };

    match (url.port(), url.port_or_known_default()) {
        (Some(port), Some(default)) if default != port => {
            origin.push(':');
            origin.push_str(&port.to_string());
        }
        (Some(port), None) => {
            origin.push(':');
            origin.push_str(&port.to_string());
        }
        _ => {}
    }

    origin
}

fn parse_header_list(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn analyze_json_message(value: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    if value.get("error").is_some_and(|v| !v.is_null()) {
        return Some((MessageOutcome::Error, None));
    }

    if let Some(result) = value.get("result") {
        return analyze_result_payload(result);
    }

    None
}

fn analyze_result_payload(result: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    if let Some(outcome) = analyze_structured_content(result) {
        return Some(outcome);
    }

    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        for item in content {
            if let Some(kind) = item.get("type").and_then(|v| v.as_str())
                && kind.eq_ignore_ascii_case("error")
            {
                return Some((MessageOutcome::Error, None));
            }
            if let Some(text) = item.get("text").and_then(|v| v.as_str())
                && let Some(code) = parse_embedded_status(text)
            {
                return Some((classify_status_code(code), Some(code)));
            }
        }
    }

    if result.get("error").is_some_and(|v| !v.is_null()) {
        return Some((MessageOutcome::Error, None));
    }

    if result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some((MessageOutcome::Error, None));
    }

    Some((MessageOutcome::Success, None))
}

fn analyze_structured_content(result: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    let structured = result.get("structuredContent")?;

    if let Some(code) = extract_status_code(structured) {
        let code_outcome = classify_status_code(code);
        if matches!(code_outcome, MessageOutcome::Success)
            && let Some(text_outcome) =
                extract_status_text(structured).and_then(classify_status_text)
        {
            return Some((text_outcome, Some(code)));
        }
        return Some((code_outcome, Some(code)));
    }

    if let Some(text_outcome) = extract_status_text(structured).and_then(classify_status_text) {
        return Some((text_outcome, None));
    }

    if structured
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some((MessageOutcome::Error, None));
    }

    structured
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str())
                    && let Some(code) = parse_embedded_status(text)
                {
                    return Some((classify_status_code(code), Some(code)));
                }
            }
            None
        })
        .or(Some((MessageOutcome::Success, None)))
}

fn extract_status_code(value: &Value) -> Option<i64> {
    if let Some(code) = value.get("status").and_then(|v| v.as_i64()) {
        return Some(code);
    }

    if let Some(detail) = value.get("detail")
        && let Some(code) = detail.get("status").and_then(|v| v.as_i64())
    {
        return Some(code);
    }

    None
}

fn classify_quarantine_reason(status_code: Option<i64>, body: &[u8]) -> Option<QuarantineDecision> {
    if let Some(code) = status_code
        && code != 401
        && code != 403
    {
        return None;
    }

    let raw = String::from_utf8_lossy(body);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let status_label = status_code
        .map(|code| format!("HTTP {code}"))
        .unwrap_or_else(|| "MCP error".to_string());
    let (reason_code, reason_summary) = if normalized.contains("deactivated") {
        (
            "account_deactivated",
            format!("Tavily account deactivated ({status_label})"),
        )
    } else if normalized.contains("revoked") {
        (
            "key_revoked",
            format!("Tavily key revoked ({status_label})"),
        )
    } else if normalized.contains("invalid api key")
        || normalized.contains("invalid_token")
        || normalized.contains("api key is invalid")
    {
        (
            "invalid_api_key",
            format!("Tavily rejected the API key as invalid ({status_label})"),
        )
    } else {
        return None;
    };

    Some(QuarantineDecision {
        reason_code: reason_code.to_string(),
        reason_summary,
        reason_detail: truncate_text(trimmed, QUARANTINE_REASON_DETAIL_MAX_LEN),
    })
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut truncated = input.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
}

fn extract_status_text(value: &Value) -> Option<&str> {
    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        return Some(status);
    }

    if let Some(detail) = value.get("detail")
        && let Some(status) = detail.get("status").and_then(|v| v.as_str())
    {
        return Some(status);
    }

    None
}

fn extract_research_request_id_from_path(path: &str) -> Option<String> {
    let encoded_request_id = path.strip_prefix("/research/")?;
    if encoded_request_id.is_empty() {
        return None;
    }
    urlencoding::decode(encoded_request_id)
        .map(|decoded| decoded.into_owned())
        .ok()
}

fn extract_research_request_id(body: &[u8]) -> Option<String> {
    let parsed = serde_json::from_slice::<Value>(body).ok()?;
    let request_id = parsed
        .get("request_id")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.get("requestId").and_then(|v| v.as_str()))?;
    let trimmed = request_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

fn request_kind_key(protocol: &str, value: &str) -> String {
    format!("{protocol}:{value}")
}

fn request_kind_label(protocol: &str, value: &str) -> String {
    format!("{protocol} | {value}")
}

fn build_api_request_kind_named(key: &str, label: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("api", key),
        request_kind_label("API", label),
        None,
    )
}

fn build_api_request_kind(value: &str) -> TokenRequestKind {
    build_api_request_kind_named(value, value)
}

fn build_mcp_request_kind_named(key: &str, label: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("mcp", key),
        request_kind_label("MCP", label),
        None,
    )
}

fn build_mcp_request_kind(value: &str) -> TokenRequestKind {
    build_mcp_request_kind_named(value, value)
}

fn build_mcp_request_kind_with_detail(
    key: &str,
    label: &str,
    detail: Option<String>,
) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("mcp", key),
        request_kind_label("MCP", label),
        detail,
    )
}

fn raw_mcp_request_kind(path: &str) -> TokenRequestKind {
    build_mcp_request_kind_named(&format!("raw:{path}"), path)
}

fn normalize_tavily_tool_name(tool: &str) -> Option<String> {
    let normalized = tool.trim().to_ascii_lowercase().replace('_', "-");
    let mapped = match normalized.as_str() {
        "tavily-search" => "search",
        "tavily-extract" => "extract",
        "tavily-crawl" => "crawl",
        "tavily-map" => "map",
        "tavily-research" => "research",
        _ => return None,
    };
    Some(mapped.to_string())
}

fn normalize_request_kind_slug(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut normalized = String::with_capacity(trimmed.len());
    let mut previous_was_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
            continue;
        }

        if !previous_was_separator {
            normalized.push('-');
            previous_was_separator = true;
        }
    }

    let slug = normalized.trim_matches('-');
    if slug.is_empty() {
        return None;
    }

    Some(slug.to_string())
}

fn request_kind_label_penalty(label: &str) -> (usize, usize, usize, String) {
    let display = label.split('|').nth(1).unwrap_or(label).trim();
    let underscore_count = display.chars().filter(|ch| *ch == '_').count();
    let dash_count = display.chars().filter(|ch| *ch == '-').count();
    let lowercase_only = usize::from(
        display.chars().any(|ch| ch.is_ascii_alphabetic())
            && !display.chars().any(|ch| ch.is_ascii_uppercase()),
    );
    (
        underscore_count,
        dash_count,
        lowercase_only,
        display.to_string(),
    )
}

fn prefer_request_kind_label(current: &str, candidate: &str) -> bool {
    request_kind_label_penalty(candidate) < request_kind_label_penalty(current)
}

fn classify_mcp_request_kind_from_message(value: &Value) -> Option<TokenRequestKind> {
    let method = value
        .get("method")
        .and_then(|raw| raw.as_str())
        .map(str::trim)
        .filter(|raw| !raw.is_empty())?;

    if matches!(method, "initialize" | "ping" | "tools/list")
        || method.starts_with("resources/")
        || method.starts_with("prompts/")
        || method.starts_with("notifications/")
    {
        return Some(build_mcp_request_kind(method));
    }

    if method == "tools/call" {
        let tool = value
            .get("params")
            .and_then(|params| params.get("name"))
            .and_then(|raw| raw.as_str())
            .map(str::trim)
            .filter(|raw| !raw.is_empty());
        return match tool {
            Some(tool) => match normalize_tavily_tool_name(tool) {
                Some(kind) => Some(build_mcp_request_kind(&kind)),
                None => {
                    let key = normalize_request_kind_slug(tool)
                        .map(|slug| format!("tool:{slug}"))
                        .unwrap_or_else(|| "tools/call".to_string());
                    Some(build_mcp_request_kind_named(&key, tool))
                }
            },
            None => Some(build_mcp_request_kind("tools/call")),
        };
    }

    Some(build_mcp_request_kind(method))
}

fn classify_mcp_request_kind(path: &str, body: Option<&[u8]>) -> TokenRequestKind {
    let Some(body) = body else {
        return raw_mcp_request_kind(path);
    };
    if body.is_empty() {
        return raw_mcp_request_kind(path);
    }

    let parsed = match serde_json::from_slice::<Value>(body) {
        Ok(value) => value,
        Err(_) => return raw_mcp_request_kind(path),
    };

    match parsed {
        Value::Array(items) => {
            let mut kinds: Vec<TokenRequestKind> = items
                .iter()
                .filter_map(classify_mcp_request_kind_from_message)
                .collect();
            if kinds.is_empty() {
                return raw_mcp_request_kind(path);
            }
            let first_key = kinds[0].key.clone();
            if kinds.iter().all(|kind| kind.key == first_key) {
                return kinds.remove(0);
            }
            let mut labels: Vec<String> = Vec::new();
            for kind in kinds {
                if let Some(label) = kind.label.strip_prefix("MCP | ")
                    && !labels.iter().any(|item| item == label)
                {
                    labels.push(label.to_string());
                }
            }
            build_mcp_request_kind_with_detail(
                "batch",
                "batch",
                (!labels.is_empty()).then(|| labels.join(", ")),
            )
        }
        Value::Object(_) => classify_mcp_request_kind_from_message(&parsed)
            .unwrap_or_else(|| raw_mcp_request_kind(path)),
        _ => raw_mcp_request_kind(path),
    }
}

pub fn classify_token_request_kind(path: &str, body: Option<&[u8]>) -> TokenRequestKind {
    match path {
        "/api/tavily/search" => build_api_request_kind("search"),
        "/api/tavily/extract" => build_api_request_kind("extract"),
        "/api/tavily/crawl" => build_api_request_kind("crawl"),
        "/api/tavily/map" => build_api_request_kind("map"),
        "/api/tavily/research" => build_api_request_kind("research"),
        "/api/tavily/usage" => build_api_request_kind("usage"),
        _ if path.starts_with("/api/tavily/research/") => {
            build_api_request_kind_named("research-result", "research result")
        }
        _ if path.starts_with("/mcp") => classify_mcp_request_kind(path, body),
        _ => build_api_request_kind_named(&format!("raw:{path}"), path),
    }
}

fn token_request_kind_fallback_key_sql() -> &'static str {
    r#"
    CASE
        WHEN path = '/api/tavily/search' THEN 'api:search'
        WHEN path = '/api/tavily/extract' THEN 'api:extract'
        WHEN path = '/api/tavily/crawl' THEN 'api:crawl'
        WHEN path = '/api/tavily/map' THEN 'api:map'
        WHEN path = '/api/tavily/research' THEN 'api:research'
        WHEN path = '/api/tavily/usage' THEN 'api:usage'
        WHEN path LIKE '/api/tavily/research/%' THEN 'api:research-result'
        WHEN path LIKE '/mcp%' THEN 'mcp:raw:' || path
        ELSE 'api:raw:' || path
    END
    "#
}

fn token_request_kind_fallback_label_sql() -> &'static str {
    r#"
    CASE
        WHEN path = '/api/tavily/search' THEN 'API | search'
        WHEN path = '/api/tavily/extract' THEN 'API | extract'
        WHEN path = '/api/tavily/crawl' THEN 'API | crawl'
        WHEN path = '/api/tavily/map' THEN 'API | map'
        WHEN path = '/api/tavily/research' THEN 'API | research'
        WHEN path = '/api/tavily/usage' THEN 'API | usage'
        WHEN path LIKE '/api/tavily/research/%' THEN 'API | research result'
        WHEN path LIKE '/mcp%' THEN 'MCP | ' || path
        ELSE 'API | ' || path
    END
    "#
}

fn token_request_kind_needs_fallback_sql() -> &'static str {
    r#"
    request_kind_key IS NULL
    OR TRIM(request_kind_key) = ''
    OR request_kind_label IS NULL
    OR TRIM(request_kind_label) = ''
    OR (
        path LIKE '/mcp/%'
        AND (
            request_kind_key = 'mcp:raw:/mcp'
            OR request_kind_label = 'MCP | /mcp'
        )
    )
    "#
}

fn derive_token_request_kind_fallback(
    _method: &str,
    path: &str,
    _query: Option<&str>,
) -> TokenRequestKind {
    classify_token_request_kind(path, None)
}

fn is_stale_root_mcp_raw_request_kind(path: &str, key: &str, label: &str) -> bool {
    path.starts_with("/mcp/") && (key.trim() == "mcp:raw:/mcp" || label.trim() == "MCP | /mcp")
}

fn finalize_token_request_kind(
    method: &str,
    path: &str,
    query: Option<&str>,
    key: Option<String>,
    label: Option<String>,
    detail: Option<String>,
) -> TokenRequestKind {
    match (
        key.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }),
        label.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }),
    ) {
        (Some(key), Some(label)) if !is_stale_root_mcp_raw_request_kind(path, &key, &label) => {
            TokenRequestKind::new(key, label, detail)
        }
        _ => derive_token_request_kind_fallback(method, path, query),
    }
}

/// Best-effort extraction of Tavily `usage.credits` from an upstream JSON response body.
///
/// - Returns `None` when the body isn't JSON or the field is missing.
/// - Handles nested MCP envelopes by recursively scanning for an object containing `{ "usage": { "credits": ... } }`.
/// - If credits is a float, rounds up to avoid under-charging.
pub fn extract_usage_credits_from_json_bytes(body: &[u8]) -> Option<i64> {
    if let Ok(parsed) = serde_json::from_slice::<Value>(body) {
        return extract_usage_credits_from_value(&parsed);
    }
    extract_usage_credits_from_sse_bytes(body)
}

/// Best-effort extraction of Tavily `usage.credits` from an upstream JSON response body,
/// summing across JSON-RPC batch responses (top-level arrays).
///
/// For non-batch responses, this matches `extract_usage_credits_from_json_bytes()`.
pub fn extract_usage_credits_total_from_json_bytes(body: &[u8]) -> Option<i64> {
    if let Ok(parsed) = serde_json::from_slice::<Value>(body) {
        return extract_usage_credits_total_from_value(&parsed);
    }
    extract_usage_credits_total_from_sse_bytes(body)
}

/// Best-effort extraction of `usage.credits` from an MCP response, keyed by JSON-RPC `id`.
///
/// This is primarily used by the `/mcp` proxy to avoid accidentally charging credits from
/// non-Tavily tool calls in a mixed JSON-RPC batch.
pub fn extract_mcp_usage_credits_by_id_from_bytes(body: &[u8]) -> HashMap<String, i64> {
    let mut messages: Vec<Value> = Vec::new();

    if let Ok(text) = std::str::from_utf8(body) {
        messages = extract_sse_json_messages(text);
        if messages.is_empty()
            && let Ok(value) = serde_json::from_str::<Value>(text)
        {
            match value {
                Value::Array(items) => messages.extend(items),
                other => messages.push(other),
            }
        }
    }

    if messages.is_empty()
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    fn ingest(value: &Value, out: &mut HashMap<String, i64>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    ingest(item, out);
                }
            }
            Value::Object(map) => {
                let Some(id) = map.get("id").filter(|v| !v.is_null()) else {
                    return;
                };
                let Some(credits) = extract_usage_credits_from_value(value) else {
                    return;
                };
                let key = id.to_string();
                out.entry(key)
                    .and_modify(|current| *current = (*current).max(credits))
                    .or_insert(credits);
            }
            _ => {}
        }
    }

    let mut out: HashMap<String, i64> = HashMap::new();
    for message in messages {
        ingest(&message, &mut out);
    }
    out
}

/// Best-effort extraction of whether an MCP response message contains an error, keyed by JSON-RPC `id`.
///
/// Values are `true` when we see any non-success outcome for that id (including quota exhausted).
/// This is used to scope billing fallbacks (like expected credits) to only the billable calls.
pub fn extract_mcp_has_error_by_id_from_bytes(body: &[u8]) -> HashMap<String, bool> {
    let mut messages: Vec<Value> = Vec::new();

    if let Ok(text) = std::str::from_utf8(body) {
        messages = extract_sse_json_messages(text);
        if messages.is_empty()
            && let Ok(value) = serde_json::from_str::<Value>(text)
        {
            match value {
                Value::Array(items) => messages.extend(items),
                other => messages.push(other),
            }
        }
    }

    if messages.is_empty()
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    fn ingest(value: &Value, out: &mut HashMap<String, bool>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    ingest(item, out);
                }
            }
            Value::Object(map) => {
                let Some(id) = map.get("id").filter(|v| !v.is_null()) else {
                    return;
                };

                let is_error = analyze_json_message(value)
                    .map(|(outcome, _code)| outcome != MessageOutcome::Success)
                    .unwrap_or(true);

                let key = id.to_string();
                out.entry(key)
                    .and_modify(|current| *current = *current || is_error)
                    .or_insert(is_error);
            }
            _ => {}
        }
    }

    let mut out: HashMap<String, bool> = HashMap::new();
    for message in messages {
        ingest(&message, &mut out);
    }
    out
}

fn extract_usage_credits_total_from_value(value: &Value) -> Option<i64> {
    match value {
        Value::Array(items) => {
            let mut total = 0i64;
            let mut found = false;
            for item in items {
                if let Some(credits) = extract_usage_credits_from_value(item) {
                    total = total.saturating_add(credits);
                    found = true;
                }
            }
            found.then_some(total)
        }
        other => extract_usage_credits_from_value(other),
    }
}

fn extract_usage_credits_from_value(value: &Value) -> Option<i64> {
    match value {
        Value::Object(map) => {
            if let Some(credits) = map
                .get("usage")
                .and_then(extract_usage_credits_from_usage_value)
            {
                return Some(credits);
            }
            // MCP responses can be wrapped in arbitrary envelopes. Scan all nested values.
            for nested in map.values() {
                if let Some(credits) = extract_usage_credits_from_value(nested) {
                    return Some(credits);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_usage_credits_from_value),
        _ => None,
    }
}

fn extract_usage_credits_from_usage_value(value: &Value) -> Option<i64> {
    let Value::Object(map) = value else {
        return None;
    };

    for key in [
        "credits",
        // Some Tavily responses report fractional usage via an exact field instead of the
        // integer `credits` counter. We round up to avoid under-billing when only the exact
        // field is present.
        "total_credits_exact",
    ] {
        if let Some(credits) = map.get(key).and_then(parse_credits_value) {
            return Some(credits);
        }
    }

    None
}

fn parse_credits_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            if let Some(v) = number.as_i64()
                && v >= 0
            {
                return Some(v);
            }
            number.as_f64().map(|v| v.ceil() as i64).filter(|v| *v >= 0)
        }
        Value::String(raw) => {
            let trimmed = raw.trim();
            if let Ok(v) = trimmed.parse::<i64>()
                && v >= 0
            {
                return Some(v);
            }
            trimmed
                .parse::<f64>()
                .ok()
                .map(|v| v.ceil() as i64)
                .filter(|v| *v >= 0)
        }
        _ => None,
    }
}

fn extract_usage_credits_from_sse_bytes(body: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(body).ok()?;
    let messages = extract_sse_json_messages(text);
    let mut best: Option<i64> = None;
    for message in messages {
        if let Some(credits) = extract_usage_credits_from_value(&message) {
            best = Some(best.map_or(credits, |current| current.max(credits)));
        }
    }
    best
}

fn extract_usage_credits_total_from_sse_bytes(body: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(body).ok()?;
    let messages = extract_sse_json_messages(text);
    if messages.is_empty() {
        return None;
    }

    // SSE streams can contain multiple messages for the same JSON-RPC `id` (e.g. progress updates).
    // To avoid double-charging, we take the maximum observed credits per id and then sum.
    let mut per_id_max: HashMap<String, i64> = HashMap::new();
    let mut found = false;

    for message in messages {
        let Some(credits) = extract_usage_credits_total_from_value(&message) else {
            continue;
        };
        found = true;

        let id_key = match &message {
            Value::Object(map) => map
                .get("id")
                .filter(|v| !v.is_null())
                .map(|v| v.to_string()),
            _ => None,
        }
        .unwrap_or_else(|| "__no_id__".to_string());

        per_id_max
            .entry(id_key)
            .and_modify(|current| *current = (*current).max(credits))
            .or_insert(credits);
    }

    found.then(|| per_id_max.values().copied().sum())
}

fn classify_status_text(status: &str) -> Option<MessageOutcome> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if matches!(
        normalized.as_str(),
        "failed" | "failure" | "error" | "errored" | "cancelled" | "canceled"
    ) {
        return Some(MessageOutcome::Error);
    }

    if matches!(
        normalized.as_str(),
        "pending"
            | "processing"
            | "running"
            | "in_progress"
            | "queued"
            | "completed"
            | "success"
            | "succeeded"
            | "done"
    ) {
        return Some(MessageOutcome::Success);
    }

    None
}

fn classify_status_code(code: i64) -> MessageOutcome {
    if code == 432 {
        MessageOutcome::QuotaExhausted
    } else if code >= 400 {
        MessageOutcome::Error
    } else {
        MessageOutcome::Success
    }
}

fn parse_embedded_status(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| {
            extract_status_code(&value).or_else(|| value.get("status").and_then(|v| v.as_i64()))
        })
}

fn extract_sse_json_messages(text: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if !current.is_empty() {
                if let Ok(value) = serde_json::from_str::<Value>(&current) {
                    messages.push(value);
                }
                current.clear();
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("data:") {
            let content = rest.trim_start();
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(content);
        }
    }

    if !current.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(&current)
    {
        messages.push(value);
    }

    messages
}

/// Recursively replace any `api_key` field values in JSON with a fixed placeholder.
fn redact_api_key_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k.eq_ignore_ascii_case("api_key") {
                    *v = Value::String("***redacted***".to_string());
                } else {
                    redact_api_key_fields(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_api_key_fields(item);
            }
        }
        _ => {}
    }
}

/// Best-effort redaction helper for request/response bodies written to persistent logs.
/// If the payload is valid JSON, any `api_key` fields are replaced; on parse failure,
/// an empty payload is returned to avoid leaking secrets in ambiguous formats.
fn redact_api_key_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(mut value) => {
            redact_api_key_fields(&mut value);
            serde_json::to_vec(&value).unwrap_or_else(|_| Vec::new())
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};
    use tokio::net::TcpListener;

    fn env_lock() -> Arc<tokio::sync::Mutex<()>> {
        static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
        LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    #[test]
    fn parse_hhmm_validates_clock_time() {
        assert_eq!(parse_hhmm("07:00"), Some((7, 0)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(parse_hhmm("7:00"), None);
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("00:60"), None);
        assert_eq!(parse_hhmm(""), None);
        assert_eq!(parse_hhmm("07:00:00"), None);
    }

    #[test]
    fn extract_usage_credits_from_json_bytes_finds_nested_usage_and_rounds_up() {
        let body = br#"{"result":{"structuredContent":{"usage":{"credits":1.2}}}}"#;
        assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
    }

    #[test]
    fn extract_usage_credits_from_json_bytes_parses_string_float_and_rounds_up() {
        let body = br#"{"usage":{"credits":"1.2"}}"#;
        assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
    }

    #[test]
    fn extract_usage_credits_from_json_bytes_supports_total_credits_exact() {
        let body = br#"{"usage":{"total_credits_exact":0.2}}"#;
        assert_eq!(extract_usage_credits_from_json_bytes(body), Some(1));
    }

    #[test]
    fn extract_usage_credits_total_from_json_bytes_sums_total_credits_exact() {
        let body =
            br#"[{"usage":{"total_credits_exact":0.2}},{"usage":{"total_credits_exact":"1.2"}}]"#;
        assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(3));
    }

    #[test]
    fn extract_usage_credits_total_from_json_bytes_sums_across_arrays() {
        let body = br#"[{"result":{"structuredContent":{"usage":{"credits":1}}}},{"result":{"structuredContent":{"usage":{"credits":2.1}}}}]"#;
        assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(4));
    }

    #[test]
    fn extract_usage_credits_from_json_bytes_parses_sse_and_returns_max() {
        let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n";
        assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
    }

    #[test]
    fn extract_usage_credits_total_from_json_bytes_parses_sse_and_sums_by_id() {
        // Duplicate id=1 message should not double count.
        let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n";
        assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(3));
    }

    #[test]
    fn extract_mcp_usage_credits_by_id_from_bytes_tracks_max_per_id() {
        let body = br#"
        [
          {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"usage":{"credits":1}}}},
          {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"usage":{"credits":2}}}},
          {"jsonrpc":"2.0","id":"abc","result":{"structuredContent":{"usage":{"credits":"3"}}}},
          {"jsonrpc":"2.0","id":null,"result":{"structuredContent":{"usage":{"credits":99}}}},
          {"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"status":200}}}
        ]
        "#;

        let credits = extract_mcp_usage_credits_by_id_from_bytes(body);

        let id1 = serde_json::json!(1).to_string();
        let id_abc = serde_json::json!("abc").to_string();
        let id2 = serde_json::json!(2).to_string();

        assert_eq!(credits.get(&id1), Some(&2));
        assert_eq!(credits.get(&id_abc), Some(&3));
        assert_eq!(
            credits.get(&id2),
            None,
            "missing usage should not create a map entry"
        );
        assert!(
            !credits.values().any(|v| *v == 99),
            "null ids should be ignored"
        );
    }

    #[test]
    fn extract_mcp_usage_credits_by_id_from_bytes_parses_sse() {
        let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n";

        let credits = extract_mcp_usage_credits_by_id_from_bytes(body);

        let id1 = serde_json::json!(1).to_string();
        let id2 = serde_json::json!(2).to_string();
        assert_eq!(credits.get(&id1), Some(&2));
        assert_eq!(credits.get(&id2), Some(&1));
    }

    #[test]
    fn extract_mcp_has_error_by_id_from_bytes_marks_error_and_quota_exhausted() {
        let body = br#"
        [
          {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"status":200}}},
          {"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"oops"}},
          {"jsonrpc":"2.0","id":3,"result":{"structuredContent":{"status":432}}}
        ]
        "#;

        let flags = extract_mcp_has_error_by_id_from_bytes(body);
        let id1 = serde_json::json!(1).to_string();
        let id2 = serde_json::json!(2).to_string();
        let id3 = serde_json::json!(3).to_string();

        assert_eq!(flags.get(&id1), Some(&false));
        assert_eq!(flags.get(&id2), Some(&true));
        assert_eq!(flags.get(&id3), Some(&true));
    }

    #[test]
    fn extract_mcp_has_error_by_id_from_bytes_or_accumulates_across_sse() {
        let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"status\":200}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"oops\"}}\n\n";

        let flags = extract_mcp_has_error_by_id_from_bytes(body);
        let id1 = serde_json::json!(1).to_string();
        assert_eq!(flags.get(&id1), Some(&true));
    }

    #[test]
    fn analyze_mcp_attempt_marks_mixed_success_and_error_as_error() {
        let body = br#"[
          {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"status":200}}},
          {"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"oops"}}
        ]"#;

        let analysis = analyze_mcp_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_ERROR);
        assert_eq!(analysis.key_health_action, KeyHealthAction::None);
        assert_eq!(analysis.tavily_status_code, Some(200));
    }

    #[test]
    fn classify_token_request_kind_maps_http_routes_and_raw_paths() {
        assert_eq!(
            classify_token_request_kind("/api/tavily/search", None),
            TokenRequestKind::new("api:search", "API | search", None)
        );
        assert_eq!(
            classify_token_request_kind("/api/tavily/research/req_123", None),
            TokenRequestKind::new("api:research-result", "API | research result", None)
        );
        assert_eq!(
            classify_token_request_kind("/api/custom/raw", None),
            TokenRequestKind::new("api:raw:/api/custom/raw", "API | /api/custom/raw", None)
        );
        assert_eq!(
            classify_token_request_kind("/mcp/sse", None),
            TokenRequestKind::new("mcp:raw:/mcp/sse", "MCP | /mcp/sse", None)
        );
    }

    #[test]
    fn classify_token_request_kind_maps_mcp_control_plane_and_tools() {
        let search_body = br#"{
          "jsonrpc": "2.0",
          "id": 1,
          "method": "tools/call",
          "params": {
            "name": "tavily-search"
          }
        }"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(search_body)),
            TokenRequestKind::new("mcp:search", "MCP | search", None)
        );

        let tool_body = br#"{
          "jsonrpc": "2.0",
          "id": 2,
          "method": "tools/call",
          "params": {
            "name": "Acme Lookup"
          }
        }"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(tool_body)),
            TokenRequestKind::new("mcp:tool:acme-lookup", "MCP | Acme Lookup", None)
        );

        let tool_variant_body = br#"{
          "jsonrpc": "2.0",
          "id": 3,
          "method": "tools/call",
          "params": {
            "name": "  acme_lookup  "
          }
        }"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(tool_variant_body)),
            TokenRequestKind::new("mcp:tool:acme-lookup", "MCP | acme_lookup", None)
        );

        let init_body = br#"{
          "jsonrpc": "2.0",
          "id": 4,
          "method": "initialize"
        }"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(init_body)),
            TokenRequestKind::new("mcp:initialize", "MCP | initialize", None)
        );
    }

    #[test]
    fn classify_token_request_kind_maps_mcp_mixed_batch_to_batch_with_detail() {
        let mixed_batch = br#"[
          {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "tavily-search" }
          },
          {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "tavily-extract" }
          }
        ]"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(mixed_batch)),
            TokenRequestKind::new(
                "mcp:batch",
                "MCP | batch",
                Some("search, extract".to_string())
            )
        );

        let same_batch = br#"[
          {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "tavily-search" }
          },
          {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "tavily_search" }
          }
        ]"#;
        assert_eq!(
            classify_token_request_kind("/mcp", Some(same_batch)),
            TokenRequestKind::new("mcp:search", "MCP | search", None)
        );
    }

    #[test]
    fn request_logs_env_settings_enforce_minimums_and_defaults() {
        let lock = env_lock();
        let _guard = lock.blocking_lock();
        let prev_days = std::env::var("REQUEST_LOGS_RETENTION_DAYS").ok();
        let prev_at = std::env::var("REQUEST_LOGS_GC_AT").ok();

        unsafe {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "3");
        }
        assert_eq!(effective_request_logs_retention_days(), 7);

        unsafe {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "10");
        }
        assert_eq!(effective_request_logs_retention_days(), 10);

        unsafe {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "not-a-number");
            std::env::set_var("REQUEST_LOGS_GC_AT", "23:30");
        }
        assert_eq!(effective_request_logs_retention_days(), 7);
        assert_eq!(effective_request_logs_gc_at(), (23, 30));

        unsafe {
            std::env::set_var("REQUEST_LOGS_GC_AT", "7:00");
        }
        assert_eq!(effective_request_logs_gc_at(), (7, 0));

        unsafe {
            if let Some(v) = prev_days {
                std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", v);
            } else {
                std::env::remove_var("REQUEST_LOGS_RETENTION_DAYS");
            }
            if let Some(v) = prev_at {
                std::env::set_var("REQUEST_LOGS_GC_AT", v);
            } else {
                std::env::remove_var("REQUEST_LOGS_GC_AT");
            }
        }
    }

    #[test]
    fn sanitize_headers_removes_blocked_and_keeps_allowed() {
        let upstream = Url::parse("https://mcp.tavily.com/mcp").unwrap();
        let origin = origin_from_url(&upstream);

        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("1.2.3.4"));
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let sanitized = sanitize_headers_inner(&headers, &upstream, &origin);
        assert!(!sanitized.headers.contains_key("X-Forwarded-For"));
        assert_eq!(
            sanitized.headers.get("Accept").unwrap(),
            &HeaderValue::from_static("application/json")
        );
        assert!(sanitized.dropped.contains(&"x-forwarded-for".to_string()));
        assert!(sanitized.forwarded.contains(&"accept".to_string()));
    }

    #[test]
    fn sanitize_headers_rewrites_origin_and_referer() {
        let upstream = Url::parse("https://mcp.tavily.com:443/mcp").unwrap();
        let origin = origin_from_url(&upstream);

        let mut headers = HeaderMap::new();
        headers.insert("Origin", HeaderValue::from_static("https://proxy.local"));
        headers.insert(
            "Referer",
            HeaderValue::from_static("https://proxy.local/mcp/endpoint"),
        );

        let sanitized = sanitize_headers_inner(&headers, &upstream, &origin);
        assert_eq!(
            sanitized.headers.get("Origin").unwrap(),
            &HeaderValue::from_str(&origin).unwrap()
        );
        assert!(
            sanitized
                .headers
                .get("Referer")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&origin)
        );
        assert!(sanitized.forwarded.contains(&"origin".to_string()));
        assert!(sanitized.forwarded.contains(&"referer".to_string()));
    }

    fn temp_db_path(prefix: &str) -> PathBuf {
        let file = format!("{}-{}.db", prefix, nanoid!(8));
        std::env::temp_dir().join(file)
    }

    #[tokio::test]
    async fn token_log_filters_and_options_use_backfilled_request_kind_columns() {
        let db_path = temp_db_path("token-log-request-kind-backfill");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("request-kind-backfill"))
            .await
            .expect("token created");

        let stale_kind = TokenRequestKind::new("mcp:raw:/mcp", "MCP | /mcp", None);
        proxy
            .record_token_attempt_with_kind(
                &token.id,
                &Method::POST,
                "/mcp/sse",
                None,
                Some(200),
                Some(200),
                false,
                OUTCOME_SUCCESS,
                None,
                &stale_kind,
            )
            .await
            .expect("record stale request kind row");

        drop(proxy);

        let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened");

        let filters = vec!["mcp:raw:/mcp/sse".to_string()];
        let (logs, total) = repaired
            .token_logs_page(&token.id, 1, 20, 0, None, &filters)
            .await
            .expect("query filtered token logs");
        assert_eq!(total, 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].request_kind_key, "mcp:raw:/mcp/sse");
        assert_eq!(logs[0].request_kind_label, "MCP | /mcp/sse");

        let options = repaired
            .token_log_request_kind_options(&token.id, 0, None)
            .await
            .expect("query request kind options");
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].key, "mcp:raw:/mcp/sse");
        assert_eq!(options[0].label, "MCP | /mcp/sse");

        sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET request_kind_key = 'mcp:tool:acme-lookup',
                request_kind_label = 'MCP | Acme Lookup'
            WHERE token_id = ?
            "#,
        )
        .bind(&token.id)
        .execute(&repaired.key_store.pool)
        .await
        .expect("stamp stored request kind");
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, request_kind_key,
                request_kind_label, result_status, error_message, created_at, counts_business_quota
            ) VALUES (?, 'POST', '/mcp', NULL, 200, 200, 'mcp:tool:acme-lookup', 'MCP | acme_lookup', 'success', NULL, ?, 1)
            "#,
        )
        .bind(&token.id)
        .bind(Utc::now().timestamp())
        .execute(&repaired.key_store.pool)
        .await
        .expect("insert mismatched duplicate option row");

        let canonicalized_options = repaired
            .token_log_request_kind_options(&token.id, 0, None)
            .await
            .expect("query canonicalized request kind options");
        assert_eq!(canonicalized_options.len(), 1);
        assert_eq!(canonicalized_options[0].key, "mcp:tool:acme-lookup");
        assert_eq!(canonicalized_options[0].label, "MCP | Acme Lookup");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn add_or_undelete_key_with_status_keeps_tx_clean_after_insert_failure() {
        let db_path = temp_db_path("api-key-upsert-clean-tx-after-failure");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");

        sqlx::query(
            r#"
            CREATE TRIGGER fail_insert_api_key
            BEFORE INSERT ON api_keys
            WHEN NEW.api_key = 'tvly-force-fail'
            BEGIN
                SELECT RAISE(ABORT, 'boom');
            END;
            "#,
        )
        .execute(&pool)
        .await
        .expect("create fail trigger");

        let first_err = proxy
            .add_or_undelete_key_with_status_in_group("tvly-force-fail", Some("team-a"))
            .await
            .expect_err("first key should fail due to trigger");
        assert!(
            first_err.to_string().contains("boom"),
            "error should include trigger message"
        );

        let (second_id, second_status) = proxy
            .add_or_undelete_key_with_status_in_group("tvly-after-failure", Some("team-a"))
            .await
            .expect("second key insert should not be polluted by previous failure");
        assert_eq!(second_status, ApiKeyUpsertStatus::Created);
        assert!(!second_id.is_empty(), "second key id must be present");

        let inserted_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE api_key = 'tvly-after-failure'",
        )
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count inserted keys");
        assert_eq!(
            inserted_count, 1,
            "follow-up insert must succeed even after previous tx failure"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn analyze_http_attempt_treats_2xx_as_success() {
        let body = br#"{"query":"test","results":[]}"#;
        let analysis = analyze_http_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_SUCCESS);
        assert_eq!(analysis.key_health_action, KeyHealthAction::None);
        assert_eq!(analysis.tavily_status_code, Some(200));
    }

    #[test]
    fn analyze_http_attempt_uses_structured_status_and_marks_quota_exhausted() {
        let body = br#"{"status":432,"error":"quota_exhausted"}"#;
        let analysis = analyze_http_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
        assert_eq!(analysis.tavily_status_code, Some(432));
    }

    #[test]
    fn analyze_http_attempt_treats_http_errors_as_error() {
        let body = br#"{"error":"upstream failed"}"#;
        let analysis = analyze_http_attempt(StatusCode::INTERNAL_SERVER_ERROR, body);
        assert_eq!(analysis.status, OUTCOME_ERROR);
        assert_eq!(analysis.key_health_action, KeyHealthAction::None);
        assert_eq!(analysis.tavily_status_code, Some(500));
    }

    #[test]
    fn analyze_http_attempt_treats_failed_status_string_as_error() {
        let body = br#"{"status":"failed"}"#;
        let analysis = analyze_http_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_ERROR);
        assert_eq!(analysis.key_health_action, KeyHealthAction::None);
        assert_eq!(analysis.tavily_status_code, Some(200));
    }

    #[test]
    fn analyze_http_attempt_treats_pending_status_string_as_success() {
        let body = br#"{"status":"pending"}"#;
        let analysis = analyze_http_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_SUCCESS);
        assert_eq!(analysis.key_health_action, KeyHealthAction::None);
        assert_eq!(analysis.tavily_status_code, Some(200));
    }

    #[test]
    fn analyze_http_attempt_prioritizes_structured_status_code_for_quota_exhausted() {
        let body = br#"{"status":432,"detail":{"status":"failed"}}"#;
        let analysis = analyze_http_attempt(StatusCode::OK, body);
        assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
        assert_eq!(analysis.tavily_status_code, Some(432));
    }

    #[test]
    fn analyze_http_attempt_marks_401_deactivated_as_quarantine() {
        let body = br#"{"detail":{"error":"The account associated with this API key has been deactivated."}}"#;
        let analysis = analyze_http_attempt(StatusCode::UNAUTHORIZED, body);
        assert_eq!(analysis.status, OUTCOME_ERROR);
        match analysis.key_health_action {
            KeyHealthAction::Quarantine(decision) => {
                assert_eq!(decision.reason_code, "account_deactivated");
                assert!(decision.reason_summary.contains("HTTP 401"));
            }
            other => panic!("expected quarantine action, got {other:?}"),
        }
        assert_eq!(analysis.tavily_status_code, Some(401));
    }

    #[test]
    fn extract_research_request_id_accepts_snake_and_camel_case() {
        let snake = br#"{"request_id":"req-snake"}"#;
        let camel = br#"{"requestId":"req-camel"}"#;
        assert_eq!(
            extract_research_request_id(snake).as_deref(),
            Some("req-snake")
        );
        assert_eq!(
            extract_research_request_id(camel).as_deref(),
            Some("req-camel")
        );
    }

    #[test]
    fn extract_research_request_id_from_path_decodes_segment() {
        assert_eq!(
            extract_research_request_id_from_path("/research/req%2Fabc").as_deref(),
            Some("req/abc")
        );
    }

    #[test]
    fn redact_api_key_bytes_removes_api_key_value() {
        let input = br#"{"api_key":"th-ABCD-secret","nested":{"api_key":"tvly-secret"}}"#;
        let redacted = redact_api_key_bytes(input);
        let text = String::from_utf8_lossy(&redacted);
        assert!(
            !text.contains("th-ABCD-secret") && !text.contains("tvly-secret"),
            "redacted payload should not contain original secrets"
        );
        assert!(
            text.contains("\"api_key\":\"***redacted***\""),
            "api_key fields should be replaced with placeholder"
        );
    }

    #[tokio::test]
    async fn proxy_http_search_marks_key_exhausted_on_quota_status() {
        let db_path = temp_db_path("http-search-quota");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-quota-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        // Mock Tavily HTTP /search that always returns structured status 432.
        let app = Router::new().route(
            "/search",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": 432,
                        "error": "quota_exhausted",
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{}", addr);

        let headers = HeaderMap::new();
        let options = serde_json::json!({ "query": "test" });

        let (_resp, analysis) = proxy
            .proxy_http_search(
                &usage_base,
                Some("tok1"),
                &Method::POST,
                "/api/tavily/search",
                options,
                &headers,
            )
            .await
            .expect("proxy search succeeded");

        assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
        assert_eq!(analysis.tavily_status_code, Some(432));

        // Verify that the key is marked exhausted in the database.
        let store = proxy.key_store.clone();
        let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
            .bind(expected_api_key)
            .fetch_one(&store.pool)
            .await
            .expect("key row exists");
        assert_eq!(status, STATUS_EXHAUSTED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_http_json_endpoint_injects_bearer_auth_when_enabled() {
        let db_path = temp_db_path("http-json-bearer-enabled");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-bearer-enabled-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let app = Router::new().route(
            "/search",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.to_string();
                    async move {
                        let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                        assert_eq!(api_key, expected_api_key);

                        let authorization = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");
                        let expected_auth = format!("Bearer {}", expected_api_key);
                        assert_eq!(
                            authorization, expected_auth,
                            "upstream authorization should use Tavily key"
                        );
                        assert!(
                            !authorization.starts_with("Bearer th-"),
                            "upstream authorization must not be Hikari token"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{}", addr);
        let headers = HeaderMap::new();
        let options = serde_json::json!({ "query": "test" });

        let _ = proxy
            .proxy_http_json_endpoint(
                &usage_base,
                "/search",
                Some("tok1"),
                &Method::POST,
                "/api/tavily/search",
                options,
                &headers,
                true,
            )
            .await
            .expect("proxy request succeeds");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_http_json_endpoint_quarantines_key_on_401_deactivated() {
        let db_path = temp_db_path("http-json-quarantine-401");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-quarantine-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let app = Router::new().route(
            "/search",
            post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "detail": {
                            "error": "The account associated with this API key has been deactivated."
                        }
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{}", addr);
        let headers = HeaderMap::new();
        let options = serde_json::json!({ "query": "test" });

        let (_resp, analysis) = proxy
            .proxy_http_search(
                &usage_base,
                Some("tok1"),
                &Method::POST,
                "/api/tavily/search",
                options,
                &headers,
            )
            .await
            .expect("proxy search succeeded");

        assert_eq!(analysis.status, OUTCOME_ERROR);
        match analysis.key_health_action {
            KeyHealthAction::Quarantine(ref decision) => {
                assert_eq!(decision.reason_code, "account_deactivated");
            }
            ref other => panic!("expected quarantine action, got {other:?}"),
        }

        let store = proxy.key_store.clone();
        let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
            .bind(expected_api_key)
            .fetch_one(&store.pool)
            .await
            .expect("key row exists");
        assert_eq!(status, STATUS_ACTIVE);

        let quarantine_row = sqlx::query(
            r#"SELECT source, reason_code, cleared_at FROM api_key_quarantines
               WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
        )
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("quarantine row exists");
        let source: String = quarantine_row.try_get("source").expect("source");
        let reason_code: String = quarantine_row.try_get("reason_code").expect("reason_code");
        let cleared_at: Option<i64> = quarantine_row.try_get("cleared_at").expect("cleared_at");
        assert_eq!(source, "/api/tavily/search");
        assert_eq!(reason_code, "account_deactivated");
        assert!(cleared_at.is_none());

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_request_quarantines_key_on_mcp_unauthorized() {
        let db_path = temp_db_path("mcp-quarantine-401");
        let db_str = db_path.to_string_lossy().to_string();

        let app = Router::new().route(
            "/mcp",
            post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "message": "Unauthorized: invalid api key"
                        },
                        "id": 1
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let expected_api_key = "tvly-mcp-quarantine-key";
        let upstream = format!("http://{addr}/mcp");
        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let request = ProxyRequest {
            method: Method::POST,
            path: "/mcp".to_string(),
            query: None,
            headers: HeaderMap::new(),
            body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
            auth_token_id: Some("tok1".to_string()),
        };

        let response = proxy.proxy_request(request).await.expect("proxy response");
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);

        let store = proxy.key_store.clone();
        let quarantine_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM api_key_quarantines
               WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
        )
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("count quarantine rows");
        assert_eq!(quarantine_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_request_quarantines_key_on_mcp_error_body_without_http_status() {
        let db_path = temp_db_path("mcp-quarantine-jsonrpc-error");
        let db_str = db_path.to_string_lossy().to_string();

        let app = Router::new().route(
            "/mcp",
            post(|| async {
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "message": "Unauthorized: invalid api key"
                    },
                    "id": 1
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let expected_api_key = "tvly-mcp-jsonrpc-error-key";
        let upstream = format!("http://{addr}/mcp");
        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let request = ProxyRequest {
            method: Method::POST,
            path: "/mcp".to_string(),
            query: None,
            headers: HeaderMap::new(),
            body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
            auth_token_id: Some("tok1".to_string()),
        };

        let response = proxy.proxy_request(request).await.expect("proxy response");
        assert_eq!(response.status, StatusCode::OK);

        let quarantine_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM api_key_quarantines
               WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
        )
        .bind(expected_api_key)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count quarantine rows");
        assert_eq!(quarantine_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn research_result_keeps_affinity_when_original_key_is_quarantined() {
        let db_path = temp_db_path("research-affinity-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-research-affinity-a".to_string(),
                "tvly-research-affinity-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("fetch keys");
        let (affinity_key_id, _other_key_id) =
            rows.into_iter()
                .fold((None, None), |mut acc, (id, secret)| {
                    if secret == "tvly-research-affinity-a" {
                        acc.0 = Some(id);
                    } else if secret == "tvly-research-affinity-b" {
                        acc.1 = Some(id);
                    }
                    acc
                });
        let affinity_key_id = affinity_key_id.expect("affinity key exists");
        let request_id = "req-affinity-quarantine";

        proxy
            .record_research_request_affinity(request_id, &affinity_key_id, "tok1")
            .await
            .expect("record research affinity");
        proxy
            .key_store
            .quarantine_key_by_id(
                &affinity_key_id,
                "/api/tavily/search",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "deactivated",
            )
            .await
            .expect("quarantine affinity key");

        let err = proxy
            .acquire_key_for_research_request(Some("tok1"), Some(request_id))
            .await
            .expect_err("result retrieval should not fall back to a different key");
        assert!(matches!(err, ProxyError::NoAvailableKeys));

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn classify_quarantine_reason_ignores_generic_unauthorized_errors() {
        let unauthorized = classify_quarantine_reason(Some(401), br#"{"error":"unauthorized"}"#);
        assert!(unauthorized.is_none());

        let forbidden = classify_quarantine_reason(Some(403), br#"{"error":"forbidden"}"#);
        assert!(forbidden.is_none());

        let invalid_payload_key =
            classify_quarantine_reason(None, br#"{"error":"invalid key \"depth\""}"#);
        assert!(invalid_payload_key.is_none());
    }

    #[tokio::test]
    async fn quarantined_keys_are_excluded_until_admin_clears_them() {
        let db_path = temp_db_path("quarantine-acquire");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-quarantine-a".to_string(),
                "tvly-quarantine-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("fetch keys");
        let (first_id, _first_secret) = rows
            .into_iter()
            .find(|(_, secret)| secret == "tvly-quarantine-a")
            .expect("first key exists");

        assert!(
            proxy
                .key_store
                .try_acquire_specific_key(&first_id)
                .await
                .expect("acquire specific before quarantine")
                .is_some()
        );

        proxy
            .key_store
            .quarantine_key_by_id(
                &first_id,
                "/api/tavily/search",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "deactivated",
            )
            .await
            .expect("quarantine key");

        assert!(
            proxy
                .key_store
                .try_acquire_specific_key(&first_id)
                .await
                .expect("acquire specific after quarantine")
                .is_none()
        );

        let summary = proxy.summary().await.expect("summary");
        assert_eq!(summary.active_keys, 1);
        assert_eq!(summary.quarantined_keys, 1);

        proxy
            .clear_key_quarantine_by_id(&first_id)
            .await
            .expect("clear quarantine");

        assert!(
            proxy
                .key_store
                .try_acquire_specific_key(&first_id)
                .await
                .expect("acquire specific after clear")
                .is_some()
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn quarantine_key_by_id_is_safe_under_concurrent_calls() {
        let db_path = temp_db_path("quarantine-concurrent");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-quarantine-race".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("seeded key id");
        let store = proxy.key_store.clone();

        let first = {
            let store = store.clone();
            let key_id = key_id.clone();
            async move {
                store
                    .quarantine_key_by_id(
                        &key_id,
                        "/api/tavily/search",
                        "account_deactivated",
                        "Tavily account deactivated (HTTP 401)",
                        "first detail",
                    )
                    .await
            }
        };
        let second = {
            let store = store.clone();
            let key_id = key_id.clone();
            async move {
                store
                    .quarantine_key_by_id(
                        &key_id,
                        "/api/tavily/search",
                        "account_deactivated",
                        "Tavily account deactivated (HTTP 401)",
                        "second detail",
                    )
                    .await
            }
        };

        let (first_result, second_result) = tokio::join!(first, second);
        first_result.expect("first quarantine succeeds");
        second_result.expect("second quarantine succeeds");

        let quarantine_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count quarantine rows");
        assert_eq!(quarantine_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn quarantine_key_by_id_preserves_original_created_at() {
        let db_path = temp_db_path("quarantine-created-at");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-quarantine-created-at".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("seeded key id");

        proxy
            .key_store
            .quarantine_key_by_id(
                &key_id,
                "/api/tavily/search",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "first detail",
            )
            .await
            .expect("first quarantine");

        let first_created_at: i64 = sqlx::query_scalar(
            "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("first created_at");

        tokio::time::sleep(Duration::from_secs(1)).await;

        proxy
            .key_store
            .quarantine_key_by_id(
                &key_id,
                "/api/tavily/search",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "second detail",
            )
            .await
            .expect("second quarantine");

        let second_created_at: i64 = sqlx::query_scalar(
            "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("second created_at");
        assert_eq!(second_created_at, first_created_at);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn list_keys_pending_quota_sync_skips_quarantined_keys() {
        let db_path = temp_db_path("quota-sync-skip-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-quota-sync-a".to_string(),
                "tvly-quota-sync-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("fetch keys");
        let (quarantined_id, active_id) =
            rows.into_iter()
                .fold((None, None), |mut acc, (id, secret)| {
                    if secret == "tvly-quota-sync-a" {
                        acc.0 = Some(id);
                    } else if secret == "tvly-quota-sync-b" {
                        acc.1 = Some(id);
                    }
                    acc
                });
        let quarantined_id = quarantined_id.expect("quarantined key exists");
        let active_id = active_id.expect("active key exists");

        proxy
            .key_store
            .quarantine_key_by_id(
                &quarantined_id,
                "/api/tavily/usage",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "deactivated",
            )
            .await
            .expect("quarantine key");

        let pending = proxy
            .list_keys_pending_quota_sync(24 * 60 * 60)
            .await
            .expect("list pending keys");
        assert_eq!(pending, vec![active_id]);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn summary_quota_totals_exclude_quarantined_keys() {
        let db_path = temp_db_path("summary-quota-excludes-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-summary-quota-a".to_string(),
                "tvly-summary-quota-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("fetch keys");
        let (quarantined_id, active_id) =
            rows.into_iter()
                .fold((None, None), |mut acc, (id, secret)| {
                    if secret == "tvly-summary-quota-a" {
                        acc.0 = Some(id);
                    } else if secret == "tvly-summary-quota-b" {
                        acc.1 = Some(id);
                    }
                    acc
                });
        let quarantined_id = quarantined_id.expect("quarantined key exists");
        let active_id = active_id.expect("active key exists");

        proxy
            .key_store
            .update_quota_for_key(&quarantined_id, 100, 80, Utc::now().timestamp())
            .await
            .expect("update quarantined key quota");
        proxy
            .key_store
            .update_quota_for_key(&active_id, 50, 40, Utc::now().timestamp())
            .await
            .expect("update active key quota");
        proxy
            .key_store
            .quarantine_key_by_id(
                &quarantined_id,
                "/api/tavily/search",
                "account_deactivated",
                "Tavily account deactivated (HTTP 401)",
                "deactivated",
            )
            .await
            .expect("quarantine key");

        let summary = proxy.summary().await.expect("summary");
        assert_eq!(summary.total_quota_limit, 50);
        assert_eq!(summary.total_quota_remaining, 40);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn research_usage_probe_401_quarantines_key() {
        let db_path = temp_db_path("research-usage-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-research-quarantine-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let app = Router::new().route(
            "/usage",
            get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "invalid api key",
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{}", addr);
        let headers = HeaderMap::new();
        let options = serde_json::json!({ "query": "test research" });

        let err = proxy
            .proxy_http_research_with_usage_diff(
                &usage_base,
                Some("tok1"),
                &Method::POST,
                "/api/tavily/research",
                options,
                &headers,
                false,
            )
            .await
            .expect_err("research should fail when usage probe is unauthorized");

        match err {
            ProxyError::UsageHttp { status, .. } => {
                assert_eq!(status, StatusCode::UNAUTHORIZED);
            }
            other => panic!("expected usage http error, got {other:?}"),
        }

        let quarantine_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM api_key_quarantines
               WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
        )
        .bind(expected_api_key)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count quarantine rows");
        assert_eq!(quarantine_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn sync_key_quota_quarantines_usage_auth_failures() {
        let db_path = temp_db_path("sync-usage-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-sync-quarantine".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("seeded key id");

        let app = Router::new().route(
            "/usage",
            get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "detail": {
                            "error": "The account associated with this API key has been deactivated."
                        }
                    })),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{addr}");
        let err = proxy
            .sync_key_quota(&key_id, &usage_base)
            .await
            .expect_err("sync should fail");
        match err {
            ProxyError::UsageHttp { status, .. } => assert_eq!(status, StatusCode::UNAUTHORIZED),
            other => panic!("expected usage http error, got {other:?}"),
        }

        let quarantine_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count quarantine rows");
        assert_eq!(quarantine_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_http_json_endpoint_does_not_inject_bearer_auth_when_disabled() {
        let db_path = temp_db_path("http-json-bearer-disabled");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-bearer-disabled-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let app = Router::new().route(
            "/search",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.to_string();
                    async move {
                        let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                        assert_eq!(api_key, expected_api_key);
                        assert!(
                            headers.get(axum::http::header::AUTHORIZATION).is_none(),
                            "upstream authorization should be absent when injection is disabled"
                        );
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let usage_base = format!("http://{}", addr);
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Bearer th-client-token"),
        );
        let options = serde_json::json!({ "query": "test" });

        let _ = proxy
            .proxy_http_json_endpoint(
                &usage_base,
                "/search",
                Some("tok1"),
                &Method::POST,
                "/api/tavily/search",
                options,
                &headers,
                false,
            )
            .await
            .expect("proxy request succeeds");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn quota_blocks_after_hourly_limit() {
        let db_path = temp_db_path("quota-test");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy.create_access_token(None).await.expect("token");

        let hourly_limit = effective_token_hourly_limit();

        for _ in 0..hourly_limit {
            let verdict = proxy
                .check_token_quota(&token.id)
                .await
                .expect("quota check ok");
            assert!(verdict.allowed, "should be allowed within limit");
        }

        let verdict = proxy
            .check_token_quota(&token.id)
            .await
            .expect("quota check ok");
        assert!(!verdict.allowed, "expected hourly limit to block");
        assert_eq!(verdict.exceeded_window, Some(QuotaWindow::Hour));

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn quota_window_name_reports_exhausted_when_at_limit() {
        let verdict = TokenQuotaVerdict::new(2, 2, 0, 10, 0, 100);
        assert!(verdict.allowed, "at-limit is not considered exceeded");
        assert_eq!(verdict.window_name(), Some("hour"));
        assert_eq!(verdict.state_key(), "hour");
    }

    #[tokio::test]
    async fn hourly_any_request_limit_blocks_after_threshold() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("any-limit-test");
        let db_str = db_path.to_string_lossy().to_string();

        // Force hourly raw request limit to a small number for this test.
        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "2");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("any-limit"))
            .await
            .expect("create token");

        let hourly_limit = effective_token_hourly_request_limit();

        for _ in 0..hourly_limit {
            let verdict = proxy
                .check_token_hourly_requests(&token.id)
                .await
                .expect("hourly-any check ok");
            assert!(verdict.allowed, "should be allowed within hourly-any limit");
        }

        let verdict = proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("hourly-any check ok");
        assert!(
            !verdict.allowed,
            "expected hourly-any limit to block additional requests"
        );
        assert_eq!(verdict.hourly_limit, hourly_limit);

        unsafe {
            std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        }

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn delete_access_token_soft_deletes_and_hides_from_list() {
        let db_path = temp_db_path("token-delete");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let token = proxy
            .create_access_token(Some("soft-delete-test"))
            .await
            .expect("create token");

        // Sanity check: token is visible before delete.
        let tokens_before = proxy
            .list_access_tokens()
            .await
            .expect("list tokens before delete");
        assert!(
            tokens_before.iter().any(|t| t.id == token.id),
            "token should appear in list before delete"
        );

        // Inspect raw row to confirm it's enabled and not deleted.
        let store = proxy.key_store.clone();
        let (enabled_before, deleted_at_before): (i64, Option<i64>) =
            sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
                .bind(&token.id)
                .fetch_one(&store.pool)
                .await
                .expect("token row exists before delete");
        assert_eq!(enabled_before, 1);
        assert!(
            deleted_at_before.is_none(),
            "deleted_at should be NULL before delete"
        );

        // Perform delete via public API (soft delete).
        proxy
            .delete_access_token(&token.id)
            .await
            .expect("delete token");

        // Row still exists but marked disabled and soft-deleted.
        let (enabled_after, deleted_at_after): (i64, Option<i64>) =
            sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
                .bind(&token.id)
                .fetch_one(&store.pool)
                .await
                .expect("token row exists after delete");
        assert_eq!(enabled_after, 0, "token should be disabled after delete");
        assert!(
            deleted_at_after.is_some(),
            "deleted_at should be set after delete"
        );

        // Token is no longer returned from management listing.
        let tokens_after = proxy
            .list_access_tokens()
            .await
            .expect("list tokens after delete");
        assert!(
            tokens_after.iter().all(|t| t.id != token.id),
            "soft-deleted token should not appear in list"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_counts_only_billable_logs() {
        let db_path = temp_db_path("rollup-billable");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-billable"))
            .await
            .expect("create token");

        let store = proxy.key_store.clone();
        let base_ts = 1_700_000_000i64;

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(base_ts)
        .execute(&store.pool)
        .await
        .expect("insert billable log");

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 0, ?)
            "#,
        )
        .bind(&token.id)
        .bind(base_ts + 10)
        .execute(&store.pool)
        .await
        .expect("insert nonbillable log");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("first rollup");

        let (success, system, external, quota): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT success_count, system_failure_count, external_failure_count, quota_exhausted_count FROM token_usage_stats WHERE token_id = ?",
        )
        .bind(&token.id)
        .fetch_one(&store.pool)
        .await
        .expect("stats row after first rollup");
        assert_eq!(success, 1, "should count only billable logs");
        assert_eq!(
            system + external + quota,
            0,
            "no failure counts expected in this test"
        );

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(base_ts + 20)
        .execute(&store.pool)
        .await
        .expect("insert second billable log");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup");

        let (success_after,): (i64,) =
            sqlx::query_as("SELECT success_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&token.id)
                .fetch_one(&store.pool)
                .await
                .expect("stats row after second rollup");
        assert_eq!(
            success_after, 2,
            "bucket should grow by billable increments"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_is_idempotent_without_new_logs() {
        let db_path = temp_db_path("rollup-idempotent");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-idempotent"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let ts = 1_700_001_000i64;

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(ts)
        .execute(&store.pool)
        .await
        .expect("insert billable log");

        let first = proxy
            .rollup_token_usage_stats()
            .await
            .expect("first rollup");
        assert!(first.0 > 0, "first rollup should process at least one row");

        let after_first = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after first rollup");
        assert_eq!(after_first.total_requests, 1);
        assert_eq!(after_first.success_count, 1);

        let second = proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup");
        assert_eq!(second.0, 0, "second rollup should be a no-op");
        assert!(second.1.is_none(), "second rollup should return no max ts");

        let after_second = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after second rollup");
        assert_eq!(after_second.total_requests, 1);
        assert_eq!(after_second.success_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_processes_same_second_log_once() {
        let db_path = temp_db_path("rollup-same-second");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-same-second"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let ts = 1_700_002_000i64;

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(ts)
        .execute(&store.pool)
        .await
        .expect("insert first log");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("first rollup");

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(ts)
        .execute(&store.pool)
        .await
        .expect("insert second log with same second");

        let second = proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup");
        assert!(second.0 > 0, "second rollup should process the new row");

        let after_second = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after second rollup");
        assert_eq!(after_second.total_requests, 2);
        assert_eq!(after_second.success_count, 2);

        let third = proxy
            .rollup_token_usage_stats()
            .await
            .expect("third rollup");
        assert_eq!(third.0, 0, "third rollup should be a no-op");

        let after_third = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after third rollup");
        assert_eq!(after_third.total_requests, 2);
        assert_eq!(after_third.success_count, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_migrates_legacy_timestamp_cursor() {
        let db_path = temp_db_path("rollup-legacy-cursor");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-legacy-cursor"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let base_ts = 1_700_003_000i64;

        for offset in [0_i64, 10, 20] {
            sqlx::query(
                r#"
                INSERT INTO auth_token_logs (
                    token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
                ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
                "#,
            )
            .bind(&token.id)
            .bind(base_ts + offset)
            .execute(&store.pool)
            .await
            .expect("insert log");
        }

        // Simulate pre-v2 state with only the legacy timestamp cursor present.
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .execute(&store.pool)
            .await
            .expect("delete v2 cursor");
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .bind((base_ts + 10).to_string())
        .execute(&store.pool)
        .await
        .expect("set legacy cursor");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup with migrated cursor");

        let summary = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after migrated rollup");
        assert_eq!(
            summary.total_requests, 2,
            "migration should include boundary-second rows to avoid undercount on legacy_ts"
        );
        assert_eq!(summary.success_count, 2);

        let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
        )
        .fetch_one(&store.pool)
        .await
        .expect("max log id")
        .expect("max log id should exist");
        let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .fetch_one(&store.pool)
            .await
            .expect("v2 cursor exists");
        let cursor_v2 = cursor_v2_raw
            .parse::<i64>()
            .expect("v2 cursor should be numeric");
        assert_eq!(cursor_v2, expected_last_id);

        let second = proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup after migration");
        assert_eq!(second.0, 0, "should not reprocess previous logs");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_migration_handles_out_of_order_timestamps() {
        let db_path = temp_db_path("rollup-legacy-cursor-out-of-order");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-legacy-cursor-out-of-order"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let legacy_ts = 1_700_020_000i64;

        // Insert a newer log first, then an older-timestamp log second to create id/timestamp disorder.
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(legacy_ts + 100)
        .execute(&store.pool)
        .await
        .expect("insert newer log first");

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(legacy_ts - 100)
        .execute(&store.pool)
        .await
        .expect("insert older log second");

        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .execute(&store.pool)
            .await
            .expect("delete v2 cursor");
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .bind(legacy_ts.to_string())
        .execute(&store.pool)
        .await
        .expect("set legacy cursor");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup with out-of-order migration");

        let summary = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after migration");
        assert_eq!(
            summary.total_requests, 1,
            "migration should include all logs newer than legacy_ts even when id/timestamp are out of order"
        );
        assert_eq!(summary.success_count, 1);

        let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
        )
        .fetch_one(&store.pool)
        .await
        .expect("max log id")
        .expect("max log id should exist");
        let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .fetch_one(&store.pool)
            .await
            .expect("v2 cursor exists");
        let cursor_v2 = cursor_v2_raw
            .parse::<i64>()
            .expect("v2 cursor should be numeric");
        assert_eq!(cursor_v2, expected_last_id);

        let second = proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup after migration");
        assert_eq!(second.0, 0, "second rollup should be a no-op");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_migration_includes_same_second_boundary_logs() {
        let db_path = temp_db_path("rollup-legacy-cursor-same-second");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-legacy-cursor-same-second"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let legacy_ts = 1_700_030_000i64;

        for _ in 0..2 {
            sqlx::query(
                r#"
                INSERT INTO auth_token_logs (
                    token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
                ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
                "#,
            )
            .bind(&token.id)
            .bind(legacy_ts)
            .execute(&store.pool)
            .await
            .expect("insert same-second log");
        }

        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .execute(&store.pool)
            .await
            .expect("delete v2 cursor");
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .bind(legacy_ts.to_string())
        .execute(&store.pool)
        .await
        .expect("set legacy cursor");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup with same-second migration boundary");

        let summary = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after migration");
        assert_eq!(
            summary.total_requests, 2,
            "migration must not miss logs at the same second as legacy_ts"
        );
        assert_eq!(summary.success_count, 2);

        let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
        )
        .fetch_one(&store.pool)
        .await
        .expect("max log id")
        .expect("max log id should exist");
        let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
            .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
            .fetch_one(&store.pool)
            .await
            .expect("v2 cursor exists");
        let cursor_v2 = cursor_v2_raw
            .parse::<i64>()
            .expect("v2 cursor should be numeric");
        assert_eq!(cursor_v2, expected_last_id);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn rollup_token_usage_stats_keeps_legacy_timestamp_cursor_monotonic() {
        let db_path = temp_db_path("rollup-legacy-ts-monotonic");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("rollup-legacy-ts-monotonic"))
            .await
            .expect("create token");
        let store = proxy.key_store.clone();
        let newer_ts = 1_700_010_000i64;
        let older_ts = newer_ts - 3_600;

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(newer_ts)
        .execute(&store.pool)
        .await
        .expect("insert newer log first");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("first rollup");

        let first_legacy_ts_raw: String =
            sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
                .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
                .fetch_one(&store.pool)
                .await
                .expect("legacy cursor exists after first rollup");
        let first_legacy_ts = first_legacy_ts_raw
            .parse::<i64>()
            .expect("legacy ts should be numeric");
        assert_eq!(first_legacy_ts, newer_ts);

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(older_ts)
        .execute(&store.pool)
        .await
        .expect("insert older log with newer id");

        let second = proxy
            .rollup_token_usage_stats()
            .await
            .expect("second rollup");
        assert_eq!(
            second.1,
            Some(newer_ts),
            "reported last_rollup_ts should stay aligned with the clamped legacy cursor"
        );

        let second_legacy_ts_raw: String =
            sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
                .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
                .fetch_one(&store.pool)
                .await
                .expect("legacy cursor exists after second rollup");
        let second_legacy_ts = second_legacy_ts_raw
            .parse::<i64>()
            .expect("legacy ts should be numeric");
        assert_eq!(
            second_legacy_ts, newer_ts,
            "legacy ts must not regress when processed logs have older timestamps"
        );

        let summary = proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .expect("summary after second rollup");
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.success_count, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn heal_orphan_auth_tokens_from_logs_creates_soft_deleted_token() {
        let db_path = temp_db_path("heal-orphan");
        let db_str = db_path.to_string_lossy().to_string();

        // Initialize schema.
        let store = KeyStore::new(&db_str).await.expect("keystore created");

        // Insert an auth_token_logs entry for a token id that does not exist in auth_tokens.
        let orphan_token_id = "ZZZZ";
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1234567890)
            "#,
        )
        .bind(orphan_token_id)
        .execute(&store.pool)
        .await
        .expect("insert orphan log");

        // Clear healer meta key so that we can invoke the healer path again for this test.
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_HEAL_ORPHAN_TOKENS_V1)
            .execute(&store.pool)
            .await
            .expect("delete meta gate");

        // Run healer directly.
        store
            .heal_orphan_auth_tokens_from_logs()
            .await
            .expect("heal orphan tokens");

        // Verify that a soft-deleted auth_tokens row was created for the orphan id.
        let (enabled, total_requests, deleted_at): (i64, i64, Option<i64>) = sqlx::query_as(
            "SELECT enabled, total_requests, deleted_at FROM auth_tokens WHERE id = ?",
        )
        .bind(orphan_token_id)
        .fetch_one(&store.pool)
        .await
        .expect("restored token row");

        assert_eq!(enabled, 0, "restored token should be disabled");
        assert_eq!(
            total_requests, 1,
            "restored token should count orphan log entries"
        );
        assert!(
            deleted_at.is_some(),
            "restored token should be marked soft-deleted"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn oauth_login_state_is_single_use() {
        let db_path = temp_db_path("oauth-state-single-use");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let state = proxy
            .create_oauth_login_state("linuxdo", Some("/"), 120)
            .await
            .expect("create oauth state");
        let first = proxy
            .consume_oauth_login_state("linuxdo", &state)
            .await
            .expect("consume oauth state first");
        let second = proxy
            .consume_oauth_login_state("linuxdo", &state)
            .await
            .expect("consume oauth state second");

        assert_eq!(first, Some(Some("/".to_string())));
        assert_eq!(second, None, "oauth state must be single-use");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn oauth_login_state_binding_hash_must_match() {
        let db_path = temp_db_path("oauth-state-binding-hash");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let state = proxy
            .create_oauth_login_state_with_binding("linuxdo", Some("/"), 120, Some("nonce-hash-a"))
            .await
            .expect("create oauth state");

        let wrong_hash = proxy
            .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-b"))
            .await
            .expect("consume oauth state with wrong hash");
        assert_eq!(wrong_hash, None, "wrong hash must not consume oauth state");

        let matched = proxy
            .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
            .await
            .expect("consume oauth state with matching hash");
        assert_eq!(matched, Some(Some("/".to_string())));

        let reused = proxy
            .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
            .await
            .expect("consume oauth state reused");
        assert_eq!(reused, None, "oauth state must remain single-use");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn oauth_login_state_payload_carries_bind_token_id() {
        let db_path = temp_db_path("oauth-state-bind-token-id");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let state = proxy
            .create_oauth_login_state_with_binding_and_token(
                "linuxdo",
                Some("/console"),
                120,
                Some("nonce-hash-a"),
                Some("a1b2"),
            )
            .await
            .expect("create oauth state");

        let payload = proxy
            .consume_oauth_login_state_with_binding_and_token(
                "linuxdo",
                &state,
                Some("nonce-hash-a"),
            )
            .await
            .expect("consume oauth state")
            .expect("payload exists");

        assert_eq!(payload.redirect_to.as_deref(), Some("/console"));
        assert_eq!(payload.bind_token_id.as_deref(), Some("a1b2"));

        let consumed_again = proxy
            .consume_oauth_login_state_with_binding_and_token(
                "linuxdo",
                &state,
                Some("nonce-hash-a"),
            )
            .await
            .expect("consume oauth state second");
        assert!(consumed_again.is_none(), "state must remain single-use");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn ensure_user_token_binding_reuses_existing_binding() {
        let db_path = temp_db_path("user-token-binding-reuse");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "alice-uid".to_string(),
                username: Some("alice".to_string()),
                name: Some("Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");

        let first = proxy
            .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
            .await
            .expect("bind alice first");
        let second = proxy
            .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
            .await
            .expect("bind alice second");

        assert_eq!(
            first.id, second.id,
            "same user should reuse one token binding"
        );
        assert_eq!(first.token, second.token);

        let bob = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "bob-uid".to_string(),
                username: Some("bob".to_string()),
                name: Some("Bob".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert bob");
        let bob_token = proxy
            .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob"))
            .await
            .expect("bind bob");

        assert_ne!(
            first.id, bob_token.id,
            "different users must not share the same token binding"
        );

        let store = proxy.key_store.clone();
        let (alice_bindings,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
                .bind(&alice.user_id)
                .fetch_one(&store.pool)
                .await
                .expect("count alice bindings");
        assert_eq!(
            alice_bindings, 1,
            "alice should have exactly one binding row"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn ensure_user_token_binding_with_preferred_keeps_existing_binding_and_adds_preferred() {
        let db_path = temp_db_path("user-token-binding-preferred-rebind");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "preferred-rebind-user".to_string(),
                username: Some("preferred_rebind".to_string()),
                name: Some("Preferred Rebind".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let original = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_rebind"))
            .await
            .expect("ensure initial binding");
        let mistaken = proxy
            .create_access_token(Some("linuxdo:mistaken"))
            .await
            .expect("create mistaken token");

        let store = proxy.key_store.clone();
        sqlx::query(
            "UPDATE user_token_bindings SET token_id = ?, updated_at = ? WHERE user_id = ?",
        )
        .bind(&mistaken.id)
        .bind(Utc::now().timestamp() - 30)
        .bind(&user.user_id)
        .execute(&store.pool)
        .await
        .expect("simulate mistaken binding");

        let rebound = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:preferred_rebind"),
                Some(&original.id),
            )
            .await
            .expect("rebind preferred token");

        assert_eq!(
            rebound.id, original.id,
            "preferred token should be bound to the user"
        );

        let (binding_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
                .bind(&user.user_id)
                .fetch_one(&store.pool)
                .await
                .expect("count user bindings");
        assert_eq!(
            binding_count, 2,
            "preferred binding should be added without removing existing token"
        );

        let preferred_owner = sqlx::query_scalar::<_, Option<String>>(
            "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
        )
        .bind(&original.id)
        .fetch_optional(&store.pool)
        .await
        .expect("query preferred owner")
        .flatten();
        assert_eq!(
            preferred_owner.as_deref(),
            Some(user.user_id.as_str()),
            "preferred token should belong to the user"
        );

        let mistaken_owner = sqlx::query_scalar::<_, Option<String>>(
            "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
        )
        .bind(&mistaken.id)
        .fetch_optional(&store.pool)
        .await
        .expect("query mistaken token owner")
        .flatten();
        assert_eq!(
            mistaken_owner.as_deref(),
            Some(user.user_id.as_str()),
            "existing token must stay bound to the same user"
        );

        let primary = proxy
            .get_user_token(&user.user_id)
            .await
            .expect("query primary user token");
        match primary {
            UserTokenLookup::Found(secret) => assert_eq!(
                secret.id, original.id,
                "latest preferred binding should be selected as primary token"
            ),
            other => panic!("expected found user token, got {other:?}"),
        }

        let (enabled, deleted_at): (i64, Option<i64>) =
            sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(&mistaken.id)
                .fetch_one(&store.pool)
                .await
                .expect("query mistaken token state");
        assert_eq!(enabled, 1, "mistaken token should remain active");
        assert!(
            deleted_at.is_none(),
            "mistaken token should not be soft-deleted"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_owned_by_other_user()
     {
        let db_path = temp_db_path("user-token-binding-preferred-conflict");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "preferred-conflict-alice".to_string(),
                username: Some("alice_conflict".to_string()),
                name: Some("Alice Conflict".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");
        let bob = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "preferred-conflict-bob".to_string(),
                username: Some("bob_conflict".to_string()),
                name: Some("Bob Conflict".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert bob");
        let bob_token = proxy
            .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob_conflict"))
            .await
            .expect("ensure bob token");

        let alice_result = proxy
            .ensure_user_token_binding_with_preferred(
                &alice.user_id,
                Some("linuxdo:alice_conflict"),
                Some(&bob_token.id),
            )
            .await
            .expect("fallback binding for alice");

        assert_ne!(
            alice_result.id, bob_token.id,
            "preferred token owned by other user must not be rebound"
        );

        let store = proxy.key_store.clone();
        let (owner,): (String,) =
            sqlx::query_as("SELECT user_id FROM user_token_bindings WHERE token_id = ?")
                .bind(&bob_token.id)
                .fetch_one(&store.pool)
                .await
                .expect("query bob token owner");
        assert_eq!(
            owner, bob.user_id,
            "conflicting token owner must remain unchanged"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_unavailable() {
        let db_path = temp_db_path("user-token-binding-preferred-unavailable");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "preferred-unavailable-user".to_string(),
                username: Some("preferred_unavailable".to_string()),
                name: Some("Preferred Unavailable".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let original = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_unavailable"))
            .await
            .expect("ensure original binding");
        let disabled = proxy
            .create_access_token(Some("linuxdo:disabled_preferred"))
            .await
            .expect("create disabled preferred token");
        proxy
            .set_access_token_enabled(&disabled.id, false)
            .await
            .expect("disable preferred token");

        let fallback_disabled = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:preferred_unavailable"),
                Some(&disabled.id),
            )
            .await
            .expect("fallback when preferred disabled");
        assert_eq!(
            fallback_disabled.id, original.id,
            "disabled preferred token should be ignored"
        );

        let deleted = proxy
            .create_access_token(Some("linuxdo:deleted_preferred"))
            .await
            .expect("create deleted preferred token");
        proxy
            .delete_access_token(&deleted.id)
            .await
            .expect("soft delete preferred token");

        let fallback_deleted = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:preferred_unavailable"),
                Some(&deleted.id),
            )
            .await
            .expect("fallback when preferred deleted");
        assert_eq!(
            fallback_deleted.id, original.id,
            "soft-deleted preferred token should be ignored"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn force_user_relogin_migration_revokes_existing_sessions_once() {
        let db_path = temp_db_path("force-user-relogin-v1");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "force-relogin-user".to_string(),
                username: Some("force_relogin".to_string()),
                name: Some("Force Relogin".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create session");

        let store = proxy.key_store.clone();
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_FORCE_USER_RELOGIN_V1)
            .execute(&store.pool)
            .await
            .expect("delete relogin migration meta key");
        drop(proxy);

        let _proxy_after_restart =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy restarted");

        let revoked_at = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT revoked_at FROM user_sessions WHERE token = ? LIMIT 1",
        )
        .bind(&session.token)
        .fetch_optional(&store.pool)
        .await
        .expect("query session after restart")
        .flatten();
        assert!(
            revoked_at.is_some(),
            "existing sessions must be revoked by one-time relogin migration"
        );

        let relogin_migration_mark =
            sqlx::query_scalar::<_, Option<String>>("SELECT value FROM meta WHERE key = ? LIMIT 1")
                .bind(META_KEY_FORCE_USER_RELOGIN_V1)
                .fetch_optional(&store.pool)
                .await
                .expect("query relogin migration mark")
                .flatten();
        assert!(
            relogin_migration_mark.is_some(),
            "relogin migration must record one-time completion mark"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn user_token_bindings_migration_supports_multi_binding_without_backfill() {
        let db_path = temp_db_path("user-token-bindings-multi-binding-migration");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-binding-user".to_string(),
                username: Some("legacy_binding_user".to_string()),
                name: Some("Legacy Binding User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert legacy user");
        let legacy = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_binding_user"))
            .await
            .expect("create legacy binding");
        drop(proxy);

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");

        let legacy_row = sqlx::query_as::<_, (String, String, i64, i64)>(
            "SELECT user_id, token_id, created_at, updated_at FROM user_token_bindings WHERE user_id = ? LIMIT 1",
        )
        .bind(&user.user_id)
        .fetch_one(&pool)
        .await
        .expect("read legacy binding row");
        sqlx::query("DROP TABLE user_token_bindings")
            .execute(&pool)
            .await
            .expect("drop user_token_bindings");
        sqlx::query(
            r#"
            CREATE TABLE user_token_bindings (
                user_id TEXT PRIMARY KEY,
                token_id TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("recreate legacy user_token_bindings");
        sqlx::query(
            "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&legacy_row.0)
        .bind(&legacy_row.1)
        .bind(legacy_row.2)
        .bind(legacy_row.3)
        .execute(&pool)
        .await
        .expect("insert legacy binding row");
        drop(pool);

        let proxy_after_restart =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy restarted");
        let preferred = proxy_after_restart
            .create_access_token(Some("linuxdo:preferred_after_migration"))
            .await
            .expect("create preferred token");
        proxy_after_restart
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:legacy_binding_user"),
                Some(&preferred.id),
            )
            .await
            .expect("bind preferred token after migration");

        let store = proxy_after_restart.key_store.clone();
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
                .bind(&user.user_id)
                .fetch_one(&store.pool)
                .await
                .expect("count user bindings after migration");
        assert_eq!(
            count, 2,
            "migrated schema should allow multiple token bindings per user"
        );

        let owners = sqlx::query_as::<_, (String, String)>(
            "SELECT token_id, user_id FROM user_token_bindings WHERE user_id = ? ORDER BY token_id ASC",
        )
        .bind(&user.user_id)
        .fetch_all(&store.pool)
        .await
        .expect("query owners after migration");
        assert!(
            owners
                .iter()
                .any(|(token_id, owner)| token_id == &legacy.id && owner == &user.user_id),
            "legacy binding should be preserved"
        );
        assert!(
            owners
                .iter()
                .any(|(token_id, owner)| token_id == &preferred.id && owner == &user.user_id),
            "preferred binding should be added"
        );

        let _ = std::fs::remove_file(db_path);
    }
    #[tokio::test]
    async fn get_user_token_returns_unavailable_after_soft_delete() {
        let db_path = temp_db_path("user-token-unavailable");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "charlie-uid".to_string(),
                username: Some("charlie".to_string()),
                name: Some("Charlie".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(0),
                raw_payload_json: None,
            })
            .await
            .expect("upsert charlie");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:charlie"))
            .await
            .expect("bind charlie");

        let before = proxy
            .get_user_token(&user.user_id)
            .await
            .expect("lookup user token before delete");
        assert!(
            matches!(before, UserTokenLookup::Found(_)),
            "token should be available before delete"
        );

        proxy
            .delete_access_token(&token.id)
            .await
            .expect("soft delete token");

        let after = proxy
            .get_user_token(&user.user_id)
            .await
            .expect("lookup user token after delete");
        assert!(
            matches!(after, UserTokenLookup::Unavailable),
            "soft-deleted binding should report unavailable"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn get_user_token_secret_returns_none_when_token_disabled() {
        let db_path = temp_db_path("user-token-secret-disabled");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "disabled-secret-user".to_string(),
                username: Some("disabled_secret_user".to_string()),
                name: Some("Disabled Secret User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(0),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:disabled_secret_user"))
            .await
            .expect("bind token");

        let before = proxy
            .get_user_token_secret(&user.user_id, &token.id)
            .await
            .expect("secret before disable");
        assert!(before.is_some(), "enabled token should expose secret");

        proxy
            .set_access_token_enabled(&token.id, false)
            .await
            .expect("disable token");

        let after = proxy
            .get_user_token_secret(&user.user_id, &token.id)
            .await
            .expect("secret after disable");
        assert!(after.is_none(), "disabled token should not expose secret");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_for_previous_subject_stays_pending_after_token_binding_changes_subject()
     {
        let db_path = temp_db_path("pending-billing-subject-flip");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("pending-billing-subject-flip"))
            .await
            .expect("create token");

        let log_id = proxy
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("simulated pending charge"),
                3,
            )
            .await
            .expect("record pending billing attempt");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "pending-billing-subject-user".to_string(),
                username: Some("pending_billing_subject".to_string()),
                name: Some("Pending Billing Subject".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:pending_billing_subject"),
                Some(&token.id),
            )
            .await
            .expect("bind existing token to user");

        let _guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("reconcile pending billing after subject flip");

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(token_minute_sum, 3);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read billing state");
        assert_eq!(billing_state, BILLING_STATE_CHARGED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_for_previous_account_subject_stays_pending_after_token_becomes_unbound()
     {
        let db_path = temp_db_path("pending-billing-account-to-token-subject-flip");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "pending-billing-account-user".to_string(),
                username: Some("pending_billing_account".to_string()),
                name: Some("Pending Billing Account".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:pending_billing_account"))
            .await
            .expect("bind token");

        let log_id = proxy
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("simulated pending charge"),
                4,
            )
            .await
            .expect("record pending billing attempt");

        sqlx::query("DELETE FROM user_token_bindings WHERE token_id = ?")
            .bind(&token.id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("unbind token");
        proxy.key_store.cache_token_binding(&token.id, None).await;

        let _guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("reconcile pending billing after unbind");

        let account_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read account minute buckets");
        assert_eq!(account_minute_sum, 4);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read billing state");
        assert_eq!(billing_state, BILLING_STATE_CHARGED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn locked_billing_subject_keeps_original_precheck_after_binding_change() {
        let db_path = temp_db_path("locked-billing-subject-precheck");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("locked-billing-subject-precheck"))
            .await
            .expect("create token");
        proxy
            .charge_token_quota(&token.id, 1)
            .await
            .expect("seed token quota before binding change");

        let guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("lock token billing");
        assert_eq!(guard.billing_subject(), format!("token:{}", token.id));

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "locked-billing-subject-precheck-user".to_string(),
                username: Some("locked_billing_precheck".to_string()),
                name: Some("Locked Billing Precheck".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:locked_billing_precheck"),
                Some(&token.id),
            )
            .await
            .expect("bind existing token to user");

        let locked_verdict = proxy
            .peek_token_quota_for_subject(guard.billing_subject())
            .await
            .expect("peek locked subject quota");
        assert_eq!(locked_verdict.hourly_used, 1);

        let current_verdict = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek current token quota");
        assert_eq!(current_verdict.hourly_used, 0);

        drop(guard);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_attempt_for_subject_charges_original_subject_after_binding_change() {
        let db_path = temp_db_path("pending-billing-for-subject");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("pending-billing-for-subject"))
            .await
            .expect("create token");

        let guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("lock token billing");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "pending-billing-for-subject-user".to_string(),
                username: Some("pending_billing_subject_charge".to_string()),
                name: Some("Pending Billing Subject Charge".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:pending_billing_subject_charge"),
                Some(&token.id),
            )
            .await
            .expect("bind existing token to user");

        let log_id = proxy
            .record_pending_billing_attempt_for_subject(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("subject pinned to original token"),
                2,
                guard.billing_subject(),
            )
            .await
            .expect("record pending billing attempt with pinned subject");
        proxy
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle pending billing attempt");

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(token_minute_sum, 2);

        let account_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read account minute buckets");
        assert_eq!(account_minute_sum, 0);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read billing state");
        assert_eq!(billing_state, BILLING_STATE_CHARGED);

        drop(guard);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn lock_token_billing_uses_fresh_binding_after_external_rebind() {
        let db_path = temp_db_path("lock-token-billing-fresh-binding");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy a created");
        let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy b created");
        let token = proxy_a
            .create_access_token(Some("fresh-binding-rebind"))
            .await
            .expect("create token");

        // Warm proxy_a's cache with the old unbound subject first.
        let initial = proxy_a
            .peek_token_quota(&token.id)
            .await
            .expect("peek unbound quota");
        assert_eq!(initial.hourly_used, 0);

        let user = proxy_b
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "fresh-binding-user".to_string(),
                username: Some("fresh_binding_user".to_string()),
                name: Some("Fresh Binding User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy_b
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:fresh_binding_user"),
                Some(&token.id),
            )
            .await
            .expect("bind token on proxy b");

        let guard = proxy_a
            .lock_token_billing(&token.id)
            .await
            .expect("lock token billing after external rebind");
        assert_eq!(guard.billing_subject(), format!("account:{}", user.user_id));

        drop(guard);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_replay_does_not_backfill_previous_month_into_current_token_quota() {
        let db_path = temp_db_path("pending-billing-token-old-month");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("pending-billing-token-old-month"))
            .await
            .expect("create token");

        let current_month_start = start_of_month(Utc::now()).timestamp();
        let previous_month_ts = current_month_start - 60;

        sqlx::query(
            "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
        )
        .bind(&token.id)
        .bind(current_month_start)
        .bind(7_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed current token month");

        let log_id = proxy
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("previous month token charge"),
                3,
            )
            .await
            .expect("record pending token billing");
        sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
            .bind(previous_month_ts)
            .bind(log_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("rewrite token log timestamp");

        proxy
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle previous month token billing");

        let token_month: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
        )
        .bind(&token.id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read token monthly quota");
        assert_eq!(token_month, (current_month_start, 7));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_replay_does_not_backfill_previous_month_into_current_account_quota() {
        let db_path = temp_db_path("pending-billing-account-old-month");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "pending-billing-account-old-month-user".to_string(),
                username: Some("pending_billing_account_old_month".to_string()),
                name: Some("Pending Billing Account Old Month".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(
                &user.user_id,
                Some("linuxdo:pending_billing_account_old_month"),
            )
            .await
            .expect("bind token");

        let current_month_start = start_of_month(Utc::now()).timestamp();
        let previous_month_ts = current_month_start - 60;

        sqlx::query(
            "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
        )
        .bind(&user.user_id)
        .bind(current_month_start)
        .bind(11_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed current account month");

        let log_id = proxy
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("previous month account charge"),
                4,
            )
            .await
            .expect("record pending account billing");
        sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
            .bind(previous_month_ts)
            .bind(log_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("rewrite account log timestamp");

        proxy
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle previous month account billing");

        let account_month: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read account monthly quota");
        assert_eq!(account_month, (current_month_start, 11));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn settle_pending_billing_attempt_is_idempotent_across_instances() {
        let db_path = temp_db_path("pending-billing-idempotent-settle");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy a created");
        let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy b created");
        let token = proxy_a
            .create_access_token(Some("pending-billing-idempotent-settle"))
            .await
            .expect("create token");

        let log_id = proxy_a
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("concurrent settle"),
                5,
            )
            .await
            .expect("record pending billing attempt");

        let settle_a = tokio::spawn(async move {
            proxy_a
                .settle_pending_billing_attempt(log_id)
                .await
                .expect("settle on proxy a");
        });
        let proxy_b_settle = proxy_b.clone();
        let settle_b = tokio::spawn(async move {
            proxy_b_settle
                .settle_pending_billing_attempt(log_id)
                .await
                .expect("settle on proxy b");
        });

        tokio::try_join!(settle_a, settle_b).expect("join settle tasks");

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_b.key_store.pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(token_minute_sum, 5);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy_b.key_store.pool)
                .await
                .expect("read billing state");
        assert_eq!(billing_state, BILLING_STATE_CHARGED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn pending_billing_claim_miss_is_retry_later_until_next_replay() {
        let db_path = temp_db_path("pending-billing-claim-miss-retry");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("pending-billing-claim-miss-retry"))
            .await
            .expect("create token");

        let log_id = proxy
            .record_pending_billing_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                Some("forced claim miss"),
                3,
            )
            .await
            .expect("record pending billing attempt");

        proxy.force_pending_billing_claim_miss_once(log_id).await;

        let outcome = proxy
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("forced claim miss should surface retry-later outcome");
        assert_eq!(outcome, PendingBillingSettleOutcome::RetryLater);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read pending billing state");
        assert_eq!(billing_state, BILLING_STATE_PENDING);

        let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("next billing lock should replay the pending charge before precheck");
        drop(guard);

        let billing_state: String =
            sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
                .bind(log_id)
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read charged billing state");
        assert_eq!(billing_state, BILLING_STATE_CHARGED);

        let verdict = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota after replay");
        assert_eq!(verdict.hourly_used, 3);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn token_billing_lock_serializes_across_proxy_instances() {
        let db_path = temp_db_path("billing-lock-cross-instance");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy a created");
        let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy b created");
        let token = proxy_a
            .create_access_token(Some("billing-lock-cross-instance"))
            .await
            .expect("create token");

        let guard = proxy_a
            .lock_token_billing(&token.id)
            .await
            .expect("acquire first billing lock");

        let token_id = token.id.clone();
        let waiter = tokio::spawn(async move {
            let _guard = proxy_b
                .lock_token_billing(&token_id)
                .await
                .expect("acquire second billing lock");
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !waiter.is_finished(),
            "second proxy instance should wait for the shared billing lock"
        );

        drop(guard);
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("second proxy acquires after release")
            .expect("waiter joins");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn research_usage_lock_serializes_across_proxy_instances() {
        let db_path = temp_db_path("research-usage-cross-instance-lock");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy a created");
        let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy b created");

        let guard = proxy_a
            .lock_research_key_usage("shared-upstream-key")
            .await
            .expect("acquire first research lock");

        let waiter = tokio::spawn(async move {
            let _guard = proxy_b
                .lock_research_key_usage("shared-upstream-key")
                .await
                .expect("acquire second research lock");
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !waiter.is_finished(),
            "second proxy instance should wait for the shared research lock"
        );

        drop(guard);
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("second proxy acquires after release")
            .expect("waiter joins");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn bound_token_quota_checks_use_account_counters() {
        let db_path = temp_db_path("bound-token-account-quota");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "quota-user".to_string(),
                username: Some("quota_user".to_string()),
                name: Some("Quota User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:quota_user"))
            .await
            .expect("bind token");

        proxy
            .charge_token_quota(&token.id, 2)
            .await
            .expect("charge business quota credits");

        let account_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read account minute buckets");
        assert_eq!(
            account_minute_sum, 2,
            "account buckets should count charged credits"
        );

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(
            token_minute_sum, 0,
            "bound token should no longer mutate token-level buckets"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn business_quota_credits_cutover_preserves_existing_counters_once() {
        let db_path = temp_db_path("business-quota-credits-cutover");
        let db_str = db_path.to_string_lossy().to_string();

        // First start: create schema + seed token/user rows for FK constraints.
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let unbound_token = proxy
            .create_access_token(Some("cutover-unbound-token"))
            .await
            .expect("create token");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "cutover-user".to_string(),
                username: Some("cutover".to_string()),
                name: Some("Cutover User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let bound_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:cutover"))
            .await
            .expect("bind token");

        // Simulate an older DB (pre-cutover) by clearing the cutover meta key and writing
        // legacy request-count counters into the buckets/quota tables. The migration should
        // preserve them so deploys do not silently reset active customer quotas.
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("reset cutover meta");

        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let month_start = start_of_month(now).timestamp();

        // Token-scoped legacy counters.
        sqlx::query(
            "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&unbound_token.id)
        .bind(minute_bucket)
        .bind(GRANULARITY_MINUTE)
        .bind(9_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token minute bucket");
        sqlx::query(
            "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&unbound_token.id)
        .bind(hour_bucket)
        .bind(GRANULARITY_HOUR)
        .bind(11_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token hour bucket");
        // Ensure the request limiter bucket is not affected by the cutover reset.
        sqlx::query(
            "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&unbound_token.id)
        .bind(minute_bucket)
        .bind(GRANULARITY_REQUEST_MINUTE)
        .bind(5_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token request_minute bucket");
        sqlx::query(
            "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
        )
        .bind(&unbound_token.id)
        .bind(month_start)
        .bind(13_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token monthly quota");

        // Account-scoped legacy counters (e.g. from old backfill).
        sqlx::query(
            "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&user.user_id)
        .bind(minute_bucket)
        .bind(GRANULARITY_MINUTE)
        .bind(7_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed account minute bucket");
        sqlx::query(
            "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&user.user_id)
        .bind(hour_bucket)
        .bind(GRANULARITY_HOUR)
        .bind(8_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed account hour bucket");
        sqlx::query(
            "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
        )
        .bind(&user.user_id)
        .bind(month_start)
        .bind(14_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed account monthly quota");

        drop(proxy);

        // Second start: cutover migration should preserve legacy counters exactly once.
        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy restarted");

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&unbound_token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(
            token_minute_sum, 9,
            "cutover should preserve token minute buckets"
        );

        let token_hour_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&unbound_token.id)
        .bind(GRANULARITY_HOUR)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read token hour buckets");
        assert_eq!(
            token_hour_sum, 11,
            "cutover should preserve token hour buckets"
        );

        let token_request_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&unbound_token.id)
        .bind(GRANULARITY_REQUEST_MINUTE)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read token request_minute buckets");
        assert_eq!(
            token_request_minute_sum, 5,
            "cutover must not clear raw request limiter buckets"
        );

        let token_monthly_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(&unbound_token.id)
        .fetch_optional(&proxy_after.key_store.pool)
        .await
        .expect("read token monthly quota")
        .unwrap_or(0);
        assert_eq!(
            token_monthly_count, 13,
            "cutover should preserve token monthly quota"
        );

        let account_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read account minute buckets");
        assert_eq!(
            account_minute_sum, 7,
            "cutover should preserve account minute buckets"
        );

        let account_hour_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_HOUR)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read account hour buckets");
        assert_eq!(
            account_hour_sum, 8,
            "cutover should preserve account hour buckets"
        );

        let account_monthly_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_optional(&proxy_after.key_store.pool)
        .await
        .expect("read account monthly quota")
        .unwrap_or(0);
        assert_eq!(
            account_monthly_count, 14,
            "cutover should preserve account monthly quota"
        );

        // Third start: cutover meta key exists, so preserved counters should remain untouched.
        sqlx::query(
            "UPDATE token_usage_buckets SET count = ? WHERE token_id = ? AND bucket_start = ? AND granularity = ?",
        )
        .bind(12_i64)
        .bind(&unbound_token.id)
        .bind(minute_bucket)
        .bind(GRANULARITY_MINUTE)
        .execute(&proxy_after.key_store.pool)
        .await
        .expect("update post-cutover token bucket");
        drop(proxy_after);

        let proxy_third =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy restarted again");

        let token_minute_sum_after: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind(&unbound_token.id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_third.key_store.pool)
        .await
        .expect("read token minute buckets after third start");
        assert_eq!(
            token_minute_sum_after, 12,
            "cutover migration must not rerun after meta is set"
        );

        // Silence unused warning for the bound token variable; it exists only for FK seeding.
        assert!(!bound_token.id.is_empty());

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn account_quota_backfill_is_idempotent() {
        let db_path = temp_db_path("account-backfill-idempotent");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "backfill-user".to_string(),
                username: Some("backfill".to_string()),
                name: Some("Backfill User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:backfill"))
            .await
            .expect("bind token");

        let month_start = start_of_month(Utc::now()).timestamp();
        sqlx::query(
            "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&token.id)
        .bind(month_start)
        .bind(GRANULARITY_MINUTE)
        .bind(3_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token minute bucket");
        sqlx::query(
            "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
        )
        .bind(&token.id)
        .bind(month_start)
        .bind(GRANULARITY_HOUR)
        .bind(5_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token hour bucket");
        sqlx::query(
            "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)\n             ON CONFLICT(token_id) DO UPDATE SET month_start = excluded.month_start, month_count = excluded.month_count",
        )
        .bind(&token.id)
        .bind(month_start)
        .bind(7_i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed token monthly quota");

        sqlx::query("DELETE FROM account_usage_buckets")
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear account buckets");
        sqlx::query("DELETE FROM account_monthly_quota")
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear account monthly");
        sqlx::query("DELETE FROM account_quota_limits")
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear account limits");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("reset backfill meta");

        drop(proxy);

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened for first backfill");

        let first_account_minute: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read account minute after first backfill");
        let first_account_hour: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_HOUR)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read account hour after first backfill");
        let first_month_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read account month after first backfill");

        assert_eq!(first_account_minute, 3);
        assert_eq!(first_account_hour, 5);
        assert_eq!(first_month_count, 7);

        drop(proxy_after);

        let proxy_again =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened for idempotent check");
        let second_account_minute: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy_again.key_store.pool)
        .await
        .expect("read account minute after second init");
        let second_account_hour: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_HOUR)
        .fetch_one(&proxy_again.key_store.pool)
        .await
        .expect("read account hour after second init");
        let second_month_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_again.key_store.pool)
        .await
        .expect("read account month after second init");

        assert_eq!(second_account_minute, first_account_minute);
        assert_eq!(second_account_hour, first_account_hour);
        assert_eq!(second_month_count, first_month_count);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn account_quota_limits_sync_with_env_defaults_on_restart() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("account-limit-sync");
        let db_str = db_path.to_string_lossy().to_string();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "limit-sync-user".to_string(),
                username: Some("limit_sync_user".to_string()),
                name: Some("Limit Sync User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_sync_user"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .expect("seed account quota row");

        let seeded_limits: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read seeded limits");
        assert_eq!(seeded_limits, (0, 0, 0, 0));

        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 11,
                   hourly_limit = 12,
                   daily_limit = 13,
                   monthly_limit = 14,
                   inherits_defaults = 1
               WHERE user_id = ?"#,
        )
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed legacy default-following row");

        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        let second_limits: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read second limits");
        assert_eq!(second_limits, (21, 22, 23, 24));

        unsafe {
            std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
            std::env::remove_var("TOKEN_HOURLY_LIMIT");
            std::env::remove_var("TOKEN_DAILY_LIMIT");
            std::env::remove_var("TOKEN_MONTHLY_LIMIT");
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn legacy_current_default_account_quota_limits_keep_following_defaults_after_reclassification()
     {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("account-limit-legacy-current-default");
        let db_str = db_path.to_string_lossy().to_string();
        let env_keys = [
            "TOKEN_HOURLY_REQUEST_LIMIT",
            "TOKEN_HOURLY_LIMIT",
            "TOKEN_DAILY_LIMIT",
            "TOKEN_MONTHLY_LIMIT",
        ];
        let previous: Vec<Option<String>> =
            env_keys.iter().map(|key| std::env::var(key).ok()).collect();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-current-default-user".to_string(),
                username: Some("legacy_current_default_user".to_string()),
                name: Some("Legacy Current Default User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_current_default_user"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .expect("seed account quota row");
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 11,
                   hourly_limit = 12,
                   daily_limit = 13,
                   monthly_limit = 14,
                   inherits_defaults = 1
               WHERE user_id = ?"#,
        )
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed legacy current default row");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear inherits defaults backfill marker");

        drop(proxy);

        let proxy_after_backfill =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened for backfill");
        let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after_backfill.key_store.pool)
        .await
        .expect("read reclassified default limits");
        assert_eq!(first_limits, (11, 12, 13, 14, 1));

        drop(proxy_after_backfill);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after_sync =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened for sync");
        let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after_sync.key_store.pool)
        .await
        .expect("read synced default limits");
        assert_eq!(second_limits, (21, 22, 23, 24, 1));

        unsafe {
            for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
                if let Some(value) = old_value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn shared_legacy_noncurrent_tuple_is_left_custom_during_reclassification() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("account-limit-legacy-shared-noncurrent");
        let db_str = db_path.to_string_lossy().to_string();
        let env_keys = [
            "TOKEN_HOURLY_REQUEST_LIMIT",
            "TOKEN_HOURLY_LIMIT",
            "TOKEN_DAILY_LIMIT",
            "TOKEN_MONTHLY_LIMIT",
        ];
        let previous: Vec<Option<String>> =
            env_keys.iter().map(|key| std::env::var(key).ok()).collect();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let alpha = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-shared-alpha".to_string(),
                username: Some("legacy_shared_alpha".to_string()),
                name: Some("Legacy Shared Alpha".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alpha");
        let beta = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-shared-beta".to_string(),
                username: Some("legacy_shared_beta".to_string()),
                name: Some("Legacy Shared Beta".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert beta");
        let custom_user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-shared-custom".to_string(),
                username: Some("legacy_shared_custom".to_string()),
                name: Some("Legacy Shared Custom".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(3),
                raw_payload_json: None,
            })
            .await
            .expect("upsert custom user");
        for user in [&alpha, &beta, &custom_user] {
            proxy
                .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_shared"))
                .await
                .expect("bind token");
            proxy
                .user_dashboard_summary(&user.user_id)
                .await
                .expect("seed account quota row");
        }
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 11,
                   hourly_limit = 12,
                   daily_limit = 13,
                   monthly_limit = 14,
                   inherits_defaults = 1,
                   updated_at = created_at + 5
               WHERE user_id IN (?, ?)"#,
        )
        .bind(&alpha.user_id)
        .bind(&beta.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate shared non-current tuple rows");
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 101,
                   hourly_limit = 102,
                   daily_limit = 103,
                   monthly_limit = 104,
                   inherits_defaults = 1,
                   updated_at = created_at
               WHERE user_id = ?"#,
        )
        .bind(&custom_user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate legacy custom row");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear inherits defaults backfill marker");

        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        for user_id in [&alpha.user_id, &beta.user_id] {
            let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
                "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
            )
            .bind(user_id)
            .fetch_one(&proxy_after.key_store.pool)
            .await
            .expect("read shared tuple limits");
            assert_eq!(limits, (11, 12, 13, 14, 0));
        }
        let custom_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&custom_user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read shared custom limits");
        assert_eq!(custom_limits, (101, 102, 103, 104, 0));

        unsafe {
            for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
                if let Some(value) = old_value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn build_account_quota_resolution_clamps_negative_tag_totals_to_zero() {
        let base = AccountQuotaLimits {
            hourly_any_limit: 10,
            hourly_limit: 20,
            daily_limit: 30,
            monthly_limit: 40,
            inherits_defaults: false,
        };
        let resolution = build_account_quota_resolution(
            base.clone(),
            vec![UserTagBindingRecord {
                source: USER_TAG_SOURCE_MANUAL.to_string(),
                tag: UserTagRecord {
                    id: "custom-tag".to_string(),
                    name: "custom_tag".to_string(),
                    display_name: "Custom Tag".to_string(),
                    icon: Some("sparkles".to_string()),
                    system_key: None,
                    effect_kind: USER_TAG_EFFECT_QUOTA_DELTA.to_string(),
                    hourly_any_delta: -100,
                    hourly_delta: -200,
                    daily_delta: -300,
                    monthly_delta: -400,
                    user_count: 1,
                },
            }],
        );

        assert_eq!(resolution.base.hourly_any_limit, 10);
        assert_eq!(resolution.effective.hourly_any_limit, 0);
        assert_eq!(resolution.effective.hourly_limit, 0);
        assert_eq!(resolution.effective.daily_limit, 0);
        assert_eq!(resolution.effective.monthly_limit, 0);
        assert_eq!(resolution.breakdown.len(), 3);
        let effective_row = resolution
            .breakdown
            .iter()
            .find(|entry| entry.kind == "effective")
            .expect("effective row present");
        assert_eq!(effective_row.effect_kind, "effective");
        assert_eq!(effective_row.hourly_any_delta, 0);
        assert_eq!(effective_row.hourly_delta, 0);
        assert_eq!(effective_row.daily_delta, 0);
        assert_eq!(effective_row.monthly_delta, 0);
    }

    #[tokio::test]
    async fn new_account_without_tags_defaults_to_zero_base_and_effective_quota() {
        let db_path = temp_db_path("new-account-zero-base");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "github".to_string(),
                provider_user_id: "new-zero-base-user".to_string(),
                username: Some("new_zero_base_user".to_string()),
                name: Some("New Zero Base User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        let resolution = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolve account quota");

        assert_eq!(resolution.base.hourly_any_limit, 0);
        assert_eq!(resolution.base.hourly_limit, 0);
        assert_eq!(resolution.base.daily_limit, 0);
        assert_eq!(resolution.base.monthly_limit, 0);
        assert!(!resolution.base.inherits_defaults);
        assert_eq!(resolution.effective.hourly_any_limit, 0);
        assert_eq!(resolution.effective.hourly_limit, 0);
        assert_eq!(resolution.effective.daily_limit, 0);
        assert_eq!(resolution.effective.monthly_limit, 0);

        let persisted: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read persisted zero base");
        assert_eq!(persisted, (0, 0, 0, 0, 0));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn new_linuxdo_account_effective_quota_comes_only_from_tags() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("new-linuxdo-tag-only-quota");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "new-linuxdo-tag-only-user".to_string(),
                username: Some("new_linuxdo_tag_only_user".to_string()),
                name: Some("New LinuxDo Tag Only User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        let resolution = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolve account quota");
        let tag_only_limits = AccountQuotaLimits::legacy_defaults();

        assert_eq!(resolution.base.hourly_any_limit, 0);
        assert_eq!(resolution.base.hourly_limit, 0);
        assert_eq!(resolution.base.daily_limit, 0);
        assert_eq!(resolution.base.monthly_limit, 0);
        assert_eq!(
            resolution.effective.hourly_any_limit,
            tag_only_limits.hourly_any_limit
        );
        assert_eq!(
            resolution.effective.hourly_limit,
            tag_only_limits.hourly_limit
        );
        assert_eq!(
            resolution.effective.daily_limit,
            tag_only_limits.daily_limit
        );
        assert_eq!(
            resolution.effective.monthly_limit,
            tag_only_limits.monthly_limit
        );
        assert!(
            resolution
                .tags
                .iter()
                .any(|binding| { binding.tag.system_key.as_deref() == Some("linuxdo_l2") })
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn historical_account_without_quota_row_keeps_legacy_defaults_on_first_resolution() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("historical-account-missing-quota-row");
        let db_str = db_path.to_string_lossy().to_string();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "github".to_string(),
                provider_user_id: "historical-missing-quota-row".to_string(),
                username: Some("historical_missing_quota_row".to_string()),
                name: Some("Historical Missing Quota Row".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        sqlx::query("DELETE FROM account_quota_limits WHERE user_id = ?")
            .bind(&user.user_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("delete quota row");
        sqlx::query("UPDATE users SET created_at = ?, updated_at = ? WHERE id = ?")
            .bind(100_i64)
            .bind(100_i64)
            .bind(&user.user_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("mark user historical");
        proxy
            .key_store
            .set_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1, 200)
            .await
            .expect("set zero-base cutover after user creation");

        let resolution = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolve historical account quota");
        let expected = AccountQuotaLimits::legacy_defaults();

        assert_eq!(resolution.base.hourly_any_limit, expected.hourly_any_limit);
        assert_eq!(resolution.base.hourly_limit, expected.hourly_limit);
        assert_eq!(resolution.base.daily_limit, expected.daily_limit);
        assert_eq!(resolution.base.monthly_limit, expected.monthly_limit);
        assert!(resolution.base.inherits_defaults);

        unsafe {
            std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
            std::env::remove_var("TOKEN_HOURLY_LIMIT");
            std::env::remove_var("TOKEN_DAILY_LIMIT");
            std::env::remove_var("TOKEN_MONTHLY_LIMIT");
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn manual_account_quota_matching_legacy_defaults_stays_custom_on_restart() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("manual-account-quota-matching-legacy-defaults");
        let db_str = db_path.to_string_lossy().to_string();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "github".to_string(),
                provider_user_id: "manual-legacy-default-tuple".to_string(),
                username: Some("manual_legacy_default_tuple".to_string()),
                name: Some("Manual Legacy Default Tuple".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        let updated = proxy
            .key_store
            .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
            .await
            .expect("update account quota");
        assert!(updated);

        let first_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read first row");
        assert_eq!(first_row, (11, 12, 13, 14, 0));

        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        let second_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read second row");
        assert_eq!(second_row, (11, 12, 13, 14, 0));

        unsafe {
            std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
            std::env::remove_var("TOKEN_HOURLY_LIMIT");
            std::env::remove_var("TOKEN_DAILY_LIMIT");
            std::env::remove_var("TOKEN_MONTHLY_LIMIT");
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn legacy_default_following_account_keeps_inherits_defaults_on_noop_save() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("legacy-default-following-noop-save");
        let db_str = db_path.to_string_lossy().to_string();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "github".to_string(),
                provider_user_id: "legacy-default-following-noop-save".to_string(),
                username: Some("legacy_default_following_noop_save".to_string()),
                name: Some("Legacy Default Following Noop Save".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 11,
                   hourly_limit = 12,
                   daily_limit = 13,
                   monthly_limit = 14,
                   inherits_defaults = 1
               WHERE user_id = ?"#,
        )
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed legacy default-following row");

        let updated = proxy
            .key_store
            .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
            .await
            .expect("update account quota");
        assert!(updated);

        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read row after noop save");
        assert_eq!(row, (11, 12, 13, 14, 1));

        unsafe {
            std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
            std::env::remove_var("TOKEN_HOURLY_LIMIT");
            std::env::remove_var("TOKEN_DAILY_LIMIT");
            std::env::remove_var("TOKEN_MONTHLY_LIMIT");
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn account_quota_resolution_cache_invalidates_on_binding_and_tag_updates() {
        let db_path = temp_db_path("account-quota-resolution-cache");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "github".to_string(),
                provider_user_id: "quota-cache-user".to_string(),
                username: Some("quota_cache_user".to_string()),
                name: Some("Quota Cache User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let defaults = AccountQuotaLimits::zero_base();

        let initial = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("initial resolution");
        assert_eq!(
            initial.effective.hourly_any_limit,
            defaults.hourly_any_limit
        );
        assert_eq!(initial.effective.hourly_limit, defaults.hourly_limit);

        let tag = proxy
            .create_user_tag(
                "quota_cache_boost",
                "Quota Cache Boost",
                Some("sparkles"),
                USER_TAG_EFFECT_QUOTA_DELTA,
                7,
                8,
                9,
                10,
            )
            .await
            .expect("create custom tag");
        proxy
            .bind_user_tag_to_user(&user.user_id, &tag.id)
            .await
            .expect("bind user tag");

        let after_bind = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolution after bind");
        assert_eq!(
            after_bind.effective.hourly_any_limit,
            defaults.hourly_any_limit + 7
        );
        assert_eq!(after_bind.effective.hourly_limit, defaults.hourly_limit + 8);

        proxy
            .update_user_tag(
                &tag.id,
                "quota_cache_boost",
                "Quota Cache Boost",
                Some("sparkles"),
                USER_TAG_EFFECT_QUOTA_DELTA,
                11,
                12,
                13,
                14,
            )
            .await
            .expect("update user tag")
            .expect("updated user tag");

        let after_update = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolution after update");
        assert_eq!(
            after_update.effective.hourly_any_limit,
            defaults.hourly_any_limit + 11
        );
        assert_eq!(
            after_update.effective.hourly_limit,
            defaults.hourly_limit + 12
        );
        assert_eq!(
            after_update.effective.daily_limit,
            defaults.daily_limit + 13
        );
        assert_eq!(
            after_update.effective.monthly_limit,
            defaults.monthly_limit + 14
        );

        proxy
            .unbind_user_tag_from_user(&user.user_id, &tag.id)
            .await
            .expect("unbind user tag");
        let after_unbind = proxy
            .key_store
            .resolve_account_quota_resolution(&user.user_id)
            .await
            .expect("resolution after unbind");
        assert_eq!(
            after_unbind.effective.hourly_any_limit,
            defaults.hourly_any_limit
        );
        assert_eq!(after_unbind.effective.hourly_limit, defaults.hourly_limit);
        assert_eq!(after_unbind.effective.daily_limit, defaults.daily_limit);
        assert_eq!(after_unbind.effective.monthly_limit, defaults.monthly_limit);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_system_tag_defaults_backfill_repairs_legacy_zero_seed() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("linuxdo-system-tag-defaults");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = 0,
                   hourly_delta = 0,
                   daily_delta = 0,
                   monthly_delta = 0
               WHERE system_key LIKE 'linuxdo_l%'"#,
        )
        .execute(&proxy.key_store.pool)
        .await
        .expect("zero system tag deltas");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear linuxdo defaults migration marker");
        drop(proxy);

        let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy recreated");
        let defaults = linuxdo_system_tag_default_deltas();
        let seeded_rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
        )
        .fetch_all(&repaired.key_store.pool)
        .await
        .expect("read repaired seeded tag rows");
        assert_eq!(seeded_rows.len(), 5);
        assert!(
            seeded_rows
                .iter()
                .all(|row| *row == (defaults.0, defaults.1, defaults.2, defaults.3))
        );
    }

    #[tokio::test]
    async fn linuxdo_system_tag_defaults_backfill_repairs_partial_legacy_zero_seed() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("linuxdo-system-tag-defaults-partial");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = 0,
                   hourly_delta = 0,
                   daily_delta = 0,
                   monthly_delta = 0
               WHERE system_key IN ('linuxdo_l1', 'linuxdo_l3')"#,
        )
        .execute(&proxy.key_store.pool)
        .await
        .expect("zero partial system tag deltas");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear linuxdo defaults migration marker");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear linuxdo defaults tuple marker");
        drop(proxy);

        let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy recreated");
        let defaults = linuxdo_system_tag_default_deltas();
        let seeded_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&repaired.key_store.pool)
        .await
        .expect("read repaired seeded tag rows");
        assert_eq!(seeded_rows.len(), 5);
        assert!(
            seeded_rows
                .iter()
                .all(|(_, hourly_any, hourly, daily, monthly)| {
                    (*hourly_any, *hourly, *daily, *monthly)
                        == (defaults.0, defaults.1, defaults.2, defaults.3)
                })
        );
    }

    #[tokio::test]
    async fn linuxdo_system_tag_defaults_follow_env_changes_without_overwriting_customized_system_tags()
     {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("linuxdo-system-tag-default-sync");
        let db_str = db_path.to_string_lossy().to_string();
        let env_keys = [
            "TOKEN_HOURLY_REQUEST_LIMIT",
            "TOKEN_HOURLY_LIMIT",
            "TOKEN_DAILY_LIMIT",
            "TOKEN_MONTHLY_LIMIT",
        ];
        let previous: Vec<Option<String>> =
            env_keys.iter().map(|key| std::env::var(key).ok()).collect();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let initial_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("read initial linuxdo system tag rows");
        assert_eq!(initial_rows.len(), 5);
        assert!(
            initial_rows
                .iter()
                .all(|(_, hourly_any, hourly, daily, monthly)| {
                    (*hourly_any, *hourly, *daily, *monthly) == (11, 12, 13, 14)
                })
        );
        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after_default_change =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened after default change");
        let synced_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&proxy_after_default_change.key_store.pool)
        .await
        .expect("read synced linuxdo system tag rows");
        assert!(
            synced_rows
                .iter()
                .all(|(_, hourly_any, hourly, daily, monthly)| {
                    (*hourly_any, *hourly, *daily, *monthly) == (21, 22, 23, 24)
                })
        );

        proxy_after_default_change
            .update_user_tag(
                "linuxdo_l2",
                "linuxdo_l2",
                "L2",
                Some("linuxdo"),
                USER_TAG_EFFECT_QUOTA_DELTA,
                101,
                102,
                103,
                104,
            )
            .await
            .expect("update system tag")
            .expect("system tag present");
        drop(proxy_after_default_change);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "31");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "32");
            std::env::set_var("TOKEN_DAILY_LIMIT", "33");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "34");
        }

        let proxy_after_customization =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened after system tag customization");
        let final_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&proxy_after_customization.key_store.pool)
        .await
        .expect("read final linuxdo system tag rows");
        assert_eq!(final_rows.len(), 5);
        for (system_key, hourly_any, hourly, daily, monthly) in final_rows {
            if system_key == "linuxdo_l2" {
                assert_eq!((hourly_any, hourly, daily, monthly), (101, 102, 103, 104));
            } else {
                assert_eq!((hourly_any, hourly, daily, monthly), (31, 32, 33, 34));
            }
        }

        unsafe {
            for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
                if let Some(value) = old_value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_system_tags_seed_backfill_and_trust_level_sync() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("linuxdo-system-tags");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let seeded_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("count seeded tags");
        assert_eq!(seeded_count, 5);

        let defaults = linuxdo_system_tag_default_deltas();
        let seeded_rows = sqlx::query_as::<_, (String, String, Option<String>, i64, i64, i64, i64)>(
            "SELECT display_name, name, icon, hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
        )
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("read seeded tag rows");
        assert_eq!(seeded_rows.len(), 5);
        assert_eq!(
            seeded_rows[0],
            (
                "L0".to_string(),
                "linuxdo_l0".to_string(),
                Some("linuxdo".to_string()),
                defaults.0,
                defaults.1,
                defaults.2,
                defaults.3,
            )
        );
        assert_eq!(
            seeded_rows[4],
            (
                "L4".to_string(),
                "linuxdo_l4".to_string(),
                Some("linuxdo".to_string()),
                defaults.0,
                defaults.1,
                defaults.2,
                defaults.3,
            )
        );

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-system-user".to_string(),
                username: Some("linuxdo_system_user".to_string()),
                name: Some("LinuxDo System User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(3),
                raw_payload_json: None,
            })
            .await
            .expect("upsert linuxdo user");

        let first_key: String = sqlx::query_scalar(
            r#"SELECT t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
               LIMIT 1"#,
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read first linuxdo binding");
        assert_eq!(first_key, "linuxdo_l3");

        sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ?")
            .bind(&user.user_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("delete bindings to simulate historical gap");
        drop(proxy);

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        let restored_key: String = sqlx::query_scalar(
            r#"SELECT t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
               LIMIT 1"#,
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read restored linuxdo binding");
        assert_eq!(restored_key, "linuxdo_l3");

        proxy_after
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-system-user".to_string(),
                username: Some("linuxdo_system_user".to_string()),
                name: Some("LinuxDo System User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("update linuxdo trust level");
        let sync_keys = sqlx::query_scalar::<_, String>(
            r#"SELECT t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
               ORDER BY t.system_key"#,
        )
        .bind(&user.user_id)
        .fetch_all(&proxy_after.key_store.pool)
        .await
        .expect("read synced linuxdo bindings");
        assert_eq!(sync_keys, vec!["linuxdo_l1".to_string()]);

        proxy_after
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-system-user".to_string(),
                username: Some("linuxdo_system_user".to_string()),
                name: Some("LinuxDo System User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: None,
                raw_payload_json: None,
            })
            .await
            .expect("update linuxdo trust level to none");
        let retained_keys = sqlx::query_scalar::<_, String>(
            r#"SELECT t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
               ORDER BY t.system_key"#,
        )
        .bind(&user.user_id)
        .fetch_all(&proxy_after.key_store.pool)
        .await
        .expect("read retained linuxdo bindings");
        assert_eq!(retained_keys, vec!["linuxdo_l1".to_string()]);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_oauth_upsert_skips_missing_tags_for_new_accounts_and_recovers_after_reseed() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("linuxdo-sync-best-effort");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        sqlx::query("DELETE FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
            .execute(&proxy.key_store.pool)
            .await
            .expect("delete linuxdo system tags");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-best-effort-user".to_string(),
                username: Some("linuxdo_best_effort_user".to_string()),
                name: Some("LinuxDo Best Effort User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("new linuxdo account should still succeed without system tags");

        let oauth_row_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM oauth_accounts WHERE provider = 'linuxdo' AND provider_user_id = ?",
        )
        .bind("linuxdo-best-effort-user")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count oauth rows");
        assert_eq!(oauth_row_count, 1);
        let user_row_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?")
                .bind("linuxdo_best_effort_user")
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("count user rows");
        assert_eq!(user_row_count, 1);

        let binding_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*)
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'"#,
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count linuxdo bindings after skipped sync");
        assert_eq!(binding_count, 0);

        proxy
            .key_store
            .seed_linuxdo_system_tags()
            .await
            .expect("reseed linuxdo system tags");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-best-effort-user".to_string(),
                username: Some("linuxdo_best_effort_user".to_string()),
                name: Some("LinuxDo Best Effort User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("oauth upsert should attach system tag after reseeding tags");

        let restored_key: String = sqlx::query_scalar(
            r#"SELECT t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
               LIMIT 1"#,
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read restored linuxdo binding");
        assert_eq!(restored_key, "linuxdo_l2");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn legacy_custom_account_quota_limits_with_initial_timestamps_are_reclassified_before_default_resync()
     {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("account-limit-legacy-custom");
        let db_str = db_path.to_string_lossy().to_string();
        let env_keys = [
            "TOKEN_HOURLY_REQUEST_LIMIT",
            "TOKEN_HOURLY_LIMIT",
            "TOKEN_DAILY_LIMIT",
            "TOKEN_MONTHLY_LIMIT",
        ];
        let previous: Vec<Option<String>> =
            env_keys.iter().map(|key| std::env::var(key).ok()).collect();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "legacy-custom-user".to_string(),
                username: Some("legacy_custom_user".to_string()),
                name: Some("Legacy Custom User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_custom_user"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .expect("seed account quota row");
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = 101,
                   hourly_limit = 102,
                   daily_limit = 103,
                   monthly_limit = 104,
                   inherits_defaults = 1,
                   updated_at = created_at
               WHERE user_id = ?"#,
        )
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate legacy custom quota row with initial timestamps");
        sqlx::query("DELETE FROM meta WHERE key = ?")
            .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
            .execute(&proxy.key_store.pool)
            .await
            .expect("clear inherits defaults backfill marker");

        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read persisted legacy custom limits");
        assert_eq!(limits, (101, 102, 103, 104, 0));

        unsafe {
            for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
                if let Some(value) = old_value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn custom_account_quota_limits_survive_default_resync() {
        let _guard = env_lock().lock_owned().await;
        let db_path = temp_db_path("account-limit-custom-persist");
        let db_str = db_path.to_string_lossy().to_string();
        let env_keys = [
            "TOKEN_HOURLY_REQUEST_LIMIT",
            "TOKEN_HOURLY_LIMIT",
            "TOKEN_DAILY_LIMIT",
            "TOKEN_MONTHLY_LIMIT",
        ];
        let previous: Vec<Option<String>> =
            env_keys.iter().map(|key| std::env::var(key).ok()).collect();

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
            std::env::set_var("TOKEN_DAILY_LIMIT", "13");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
        }

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "limit-custom-user".to_string(),
                username: Some("limit_custom_user".to_string()),
                name: Some("Limit Custom User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_custom_user"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .expect("seed account quota row");
        let updated = proxy
            .update_account_quota_limits(&user.user_id, 101, 102, 103, 104)
            .await
            .expect("update custom base quota");
        assert!(updated);

        let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read custom limits");
        assert_eq!(first_limits, (101, 102, 103, 104, 0));

        drop(proxy);

        unsafe {
            std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
            std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
            std::env::set_var("TOKEN_DAILY_LIMIT", "23");
            std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
        }

        let proxy_after =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
                .await
                .expect("proxy reopened");
        let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(&user.user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read persisted custom limits");
        assert_eq!(second_limits, (101, 102, 103, 104, 0));

        unsafe {
            for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
                if let Some(value) = old_value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn block_all_user_tag_zeroes_effective_quota_and_blocks_account_usage() {
        let db_path = temp_db_path("user-tag-block-all");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "block-all-user".to_string(),
                username: Some("block_all_user".to_string()),
                name: Some("Block All User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:block_all_user"))
            .await
            .expect("bind token");
        let tag = proxy
            .create_user_tag(
                "blocked_all",
                "Blocked All",
                Some("ban"),
                USER_TAG_EFFECT_BLOCK_ALL,
                0,
                0,
                0,
                0,
            )
            .await
            .expect("create block all tag");
        let bound = proxy
            .bind_user_tag_to_user(&user.user_id, &tag.id)
            .await
            .expect("bind block all tag");
        assert!(bound);

        let details = proxy
            .get_admin_user_quota_details(&user.user_id)
            .await
            .expect("quota details")
            .expect("quota details present");
        assert_eq!(details.effective.hourly_any_limit, 0);
        assert_eq!(details.effective.hourly_limit, 0);
        assert_eq!(details.effective.daily_limit, 0);
        assert_eq!(details.effective.monthly_limit, 0);
        assert!(
            details
                .breakdown
                .iter()
                .any(|entry| entry.effect_kind == USER_TAG_EFFECT_BLOCK_ALL)
        );

        let hourly_any_verdict = proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("hourly-any verdict");
        assert!(!hourly_any_verdict.allowed);
        assert_eq!(hourly_any_verdict.hourly_limit, 0);

        let quota_verdict = proxy
            .check_token_quota(&token.id)
            .await
            .expect("business quota verdict");
        assert!(!quota_verdict.allowed);
        assert_eq!(quota_verdict.hourly_limit, 0);
        assert_eq!(quota_verdict.daily_limit, 0);
        assert_eq!(quota_verdict.monthly_limit, 0);

        let request_usage: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_REQUEST_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read raw request usage");
        assert_eq!(request_usage, 0);
        let hourly_usage: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_MINUTE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read hourly business usage");
        assert_eq!(hourly_usage, 0);
        let daily_usage: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
        )
        .bind(&user.user_id)
        .bind(GRANULARITY_HOUR)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read daily business usage");
        assert_eq!(daily_usage, 0);
        let monthly_usage = sqlx::query_scalar::<_, i64>(
            "SELECT month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
        )
        .bind(&user.user_id)
        .fetch_optional(&proxy.key_store.pool)
        .await
        .expect("read monthly business usage")
        .unwrap_or(0);
        assert_eq!(monthly_usage, 0);

        let unbound = proxy
            .unbind_user_tag_from_user(&user.user_id, &tag.id)
            .await
            .expect("unbind block all tag");
        assert!(unbound);

        let hourly_any_after_unbind = proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("hourly-any verdict after unbind");
        assert!(hourly_any_after_unbind.allowed);

        let quota_after_unbind = proxy
            .check_token_quota(&token.id)
            .await
            .expect("business quota verdict after unbind");
        assert!(quota_after_unbind.allowed);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn list_recent_jobs_paginated_includes_key_group() {
        let db_path = temp_db_path("jobs-list-key-group");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let grouped_key_id = proxy
            .add_or_undelete_key_in_group("tvly-jobs-grouped", Some("ops"))
            .await
            .expect("create grouped key");
        let ungrouped_key_id = proxy
            .add_or_undelete_key_in_group("tvly-jobs-ungrouped", None)
            .await
            .expect("create ungrouped key");

        let grouped_job_id = proxy
            .scheduled_job_start("quota_sync", Some(&grouped_key_id), 1)
            .await
            .expect("start grouped job");
        proxy
            .scheduled_job_finish(grouped_job_id, "error", Some("usage_http 401"))
            .await
            .expect("finish grouped job");

        let ungrouped_job_id = proxy
            .scheduled_job_start("quota_sync", Some(&ungrouped_key_id), 1)
            .await
            .expect("start ungrouped job");
        proxy
            .scheduled_job_finish(ungrouped_job_id, "success", Some("limit=100 remaining=99"))
            .await
            .expect("finish ungrouped job");

        let cleanup_job_id = proxy
            .scheduled_job_start("auth_token_logs_gc", None, 1)
            .await
            .expect("start cleanup job");
        proxy
            .scheduled_job_finish(cleanup_job_id, "success", Some("pruned=10"))
            .await
            .expect("finish cleanup job");

        let (items, total) = proxy
            .list_recent_jobs_paginated("all", 1, 10)
            .await
            .expect("list jobs");

        assert_eq!(total, 3);

        let grouped_job = items
            .iter()
            .find(|item| item.key_id.as_deref() == Some(grouped_key_id.as_str()))
            .expect("grouped job present");
        assert_eq!(grouped_job.key_group.as_deref(), Some("ops"));

        let ungrouped_job = items
            .iter()
            .find(|item| item.key_id.as_deref() == Some(ungrouped_key_id.as_str()))
            .expect("ungrouped job present");
        assert_eq!(ungrouped_job.key_group, None);

        let cleanup_job = items
            .iter()
            .find(|item| item.key_id.is_none())
            .expect("cleanup job present");
        assert_eq!(cleanup_job.key_group, None);

        let _ = std::fs::remove_file(db_path);
    }
}
