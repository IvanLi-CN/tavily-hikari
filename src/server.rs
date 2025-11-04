use std::{
    fs,
    io::Read,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use axum::http::header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use axum::response::IntoResponse;
use axum::{
    Router,
    body::{self, Body},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, Method, Request, Response, StatusCode},
    response::{Json, Redirect},
    routing::{any, delete, get, patch, post},
};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use serde::{Deserialize, Serialize};
use tavily_hikari::{
    ApiKeyMetrics, ProxyRequest, ProxyResponse, ProxySummary, RequestLogRecord, TavilyProxy,
};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
}

#[derive(Clone, Debug)]
pub struct ForwardAuthConfig {
    user_header: Option<HeaderName>,
    admin_value: Option<String>,
    nickname_header: Option<HeaderName>,
    admin_override_name: Option<String>,
}

impl ForwardAuthConfig {
    pub fn new(
        user_header: Option<HeaderName>,
        admin_value: Option<String>,
        nickname_header: Option<HeaderName>,
        admin_override_name: Option<String>,
    ) -> Self {
        Self {
            user_header,
            admin_value,
            nickname_header,
            admin_override_name,
        }
    }

    fn is_enabled(&self) -> bool {
        self.user_header.is_some() || self.admin_override_name.is_some()
    }

    fn user_header(&self) -> Option<&HeaderName> {
        self.user_header.as_ref()
    }

    fn nickname_header(&self) -> Option<&HeaderName> {
        self.nickname_header.as_ref()
    }

    fn admin_value(&self) -> Option<&str> {
        self.admin_value.as_deref()
    }

    fn admin_override_name(&self) -> Option<&str> {
        self.admin_override_name.as_deref()
    }

    fn user_value<'a>(&self, headers: &'a HeaderMap) -> Option<&'a str> {
        self.user_header()
            .and_then(|name| headers.get(name))
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
    }

    fn nickname_value(&self, headers: &HeaderMap) -> Option<String> {
        self.nickname_header()
            .and_then(|name| headers.get(name))
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    fn is_request_admin(&self, headers: &HeaderMap) -> bool {
        if self.admin_override_name().is_some() {
            return true;
        }

        if !self.is_enabled() {
            return false;
        }

        match (self.admin_value(), self.user_value(headers)) {
            (Some(expected), Some(actual)) => actual == expected,
            _ => false,
        }
    }
}

async fn health_check() -> &'static str {
    "ok"
}

// kept for potential future direct serving; currently ServeDir handles '/'
#[allow(dead_code)]
async fn serve_index(State(state): State<Arc<AppState>>) -> Result<Response<Body>, StatusCode> {
    let Some(dir) = state.static_dir.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let path = dir.join("index.html");
    let Ok(bytes) = tokio::fs::read(path).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

const BASE_404_STYLES: &str = r#"
  :root {
    color-scheme: light;
    font-family: 'Inter', 'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    text-rendering: optimizeLegibility;
  }

  body {
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(circle at top left, rgba(99, 102, 241, 0.12), transparent 45%),
      radial-gradient(circle at bottom right, rgba(236, 72, 153, 0.12), transparent 50%),
      #f5f6fb;
    color: #1f2937;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
    }
    body {
      background: radial-gradient(circle at top left, rgba(129, 140, 248, 0.22), transparent 45%),
        radial-gradient(circle at bottom right, rgba(236, 72, 153, 0.18), transparent 50%),
        #0f172a;
      color: #e2e8f0;
    }
  }

  .not-found-shell {
    max-width: 520px;
    width: calc(100% - 48px);
    padding: 48px 40px;
    border-radius: 28px;
    background: rgba(255, 255, 255, 0.82);
    border: 1px solid rgba(15, 23, 42, 0.08);
    backdrop-filter: blur(18px);
    box-shadow: 0 28px 65px rgba(15, 23, 42, 0.12);
    text-align: center;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-shell {
      background: rgba(15, 23, 42, 0.7);
      border: 1px solid rgba(148, 163, 184, 0.18);
      box-shadow: 0 32px 65px rgba(15, 23, 42, 0.5);
    }
  }

  .not-found-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 999px;
    background: rgba(99, 102, 241, 0.16);
    color: #4338ca;
    font-size: 0.85rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .not-found-code {
    margin: 28px 0 12px;
    font-size: clamp(4rem, 13vw, 6rem);
    font-weight: 800;
    line-height: 1;
    letter-spacing: -0.04em;
    color: #4f46e5;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-code {
      color: #a5b4fc;
    }
  }

  .not-found-title {
    margin: 0;
    font-size: clamp(1.5rem, 4vw, 2.25rem);
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .not-found-description {
    margin: 20px 0 30px;
    color: rgba(100, 116, 139, 0.95);
    font-size: 1rem;
    line-height: 1.7;
  }

  @media (prefers-color-scheme: dark) {
    .not-found-description {
      color: rgba(203, 213, 225, 0.82);
    }
  }

  .not-found-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 28px;
  }

  .not-found-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 12px 22px;
    border-radius: 999px;
    font-weight: 600;
    color: #fff;
    background: linear-gradient(135deg, #6366f1, #8b5cf6);
    box-shadow: 0 16px 35px rgba(99, 102, 241, 0.35);
    text-decoration: none;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .not-found-primary:hover {
    transform: translateY(-1px);
    box-shadow: 0 20px 40px rgba(99, 102, 241, 0.4);
  }

  .not-found-footer {
    margin-top: 36px;
    font-size: 0.85rem;
    color: rgba(100, 116, 139, 0.75);
  }

  @media (prefers-color-scheme: dark) {
    .not-found-footer {
      color: rgba(148, 163, 184, 0.78);
    }
  }
"#;

fn find_frontend_css_href(static_dir: Option<&FsPath>) -> Option<String> {
    let dir = static_dir?;
    let index_path = dir.join("index.html");
    let mut s = String::new();
    if fs::File::open(&index_path)
        .ok()?
        .read_to_string(&mut s)
        .is_ok()
    {
        // naive scan for first stylesheet href
        if let Some(idx) = s.find("rel=\"stylesheet\"") {
            let frag = &s[idx..];
            if let Some(href_idx) = frag.find("href=\"") {
                let frag2 = &frag[href_idx + 6..];
                if let Some(end_idx) = frag2.find('\"') {
                    let href = &frag2[..end_idx];
                    return Some(href.to_string());
                }
            }
        }
    }
    None
}

fn load_frontend_css_content(static_dir: Option<&FsPath>) -> Option<String> {
    let dir = static_dir?;
    let href = find_frontend_css_href(Some(dir))?;
    // href like "/assets/index-xxxx.css" => remove leading slash and read from static_dir root
    let rel = href.trim_start_matches('/');
    let path = dir.join(
        rel.strip_prefix("assets/")
            .map(|s| FsPath::new("assets").join(s))
            .unwrap_or_else(|| FsPath::new(rel).to_path_buf()),
    );
    fs::read_to_string(path).ok()
}

#[derive(Deserialize)]
struct FallbackQuery {
    path: Option<String>,
}

async fn not_found_landing(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FallbackQuery>,
) -> Response<Body> {
    let css = load_frontend_css_content(state.static_dir.as_deref());
    let html = build_404_landing_inline(css.as_deref(), q.path.unwrap_or_else(|| "/".to_string()));
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CONTENT_LENGTH, html.len().to_string())
        .body(Body::from(html))
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
}

