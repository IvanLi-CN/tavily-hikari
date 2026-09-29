//! Mock Exa AI upstream (POST /search, /contents, /answer) for offline tests.
//!
//! Per-endpoint call counters are exposed via GET /metrics so facade suites
//! can prove zero-cost local rejections (plan todo 13 QA: a typed stream-400
//! must leave the answer counter untouched).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde_json::{Value, json};

const KEY_EXHAUSTED: &str = "key-exhausted";
const KEY_THROTTLED: &str = "key-throttled";
const KEY_BUDGET: &str = "key-budget";

#[derive(Parser)]
#[command(name = "mock_exa", about = "Mock Exa AI upstream for offline tests")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:58092")]
    bind: String,
    #[arg(
        long,
        env = "MOCK_EXA_KEYS",
        value_delimiter = ',',
        default_value = "mock-exa-key"
    )]
    keys: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyStatus {
    Valid,
    Invalid,
    Exhausted,
    Budget,
    Throttled,
}

#[derive(Debug, Default)]
struct MockCounters {
    search: AtomicU64,
    contents: AtomicU64,
    answer: AtomicU64,
}

/// Last request body received per endpoint (plan todo 17 additive knob): lets
/// facade suites assert the MCP→REST translation wire body (e.g. urls-vs-ids
/// mapping and the stream-never rule) without changing any existing response.
#[derive(Debug, Default)]
struct LastBodies {
    search: Mutex<Option<String>>,
    contents: Mutex<Option<String>>,
    answer: Mutex<Option<String>>,
}

fn remember_last_body(slot: &Mutex<Option<String>>, body: Option<&str>) {
    *slot.lock().expect("last body lock") = Some(body.unwrap_or_default().to_string());
}

fn read_last_body(slot: &Mutex<Option<String>>) -> Value {
    let Some(text) = slot.lock().expect("last body lock").clone() else {
        return Value::Null;
    };
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

struct MockState {
    keys: Vec<String>,
    counters: MockCounters,
    last_bodies: LastBodies,
}

impl MockState {
    fn classify(&self, key: &str) -> KeyStatus {
        match key {
            KEY_EXHAUSTED => KeyStatus::Exhausted,
            KEY_BUDGET => KeyStatus::Budget,
            KEY_THROTTLED => KeyStatus::Throttled,
            _ if self.keys.iter().any(|known| known == key) => KeyStatus::Valid,
            _ => KeyStatus::Invalid,
        }
    }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "requestId": "mock",
            "error": "Invalid API key",
            "tag": "INVALID_API_KEY"
        })),
    )
        .into_response()
}

fn payment_required_response(message: &str, tag: &str) -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({ "requestId": "mock", "error": message, "tag": tag })),
    )
        .into_response()
}

fn rate_limited_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, "2")],
        Json(json!({
            "requestId": "mock",
            "error": "Rate limit exceeded",
            "tag": "RATE_LIMIT_EXCEEDED"
        })),
    )
        .into_response()
}

fn authorize(state: &MockState, headers: &HeaderMap) -> Option<Response> {
    let key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .unwrap_or_default();
    match state.classify(key) {
        KeyStatus::Valid => None,
        KeyStatus::Invalid => Some(unauthorized_response()),
        KeyStatus::Exhausted => Some(payment_required_response(
            "You have no more credits",
            "NO_MORE_CREDITS",
        )),
        KeyStatus::Budget => Some(payment_required_response(
            "API key budget exceeded",
            "API_KEY_BUDGET_EXCEEDED",
        )),
        KeyStatus::Throttled => Some(rate_limited_response()),
    }
}

async fn handle_search(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    body: Option<String>,
) -> Response {
    state.counters.search.fetch_add(1, Ordering::Relaxed);
    remember_last_body(&state.last_bodies.search, body.as_deref());
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
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
    }))
    .into_response()
}

async fn handle_contents(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    body: Option<String>,
) -> Response {
    state.counters.contents.fetch_add(1, Ordering::Relaxed);
    remember_last_body(&state.last_bodies.contents, body.as_deref());
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
        "results": [
            { "title": "Mock", "url": "https://example.com/1", "text": "mock text" }
        ],
        "statuses": [
            { "id": "https://example.com/1", "status": "success", "source": "cached" }
        ]
    }))
    .into_response()
}

