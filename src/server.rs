use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{error, info};
use uuid::Uuid;

use crate::{SearchDepth, SearchOptions, TavilyBalancer, TavilyError};

const SESSION_HEADER: &str = "Mcp-Session-Id";

#[derive(Clone)]
struct SessionHandle {
    events: broadcast::Sender<Value>,
}

struct ServerState {
    balancer: TavilyBalancer,
    sessions: Mutex<HashMap<String, SessionHandle>>,
}

impl ServerState {
    fn new(balancer: TavilyBalancer) -> Arc<Self> {
        Arc::new(Self {
            balancer,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    fn create_session(self: &Arc<Self>) -> (String, SessionHandle) {
        let session_id = Uuid::new_v4().to_string();
        let (tx, _rx) = broadcast::channel(64);
        let handle = SessionHandle { events: tx };
        self.sessions
            .lock()
            .insert(session_id.clone(), handle.clone());
        (session_id, handle)
    }

    fn get_session(&self, session_id: &str) -> Option<SessionHandle> {
        self.sessions.lock().get(session_id).cloned()
    }

    async fn call_search(&self, args: TavilySearchArgs) -> Result<Value, TavilyError> {
        let query = args.query.trim();
        let options = SearchOptions {
            search_depth: args.search_depth,
            max_results: args.max_results,
            include_answer: args.include_answer,
            include_images: args.include_images,
            include_raw_content: args.include_raw_content,
            include_domains: sanitize_optional(args.include_domains),
            exclude_domains: sanitize_optional(args.exclude_domains),
        };

        self.balancer.search(query, &options).await
    }
}

#[derive(Debug, Deserialize)]
struct TavilySearchArgs {
    query: String,
    #[serde(default)]
    search_depth: Option<SearchDepth>,
    #[serde(default)]
    max_results: Option<u8>,
    #[serde(default)]
    include_answer: Option<bool>,
    #[serde(default)]
    include_images: Option<bool>,
    #[serde(default)]
    include_raw_content: Option<bool>,
    #[serde(default)]
    include_domains: Option<Vec<String>>,
    #[serde(default)]
    exclude_domains: Option<Vec<String>>,
}

pub async fn serve(
    addr: std::net::SocketAddr,
    balancer: TavilyBalancer,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let state = ServerState::new(balancer);

    let app = Router::new()
        .route("/mcp", get(handle_sse).post(handle_rpc))
        .with_state(state);

    info!(%addr, "Starting MCP server");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

async fn handle_rpc(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return bad_request(None, format!("invalid JSON: {err}")),
    };

    let id = payload.get("id").cloned();
    let method = match payload.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => return bad_request(id, "missing method".to_string()),
    };

    match method {
        "initialize" => handle_initialize(state, id, payload),
        "tools/list" => handle_tools_list(state, headers, id),
        "tools/call" => handle_tools_call(state, headers, id, payload).await,
        other => method_not_found(id, other),
    }
}

fn handle_initialize(state: Arc<ServerState>, id: Option<Value>, payload: Value) -> Response {
    let params = payload.get("params");
    let protocol_version = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");

    let (session_id, _handle) = state.create_session();

    let result = json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": {
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "tavily-hikari",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    });

    let mut response = success_response(result);
    if let Ok(value) = HeaderValue::from_str(&session_id) {
        response.headers_mut().insert(SESSION_HEADER, value);
    }
    response
}

fn handle_tools_list(state: Arc<ServerState>, headers: HeaderMap, id: Option<Value>) -> Response {
    if get_session_header(&state, &headers).is_none() {
        return missing_session(id);
    }

    let result = json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": {
            "tools": [tool_definition()],
        }
    });

    success_response(result)
}

