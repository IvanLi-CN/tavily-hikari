use std::{
    collections::{HashMap, HashSet},
    env, fs,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use prometheus::{
    Counter, Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
    TextEncoder,
};
use tokio::{sync::watch, task::JoinHandle};

const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

const HTTP_LABELS: &[&str] = &["route_family", "method", "status_class", "outcome"];
const HTTP_DURATION_LABELS: &[&str] = &["route_family", "method", "outcome", "phase"];
const UPSTREAM_LABELS: &[&str] = &["upstream_class", "operation", "outcome", "status_class"];
const UPSTREAM_DURATION_LABELS: &[&str] = &["upstream_class", "operation", "outcome", "phase"];
const SQLITE_WAIT_LABELS: &[&str] = &["workload_class", "operation", "reason"];
const SQLITE_TRANSACTION_LABELS: &[&str] = &["workload_class", "operation", "outcome"];
const BACKGROUND_LABELS: &[&str] = &["operation", "outcome", "reason"];

#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    recording_enabled: bool,
    http_requests: IntCounterVec,
    http_duration: HistogramVec,
    upstream_requests: IntCounterVec,
    upstream_duration: HistogramVec,
    sqlite_wait_duration: HistogramVec,
    sqlite_transactions: IntCounterVec,
    background_tasks: IntCounterVec,
    process_cpu_seconds: Counter,
    process_memory_bytes: IntGaugeVec,
    observability_flush: IntCounterVec,
    observability_dropped: IntCounterVec,
    route_allowlist: Option<HashSet<&'static str>>,
    last_cpu_seconds: Mutex<Option<f64>>,
}

impl Metrics {
    pub fn new(route_allowlist: Option<HashSet<&'static str>>) -> Result<Self, prometheus::Error> {
        Self::new_with_recording(route_allowlist, true)
    }

    fn new_with_recording(
        route_allowlist: Option<HashSet<&'static str>>,
        recording_enabled: bool,
    ) -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        let const_labels = common_const_labels();
        let http_requests = IntCounterVec::new(
            Opts::new(
                "hikari_http_requests_total",
                "Completed or terminally rejected HTTP and MCP requests.",
            )
            .const_labels(const_labels.clone()),
            HTTP_LABELS,
        )?;
        let http_duration = HistogramVec::new(
            HistogramOpts::new(
                "hikari_http_duration_seconds",
                "HTTP or MCP request and phase duration in seconds.",
            )
            .const_labels(const_labels.clone())
            .buckets(HISTOGRAM_BUCKETS.to_vec()),
            HTTP_DURATION_LABELS,
        )?;
        let upstream_requests = IntCounterVec::new(
            Opts::new(
                "hikari_upstream_requests_total",
                "Completed upstream attempts.",
            )
            .const_labels(const_labels.clone()),
            UPSTREAM_LABELS,
        )?;
        let upstream_duration = HistogramVec::new(
            HistogramOpts::new(
                "hikari_upstream_duration_seconds",
                "Upstream attempt duration in seconds.",
            )
            .const_labels(const_labels.clone())
            .buckets(HISTOGRAM_BUCKETS.to_vec()),
            UPSTREAM_DURATION_LABELS,
        )?;
        let sqlite_wait_duration = HistogramVec::new(
            HistogramOpts::new(
                "hikari_sqlite_wait_duration_seconds",
                "SQLite pool and transaction wait duration in seconds.",
            )
            .const_labels(const_labels.clone())
            .buckets(HISTOGRAM_BUCKETS.to_vec()),
            SQLITE_WAIT_LABELS,
        )?;
        let sqlite_transactions = IntCounterVec::new(
            Opts::new(
                "hikari_sqlite_transactions_total",
                "SQLite transaction outcomes.",
            )
            .const_labels(const_labels.clone()),
            SQLITE_TRANSACTION_LABELS,
        )?;
        let background_tasks = IntCounterVec::new(
            Opts::new(
                "hikari_background_tasks_total",
                "Background, maintenance, and reconciliation task outcomes.",
            )
            .const_labels(const_labels.clone()),
            BACKGROUND_LABELS,
        )?;
        let process_cpu_seconds = Counter::with_opts(
            Opts::new(
                "hikari_process_cpu_seconds_total",
                "Accumulated process CPU time in seconds.",
            )
            .const_labels(const_labels.clone()),
        )?;
        let process_memory_bytes = IntGaugeVec::new(
            Opts::new(
                "hikari_process_memory_bytes",
                "Current process memory measurements in bytes.",
            )
            .const_labels(const_labels.clone()),
            &["reason"],
        )?;
        let observability_flush = IntCounterVec::new(
            Opts::new(
                "hikari_observability_flush_total",
                "Exporter scrape and recorder flush outcomes.",
            )
            .const_labels(const_labels.clone()),
            &["outcome", "reason"],
        )?;
        let observability_dropped = IntCounterVec::new(
            Opts::new(
                "hikari_observability_dropped_total",
                "Samples dropped by bounded observability limits.",
            )
            .const_labels(const_labels),
            &["reason"],
        )?;