fn build_404_landing_inline(css_content: Option<&str>, original: String) -> String {
    let mut style_block = String::from("<style>\n");
    style_block.push_str(BASE_404_STYLES);
    if let Some(content) = css_content {
        style_block.push_str(content);
    }
    style_block.push_str("\n</style>\n");
    let script = format!(
        "<script>try{{history.replaceState(null,'', '{}')}}catch(_e){{}}</script>",
        html_escape::encode_double_quoted_attribute(&original)
    );
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"UTF-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n    <title>404 Not Found</title>\n    {}  </head>\n  <body>\n    <main class=\"not-found-shell\" role=\"main\">\n      <span class=\"not-found-badge\" aria-hidden=\"true\">Tavily Hikari Proxy</span>\n      <p class=\"not-found-code\">404</p>\n      <h1 class=\"not-found-title\">Page not found</h1>\n      <p class=\"not-found-description\">The page you’re trying to visit, <code>{}</code>, isn’t available right now.</p>\n      <div class=\"not-found-actions\">\n        <a href=\"/\" class=\"not-found-primary\" aria-label=\"Back to dashboard\">Return to dashboard</a>\n      </div>\n      <p class=\"not-found-footer\">Error reference: 404</p>\n    </main>\n    {}\n  </body>\n</html>",
        style_block,
        html_escape::encode_text(&original),
        script
    )
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionView {
    backend: String,
    frontend: String,
}

