//! Mock Brave Search upstream (web/news/llm-context) for offline tests.
//!
//! Per-endpoint call counters are exposed via GET /metrics so facade suites
//! can prove zero-extra-request passive quota harvest (plan todo 14 / M11:
//! the harvest must never add an outbound call beyond the facade request).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use serde_json::json;

const KEY_EXHAUSTED: &str = "key-exhausted";
const KEY_THROTTLED: &str = "key-throttled";
const MONTHLY_LIMIT: u64 = 1_000;

#[derive(Parser)]
#[command(
    name = "mock_brave",
    about = "Mock Brave Search upstream for offline tests"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:58093")]
    bind: String,
    #[arg(
        long,
        env = "MOCK_BRAVE_KEYS",
        value_delimiter = ',',
        default_value = "mock-brave-key"
    )]
    keys: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyStatus {
    Valid,
    Invalid,
    Exhausted,
    Throttled,
}

#[derive(Debug, Default)]
struct MockCounters {
    web: AtomicU64,
    news: AtomicU64,
    llm_context: AtomicU64,
}

struct MockState {
    keys: Vec<String>,
    monthly_remaining: AtomicU64,
    counters: MockCounters,
}

impl MockState {
    fn classify(&self, key: &str) -> KeyStatus {
        match key {
            KEY_EXHAUSTED => KeyStatus::Exhausted,
            KEY_THROTTLED => KeyStatus::Throttled,
            _ if self.keys.iter().any(|known| known == key) => KeyStatus::Valid,
            _ => KeyStatus::Invalid,
        }
    }

    fn consume_monthly_search(&self) -> u64 {
        match self
            .monthly_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            }) {
            Ok(previous) => previous - 1,
            Err(current) => current,
        }
    }
}

fn missing_token_response() -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": {
                "code": "VALIDATION",
                "detail": "Unable to validate request parameter(s)",
                "meta": {
                    "errors": [
                        { "loc": ["header", "x-subscription-token"], "msg": "Field required" }
                    ]
                }
            }
        })),
    )
        .into_response()
}

fn invalid_token_response() -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": {
                "code": "SUBSCRIPTION_TOKEN_INVALID",
                "detail": "The provided subscription token is invalid."
            }
        })),
    )
        .into_response()
}

fn error_code_response(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": { "code": code } }))).into_response()
}

fn authorize(state: &MockState, headers: &HeaderMap) -> Option<Response> {
    let token = headers
        .get("x-subscription-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if token.is_empty() {
        return Some(missing_token_response());
    }
    match state.classify(token) {
        KeyStatus::Valid => None,
        KeyStatus::Invalid => Some(invalid_token_response()),
        KeyStatus::Exhausted => Some(error_code_response(
            StatusCode::PAYMENT_REQUIRED,
            "QUOTA_LIMITED",
        )),
        KeyStatus::Throttled => Some(error_code_response(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
        )),
    }
}

fn rate_limit_headers(remaining: u64) -> [(HeaderName, String); 4] {
    [
        (
            HeaderName::from_static("x-ratelimit-limit"),
            "50, 1000".to_string(),
        ),
        (
            HeaderName::from_static("x-ratelimit-policy"),
            "1;w=1, 1000;w=2592000".to_string(),
        ),
        (
            HeaderName::from_static("x-ratelimit-remaining"),
            format!("1, {remaining}"),
        ),
        (
            HeaderName::from_static("x-ratelimit-reset"),
            "1, 2592000".to_string(),
        ),
    ]
}

async fn handle_web_search(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    state.counters.web.fetch_add(1, Ordering::Relaxed);
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    let remaining = state.consume_monthly_search();
    (
        StatusCode::OK,
        rate_limit_headers(remaining),
        Json(json!({
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
        })),
    )
        .into_response()
}

async fn handle_news_search(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    state.counters.news.fetch_add(1, Ordering::Relaxed);
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
        "type": "news",
        "results": [
            { "title": "Mock News", "url": "https://example.com/n1", "description": "mock" }
        ]
    }))
    .into_response()
}

async fn handle_llm_context(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    state.counters.llm_context.fetch_add(1, Ordering::Relaxed);
    if let Some(response) = authorize(&state, &headers) {
        return response;
    }
    Json(json!({
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
    }))
    .into_response()
}

/// Request-counter readout (plan todo 14 / M11 zero-extra-request proof):
/// counts EVERY received per-endpoint call, authorized or not. The passive
/// harvest must never add a call, so a facade suite's counter total equals
/// its own request count exactly.
async fn handle_metrics(State(state): State<Arc<MockState>>) -> Response {
    Json(json!({
        "web": state.counters.web.load(Ordering::Relaxed),
        "news": state.counters.news.load(Ordering::Relaxed),
        "llm_context": state.counters.llm_context.load(Ordering::Relaxed),
    }))
    .into_response()
}

