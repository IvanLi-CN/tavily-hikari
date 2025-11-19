use std::{cmp::min, collections::HashMap, sync::Arc, time::Duration};

use bytes::Bytes;
use chrono::{Datelike, TimeZone, Utc};
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
use tokio::sync::Mutex;
use url::form_urlencoded;

/// Tavily MCP upstream默认端点。
pub const DEFAULT_UPSTREAM: &str = "https://mcp.tavily.com/mcp";

const STATUS_ACTIVE: &str = "active";
const STATUS_EXHAUSTED: &str = "exhausted";
const STATUS_DISABLED: &str = "disabled";

const OUTCOME_SUCCESS: &str = "success";
const OUTCOME_ERROR: &str = "error";
const OUTCOME_QUOTA_EXHAUSTED: &str = "quota_exhausted";
const OUTCOME_UNKNOWN: &str = "unknown";

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

pub const TOKEN_HOURLY_LIMIT: i64 = 100;
pub const TOKEN_DAILY_LIMIT: i64 = 500;
pub const TOKEN_MONTHLY_LIMIT: i64 = 3000;
// Soft affinity window for mapping access tokens to API keys (in seconds).
// Within this window, a token will try to reuse the same API key if it is still active.
const TOKEN_AFFINITY_TTL_SECS: i64 = 15 * 60;
// Hard cap on the number of token→key affinity entries kept in memory to prevent
// unbounded growth under churny traffic (many distinct tokens).
const TOKEN_AFFINITY_MAX_ENTRIES: usize = 10_000;

const GRANULARITY_MINUTE: &str = "minute";
const GRANULARITY_HOUR: &str = "hour";
const BUCKET_RETENTION_SECS: i64 = 2 * 24 * 3600; // 48h，足够覆盖 24h 窗口
const CLEANUP_INTERVAL_SECS: i64 = 600;
const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3600;
const SECS_PER_DAY: i64 = 24 * SECS_PER_HOUR;
const TOKEN_USAGE_STATS_BUCKET_SECS: i64 = SECS_PER_HOUR;

// Time-based retention for per-token access logs (auth_token_logs).
// This is purely time-driven and must not depend on access token enable/disable/delete status,
// to preserve auditability.
const AUTH_TOKEN_LOG_RETENTION_SECS: i64 = 90 * SECS_PER_DAY;

const META_KEY_DATA_CONSISTENCY_DONE: &str = "data_consistency_v1_done";
const META_KEY_TOKEN_USAGE_ROLLUP_TS: &str = "token_usage_rollup_last_ts";
const META_KEY_HEAL_ORPHAN_TOKENS_V1: &str = "heal_orphan_auth_tokens_from_logs_v1";

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

#[derive(Clone, Debug)]
struct TokenQuota {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
}

#[derive(Default, Debug)]
struct CleanupState {
    last_pruned: i64,
}

/// 负责均衡 Tavily API key 并透传请求的代理。
#[derive(Clone, Debug)]
pub struct TavilyProxy {
    client: Client,
    upstream: Url,
    key_store: Arc<KeyStore>,
    upstream_origin: String,
    token_quota: TokenQuota,
    affinity: Arc<Mutex<TokenAffinityState>>,
}

impl TavilyProxy {
    pub async fn new<I, S>(keys: I, database_path: &str) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_endpoint(keys, DEFAULT_UPSTREAM, database_path).await
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
        let key_store = Arc::new(key_store);
        let token_quota = TokenQuota::new(key_store.clone());

