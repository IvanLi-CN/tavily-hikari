use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::http::header::{CONNECTION, CONTENT_LENGTH, TRANSFER_ENCODING};
use axum::{
    Router,
    body::{self, Body},
    extract::{Query, State},
    http::{HeaderMap, Method, Request, Response, StatusCode},
    response::{Json, Redirect},
    routing::{any, get},
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tavily_hikari::{
    ApiKeyMetrics, ProxyRequest, ProxyResponse, ProxySummary, RequestLogRecord, TavilyProxy,
};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
}

async fn health_check() -> &'static str {
    "ok"
}

async fn fetch_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SummaryView>, StatusCode> {
    state
        .proxy
        .summary()
        .await
        .map(|summary| Json(summary.into()))
        .map_err(|err| {
            eprintln!("summary error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ApiKeyView>>, StatusCode> {
    state
        .proxy
        .list_api_key_metrics()
        .await
        .map(|metrics| Json(metrics.into_iter().map(ApiKeyView::from).collect()))
        .map_err(|err| {
            eprintln!("list keys error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn list_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LogsQuery>,
) -> Result<Json<Vec<RequestLogView>>, StatusCode> {
    let limit = params.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, 500);

    state
        .proxy
        .recent_request_logs(limit)
        .await
        .map(|logs| Json(logs.into_iter().map(RequestLogView::from).collect()))
        .map_err(|err| {
            eprintln!("list logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

pub async fn serve(
    addr: SocketAddr,
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState { proxy });

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/api/summary", get(fetch_summary))
        .route("/api/keys", get(list_keys))
        .route("/api/logs", get(list_logs));

    if let Some(dir) = static_dir {
        if dir.is_dir() {
            let index_file = dir.join("index.html");
            if index_file.exists() {
                router = router.route("/", get(|| async { Redirect::temporary("/ui") }));
                router =
                    router.route_service("/favicon.svg", ServeFile::new(dir.join("favicon.svg")));
                router = router.nest_service("/assets", ServeDir::new(dir.join("assets")));
                let index_file_service = ServeFile::new(index_file.clone());
                router = router.route_service("/ui", index_file_service.clone());
                router = router.route_service("/ui/", index_file_service.clone());
                router = router.route_service("/ui/*path", index_file_service);
            } else {
                eprintln!(
                    "static index.html not found at {} — skip serving SPA",
                    index_file.display()
                );
            }
        } else {
            eprintln!("static dir '{}' is not a directory", dir.display());
        }
    }

    router = router
        .route("/mcp", any(proxy_handler))
        .route("/mcp/*path", any(proxy_handler));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;
    println!("Tavily proxy listening on http://{bound_addr}");

    axum::serve(
        listener,
        router
            .with_state(state)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

const BODY_LIMIT: usize = 16 * 1024 * 1024; // 16 MiB 默认限制
const DEFAULT_LOG_LIMIT: usize = 50;

#[derive(Debug, Serialize)]
struct ApiKeyView {
    key_id: String,
    key_preview: String,
    status: String,
    status_changed_at: Option<i64>,
    last_used_at: Option<i64>,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
}

#[derive(Debug, Serialize)]
struct RequestLogView {
    id: i64,
    key_preview: String,
    key_id: String,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    created_at: i64,
    error_message: Option<String>,
}

#[derive(Debug, Serialize)]
struct SummaryView {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    active_keys: i64,
    exhausted_keys: i64,
    last_activity: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let query = parts.uri.query().map(|q| q.to_owned());

    if method == Method::GET && accepts_event_stream(&parts.headers) {
        let response = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(response);
    }

    let headers = clone_headers(&parts.headers);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let proxy_request = ProxyRequest {
        method,
        path,
        query,
        headers,
        body: body_bytes.clone(),
    };

    match state.proxy.proxy_request(proxy_request).await {
        Ok(resp) => Ok(build_response(resp)),
        Err(err) => {
            eprintln!("proxy error: {err}");
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

fn clone_headers(headers: &HeaderMap) -> ReqHeaderMap {
    let mut map = ReqHeaderMap::new();
    for (name, value) in headers.iter() {
        if let Ok(cloned) = ReqHeaderValue::from_bytes(value.as_bytes()) {
            map.insert(name.clone(), cloned);
        }
    }
    map
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        })
        .unwrap_or(false)
}

fn build_response(resp: ProxyResponse) -> Response<Body> {
    let mut builder = Response::builder().status(resp.status);
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in resp.headers.iter() {
            if name == TRANSFER_ENCODING || name == CONNECTION || name == CONTENT_LENGTH {
                continue;
            }
            headers.append(name.clone(), value.clone());
        }
        headers.insert(CONTENT_LENGTH, value_from_len(resp.body.len()));
    }
    builder
        .body(Body::from(resp.body))
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
}

fn value_from_len(len: usize) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(len.to_string().as_str())
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("0"))
}

fn mask_key(key: &str) -> String {
    if key.len() <= 12 {
        return key.to_owned();
    }

    let prefix: String = key.chars().take(6).collect();
    let suffix: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}…{suffix}")
}

fn key_identifier(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest)
}

impl From<ApiKeyMetrics> for ApiKeyView {
    fn from(metrics: ApiKeyMetrics) -> Self {
        Self {
            key_id: key_identifier(&metrics.api_key),
            key_preview: mask_key(&metrics.api_key),
            status: metrics.status,
            status_changed_at: metrics.status_changed_at,
            last_used_at: metrics.last_used_at,
            total_requests: metrics.total_requests,
            success_count: metrics.success_count,
            error_count: metrics.error_count,
            quota_exhausted_count: metrics.quota_exhausted_count,
        }
    }
}

impl From<RequestLogRecord> for RequestLogView {
    fn from(record: RequestLogRecord) -> Self {
        Self {
            id: record.id,
            key_preview: mask_key(&record.api_key),
            key_id: key_identifier(&record.api_key),
            http_status: record.status_code,
            mcp_status: record.tavily_status_code,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
        }
    }
}

impl From<ProxySummary> for SummaryView {
    fn from(summary: ProxySummary) -> Self {
        Self {
            total_requests: summary.total_requests,
            success_count: summary.success_count,
            error_count: summary.error_count,
            quota_exhausted_count: summary.quota_exhausted_count,
            active_keys: summary.active_keys,
            exhausted_keys: summary.exhausted_keys,
            last_activity: summary.last_activity,
        }
    }
}