fn build_router(state: Arc<MockState>) -> Router {
    Router::new()
        .route("/res/v1/web/search", get(handle_web_search))
        .route("/res/v1/news/search", get(handle_news_search))
        .route("/res/v1/llm/context", get(handle_llm_context))
        .route("/metrics", get(handle_metrics))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let state = Arc::new(MockState {
        keys: cli.keys,
        monthly_remaining: AtomicU64::new(MONTHLY_LIMIT),
        counters: MockCounters::default(),
    });
    let app = build_router(state);
    println!("Mock Brave upstream listening on http://{}", cli.bind);
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
            keys: vec!["mock-brave-key".to_string()],
            monthly_remaining: AtomicU64::new(MONTHLY_LIMIT),
            counters: MockCounters::default(),
        });
        tokio::spawn(async move {
            axum::serve(listener, build_router(state))
                .await
                .expect("mock server runs");
        });
        base
    }

    async fn get_with_token(base: &str, path: &str, token: Option<&str>) -> reqwest::Response {
        let mut request = reqwest::Client::new().get(format!("{base}{path}"));
        if let Some(token) = token {
            request = request.header("x-subscription-token", token);
        }
        request.send().await.expect("request completes")
    }

    #[tokio::test]
    async fn missing_token_returns_422_validation_envelope() {
        // Given a running mock Brave upstream; When web search is called without the token header
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/web/search", None).await;
        // Then it answers 422 with the Brave VALIDATION envelope
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["error"]["code"], json!("VALIDATION"));
        assert_eq!(
            body["error"]["detail"],
            json!("Unable to validate request parameter(s)")
        );
        assert_eq!(
            body["error"]["meta"]["errors"][0]["loc"],
            json!(["header", "x-subscription-token"])
        );
        assert_eq!(
            body["error"]["meta"]["errors"][0]["msg"],
            json!("Field required")
        );
    }

    #[tokio::test]
    async fn invalid_token_returns_422_subscription_token_invalid() {
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/web/search", Some("bogus")).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["error"]["code"], json!("SUBSCRIPTION_TOKEN_INVALID"));
        assert_eq!(
            body["error"]["detail"],
            json!("The provided subscription token is invalid.")
        );
    }

    #[tokio::test]
    async fn exhausted_key_returns_402_quota_limited() {
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/web/search", Some(KEY_EXHAUSTED)).await;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["error"]["code"], json!("QUOTA_LIMITED"));
    }

    #[tokio::test]
    async fn throttled_key_returns_429_rate_limited() {
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/web/search", Some(KEY_THROTTLED)).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["error"]["code"], json!("RATE_LIMITED"));
    }

    #[tokio::test]
    async fn web_search_success_returns_body_and_decrementing_rate_limit_headers() {
        // Given a running mock with a full monthly budget
        let base = spawn_mock().await;
        // When the first web search succeeds
        let response = get_with_token(&base, "/res/v1/web/search", Some("mock-brave-key")).await;
        // Then the body carries the web-results envelope
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[HeaderName::from_static("x-ratelimit-limit")],
            "50, 1000"
        );
        assert_eq!(
            response.headers()[HeaderName::from_static("x-ratelimit-policy")],
            "1;w=1, 1000;w=2592000"
        );
        assert_eq!(
            response.headers()[HeaderName::from_static("x-ratelimit-remaining")],
            format!("1, {}", MONTHLY_LIMIT - 1)
        );
        assert_eq!(
            response.headers()[HeaderName::from_static("x-ratelimit-reset")],
            "1, 2592000"
        );
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["type"], json!("search"));
        assert_eq!(body["query"]["original"], json!("mock"));
        assert_eq!(body["query"]["more_results_available"], json!(false));
        assert_eq!(body["web"]["results"][0]["title"], json!("Mock"));
        assert_eq!(
            body["web"]["results"][0]["url"],
            json!("https://example.com/1")
        );
        assert_eq!(body["web"]["results"][0]["description"], json!("mock desc"));
        // And the next success decrements the monthly remaining counter again
        let second = get_with_token(&base, "/res/v1/web/search", Some("mock-brave-key")).await;
        assert_eq!(
            second.headers()[HeaderName::from_static("x-ratelimit-remaining")],
            format!("1, {}", MONTHLY_LIMIT - 2)
        );
    }

    #[tokio::test]
    async fn news_search_success_returns_news_envelope() {
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/news/search", Some("mock-brave-key")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["type"], json!("news"));
        assert_eq!(body["results"][0]["title"], json!("Mock News"));
        assert_eq!(body["results"][0]["url"], json!("https://example.com/n1"));
        assert_eq!(body["results"][0]["description"], json!("mock"));
    }

    #[tokio::test]
    async fn llm_context_success_returns_grounding_envelope() {
        let base = spawn_mock().await;
        let response = get_with_token(&base, "/res/v1/llm/context", Some("mock-brave-key")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(
            body["grounding"]["generic"][0]["url"],
            json!("https://example.com/1")
        );
        assert_eq!(body["grounding"]["generic"][0]["title"], json!("Mock"));
        assert_eq!(
            body["grounding"]["generic"][0]["snippets"],
            json!(["mock snippet"])
        );
        assert_eq!(body["sources"], json!({}));
    }

    #[tokio::test]
    async fn metrics_counters_track_received_calls_per_endpoint() {
        // Given a running mock with all counters at zero (plan todo 14 / M11)
        let base = spawn_mock().await;
        let client = reqwest::Client::new();
        let metrics = client
            .get(format!("{base}/metrics"))
            .send()
            .await
            .expect("metrics request");
        assert_eq!(
            metrics.json::<Value>().await.expect("json body"),
            json!({ "web": 0, "news": 0, "llm_context": 0 })
        );

        // When web (valid + invalid key), news, and llm-context are each called
        get_with_token(&base, "/res/v1/web/search", Some("mock-brave-key")).await;
        get_with_token(&base, "/res/v1/web/search", Some("bogus")).await;
        get_with_token(&base, "/res/v1/news/search", Some("mock-brave-key")).await;
        get_with_token(&base, "/res/v1/llm/context", Some("mock-brave-key")).await;

        // Then /metrics reflects every received call, authorized or not, and an
        // uncalled endpoint stays at zero (zero-extra-request harvest proof).
        let metrics = client
            .get(format!("{base}/metrics"))
            .send()
            .await
            .expect("metrics request");
        assert_eq!(
            metrics.json::<Value>().await.expect("json body"),
            json!({ "web": 2, "news": 1, "llm_context": 1 })
        );
    }
}
