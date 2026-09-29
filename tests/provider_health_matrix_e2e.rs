//! Plan todo 18 — provider health wiring E2E matrix (real server binary +
//! real mock provider binaries + direct sqlite assertions; the proven
//! provider_facades_contract harness pattern). Every upstream is a stub:
//! production endpoints are NEVER contacted (AGENTS.md incident rule).
//!
//! Coverage (plan L231-232 + task gates):
//! - brave 429 → ≤2s-scale backoff row and the NEXT request leases a
//!   DIFFERENT key successfully (200) — QA happy mirror.
//! - serpapi hourly 429 → HOUR-scale backoff (retry_after_secs 3600) and the
//!   next request leases a different key (200).
//! - serpapi 401 → quarantine row + auto_quarantine maintenance record +
//!   alert projection payload carrying provider='serpapi' on the EXISTING
//!   upstream_key_blocked type (zero new alert kinds).
//! - M8 binding negative: exhaustion flows (serpapi run-out 429, exa 402
//!   tags, brave 402 QUOTA_LIMITED incl. a passive-harvest run) write ZERO
//!   api_key_low_quota_depletions rows.
//! - recovery: exa SuccessOnly success-restore, serpapi renewal-date flip,
//!   brave month-window flip — end to end through the live acquire path.
//! - FU-A path decision pin: a request from the registered native path
//!   /api/brave/res/v1/web/search logs kind api:brave/web; request_logs keeps
//!   the CANONICAL path (golden-pinned) while auth_token_logs keeps the
//!   registered full path (T14-committed bin boundary).

use std::{
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nanoid::nanoid;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[path = "common/support_binaries.rs"]
mod support_binaries;

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.db", nanoid!(8)))
}

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind random port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

fn sqlite_observability_db_path(db_path: &Path) -> PathBuf {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("sqlite");
    parent.join(format!("{stem}-observability.db"))
}

async fn connect_sqlite_test_pool(db_path: &str) -> sqlx::SqlitePool {
    let observability_db_path = sqlite_observability_db_path(Path::new(db_path));
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .after_connect(move |conn, _meta| {
            let observability_db_path = observability_db_path.clone();
            Box::pin(async move {
                let attach_sql = format!(
                    "ATTACH DATABASE '{}' AS observability",
                    observability_db_path.to_string_lossy().replace('\'', "''")
                );
                sqlx::query(&attach_sql).execute(conn).await?;
                Ok(())
            })
        })
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect sqlite pool")
}

fn remove_sqlite_files(db_path: &Path) {
    let _ = std::fs::remove_file(sqlite_observability_db_path(db_path));
    for suffix in ["", "-wal", "-shm"] {
        let mut path = db_path.as_os_str().to_os_string();
        path.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
}

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn health_ready_within(port: u16, timeout_secs: u64) -> bool {
    let client = Client::new();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if Instant::now() > deadline {
            return false;
        }
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            && response.status().is_success()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// One provider's mock + pool configuration for the shared harness.
struct ProviderCfg {
    provider: &'static str,
    mock_bin_env: &'static str,
    mock_bin_name: &'static str,
    mock_bin_exe: &'static str,
    upstream_env: &'static str,
    keys_env: &'static str,
    pool_keys: &'static [&'static str],
    mock_keys: &'static [&'static str],
}

const SERPAPI_CFG_THROTTLED: ProviderCfg = ProviderCfg {
    provider: "serpapi",
    mock_bin_env: "MOCK_SERPAPI_TEST_BIN",
    mock_bin_name: "mock_serpapi",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_serpapi"),
    upstream_env: "SERPAPI_UPSTREAM",
    keys_env: "SERPAPI_API_KEYS",
    pool_keys: &["key-throttled", "sp-live"],
    mock_keys: &["sp-live"],
};