        for collector in [
            Box::new(http_requests.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(http_duration.clone()),
            Box::new(upstream_requests.clone()),
            Box::new(upstream_duration.clone()),
            Box::new(sqlite_wait_duration.clone()),
            Box::new(sqlite_transactions.clone()),
            Box::new(background_tasks.clone()),
            Box::new(process_cpu_seconds.clone()),
            Box::new(process_memory_bytes.clone()),
            Box::new(observability_flush.clone()),
            Box::new(observability_dropped.clone()),
        ] {
            registry.register(collector)?;
        }

        Ok(Self {
            registry,
            recording_enabled,
            http_requests,
            http_duration,
            upstream_requests,
            upstream_duration,
            sqlite_wait_duration,
            sqlite_transactions,
            background_tasks,
            process_cpu_seconds,
            process_memory_bytes,
            observability_flush,
            observability_dropped,
            route_allowlist,
            last_cpu_seconds: Mutex::new(None),
        })
    }

    pub fn record_http(&self, path: &str, method: &str, status: StatusCode, elapsed: Duration) {
        if !self.recording_enabled {
            return;
        }
        let route_family = route_family(path);
        if !self.route_allowed(route_family) {
            self.record_drop("route_allowlist");
            return;
        }
        let method = metric_method(path, method);
        let status_class = status_class(status);
        let outcome = outcome_for_status(status);
        self.http_requests
            .with_label_values(&[route_family, method, status_class, outcome])
            .inc();
        self.http_duration
            .with_label_values(&[route_family, method, outcome, "none"])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_upstream(
        &self,
        upstream_class: &str,
        operation: &str,
        response: Result<StatusCode, ()>,
        elapsed: Duration,
    ) {
        if !self.recording_enabled {
            return;
        }
        let upstream_class = match upstream_class {
            "tavily_http" | "tavily_mcp" | "forward_proxy" => upstream_class,
            _ => "tavily_http",
        };
        let operation = normalize_operation(operation);
        let (status_class, outcome) = match response {
            Ok(status) => (status_class(status), outcome_for_status(status)),
            Err(()) => ("none", "error"),
        };
        self.upstream_requests
            .with_label_values(&[upstream_class, operation, outcome, status_class])
            .inc();
        self.upstream_duration
            .with_label_values(&[upstream_class, operation, outcome, "upstream"])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_sqlite(
        &self,
        workload_class: &str,
        operation: &str,
        pool_wait: Duration,
        begin_wait: Duration,
        outcome: &str,
        reason: &str,
    ) {
        if !self.recording_enabled {
            return;
        }
        let workload_class = normalize_workload_class(workload_class);
        let operation = normalize_operation(operation);
        let outcome = normalize_outcome(outcome);
        let reason = normalize_reason(reason);
        self.sqlite_transactions
            .with_label_values(&[workload_class, operation, outcome])
            .inc();
        if !pool_wait.is_zero() {
            self.sqlite_wait_duration
                .with_label_values(&[workload_class, operation, "pool"])
                .observe(pool_wait.as_secs_f64());
        }
        if !begin_wait.is_zero() {
            self.sqlite_wait_duration
                .with_label_values(&[workload_class, operation, "begin"])
                .observe(begin_wait.as_secs_f64());
        }
        if reason != "none" {
            self.sqlite_wait_duration
                .with_label_values(&[workload_class, operation, reason])
                .observe(pool_wait.saturating_add(begin_wait).as_secs_f64());
        }
    }

    pub fn record_background(&self, operation: &str, outcome: &str, reason: &str) {
        if !self.recording_enabled {
            return;
        }
        self.background_tasks
            .with_label_values(&[
                normalize_operation(operation),
                normalize_outcome(outcome),
                normalize_reason(reason),
            ])
            .inc();
    }

    pub fn record_drop(&self, reason: &str) {
        if !self.recording_enabled {
            return;
        }
        self.observability_dropped
            .with_label_values(&[normalize_reason(reason)])
            .inc();
    }

    pub fn sample_runtime(&self) {
        if !self.recording_enabled {
            return;
        }
        if let Some(cpu_seconds) = process_cpu_seconds() {
            let mut last = self
                .last_cpu_seconds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let delta = last
                .map(|previous| (cpu_seconds - previous).max(0.0))
                .unwrap_or(0.0);
            if delta > 0.0 {
                self.process_cpu_seconds.inc_by(delta);
            }
            *last = Some(cpu_seconds);
        }
        let memory = crate::capture_runtime_memory_snapshot();
        set_memory_gauge(&self.process_memory_bytes, "rss", memory.process_rss_bytes);
        set_memory_gauge(
            &self.process_memory_bytes,
            "limit",
            memory.memory_limit_bytes,
        );
        set_memory_gauge(
            &self.process_memory_bytes,
            "headroom",
            memory.headroom_bytes,
        );
        set_memory_gauge(
            &self.process_memory_bytes,
            "swap",
            memory.process_swap_bytes,
        );
        set_memory_gauge(&self.process_memory_bytes, "hwm", memory.process_hwm_bytes);
        self.record_background("observability_sampler", "success", "sample");
    }

    fn route_allowed(&self, route_family: &'static str) -> bool {
        self.route_allowlist
            .as_ref()
            .is_none_or(|allowlist| allowlist.contains(route_family))
    }

    fn gather(&self) -> Result<Vec<u8>, prometheus::Error> {
        let families = self.registry.gather();
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&families, &mut output)
            .map(|_| output)
    }
}

#[derive(Clone)]
struct MetricsEndpoint {
    metrics: Arc<Metrics>,
    token: Arc<str>,
}

pub struct ExporterHandle {
    stop: watch::Sender<bool>,
    server: JoinHandle<()>,
    sampler: JoinHandle<()>,
}

impl ExporterHandle {
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.server.await;
        let _ = self.sampler.await;
    }
}

