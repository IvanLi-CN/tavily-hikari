//! Provider REST facade contract suites (plan todos 12-14).
//!
//! Section layout: the SerpApi section (todo 12) lives in `mod serpapi`;
//! todos 13/14 append `mod exa` and `mod brave` sections to this file using
//! the same shared real-binary harness below (pattern proven by
//! tests/mcp_billing_regression.rs: real server binary + mock provider
//! binary + direct sqlite assertions).
//!
//! Every test targets stub upstreams ONLY (never production endpoints).

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

// ---------------------------------------------------------------------------
// Shared harness (all provider sections).
// ---------------------------------------------------------------------------

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

/// Wait until the startup key sync has stamped the expected number of
/// provider rows (SERPAPI_API_KEYS env bootstrap, plan todo 7).
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

async fn create_test_token(base_url: &str) -> (String, String) {
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
    let token = payload["token"].as_str().expect("token secret").to_string();
    let token_id = token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()))
        .expect("token id embedded in secret");
    (token, token_id)
}

// ---------------------------------------------------------------------------
// SerpApi REST facade section (plan todo 12).
// ---------------------------------------------------------------------------

mod serpapi {
    use super::*;

    /// Exact bodies mock_serpapi emits (mirrors src/bin/mock_serpapi.rs so the
    /// verbatim-passthrough assertions compare against the stub's contract).
    fn mock_success_body() -> Value {
        json!({
            "search_metadata": { "id": "mock", "status": "Success" },
            "search_parameters": {},
            "organic_results": [
                {
                    "position": 1,
                    "title": "Mock Result",
                    "link": "https://example.com/1",
                    "snippet": "mock snippet"
                }
            ]
        })
    }

    fn mock_invalid_key_body() -> Value {
        json!({
            "error": "Invalid API key. Your API key should be here: https://serpapi.com/manage-api-key"
        })
    }

    fn mock_run_out_body() -> Value {
        json!({ "error": "Your account has run out of searches." })
    }

    fn mock_throttled_body() -> Value {
        json!({ "error": "Your account has exceeded its hourly throughput limit." })
    }

    struct SerpapiHarness {
        base_url: String,
        token: String,
        token_id: String,
        db_path: PathBuf,
        db_str: String,
        _server: ChildGuard,
        _mock: ChildGuard,
    }

    impl Drop for SerpapiHarness {
        fn drop(&mut self) {
            remove_sqlite_files(&self.db_path);
        }
    }