const BRAVE_CFG_THROTTLED: ProviderCfg = ProviderCfg {
    provider: "brave",
    mock_bin_env: "MOCK_BRAVE_TEST_BIN",
    mock_bin_name: "mock_brave",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_brave"),
    upstream_env: "BRAVE_UPSTREAM",
    keys_env: "BRAVE_API_KEYS",
    pool_keys: &["key-throttled", "br-live"],
    mock_keys: &["br-live"],
};

const SERPAPI_CFG_BAD: ProviderCfg = ProviderCfg {
    provider: "serpapi",
    mock_bin_env: "MOCK_SERPAPI_TEST_BIN",
    mock_bin_name: "mock_serpapi",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_serpapi"),
    upstream_env: "SERPAPI_UPSTREAM",
    keys_env: "SERPAPI_API_KEYS",
    pool_keys: &["sp-bad"],
    mock_keys: &["sp-good"],
};

const SERPAPI_CFG_EXHAUSTED: ProviderCfg = ProviderCfg {
    provider: "serpapi",
    mock_bin_env: "MOCK_SERPAPI_TEST_BIN",
    mock_bin_name: "mock_serpapi",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_serpapi"),
    upstream_env: "SERPAPI_UPSTREAM",
    keys_env: "SERPAPI_API_KEYS",
    pool_keys: &["key-exhausted"],
    mock_keys: &["sp-good"],
};

const EXA_CFG_EXHAUSTED: ProviderCfg = ProviderCfg {
    provider: "exa",
    mock_bin_env: "MOCK_EXA_TEST_BIN",
    mock_bin_name: "mock_exa",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_exa"),
    upstream_env: "EXA_UPSTREAM",
    keys_env: "EXA_API_KEYS",
    pool_keys: &["key-exhausted"],
    mock_keys: &["ex-good"],
};

const BRAVE_CFG_HARVEST_EXHAUST: ProviderCfg = ProviderCfg {
    provider: "brave",
    mock_bin_env: "MOCK_BRAVE_TEST_BIN",
    mock_bin_name: "mock_brave",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_brave"),
    upstream_env: "BRAVE_UPSTREAM",
    keys_env: "BRAVE_API_KEYS",
    pool_keys: &["br-live", "key-exhausted"],
    mock_keys: &["br-live"],
};

const EXA_CFG_LIVE: ProviderCfg = ProviderCfg {
    provider: "exa",
    mock_bin_env: "MOCK_EXA_TEST_BIN",
    mock_bin_name: "mock_exa",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_exa"),
    upstream_env: "EXA_UPSTREAM",
    keys_env: "EXA_API_KEYS",
    pool_keys: &["ex-live"],
    mock_keys: &["ex-live"],
};

const SERPAPI_CFG_RENEWAL: ProviderCfg = ProviderCfg {
    provider: "serpapi",
    mock_bin_env: "MOCK_SERPAPI_TEST_BIN",
    mock_bin_name: "mock_serpapi",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_serpapi"),
    upstream_env: "SERPAPI_UPSTREAM",
    keys_env: "SERPAPI_API_KEYS",
    pool_keys: &["sp-renew"],
    mock_keys: &["sp-renew"],
};

const BRAVE_CFG_MONTH: ProviderCfg = ProviderCfg {
    provider: "brave",
    mock_bin_env: "MOCK_BRAVE_TEST_BIN",
    mock_bin_name: "mock_brave",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_brave"),
    upstream_env: "BRAVE_UPSTREAM",
    keys_env: "BRAVE_API_KEYS",
    pool_keys: &["br-month"],
    mock_keys: &["br-month"],
};

const BRAVE_CFG_LIVE: ProviderCfg = ProviderCfg {
    provider: "brave",
    mock_bin_env: "MOCK_BRAVE_TEST_BIN",
    mock_bin_name: "mock_brave",
    mock_bin_exe: env!("CARGO_BIN_EXE_mock_brave"),
    upstream_env: "BRAVE_UPSTREAM",
    keys_env: "BRAVE_API_KEYS",
    pool_keys: &["br-live"],
    mock_keys: &["br-live"],
};

