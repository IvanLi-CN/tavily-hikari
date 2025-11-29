use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
struct Cli {
    /// Address to bind the mock Tavily server
    #[arg(long, default_value = "127.0.0.1:58088")]
    bind: SocketAddr,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct KeyRecord {
    limit: i64,
    remaining: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct ForcedResponse {
    #[serde(default)]
    http_status: Option<u16>,
    #[serde(default)]
    structured_status: Option<i64>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    once: bool,
    #[serde(default)]
    delay_ms: Option<u64>,
}

#[derive(Default, Clone, Serialize)]
struct SnapshotState {
    keys: HashMap<String, KeyRecord>,
    forced: Option<ForcedResponse>,
}

#[derive(Default)]
struct AppState {
    inner: RwLock<SnapshotState>,
}

#[derive(Deserialize)]
struct AddKeyRequest {
    secret: String,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    remaining: Option<i64>,
}

fn default_limit() -> i64 {
    1_000
}

#[derive(Deserialize)]
struct UpdateKeyRequest {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    remaining: Option<i64>,
}

#[derive(Deserialize)]
struct ForceRequest {
    #[serde(default)]
    http_status: Option<u16>,
    #[serde(default)]
    structured_status: Option<i64>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    once: bool,
    #[serde(default)]
    delay_ms: Option<u64>,
}

#[derive(Deserialize)]
struct McpQuery {
    #[serde(rename = "tavilyApiKey")]
    key: Option<String>,
    status: Option<i64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let state = Arc::new(AppState::default());
    let app = Router::new()
        .route("/mcp", post(handle_mcp).get(handle_mcp))
        .route("/mcp/*path", post(handle_mcp).get(handle_mcp))
        .route("/search", post(handle_http_search))
        .route("/extract", post(handle_http_extract))
        .route("/crawl", post(handle_http_crawl))
        .route("/map", post(handle_http_map))
        .route("/admin/keys", post(add_key).get(list_keys))
        .route("/admin/keys/:secret", patch(update_key).delete(delete_key))
        .route(
            "/admin/force-response",
            post(set_forced_response).delete(clear_forced_response),
        )
        .route("/admin/state", get(read_state))
        .with_state(state);

    println!("Mock Tavily upstream listening on http://{}", cli.bind);
    axum::serve(tokio::net::TcpListener::bind(cli.bind).await?, app).await?;
    Ok(())
}

async fn add_key(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AddKeyRequest>,
) -> (StatusCode, Json<Value>) {
    let mut guard = state.inner.write().await;
    let remaining = payload.remaining.unwrap_or(payload.limit);
    guard.keys.insert(
        payload.secret.clone(),
        KeyRecord {
            limit: payload.limit,
            remaining,
        },
    );
    (
        StatusCode::CREATED,
        Json(json!({ "secret": payload.secret, "limit": payload.limit, "remaining": remaining })),
    )
}

async fn update_key(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(payload): Json<UpdateKeyRequest>,
) -> (StatusCode, Json<Value>) {
    let mut guard = state.inner.write().await;
    if let Some(entry) = guard.keys.get_mut(&secret) {
        if let Some(limit) = payload.limit {
            entry.limit = limit.max(0);
        }
        if let Some(remaining) = payload.remaining {
            entry.remaining = remaining.max(0);
        }
        (
            StatusCode::OK,
            Json(json!({ "secret": secret, "limit": entry.limit, "remaining": entry.remaining })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown key" })),
        )
    }
}

async fn delete_key(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
) -> (StatusCode, Json<Value>) {
    let mut guard = state.inner.write().await;
    if guard.keys.remove(&secret).is_some() {
        (StatusCode::NO_CONTENT, Json(json!({})))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown key" })),
        )
    }
}

async fn list_keys(State(state): State<Arc<AppState>>) -> Json<Value> {
    let guard = state.inner.read().await;
    let keys: Vec<_> = guard
        .keys
        .iter()
        .map(|(secret, record)| json!({ "secret": secret, "limit": record.limit, "remaining": record.remaining }))
        .collect();
    Json(json!({ "keys": keys, "forced": guard.forced }))
}

async fn set_forced_response(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ForceRequest>,
) -> (StatusCode, Json<Value>) {
    if payload.http_status.is_none()
        && payload.structured_status.is_none()
        && payload.body.is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "One of http_status, structured_status, or body is required" })),
        );
    }
    let mut guard = state.inner.write().await;
    guard.forced = Some(ForcedResponse {
        http_status: payload.http_status,
        structured_status: payload.structured_status,
        body: payload.body,
        once: payload.once,
        delay_ms: payload.delay_ms,
    });
    (StatusCode::OK, Json(json!({ "forced": guard.forced })))
}