    /// Spawn mock_serpapi + the real server binary (stub upstreams only).
    /// `pool_keys` seeds SERPAPI_API_KEYS; `mock_keys` are the values the
    /// mock accepts as valid (`key-exhausted` / `key-throttled` are mock
    /// built-in special keys and need no listing).
    async fn spawn_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> SerpapiHarness {
        // Port reservations race across the parallel suite; retry with fresh
        // ephemeral ports (server_http_contract respawn pattern).
        for attempt in 1..=5usize {
            if let Some(harness) = try_spawn_harness(prefix, pool_keys, mock_keys).await {
                return harness;
            }
            if attempt < 5 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        panic!("serpapi harness never became healthy after 5 spawn attempts");
    }

    async fn try_spawn_harness(
        prefix: &str,
        pool_keys: &[&str],
        mock_keys: &[&str],
    ) -> Option<SerpapiHarness> {
        let mock_port = reserve_local_port();
        let mock = Command::new(support_binaries::resolve_support_binary(
            "MOCK_SERPAPI_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_serpapi"),
        ))
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mock_serpapi");

        let port = reserve_local_port();
        let db_path = temp_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
        // Tavily env stays pointed at a dead local port: the facade tests never
        // exercise Tavily, and production endpoints must never be contacted.
        let dead_port = reserve_local_port();
        let server = Command::new(support_binaries::resolve_support_binary(
            "TAVILY_HIKARI_TEST_BIN",
            env!("CARGO_BIN_EXE_tavily-hikari"),
        ))
        .env("TAVILY_API_KEYS", "tvly-facade-contract-key")
        .env(
            "TAVILY_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env("TAVILY_USAGE_BASE", format!("http://127.0.0.1:{dead_port}"))
        .env("SERPAPI_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
        .env(
            "SERPAPI_MCP_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env("SERPAPI_API_KEYS", pool_keys.join(","))
        .env("PROXY_BIND", "127.0.0.1")
        .env("PROXY_PORT", port.to_string())
        .env("PROXY_DB_PATH", &db_str)
        .env("DEV_OPEN_ADMIN", "true")
        .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tavily-hikari");

        if !health_ready_within(port, 25).await {
            // Guards below kill both children on drop; the caller retries.
            drop(ChildGuard { child: server });
            drop(ChildGuard { child: mock });
            remove_sqlite_files(&db_path);
            return None;
        }
        wait_for_provider_keys(&db_str, "serpapi", i64::try_from(pool_keys.len()).unwrap()).await;
        let base_url = format!("http://127.0.0.1:{port}");
        let (token, token_id) = create_test_token(&base_url).await;

        Some(SerpapiHarness {
            base_url,
            token,
            token_id,
            db_path,
            db_str,
            _server: ChildGuard { child: server },
            _mock: ChildGuard { child: mock },
        })
    }

    impl SerpapiHarness {
        async fn pool(&self) -> sqlx::SqlitePool {
            connect_sqlite_test_pool(&self.db_str).await
        }

        async fn get_search_with_token(
            &self,
            token: &str,
            query: &str,
        ) -> (reqwest::StatusCode, Value) {
            let response = Client::new()
                .get(format!(
                    "{}/api/serpapi/search?api_key={token}&{query}",
                    self.base_url
                ))
                .send()
                .await
                .expect("serpapi get request");
            let status = response.status();
            (status, response.json().await.expect("json body"))
        }

        async fn get_search(&self, query: &str) -> (reqwest::StatusCode, Value) {
            self.get_search_with_token(&self.token, query).await
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (plan L189; one named test per acceptance criterion).
    // -----------------------------------------------------------------------

    /// Happy passthrough: field-identical body, pooled key arrives as
    /// `api_key`, request_logs row stamped, 1 credit charged + billing_ledger
    /// row, no x-project-id forwarded (M2).
    #[tokio::test]
    async fn serpapi_get_happy_passthrough_bills_one_credit_and_logs_row() {
        let harness = spawn_harness("facades-serpapi-happy", &["sp-pool-1"], &["sp-pool-1"]).await;

        let (status, body) = harness.get_search("q=rust&engine=google").await;
        assert_eq!(status, reqwest::StatusCode::OK);
        // Field-identical to the mock response (verbatim passthrough, no
        // transformation; M22 forbids response inspection).
        assert_eq!(body, mock_success_body());

        let pool = harness.pool().await;

        // request_logs row: provider/kind/status/legacy code column/query.
        let row = sqlx::query(
            "SELECT id, api_key_id, method, path, query, status_code, tavily_status_code, \
             result_status, request_kind_key, provider, upstream_operation, forwarded_headers, \
             dropped_headers, CAST(response_body AS TEXT) AS response_body_text \
             FROM observability.request_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(row.get::<String, _>("provider"), "serpapi");
        assert_eq!(row.get::<String, _>("path"), "/api/serpapi/search");
        assert_eq!(row.get::<String, _>("method"), "GET");
        assert_eq!(
            row.get::<String, _>("request_kind_key"),
            "api:serpapi/search"
        );
        assert_eq!(row.get::<i64, _>("status_code"), 200);
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 200);
        assert_eq!(row.get::<String, _>("result_status"), "success");
        assert_eq!(
            row.get::<Option<String>, _>("upstream_operation")
                .as_deref(),
            Some("serpapi_http_search")
        );
        assert!(row.get::<Option<String>, _>("api_key_id").is_some());

        // Client slots stripped, pooled key never persisted (M17).
        let query = row.get::<Option<String>, _>("query").unwrap_or_default();
        assert!(
            query.contains("q=rust") && query.contains("engine=google"),
            "{query}"
        );
        assert!(!query.contains("api_key"), "{query}");
        assert!(!query.contains("th-"), "{query}");
        assert!(!query.contains(&harness.token), "{query}");
        assert!(!query.contains("sp-pool-1"), "{query}");

        // M2: x-project-id never forwarded on the serpapi surface.
        let forwarded = row.get::<String, _>("forwarded_headers");
        assert!(!forwarded.contains("x-project-id"), "{forwarded}");
        let response_body_text = row
            .get::<Option<String>, _>("response_body_text")
            .unwrap_or_default();
        assert!(
            response_body_text.contains("organic_results"),
            "response body persisted per retention: {response_body_text}"
        );

        // Pooled key arrived as api_key: the mock only answers 200 with the
        // exact pooled key in its native query slot (positive proof), and the
        // persisted row proves neither token nor pooled key leaked.

        // Billing: 1 credit charged, billing_ledger row settled and linked.
        let credits: Option<i64> = sqlx::query_scalar(
            "SELECT business_credits FROM auth_token_logs WHERE token_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("token log row");
        assert_eq!(credits, Some(1));

        let log_id = row.get::<i64, _>("id");
        let ledger = sqlx::query(
            "SELECT billing_state, business_credits, request_log_id, settled_at \
             FROM billing_ledger WHERE token_id = ? ORDER BY auth_token_log_id DESC LIMIT 1",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("billing ledger row");
        assert_eq!(ledger.get::<String, _>("billing_state"), "charged");
        assert_eq!(ledger.get::<Option<i64>, _>("business_credits"), Some(1));
        assert_eq!(ledger.get::<Option<i64>, _>("request_log_id"), Some(log_id));
        assert!(ledger.get::<Option<i64>, _>("settled_at").is_some());

        let month_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0)",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("token quota row");
        assert_eq!(month_count, 1);
        pool.close().await;
    }

    /// POST convenience variant: JSON body maps onto upstream query params
    /// (upstream stays GET), a body-borne `api_key` client slot is dropped
    /// and redacted, persistence carries the mapped query.
    #[tokio::test]
    async fn serpapi_post_maps_json_body_to_query_and_redacts_client_slot() {
        let harness = spawn_harness("facades-serpapi-post", &["sp-pool-2"], &["sp-pool-2"]).await;

        let response = Client::new()
            .post(format!("{}/api/serpapi/search", harness.base_url))
            .bearer_auth(&harness.token)
            .json(&json!({
                "q": "hello world",
                "engine": "google",
                "num": 5,
                "no_cache": false,
                "json_restrictor": "organic_results",
                "api_key": "th-sneaky-body-slot"
            }))
            .send()
            .await
            .expect("serpapi post request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, mock_success_body());

        let pool = harness.pool().await;
        let row = sqlx::query(
            "SELECT method, query, CAST(request_body AS TEXT) AS request_body_text \
             FROM observability.request_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(row.get::<String, _>("method"), "POST");

        // Body fields landed as upstream query params; the client-key slot
        // never forwarded nor persisted raw.
        let query = row.get::<Option<String>, _>("query").unwrap_or_default();
        assert!(query.contains("q=hello"), "{query}");
        assert!(query.contains("engine=google"), "{query}");
        assert!(query.contains("num=5"), "{query}");
        assert!(query.contains("no_cache=false"), "{query}");
        assert!(query.contains("json_restrictor=organic_results"), "{query}");
        assert!(!query.contains("api_key"), "{query}");
        assert!(!query.contains("th-"), "{query}");
        assert!(!query.contains("sp-pool-2"), "{query}");

        // Retained request body keeps the mapped fields but redacts api_key.
        let request_body_text = row
            .get::<Option<String>, _>("request_body_text")
            .unwrap_or_default();
        assert!(
            request_body_text.contains("hello world"),
            "{request_body_text}"
        );
        assert!(
            !request_body_text.contains("th-sneaky-body-slot"),
            "client body slot persisted raw: {request_body_text}"
        );

        // Billed exactly 1 credit.
        let credits: Option<i64> = sqlx::query_scalar(
            "SELECT business_credits FROM auth_token_logs WHERE token_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("token log row");
        assert_eq!(credits, Some(1));
        pool.close().await;
    }

    /// th- tokens are redacted from every persisted log surface across GET
    /// (query slot) and POST (bearer + body slot) variants (M17).
    #[tokio::test]
    async fn serpapi_th_token_never_persisted_in_logs() {
        let harness = spawn_harness("facades-serpapi-redact", &["sp-pool-3"], &["sp-pool-3"]).await;

        let (status, _body) = harness.get_search("q=redaction").await;
        assert_eq!(status, reqwest::StatusCode::OK);
        let response = Client::new()
            .post(format!("{}/api/serpapi/search", harness.base_url))
            .header("x-api-key", "not-used-by-serpapi")
            .query(&[("api_key", harness.token.as_str())])
            .json(&json!({ "q": "redaction post", "api_key": harness.token.clone() }))
            .send()
            .await
            .expect("post request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let pool = harness.pool().await;
        for (label, sql) in [
            (
                "request_logs.query",
                "SELECT COUNT(*) FROM observability.request_logs WHERE query LIKE '%th-%' OR query LIKE ?1",
            ),
            (
                "request_logs.request_body",
                "SELECT COUNT(*) FROM observability.request_logs WHERE CAST(request_body AS TEXT) LIKE '%th-%' OR CAST(request_body AS TEXT) LIKE ?1",
            ),
            (
                "request_logs.response_body",
                "SELECT COUNT(*) FROM observability.request_logs WHERE CAST(response_body AS TEXT) LIKE '%th-%' OR CAST(response_body AS TEXT) LIKE ?1",
            ),
            (
                "request_logs.error_message",
                "SELECT COUNT(*) FROM observability.request_logs WHERE error_message LIKE '%th-%' OR error_message LIKE ?1",
            ),
            (
                "auth_token_logs.query",
                "SELECT COUNT(*) FROM auth_token_logs WHERE query LIKE '%th-%' OR query LIKE ?1",
            ),
            (
                "auth_token_logs.error_message",
                "SELECT COUNT(*) FROM auth_token_logs WHERE error_message LIKE '%th-%' OR error_message LIKE ?1",
            ),
        ] {
            let leaks: i64 = sqlx::query_scalar(sql)
                .bind(format!("%{}%", harness.token))
                .fetch_one(&pool)
                .await
                .unwrap_or(1);
            assert_eq!(leaks, 0, "{label} leaked a client token");
        }
        // The pooled upstream key is never persisted either.
        let key_leaks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.request_logs WHERE query LIKE '%sp-pool-3%' \
             OR CAST(request_body AS TEXT) LIKE '%sp-pool-3%'",
        )
        .fetch_one(&pool)
        .await
        .expect("key leak scan");
        assert_eq!(key_leaks, 0, "pooled serpapi key persisted");
        pool.close().await;
    }

    /// Upstream 401 passes through verbatim; the todo-3 classifier's
    /// AuthFailed signal feeds reconcile_key_health (key quarantined via the
    /// existing machinery), the row carries the failure kind, no credit set.
    #[tokio::test]
    async fn serpapi_upstream_401_passes_through_and_applies_auth_fail() {
        let harness = spawn_harness(
            "facades-serpapi-401",
            &["sp-invalid-at-mock"],
            &["sp-some-other-key"],
        )
        .await;

        let (status, body) = harness.get_search("q=auth").await;
        assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(body, mock_invalid_key_body(), "401 body verbatim");

        let pool = harness.pool().await;
        let row = sqlx::query(
            "SELECT result_status, failure_kind, key_effect_code, status_code, tavily_status_code \
             FROM observability.request_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(row.get::<String, _>("result_status"), "error");
        assert_eq!(
            row.get::<Option<String>, _>("failure_kind").as_deref(),
            Some("upstream_account_deactivated_401")
        );
        assert_eq!(row.get::<String, _>("key_effect_code"), "quarantined");
        assert_eq!(row.get::<i64, _>("status_code"), 401);
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 401);

        // Auth-fail applied to the key through the existing quarantine table.
        let quarantined: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines q \
             JOIN api_keys k ON k.id = q.key_id \
             WHERE k.api_key = 'sp-invalid-at-mock' AND q.cleared_at IS NULL \
             AND q.reason_code = 'invalid_api_key'",
        )
        .fetch_one(&pool)
        .await
        .expect("quarantine scan");
        assert_eq!(quarantined, 1, "401 must quarantine the pooled key");

        // No credit charged for the failed attempt.
        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger scan");
        assert_eq!(charged, 0);
        pool.close().await;
    }

    /// 429 run-out passes through verbatim; the leased key is marked
    /// exhausted; the next request leases the OTHER pooled key (200).
    #[tokio::test]
    async fn serpapi_429_run_out_marks_exhausted_and_next_request_leases_next_key() {
        let harness = spawn_harness(
            "facades-serpapi-runout",
            &["key-exhausted", "sp-good-2"],
            &["sp-good-2"],
        )
        .await;

        // Fresh keys tie on last_used_at and the id tie-break is a random
        // nanoid, so drive the LRU deterministically: a fresh token has no
        // affinity, and once the good key has been leased (touched) the
        // next fresh token MUST land on the untouched key-exhausted.
        let mut run_out_token: Option<(String, String)> = None;
        for _ in 0..3 {
            let (token, token_id) = create_test_token(&harness.base_url).await;
            let (status, body) = harness.get_search_with_token(&token, "q=runout").await;
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                assert_eq!(body, mock_run_out_body(), "run-out body verbatim");
                run_out_token = Some((token, token_id));
                break;
            }
            assert_eq!(status, reqwest::StatusCode::OK, "fresh key leases succeed");
            assert_eq!(body, mock_success_body());
        }
        let Some((run_out_token, run_out_token_id)) = run_out_token else {
            panic!("run-out 429 never observed while a special key is pooled");
        };

        let pool = harness.pool().await;
        let exhausted_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-exhausted'")
                .fetch_one(&pool)
                .await
                .expect("exhausted key row");
        assert_eq!(exhausted_status, "exhausted");

        let row = sqlx::query(
            "SELECT api_key_id, result_status, key_effect_code, failure_kind, tavily_status_code \
             FROM observability.request_logs WHERE auth_token_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&run_out_token_id)
        .fetch_one(&pool)
        .await
        .expect("429 request log row");
        assert_eq!(row.get::<String, _>("result_status"), "quota_exhausted");
        assert_eq!(row.get::<String, _>("key_effect_code"), "marked_exhausted");
        assert!(row.get::<Option<String>, _>("failure_kind").is_none());
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 429);
        let exhausted_key_id = row.get::<Option<String>, _>("api_key_id");
        assert!(exhausted_key_id.is_some());
        // The 429 row belongs to the key-exhausted pool entry.
        let exhausted_row_key: String =
            sqlx::query_scalar("SELECT api_key FROM api_keys WHERE id = ?")
                .bind(exhausted_key_id.clone().unwrap_or_default())
                .fetch_one(&pool)
                .await
                .expect("exhausted row key");
        assert_eq!(exhausted_row_key, "key-exhausted");
        pool.close().await;

        // Next request on the SAME token rebinds off the exhausted key and
        // leases the other pooled key.
        let (status2, body2) = harness
            .get_search_with_token(&run_out_token, "q=runout")
            .await;
        assert_eq!(status2, reqwest::StatusCode::OK);
        assert_eq!(body2, mock_success_body());

        let pool = harness.pool().await;
        let row2 = sqlx::query(
            "SELECT api_key_id FROM observability.request_logs WHERE auth_token_id = ? \
             ORDER BY id DESC LIMIT 1",
        )
        .bind(&run_out_token_id)
        .fetch_one(&pool)
        .await
        .expect("follow-up request log row");
        let next_key_id = row2.get::<Option<String>, _>("api_key_id");
        assert!(next_key_id.is_some());
        assert_ne!(
            next_key_id, exhausted_key_id,
            "next request must lease a different key"
        );

        // The run-out attempt billed nothing; the success billed 1.
        let charged: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(business_credits), 0) FROM billing_ledger \
             WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&run_out_token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger sum");
        assert_eq!(charged, 1);
        pool.close().await;
    }

    /// Fully quarantined pool: after a 401 auth-fail quarantines the ONLY
    /// key, the next request surfaces the Hikari NoAvailableKeys shape (502
    /// proxy_error), never a provider body. (Exhausted keys keep the house
    /// last-resort fallback lease; quarantined keys never do.)
    #[tokio::test]
    async fn serpapi_quarantined_pool_returns_502_no_available_keys_shape() {
        let harness = spawn_harness(
            "facades-serpapi-nopool",
            &["sp-invalid-at-mock"],
            &["sp-some-other-key"],
        )
        .await;

        let (status, body) = harness.get_search("q=solo").await;
        assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(body, mock_invalid_key_body());

        let response = Client::new()
            .get(format!(
                "{}/api/serpapi/search?api_key={}&q=solo",
                harness.base_url, harness.token
            ))
            .send()
            .await
            .expect("second request");
        assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("proxy_error")
        );
        assert_eq!(
            body.get("message").and_then(Value::as_str),
            Some("upstream unavailable")
        );
    }

    /// Hourly-flavor 429 (NOT run-out): verbatim passthrough, transient
    /// backoff armed, key stays active (never exhausted) — dual-429 split.
    #[tokio::test]
    async fn serpapi_hourly_throttle_429_arms_backoff_without_exhaustion() {
        let harness =
            spawn_harness("facades-serpapi-throttle", &["key-throttled"], &["sp-x"]).await;

        let (status, body) = harness.get_search("q=throttle").await;
        assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body, mock_throttled_body(), "hourly body verbatim");

        let pool = harness.pool().await;
        let key_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-throttled'")
                .fetch_one(&pool)
                .await
                .expect("throttled key row");
        assert_eq!(key_status, "active", "hourly 429 must NOT exhaust the key");

        let row = sqlx::query(
            "SELECT result_status, failure_kind, key_effect_code \
             FROM observability.request_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(row.get::<String, _>("result_status"), "error");
        assert_eq!(
            row.get::<Option<String>, _>("failure_kind").as_deref(),
            Some("upstream_rate_limited_429")
        );
        assert_eq!(
            row.get::<String, _>("key_effect_code"),
            "transient_backoff_set"
        );

        let backoff: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_transient_backoffs b \
             JOIN api_keys k ON k.id = b.key_id \
             WHERE k.api_key = 'key-throttled' AND b.scope = 'http_global' \
             AND b.reason_code = 'upstream_rate_limited_429'",
        )
        .fetch_one(&pool)
        .await
        .expect("backoff scan");
        assert_eq!(
            backoff, 1,
            "hourly 429 must arm the shared HTTP backoff scope"
        );

        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger scan");
        assert_eq!(charged, 0, "failed attempt bills nothing");
        pool.close().await;
    }

    /// Invalid POST body (non-object JSON) is rejected locally with the
    /// typed Hikari 400 shape and ZERO upstream cost / persistence.
    #[tokio::test]
    async fn serpapi_post_non_object_body_gets_typed_400() {
        let harness =
            spawn_harness("facades-serpapi-badpost", &["sp-pool-4"], &["sp-pool-4"]).await;

        let response = Client::new()
            .post(format!("{}/api/serpapi/search", harness.base_url))
            .bearer_auth(&harness.token)
            .json(&json!(["q", "array-not-object"]))
            .send()
            .await
            .expect("bad post request");
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("invalid_request")
        );

        let pool = harness.pool().await;
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
            .fetch_one(&pool)
            .await
            .expect("row count");
        assert_eq!(rows, 0, "local 400 must not hit upstream or persist rows");
        pool.close().await;
    }
}