struct HealthHarness {
    base_url: String,
    token: String,
    db_path: PathBuf,
    db_str: String,
    _server: ChildGuard,
    _mocks: Vec<ChildGuard>,
}

impl Drop for HealthHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

async fn spawn_harness(prefix: &str, providers: &[ProviderCfg]) -> HealthHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_harness(prefix, providers).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    panic!("health matrix harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_harness(prefix: &str, providers: &[ProviderCfg]) -> Option<HealthHarness> {
    let mut mocks = Vec::new();
    let mut server_env: Vec<(&'static str, String)> = Vec::new();
    for cfg in providers {
        let mock_port = reserve_local_port();
        let mock = Command::new(support_binaries::resolve_support_binary(
            cfg.mock_bin_env,
            cfg.mock_bin_exe,
        ))
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(cfg.mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|_| panic!("spawn {}", cfg.mock_bin_name));
        mocks.push(ChildGuard { child: mock });
        server_env.push((cfg.upstream_env, format!("http://127.0.0.1:{mock_port}")));
        server_env.push((cfg.keys_env, cfg.pool_keys.join(",")));
    }

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    // Tavily env stays pointed at a dead local port: this suite never
    // exercises Tavily upstreams, and production endpoints must never be
    // contacted.
    let dead_port = reserve_local_port();
    let mut command = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ));
    command
        .env("TAVILY_API_KEYS", "tvly-health-matrix-key")
        .env(
            "TAVILY_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env("TAVILY_USAGE_BASE", format!("http://127.0.0.1:{dead_port}"))
        .env(
            "SERPAPI_MCP_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env("PROXY_BIND", "127.0.0.1")
        .env("PROXY_PORT", port.to_string())
        .env("PROXY_DB_PATH", &db_str)
        .env("DEV_OPEN_ADMIN", "true")
        .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error");
    for (name, value) in &server_env {
        command.env(name, value);
    }
    let server = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tavily-hikari");

    if !health_ready_within(port, 25).await {
        drop(ChildGuard { child: server });
        drop(mocks);
        remove_sqlite_files(&db_path);
        return None;
    }
    for cfg in providers {
        wait_for_provider_keys(
            &db_str,
            cfg.provider,
            i64::try_from(cfg.pool_keys.len()).unwrap(),
        )
        .await;
    }
    let base_url = format!("http://127.0.0.1:{port}");
    let token = create_test_token(&base_url).await;

    Some(HealthHarness {
        base_url,
        token,
        db_path,
        db_str,
        _server: ChildGuard { child: server },
        _mocks: mocks,
    })
}

async fn wait_for_provider_keys(db_str: &str, provider: &str, expected: i64) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let pool = connect_sqlite_test_pool(db_str).await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE provider = ? AND deleted_at IS NULL",
        )
        .bind(provider)
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
        pool.close().await;
        if count >= expected {
            return;
        }
        if Instant::now() > deadline {
            panic!("provider '{provider}' keys never reached {expected} rows (saw {count})");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn create_test_token(base_url: &str) -> String {
    let payload = Client::new()
        .post(format!("{base_url}/api/tokens"))
        .json(&json!({}))
        .send()
        .await
        .expect("create token request")
        .error_for_status()
        .expect("create token status")
        .json::<Value>()
        .await
        .expect("decode token payload");
    payload["token"].as_str().expect("token secret").to_string()
}

impl HealthHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    /// Force a key to be the least-recently-used acquire candidate so the
    /// next lease is deterministic regardless of insertion order.
    async fn make_lru_first(&self, secret: &str) {
        let pool = self.pool().await;
        sqlx::query("UPDATE api_keys SET last_used_at = 0 WHERE api_key = ?")
            .bind(secret)
            .execute(&pool)
            .await
            .expect("force lru candidate");
        sqlx::query("UPDATE api_keys SET last_used_at = ? WHERE api_key != ?")
            .bind(1)
            .bind(secret)
            .execute(&pool)
            .await
            .expect("push siblings back");
        pool.close().await;
    }

    async fn key_id(&self, secret: &str) -> String {
        let pool = self.pool().await;
        let id: String = sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
            .bind(secret)
            .fetch_one(&pool)
            .await
            .expect("key id");
        pool.close().await;
        id
    }

    async fn key_status(&self, secret: &str) -> String {
        let pool = self.pool().await;
        let status: String = sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = ?")
            .bind(secret)
            .fetch_one(&pool)
            .await
            .expect("key status");
        pool.close().await;
        status
    }

    async fn get_serpapi(&self, query: &str) -> (reqwest::StatusCode, Value) {
        let response = Client::new()
            .get(format!(
                "{}/api/serpapi/search?api_key={}&{query}",
                self.base_url, self.token
            ))
            .send()
            .await
            .expect("serpapi request");
        let status = response.status();
        (status, response.json().await.expect("json body"))
    }

    async fn post_exa_search(&self, query: &str) -> (reqwest::StatusCode, Value) {
        let response = Client::new()
            .post(format!("{}/api/exa/search", self.base_url))
            .bearer_auth(&self.token)
            .json(&json!({ "query": query }))
            .send()
            .await
            .expect("exa request");
        let status = response.status();
        (status, response.json().await.expect("json body"))
    }

    async fn get_brave_web(&self, query: &str) -> (reqwest::StatusCode, Value) {
        self.get_brave_web_with_token(query, &self.token).await
    }

    async fn get_brave_web_with_token(
        &self,
        query: &str,
        token: &str,
    ) -> (reqwest::StatusCode, Value) {
        let response = Client::new()
            .get(format!(
                "{}/api/brave/res/v1/web/search?{query}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await
            .expect("brave request");
        let status = response.status();
        (status, response.json().await.expect("json body"))
    }

    async fn armed_backoff(&self, secret: &str) -> Option<(String, String, i64)> {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT b.scope, b.reason_code, b.retry_after_secs \
             FROM api_key_transient_backoffs b JOIN api_keys k ON k.id = b.key_id \
             WHERE k.api_key = ? AND b.cooldown_until > ? \
             ORDER BY b.cooldown_until DESC LIMIT 1",
        )
        .bind(secret)
        .bind(chrono::Utc::now().timestamp())
        .fetch_optional(&pool)
        .await
        .expect("backoff scan");
        pool.close().await;
        row.map(|row| {
            (
                row.get::<Option<String>, _>("scope").unwrap_or_default(),
                row.get::<Option<String>, _>("reason_code")
                    .unwrap_or_default(),
                row.get::<Option<i64>, _>("retry_after_secs").unwrap_or(-1),
            )
        })
    }

    async fn request_log_key_ids(&self) -> Vec<(String, i64, String)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT COALESCE(api_key_id, '') AS leased_key_id, status_code, path \
             FROM observability.request_logs ORDER BY id ASC",
        )
        .fetch_all(&pool)
        .await
        .expect("request log scan");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("leased_key_id"),
                    row.get::<Option<i64>, _>("status_code").unwrap_or(-1),
                    row.get::<String, _>("path"),
                )
            })
            .collect()
    }

    async fn depletion_rows(&self) -> i64 {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_key_low_quota_depletions")
            .fetch_one(&pool)
            .await
            .expect("depletion count");
        pool.close().await;
        count
    }

    async fn maintenance_count(&self, secret: &str, operation_code: &str) -> i64 {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_maintenance_records m \
             JOIN api_keys k ON k.id = m.key_id \
             WHERE k.api_key = ? AND m.operation_code = ?",
        )
        .bind(secret)
        .bind(operation_code)
        .fetch_one(&pool)
        .await
        .expect("maintenance count");
        pool.close().await;
        count
    }

    /// Poll the alert projection until one payload matches ALL needles
    /// (server-side projection lane runs every 10-20s; bounded wait).
    async fn wait_for_projection_payload(&self, needles: &[&str]) -> String {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let pool = self.pool().await;
            let payloads: Vec<String> = sqlx::query_scalar(
                "SELECT payload_json FROM observability.dashboard_alert_projection_events",
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
            pool.close().await;
            if let Some(payload) = payloads
                .iter()
                .find(|payload| needles.iter().all(|needle| payload.contains(needle)))
            {
                return payload.clone();
            }
            if Instant::now() > deadline {
                panic!(
                    "alert projection never carried {needles:?}; last dump: {}",
                    payloads.join("\n")
                );
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Given a two-key serpapi pool where the LRU-first key is the mock's hourly
/// 429 knob; When request 1 hits the hourly throttle and request 2 follows;
/// Then request 1 arms an HOUR-scale backoff on the shared http_global scope
/// (D6 "hour scope") and request 2 leases the DIFFERENT healthy key (200).
#[tokio::test]
async fn serpapi_hourly_429_arms_hour_scale_backoff_and_next_request_leases_a_different_key() {
    let harness = spawn_harness("health-serpapi-hourly", &[SERPAPI_CFG_THROTTLED]).await;
    harness.make_lru_first("key-throttled").await;

    let (status, body) = harness.get_serpapi("q=throttle-probe").await;
    assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body,
        json!({ "error": "Your account has exceeded its hourly throughput limit." }),
        "hourly 429 body verbatim"
    );

    let (scope, reason, retry_after) = harness
        .armed_backoff("key-throttled")
        .await
        .expect("hour-scale backoff armed");
    assert_eq!(scope, "http_global", "shared scope column stays pinned");
    assert_eq!(reason, "upstream_rate_limited_429");
    assert_eq!(
        retry_after, 3600,
        "SerpApi hourly 429 sizes to the hour scale"
    );
    assert_eq!(harness.key_status("key-throttled").await, "active");

    let (status2, _) = harness.get_serpapi("q=throttle-probe-2").await;
    assert_eq!(status2, reqwest::StatusCode::OK, "next request succeeds");
    let rows = harness.request_log_key_ids().await;
    assert_eq!(rows.len(), 2, "both attempts logged");
    let throttled_id = harness.key_id("key-throttled").await;
    let live_id = harness.key_id("sp-live").await;
    assert_eq!(
        rows[0].0, throttled_id,
        "request 1 leased the throttled key"
    );
    assert_eq!(
        rows[1].0, live_id,
        "request 2 leased the DIFFERENT healthy key"
    );
    assert_eq!(rows[1].1, 200);
}

/// Given a two-key brave pool with the mock's 429 knob LRU-first; When
/// request 1 is rate limited and request 2 follows; Then request 1 arms a
/// ≤2s-scale backoff (retry_after_secs=2 on scope http_global) and request 2
/// leases the DIFFERENT key successfully (plan L232 QA happy mirror).
#[tokio::test]
async fn brave_429_arms_second_scale_backoff_and_next_request_leases_a_different_key() {
    let harness = spawn_harness("health-brave-second", &[BRAVE_CFG_THROTTLED]).await;
    harness.make_lru_first("key-throttled").await;

    let (status, body) = harness.get_brave_web("q=brave-throttle-probe").await;
    assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body.get("error").and_then(|error| error.get("code")),
        Some(&json!("RATE_LIMITED")),
        "brave 429 envelope verbatim"
    );

    let (scope, reason, retry_after) = harness
        .armed_backoff("key-throttled")
        .await
        .expect("second-scale backoff armed");
    assert_eq!(scope, "http_global");
    assert_eq!(reason, "upstream_rate_limited_429");
    assert!(
        retry_after <= 2,
        "brave backoff stays second-scale: {retry_after}"
    );
    assert_eq!(harness.key_status("key-throttled").await, "active");

    let (status2, _) = harness.get_brave_web("q=brave-throttle-probe-2").await;
    assert_eq!(status2, reqwest::StatusCode::OK, "next request succeeds");
    let rows = harness.request_log_key_ids().await;
    assert_eq!(rows.len(), 2);
    let throttled_id = harness.key_id("key-throttled").await;
    let live_id = harness.key_id("br-live").await;
    assert_eq!(rows[0].0, throttled_id);
    assert_eq!(
        rows[1].0, live_id,
        "request 2 leased the DIFFERENT healthy key"
    );
    assert_eq!(rows[1].1, 200);
}

/// Given a pooled serpapi key the mock rejects as invalid; When a search
/// request flows; Then the key is quarantined with an auto_quarantine
/// maintenance audit row, the request_logs row carries key_effect
/// 'quarantined' + provider 'serpapi', and the alert projection exposes the
/// EXISTING upstream_key_blocked type with provider='serpapi' (plan L232 QA
/// failure mirror; zero new alert kinds).
#[tokio::test]
async fn serpapi_401_quarantines_with_maintenance_row_and_provider_alert() {
    let harness = spawn_harness("health-serpapi-authfail", &[SERPAPI_CFG_BAD]).await;

    let (status, _) = harness.get_serpapi("q=auth-fail-probe").await;
    assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);

    let pool = harness.pool().await;
    let key_id = harness.key_id("sp-bad").await;
    let quarantine =
        sqlx::query("SELECT reason_code, cleared_at FROM api_key_quarantines WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&pool)
            .await
            .expect("quarantine row");
    assert_eq!(
        quarantine.get::<String, _>("reason_code"),
        "invalid_api_key",
        "mirrors the Tavily auth-fail quarantine reason"
    );
    assert!(quarantine.get::<Option<i64>, _>("cleared_at").is_none());
    pool.close().await;

    assert_eq!(
        harness.maintenance_count("sp-bad", "auto_quarantine").await,
        1,
        "auto_quarantine audit row present"
    );

    let pool = harness.pool().await;
    let log_row = sqlx::query(
        "SELECT provider, key_effect_code, result_status FROM observability.request_logs \
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(
        log_row.get::<Option<String>, _>("provider").as_deref(),
        Some("serpapi")
    );
    assert_eq!(log_row.get::<String, _>("key_effect_code"), "quarantined");
    assert_eq!(log_row.get::<String, _>("result_status"), "error");
    pool.close().await;

    let payload = harness
        .wait_for_projection_payload(&["upstream_key_blocked", "\"provider\":\"serpapi\""])
        .await;
    assert!(
        payload.contains(&key_id) || payload.contains("sp-bad"),
        "alert payload references the quarantined key: {payload}"
    );
}

/// M8 binding negative, SerpApi leg: a run-out 429 exhausts the key with its
/// audit row and writes ZERO api_key_low_quota_depletions rows.
#[tokio::test]
async fn serpapi_run_out_exhausts_without_depletion_rows() {
    let harness = spawn_harness("health-serpapi-m8", &[SERPAPI_CFG_EXHAUSTED]).await;

    let (status, body) = harness.get_serpapi("q=run-out-probe").await;
    assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body,
        json!({ "error": "Your account has run out of searches." }),
        "run-out body verbatim"
    );
    assert_eq!(harness.key_status("key-exhausted").await, "exhausted");
    assert_eq!(
        harness
            .maintenance_count("key-exhausted", "auto_mark_exhausted")
            .await,
        1
    );
    assert_eq!(
        harness.depletion_rows().await,
        0,
        "M8: serpapi exhaustion never writes depletion rows"
    );
}