async fn handle_answer(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    body: Option<String>,
) -> Response {
    state.counters.answer.fetch_add(1, Ordering::Relaxed);
    remember_last_body(&state.last_bodies.answer, body.as_deref());
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
        "requestId": "mock",
        "answer": "mock answer",
        "citations": [{ "url": "https://example.com/1" }],
        "costDollars": { "total": 0.005 }
    }))
    .into_response()
}

/// Request-counter readout (plan todo 13 zero-cost proof): counts EVERY
/// received per-endpoint call, authorized or not.
async fn handle_metrics(State(state): State<Arc<MockState>>) -> Response {
    Json(json!({
        "search": state.counters.search.load(Ordering::Relaxed),
        "contents": state.counters.contents.load(Ordering::Relaxed),
        "answer": state.counters.answer.load(Ordering::Relaxed),
    }))
    .into_response()
}

/// Last received request body per endpoint (plan todo 17 additive knob):
/// parsed back to JSON when possible, else the raw string; null before the
/// first call. Used by the exa MCP e2e to assert the translated wire body.
async fn handle_last_bodies(State(state): State<Arc<MockState>>) -> Response {
    Json(json!({
        "search": read_last_body(&state.last_bodies.search),
        "contents": read_last_body(&state.last_bodies.contents),
        "answer": read_last_body(&state.last_bodies.answer),
    }))
    .into_response()
}

