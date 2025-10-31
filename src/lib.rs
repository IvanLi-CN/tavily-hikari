use std::{cmp::min, sync::Arc};

use bytes::Bytes;
use chrono::{Datelike, TimeZone, Utc};
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{CONTENT_LENGTH, HOST, HeaderMap},
};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{QueryBuilder, SqlitePool};
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

/// 负责均衡 Tavily API key 并透传请求的代理。
#[derive(Clone, Debug)]
pub struct TavilyProxy {
    client: Client,
    upstream: Url,
    key_store: Arc<KeyStore>,
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

        Ok(Self {
            client: Client::new(),
            upstream,
            key_store: Arc::new(key_store),
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
            pairs.append_pair("tavilyApiKey", lease.key.as_str());
        }

        drop(url.query_pairs_mut());

        let mut builder = self.client.request(request.method.clone(), url.clone());

        for (name, value) in request.headers.iter() {
            // Host/Content-Length 由 reqwest 重算。
            if name == HOST || name == CONTENT_LENGTH {
                continue;
            }
            builder = builder.header(name, value);
        }

        builder = builder.header("Tavily-Api-Key", lease.key.as_str());

        let response = builder.body(request.body.clone()).send().await;

        match response {
            Ok(response) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                let outcome = analyze_attempt(status, &body_bytes);

                log_success(
                    &lease.key,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    status,
                );

                self.key_store
                    .log_attempt(
                        &lease.key,
                        &request.method,
                        request.path.as_str(),
                        request.query.as_deref(),
                        Some(status),
                        None,
                        &body_bytes,
                        outcome.status,
                    )
                    .await?;

                if status.as_u16() == 432 || outcome.mark_exhausted {
                    self.key_store.mark_quota_exhausted(&lease.key).await?;
                }

                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                })
            }
            Err(err) => {
                log_error(
                    &lease.key,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    &err,
                );
                self.key_store
                    .log_attempt(
                        &lease.key,
                        &request.method,
                        request.path.as_str(),
                        request.query.as_deref(),
                        None,
                        Some(&err.to_string()),
                        &[],
                        OUTCOME_ERROR,
                    )
                    .await?;
                Err(ProxyError::Http(err))
            }
        }
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
                api_key TEXT PRIMARY KEY,
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
                api_key TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                response_body BLOB,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key) REFERENCES api_keys(api_key)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_request_logs_schema().await?;

        Ok(())
    }

    async fn upgrade_api_keys_schema(&self) -> Result<(), ProxyError> {
        if self.api_keys_column_exists("disabled_at").await? {
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

        // Keys that previously had disabled_at set should now be marked as exhausted.
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

        Ok(())
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
        let now = Utc::now().timestamp();

        for key in keys {
            sqlx::query(
                r#"
                INSERT INTO api_keys (api_key, status, status_changed_at)
                VALUES (?, ?, ?)
                ON CONFLICT(api_key) DO NOTHING
                "#,
            )
            .bind(key)
            .bind(STATUS_ACTIVE)
            .bind(now)
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

        if let Some(api_key) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT api_key
            FROM api_keys
            WHERE status = ?
            ORDER BY last_used_at ASC, api_key ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease { key: api_key });
        }

        if let Some(api_key) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT api_key
            FROM api_keys
            WHERE status <> ?
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                api_key ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease { key: api_key });
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

    async fn log_attempt(
        &self,
        key: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        status: Option<StatusCode>,
        error: Option<&str>,
        response_body: &[u8],
        outcome: &str,
    ) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        let status_code = status.map(|code| code.as_u16() as i64);

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key,
                method,
                path,
                query,
                status_code,
                error_message,
                result_status,
                response_body,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(key)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(status_code)
        .bind(error)
        .bind(outcome)
        .bind(response_body)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
struct ApiKeyLease {
    key: String,
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
        };
    }

    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return AttemptAnalysis {
                status: OUTCOME_UNKNOWN,
                mark_exhausted: false,
            };
        }
    };

    let mut any_success = false;
    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty() {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            messages.push(value);
        }
    }

    for message in messages {
        if let Some(outcome) = analyze_json_message(&message) {
            match outcome {
                MessageOutcome::QuotaExhausted => {
                    return AttemptAnalysis {
                        status: OUTCOME_QUOTA_EXHAUSTED,
                        mark_exhausted: true,
                    };
                }
                MessageOutcome::Error => {
                    return AttemptAnalysis {
                        status: OUTCOME_ERROR,
                        mark_exhausted: false,
                    };
                }
                MessageOutcome::Success => any_success = true,
            }
        }
    }

    if any_success {
        AttemptAnalysis {
            status: OUTCOME_SUCCESS,
            mark_exhausted: false,
        }
    } else {
        AttemptAnalysis {
            status: OUTCOME_UNKNOWN,
            mark_exhausted: false,
        }
    }
}

fn analyze_json_message(value: &Value) -> Option<MessageOutcome> {
    if value.get("error").is_some() {
        return Some(MessageOutcome::Error);
    }

    if let Some(result) = value.get("result") {
        return analyze_result_payload(result);
    }

    None
}

fn analyze_result_payload(result: &Value) -> Option<MessageOutcome> {
    if let Some(outcome) = analyze_structured_content(result) {
        return Some(outcome);
    }

    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        for item in content {
            if let Some(kind) = item.get("type").and_then(|v| v.as_str()) {
                if kind.eq_ignore_ascii_case("error") {
                    return Some(MessageOutcome::Error);
                }
            }
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                if let Some(code) = parse_embedded_status(text) {
                    return Some(classify_status_code(code));
                }
            }
        }
    }

    if result.get("error").is_some() {
        return Some(MessageOutcome::Error);
    }

    if result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some(MessageOutcome::Error);
    }

    Some(MessageOutcome::Success)
}

fn analyze_structured_content(result: &Value) -> Option<MessageOutcome> {
    let structured = result.get("structuredContent")?;

    if let Some(code) = extract_status_code(structured) {
        return Some(classify_status_code(code));
    }

    if structured
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some(MessageOutcome::Error);
    }

    structured
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    if let Some(code) = parse_embedded_status(text) {
                        return Some(classify_status_code(code));
                    }
                }
            }
            None
        })
        .or(Some(MessageOutcome::Success))
}

fn extract_status_code(value: &Value) -> Option<i64> {
    if let Some(code) = value.get("status").and_then(|v| v.as_i64()) {
        return Some(code);
    }

    if let Some(detail) = value.get("detail") {
        if let Some(code) = detail.get("status").and_then(|v| v.as_i64()) {
            return Some(code);
        }
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

    if !current.is_empty() {
        if let Ok(value) = serde_json::from_str::<Value>(&current) {
            messages.push(value);
        }
    }

    messages
}