async fn get_versions(State(state): State<Arc<AppState>>) -> Result<Json<VersionView>, StatusCode> {
    let (backend, frontend) = detect_versions(state.static_dir.as_deref());
    Ok(Json(VersionView { backend, frontend }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    display_name: Option<String>,
    is_admin: bool,
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ProfileView>, StatusCode> {
    let config = &state.forward_auth;

    if let Some(name) = config.admin_override_name() {
        return Ok(Json(ProfileView {
            display_name: Some(name.to_owned()),
            is_admin: true,
        }));
    }

    if !config.is_enabled() {
        return Ok(Json(ProfileView {
            display_name: None,
            is_admin: false,
        }));
    }

    let user_value = config.user_value(&headers).map(str::to_string);

    let nickname = config
        .nickname_value(&headers)
        .or_else(|| user_value.clone());

    if nickname.is_none() {
        return Ok(Json(ProfileView {
            display_name: None,
            is_admin: false,
        }));
    }

    let is_admin = config.is_request_admin(&headers);

    Ok(Json(ProfileView {
        display_name: nickname,
        is_admin,
    }))
}

fn detect_versions(static_dir: Option<&FsPath>) -> (String, String) {
    let backend_base = option_env!("APP_EFFECTIVE_VERSION")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let backend = if cfg!(debug_assertions) {
        format!("{}-dev", backend_base)
    } else {
        backend_base
    };

    // Try reading version.json produced by front-end build
    let frontend_from_dist = static_dir.and_then(|dir| {
        let path = dir.join("version.json");
        fs::File::open(&path).ok().and_then(|mut f| {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| {
                        v.get("version")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
            } else {
                None
            }
        })
    });

    // Fallback to web/package.json for dev setups
    let frontend = frontend_from_dist
        .or_else(|| {
            let path = FsPath::new("web").join("package.json");
            fs::File::open(&path).ok().and_then(|mut f| {
                let mut s = String::new();
                if f.read_to_string(&mut s).is_ok() {
                    serde_json::from_str::<serde_json::Value>(&s)
                        .ok()
                        .and_then(|v| {
                            v.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let frontend = if cfg!(debug_assertions) {
        format!("{}-dev", frontend)
    } else {
        frontend
    };

    (backend, frontend)
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

#[derive(Debug, Deserialize)]
struct CreateKeyRequest {
    api_key: String,
}

#[derive(Debug, Serialize)]
struct CreateKeyResponse {
    id: String,
}

async fn create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), StatusCode> {
    if !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let api_key = payload.api_key.trim();
    if api_key.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    match state.proxy.add_or_undelete_key(api_key).await {
        Ok(id) => Ok((StatusCode::CREATED, Json(CreateKeyResponse { id }))),
        Err(err) => {
            eprintln!("create api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.soft_delete_key_by_id(&id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(err) => {
            eprintln!("delete api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpdateKeyStatus {
    status: String,
}

async fn update_api_key_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateKeyStatus>,
) -> Result<StatusCode, StatusCode> {
    if !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let status = payload.status.trim().to_ascii_lowercase();
    match status.as_str() {
        "disabled" => match state.proxy.disable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("disable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        "active" => match state.proxy.enable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("enable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn get_api_key_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeySecretView>, StatusCode> {
    if !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.get_api_key_secret(&id).await {
        Ok(Some(secret)) => Ok(Json(ApiKeySecretView { api_key: secret })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("fetch api key secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
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
    forward_auth: ForwardAuthConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState {
        proxy,
        static_dir: static_dir.clone(),
        forward_auth,
    });

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/api/version", get(get_versions))
        .route("/api/profile", get(get_profile))
        .route("/api/summary", get(fetch_summary))
        .route("/api/keys", get(list_keys))
        .route("/api/keys", post(create_api_key))
        .route("/api/keys/:id/secret", get(get_api_key_secret))
        .route("/api/keys/:id", delete(delete_api_key))
        .route("/api/keys/:id/status", patch(update_api_key_status))
        .route("/api/logs", get(list_logs));

    if let Some(dir) = static_dir.as_ref() {
        if dir.is_dir() {
            let index_file = dir.join("index.html");
            if index_file.exists() {
                router = router.nest_service("/assets", ServeDir::new(dir.join("assets")));
                router = router.route("/", get(serve_index));
                router =
                    router.route_service("/favicon.svg", ServeFile::new(dir.join("favicon.svg")));
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

    // 404 landing page that updates URL back to original via history API
    router = router.route("/__404", get(not_found_landing));

    // Fallback: if UA/Accept 支持 HTML 则重定向到 __404；否则返回纯 404
    async fn supports_html(headers: &HeaderMap) -> bool {
        let accept = headers
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if accept.contains("text/html") {
            return true;
        }
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        ua.contains("mozilla/")
    }

    router = router.fallback(|req: Request<Body>| async move {
        let headers = req.headers().clone();
        if supports_html(&headers).await {
            // 302 for GET/HEAD; 303 for others
            let uri = req.uri();
            let pq = uri
                .path_and_query()
                .map(|v| v.as_str())
                .unwrap_or(uri.path());
            let target = format!("/__404?path={}", urlencoding::encode(pq));
            match *req.method() {
                Method::GET | Method::HEAD => Redirect::temporary(&target).into_response(),
                _ => Redirect::to(&target).into_response(), // 303 See Other
            }
        } else {
            (StatusCode::NOT_FOUND, Body::empty()).into_response()
        }
    });

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
    id: String,
    status: String,
    status_changed_at: Option<i64>,
    last_used_at: Option<i64>,
    deleted_at: Option<i64>,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
}

#[derive(Debug, Serialize)]
struct ApiKeySecretView {
    api_key: String,
}

#[derive(Debug, Serialize)]
struct RequestLogView {
    id: i64,
    key_id: String,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    created_at: i64,
    error_message: Option<String>,
    forwarded_headers: Vec<String>,
    dropped_headers: Vec<String>,
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

impl From<ApiKeyMetrics> for ApiKeyView {
    fn from(metrics: ApiKeyMetrics) -> Self {
        Self {
            id: metrics.id,
            status: metrics.status,
            status_changed_at: metrics.status_changed_at,
            last_used_at: metrics.last_used_at,
            deleted_at: metrics.deleted_at,
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
            key_id: record.key_id,
            http_status: record.status_code,
            mcp_status: record.tavily_status_code,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
            forwarded_headers: record.forwarded_headers,
            dropped_headers: record.dropped_headers,
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