static GLOBAL_METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();

pub fn global_metrics() -> Arc<Metrics> {
    GLOBAL_METRICS
        .get_or_init(|| {
            Arc::new(
                Metrics::new_with_recording(
                    route_allowlist_from_env(),
                    env_flag("OBSERVABILITY_ENABLED"),
                )
                .expect("metrics catalog"),
            )
        })
        .clone()
}

pub async fn start_exporter(metrics: Arc<Metrics>) -> Option<ExporterHandle> {
    if !env_flag("OBSERVABILITY_ENABLED") {
        return None;
    }
    let bind = match env::var("METRICS_BIND") {
        Ok(value) => match value.parse::<SocketAddr>() {
            Ok(bind) => bind,
            Err(_) => {
                tracing::warn!(
                    component = "observability",
                    event = "exporter_disabled",
                    reason = "invalid_bind",
                    "metrics exporter disabled because METRICS_BIND is invalid"
                );
                return None;
            }
        },
        Err(_) => {
            tracing::warn!(
                component = "observability",
                event = "exporter_disabled",
                reason = "missing_bind",
                "metrics exporter disabled because METRICS_BIND is not set"
            );
            return None;
        }
    };
    let token_path = match env::var("METRICS_TOKEN_FILE") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => {
            tracing::warn!(
                component = "observability",
                event = "exporter_disabled",
                reason = "missing_token_file",
                "metrics exporter disabled because METRICS_TOKEN_FILE is not set"
            );
            return None;
        }
    };
    let token = match fs::read_to_string(&token_path) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
        Ok(_) => {
            tracing::warn!(
                component = "observability",
                event = "exporter_disabled",
                reason = "empty_token_file",
                "metrics exporter disabled because the token file is empty"
            );
            return None;
        }
        Err(_) => {
            tracing::warn!(
                component = "observability",
                event = "exporter_disabled",
                reason = "unreadable_token_file",
                "metrics exporter disabled because the token file cannot be read"
            );
            return None;
        }
    };
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(_) => {
            tracing::warn!(
                component = "observability",
                event = "exporter_disabled",
                reason = "bind_failed",
                "metrics exporter failed to bind; business service remains available"
            );
            return None;
        }
    };
    let actual_bind = listener.local_addr().ok();
    let endpoint = MetricsEndpoint {
        metrics: metrics.clone(),
        token: Arc::from(token),
    };
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(Arc::new(endpoint));
    let (stop, stop_rx) = watch::channel(false);
    let server_rx = stop_rx.clone();
    let sampler_rx = stop_rx;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let mut receiver = server_rx;
                let _ = receiver.changed().await;
            })
            .await;
    });
    let sampler = tokio::spawn(runtime_sampler(metrics, sampler_rx));
    tracing::info!(
        component = "observability",
        event = "exporter_started",
        bind_addr = ?actual_bind,
        "metrics exporter started"
    );
    Some(ExporterHandle {
        stop,
        server,
        sampler,
    })
}