/// M8 binding negative, Exa leg: a 402 NO_MORE_CREDITS exhausts the key and
/// writes zero depletion rows.
#[tokio::test]
async fn exa_402_exhausts_without_depletion_rows() {
    let harness = spawn_harness("health-exa-m8", &[EXA_CFG_EXHAUSTED]).await;

    let (status, body) = harness.post_exa_search("m8 exhaustion probe").await;
    assert_eq!(status, reqwest::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        body.get("tag").and_then(Value::as_str),
        Some("NO_MORE_CREDITS")
    );
    assert_eq!(harness.key_status("key-exhausted").await, "exhausted");
    assert_eq!(
        harness
            .maintenance_count("key-exhausted", "auto_mark_exhausted")
            .await,
        1
    );
    assert_eq!(
        harness.depletion_rows().await,
        0,
        "M8: exa exhaustion never writes depletion rows"
    );
}

/// M8 binding negative, Brave leg incl. the passive-harvest flow: request 1
/// succeeds against the live key and harvests the month window (samples
/// written, quota fields updated — depletion table still untouched), then
/// request 2 exhausts via the 402 knob with zero depletion rows.
#[tokio::test]
async fn brave_harvest_then_exhaustion_writes_zero_depletion_rows() {
    let harness = spawn_harness("health-brave-m8", &[BRAVE_CFG_HARVEST_EXHAUST]).await;
    harness.make_lru_first("br-live").await;

    let (status, _) = harness.get_brave_web("q=harvest-probe").await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let pool = harness.pool().await;
    let live_id = harness.key_id("br-live").await;
    let samples: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quota_sync_samples WHERE key_id = ? AND source = 'brave_passive'",
    )
    .bind(&live_id)
    .fetch_one(&pool)
    .await
    .expect("harvest sample count");
    assert!(samples >= 1, "passive harvest ran on the success path");
    let remaining: Option<i64> =
        sqlx::query_scalar("SELECT quota_remaining FROM api_keys WHERE id = ?")
            .bind(&live_id)
            .fetch_one(&pool)
            .await
            .expect("harvested quota");
    assert!(remaining.is_some(), "harvest updated quota_remaining");
    pool.close().await;
    assert_eq!(harness.depletion_rows().await, 0, "M8 after harvest");

    // Second token: a fresh token has no primary affinity, so the LRU-first
    // exhausted knob is leased by tier 1/2 and its 402 lands (the first
    // token's primary affinity would re-lease br-live instead).
    let exhaust_token = create_test_token(&harness.base_url).await;
    harness.make_lru_first("key-exhausted").await;
    let (status2, body2) = harness
        .get_brave_web_with_token("q=exhaust-probe", &exhaust_token)
        .await;
    assert_eq!(status2, reqwest::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        body2.get("error").and_then(|error| error.get("code")),
        Some(&json!("QUOTA_LIMITED"))
    );
    assert_eq!(harness.key_status("key-exhausted").await, "exhausted");
    assert_eq!(
        harness
            .maintenance_count("key-exhausted", "auto_mark_exhausted")
            .await,
        1
    );
    assert_eq!(
        harness.depletion_rows().await,
        0,
        "M8: brave exhaustion + harvest never writes depletion rows"
    );
}