async fn handle_tools_call(
    state: Arc<ServerState>,
    headers: HeaderMap,
    id: Option<Value>,
    payload: Value,
) -> Response {
    if get_session_header(&state, &headers).is_none() {
        return missing_session(id);
    }

    let params = match payload.get("params") {
        Some(params) => params,
        None => return invalid_params(id, "missing params"),
    };

    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => return invalid_params(id, "missing tool name"),
    };

    if name != "tavily.search" {
        return method_not_found(id, name);
    }

    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    let args: TavilySearchArgs = match serde_json::from_value(arguments) {
        Ok(args) => args,
        Err(err) => return invalid_params(id, format!("invalid arguments: {err}")),
    };

    match state.call_search(args).await {
        Ok(response_json) => {
            let pretty = serde_json::to_string_pretty(&response_json)
                .unwrap_or_else(|_| response_json.to_string());

            let result = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": pretty,
                        }
                    ],
                    "structuredContent": response_json,
                    "isError": false
                }
            });

            success_response(result)
        }
        Err(err) => {
            error!("tool_call_error" = %err);
            let message = format!("{err}");
            let result = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": message,
                        }
                    ],
                    "isError": true
                }
            });
            success_response(result)
        }
    }
}

async fn handle_sse(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let Some(session) = get_session_header(&state, &headers) else {
        return (StatusCode::BAD_REQUEST, "missing Mcp-Session-Id header").into_response();
    };

    let receiver = session.events.subscribe();

    let event_stream = BroadcastStream::new(receiver).filter_map(|event| async move {
        match event {
            Ok(value) => match Event::default().json_data(value) {
                Ok(event) => Some(event),
                Err(_) => None,
            },
            Err(_) => None,
        }
    });

    let keep_alive_stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(30)))
            .map(|_| Event::default().comment("keep-alive"));

    let stream = futures::stream::select(event_stream, keep_alive_stream)
        .map(|event| Ok::<Event, std::convert::Infallible>(event));

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
        .into_response()
}

fn get_session_header<'a>(
    state: &'a Arc<ServerState>,
    headers: &'a HeaderMap,
) -> Option<SessionHandle> {
    let value = headers.get(SESSION_HEADER)?.to_str().ok()?;
    state.get_session(value)
}

fn tool_definition() -> Value {
    json!({
        "name": "tavily.search",
        "description": "Perform a Tavily search using load-balanced API keys. Returns the raw Tavily JSON payload.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string",
                },
                "search_depth": {
                    "type": "string",
                    "enum": ["basic", "advanced"],
                    "description": "Search depth level"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "description": "Maximum number of results"
                },
                "include_answer": {
                    "type": "boolean",
                    "description": "Request synthesized answer"
                },
                "include_images": {
                    "type": "boolean",
                    "description": "Include image URLs"
                },
                "include_raw_content": {
                    "type": "boolean",
                    "description": "Include raw content bodies"
                },
                "include_domains": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Restrict to specific domains"
                },
                "exclude_domains": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Exclude specific domains"
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }
    })
}

fn success_response(value: Value) -> Response {
    (StatusCode::OK, axum::Json(value)).into_response()
}

fn bad_request(id: Option<Value>, message: String) -> Response {
    json_error(id, -32600, message)
}

fn invalid_params(id: Option<Value>, message: impl Into<String>) -> Response {
    json_error(id, -32602, message.into())
}

fn method_not_found(id: Option<Value>, method: &str) -> Response {
    json_error(id, -32601, format!("method not found: {method}"))
}

fn missing_session(id: Option<Value>) -> Response {
    json_error(
        id,
        -32002,
        "missing or invalid Mcp-Session-Id header".into(),
    )
}

fn json_error(id: Option<Value>, code: i32, message: String) -> Response {
    let error = json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message,
        }
    });
    (StatusCode::OK, axum::Json(error)).into_response()
}

fn sanitize_optional(items: Option<Vec<String>>) -> Option<Vec<String>> {
    let items = items.map(|values| {
        values
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    });
    items.filter(|list| !list.is_empty())
}
