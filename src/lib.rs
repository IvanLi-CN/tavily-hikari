use std::{cmp::min, sync::Arc};

use bytes::Bytes;
use chrono::{Datelike, TimeZone, Utc};
use nanoid::nanoid;
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

        if sanitized.is_empty() {
            return Err(ProxyError::EmptyKeySet);
        }

        let key_store = KeyStore::new(&sanitized, database_path).await?;
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
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: Some(status),
                        tavily_status_code: outcome.tavily_status_code,
                        error: None,
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
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
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

    /// 根据 ID 获取真实 API key，仅供管理员调用。
    pub async fn get_api_key_secret(&self, key_id: &str) -> Result<Option<String>, ProxyError> {
        self.key_store.fetch_api_key_secret(key_id).await
    }

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
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
    async fn new(keys: &[String], database_path: &str) -> Result<Self, ProxyError> {
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
        store.sync_keys(keys).await?;
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
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
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

        Ok(())
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
                    method TEXT NOT NULL,
                    path TEXT NOT NULL,
                    query TEXT,
                    status_code INTEGER,
                    tavily_status_code INTEGER,
                    error_message TEXT,
                    result_status TEXT NOT NULL DEFAULT 'unknown',
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
                    method,
                    path,
                    query,
                    status_code,
                    tavily_status_code,
                    error_message,
                    result_status,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                )
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

    async fn sync_keys(&self, keys: &[String]) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;

        for key in keys {
            let exists = sqlx::query_scalar::<_, Option<String>>(
                "SELECT id FROM api_keys WHERE api_key = ? LIMIT 1",
            )
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;

            if exists.is_some() {
                continue;
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (id, api_key, status)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(key)
            .bind(STATUS_ACTIVE)
            .execute(&mut *tx)
            .await?;
        }

        if keys.is_empty() {
            sqlx::query("DELETE FROM api_keys")
                .execute(&mut *tx)
                .await?;
        } else {
            let mut builder = QueryBuilder::new("DELETE FROM api_keys WHERE api_key NOT IN (");
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
            WHERE status = ?
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
            WHERE status <> ?
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                id ASC
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

        Err(ProxyError::NoAvailableKeys)
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
            WHERE api_key = ?
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(now)
        .bind(now)
        .bind(key)
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
            WHERE api_key = ? AND status <> ?
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key)
        .bind(STATUS_ACTIVE)
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
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(entry.key_id)
        .bind(entry.method.as_str())
        .bind(entry.path)
        .bind(entry.query)
        .bind(status_code)
        .bind(entry.tavily_status_code)
        .bind(entry.error)
        .bind(entry.outcome)
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
                let total_requests: i64 = row.try_get("total_requests")?;
                let success_count: i64 = row.try_get("success_count")?;
                let error_count: i64 = row.try_get("error_count")?;
                let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;

                Ok(ApiKeyMetrics {
                    id,
                    status,
                    status_changed_at: status_changed_at.and_then(normalize_timestamp),
                    last_used_at: normalize_timestamp(last_used_at),
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
}

#[derive(Debug)]
struct ApiKeyLease {
    id: String,
    secret: String,
}

struct AttemptLog<'a> {
    key_id: &'a str,
    method: &'a Method,
    path: &'a str,
    query: Option<&'a str>,
    status: Option<StatusCode>,
    tavily_status_code: Option<i64>,
    error: Option<&'a str>,
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

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("no API keys provided")]
    EmptyKeySet,
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