pub async fn http_metrics_middleware(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().as_str().to_owned();
    let started = Instant::now();
    let response = next.run(req).await;
    global_metrics().record_http(&path, &method, response.status(), started.elapsed());
    response
}

async fn metrics_handler(
    axum::extract::State(endpoint): axum::extract::State<Arc<MetricsEndpoint>>,
    headers: HeaderMap,
) -> Response {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    if provided != Some(endpoint.token.as_ref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match endpoint.metrics.gather() {
        Ok(body) => {
            endpoint
                .metrics
                .observability_flush
                .with_label_values(&["success", "scrape"])
                .inc();
            let mut response = Response::new(body.into());
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
            );
            response
        }
        Err(_) => {
            endpoint
                .metrics
                .observability_flush
                .with_label_values(&["error", "scrape"])
                .inc();
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn runtime_sampler(metrics: Arc<Metrics>, mut stop: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = interval.tick() => metrics.sample_runtime(),
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
            }
        }
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn route_allowlist_from_env() -> Option<HashSet<&'static str>> {
    let value = env::var("OBSERVABILITY_ROUTE_ALLOWLIST").ok()?;
    if value.trim().is_empty() {
        return None;
    }
    let allowlist: HashSet<_> = value
        .split(',')
        .filter_map(|value| allowlisted_route_family(value.trim()))
        .collect();
    Some(allowlist)
}

fn allowlisted_route_family(value: &str) -> Option<&'static str> {
    match value {
        "mcp" => Some("mcp"),
        "http_search" => Some("http_search"),
        "http_extract" => Some("http_extract"),
        "http_crawl" => Some("http_crawl"),
        "http_map" => Some("http_map"),
        "http_research" => Some("http_research"),
        "admin" => Some("admin"),
        "user" => Some("user"),
        "public" => Some("public"),
        "internal" => Some("internal"),
        _ => None,
    }
}

fn route_family(path: &str) -> &'static str {
    if path == "/mcp" || path.starts_with("/mcp/") {
        "mcp"
    } else if path.starts_with("/api/tavily/search") {
        "http_search"
    } else if path.starts_with("/api/tavily/extract") {
        "http_extract"
    } else if path.starts_with("/api/tavily/crawl") {
        "http_crawl"
    } else if path.starts_with("/api/tavily/map") {
        "http_map"
    } else if path.starts_with("/api/tavily/research") {
        "http_research"
    } else if path.starts_with("/api/admin") || path.starts_with("/api/keys") {
        "admin"
    } else if path.starts_with("/api/user") || path.starts_with("/api/tokens") {
        "user"
    } else if path.starts_with("/api/public") || path == "/health" {
        "public"
    } else {
        "internal"
    }
}

fn metric_method(path: &str, method: &str) -> &'static str {
    if path == "/mcp" || path.starts_with("/mcp/") {
        "MCP"
    } else {
        match method {
            "GET" => "GET",
            "POST" => "POST",
            "PUT" => "PUT",
            "PATCH" => "PATCH",
            "DELETE" => "DELETE",
            _ => "GET",
        }
    }
}

fn status_class(status: StatusCode) -> &'static str {
    match status.as_u16() {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "none",
    }
}

fn outcome_for_status(status: StatusCode) -> &'static str {
    if status == StatusCode::TOO_MANY_REQUESTS {
        "rate_limited"
    } else if status == StatusCode::REQUEST_TIMEOUT || status == StatusCode::GATEWAY_TIMEOUT {
        "timeout"
    } else if status.is_success() || status.is_redirection() {
        "success"
    } else {
        "error"
    }
}

fn normalize_operation(operation: &str) -> &'static str {
    match operation {
        "mcp" => "mcp",
        "search" | "http_search" => "search",
        "extract" | "http_extract" => "extract",
        "crawl" | "http_crawl" => "crawl",
        "map" | "http_map" => "map",
        "research" | "http_research" => "research",
        "observability_sampler" => "observability_sampler",
        "background_tasks" => "background_tasks",
        "foreground_job_trigger" => "foreground_job_trigger",
        _ if operation.contains("alert") => "alerts",
        _ if operation.contains("reconcil") => "reconciliation",
        _ if operation.contains("request") => "request",
        _ => "other",
    }
}