/// Exa SuccessOnly recovery end to end: an exhausted exa key re-leased via
/// the tier-2 fallback flips back to active on the next successful call with
/// an auto_restore_active audit row (no calendar involved).
#[tokio::test]
async fn exa_success_restores_exhausted_key_end_to_end() {
    let harness = spawn_harness("health-exa-restore", &[EXA_CFG_LIVE]).await;

    let pool = harness.pool().await;
    sqlx::query(
        "UPDATE api_keys SET status = 'exhausted', status_changed_at = ? WHERE api_key = 'ex-live'",
    )
    .bind(chrono::Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("seed exhausted exa key");
    pool.close().await;

    let (status, _) = harness.post_exa_search("success restore probe").await;
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "tier-2 fallback leases the exhausted key"
    );
    assert_eq!(harness.key_status("ex-live").await, "active");
    assert_eq!(
        harness
            .maintenance_count("ex-live", "auto_restore_active")
            .await,
        1,
        "success-restore audit row (Exa SuccessOnly completes here)"
    );
}

/// Recovery calendar policies end to end through the live acquire path:
/// SerpApi flips exhausted→active once quota_renews_at has passed
/// (RenewalDate, T9 policy — pinned, not reimplemented) and Brave flips at
/// the UTC month window (RateLimitResetWindow→UtcMonth pool default).
#[tokio::test]
async fn serpapi_renewal_flip_and_brave_month_window_recover_end_to_end() {
    let harness = spawn_harness(
        "health-recovery-flips",
        &[SERPAPI_CFG_RENEWAL, BRAVE_CFG_MONTH],
    )
    .await;

    let now = chrono::Utc::now().timestamp();
    let pool = harness.pool().await;
    sqlx::query(
        "UPDATE api_keys SET status = 'exhausted', status_changed_at = ?, quota_renews_at = ? \
         WHERE api_key = 'sp-renew'",
    )
    .bind(now - 86_400)
    .bind(now - 3_600)
    .execute(&pool)
    .await
    .expect("seed exhausted serpapi key past renewal");
    sqlx::query(
        "UPDATE api_keys SET status = 'exhausted', status_changed_at = ? WHERE api_key = 'br-month'",
    )
    .bind(now - 70 * 86_400)
    .execute(&pool)
    .await
    .expect("seed exhausted brave key from a prior month window");
    pool.close().await;

    let (status, _) = harness.get_serpapi("q=renewal-flip-probe").await;
    assert_eq!(status, reqwest::StatusCode::OK, "renewal-passed key leased");
    assert_eq!(
        harness.key_status("sp-renew").await,
        "active",
        "serpapi renewal-date flip (T9 RenewalDate policy, pinned E2E)"
    );

    let (status, _) = harness.get_brave_web("q=month-flip-probe").await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(
        harness.key_status("br-month").await,
        "active",
        "brave month-window flip (T9 policy, pinned E2E)"
    );
}