async fn clear_forced_response(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    let mut guard = state.inner.write().await;
    guard.forced = None;
    (StatusCode::NO_CONTENT, Json(json!({})))
}

async fn read_state(State(state): State<Arc<AppState>>) -> Json<Value> {
    let guard = state.inner.read().await;
    Json(json!({ "keys": guard.keys, "forced": guard.forced }))
}

#[derive(Clone, Copy)]
enum HttpEndpoint {
    Search,
    Extract,
    Crawl,
    Map,
}

async fn handle_http_json(
    state: &AppState,
    endpoint: HttpEndpoint,
    body: Value,
) -> (StatusCode, Json<Value>) {
    let key = body
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let key = match key {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "missing api_key" })),
            );
        }
    };

    let forced = {
        let mut guard = state.inner.write().await;
        let forced = guard.forced.clone();
        if guard.forced.as_ref().is_some_and(|force| force.once) {
            guard.forced = None;
        }
        forced
    };

    if let Some(force) = forced {
        if let Some(delay) = force.delay_ms {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
        if let Some(status) = force.http_status {
            let body = force
                .body
                .unwrap_or_else(|| json!({ "error": format!("forced status {status}") }));
            return (
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                Json(body),
            );
        }
        if let Some(custom) = force.body {
            return (StatusCode::OK, Json(custom));
        }
        let structured_status = force.structured_status.unwrap_or(200);
        return match endpoint {
            HttpEndpoint::Search => (
                StatusCode::OK,
                Json(json!({
                    "query": body.get("query").and_then(|v| v.as_str()).unwrap_or(""),
                    "results": [],
                    "answer": null,
                    "images": [],
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "forced-search"
                })),
            ),
            HttpEndpoint::Extract => (
                StatusCode::OK,
                Json(json!({
                    "results": [],
                    "failed_results": [],
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "forced-extract"
                })),
            ),
            HttpEndpoint::Crawl => (
                StatusCode::OK,
                Json(json!({
                    "base_url": body.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                    "results": [],
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "forced-crawl"
                })),
            ),
            HttpEndpoint::Map => (
                StatusCode::OK,
                Json(json!({
                    "base_url": body.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                    "results": [],
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "forced-map"
                })),
            ),
        };
    }

    let mut guard = state.inner.write().await;
    let entry = match guard.keys.get_mut(&key) {
        Some(entry) => entry,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid key" })),
            );
        }
    };

    if entry.remaining <= 0 {
        return quota_response("quota_exhausted", 432);
    }

    entry.remaining -= 1;

    let structured_status = 200;
    match endpoint {
        HttpEndpoint::Search => {
            let query = body.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let results = vec![json!({
                "url": "https://example.com/search",
                "title": "Example Search Result",
                "content": "Example content",
                "raw_content": "Example raw content",
                "score": 0.99,
                "published_date": null,
                "favicon": null
            })];
            (
                StatusCode::OK,
                Json(json!({
                    "query": query,
                    "results": results,
                    "answer": null,
                    "images": [],
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "mock-search-req",
                    "remaining_requests": entry.remaining
                })),
            )
        }
        HttpEndpoint::Extract => {
            let urls = body
                .get("urls")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_else(|| vec![Value::String("https://example.com".into())]);
            let results: Vec<Value> = urls
                .into_iter()
                .map(|u| {
                    let url_str = u.as_str().unwrap_or("https://example.com").to_string();
                    json!({
                        "url": url_str,
                        "raw_content": "mock extracted content",
                        "images": [],
                        "favicon": null
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(json!({
                    "results": results,
                    "failed_results": [],
                    "response_time": 0.02,
                    "status": structured_status,
                    "request_id": "mock-extract-req",
                    "remaining_requests": entry.remaining
                })),
            )
        }
        HttpEndpoint::Crawl => {
            let url = body
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://example.com");
            let results = vec![json!({
                "url": url,
                "raw_content": "mock crawled content",
                "images": [],
                "favicon": null
            })];
            (
                StatusCode::OK,
                Json(json!({
                    "base_url": url,
                    "results": results,
                    "response_time": 0.03,
                    "status": structured_status,
                    "request_id": "mock-crawl-req",
                    "remaining_requests": entry.remaining
                })),
            )
        }
        HttpEndpoint::Map => {
            let url = body
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://example.com");
            let results = vec![json!({
                "url": url,
                "links": [],
            })];
            (
                StatusCode::OK,
                Json(json!({
                    "base_url": url,
                    "results": results,
                    "response_time": 0.01,
                    "status": structured_status,
                    "request_id": "mock-map-req",
                    "remaining_requests": entry.remaining
                })),
            )
        }
    }
}

async fn handle_http_search(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    handle_http_json(&state, HttpEndpoint::Search, body).await
}

async fn handle_http_extract(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    handle_http_json(&state, HttpEndpoint::Extract, body).await
}

async fn handle_http_crawl(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    handle_http_json(&state, HttpEndpoint::Crawl, body).await
}

async fn handle_http_map(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    handle_http_json(&state, HttpEndpoint::Map, body).await
}

async fn handle_mcp(
    State(state): State<Arc<AppState>>,
    Query(query): Query<McpQuery>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    let key = query.key.or_else(|| {
        headers
            .get("tavily-api-key")
            .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
    });
    let key = match key {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "missing tavilyApiKey" })),
            );
        }
    };

    let forced = {
        let mut guard = state.inner.write().await;
        let forced = guard.forced.clone();
        if guard.forced.as_ref().is_some_and(|force| force.once) {
            guard.forced = None;
        }
        forced
    };

    if let Some(force) = forced {
        if let Some(delay) = force.delay_ms {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
        if let Some(status) = force.http_status {
            let body = force
                .body
                .unwrap_or_else(|| json!({ "error": format!("forced status {status}") }));
            return (
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                Json(body),
            );
        }
        if let Some(custom) = force.body {
            return (StatusCode::OK, Json(custom));
        }
        let structured_status = force.structured_status.unwrap_or(200);
        return (
            StatusCode::OK,
            Json(json!({
                "result": {
                    "structuredContent": {
                        "status": structured_status,
                        "forced": true
                    }
                }
            })),
        );
    }

    let mut guard = state.inner.write().await;
    let entry = match guard.keys.get_mut(&key) {
        Some(entry) => entry,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid key" })),
            );
        }
    };

    if entry.remaining <= 0 {
        return quota_response("quota_exhausted", 432);
    }

    entry.remaining -= 1;

    let structured_status = query.status.unwrap_or(200);
    let mut payload = Map::new();
    payload.insert("status".into(), Value::Number(structured_status.into()));
    if let Some(Json(body_value)) = body {
        payload.insert("echo".into(), body_value);
    }
    payload.insert("remaining".into(), Value::Number(entry.remaining.into()));

    (
        StatusCode::OK,
        Json(json!({
            "result": {
                "structuredContent": Value::Object(payload)
            }
        })),
    )
}

fn quota_response(reason: &str, status: i64) -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "result": {
                "structuredContent": {
                    "status": status,
                    "error": reason
                }
            }
        })),
    )
}
