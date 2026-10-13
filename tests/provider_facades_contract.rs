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
    // Same documented transient as provider_mcp_e2e: the post-listen
    // maintenance burst can saturate the small SQLite pool, so admin
    // mutations answer 503 (Retry-After when set). Ride it out with a
    // bounded retry instead of a single-shot panic; see
    // .omo/evidence/e5-regression-debug.log for the upstream byte-identity
    // proof and flake-rate measurements.
    let client = Client::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    let payload = loop {
        let response = client
            .post(format!("{base_url}/api/tokens"))
            .json(&json!({}))
            .send()
            .await
            .expect("create token request");
        let status = response.status();
        if status.as_u16() == 503 && Instant::now() < deadline {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|secs| secs.parse::<u64>().ok())
                .map(|secs| Duration::from_secs(secs.min(2)))
                .unwrap_or(Duration::from_millis(400));
            tokio::time::sleep(wait).await;
            continue;
        }
        break response
            .error_for_status()
            .expect("create token status")
            .json::<Value>()
            .await
            .expect("decode token payload");
    };
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

// ---------------------------------------------------------------------------
// Exa REST facade section (plan todo 13).
// ---------------------------------------------------------------------------

mod exa {
    use super::*;

    /// Exact bodies mock_exa emits (mirrors src/bin/mock_exa.rs so the
    /// verbatim-passthrough assertions compare against the stub's contract,
    /// costDollars/requestId/statuses included).
    fn mock_search_body() -> Value {
        json!({
            "requestId": "mock",
            "results": [
                {
                    "title": "Mock",
                    "url": "https://example.com/1",
                    "text": "mock text",
                    "publishedDate": "2026-01-01T00:00:00.000Z"
                }
            ],
            "costDollars": { "total": 0.007 },
            "searchTime": 123.4
        })
    }

    fn mock_contents_body() -> Value {
        json!({
            "results": [
                { "title": "Mock", "url": "https://example.com/1", "text": "mock text" }
            ],
            "statuses": [
                { "id": "https://example.com/1", "status": "success", "source": "cached" }
            ]
        })
    }

    fn mock_answer_body() -> Value {
        json!({
            "requestId": "mock",
            "answer": "mock answer",
            "citations": [{ "url": "https://example.com/1" }],
            "costDollars": { "total": 0.005 }
        })
    }

    fn mock_invalid_key_body() -> Value {
        json!({
            "requestId": "mock",
            "error": "Invalid API key",
            "tag": "INVALID_API_KEY"
        })
    }

    fn mock_no_more_credits_body() -> Value {
        json!({
            "requestId": "mock",
            "error": "You have no more credits",
            "tag": "NO_MORE_CREDITS"
        })
    }

    fn mock_rate_limited_body() -> Value {
        json!({
            "requestId": "mock",
            "error": "Rate limit exceeded",
            "tag": "RATE_LIMIT_EXCEEDED"
        })
    }

    /// Documented todo-13 boundary message (typed 400, pre-upstream).
    const STREAM_REJECT_MESSAGE: &str = "streaming not supported; omit stream or set false";

    struct ExaHarness {
        base_url: String,
        mock_base_url: String,
        token: String,
        token_id: String,
        db_path: PathBuf,
        db_str: String,
        _server: ChildGuard,
        _mock: ChildGuard,
    }

    impl Drop for ExaHarness {
        fn drop(&mut self) {
            remove_sqlite_files(&self.db_path);
        }
    }

    /// Spawn mock_exa + the real server binary (stub upstreams ONLY; every
    /// non-Exa provider upstream points at a dead local port so production
    /// endpoints are unreachable by construction).
    async fn spawn_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> ExaHarness {
        // Port reservations race across the parallel suite; retry with fresh
        // ephemeral ports (serpapi section pattern).
        for attempt in 1..=5usize {
            if let Some(harness) = try_spawn_harness(prefix, pool_keys, mock_keys).await {
                return harness;
            }
            if attempt < 5 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        panic!("exa harness never became healthy after 5 spawn attempts");
    }

    async fn try_spawn_harness(
        prefix: &str,
        pool_keys: &[&str],
        mock_keys: &[&str],
    ) -> Option<ExaHarness> {
        let mock_port = reserve_local_port();
        let mock = Command::new(support_binaries::resolve_support_binary(
            "MOCK_EXA_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_exa"),
        ))
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mock_exa");

        let port = reserve_local_port();
        let db_path = temp_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
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
        .env("EXA_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
        .env("EXA_API_KEYS", pool_keys.join(","))
        .env(
            "SERPAPI_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}"),
        )
        .env(
            "SERPAPI_MCP_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env(
            "BRAVE_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}"),
        )
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
        wait_for_provider_keys(&db_str, "exa", i64::try_from(pool_keys.len()).unwrap()).await;
        let base_url = format!("http://127.0.0.1:{port}");
        let mock_base_url = format!("http://127.0.0.1:{mock_port}");
        let (token, token_id) = create_test_token(&base_url).await;