        Ok(Self {
            client: Client::new(),
            upstream,
            key_store,
            upstream_origin,
            token_quota,
            affinity: Arc::new(Mutex::new(TokenAffinityState::new(TOKEN_AFFINITY_TTL_SECS))),
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

        let mut builder = self.client.request(request.method.clone(), url.clone());

        let sanitized_headers = self.sanitize_headers(&request.headers);
        for (name, value) in sanitized_headers.headers.iter() {
            // Host/Content-Length 由 reqwest 重算。
            if name == HOST || name == CONTENT_LENGTH {
                continue;
            }
            builder = builder.header(name, value);
        }

        builder = builder.header("Tavily-Api-Key", lease.secret.as_str());

        let response = builder.body(request.body.clone()).send().await;

        match response {
            Ok(response) => {
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

                if status.as_u16() == 432 || outcome.mark_exhausted {
                    self.key_store.mark_quota_exhausted(&lease.secret).await?;
                } else {
                    self.key_store.restore_active_status(&lease.secret).await?;
                }

                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                })
            }
            Err(err) => {
                log_error(
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
                Err(ProxyError::Http(err))
            }
        }
    }

    /// 获取全部 API key 的统计信息，按状态与最近使用时间排序。
    pub async fn list_api_key_metrics(&self) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metrics().await
    }

    /// 获取最近的请求日志，按时间倒序排列。
    pub async fn recent_request_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_recent_logs(limit).await
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
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % 60);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * 60;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let hourly_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let daily_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let month_start = start_of_month(now);
        let next_month_reset = start_of_next_month(month_start).timestamp();
        for token in tokens.iter_mut() {
            if let Some(verdict) = verdicts.get(&token.id) {
                token.quota_hourly_reset_at = if verdict.hourly_used > 0 {
                    hourly_oldest
                        .get(&token.id)
                        .copied()
                        .map(|bucket| bucket + SECS_PER_HOUR)
                } else {
                    None
                };
                token.quota_daily_reset_at = if verdict.daily_used > 0 {
                    daily_oldest
                        .get(&token.id)
                        .copied()
                        .map(|bucket| bucket + SECS_PER_DAY)
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
        result_status: &str,
        error_message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_token_log(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                result_status,
                error_message,
            )
            .await
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

    /// Token logs (page-based pagination)
    pub async fn token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
    ) -> Result<(Vec<TokenLogRecord>, i64), ProxyError> {
        self.key_store
            .fetch_token_logs_page(token_id, page, per_page, since, until)
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

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
    }

    /// Public metrics: successful requests today and this month.
    pub async fn success_breakdown(&self) -> Result<SuccessBreakdown, ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let day_start = start_of_day(now).timestamp();
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
            hourly_limit: TOKEN_HOURLY_LIMIT,
            daily_limit: TOKEN_DAILY_LIMIT,
            monthly_limit: TOKEN_MONTHLY_LIMIT,
        }
    }

    async fn check(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        // Increment usage buckets and monthly quota as an approximate, cheap counter.
        // This path is allowed to drift slightly from the detailed logs in exchange
        // for lower per-request overhead.
        self.store
            .increment_usage_bucket(token_id, minute_bucket, GRANULARITY_MINUTE)
            .await?;
        self.store
            .increment_usage_bucket(token_id, hour_bucket, GRANULARITY_HOUR)
            .await?;

        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;

        let hourly_used = self
            .store
            .sum_usage_buckets(token_id, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let daily_used = self
            .store
            .sum_usage_buckets(token_id, GRANULARITY_HOUR, day_window_start)
            .await?;

        let month_start = start_of_month(now).timestamp();
        let monthly_used = self
            .store
            .increment_monthly_quota(token_id, month_start)
            .await?;

        self.maybe_cleanup(now_ts).await?;

        Ok(TokenQuotaVerdict::new(
            hourly_used,
            self.hourly_limit,
            daily_used,
            self.daily_limit,
            monthly_used,
            self.monthly_limit,
        ))
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
        let minute_bucket = now_ts - (now_ts % 60);
        let hour_bucket = now_ts - (now_ts % 3600);
        let hour_window_start = minute_bucket - 59 * 60;
        let day_window_start = hour_bucket - 23 * 3600;
        let hourly_totals = self
            .store
            .sum_usage_buckets_bulk(token_ids, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let daily_totals = self
            .store
            .sum_usage_buckets_bulk(token_ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let month_start = start_of_month(now).timestamp();
        let monthly_totals = self
            .store
            .fetch_monthly_counts(token_ids, month_start)
            .await?;
        let mut verdicts = HashMap::new();
        for token_id in token_ids {
            let hourly_used = hourly_totals.get(token_id).copied().unwrap_or(0);
            let daily_used = daily_totals.get(token_id).copied().unwrap_or(0);
            let monthly_used = monthly_totals.get(token_id).copied().unwrap_or(0);
            verdicts.insert(
                token_id.clone(),
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
        let base = Url::parse(usage_base).map_err(|e| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_string(),
            source: e,
        })?;
        let mut url = base.clone();
        url.set_path("/usage");

        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", secret))
            .send()
            .await
            .map_err(ProxyError::Http)?;
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
        let now = Utc::now().timestamp();
        self.key_store
            .update_quota_for_key(key_id, limit, remaining, now)
            .await?;
        Ok((limit, remaining))
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

#[derive(Debug)]
struct KeyStore {
    pool: SqlitePool,
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

        let store = Self { pool };
        store.initialize_schema().await?;
        Ok(store)
    }

    async fn initialize_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
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
                result_status TEXT NOT NULL,
                error_message TEXT,
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

    async fn increment_usage_bucket(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, 1)
            ON CONFLICT(token_id, bucket_start, granularity)
            DO UPDATE SET count = count + 1
            "#,
        )
        .bind(token_id)
        .bind(bucket_start)
        .bind(granularity)
        .execute(&self.pool)
        .await?;
        Ok(())
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

    /// Aggregate per-token usage logs into hourly buckets in token_usage_stats.
    /// Returns (rows_affected, new_last_rollup_ts). When there are no new logs,
    /// rows_affected is 0 and new_last_rollup_ts is None.
    async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        let last_ts = self
            .get_meta_i64(META_KEY_TOKEN_USAGE_ROLLUP_TS)
            .await?
            .unwrap_or(0);

        // Use an inclusive lower bound so that logs with the same created_at
        // as the previous max_ts are reprocessed rather than skipped.
        let max_created: Option<i64> =
            sqlx::query_scalar("SELECT MAX(created_at) FROM auth_token_logs WHERE created_at >= ?")
                .bind(last_ts)
                .fetch_one(&self.pool)
                .await?;

        let Some(max_ts) = max_created else {
            return Ok((0, None));
        };

        let bucket_secs = TOKEN_USAGE_STATS_BUCKET_SECS;

        let result = sqlx::query(
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
            WHERE created_at >= ? AND created_at <= ?
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
        .bind(last_ts)
        .bind(max_ts)
        .execute(&self.pool)
        .await?;

        let affected = result.rows_affected() as i64;

        self.set_meta_i64(META_KEY_TOKEN_USAGE_ROLLUP_TS, max_ts)
            .await?;

        Ok((affected, Some(max_ts)))
    }

    async fn increment_monthly_quota(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, 1)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                    ELSE auth_token_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN 1
                    ELSE auth_token_quota.month_count + 1
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(token_id)
        .bind(current_month_start)
        .fetch_one(&self.pool)
        .await?;

        Ok(month_count)
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys_new (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO api_keys_new (id, api_key, status, status_changed_at, last_used_at)
            SELECT id, api_key, status, status_changed_at, last_used_at
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
        let totals_row = sqlx::query(
            r#"
            SELECT
              COUNT(1) AS total_requests,
              SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
              SUM(CASE WHEN result_status = 'error' THEN 1 ELSE 0 END) AS error_count,
              SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
            FROM request_logs
            WHERE api_key_id = ? AND created_at >= ?
            "#,
        )
        .bind(key_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(created_at) FROM request_logs WHERE api_key_id = ? AND created_at >= ?",
        )
        .bind(key_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        // Active/exhausted counts in this scope are not meaningful per single key; expose 1/0 for convenience
        // We will compute based on current key status
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        let (active_keys, exhausted_keys) = match status.as_deref() {
            Some(STATUS_EXHAUSTED) => (0, 1),
            _ => (1, 0),
        };

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys,
            exhausted_keys,
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

    // ----- Token usage logs & metrics -----
    #[allow(clippy::too_many_arguments)]
    async fn insert_token_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        result_status: &str,
        error_message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(result_status)
        .bind(error_message)
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

    pub async fn fetch_token_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = if let Some(bid) = before_id {
            sqlx::query_as::<_, (
                i64,
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                String,
                Option<String>,
                i64,
            )>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
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
            sqlx::query_as::<_, (
                i64,
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                String,
                Option<String>,
                i64,
            )>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
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
                    result_status,
                    error_message,
                    created_at,
                )| TokenLogRecord {
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    result_status,
                    error_message,
                    created_at,
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
    ) -> Result<(Vec<TokenLogRecord>, i64), ProxyError> {
        let per_page = per_page.clamp(1, 200) as i64;
        let page = page.max(1) as i64;
        let offset = (page - 1) * per_page;

        let total: i64 = if let Some(until) = until {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ? AND created_at >= ? AND created_at < ?",
            )
            .bind(token_id)
            .bind(since)
            .bind(until)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ? AND created_at >= ?",
            )
            .bind(token_id)
            .bind(since)
            .fetch_one(&self.pool)
            .await?
        };

        let rows = if let Some(until) = until {
            sqlx::query_as::<_, (
                i64,
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                String,
                Option<String>,
                i64,
            )>(
                r#"
            SELECT id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
            FROM auth_token_logs
            WHERE token_id = ? AND created_at >= ? AND created_at < ?
            ORDER BY created_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
            )
            .bind(token_id)
            .bind(since)
            .bind(until)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (
            i64,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            String,
            Option<String>,
            i64,
        )>(
            r#"
            SELECT id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
            FROM auth_token_logs
            WHERE token_id = ? AND created_at >= ?
            ORDER BY created_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(token_id)
        .bind(since)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?
        };

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
                    result_status,
                    error_message,
                    created_at,
                )| TokenLogRecord {
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    result_status,
                    error_message,
                    created_at,
                },
            )
            .collect();

        Ok((items, total))
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

    async fn mark_quota_exhausted(&self, key: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
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
        Ok(())
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

    // Admin ops: add/undelete key by secret
    async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().timestamp();
        if let Some((id, deleted_at)) = sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT id, deleted_at FROM api_keys WHERE api_key = ? LIMIT 1",
        )
        .bind(api_key)
        .fetch_optional(&mut *tx)
        .await?
        {
            if deleted_at.is_some() {
                sqlx::query("UPDATE api_keys SET deleted_at = NULL WHERE id = ?")
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            return Ok(id);
        }

        let id = Self::generate_unique_key_id(&mut tx).await?;
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, api_key, status, status_changed_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(api_key)
        .bind(STATUS_ACTIVE)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
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
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn fetch_api_key_metrics(&self) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COUNT(*) AS total_requests,
                    SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END) AS success_count,
                    SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END) AS error_count,
                    SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM request_logs
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            WHERE ak.deleted_at IS NULL
            ORDER BY ak.status ASC, ak.last_used_at ASC, ak.id ASC
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(OUTCOME_ERROR)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .fetch_all(&self.pool)
        .await?;

        let metrics = rows
            .into_iter()
            .map(|row| -> Result<ApiKeyMetrics, sqlx::Error> {
                let id: String = row.try_get("id")?;
                let status: String = row.try_get("status")?;
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

                Ok(ApiKeyMetrics {
                    id,
                    status,
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
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(metrics)
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
            WHERE deleted_at IS NULL AND (
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
            r#"SELECT id, job_type, key_id, status, attempt, message, started_at, finished_at
                FROM scheduled_jobs
                ORDER BY started_at DESC, id DESC
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
            "quota" => "WHERE job_type = 'quota_sync' OR job_type = 'quota_sync/manual'",
            "usage" => "WHERE job_type = 'token_usage_rollup'",
            "logs" => "WHERE job_type = 'auth_token_logs_gc'",
            _ => "",
        };

        let count_query = format!("SELECT COUNT(*) FROM scheduled_jobs {}", where_clause);
        let total: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(&self.pool)
            .await?;

        let select_query = format!(
            r#"
            SELECT id, job_type, key_id, status, attempt, message, started_at, finished_at
            FROM scheduled_jobs
            {}
            ORDER BY started_at DESC, id DESC
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

    async fn get_meta_i64(&self, key: &str) -> Result<Option<i64>, ProxyError> {
        let value = sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(v) = value {
            match v.parse::<i64>() {
                Ok(parsed) => Ok(Some(parsed)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    async fn set_meta_i64(&self, key: &str, value: i64) -> Result<(), ProxyError> {
        let v = value.to_string();
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(v)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fetch_summary(&self) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS success_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS error_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS quota_exhausted_count
            FROM request_logs
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(OUTCOME_ERROR)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .fetch_one(&self.pool)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN status = ? THEN 1 ELSE 0 END), 0) AS active_keys,
                COALESCE(SUM(CASE WHEN status = ? THEN 1 ELSE 0 END), 0) AS exhausted_keys
            FROM api_keys
            WHERE deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .fetch_one(&self.pool)
        .await?;

        let last_activity =
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(created_at) FROM request_logs")
                .fetch_one(&self.pool)
                .await?;

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0) AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0) AS total_quota_remaining
            FROM api_keys
            WHERE deleted_at IS NULL
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
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_success
            FROM request_logs
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
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
            FROM request_logs
            WHERE auth_token_id = ?
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
        let mut exceeded_window = None;
        let mut allowed = true;
        if hourly_used_raw > hourly_limit {
            exceeded_window = Some(QuotaWindow::Hour);
            allowed = false;
        }
        if daily_used_raw > daily_limit {
            exceeded_window = Some(QuotaWindow::Day);
            allowed = false;
        }
        if monthly_used_raw > monthly_limit {
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

    pub fn window_name(&self) -> Option<&'static str> {
        self.exceeded_window.map(|w| w.as_str())
    }

    pub fn state_key(&self) -> &'static str {
        self.window_name().unwrap_or("normal")
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

/// Per-token log for detail UI
#[derive(Debug, Clone)]
pub struct TokenLogRecord {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub http_status: Option<i64>,
    pub mcp_status: Option<i64>,
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

fn start_of_month(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
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

#[derive(Debug, Clone, Copy)]
struct AttemptAnalysis {
    status: &'static str,
    mark_exhausted: bool,
    tavily_status_code: Option<i64>,
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
            mark_exhausted: false,
            tavily_status_code: Some(status.as_u16() as i64),
        };
    }

    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return AttemptAnalysis {
                status: OUTCOME_UNKNOWN,
                mark_exhausted: false,
                tavily_status_code: None,
            };
        }
    };

    let mut any_success = false;
    let mut detected_code = None;
    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        messages.push(value);
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
                        mark_exhausted: true,
                        tavily_status_code: code.or(detected_code),
                    };
                }
                MessageOutcome::Error => {
                    return AttemptAnalysis {
                        status: OUTCOME_ERROR,
                        mark_exhausted: false,
                        tavily_status_code: code.or(detected_code),
                    };
                }
                MessageOutcome::Success => any_success = true,
            }
        }
    }

    if any_success {
        return AttemptAnalysis {
            status: OUTCOME_SUCCESS,
            mark_exhausted: false,
            tavily_status_code: detected_code,
        };
    }

    AttemptAnalysis {
        status: OUTCOME_UNKNOWN,
        mark_exhausted: false,
        tavily_status_code: detected_code,
    }
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
    if value.get("error").is_some() {
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

    if result.get("error").is_some() {
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
        return Some((classify_status_code(code), Some(code)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    async fn quota_blocks_after_hourly_limit() {
        let db_path = temp_db_path("quota-test");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy.create_access_token(None).await.expect("token");

        for _ in 0..TOKEN_HOURLY_LIMIT {
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
}
