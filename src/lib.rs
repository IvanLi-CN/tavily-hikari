use std::{cmp::min, sync::Arc};

use bytes::Bytes;
use chrono::{Datelike, TimeZone, Utc};
use nanoid::nanoid;
use rand::Rng;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{CONTENT_LENGTH, HOST, HeaderMap, HeaderValue},
};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
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

#[derive(Debug, Clone)]
struct SanitizedHeaders {
    headers: HeaderMap,
    forwarded: Vec<String>,
    dropped: Vec<String>,
}

/// 负责均衡 Tavily API key 并透传请求的代理。
#[derive(Clone, Debug)]
pub struct TavilyProxy {
    client: Client,
    upstream: Url,
    key_store: Arc<KeyStore>,
    upstream_origin: String,
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

        Ok(Self {
            client: Client::new(),
            upstream,
            key_store: Arc::new(key_store),
            upstream_origin,
        })
    }

    /// 将请求透传到 Tavily upstream 并记录日志。
    pub async fn proxy_request(&self, request: ProxyRequest) -> Result<ProxyResponse, ProxyError> {
        let lease = self.key_store.acquire_key().await?;

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

    /// Admin: list tokens for management.
    pub async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        self.key_store.list_access_tokens().await
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

#[derive(Debug)]
struct KeyStore {
    pool: SqlitePool,
}

impl KeyStore {
    async fn new(database_path: &str) -> Result<Self, ProxyError> {
        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true);

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
                last_used_at INTEGER NOT NULL DEFAULT 0
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

        // Access tokens for /mcp authentication
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_tokens (
                id TEXT PRIMARY KEY,           -- 4-char id code
                secret TEXT NOT NULL,          -- 12-char secret
                enabled INTEGER NOT NULL DEFAULT 1,
                note TEXT,
                total_requests INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_auth_tokens_schema().await?;

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

        Ok(())
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
                SELECT id, api_key_id, method, path, query, status_code, tavily_status_code, error_message,
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
                SELECT id, api_key_id, method, path, query, status_code, tavily_status_code, error_message,
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
        if id.len() != 4 || secret.len() != 12 {
            return Ok(false);
        }

        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COUNT(1) as cnt, enabled FROM auth_tokens WHERE id = ? AND secret = ? LIMIT 1",
        )
        .bind(id)
        .bind(secret)
        .fetch_optional(&mut *tx)
        .await?;

        match row {
            Some((cnt, enabled)) if cnt > 0 && enabled == 1 => {
                sqlx::query(
                    "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ?",
                )
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(true)
            }
            _ => {
                tx.rollback().await.ok();
                Ok(false)
            }
        }
    }

    async fn create_access_token(&self, note: Option<&str>) -> Result<AuthTokenSecret, ProxyError> {
        let now = Utc::now().timestamp();
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        loop {
            let id = random_string(ALPHABET, 4);
            let secret = random_string(ALPHABET, 12);
            let res = sqlx::query(
                r#"INSERT INTO auth_tokens (id, secret, enabled, note, total_requests, created_at, last_used_at)
                   VALUES (?, ?, 1, ?, 0, ?, NULL)"#,
            )
            .bind(&id)
            .bind(&secret)
            .bind(note.unwrap_or(""))
            .bind(now)
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

    // Generate random string of given length from provided alphabet
    // Alphabet is a byte slice of ASCII alphanumerics
    // Using ThreadRng for simplicity

    async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64, Option<String>, i64, i64, Option<i64>)>(
            r#"SELECT id, enabled, note, total_requests, created_at, last_used_at FROM auth_tokens ORDER BY id ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                },
            )
            .collect())
    }

    async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        sqlx::query("DELETE FROM auth_tokens WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_access_token_enabled(&self, id: &str, enabled: bool) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET enabled = ? WHERE id = ?")
            .bind(if enabled { 1 } else { 0 })
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_access_token_note(&self, id: &str, note: &str) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET note = ? WHERE id = ?")
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
        let row =
            sqlx::query_as::<_, (String,)>("SELECT secret FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &secret),
        }))
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
            "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ?",
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
        let row = if let Some(until) = until {
            sqlx::query(
                r#"
            SELECT
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS success_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS error_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS quota_exhausted_count
            FROM auth_token_logs
            WHERE token_id = ? AND created_at >= ? AND created_at < ?
            "#,
            )
            .bind(OUTCOME_SUCCESS)
            .bind(OUTCOME_ERROR)
            .bind(OUTCOME_QUOTA_EXHAUSTED)
            .bind(token_id)
            .bind(since)
            .bind(until)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
            SELECT
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS success_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS error_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS quota_exhausted_count
            FROM auth_token_logs
            WHERE token_id = ? AND created_at >= ?
            "#,
            )
            .bind(OUTCOME_SUCCESS)
            .bind(OUTCOME_ERROR)
            .bind(OUTCOME_QUOTA_EXHAUSTED)
            .bind(token_id)
            .bind(since)
            .fetch_one(&self.pool)
            .await?
        };

        let last_activity_query = if let Some(until) = until {
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(created_at) FROM auth_token_logs WHERE token_id = ? AND created_at >= ? AND created_at < ?")
                .bind(token_id)
                .bind(since)
                .bind(until)
        } else {
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(created_at) FROM auth_token_logs WHERE token_id = ? AND created_at >= ?")
                .bind(token_id)
                .bind(since)
        };
        let last_activity = last_activity_query.fetch_one(&self.pool).await?;

        Ok(TokenSummary {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
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

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            last_activity,
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

/// 每个 API key 的聚合统计信息。
#[derive(Debug, Clone)]
pub struct ApiKeyMetrics {
    pub id: String,
    pub status: String,
    pub status_changed_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub deleted_at: Option<i64>,
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
}

/// Successful request counters for public metrics.
#[derive(Debug, Clone)]
pub struct SuccessBreakdown {
    pub monthly_success: i64,
    pub daily_success: i64,
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
    pub total_requests: i64,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
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
}

fn start_of_month(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
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
}