        Some(ExaHarness {
            base_url,
            mock_base_url,
            token,
            token_id,
            db_path,
            db_str,
            _server: ChildGuard { child: server },
            _mock: ChildGuard { child: mock },
        })
    }

    impl ExaHarness {
        async fn pool(&self) -> sqlx::SqlitePool {
            connect_sqlite_test_pool(&self.db_str).await
        }

        /// POST one facade path with the harness token (universal Bearer
        /// slot); returns status + response headers + decoded JSON body.
        async fn post_facade(
            &self,
            path: &str,
            body: &Value,
        ) -> (reqwest::StatusCode, reqwest::header::HeaderMap, Value) {
            self.post_facade_with_token(path, &self.token, body)
                .await
        }

        async fn post_facade_with_token(
            &self,
            path: &str,
            token: &str,
            body: &Value,
        ) -> (reqwest::StatusCode, reqwest::header::HeaderMap, Value) {
            let response = Client::new()
                .post(format!("{}{path}", self.base_url))
                .bearer_auth(token)
                .json(body)
                .send()
                .await
                .expect("exa facade request");
            let status = response.status();
            let headers = response.headers().clone();
            (status, headers, response.json().await.expect("json body"))
        }

        /// mock_exa per-endpoint call counters (zero-cost proof source).
        async fn mock_metrics(&self) -> Value {
            Client::new()
                .get(format!("{}/metrics", self.mock_base_url))
                .send()
                .await
                .expect("mock metrics request")
                .json::<Value>()
                .await
                .expect("mock metrics body")
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (plan L196; one named test per acceptance criterion).
    // -----------------------------------------------------------------------

    /// Happy passthrough for all three endpoints: field-identical bodies
    /// (costDollars/requestId/statuses included), provider='exa' rows with
    /// api:exa/{search,contents,answer} kinds, and credit tiers 1/1/2 from
    /// the committed todo-2 registry charged with settled billing_ledger
    /// rows.
    #[tokio::test]
    async fn exa_happy_passthrough_bills_credit_tiers_and_logs_rows() {
        let harness = spawn_harness("facades-exa-happy", &["ex-pool-1"], &["ex-pool-1"]).await;

        let expectations = [
            (
                "/api/exa/search",
                json!({ "query": "rust", "numResults": 3 }),
                mock_search_body(),
                "api:exa/search",
                "exa_http_search",
                1i64,
            ),
            (
                "/api/exa/contents",
                json!({ "urls": ["https://example.com/1"] }),
                mock_contents_body(),
                "api:exa/contents",
                "exa_http_contents",
                1,
            ),
            (
                "/api/exa/answer",
                json!({ "query": "why", "type": "fast" }),
                mock_answer_body(),
                "api:exa/answer",
                "exa_http_answer",
                2,
            ),
        ];
        for (path, request_body, expected, kind_key, operation, credits) in expectations {
            // Fresh token per endpoint so the ledger/quota asserts are exact.
            let (token, token_id) = create_test_token(&harness.base_url).await;
            let (status, _headers, body) = harness
                .post_facade_with_token(path, &token, &request_body)
                .await;
            assert_eq!(status, reqwest::StatusCode::OK, "{path}");
            // Field-identical to the mock response (verbatim passthrough, no
            // transformation).
            assert_eq!(body, expected, "{path} body must pass through verbatim");

            let pool = harness.pool().await;
            let row = sqlx::query(
                "SELECT id, api_key_id, method, path, status_code, tavily_status_code, \
                  result_status, request_kind_key, provider, upstream_operation, \
                  CAST(request_body AS TEXT) AS request_body_text \
                  FROM observability.request_logs WHERE auth_token_id = ? \
                  ORDER BY id DESC LIMIT 1",
            )
            .bind(&token_id)
            .fetch_one(&pool)
            .await
            .expect("request log row");
            assert_eq!(row.get::<String, _>("provider"), "exa", "{path}");
            assert_eq!(row.get::<String, _>("path"), path, "{path}");
            assert_eq!(row.get::<String, _>("method"), "POST", "{path}");
            assert_eq!(row.get::<String, _>("request_kind_key"), kind_key, "{path}");
            assert_eq!(row.get::<i64, _>("status_code"), 200, "{path}");
            // Legacy column carries the upstream HTTP status verbatim.
            assert_eq!(row.get::<i64, _>("tavily_status_code"), 200, "{path}");
            assert_eq!(row.get::<String, _>("result_status"), "success", "{path}");
            assert_eq!(
                row.get::<Option<String>, _>("upstream_operation").as_deref(),
                Some(operation),
                "{path}"
            );
            assert!(row.get::<Option<String>, _>("api_key_id").is_some());
            let request_body_text = row
                .get::<Option<String>, _>("request_body_text")
                .unwrap_or_default();
            assert!(!request_body_text.contains("ex-pool-1"), "{path}");
            assert!(!request_body_text.contains("th-"), "{path}");

            // Credit tier from the registry table charged + settled ledger row.
            let log_id = row.get::<i64, _>("id");
            let ledger = sqlx::query(
                "SELECT billing_state, business_credits, request_log_id, settled_at \
                  FROM billing_ledger WHERE token_id = ? \
                  ORDER BY auth_token_log_id DESC LIMIT 1",
            )
            .bind(&token_id)
            .fetch_one(&pool)
            .await
            .expect("billing ledger row");
            assert_eq!(ledger.get::<String, _>("billing_state"), "charged", "{path}");
            assert_eq!(
                ledger.get::<Option<i64>, _>("business_credits"),
                Some(credits),
                "{path} credit tier"
            );
            assert_eq!(
                ledger.get::<Option<i64>, _>("request_log_id"),
                Some(log_id),
                "{path}"
            );
            assert!(ledger.get::<Option<i64>, _>("settled_at").is_some());

            let month_count: i64 = sqlx::query_scalar(
                "SELECT COALESCE((SELECT month_count FROM auth_token_quota \
                  WHERE token_id = ? LIMIT 1), 0)",
            )
            .bind(&token_id)
            .fetch_one(&pool)
            .await
            .expect("token quota row");
            assert_eq!(month_count, credits, "{path} month_count");
            pool.close().await;
        }

        // One upstream call per endpoint (the counters prove the calls hit
        // the mock exactly once each).
        assert_eq!(
            harness.mock_metrics().await,
            json!({ "search": 1, "contents": 1, "answer": 1 })
        );
    }

    /// stream:true — and ANY truthy stream value — is a typed 400 Hikari
    /// rejection BEFORE any upstream call: mock counters UNCHANGED (zero
    /// upstream cost), zero persisted rows, zero billing. stream:false and
    /// an omitted stream pass through verbatim.
    #[tokio::test]
    async fn exa_truthy_stream_gets_typed_400_with_zero_upstream_cost() {
        let harness =
            spawn_harness("facades-exa-stream", &["ex-pool-stream"], &["ex-pool-stream"]).await;

        let before = harness.mock_metrics().await;
        assert_eq!(before, json!({ "search": 0, "contents": 0, "answer": 0 }));

        for (path, body) in [
            ("/api/exa/answer", json!({ "query": "x", "stream": true })),
            ("/api/exa/answer", json!({ "query": "x", "stream": "always" })),
            ("/api/exa/answer", json!({ "query": "x", "stream": 1 })),
            ("/api/exa/search", json!({ "query": "y", "stream": "auto" })),
            ("/api/exa/contents", json!({ "urls": ["u"], "stream": true })),
        ] {
            let (status, _headers, response_body) = harness.post_facade(path, &body).await;
            assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{path} {body}");
            assert_eq!(
                response_body.get("error").and_then(Value::as_str),
                Some("invalid_request"),
                "{path} {body}"
            );
            assert_eq!(
                response_body.get("message").and_then(Value::as_str),
                Some(STREAM_REJECT_MESSAGE),
                "{path} {body} must name the documented streaming boundary"
            );
        }

        // Zero upstream cost: the mock never saw ANY of the rejected calls.
        assert_eq!(
            harness.mock_metrics().await,
            before,
            "rejected stream calls must never reach the mock"
        );

        // Zero persistence and zero billing for the local rejections.
        let pool = harness.pool().await;
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
            .fetch_one(&pool)
            .await
            .expect("row count");
        assert_eq!(rows, 0, "stream 400 must not persist rows");
        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE billing_state = 'charged'",
        )
        .fetch_one(&pool)
        .await
        .expect("ledger count");
        assert_eq!(charged, 0, "stream 400 must not bill");
        pool.close().await;

        // Explicit stream:false is NOT truthy: verbatim passthrough.
        let (status, _headers, body) = harness
            .post_facade(
                "/api/exa/answer",
                &json!({ "query": "x", "stream": false }),
            )
            .await;
        assert_eq!(status, reqwest::StatusCode::OK);
        assert_eq!(body, mock_answer_body());
        assert_eq!(
            harness.mock_metrics().await,
            json!({ "search": 0, "contents": 0, "answer": 1 })
        );
    }

    /// Client auth never relays upstream and never persists: a forged
    /// x-api-key header is stripped (the pool key replaces it), body
    /// api_key-like fields are removed before forwarding/persistence, and
    /// the th- token appears on no persisted surface.
    #[tokio::test]
    async fn exa_client_key_slots_stripped_and_th_token_never_persisted() {
        let harness =
            spawn_harness("facades-exa-redact", &["ex-pool-3"], &["ex-pool-3"]).await;

        // Forged native header + body key slots alongside the Bearer token:
        // 200 is positive proof the POOLED key arrived in x-api-key (the
        // mock 401s on any unknown key value).
        let response = Client::new()
            .post(format!("{}/api/exa/search", harness.base_url))
            .bearer_auth(&harness.token)
            .header("x-api-key", "client-forged-not-a-token")
            .json(&json!({
                "query": "redaction scan",
                "api_key": "th-body-sneaky-slot",
                "apiKey": "client-api-key-value"
            }))
            .send()
            .await
            .expect("forged slot request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, mock_search_body());

        // Native x-api-key slot auth (token in the header, no Bearer) works
        // and routes to /contents verbatim.
        let response = Client::new()
            .post(format!("{}/api/exa/contents", harness.base_url))
            .header("x-api-key", harness.token.clone())
            .header("content-type", "application/json")
            .body(r#"{"urls":["https://example.com/1"]}"#)
            .send()
            .await
            .expect("native slot request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, mock_contents_body());
        // Raw untouched body bytes forward verbatim when no field is stripped.
        let pool = harness.pool().await;
        let raw_row = sqlx::query(
            "SELECT CAST(request_body AS TEXT) AS request_body_text \
              FROM observability.request_logs WHERE path = '/api/exa/contents' \
              ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("contents row");
        let raw_body_text = raw_row
            .get::<Option<String>, _>("request_body_text")
            .unwrap_or_default();
        assert!(
            raw_body_text.contains(r#"https://example.com/1"#),
            "{raw_body_text}"
        );

        let row = sqlx::query(
            "SELECT query, CAST(request_body AS TEXT) AS request_body_text \
              FROM observability.request_logs WHERE path = '/api/exa/search' \
              ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("search row");
        let request_body_text = row
            .get::<Option<String>, _>("request_body_text")
            .unwrap_or_default();
        assert!(
            request_body_text.contains("redaction scan"),
            "client fields survive: {request_body_text}"
        );
        assert!(
            !request_body_text.contains("th-body-sneaky-slot"),
            "body api_key slot persisted raw: {request_body_text}"
        );
        assert!(
            !request_body_text.contains("client-api-key-value"),
            "body apiKey slot persisted raw: {request_body_text}"
        );
        assert!(
            !request_body_text.contains("client-forged-not-a-token"),
            "forged header value persisted: {request_body_text}"
        );
        pool.close().await;

        // Full-surface leak scan: the th- token and the pooled key never
        // survive into any persisted text column.
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
        let key_leaks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.request_logs WHERE query LIKE '%ex-pool-3%' \
              OR CAST(request_body AS TEXT) LIKE '%ex-pool-3%' \
              OR CAST(response_body AS TEXT) LIKE '%ex-pool-3%'",
        )
        .fetch_one(&pool)
        .await
        .expect("key leak scan");
        assert_eq!(key_leaks, 0, "pooled exa key persisted");
        pool.close().await;
    }

    /// 402 NO_MORE_CREDITS passes through verbatim and emits the exhausted
    /// signal through reconcile_key_health (key flips to exhausted —
    /// Exa's SuccessOnly recovery clears it on the next success, todo 18);
    /// the next request leases the other pooled key and bills 1.
    #[tokio::test]
    async fn exa_402_budget_tag_passes_through_and_marks_key_exhausted() {
        let harness = spawn_harness(
            "facades-exa-402",
            &["key-exhausted", "ex-good-2"],
            &["ex-good-2"],
        )
        .await;

        // Same LRU-driving pattern as the serpapi run-out test: fresh tokens
        // land on the untouched special key within a few leases.
        let mut budget_token: Option<(String, String)> = None;
        for _ in 0..3 {
            let (token, token_id) = create_test_token(&harness.base_url).await;
            let (status, _headers, body) = harness
                .post_facade_with_token("/api/exa/search", &token, &json!({ "query": "budget" }))
                .await;
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                assert_eq!(body, mock_no_more_credits_body(), "402 body verbatim");
                budget_token = Some((token, token_id));
                break;
            }
            assert_eq!(status, reqwest::StatusCode::OK, "fresh key leases succeed");
            assert_eq!(body, mock_search_body());
        }
        let Some((budget_token, budget_token_id)) = budget_token else {
            panic!("budget-402 never observed while the special key is pooled");
        };

        let pool = harness.pool().await;
        let exhausted_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-exhausted'")
                .fetch_one(&pool)
                .await
                .expect("exhausted key row");
        assert_eq!(
            exhausted_status, "exhausted",
            "402 budget tag must emit the exhausted signal"
        );

        let row = sqlx::query(
            "SELECT api_key_id, result_status, key_effect_code, failure_kind, tavily_status_code \
              FROM observability.request_logs WHERE auth_token_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&budget_token_id)
        .fetch_one(&pool)
        .await
        .expect("402 request log row");
        assert_eq!(row.get::<String, _>("result_status"), "quota_exhausted");
        assert_eq!(row.get::<String, _>("key_effect_code"), "marked_exhausted");
        assert!(row.get::<Option<String>, _>("failure_kind").is_none());
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 402);
        let exhausted_key_id = row.get::<Option<String>, _>("api_key_id");
        assert!(exhausted_key_id.is_some());
        pool.close().await;

        // The 402 attempt billed nothing.
        let pool = harness.pool().await;
        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&budget_token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger scan");
        assert_eq!(charged, 0, "exhausted attempt bills nothing");
        pool.close().await;

        // Next request on the SAME token rebinds off the exhausted key.
        let (status2, _headers, body2) = harness
            .post_facade_with_token("/api/exa/search", &budget_token, &json!({ "query": "budget" }))
            .await;
        assert_eq!(status2, reqwest::StatusCode::OK);
        assert_eq!(body2, mock_search_body());

        let pool = harness.pool().await;
        let row2 = sqlx::query(
            "SELECT api_key_id FROM observability.request_logs WHERE auth_token_id = ? \
              ORDER BY id DESC LIMIT 1",
        )
        .bind(&budget_token_id)
        .fetch_one(&pool)
        .await
        .expect("follow-up request log row");
        assert_ne!(
            row2.get::<Option<String>, _>("api_key_id"),
            exhausted_key_id,
            "next request must lease a different key"
        );
        let charged: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(business_credits), 0) FROM billing_ledger \
              WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&budget_token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger sum");
        assert_eq!(charged, 1, "the successful retry bills exactly 1");
        pool.close().await;
    }

    /// 429 passes through verbatim (Retry-After header visible to the
    /// client) and the parsed Retry-After arms the transient backoff on the
    /// shared HTTP scope; the key stays active and nothing bills.
    #[tokio::test]
    async fn exa_429_retry_after_passes_through_and_arms_transient_backoff() {
        let harness =
            spawn_harness("facades-exa-429", &["key-throttled"], &["ex-x"]).await;

        let (status, headers, body) = harness
            .post_facade("/api/exa/search", &json!({ "query": "throttle" }))
            .await;
        assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body, mock_rate_limited_body(), "429 body verbatim");
        assert_eq!(
            headers
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("2"),
            "upstream Retry-After must survive the verbatim passthrough"
        );

        let pool = harness.pool().await;
        let key_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-throttled'")
                .fetch_one(&pool)
                .await
                .expect("throttled key row");
        assert_eq!(key_status, "active", "429 must NOT exhaust the key");

        let row = sqlx::query(
            "SELECT result_status, failure_kind, key_effect_code, tavily_status_code \
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
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 429);

        // Retry-After parsed into the backoff signal: the armed row carries
        // the sized value (2s header → 30s shared min-ladder clamp).
        let backoff = sqlx::query(
            "SELECT b.retry_after_secs FROM api_key_transient_backoffs b \
              JOIN api_keys k ON k.id = b.key_id \
              WHERE k.api_key = 'key-throttled' AND b.scope = 'http_global' \
              AND b.reason_code = 'upstream_rate_limited_429'",
        )
        .fetch_one(&pool)
        .await
        .expect("backoff row");
        assert_eq!(
            backoff.get::<i64, _>("retry_after_secs"),
            30,
            "parsed Retry-After must size the armed backoff"
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

    /// Invalid pooled key: upstream 401 passes through verbatim and the
    /// auth-fail signal quarantines the key through the existing machinery.
    #[tokio::test]
    async fn exa_401_invalid_key_passes_through_and_applies_auth_fail() {
        let harness = spawn_harness(
            "facades-exa-401",
            &["ex-invalid-at-mock"],
            &["ex-some-other-key"],
        )
        .await;

        let (status, _headers, body) = harness
            .post_facade("/api/exa/search", &json!({ "query": "auth" }))
            .await;
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

        let quarantined: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines q \
              JOIN api_keys k ON k.id = q.key_id \
              WHERE k.api_key = 'ex-invalid-at-mock' AND q.cleared_at IS NULL \
              AND q.reason_code = 'invalid_api_key'",
        )
        .fetch_one(&pool)
        .await
        .expect("quarantine scan");
        assert_eq!(quarantined, 1, "401 must quarantine the pooled key");

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
}

// ---------------------------------------------------------------------------
// Brave REST facade section (plan todo 14; amendment M11 passive harvest).
// ---------------------------------------------------------------------------

mod brave {
    use super::*;

    /// Exact bodies mock_brave emits (mirrors src/bin/mock_brave.rs so the
    /// verbatim-passthrough assertions compare against the stub's contract).
    fn mock_web_body() -> Value {
        json!({
            "type": "search",
            "query": { "original": "mock", "more_results_available": false },
            "web": {
                "results": [
                    {
                        "title": "Mock",
                        "url": "https://example.com/1",
                        "description": "mock desc"
                    }
                ]
            }
        })
    }

    fn mock_news_body() -> Value {
        json!({
            "type": "news",
            "results": [
                { "title": "Mock News", "url": "https://example.com/n1", "description": "mock" }
            ]
        })
    }

    fn mock_llm_context_body() -> Value {
        json!({
            "grounding": {
                "generic": [
                    {
                        "url": "https://example.com/1",
                        "title": "Mock",
                        "snippets": ["mock snippet"]
                    }
                ]
            },
            "sources": {}
        })
    }

    fn mock_invalid_token_body() -> Value {
        json!({
            "error": {
                "code": "SUBSCRIPTION_TOKEN_INVALID",
                "detail": "The provided subscription token is invalid."
            }
        })
    }

    fn mock_quota_limited_body() -> Value {
        json!({ "error": { "code": "QUOTA_LIMITED" } })
    }

    fn mock_rate_limited_body() -> Value {
        json!({ "error": { "code": "RATE_LIMITED" } })
    }

    /// mock_brave's success rate-limit header grammar (positional pairing;
    /// month window = w=2592000 at position 1). Harvest reads exactly these.
    const WEB_PATH: &str = "/api/brave/res/v1/web/search";
    const NEWS_PATH: &str = "/api/brave/res/v1/news/search";
    const LLM_PATH: &str = "/api/brave/res/v1/llm/context";

    struct BraveHarness {
        base_url: String,
        mock_base_url: String,
        token: String,
        token_id: String,
        db_path: PathBuf,
        db_str: String,
        _server: ChildGuard,
        _mock: ChildGuard,
    }

    impl Drop for BraveHarness {
        fn drop(&mut self) {
            remove_sqlite_files(&self.db_path);
        }
    }

    /// Spawn mock_brave + the real server binary (stub upstreams ONLY; every
    /// non-Brave provider upstream points at a dead local port so production
    /// endpoints are unreachable by construction — high-anonymity incident rule).
    async fn spawn_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> BraveHarness {
        // Port reservations race across the parallel suite; retry with fresh
        // ephemeral ports (serpapi/exa section pattern).
        for attempt in 1..=5usize {
            if let Some(harness) = try_spawn_harness(prefix, pool_keys, mock_keys).await {
                return harness;
            }
            if attempt < 5 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        panic!("brave harness never became healthy after 5 spawn attempts");
    }

    async fn try_spawn_harness(
        prefix: &str,
        pool_keys: &[&str],
        mock_keys: &[&str],
    ) -> Option<BraveHarness> {
        let mock_port = reserve_local_port();
        let mock = Command::new(support_binaries::resolve_support_binary(
            "MOCK_BRAVE_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_brave"),
        ))
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mock_brave");

        let port = reserve_local_port();
        let db_path = temp_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
        // Tavily stays pointed at a dead local port: the facade tests never
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
        .env("BRAVE_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
        .env("BRAVE_API_KEYS", pool_keys.join(","))
        .env(
            "SERPAPI_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}"),
        )
        .env(
            "SERPAPI_MCP_UPSTREAM",
            format!("http://127.0.0.1:{dead_port}/mcp"),
        )
        .env("EXA_UPSTREAM", format!("http://127.0.0.1:{dead_port}"))
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
        wait_for_provider_keys(&db_str, "brave", i64::try_from(pool_keys.len()).unwrap()).await;
        let base_url = format!("http://127.0.0.1:{port}");
        let mock_base_url = format!("http://127.0.0.1:{mock_port}");
        let (token, token_id) = create_test_token(&base_url).await;

        Some(BraveHarness {
            base_url,
            mock_base_url,
            token,
            token_id,
            db_path,
            db_str,
            _server: ChildGuard { child: server },
            _mock: ChildGuard { child: mock },
        })
    }

    impl BraveHarness {
        async fn pool(&self) -> sqlx::SqlitePool {
            connect_sqlite_test_pool(&self.db_str).await
        }

        /// GET one facade path with the harness Bearer token; returns status +
        /// CLIENT-visible response headers (X-RateLimit-* passthrough proof) +
        /// decoded JSON body.
        async fn get_facade(
            &self,
            path: &str,
            query: &str,
        ) -> (reqwest::StatusCode, reqwest::header::HeaderMap, Value) {
            self.get_facade_with_token(path, &self.token, query).await
        }

        async fn get_facade_with_token(
            &self,
            path: &str,
            token: &str,
            query: &str,
        ) -> (reqwest::StatusCode, reqwest::header::HeaderMap, Value) {
            let url = if query.is_empty() {
                format!("{}{path}", self.base_url)
            } else {
                format!("{}{path}?{query}", self.base_url)
            };
            let response = Client::new()
                .get(url)
                .bearer_auth(token)
                .send()
                .await
                .expect("brave facade request");
            let status = response.status();
            let headers = response.headers().clone();
            (status, headers, response.json().await.expect("json body"))
        }

        /// mock_brave per-endpoint call counters (zero-extra-request proof).
        async fn mock_metrics(&self) -> Value {
            Client::new()
                .get(format!("{}/metrics", self.mock_base_url))
                .send()
                .await
                .expect("mock metrics request")
                .json::<Value>()
                .await
                .expect("mock metrics body")
        }

        /// api_keys quota view for a pooled brave key.
        async fn key_quota(&self, api_key: &str) -> (Option<i64>, Option<i64>, Option<i64>) {
            let pool = self.pool().await;
            let row = sqlx::query(
                "SELECT quota_limit, quota_remaining, quota_synced_at FROM api_keys \
                 WHERE api_key = ? AND provider = 'brave'",
            )
            .bind(api_key)
            .fetch_one(&pool)
            .await
            .expect("brave key row");
            let out = (
                row.get::<Option<i64>, _>("quota_limit"),
                row.get::<Option<i64>, _>("quota_remaining"),
                row.get::<Option<i64>, _>("quota_synced_at"),
            );
            pool.close().await;
            out
        }

        async fn brave_passive_sample_count(&self) -> i64 {
            let pool = self.pool().await;
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM api_key_quota_sync_samples s \
                 JOIN api_keys k ON k.id = s.key_id \
                 WHERE k.provider = 'brave' AND s.source = 'brave_passive'",
            )
            .fetch_one(&pool)
            .await
            .expect("sample count");
            pool.close().await;
            count
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (plan L203 + M11; one named test per criterion).
    // -----------------------------------------------------------------------

    /// Happy passthrough for all three GET endpoints: field-identical bodies,
    /// X-RateLimit-* headers VISIBLE to the client (web/search success carries
    /// them), verbatim query forwarding, provider='brave' rows with the
    /// api:brave/{web,news,llm-context} kinds, and credit tiers 1/1/2 from the
    /// committed todo-2 registry charged with settled billing_ledger rows.
    #[tokio::test]
    async fn brave_get_happy_passthrough_bills_tiers_and_shows_rate_limit_headers() {
        let harness = spawn_harness("facades-brave-happy", &["br-pool-1"], &["br-pool-1"]).await;

        // Tuple: (registered route path for the request, canonical log path
        // recorded in request_logs, query forwarded verbatim, expected body,
        // request_kind key, upstream_operation, credit tier, emits rate-limit
        // headers). request_logs keys on the CANONICAL kind path (the store
        // derives request_kind_key from that column), while the client-facing
        // full `/res/v1` path lives on auth_token_logs (bin boundary).
        let expectations = [
            (
                WEB_PATH,
                "/api/brave/web/search",
                "q=brave%20rust&count=5&freshness=pd&country=US&safesearch=moderate&result_filter=faq",
                mock_web_body(),
                "api:brave/web",
                "brave_http_web_search",
                1i64,
                true, // web/search success emits X-RateLimit-*
            ),
            (
                NEWS_PATH,
                "/api/brave/news/search",
                "q=news&count=10",
                mock_news_body(),
                "api:brave/news",
                "brave_http_news_search",
                1,
                false,
            ),
            (
                LLM_PATH,
                "/api/brave/llm/context",
                "q=context&maximum_number_of_tokens=4096",
                mock_llm_context_body(),
                "api:brave/llm-context",
                "brave_http_llm_context",
                2,
                false,
            ),
        ];
        for (path, log_path, query, expected, kind_key, operation, credits, has_rate_headers) in
            expectations
        {
            // Fresh token per endpoint so the ledger/quota asserts are exact.
            let (token, token_id) = create_test_token(&harness.base_url).await;
            let (status, headers, body) = harness
                .get_facade_with_token(path, &token, query)
                .await;
            assert_eq!(status, reqwest::StatusCode::OK, "{path}");
            // Field-identical to the mock response (verbatim passthrough, no
            // transformation; M22 forbids response inspection).
            assert_eq!(body, expected, "{path} body must pass through verbatim");

            // X-RateLimit-* survive build_response's hop-by-hop-only strip and
            // are VISIBLE to the client (plan todo 14 passthrough contract).
            if has_rate_headers {
                assert_eq!(
                    headers
                        .get("x-ratelimit-limit")
                        .and_then(|value| value.to_str().ok()),
                    Some("50, 1000"),
                    "{path} rate-limit header passthrough"
                );
                assert_eq!(
                    headers
                        .get("x-ratelimit-policy")
                        .and_then(|value| value.to_str().ok()),
                    Some("1;w=1, 1000;w=2592000"),
                    "{path} rate-limit policy passthrough"
                );
                assert_eq!(
                    headers
                        .get("x-ratelimit-reset")
                        .and_then(|value| value.to_str().ok()),
                    Some("1, 2592000"),
                    "{path} rate-limit reset passthrough"
                );
                assert!(
                    headers.contains_key("x-ratelimit-remaining"),
                    "{path} remaining header passthrough"
                );
            }

            let pool = harness.pool().await;
            let row = sqlx::query(
                "SELECT id, api_key_id, method, path, query, status_code, tavily_status_code, \
                   result_status, request_kind_key, provider, upstream_operation, \
                   forwarded_headers \
                   FROM observability.request_logs WHERE auth_token_id = ? \
                   ORDER BY id DESC LIMIT 1",
            )
            .bind(&token_id)
            .fetch_one(&pool)
            .await
            .expect("request log row");
            assert_eq!(row.get::<String, _>("provider"), "brave", "{path}");
            assert_eq!(row.get::<String, _>("path"), log_path, "{path}");
            assert_eq!(row.get::<String, _>("method"), "GET", "{path}");
            assert_eq!(row.get::<String, _>("request_kind_key"), kind_key, "{path}");
            assert_eq!(row.get::<i64, _>("status_code"), 200, "{path}");
            // Legacy column carries the upstream HTTP status verbatim.
            assert_eq!(row.get::<i64, _>("tavily_status_code"), 200, "{path}");
            assert_eq!(row.get::<String, _>("result_status"), "success", "{path}");
            assert_eq!(
                row.get::<Option<String>, _>("upstream_operation").as_deref(),
                Some(operation),
                "{path}"
            );
            assert!(row.get::<Option<String>, _>("api_key_id").is_some());

            // Client query forwarded verbatim (the mock ignores params; the
            // persisted stripped query proves the passthrough text).
            let persisted_query = row.get::<Option<String>, _>("query").unwrap_or_default();
            assert!(persisted_query.contains("q="), "{path}: {persisted_query}");
            assert!(!persisted_query.contains("th-"), "{path}: {persisted_query}");
            assert!(
                !persisted_query.contains("X-Subscription-Token"),
                "{path}: {persisted_query}"
            );
            // x-subscription-token is the injected pooled-key slot (forwarded);
            // no client project-id leaks (M2). The injected slot means the
            // forwarded set contains it exactly once.
            let forwarded = row.get::<String, _>("forwarded_headers");
            assert!(!forwarded.contains("x-project-id"), "{path}: {forwarded}");

            let log_id = row.get::<i64, _>("id");
            let ledger = sqlx::query(
                "SELECT billing_state, business_credits, request_log_id, settled_at \
                   FROM billing_ledger WHERE token_id = ? \
                   ORDER BY auth_token_log_id DESC LIMIT 1",
            )
            .bind(&token_id)
            .fetch_one(&pool)
            .await
            .expect("billing ledger row");
            assert_eq!(ledger.get::<String, _>("billing_state"), "charged", "{path}");
            assert_eq!(
                ledger.get::<Option<i64>, _>("business_credits"),
                Some(credits),
                "{path} credit tier"
            );
            assert_eq!(
                ledger.get::<Option<i64>, _>("request_log_id"),
                Some(log_id),
                "{path}"
            );
            assert!(ledger.get::<Option<i64>, _>("settled_at").is_some());
            pool.close().await;
        }

        // One upstream call per endpoint — the counters prove the passive
        // harvest added ZERO extra outbound requests (M11 hard MUST NOT).
        assert_eq!(
            harness.mock_metrics().await,
            json!({ "web": 1, "news": 1, "llm_context": 1 })
        );
    }

    /// Native client slot authentication: a th-token presented in the Brave
    /// `X-Subscription-Token` header authenticates, the client slot is
    /// STRIPPED (else the mock would 422 on the th-value) and replaced by the
    /// pooled key, and neither the th-token nor the pooled key ever persists.
    #[tokio::test]
    async fn brave_native_slot_auth_strips_client_key_and_never_persists_secrets() {
        let harness =
            spawn_harness("facades-brave-redact", &["br-pool-3"], &["br-pool-3"]).await;

        let url = format!("{}{WEB_PATH}?q=native-slot", harness.base_url);
        let response = Client::new()
            .get(&url)
            // th-token in the Brave native client slot (no Bearer).
            .header("x-subscription-token", harness.token.clone())
            .send()
            .await
            .expect("native slot request");
        // 200 is positive proof: the client slot was stripped and the pooled
        // key was injected into X-Subscription-Token (mock 422s on any other).
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, mock_web_body());

        let pool = harness.pool().await;
        // Full-surface leak scan: neither the th-token nor the pooled brave key
        // survives into any persisted text column (M17 strip-before-log).
        for (label, sql) in [
            (
                "request_logs.query",
                "SELECT COUNT(*) FROM observability.request_logs WHERE query LIKE '%th-%' OR query LIKE ?1 OR query LIKE '%br-pool-3%'",
            ),
            (
                "request_logs.request_body",
                "SELECT COUNT(*) FROM observability.request_logs WHERE CAST(request_body AS TEXT) LIKE '%th-%' OR CAST(request_body AS TEXT) LIKE ?1 OR CAST(request_body AS TEXT) LIKE '%br-pool-3%'",
            ),
            (
                "request_logs.response_body",
                "SELECT COUNT(*) FROM observability.request_logs WHERE CAST(response_body AS TEXT) LIKE '%th-%' OR CAST(response_body AS TEXT) LIKE ?1 OR CAST(response_body AS TEXT) LIKE '%br-pool-3%'",
            ),
            (
                "request_logs.error_message",
                "SELECT COUNT(*) FROM observability.request_logs WHERE error_message LIKE '%th-%' OR error_message LIKE ?1 OR error_message LIKE '%br-pool-3%'",
            ),
            (
                "auth_token_logs.query",
                "SELECT COUNT(*) FROM auth_token_logs WHERE query LIKE '%th-%' OR query LIKE ?1",
            ),
        ] {
            let leaks: i64 = sqlx::query_scalar(sql)
                .bind(format!("%{}%", harness.token))
                .fetch_one(&pool)
                .await
                .unwrap_or(1);
            assert_eq!(leaks, 0, "{label} leaked a client token or pooled key");
        }
        pool.close().await;
    }

    /// M11 passive harvest: after 2 web/search successes the leased key carries
    /// the month-window Limit (1000), the DECREMENTING month Remaining
    /// (999 -> 998), quota_synced_at, and two `brave_passive` samples. A
    /// following news/search success (which emits NO rate-limit headers)
    /// harvests nothing — the sample count and synced_at do NOT advance
    /// (bounded-write proof: not every response writes). Zero extra requests.
    #[tokio::test]
    async fn brave_passive_harvest_updates_key_quota_and_skips_non_material() {
        let harness =
            spawn_harness("facades-brave-harvest", &["br-harvest"], &["br-harvest"]).await;

        // Before any brave call the key has no synced quota state.
        let (limit0, remaining0, synced0) = harness.key_quota("br-harvest").await;
        assert_eq!(limit0, None, "fresh key starts with NULL quota_limit");
        assert_eq!(remaining0, None);
        assert_eq!(synced0, None);
        assert_eq!(harness.brave_passive_sample_count().await, 0);

        // Request 1: mock month Remaining = 1000-1 = 999.
        let (s1, _h1, b1) = harness.get_facade(WEB_PATH, "q=harvest1").await;
        assert_eq!(s1, reqwest::StatusCode::OK);
        assert_eq!(b1, mock_web_body());
        let (limit1, remaining1, synced1) = harness.key_quota("br-harvest").await;
        assert_eq!(limit1, Some(1000), "month-window Limit harvested");
        assert_eq!(remaining1, Some(999), "first month Remaining harvested");
        assert!(synced1.is_some(), "quota_synced_at set on material change");
        assert_eq!(harness.brave_passive_sample_count().await, 1);

        // Request 2: mock month Remaining = 998 (decrementing — material).
        let (s2, _h2, b2) = harness.get_facade(WEB_PATH, "q=harvest2").await;
        assert_eq!(s2, reqwest::StatusCode::OK);
        assert_eq!(b2, mock_web_body());
        let (limit2, remaining2, synced2) = harness.key_quota("br-harvest").await;
        assert_eq!(limit2, Some(1000));
        assert_eq!(remaining2, Some(998), "second month Remaining decremented");
        assert_eq!(harness.brave_passive_sample_count().await, 2);

        // Non-material path: news/search success emits NO rate-limit headers,
        // so the harvest parses nothing and writes nothing (bounded write).
        let (s3, _h3, _b3) = harness.get_facade(NEWS_PATH, "q=nomaterial").await;
        assert_eq!(s3, reqwest::StatusCode::OK);
        assert_eq!(harness.brave_passive_sample_count().await, 2, "no new sample");
        let (limit3, remaining3, synced3) = harness.key_quota("br-harvest").await;
        assert_eq!(limit3, Some(1000));
        assert_eq!(remaining3, Some(998));
        assert_eq!(synced3, synced2, "synced_at unchanged on non-material");

        // Zero extra outbound requests: exactly the 3 facade calls reached mock.
        assert_eq!(
            harness.mock_metrics().await,
            json!({ "web": 2, "news": 1, "llm_context": 0 })
        );
    }

    /// 422 SUBSCRIPTION_TOKEN_INVALID passes through verbatim; the todo-3
    /// classifier's AuthFailed signal feeds reconcile_key_health (key
    /// quarantined through the existing machinery), no credit set. mock_brave
    /// emits no rate-limit header on this error, so harvest is a no-op (the
    /// error path neither fails nor writes).
    #[tokio::test]
    async fn brave_invalid_token_422_passes_through_and_applies_auth_fail() {
        let harness = spawn_harness(
            "facades-brave-422",
            &["br-invalid-at-mock"],
            &["br-some-other-key"],
        )
        .await;

        let (status, _headers, body) = harness.get_facade(WEB_PATH, "q=auth").await;
        assert_eq!(status, reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, mock_invalid_token_body(), "422 body verbatim");

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
        assert_eq!(row.get::<i64, _>("status_code"), 422);
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 422);

        let quarantined: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines q \
              JOIN api_keys k ON k.id = q.key_id \
              WHERE k.api_key = 'br-invalid-at-mock' AND q.cleared_at IS NULL \
              AND q.reason_code = 'invalid_api_key'",
        )
        .fetch_one(&pool)
        .await
        .expect("quarantine scan");
        assert_eq!(quarantined, 1, "422 auth-fail must quarantine the pooled key");

        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&harness.token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger scan");
        assert_eq!(charged, 0);
        // No spurious harvest on the error path (mock emits no rate headers).
        assert_eq!(harness.brave_passive_sample_count().await, 0);
        pool.close().await;
    }

    /// 402 QUOTA_LIMITED passes through verbatim and marks the leased key
    /// exhausted through the existing machinery; the next request leases the
    /// OTHER pooled key (200). Exhausted Brave keys recover via T9's
    /// reset_monthly RateLimitResetWindow — NOT duplicated here (todo-18 depth).
    #[tokio::test]
    async fn brave_402_quota_limited_passes_through_and_marks_exhausted() {
        let harness = spawn_harness(
            "facades-brave-402",
            &["key-exhausted", "br-good-2"],
            &["br-good-2"],
        )
        .await;

        // Drive the LRU deterministically (serpapi/exa pattern): fresh tokens
        // land on the untouched special key within a few leases.
        let mut quota_token: Option<(String, String)> = None;
        for _ in 0..3 {
            let (token, token_id) = create_test_token(&harness.base_url).await;
            let (status, _headers, body) =
                harness.get_facade_with_token(WEB_PATH, &token, "q=quota").await;
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                assert_eq!(body, mock_quota_limited_body(), "402 body verbatim");
                quota_token = Some((token, token_id));
                break;
            }
            assert_eq!(status, reqwest::StatusCode::OK, "fresh key leases succeed");
            assert_eq!(body, mock_web_body());
        }
        let Some((quota_token, quota_token_id)) = quota_token else {
            panic!("quota-402 never observed while the special key is pooled");
        };

        let pool = harness.pool().await;
        let exhausted_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-exhausted'")
                .fetch_one(&pool)
                .await
                .expect("exhausted key row");
        assert_eq!(
            exhausted_status, "exhausted",
            "402 QUOTA_LIMITED must emit the exhausted signal"
        );
        let row = sqlx::query(
            "SELECT result_status, key_effect_code, failure_kind, tavily_status_code \
               FROM observability.request_logs WHERE auth_token_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&quota_token_id)
        .fetch_one(&pool)
        .await
        .expect("402 request log row");
        assert_eq!(row.get::<String, _>("result_status"), "quota_exhausted");
        assert_eq!(row.get::<String, _>("key_effect_code"), "marked_exhausted");
        assert!(row.get::<Option<String>, _>("failure_kind").is_none());
        assert_eq!(row.get::<i64, _>("tavily_status_code"), 402);
        let charged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&quota_token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger scan");
        assert_eq!(charged, 0, "exhausted attempt bills nothing");
        pool.close().await;

        // Next request on the SAME token rebinds off the exhausted key.
        let (status2, _headers2, body2) = harness
            .get_facade_with_token(WEB_PATH, &quota_token, "q=quota")
            .await;
        assert_eq!(status2, reqwest::StatusCode::OK);
        assert_eq!(body2, mock_web_body());
    }

    /// 429 RATE_LIMITED passes through verbatim and arms a SHORT second-scale
    /// transient backoff on the shared HTTP scope (Brave w=1 window: the 2s
    /// default, distinct from Exa's minute-scale 30s clamp); the key stays
    /// active and nothing bills.
    #[tokio::test]
    async fn brave_429_rate_limited_arms_second_scale_backoff_without_exhaustion() {
        let harness =
            spawn_harness("facades-brave-429", &["key-throttled"], &["br-x"]).await;

        let (status, _headers, body) = harness.get_facade(WEB_PATH, "q=throttle").await;
        assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body, mock_rate_limited_body(), "429 body verbatim");

        let pool = harness.pool().await;
        let key_status: String =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-throttled'")
                .fetch_one(&pool)
                .await
                .expect("throttled key row");
        assert_eq!(key_status, "active", "429 must NOT exhaust the key");

        let row = sqlx::query(
            "SELECT result_status, failure_kind, key_effect_code, tavily_status_code \
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

        // Second-scale sizing: no Retry-After from the mock, so the Brave
        // default (2s) arms — NOT the minute-scale 30s used by Exa/SerpApi.
        let backoff = sqlx::query(
            "SELECT b.retry_after_secs FROM api_key_transient_backoffs b \
              JOIN api_keys k ON k.id = b.key_id \
              WHERE k.api_key = 'key-throttled' AND b.scope = 'http_global' \
              AND b.reason_code = 'upstream_rate_limited_429'",
        )
        .fetch_one(&pool)
        .await
        .expect("backoff row");
        assert_eq!(backoff.get::<i64, _>("retry_after_secs"), 2, "second-scale");

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

    /// Quarantined pool: after a 422 auth-fail quarantines the ONLY key, the
    /// next request surfaces the Hikari NoAvailableKeys shape (502
    /// proxy_error), never a provider body (serpapi/exa parity).
    #[tokio::test]
    async fn brave_quarantined_pool_returns_502_no_available_keys_shape() {
        let harness = spawn_harness(
            "facades-brave-nopool",
            &["br-invalid-at-mock"],
            &["br-some-other-key"],
        )
        .await;

        let (status, _headers, body) = harness.get_facade(WEB_PATH, "q=solo").await;
        assert_eq!(status, reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, mock_invalid_token_body());

        let (status2, _headers2, body2) = harness.get_facade(WEB_PATH, "q=solo2").await;
        assert_eq!(status2, reqwest::StatusCode::BAD_GATEWAY);
        assert_eq!(
            body2.get("error").and_then(Value::as_str),
            Some("proxy_error")
        );
        assert_eq!(
            body2.get("message").and_then(Value::as_str),
            Some("upstream unavailable")
        );
    }
}