fn build_router(state: Arc<MockState>) -> Router {
    Router::new()
        .route("/search", post(handle_search))
        .route("/contents", post(handle_contents))
        .route("/answer", post(handle_answer))
        .route("/metrics", get(handle_metrics))
        .route("/last-bodies", get(handle_last_bodies))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let state = Arc::new(MockState {
        keys: cli.keys,
        counters: MockCounters::default(),
        last_bodies: LastBodies::default(),
    });
    let app = build_router(state);
    println!("Mock Exa upstream listening on http://{}", cli.bind);
    axum::serve(tokio::net::TcpListener::bind(cli.bind).await?, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    async fn spawn_mock() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let base = format!("http://{}", listener.local_addr().expect("local addr"));
        let state = Arc::new(MockState {
            keys: vec!["mock-exa-key".to_string()],
            counters: MockCounters::default(),
            last_bodies: LastBodies::default(),
        });
        tokio::spawn(async move {
            axum::serve(listener, build_router(state))
                .await
                .expect("mock server runs");
        });
        base
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    async fn post_with_key(base: &str, path: &str, key: Option<&str>) -> reqwest::Response {
        let mut request = client().post(format!("{base}{path}")).json(&json!({}));
        if let Some(key) = key {
            request = request.header("x-api-key", key);
        }
        request.send().await.expect("request completes")
    }

    #[tokio::test]
    async fn missing_key_returns_401_invalid_api_key_tag() {
        // Given a running mock Exa upstream; When /search is called without a key
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", None).await;
        // Then it answers 401 with the Exa error envelope
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["requestId"], json!("mock"));
        assert_eq!(body["error"], json!("Invalid API key"));
        assert_eq!(body["tag"], json!("INVALID_API_KEY"));
    }

    #[tokio::test]
    async fn invalid_key_returns_401_invalid_api_key_tag() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", Some("not-a-key")).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["tag"], json!("INVALID_API_KEY"));
    }

    #[tokio::test]
    async fn exhausted_key_returns_402_no_more_credits() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", Some(KEY_EXHAUSTED)).await;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["tag"], json!("NO_MORE_CREDITS"));
    }

    #[tokio::test]
    async fn budget_key_returns_402_budget_exceeded() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", Some(KEY_BUDGET)).await;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["tag"], json!("API_KEY_BUDGET_EXCEEDED"));
    }

    #[tokio::test]
    async fn throttled_key_returns_429_with_retry_after_header() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", Some(KEY_THROTTLED)).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "2");
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["tag"], json!("RATE_LIMIT_EXCEEDED"));
    }

    #[tokio::test]
    async fn bearer_auth_is_accepted_on_search() {
        let base = spawn_mock().await;
        let response = client()
            .post(format!("{base}/search"))
            .bearer_auth("mock-exa-key")
            .json(&json!({}))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn search_success_returns_results_and_cost() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/search", Some("mock-exa-key")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["requestId"], json!("mock"));
        assert_eq!(body["results"][0]["title"], json!("Mock"));
        assert_eq!(body["results"][0]["url"], json!("https://example.com/1"));
        assert_eq!(body["results"][0]["text"], json!("mock text"));
        assert_eq!(
            body["results"][0]["publishedDate"],
            json!("2026-01-01T00:00:00.000Z")
        );
        assert_eq!(body["costDollars"]["total"], json!(0.007));
        assert_eq!(body["searchTime"], json!(123.4));
    }

    #[tokio::test]
    async fn contents_success_returns_results_and_statuses() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/contents", Some("mock-exa-key")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["results"][0]["title"], json!("Mock"));
        assert_eq!(body["results"][0]["url"], json!("https://example.com/1"));
        assert_eq!(body["results"][0]["text"], json!("mock text"));
        assert_eq!(body["statuses"][0]["id"], json!("https://example.com/1"));
        assert_eq!(body["statuses"][0]["status"], json!("success"));
        assert_eq!(body["statuses"][0]["source"], json!("cached"));
    }

    #[tokio::test]
    async fn answer_success_returns_answer_and_citations() {
        let base = spawn_mock().await;
        let response = post_with_key(&base, "/answer", Some("mock-exa-key")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["requestId"], json!("mock"));
        assert_eq!(body["answer"], json!("mock answer"));
        assert_eq!(body["citations"][0]["url"], json!("https://example.com/1"));
        assert_eq!(body["costDollars"]["total"], json!(0.005));
    }

    #[tokio::test]
    async fn metrics_counters_track_received_calls_per_endpoint() {
        // Given a running mock with all counters at zero
        let base = spawn_mock().await;
        let metrics = client()
            .get(format!("{base}/metrics"))
            .send()
            .await
            .expect("metrics request");
        assert_eq!(
            metrics.json::<Value>().await.expect("json body"),
            json!({ "search": 0, "contents": 0, "answer": 0 })
        );

        // When /search (valid + invalid key) and /answer are each called once
        post_with_key(&base, "/search", Some("mock-exa-key")).await;
        post_with_key(&base, "/search", Some("not-a-key")).await;
        post_with_key(&base, "/answer", Some("mock-exa-key")).await;

        // Then /metrics reflects every received call, authorized or not, and
        // an uncalled endpoint stays at zero (todo 13 zero-cost assertion).
        let metrics = client()
            .get(format!("{base}/metrics"))
            .send()
            .await
            .expect("metrics request");
        assert_eq!(
            metrics.json::<Value>().await.expect("json body"),
            json!({ "search": 2, "contents": 0, "answer": 1 })
        );
    }

    #[tokio::test]
    async fn last_bodies_readout_records_received_request_bodies() {
        // Given a running mock with no calls yet; Then the readout is all-null.
        let base = spawn_mock().await;
        let readout = client()
            .get(format!("{base}/last-bodies"))
            .send()
            .await
            .expect("last-bodies request");
        assert_eq!(
            readout.json::<Value>().await.expect("json body"),
            json!({ "search": null, "contents": null, "answer": null })
        );

        // When /search and /contents each receive one JSON body (including one
        // rejected by auth); Then the readout echoes the LAST body per endpoint
        // parsed back to JSON, and the uncalled endpoint stays null.
        client()
            .post(format!("{base}/search"))
            .header("x-api-key", "mock-exa-key")
            .json(&json!({ "query": "first", "numResults": 3 }))
            .send()
            .await
            .expect("search request");
        client()
            .post(format!("{base}/search"))
            .header("x-api-key", "not-a-key")
            .json(&json!({ "query": "second" }))
            .send()
            .await
            .expect("search request");
        client()
            .post(format!("{base}/contents"))
            .header("x-api-key", "mock-exa-key")
            .json(&json!({ "urls": ["https://example.com"] }))
            .send()
            .await
            .expect("contents request");
        let readout = client()
            .get(format!("{base}/last-bodies"))
            .send()
            .await
            .expect("last-bodies request");
        assert_eq!(
            readout.json::<Value>().await.expect("json body"),
            json!({
                "search": { "query": "second" },
                "contents": { "urls": ["https://example.com"] },
                "answer": null
            })
        );
    }
}
