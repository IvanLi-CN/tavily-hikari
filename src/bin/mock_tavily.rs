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
