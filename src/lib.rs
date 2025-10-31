use std::sync::Arc;

use chrono::{Datelike, TimeZone, Utc};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{QueryBuilder, SqlitePool};
use thiserror::Error;

/// Default Tavily search endpoint.
pub const DEFAULT_SEARCH_ENDPOINT: &str = "https://api.tavily.com/search";

/// Client that balances requests across multiple Tavily API keys while persisting
/// key health in SQLite.
#[derive(Clone, Debug)]
pub struct TavilyBalancer {
    client: Client,
    endpoint: Url,
    key_store: Arc<KeyStore>,
}

impl TavilyBalancer {
    /// Build a new balancer targeting the default Tavily endpoint.
    pub async fn new<I, S>(keys: I, database_path: &str) -> Result<Self, TavilyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_endpoint(keys, DEFAULT_SEARCH_ENDPOINT, database_path).await
    }

    /// Build a new balancer using a custom Tavily-compatible endpoint.
    pub async fn with_endpoint<I, S>(
        keys: I,
        endpoint: &str,
        database_path: &str,
    ) -> Result<Self, TavilyError>
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
            return Err(TavilyError::EmptyKeySet);
        }

        let key_store = KeyStore::new(&sanitized, database_path).await?;
        let endpoint = Url::parse(endpoint).map_err(|source| TavilyError::InvalidEndpoint {
            endpoint: endpoint.to_owned(),
            source,
        })?;

        Ok(Self {
            client: Client::new(),
            endpoint,
            key_store: Arc::new(key_store),
        })
    }

    /// Exposes the configured endpoint.
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Performs a Tavily search request using the next key in the pool. Each
    /// invocation is logged to SQLite for downstream analysis. When Tavily
    /// reports quota exhaustion (status 432), the selected key is marked as
    /// temporarily disabled until the next monthly reset.
    pub async fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<serde_json::Value, TavilyError> {
        #[derive(Serialize)]
        struct SearchPayload<'a> {
            api_key: &'a str,
            query: &'a str,
            #[serde(flatten)]
            options: &'a SearchOptions,
        }

        let lease = self.key_store.acquire_key().await?;
        let payload = SearchPayload {
            api_key: &lease.key,
            query,
            options,
        };

        let options_json = serde_json::to_string(options).map_err(TavilyError::Serialization)?;

        let response = self
            .client
            .post(self.endpoint.clone())
            .json(&payload)
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                let status_err = response.error_for_status_ref().err();
                let status_err_message = status_err.as_ref().map(|err| err.to_string());
                let body_text = response.text().await.map_err(TavilyError::Http)?;

                self.key_store
                    .log_attempt(
                        &lease.key,
                        query,
                        &options_json,
                        Some(status),
                        status_err_message.as_deref(),
                        Some(&body_text),
                    )
                    .await?;

                if let Some(err) = status_err {
                    if is_quota_exhausted(status) {
                        self.key_store.mark_quota_exhausted(&lease.key).await?;
                    }
                    return Err(TavilyError::Http(err));
                }

                Ok(serde_json::from_str(&body_text).map_err(TavilyError::Serialization)?)
            }
            Err(err) => {
                self.key_store
                    .log_attempt(
                        &lease.key,
                        query,
                        &options_json,
                        None,
                        Some(&err.to_string()),
                        None,
                    )
                    .await?;
                Err(TavilyError::Http(err))
            }
        }
    }
}

#[derive(Debug, Clone)]
struct KeyLease {
    key: String,
}

#[derive(Debug, Clone)]
struct KeyStore {
    pool: SqlitePool,
}

impl KeyStore {
    async fn new(keys: &[String], database_path: &str) -> Result<Self, TavilyError> {
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

    async fn initialize_schema(&self) -> Result<(), TavilyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                api_key TEXT PRIMARY KEY,
                disabled_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key TEXT NOT NULL,
                query TEXT NOT NULL,
                options_json TEXT NOT NULL,
                status_code INTEGER,
                error_message TEXT,
                response_body TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key) REFERENCES api_keys(api_key)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn sync_keys(&self, keys: &[String]) -> Result<(), TavilyError> {
        let mut tx = self.pool.begin().await?;

        for key in keys {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO api_keys (api_key)
                VALUES (?)
                "#,
            )
            .bind(key)
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

    async fn acquire_key(&self) -> Result<KeyLease, TavilyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();

        if let Some(api_key) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT api_key
            FROM api_keys
            WHERE disabled_at IS NULL
            ORDER BY last_used_at ASC, api_key ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(KeyLease { key: api_key });
        }

        if let Some(api_key) = sqlx::query_scalar::<_, String>(
            r#"
            SELECT api_key
            FROM api_keys
            WHERE disabled_at IS NOT NULL
            ORDER BY disabled_at ASC, api_key ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(KeyLease { key: api_key });
        }

        Err(TavilyError::NoAvailableKeys)
    }

    async fn reset_monthly(&self) -> Result<(), TavilyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled_at = NULL
            WHERE disabled_at IS NOT NULL AND disabled_at < ?
            "#,
        )
        .bind(month_start)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn touch_key(&self, key: &str, timestamp: i64) -> Result<(), TavilyError> {
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
        query: &str,
        options_json: &str,
        status: Option<StatusCode>,
        error_message: Option<&str>,
        response_body: Option<&str>,
    ) -> Result<(), TavilyError> {
        let status_code = status.map(|code| code.as_u16() as i64);
        let created_at = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key,
                query,
                options_json,
                status_code,
                error_message,
                response_body,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(key)
        .bind(query)
        .bind(options_json)
        .bind(status_code)
        .bind(error_message)
        .bind(response_body)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn mark_quota_exhausted(&self, key: &str) -> Result<(), TavilyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET disabled_at = ?, last_used_at = ?
            WHERE api_key = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn start_of_month(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
}

fn is_quota_exhausted(status: StatusCode) -> bool {
    status.as_u16() == 432
}

/// Tavily search options corresponding to documented request fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_depth: Option<SearchDepth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_answer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_raw_content: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
}

/// Enumeration of supported Tavily search depths.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SearchDepth {
    Basic,
    Advanced,
}

/// Errors returned by [`TavilyBalancer`].
#[derive(Debug, Error)]
pub enum TavilyError {
    #[error("no API keys provided")]
    EmptyKeySet,
    #[error("invalid API endpoint '{endpoint}': {source}")]
    InvalidEndpoint {
        endpoint: String,
        #[source]
        source: url::ParseError,
    },
    #[error("no API keys available in the store")]
    NoAvailableKeys,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("http request failed: {0}")]
    Http(reqwest::Error),
}
