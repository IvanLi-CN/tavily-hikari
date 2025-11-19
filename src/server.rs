use std::{
    collections::HashMap,
    fs,
    io::Read,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use async_stream::stream;
use axum::http::header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Router,
    body::{self, Body},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, Method, Request, Response, StatusCode},
    response::{Json, Redirect},
    routing::{any, delete, get, patch, post},
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use futures_util::Stream;
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderValue as ReqHeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
type SummarySig = (i64, i64, i64, i64, i64, i64, Option<i64>);
use std::time::Duration;
use tavily_hikari::{
    ApiKeyMetrics, AuthToken, ProxyError, ProxyRequest, ProxyResponse, ProxySummary, QuotaWindow,
    RequestLogRecord, TOKEN_DAILY_LIMIT, TOKEN_HOURLY_LIMIT, TOKEN_MONTHLY_LIMIT, TavilyProxy,
    TokenHourlyBucket, TokenLogRecord, TokenQuotaVerdict, TokenSummary, TokenUsageBucket,
};
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
    usage_base: String,
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
        // direct get
        if let Some(name) = self.user_header() {
            if let Some(value) = headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .filter(|v| !v.is_empty())
            {
                return Some(value);
            }
            // fallback: scan case-insensitively in case upstream mutated header casing
            let target = name.as_str();
            for (k, v) in headers.iter() {
                let Ok(s) = v.to_str() else {
                    continue;
                };
                if k.as_str().eq_ignore_ascii_case(target) && !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }

    fn nickname_value(&self, headers: &HeaderMap) -> Option<String> {
        self.nickname_header()
            .and_then(|name| headers.get(name))
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    fn is_request_admin(&self, headers: &HeaderMap) -> bool {
        if !self.is_enabled() {
            return false;
        }

        match (self.admin_value(), self.user_value(headers)) {
            (Some(expected), Some(actual)) => actual == expected,
            _ => false,
        }
    }
}

fn parse_iso_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .ok()
}

fn default_since(period: Option<&str>) -> i64 {
    let now = Utc::now();
    match period {
        Some("day") => now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp(),
        Some("week") => {
            let weekday = now.weekday().num_days_from_monday() as i64;
            (now - ChronoDuration::days(weekday))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        }
        _ => {
            let first = Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()
                .expect("valid start of month");
            first.timestamp()
        }
    }
}

fn default_until(period: Option<&str>, since: i64) -> i64 {
    let base = DateTime::<Utc>::from_timestamp(since, 0).unwrap_or_else(Utc::now);
    match period {
        Some("day") => (base + ChronoDuration::days(1)).timestamp(),
        Some("week") => (base + ChronoDuration::days(7)).timestamp(),
        _ => {
            let date = base.date_naive();
            let (year, month) = if date.month() == 12 {
                (date.year() + 1, 1)
            } else {
                (date.year(), date.month() + 1)
            };
            let naive = NaiveDate::from_ymd_opt(year, month, 1)
                .unwrap_or(date)
                .and_hms_opt(0, 0, 0)
                .unwrap();
            Utc.from_utc_datetime(&naive).timestamp()
        }
    }
}

#[derive(Debug, Serialize)]
struct IsAdminDebug {
    is_admin: bool,
    user_value: Option<String>,
}

async fn debug_is_admin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<IsAdminDebug>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let cfg = &state.forward_auth;
    let user_value = cfg.user_value(&headers).map(|s| s.to_string());
    let is_admin = cfg.is_request_admin(&headers);
    Ok(Json(IsAdminDebug {
        is_admin,
        user_value,
    }))
}

async fn health_check() -> &'static str {
    "ok"
}

fn random_delay_secs() -> u64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen_range(0..=300)
}

fn twenty_four_hours_secs() -> i64 {
    24 * 60 * 60
}