fn normalize_workload_class(workload_class: &str) -> &'static str {
    if workload_class.contains("foreground") {
        "foreground"
    } else if workload_class.contains("reconcil") {
        "reconciliation"
    } else if workload_class.contains("maintenance") {
        "maintenance"
    } else {
        "background"
    }
}

fn normalize_outcome(outcome: &str) -> &'static str {
    match outcome {
        "success" => "success",
        "error" => "error",
        "timeout" => "timeout",
        "cancelled" => "cancelled",
        "rate_limited" => "rate_limited",
        "upstream_exhausted" => "upstream_exhausted",
        "degraded" => "degraded",
        _ => "error",
    }
}

fn normalize_reason(reason: &str) -> &'static str {
    match reason {
        "none" => "none",
        "pool" => "pool",
        "begin" => "begin",
        "sample" => "sample",
        "started" => "started",
        "route_allowlist" => "route_allowlist",
        "cardinality" => "cardinality",
        "queue" => "queue",
        "timeout" => "timeout",
        "bulk_busy" => "bulk_busy",
        "sqlite_busy" => "sqlite_busy",
        "database" => "database",
        "deferred" => "deferred",
        "query_deadline" => "query_deadline",
        "foreground_pressure" => "foreground_pressure",
        "pool_pressure" => "pool_pressure",
        "recent_contention" => "recent_contention",
        _ => "other",
    }
}

fn common_const_labels() -> HashMap<String, String> {
    let mut labels = HashMap::new();
    labels.insert("service".to_owned(), "tavily-hikari".to_owned());
    labels.insert(
        "environment".to_owned(),
        bounded_identity("OBSERVABILITY_ENVIRONMENT"),
    );
    labels.insert(
        "instance".to_owned(),
        bounded_identity("OBSERVABILITY_INSTANCE"),
    );
    labels
}

fn bounded_identity(name: &str) -> String {
    let value = env::var(name).unwrap_or_default();
    let value = value.trim();
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn set_memory_gauge(gauge: &IntGaugeVec, reason: &'static str, value: Option<u64>) {
    if let Some(value) = value {
        gauge
            .with_label_values(&[reason])
            .set(value.min(i64::MAX as u64) as i64);
    }
}

fn process_cpu_seconds() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let usage = unsafe { usage.assume_init() };
    let user = usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0;
    let system = usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0;
    Some(user + system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_method_values_are_bounded() {
        assert_eq!(route_family("/api/tavily/search"), "http_search");
        assert_eq!(route_family("/mcp"), "mcp");
        assert_eq!(metric_method("/mcp", "POST"), "MCP");
        assert_eq!(metric_method("/anything", "OPTIONS"), "GET");
    }

    #[test]
    fn metrics_render_only_fixed_dimensions() {
        let metrics = Metrics::new(None).expect("metrics");
        metrics.record_http(
            "/api/tavily/search",
            "POST",
            StatusCode::OK,
            Duration::from_millis(5),
        );
        metrics.record_upstream(
            "tavily_http",
            "search",
            Ok(StatusCode::OK),
            Duration::from_millis(10),
        );
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("hikari_http_requests_total"));
        assert!(output.contains("route_family=\"http_search\""));
        assert!(!output.contains("request_id"));
    }

    #[test]
    fn unknown_dimensions_are_mapped_to_fixed_values() {
        assert_eq!(normalize_operation("user supplied operation"), "other");
        assert_eq!(normalize_reason("user supplied reason"), "other");
        assert_eq!(normalize_workload_class("maintenance_bulk"), "maintenance");
    }

    #[test]
    fn disabled_metrics_do_not_record() {
        let metrics = Metrics::new_with_recording(None, false).expect("metrics");
        metrics.record_http(
            "/api/tavily/search",
            "POST",
            StatusCode::OK,
            Duration::from_millis(5),
        );
        metrics.record_background("background_tasks", "success", "started");
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(!output.contains("hikari_http_requests_total{"));
        assert!(!output.contains("hikari_background_tasks_total{"));
    }

    #[tokio::test]
    async fn metrics_endpoint_requires_bearer_token() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let endpoint = Arc::new(MetricsEndpoint {
            metrics,
            token: Arc::from("test-token"),
        });
        let denied =
            metrics_handler(axum::extract::State(endpoint.clone()), HeaderMap::new()).await;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-token"),
        );
        let allowed = metrics_handler(axum::extract::State(endpoint), headers).await;
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(
            allowed
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; version=0.0.4; charset=utf-8")
        );
    }
}