/// FU-A path decision pin: requesting the REGISTERED native brave path logs
/// kind api:brave/web; request_logs keeps the canonical path (golden-pinned
/// FORBIDDEN-file contract) while auth_token_logs keeps the registered full
/// path — kinds and buckets stay correct on both surfaces.
#[tokio::test]
async fn brave_registered_full_path_logs_canonical_kind_with_full_path_on_token_logs() {
    let harness = spawn_harness("health-brave-fu-a", &[BRAVE_CFG_LIVE]).await;

    let (status, _) = harness.get_brave_web("q=fu-a-path-probe").await;
    assert_eq!(status, reqwest::StatusCode::OK);

    let pool = harness.pool().await;
    let log_row = sqlx::query(
        "SELECT path, request_kind_key, request_kind_label, provider \
         FROM observability.request_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(
        log_row.get::<String, _>("path"),
        "/api/brave/web/search",
        "request_logs keeps the canonical kind path (golden-pinned)"
    );
    assert_eq!(
        log_row
            .get::<Option<String>, _>("request_kind_key")
            .as_deref(),
        Some("api:brave/web")
    );
    assert_eq!(
        log_row
            .get::<Option<String>, _>("request_kind_label")
            .as_deref(),
        Some("API | brave/web")
    );
    assert_eq!(
        log_row.get::<Option<String>, _>("provider").as_deref(),
        Some("brave")
    );

    let token_row = sqlx::query(
        "SELECT path, request_kind_key, provider FROM auth_token_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("token log row");
    assert_eq!(
        token_row.get::<String, _>("path"),
        "/api/brave/res/v1/web/search",
        "auth_token_logs keeps the registered native path (T14 contract)"
    );
    assert_eq!(
        token_row
            .get::<Option<String>, _>("request_kind_key")
            .as_deref(),
        Some("api:brave/web"),
        "registered path logs the canonical kind"
    );
    pool.close().await;
}