fn spawn_quota_sync_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            // Initial cycle runs immediately on startup
            let keys = match state
                .proxy
                .list_keys_pending_quota_sync(twenty_four_hours_secs())
                .await
            {
                Ok(list) => list,
                Err(err) => {
                    eprintln!("quota-sync: list pending error: {err}");
                    vec![]
                }
            };

            for key_id in keys {
                let delay = random_delay_secs();
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let job_id = match state
                    .proxy
                    .scheduled_job_start("quota_sync", Some(&key_id), 1)
                    .await
                {
                    Ok(id) => id,
                    Err(err) => {
                        eprintln!("quota-sync: start job error: {err}");
                        continue;
                    }
                };
                match state.proxy.sync_key_quota(&key_id, &state.usage_base).await {
                    Ok((limit, remaining)) => {
                        let msg = format!("limit={limit} remaining={remaining}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "success", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::QuotaDataMissing { reason }) => {
                        let msg = format!("quota_data_missing: {reason}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::UsageHttp { status, body }) => {
                        let msg = format!("usage_http {status}: {body}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(err) => {
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                            .await;
                    }
                }
            }

            // Sleep one hour before next cycle
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_token_usage_rollup_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let job_id = match state
                .proxy
                .scheduled_job_start("token_usage_rollup", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("token-usage-rollup: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(300)).await;
                    continue;
                }
            };

            match state.proxy.rollup_token_usage_stats().await {
                Ok((rows, last_ts)) => {
                    let msg = match last_ts {
                        Some(ts) => format!("rows={rows} last_rollup_ts={ts}"),
                        None => format!("rows={rows} last_rollup_ts=none"),
                    };
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }

            // Run rollup every 5 minutes to keep charts reasonably fresh
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
}

fn spawn_auth_token_logs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let job_id = match state
                .proxy
                .scheduled_job_start("auth_token_logs_gc", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("auth-token-logs-gc: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    continue;
                }
            };

            match state.proxy.gc_auth_token_logs().await {
                Ok(deleted) => {
                    let msg = format!("deleted_rows={deleted}");
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }

            // Run GC once per hour; retention window is enforced inside the proxy.
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

// kept for potential future direct serving; currently ServeDir handles '/'
#[allow(dead_code)]
async fn load_spa_response(
    state: &AppState,
    file_name: &str,
) -> Result<Response<Body>, StatusCode> {
    let Some(dir) = state.static_dir.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let path = dir.join(file_name);
    let Ok(bytes) = tokio::fs::read(path).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn serve_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    // Only auto-redirect to admin when explicit dev convenience flag is enabled.
    // Admin users should still be able to access the public page without forced redirection.
    if state.dev_open_admin {
        return Ok(Redirect::temporary("/admin").into_response());
    }

    let _ = headers; // keep parameter for potential future use
    load_spa_response(state.as_ref(), "index.html").await
}

async fn serve_admin_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    load_spa_response(state.as_ref(), "admin.html").await
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
    // Safer: pass original path via data attribute and read it in script without string concatenation
    let script = format!(
        "<script data-p=\"{}\">!function(){{try{{var s=document.currentScript;var p=s&&s.getAttribute('data-p')||'/';history.replaceState(null,'', p)}}catch(_e){{}}}}()</script>",
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

async fn get_public_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicMetricsView>, StatusCode> {
    state
        .proxy
        .success_breakdown()
        .await
        .map(|metrics| {
            Json(PublicMetricsView {
                monthly_success: metrics.monthly_success,
                daily_success: metrics.daily_success,
            })
        })
        .map_err(|err| {
            eprintln!("public metrics error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenMetricsView {
    monthly_success: i64,
    daily_success: i64,
    daily_failure: i64,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

async fn get_token_metrics_public(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<TokenMetricsView>, StatusCode> {
    // Validate token first
    if !state
        .proxy
        .validate_access_token(&q.token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract id
    let token_id = q
        .token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
        .ok_or(StatusCode::BAD_REQUEST)?;

    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(token_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TokenMetricsView {
        monthly_success,
        daily_success,
        daily_failure,
    }))
}

#[derive(Deserialize)]
struct PublicLogsQuery {
    token: String,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicTokenLogView {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    error_message: Option<String>,
    created_at: i64,
}

impl From<TokenLogRecord> for PublicTokenLogView {
    fn from(r: TokenLogRecord) -> Self {
        Self {
            id: r.id,
            method: r.method,
            path: r.path,
            query: r.query,
            http_status: r.http_status,
            mcp_status: r.mcp_status,
            result_status: r.result_status,
            error_message: r.error_message,
            created_at: r.created_at,
        }
    }
}

fn redact_sensitive(input: &str) -> String {
    // Redact query parameter values like tavilyApiKey=... (case-insensitive)
    let mut s = input.to_string();
    let mut lower = s.to_lowercase();
    let needle = "tavilyapikey=";
    let redacted = "<redacted>";
    let mut offset = 0usize;
    while let Some(pos) = lower[offset..].find(needle) {
        let idx = offset + pos;
        let start = idx + needle.len();
        // find earliest delimiter among &, ), space, quote, newline
        let mut end = s.len();
        for delim in ['&', ')', ' ', '"', '\'', '\n'] {
            if let Some(p) = s[start..].find(delim) {
                end = (start + p).min(end);
            }
        }
        s.replace_range(start..end, redacted);
        lower = s.to_lowercase();
        offset = start + redacted.len();
    }
    // Redact header-like phrase "Tavily-Api-Key: <value>"
    // naive pass: case-insensitive search for "tavily-api-key"
    let mut out = String::new();
    let mut i = 0usize;
    let s_lower = s.to_lowercase();
    while let Some(pos) = s_lower[i..].find("tavily-api-key") {
        let idx = i + pos;
        out.push_str(&s[i..idx]);
        // advance to after possible colon
        let rest = &s[idx..];
        if let Some(colon) = rest.find(':') {
            out.push_str(&s[idx..idx + colon + 1]);
            out.push(' ');
            out.push_str(redacted);
            // skip value until whitespace or line break
            let after = idx + colon + 1;
            let mut end = s.len();
            for delim in ['\n', '\r'] {
                if let Some(p) = s[after..].find(delim) {
                    end = (after + p).min(end);
                }
            }
            i = end;
        } else {
            // no colon, just append token
            out.push_str("tavily-api-key");
            i = idx + "tavily-api-key".len();
        }
    }
    out.push_str(&s[i..]);
    out
}

async fn get_public_logs(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PublicLogsQuery>,
) -> Result<Json<Vec<PublicTokenLogView>>, StatusCode> {
    // Validate full token first
    if !state
        .proxy
        .validate_access_token(&q.token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract short token id
    let token_id = q
        .token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
        .ok_or(StatusCode::BAD_REQUEST)?;

    let limit = q.limit.unwrap_or(20).clamp(1, 20);

    state
        .proxy
        .token_recent_logs(token_id, limit, None)
        .await
        .map(|items| {
            let mapped: Vec<PublicTokenLogView> = items
                .into_iter()
                .map(PublicTokenLogView::from)
                .map(|mut v| {
                    // Redact sensitive patterns across error_message, path and query
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v.path = redact_sensitive(&v.path);
                    if let Some(q) = v.query.as_ref() {
                        v.query = Some(redact_sensitive(q));
                    }
                    v
                })
                .collect();
            Json(mapped)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSnapshot {
    summary: SummaryView,
    keys: Vec<ApiKeyView>,
    logs: Vec<RequestLogView>,
}

async fn sse_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let state = state.clone();

    let stream = stream! {
        let mut last_log_id: Option<i64> = None;
        let mut last_sig: Option<SummarySig> = None;

        // send initial snapshot regardless
        if let Some(event) = build_snapshot_event(&state).await {
            // prime signatures from payload
            if let Ok((sig, latest_id)) = compute_signatures(&state).await {
                last_sig = sig;
                last_log_id = latest_id;
            }
            yield Ok(event);
        }

        loop {
            // detect changes
            match compute_signatures(&state).await {
                Ok((sig, latest_id)) => {
                    if sig != last_sig || latest_id != last_log_id {
                        if let Some(event) = build_snapshot_event(&state).await {
                            yield Ok(event);
                        }
                        last_sig = sig;
                        last_log_id = latest_id;
                    } else {
                        // heartbeat to keep connections alive on proxies
                        let keep = Event::default().event("ping").data("{}");
                        yield Ok(keep);
                    }
                }
                Err(_e) => {
                    // On error, still try to keep connection with heartbeat
                    let keep = Event::default().event("ping").data("{}");
                    yield Ok(keep);
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

#[derive(Deserialize)]
struct PublicEventsQuery {
    token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicMetricsPayload {
    public: PublicMetricsView,
    token: Option<TokenMetricsView>,
}

async fn sse_public(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PublicEventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    let state = state.clone();
    let token_param = q.token.clone();

    let stream = stream! {
        type PublicSig = (i64, i64, Option<(i64, i64, i64)>);
        async fn compute(state: &Arc<AppState>, token_param: &Option<String>) -> Option<(PublicMetricsPayload, PublicSig)> {
            let m = state.proxy.success_breakdown().await.ok()?;
            let public = PublicMetricsView { monthly_success: m.monthly_success, daily_success: m.daily_success };
            let token_sig: Option<(i64,i64,i64)> = if let Some(token) = token_param.as_ref() {
                let valid = state.proxy.validate_access_token(token).await.ok()?;
                if !valid { None } else {
                    let id = token.strip_prefix("th-").and_then(|r| r.split_once('-').map(|(id, _)| id))?;
                    let (ms, ds, df) = state.proxy.token_success_breakdown(id).await.ok()?;
                    Some((ms, ds, df))
                }
            } else { None };
            let token = token_sig.map(|(ms,ds,df)| TokenMetricsView { monthly_success: ms, daily_success: ds, daily_failure: df });
            let sig: PublicSig = (public.monthly_success, public.daily_success, token_sig);
            let payload = PublicMetricsPayload { public, token };
            Some((payload, sig))
        }

        let mut last_sig: Option<PublicSig> = None;
        if let Some((payload, sig)) = compute(&state, &token_param).await {
            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default().event("metrics").data(json));
            last_sig = Some(sig);
        }
        loop {
            if let Some((payload, sig)) = compute(&state, &token_param).await {
                if last_sig != Some(sig) {
                    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("metrics").data(json));
                    last_sig = Some(sig);
                } else {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

async fn build_snapshot_event(state: &Arc<AppState>) -> Option<Event> {
    let summary = state.proxy.summary().await.ok()?;
    let keys = state.proxy.list_api_key_metrics().await.ok()?;
    let logs = state
        .proxy
        .recent_request_logs(DEFAULT_LOG_LIMIT)
        .await
        .ok()?;

    let payload = DashboardSnapshot {
        summary: summary.into(),
        keys: keys.into_iter().map(ApiKeyView::from).collect(),
        logs: logs.into_iter().map(RequestLogView::from).collect(),
    };

    let json = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("snapshot").data(json))
}

async fn compute_signatures(
    state: &Arc<AppState>,
) -> Result<(Option<SummarySig>, Option<i64>), ()> {
    let summary = state.proxy.summary().await.map_err(|_| ())?;
    let logs = state.proxy.recent_request_logs(1).await.map_err(|_| ())?;
    let latest_id = logs.first().map(|l| l.id);
    let sig: Option<SummarySig> = Some((
        summary.total_requests,
        summary.success_count,
        summary.error_count,
        summary.quota_exhausted_count,
        summary.active_keys,
        summary.exhausted_keys,
        summary.last_activity,
    ));
    Ok((sig, latest_id))
}

// ---- Jobs listing ----

#[derive(Deserialize)]
struct JobsQuery {
    limit: Option<usize>,
    group: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedJobsView {
    items: Vec<JobLogView>,
    total: i64,
    page: usize,
    per_page: usize,
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<JobsQuery>,
) -> Result<Json<PaginatedJobsView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.or(q.limit).unwrap_or(10).clamp(1, 100);
    let group = q.group.as_deref().unwrap_or("all");

    state
        .proxy
        .list_recent_jobs_paginated(group, page, per_page)
        .await
        .map(|(items, total)| {
            let view_items = items
                .into_iter()
                .map(|j| JobLogView {
                    id: j.id,
                    job_type: j.job_type,
                    key_id: j.key_id,
                    status: j.status,
                    attempt: j.attempt,
                    message: j.message,
                    started_at: j.started_at,
                    finished_at: j.finished_at,
                })
                .collect();
            Json(PaginatedJobsView {
                items: view_items,
                total,
                page,
                per_page,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- Key detail & manual quota sync ----

async fn get_api_key_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiKeyView>, StatusCode> {
    let items = state
        .proxy
        .list_api_key_metrics()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(found) = items.into_iter().find(|k| k.id == id) {
        Ok(Json(ApiKeyView::from(found)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn post_sync_key_usage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let job_id = state
        .proxy
        .scheduled_job_start("quota_sync/manual", Some(&id), 1)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match state.proxy.sync_key_quota(&id, &state.usage_base).await {
        Ok((limit, remaining)) => {
            let msg = format!("limit={limit} remaining={remaining}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Err(ProxyError::QuotaDataMissing { reason }) => {
            let msg = format!("quota_data_missing: {reason}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&msg))
                .await;
            let body = Json(json!({
                "error": "quota_data_missing",
                "detail": reason,
            }));
            Ok((StatusCode::BAD_REQUEST, body).into_response())
        }
        Err(ProxyError::UsageHttp { status, body }) => {
            let detail = format!("Tavily usage request failed with {status}: {body}");
            let http_status = if status == reqwest::StatusCode::UNAUTHORIZED {
                StatusCode::UNAUTHORIZED
            } else if status == reqwest::StatusCode::FORBIDDEN {
                StatusCode::FORBIDDEN
            } else if status.is_client_error() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&detail))
                .await;
            let body = Json(json!({
                "error": "usage_http",
                "detail": detail,
            }));
            Ok((http_status, body).into_response())
        }
        Err(err) => {
            let reason = err.to_string();
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&reason))
                .await;
            let body = Json(json!({
                "error": "sync_failed",
                "detail": reason,
            }));
            Ok((StatusCode::BAD_GATEWAY, body).into_response())
        }
    }
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
struct AdminDebug {
    dev_open_admin: bool,
}

async fn get_admin_debug(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminDebug>, StatusCode> {
    Ok(Json(AdminDebug {
        dev_open_admin: state.dev_open_admin,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    display_name: Option<String>,
    is_admin: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardAuthDebugView {
    enabled: bool,
    user_header: Option<String>,
    admin_value: Option<String>,
    nickname_header: Option<String>,
}

async fn get_forward_auth_debug(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ForwardAuthDebugView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let cfg = &state.forward_auth;
    Ok(Json(ForwardAuthDebugView {
        enabled: cfg.is_enabled(),
        user_header: cfg.user_header().map(|h| h.to_string()),
        admin_value: None,
        nickname_header: cfg.nickname_header().map(|h| h.to_string()),
    }))
}

async fn debug_headers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        map.insert(
            k.as_str().to_string(),
            serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
        );
    }
    Ok((StatusCode::OK, Json(serde_json::Value::Object(map))))
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

    if state.dev_open_admin {
        return Ok(Json(ProfileView {
            display_name: Some("dev-mode".to_string()),
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
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeyView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
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
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
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
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
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
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
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
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedLogsView {
    items: Vec<RequestLogView>,
    total: i64,
    page: i64,
    per_page: i64,
}

async fn list_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<LogsQuery>,
) -> Result<Json<PaginatedLogsView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 200);

    // Optional result_status filter: normalize to known values.
    let result_status: Option<&str> = match params.result.as_deref().map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("success") => Some("success"),
        Some(v) if v.eq_ignore_ascii_case("error") => Some("error"),
        Some(v) if v.eq_ignore_ascii_case("quota_exhausted") || v.eq_ignore_ascii_case("quota") => {
            Some("quota_exhausted")
        }
        _ => None,
    };

    state
        .proxy
        .recent_request_logs_page(result_status, page, per_page)
        .await
        .map(|(logs, total)| {
            let view_items = logs.into_iter().map(RequestLogView::from).collect();
            Json(PaginatedLogsView {
                items: view_items,
                total,
                page,
                per_page,
            })
        })
        .map_err(|err| {
            eprintln!("list logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

// ----- Access token management handlers -----

#[derive(Debug, Deserialize)]
struct ListTokensQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    group: Option<String>,
    no_group: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListTokensResponse {
    items: Vec<AuthTokenView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenGroupView {
    name: String,
    token_count: i64,
    latest_created_at: i64,
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListTokensQuery>,
) -> Result<Json<ListTokensResponse>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(10).clamp(1, 200);
    let group = q
        .group
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let no_group = q.no_group.unwrap_or(false);

    if no_group {
        match state.proxy.list_access_tokens().await {
            Ok(items) => {
                let filtered: Vec<AuthToken> = items
                    .into_iter()
                    .filter(|t| {
                        t.group_name
                            .as_deref()
                            .map(str::trim)
                            .map(|g| g.is_empty())
                            .unwrap_or(true)
                    })
                    .collect();
                let total = filtered.len() as i64;
                let start = ((page - 1) * per_page).max(0) as usize;
                let end = start.saturating_add(per_page as usize).min(total as usize);
                let slice = if start >= total as usize {
                    Vec::new()
                } else {
                    filtered[start..end].to_vec()
                };
                Ok(Json(ListTokensResponse {
                    items: slice.into_iter().map(AuthTokenView::from).collect(),
                    total,
                    page,
                    per_page,
                }))
            }
            Err(err) => {
                eprintln!("list tokens (no_group filter) error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else if let Some(group) = group {
        match state.proxy.list_access_tokens().await {
            Ok(items) => {
                let filtered: Vec<AuthToken> = items
                    .into_iter()
                    .filter(|t| t.group_name.as_deref() == Some(group.as_str()))
                    .collect();
                let total = filtered.len() as i64;
                let start = ((page - 1) * per_page).max(0) as usize;
                let end = start.saturating_add(per_page as usize).min(total as usize);
                let slice = if start >= total as usize {
                    Vec::new()
                } else {
                    filtered[start..end].to_vec()
                };
                Ok(Json(ListTokensResponse {
                    items: slice.into_iter().map(AuthTokenView::from).collect(),
                    total,
                    page,
                    per_page,
                }))
            }
            Err(err) => {
                eprintln!("list tokens (group filter) error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        match state.proxy.list_access_tokens_paged(page, per_page).await {
            Ok((items, total)) => Ok(Json(ListTokensResponse {
                items: items.into_iter().map(AuthTokenView::from).collect(),
                total,
                page,
                per_page,
            })),
            Err(err) => {
                eprintln!("list tokens error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

#[axum::debug_handler]
async fn list_token_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TokenGroupView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.list_access_tokens().await {
        Ok(tokens) => {
            let mut groups: HashMap<String, TokenGroupView> = HashMap::new();
            for t in tokens {
                let raw = t.group_name.as_deref().map(str::trim).unwrap_or("");
                let key = raw.to_owned();
                let entry = groups.entry(key.clone()).or_insert(TokenGroupView {
                    name: key.clone(),
                    token_count: 0,
                    latest_created_at: t.created_at,
                });
                entry.token_count += 1;
                if t.created_at > entry.latest_created_at {
                    entry.latest_created_at = t.created_at;
                }
            }
            let mut out: Vec<TokenGroupView> = groups.into_values().collect();
            out.sort_by(|a, b| {
                b.latest_created_at
                    .cmp(&a.latest_created_at)
                    .then_with(|| a.name.cmp(&b.name))
            });
            Ok(Json(out))
        }
        Err(err) => {
            eprintln!("list token groups error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[axum::debug_handler]
async fn create_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateTokenRequest>,
) -> Result<(StatusCode, Json<AuthTokenSecretView>), StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .create_access_token(payload.note.as_deref())
        .await
        .map(|secret| {
            (
                StatusCode::CREATED,
                Json(AuthTokenSecretView {
                    token: secret.token,
                }),
            )
        })
        .map_err(|err| {
            eprintln!("create token error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn delete_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .delete_access_token(&id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("delete token error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct UpdateTokenStatus {
    enabled: bool,
}

async fn update_token_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTokenStatus>,
) -> Result<StatusCode, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .set_access_token_enabled(&id, payload.enabled)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("update token status error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct UpdateTokenNote {
    note: String,
}

async fn update_token_note(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTokenNote>,
) -> Result<StatusCode, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .update_access_token_note(&id, payload.note.trim())
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("update token note error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_token_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenSecretView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    match state.proxy.get_access_token_secret(&id).await {
        Ok(Some(secret)) => Ok(Json(AuthTokenSecretView {
            token: secret.token,
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("get token secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[axum::debug_handler]
async fn rotate_token_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenSecretView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .rotate_access_token_secret(&id)
        .await
        .map(|secret| {
            Json(AuthTokenSecretView {
                token: secret.token,
            })
        })
        .map_err(|err| {
            eprintln!("rotate token secret error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct BatchCreateTokenRequest {
    group: String,
    count: usize,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchCreateTokenResponse {
    tokens: Vec<String>,
}

#[axum::debug_handler]
async fn create_tokens_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BatchCreateTokenRequest>,
) -> Result<Json<BatchCreateTokenResponse>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let group = payload.group.trim();
    if group.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let count = payload.count.clamp(1, 1000);
    state
        .proxy
        .create_access_tokens_batch(group, count, payload.note.as_deref())
        .await
        .map(|secrets| {
            Json(BatchCreateTokenResponse {
                tokens: secrets.into_iter().map(|s| s.token).collect(),
            })
        })
        .map_err(|err| {
            eprintln!("batch create tokens error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

pub async fn serve(
    addr: SocketAddr,
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
    usage_base: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState {
        proxy,
        static_dir: static_dir.clone(),
        forward_auth,
        dev_open_admin,
        usage_base: usage_base.clone(),
    });

    if let Some(h) = state.forward_auth.user_header() {
        println!(
            "Forward-Auth: header='{}' admin_value='{}'",
            h,
            state.forward_auth.admin_value().unwrap_or("<none>")
        );
    } else {
        println!(
            "Forward-Auth: disabled (no user header), admin_override={} dev_open_admin={}",
            state.forward_auth.admin_override_name().unwrap_or("<none>"),
            state.dev_open_admin
        );
    }

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/api/debug/headers", get(debug_headers))
        .route("/api/debug/is-admin", get(debug_is_admin))
        .route("/api/debug/forward-auth", get(get_forward_auth_debug))
        .route("/api/debug/admin", get(get_admin_debug))
        .route("/api/public/events", get(sse_public))
        .route("/api/public/logs", get(get_public_logs))
        .route("/api/token/metrics", get(get_token_metrics_public))
        .route("/api/events", get(sse_dashboard))
        .route("/api/version", get(get_versions))
        .route("/api/profile", get(get_profile))
        .route("/api/summary", get(fetch_summary))
        .route("/api/public/metrics", get(get_public_metrics))
        .route("/api/keys", get(list_keys))
        .route("/api/keys", post(create_api_key))
        .route("/api/keys/:id", get(get_api_key_detail))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/keys/:id/secret", get(get_api_key_secret))
        .route("/api/keys/:id", delete(delete_api_key))
        .route("/api/keys/:id/status", patch(update_api_key_status))
        .route("/api/jobs", get(list_jobs))
        .route("/api/logs", get(list_logs))
        // Key details
        .route("/api/keys/:id/metrics", get(get_key_metrics))
        .route("/api/keys/:id/logs", get(get_key_logs))
        // Token details
        .route("/api/tokens/:id", get(get_token_detail))
        .route("/api/tokens/:id/metrics", get(get_token_metrics))
        .route(
            "/api/tokens/:id/metrics/usage-series",
            get(get_token_usage_series),
        )
        .route(
            "/api/tokens/:id/metrics/hourly",
            get(get_token_hourly_breakdown),
        )
        .route("/api/tokens/:id/logs", get(get_token_logs))
        .route("/api/tokens/:id/logs/page", get(get_token_logs_page))
        .route("/api/tokens/:id/events", get(sse_token))
        // Access token management (admin only)
        .route("/api/tokens", get(list_tokens))
        .route("/api/tokens", post(create_token))
        .route("/api/tokens/groups", get(list_token_groups))
        .route("/api/tokens/batch", post(create_tokens_batch))
        .route("/api/tokens/:id", delete(delete_token))
        .route("/api/tokens/:id/status", patch(update_token_status))
        .route("/api/tokens/:id/note", patch(update_token_note))
        .route("/api/tokens/:id/secret", get(get_token_secret))
        .route("/api/tokens/:id/secret/rotate", post(rotate_token_secret));

    if let Some(dir) = static_dir.as_ref() {
        if dir.is_dir() {
            let index_file = dir.join("index.html");
            if index_file.exists() {
                router = router.nest_service("/assets", ServeDir::new(dir.join("assets")));
                router = router.route("/", get(serve_index));
                router = router.route("/admin", get(serve_admin_index));
                router = router.route("/admin/", get(serve_admin_index));
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

    // Spawn background schedulers
    spawn_quota_sync_scheduler(state.clone());
    spawn_token_usage_rollup_scheduler(state.clone());
    spawn_auth_token_logs_gc_scheduler(state.clone());

    axum::serve(
        listener,
        router
            .with_state(state)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    println!("Server shut down gracefully.");
    Ok(())
}

async fn wait_for_ctrl_c() -> &'static str {
    match signal::ctrl_c().await {
        Ok(()) => "ctrl_c",
        Err(err) => {
            eprintln!("Failed to listen for Ctrl+C: {err}");
            "ctrl_c_error"
        }
    }
}

#[cfg(unix)]
async fn wait_for_sigterm() -> &'static str {
    match unix_signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
            "sigterm"
        }
        Err(err) => {
            eprintln!("Failed to listen for SIGTERM: {err}");
            wait_for_ctrl_c().await
        }
    }
}

async fn shutdown_signal() {
    let signal = {
        #[cfg(unix)]
        {
            tokio::select! {
                reason = wait_for_ctrl_c() => reason,
                reason = wait_for_sigterm() => reason,
            }
        }

        #[cfg(not(unix))]
        {
            wait_for_ctrl_c().await
        }
    };

    println!("Shutdown signal ({signal}) received, waiting for in-flight requests to finish...");
}

const BODY_LIMIT: usize = 16 * 1024 * 1024; // 16 MiB 默认限制
const DEFAULT_LOG_LIMIT: usize = 200;

#[derive(Debug, Serialize)]
struct ApiKeyView {
    id: String,
    status: String,
    status_changed_at: Option<i64>,
    last_used_at: Option<i64>,
    deleted_at: Option<i64>,
    quota_limit: Option<i64>,
    quota_remaining: Option<i64>,
    quota_synced_at: Option<i64>,
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
    auth_token_id: Option<String>,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    created_at: i64,
    error_message: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
    forwarded_headers: Vec<String>,
    dropped_headers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobLogView {
    id: i64,
    job_type: String,
    key_id: Option<String>,
    status: String,
    attempt: i64,
    message: Option<String>,
    started_at: i64,
    finished_at: Option<i64>,
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
    total_quota_limit: i64,
    total_quota_remaining: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicMetricsView {
    monthly_success: i64,
    daily_success: i64,
}

// ---- Access Token views ----
#[derive(Debug, Serialize)]
struct AuthTokenView {
    id: String,
    enabled: bool,
    note: Option<String>,
    group: Option<String>,
    total_requests: i64,
    created_at: i64,
    last_used_at: Option<i64>,
    quota_state: String,
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
    quota_hourly_reset_at: Option<i64>,
    quota_daily_reset_at: Option<i64>,
    quota_monthly_reset_at: Option<i64>,
}

impl From<AuthToken> for AuthTokenView {
    fn from(t: AuthToken) -> Self {
        let (
            quota_state,
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
        ) = if let Some(quota) = t.quota {
            (
                quota.state_key().to_string(),
                quota.hourly_used,
                quota.hourly_limit,
                quota.daily_used,
                quota.daily_limit,
                quota.monthly_used,
                quota.monthly_limit,
            )
        } else {
            (
                "normal".to_string(),
                0,
                TOKEN_HOURLY_LIMIT,
                0,
                TOKEN_DAILY_LIMIT,
                0,
                TOKEN_MONTHLY_LIMIT,
            )
        };
        Self {
            id: t.id,
            enabled: t.enabled,
            note: t.note,
            group: t.group_name,
            total_requests: t.total_requests,
            created_at: t.created_at,
            last_used_at: t.last_used_at,
            quota_state,
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
            quota_hourly_reset_at: t.quota_hourly_reset_at,
            quota_daily_reset_at: t.quota_daily_reset_at,
            quota_monthly_reset_at: t.quota_monthly_reset_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuthTokenSecretView {
    token: String,
}

// ---- Token Detail views ----
#[derive(Debug, Serialize)]
struct TokenSummaryView {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    last_activity: Option<i64>,
}

impl From<TokenSummary> for TokenSummaryView {
    fn from(s: TokenSummary) -> Self {
        Self {
            total_requests: s.total_requests,
            success_count: s.success_count,
            error_count: s.error_count,
            quota_exhausted_count: s.quota_exhausted_count,
            last_activity: s.last_activity,
        }
    }
}

#[derive(Debug, Serialize)]
struct TokenLogView {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    error_message: Option<String>,
    created_at: i64,
}

impl From<TokenLogRecord> for TokenLogView {
    fn from(r: TokenLogRecord) -> Self {
        Self {
            id: r.id,
            method: r.method,
            path: r.path,
            query: r.query,
            http_status: r.http_status,
            mcp_status: r.mcp_status,
            result_status: r.result_status,
            error_message: r.error_message,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    result: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KeyMetricsQuery {
    period: Option<String>,
    since: Option<i64>,
}

async fn get_key_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<KeyMetricsQuery>,
) -> Result<Json<SummaryView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let since = if let Some(since) = q.since {
        since
    } else {
        // fallback by period
        let now = chrono::Utc::now();
        match q.period.as_deref() {
            Some("day") => (now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()).timestamp(),
            Some("week") => {
                let weekday = now.weekday().num_days_from_monday() as i64;
                (now - chrono::Duration::days(weekday))
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            }
            _ => {
                // month default
                let first = Utc
                    .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                    .single()
                    .expect("valid start of month");
                first.timestamp()
            }
        }
    };

    state
        .proxy
        .key_summary_since(&id, since)
        .await
        .map(|s| Json(s.into()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct KeyLogsQuery {
    limit: Option<usize>,
    since: Option<i64>,
}

async fn get_key_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<KeyLogsQuery>,
) -> Result<Json<Vec<RequestLogView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let limit = q.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, 500);
    state
        .proxy
        .key_recent_logs(&id, limit, q.since)
        .await
        .map(|logs| Json(logs.into_iter().map(RequestLogView::from).collect()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- Token detail endpoints ----

#[derive(Debug, Deserialize)]
struct TokenMetricsQuery {
    period: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

async fn get_token_metrics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenMetricsQuery>,
) -> Result<Json<TokenSummaryView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_since(q.period.as_deref()));
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_until(q.period.as_deref(), since));

    state
        .proxy
        .token_summary_since(&id, since, Some(until))
        .await
        .map(|s| Json(s.into()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct TokenLogsQuery {
    limit: Option<usize>,
    before: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenHourlyQuery {
    hours: Option<i64>,
}

async fn get_token_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenLogsQuery>,
) -> Result<Json<Vec<TokenLogView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let limit = q.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, 500);
    state
        .proxy
        .token_recent_logs(&id, limit, q.before)
        .await
        .map(|logs| {
            let mapped: Vec<TokenLogView> = logs
                .into_iter()
                .map(TokenLogView::from)
                .map(|mut v| {
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v
                })
                .collect();
            Json(mapped)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct TokenLogsPageQuery {
    page: Option<usize>,
    per_page: Option<usize>,
    since: Option<String>,
    until: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenLogsPageView {
    items: Vec<TokenLogView>,
    page: usize,
    per_page: usize,
    total: i64,
}

#[derive(Debug, Serialize)]
struct TokenHourlyBucketView {
    bucket_start: i64,
    success_count: i64,
    system_failure_count: i64,
    external_failure_count: i64,
}

#[derive(Debug, Serialize)]
struct TokenUsageBucketView {
    bucket_start: i64,
    success_count: i64,
    system_failure_count: i64,
    external_failure_count: i64,
}

async fn get_token_logs_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenLogsPageQuery>,
) -> Result<Json<TokenLogsPageView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 200);
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_since(Some("month")));
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_until(Some("month"), since));
    if until <= since {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .proxy
        .token_logs_page(&id, page, per_page, since, Some(until))
        .await
        .map(|(items, total)| {
            let mapped: Vec<TokenLogView> = items
                .into_iter()
                .map(TokenLogView::from)
                .map(|mut v| {
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v
                })
                .collect();
            Json(TokenLogsPageView {
                items: mapped,
                page,
                per_page,
                total,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_token_hourly_breakdown(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenHourlyQuery>,
) -> Result<Json<Vec<TokenHourlyBucketView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let hours = q.hours.unwrap_or(25);
    state
        .proxy
        .token_hourly_breakdown(&id, hours)
        .await
        .map(|buckets| {
            Json(
                buckets
                    .into_iter()
                    .map(
                        |TokenHourlyBucket {
                             bucket_start,
                             success_count,
                             system_failure_count,
                             external_failure_count,
                         }| TokenHourlyBucketView {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        },
                    )
                    .collect(),
            )
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct UsageSeriesQuery {
    since: Option<String>,
    until: Option<String>,
    bucket_secs: Option<i64>,
}

async fn get_token_usage_series(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<UsageSeriesQuery>,
) -> Result<Json<Vec<TokenUsageBucketView>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let now = Utc::now().timestamp();
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or(now);
    let default_since = until - ChronoDuration::hours(25).num_seconds();
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or(default_since);
    if until <= since {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bucket_secs = q
        .bucket_secs
        .unwrap_or(ChronoDuration::hours(1).num_seconds());
    state
        .proxy
        .token_usage_series(&id, since, until, bucket_secs)
        .await
        .map(|series| {
            Json(
                series
                    .into_iter()
                    .map(
                        |TokenUsageBucket {
                             bucket_start,
                             success_count,
                             system_failure_count,
                             external_failure_count,
                         }| TokenUsageBucketView {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        },
                    )
                    .collect(),
            )
        })
        .map_err(|err| match err {
            ProxyError::Other(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}

async fn get_token_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenView>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let tokens = state
        .proxy
        .list_access_tokens()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match tokens.into_iter().find(|t| t.id == id) {
        Some(t) => Ok(Json(t.into())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, Serialize)]
struct TokenSnapshot {
    summary: TokenSummaryView,
    logs: Vec<TokenLogView>,
}

async fn sse_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !state.dev_open_admin && !state.forward_auth.is_request_admin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let state = state.clone();
    let stream = stream! {
        let mut last_log_id: Option<i64> = None;
        if let Some(event) = build_token_snapshot_event(&state, &id).await { yield Ok(event); }
        if let Ok(logs) = state.proxy.token_recent_logs(&id, 1, None).await {
            last_log_id = logs.first().map(|l| l.id);
        }
        loop {
            match state.proxy.token_recent_logs(&id, 1, None).await {
                Ok(logs) => {
                    let latest = logs.first().map(|l| l.id);
                    if latest != last_log_id {
                        if let Some(event) = build_token_snapshot_event(&state, &id).await { yield Ok(event); }
                        last_log_id = latest;
                    } else {
                        let keep = Event::default().event("ping").data("{}");
                        yield Ok(keep);
                    }
                }
                Err(_) => {
                    let keep = Event::default().event("ping").data("{}");
                    yield Ok(keep);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

async fn build_token_snapshot_event(state: &Arc<AppState>, id: &str) -> Option<Event> {
    let now = Utc::now();
    let month_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()?
        .timestamp();
    let summary = state
        .proxy
        .token_summary_since(id, month_start, None)
        .await
        .ok()?;
    let logs = state
        .proxy
        .token_recent_logs(id, DEFAULT_LOG_LIMIT, None)
        .await
        .ok()?;
    let payload = TokenSnapshot {
        summary: summary.into(),
        logs: logs
            .into_iter()
            .map(TokenLogView::from)
            .map(|mut v| {
                if let Some(err) = v.error_message.as_ref() {
                    v.error_message = Some(redact_sensitive(err));
                }
                v
            })
            .collect(),
    };
    let json = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("snapshot").data(json))
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

    // Require Authorization: Bearer th-<id>-<secret>
    let auth_bearer = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    let token = match auth_bearer
        .as_deref()
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim)
    {
        Some(t) if !t.is_empty() => t.to_string(),
        _ if state.dev_open_admin => "th-dev-override".to_string(),
        _ => {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from("{\"error\":\"missing token\"}"))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let valid = if state.dev_open_admin {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    if !valid {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut headers = clone_headers(&parts.headers);
    // prevent leaking our Authorization to upstream
    headers.remove(axum::http::header::AUTHORIZATION);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let auth_token_id = if state.dev_open_admin {
        Some("dev".to_string())
    } else {
        token
            .strip_prefix("th-")
            .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
            .map(|s| s.to_string())
    };

    let proxy_request = ProxyRequest {
        method: method.clone(),
        path: path.clone(),
        query,
        headers,
        body: body_bytes.clone(),
        auth_token_id,
    };

    let token_id = token
        .strip_prefix("th-")
        .and_then(|rest| rest.split('-').next())
        .map(|s| s.to_string());

    let mut _quota_verdict: Option<TokenQuotaVerdict> = None;
    if let Some(tid) = token_id.as_deref() {
        match state.proxy.check_token_quota(tid).await {
            Ok(verdict) => {
                if !state.dev_open_admin && !verdict.allowed {
                    let message = build_quota_error_message(&verdict);
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            parts.uri.query(),
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            "quota_exhausted",
                            Some(&message),
                        )
                        .await;
                    let response = quota_exceeded_response(&verdict)?;
                    return Ok(response);
                }
                _quota_verdict = Some(verdict);
            }
            Err(err) => {
                eprintln!("quota check failed: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    match state.proxy.proxy_request(proxy_request).await {
        Ok(resp) => {
            if let Some(tid) = token_id.as_deref() {
                // 尝试从 Tavily JSON 回复中解析结构化状态码
                let mut tavily_code: Option<i64> = None;
                let mut result_status = "success";
                #[allow(clippy::collapsible_if)]
                {
                    if let Ok(text) = std::str::from_utf8(&resp.body) {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
                            if let Some(sc) = value
                                .get("result")
                                .and_then(|v| v.get("structuredContent"))
                                .and_then(|v| v.get("status"))
                                .and_then(|v| v.as_i64())
                            {
                                tavily_code = Some(sc);
                                result_status = if sc == 432 {
                                    "quota_exhausted"
                                } else if sc >= 400 {
                                    "error"
                                } else {
                                    "success"
                                };
                            } else if value
                                .get("result")
                                .and_then(|v| v.get("structuredContent"))
                                .and_then(|v| v.get("isError"))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                result_status = "error";
                            }
                        }
                    }
                }

                if result_status == "success" && !resp.status.is_success() {
                    result_status = "error";
                }

                let http_code = resp.status.as_u16() as i64;
                let _ = state
                    .proxy
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        parts.uri.query(),
                        Some(http_code),
                        tavily_code,
                        result_status,
                        None,
                    )
                    .await;
            }
            Ok(build_response(resp))
        }
        Err(err) => {
            eprintln!("proxy error: {err}");
            if let Some(tid) = token_id.as_deref() {
                let err_str = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        parts.uri.query(),
                        None,
                        None,
                        "error",
                        Some(err_str.as_str()),
                    )
                    .await;
            }
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

fn quota_exceeded_response(verdict: &TokenQuotaVerdict) -> Result<Response<Body>, StatusCode> {
    let payload = json!({
        "error": "quota_exceeded",
        "window": verdict.window_name(),
        "hourly": {
            "limit": verdict.hourly_limit,
            "used": verdict.hourly_used,
        },
        "daily": {
            "limit": verdict.daily_limit,
            "used": verdict.daily_used,
        },
        "monthly": {
            "limit": verdict.monthly_limit,
            "used": verdict.monthly_used,
        },
    });

    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_quota_error_message(verdict: &TokenQuotaVerdict) -> String {
    let (limit, used) = quota_window_stats(verdict);
    let window = verdict.window_name().unwrap_or("unknown");
    format!("token quota exceeded on {window} window (limit {limit}, used {used})")
}

fn quota_window_stats(verdict: &TokenQuotaVerdict) -> (i64, i64) {
    match verdict.exceeded_window.unwrap_or(QuotaWindow::Hour) {
        QuotaWindow::Hour => (verdict.hourly_limit, verdict.hourly_used),
        QuotaWindow::Day => (verdict.daily_limit, verdict.daily_used),
        QuotaWindow::Month => (verdict.monthly_limit, verdict.monthly_used),
    }
}

impl From<ApiKeyMetrics> for ApiKeyView {
    fn from(metrics: ApiKeyMetrics) -> Self {
        Self {
            id: metrics.id,
            status: metrics.status,
            status_changed_at: metrics.status_changed_at,
            last_used_at: metrics.last_used_at,
            deleted_at: metrics.deleted_at,
            quota_limit: metrics.quota_limit,
            quota_remaining: metrics.quota_remaining,
            quota_synced_at: metrics.quota_synced_at,
            total_requests: metrics.total_requests,
            success_count: metrics.success_count,
            error_count: metrics.error_count,
            quota_exhausted_count: metrics.quota_exhausted_count,
        }
    }
}

fn decode_body(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

impl From<RequestLogRecord> for RequestLogView {
    fn from(record: RequestLogRecord) -> Self {
        Self {
            id: record.id,
            key_id: record.key_id,
            auth_token_id: record.auth_token_id,
            method: record.method,
            path: record.path,
            query: record.query,
            http_status: record.status_code,
            mcp_status: record.tavily_status_code,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
            request_body: decode_body(&record.request_body),
            response_body: decode_body(&record.response_body),
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
            total_quota_limit: summary.total_quota_limit,
            total_quota_remaining: summary.total_quota_remaining,
        }
    }
}
