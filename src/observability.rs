use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    env, fs,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::{Duration, Instant},
};

#[cfg(test)]
use std::future::Future;

use axum::extract::Extension;
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
#[cfg(test)]
use tokio::sync::Notify;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

const HTTP_LABELS: &[&str] = &["route_family", "method", "status_class", "outcome"];
const HTTP_DURATION_LABELS: &[&str] = &["route_family", "method", "outcome", "phase"];
const HTTP_REQUEST_DURATION_LABELS: &[&str] =
    &["route_family", "method", "operation", "outcome", "phase"];
const UPSTREAM_LABELS: &[&str] = &["upstream_class", "operation", "outcome", "status_class"];
const UPSTREAM_DURATION_LABELS: &[&str] = &["upstream_class", "operation", "outcome", "phase"];
const SQLITE_WAIT_LABELS: &[&str] = &["workload_class", "operation", "reason"];
const SQLITE_TRANSACTION_LABELS: &[&str] = &["workload_class", "operation", "outcome"];
const SQLITE_ADMISSION_LABELS: &[&str] = &["workload_class", "operation", "reason"];
const BACKGROUND_LABELS: &[&str] = &["operation", "outcome", "reason"];
const MAX_CARDINALITY_TUPLES: usize = 2048;
const CARDINALITY_PROBES: usize = 4;
const EVENT_QUEUE_CAPACITY: usize = 4096;
const EXPORTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_PHASES: [&str; 11] = [
    "configuration",
    "billing_lock_wait",
    "preflight",
    "key_selection",
    "upstream",
    "response_body",
    "persistence",
    "settlement",
    "reconciliation_usage",
    "reconciliation_schedule",
    "other",
];
const REQUEST_PHASE_COUNT: usize = REQUEST_PHASES.len();

#[derive(Debug)]
enum MetricEvent {
    Http {
        route_family: &'static str,
        method: &'static str,
        status_class: &'static str,
        outcome: &'static str,
        elapsed_seconds: f64,
    },
    HttpPhase {
        route_family: &'static str,
        method: &'static str,
        outcome: &'static str,
        phase: &'static str,
        elapsed_seconds: f64,
    },
    HttpRequestTiming {
        route_family: &'static str,
        method: &'static str,
        status_class: &'static str,
        operation: &'static str,
        outcome: &'static str,
        total_seconds: f64,
        phase_seconds: [f64; REQUEST_PHASE_COUNT],
    },
    UpstreamResponse {
        upstream_class: &'static str,
        operation: &'static str,
        outcome: &'static str,
        status_class: &'static str,
        elapsed_seconds: f64,
    },
    UpstreamFailure {
        upstream_class: &'static str,
        operation: &'static str,
        outcome: &'static str,
        elapsed_seconds: f64,
    },
    Sqlite {
        workload_class: &'static str,
        operation: &'static str,
        outcome: &'static str,
        reason: &'static str,
        pool_wait_seconds: f64,
        begin_wait_seconds: f64,
        transaction_admitted: bool,
        admission_metric_admitted: bool,
    },
    Background {
        operation: &'static str,
        outcome: &'static str,
        reason: &'static str,
    },
    Drop {
        reason: &'static str,
    },
}

#[derive(Debug)]
struct RequestTimingState {
    phases: [Duration; REQUEST_PHASE_COUNT],
    active: [Option<ActiveRequestPhase>; REQUEST_PHASE_COUNT],
    active_len: usize,
    next_token: u64,
}

#[derive(Debug)]
struct ActiveRequestPhase {
    token: u64,
    phase: usize,
    started: Instant,
}

#[derive(Debug)]
struct RequestTimingInner {
    metrics: Arc<Metrics>,
    route_family: &'static str,
    method: &'static str,
    terminal: AtomicBool,
    state: Mutex<RequestTimingState>,
}

#[derive(Debug)]
pub(crate) struct RequestTimingScope {
    metrics: Arc<Metrics>,
    route_family: &'static str,
    method: &'static str,
    started: Instant,
    operation: Cell<&'static str>,
    terminal: AtomicBool,
    phase_state: OnceLock<Arc<RequestTimingInner>>,
}

#[derive(Debug)]
pub struct RequestPhaseGuard {
    inner: Arc<RequestTimingInner>,
    token: u64,
}

tokio::task_local! {
    static REQUEST_TIMING_SCOPE: RequestTimingScope;
}

impl RequestTimingScope {
    fn new(metrics: Arc<Metrics>, path: &str, method: &str) -> Self {
        let route_family = route_family(path);
        let method = metric_method(path, method);
        let operation = default_request_operation(route_family);
        Self {
            metrics,
            route_family,
            method,
            started: Instant::now(),
            operation: Cell::new(operation),
            terminal: AtomicBool::new(false),
            phase_state: OnceLock::new(),
        }
    }

    fn phase_inner(&self) -> &Arc<RequestTimingInner> {
        self.phase_state.get_or_init(|| {
            Arc::new(RequestTimingInner {
                metrics: self.metrics.clone(),
                route_family: self.route_family,
                method: self.method,
                terminal: AtomicBool::new(false),
                state: Mutex::new(RequestTimingState {
                    phases: [Duration::ZERO; REQUEST_PHASE_COUNT],
                    active: std::array::from_fn(|_| None),
                    active_len: 0,
                    next_token: 1,
                }),
            })
        })
    }

    fn finish(&self, status: StatusCode) {
        self.finish_with_labels(status_class(status), outcome_for_status(status));
    }

    fn finish_cancelled(&self) {
        self.finish_with_labels("none", "cancelled");
    }

    fn finish_with_labels(&self, status_class: &'static str, outcome: &'static str) {
        if let Some(inner) = self.phase_state.get() {
            let (operation, total, phases) = {
                let mut state = inner
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if inner
                    .terminal
                    .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
                    .is_err()
                {
                    return;
                }
                let now = Instant::now();
                pause_active_phase(&mut state, now);
                let total = now.saturating_duration_since(self.started);
                let phases = reconcile_request_phases(state.phases, total);
                (self.operation.get(), total, phases)
            };
            inner.metrics.enqueue_request_timing(
                inner.route_family,
                inner.method,
                status_class,
                operation,
                outcome,
                total,
                phases,
            );
            return;
        }

        if self
            .terminal
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_err()
        {
            return;
        }
        let now = Instant::now();
        let total = now.saturating_duration_since(self.started);
        let phases = reconcile_request_phases([Duration::ZERO; REQUEST_PHASE_COUNT], total);
        self.metrics.enqueue_request_timing(
            self.route_family,
            self.method,
            status_class,
            self.operation.get(),
            outcome,
            total,
            phases,
        );
    }

    fn set_operation(&self, operation: &str) {
        if self.terminal.load(AtomicOrdering::Acquire)
            || self
                .phase_state
                .get()
                .is_some_and(|inner| inner.terminal.load(AtomicOrdering::Acquire))
        {
            return;
        }
        self.operation.set(normalize_request_operation(operation));
    }

    fn begin_phase(&self, phase: &str) -> Option<RequestPhaseGuard> {
        let phase = request_phase_index(phase)?;
        if self.terminal.load(AtomicOrdering::Acquire) {
            return None;
        }
        let inner = self.phase_inner();
        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.terminal.load(AtomicOrdering::Acquire) {
            return None;
        }
        if state.active_len == REQUEST_PHASE_COUNT {
            return None;
        }
        let now = Instant::now();
        pause_active_phase(&mut state, now);
        let token = state.next_token;
        state.next_token = state.next_token.wrapping_add(1).max(1);
        let active_index = state.active_len;
        state.active[active_index] = Some(ActiveRequestPhase {
            token,
            phase,
            started: now,
        });
        state.active_len += 1;
        Some(RequestPhaseGuard {
            inner: inner.clone(),
            token,
        })
    }

    fn record_phase_duration(&self, phase: &str, elapsed: Duration) {
        let Some(phase) = request_phase_index(phase) else {
            return;
        };
        if self.terminal.load(AtomicOrdering::Acquire) {
            return;
        }
        let inner = self.phase_inner();
        let mut state = inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.terminal.load(AtomicOrdering::Acquire) {
            return;
        }
        let now = Instant::now();
        pause_active_phase(&mut state, now);
        state.phases[phase] = state.phases[phase].saturating_add(elapsed);
        resume_active_phase(&mut state, now);
    }
}

impl Drop for RequestTimingScope {
    fn drop(&mut self) {
        self.finish_cancelled();
    }
}

impl Drop for RequestPhaseGuard {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.inner.terminal.load(AtomicOrdering::Acquire) {
            return;
        }
        if state.active_len == 0 {
            return;
        }
        let active_index = state.active_len - 1;
        let Some(active) = state.active[active_index].as_ref() else {
            return;
        };
        if active.token != self.token {
            return;
        }
        let now = Instant::now();
        pause_active_phase(&mut state, now);
        state.active[active_index] = None;
        state.active_len -= 1;
        resume_active_phase(&mut state, now);
    }
}

fn pause_active_phase(state: &mut RequestTimingState, now: Instant) {
    if state.active_len == 0 {
        return;
    }
    if let Some(active) = state.active[state.active_len - 1].as_mut() {
        state.phases[active.phase] = state.phases[active.phase]
            .saturating_add(now.saturating_duration_since(active.started));
        active.started = now;
    }
}

fn resume_active_phase(state: &mut RequestTimingState, now: Instant) {
    if state.active_len == 0 {
        return;
    }
    if let Some(active) = state.active[state.active_len - 1].as_mut() {
        active.started = now;
    }
}

fn reconcile_request_phases(
    mut phases: [Duration; REQUEST_PHASE_COUNT],
    total: Duration,
) -> [Duration; REQUEST_PHASE_COUNT] {
    let other = REQUEST_PHASE_COUNT - 1;
    let phase_sum = phases
        .iter()
        .fold(Duration::ZERO, |sum, phase| sum.saturating_add(*phase));
    if phase_sum < total {
        phases[other] = phases[other].saturating_add(total - phase_sum);
        return phases;
    }
    if phase_sum == total {
        return phases;
    }
    let total_nanos = total.as_nanos();
    let phase_sum_nanos = phase_sum.as_nanos();
    let mut assigned_nanos = 0_u128;
    for phase in &mut phases {
        let nanos = phase.as_nanos().saturating_mul(total_nanos) / phase_sum_nanos;
        *phase = Duration::from_nanos(nanos.min(u64::MAX as u128) as u64);
        assigned_nanos = assigned_nanos.saturating_add(nanos);
    }
    let remainder = total_nanos.saturating_sub(assigned_nanos);
    phases[other] =
        phases[other].saturating_add(Duration::from_nanos(remainder.min(u64::MAX as u128) as u64));
    phases
}

pub fn begin_request_phase(phase: &str) -> Option<RequestPhaseGuard> {
    REQUEST_TIMING_SCOPE
        .try_with(|scope| scope.begin_phase(phase))
        .ok()
        .flatten()
}

pub fn set_request_operation(operation: &str) {
    let _ = REQUEST_TIMING_SCOPE.try_with(|scope| scope.set_operation(operation));
}

pub fn set_mcp_request_operation_kind(method_is_tool_call: bool, billable: bool) {
    set_request_operation(if billable {
        "mcp_business"
    } else if method_is_tool_call {
        "mcp_unknown"
    } else {
        "mcp_control"
    });
}

#[cfg(test)]
pub(crate) async fn with_test_request_timing_scope<F>(
    path: &str,
    method: &str,
    operation: &str,
    status: StatusCode,
    future: F,
) -> (F::Output, String)
where
    F: Future,
{
    let metrics = Arc::new(Metrics::new(None).expect("test metrics"));
    let scope = RequestTimingScope::new(metrics.clone(), path, method);
    let result = REQUEST_TIMING_SCOPE
        .scope(scope, async {
            set_request_operation(operation);
            let result = future.await;
            REQUEST_TIMING_SCOPE.with(|scope| scope.finish(status));
            result
        })
        .await;
    let output = String::from_utf8(metrics.gather().expect("gather test metrics"))
        .expect("test metrics are utf8");
    (result, output)
}

#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    recording_enabled: bool,
    http_requests: IntCounterVec,
    http_duration: HistogramVec,
    http_request_duration: HistogramVec,
    upstream_requests: IntCounterVec,
    upstream_duration: HistogramVec,
    sqlite_wait_duration: HistogramVec,
    sqlite_transactions: IntCounterVec,
    sqlite_admission_defers: IntCounterVec,
    background_tasks: IntCounterVec,
    process_cpu_seconds: Counter,
    process_memory_bytes: IntGaugeVec,
    observability_flush: IntCounterVec,
    observability_dropped: IntCounterVec,
    route_allowlist: Option<HashSet<&'static str>>,
    cardinality_slots: Box<[AtomicU64]>,
    event_tx: mpsc::Sender<MetricEvent>,
    event_rx: Mutex<mpsc::Receiver<MetricEvent>>,
    pending_queue_drops: AtomicU64,
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
        let (event_tx, event_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
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
        let http_request_duration = HistogramVec::new(
            HistogramOpts::new(
                "hikari_http_request_duration_seconds",
                "Terminal HTTP or MCP request duration and mutually exclusive leaf phases.",
            )
            .const_labels(const_labels.clone())
            .buckets(HISTOGRAM_BUCKETS.to_vec()),
            HTTP_REQUEST_DURATION_LABELS,
        )?;
        let upstream_requests = IntCounterVec::new(
            Opts::new(
                "hikari_upstream_requests_total",
                "Upstream attempts that received response headers.",
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
        let sqlite_admission_defers = IntCounterVec::new(
            Opts::new(
                "hikari_sqlite_admission_defers_total",
                "SQLite work deferred before transaction admission.",
            )
            .const_labels(const_labels.clone()),
            SQLITE_ADMISSION_LABELS,
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
            Box::new(http_request_duration.clone()),
            Box::new(upstream_requests.clone()),
            Box::new(upstream_duration.clone()),
            Box::new(sqlite_wait_duration.clone()),
            Box::new(sqlite_transactions.clone()),
            Box::new(sqlite_admission_defers.clone()),
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
            http_request_duration,
            upstream_requests,
            upstream_duration,
            sqlite_wait_duration,
            sqlite_transactions,
            sqlite_admission_defers,
            background_tasks,
            process_cpu_seconds,
            process_memory_bytes,
            observability_flush,
            observability_dropped,
            route_allowlist,
            cardinality_slots: (0..MAX_CARDINALITY_TUPLES)
                .map(|_| AtomicU64::new(0))
                .collect(),
            event_tx,
            event_rx: Mutex::new(event_rx),
            pending_queue_drops: AtomicU64::new(0),
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
        if !self.admit_label_tuple(
            "hikari_http_requests_total",
            &[route_family, method, status_class, outcome],
        ) || !self.admit_label_tuple(
            "hikari_http_duration_seconds",
            &[route_family, method, outcome, "none"],
        ) {
            self.record_drop("cardinality");
            return;
        }
        self.enqueue_event(MetricEvent::Http {
            route_family,
            method,
            status_class,
            outcome,
            elapsed_seconds: elapsed.as_secs_f64(),
        });
    }

    fn record_http_phase_internal(
        &self,
        path: &str,
        method: &str,
        outcome: &str,
        phase: &str,
        elapsed: Duration,
        record_request_timing: bool,
    ) {
        if !self.recording_enabled {
            return;
        }
        let route_family = route_family(path);
        if !self.route_allowed(route_family) {
            self.record_drop("route_allowlist");
            return;
        }
        let method = metric_method(path, method);
        let outcome = normalize_outcome(outcome);
        let phase = normalize_phase(phase);
        if record_request_timing && phase != "none" {
            let _ = REQUEST_TIMING_SCOPE.try_with(|scope| {
                scope.record_phase_duration(legacy_request_phase(phase), elapsed)
            });
        }
        if phase == "none"
            || !self.admit_label_tuple(
                "hikari_http_duration_seconds",
                &[route_family, method, outcome, phase],
            )
        {
            if phase != "none" {
                self.record_drop("cardinality");
            }
            return;
        }
        self.enqueue_event(MetricEvent::HttpPhase {
            route_family,
            method,
            outcome,
            phase,
            elapsed_seconds: elapsed.as_secs_f64(),
        });
    }

    pub fn record_http_phase(
        &self,
        path: &str,
        method: &str,
        outcome: &str,
        phase: &str,
        elapsed: Duration,
    ) {
        self.record_http_phase_internal(path, method, outcome, phase, elapsed, true);
    }

    /// Emits the legacy phase histogram without attributing elapsed time to the terminal request.
    pub(crate) fn record_http_phase_legacy(
        &self,
        path: &str,
        method: &str,
        outcome: &str,
        phase: &str,
        elapsed: Duration,
    ) {
        self.record_http_phase_internal(path, method, outcome, phase, elapsed, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn enqueue_request_timing(
        &self,
        route_family: &'static str,
        method: &'static str,
        status_class: &'static str,
        operation: &'static str,
        outcome: &'static str,
        total: Duration,
        phases: [Duration; REQUEST_PHASE_COUNT],
    ) {
        if !self.recording_enabled {
            return;
        }
        if !self.route_allowed(route_family)
            || !self.admit_label_tuple(
                "hikari_http_requests_total",
                &[route_family, method, status_class, outcome],
            )
            || !self.admit_label_tuple(
                "hikari_http_duration_seconds",
                &[route_family, method, outcome, "none"],
            )
            || !self.admit_label_tuple(
                "hikari_http_request_duration_seconds",
                &[route_family, method, operation, outcome, "total"],
            )
            || phases.iter().enumerate().any(|(index, _)| {
                !self.admit_label_tuple(
                    "hikari_http_request_duration_seconds",
                    &[
                        route_family,
                        method,
                        operation,
                        outcome,
                        REQUEST_PHASES[index],
                    ],
                )
            })
        {
            self.record_drop(if self.route_allowed(route_family) {
                "cardinality"
            } else {
                "route_allowlist"
            });
            return;
        }
        self.enqueue_event(MetricEvent::HttpRequestTiming {
            route_family,
            method,
            status_class,
            operation,
            outcome,
            total_seconds: total.as_secs_f64(),
            phase_seconds: phases.map(|phase| phase.as_secs_f64()),
        });
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
            "tavily_http" => "tavily_http",
            "tavily_mcp" => "tavily_mcp",
            "forward_proxy" => "forward_proxy",
            _ => "tavily_http",
        };
        let operation = normalize_operation(operation);
        let Ok(status) = response else {
            self.record_upstream_failure(upstream_class, operation, false, elapsed);
            return;
        };
        let status_class = status_class(status);
        let outcome = outcome_for_status(status);
        if !self.admit_label_tuple(
            "hikari_upstream_requests_total",
            &[upstream_class, operation, outcome, status_class],
        ) || !self.admit_label_tuple(
            "hikari_upstream_duration_seconds",
            &[upstream_class, operation, outcome, "upstream"],
        ) {
            self.record_drop("cardinality");
            return;
        }
        self.enqueue_event(MetricEvent::UpstreamResponse {
            upstream_class,
            operation,
            outcome,
            status_class,
            elapsed_seconds: elapsed.as_secs_f64(),
        });
    }

    pub fn record_upstream_failure(
        &self,
        upstream_class: &str,
        operation: &str,
        timed_out: bool,
        elapsed: Duration,
    ) {
        if !self.recording_enabled {
            return;
        }
        let upstream_class = match upstream_class {
            "tavily_http" => "tavily_http",
            "tavily_mcp" => "tavily_mcp",
            "forward_proxy" => "forward_proxy",
            _ => "tavily_http",
        };
        let operation = normalize_operation(operation);
        let outcome = if timed_out { "timeout" } else { "error" };
        if !self.admit_label_tuple(
            "hikari_upstream_duration_seconds",
            &[upstream_class, operation, outcome, "upstream"],
        ) {
            self.record_drop("cardinality");
            return;
        }
        self.enqueue_event(MetricEvent::UpstreamFailure {
            upstream_class,
            operation,
            outcome,
            elapsed_seconds: elapsed.as_secs_f64(),
        });
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
        let transaction_admitted = outcome != "degraded";
        let admission_metric_admitted = if !transaction_admitted {
            let admitted = self.admit_label_tuple(
                "hikari_sqlite_admission_defers_total",
                &[workload_class, operation, reason],
            );
            if !admitted {
                self.record_drop("cardinality");
            }
            admitted
        } else {
            true
        };
        if transaction_admitted
            && !self.admit_label_tuple(
                "hikari_sqlite_transactions_total",
                &[workload_class, operation, outcome],
            )
        {
            self.record_drop("cardinality");
            return;
        }
        if reason == "none" {
            if !pool_wait.is_zero()
                && !self.admit_label_tuple(
                    "hikari_sqlite_wait_duration_seconds",
                    &[workload_class, operation, "pool"],
                )
            {
                self.record_drop("cardinality");
                return;
            }
            if !begin_wait.is_zero()
                && !self.admit_label_tuple(
                    "hikari_sqlite_wait_duration_seconds",
                    &[workload_class, operation, "begin"],
                )
            {
                self.record_drop("cardinality");
                return;
            }
        } else if !pool_wait.saturating_add(begin_wait).is_zero()
            && !self.admit_label_tuple(
                "hikari_sqlite_wait_duration_seconds",
                &[workload_class, operation, reason],
            )
        {
            self.record_drop("cardinality");
            return;
        }
        self.enqueue_event(MetricEvent::Sqlite {
            workload_class,
            operation,
            outcome,
            reason,
            pool_wait_seconds: pool_wait.as_secs_f64(),
            begin_wait_seconds: begin_wait.as_secs_f64(),
            transaction_admitted,
            admission_metric_admitted,
        });
    }

    pub fn record_background(&self, operation: &str, outcome: &str, reason: &str) {
        if !self.recording_enabled {
            return;
        }
        let labels = [
            normalize_operation(operation),
            normalize_outcome(outcome),
            normalize_reason(reason),
        ];
        if self.admit_label_tuple("hikari_background_tasks_total", &labels) {
            self.enqueue_event(MetricEvent::Background {
                operation: labels[0],
                outcome: labels[1],
                reason: labels[2],
            });
        } else {
            self.record_drop("cardinality");
        }
    }

    pub fn record_drop(&self, reason: &str) {
        if !self.recording_enabled {
            return;
        }
        self.enqueue_event(MetricEvent::Drop {
            reason: normalize_reason(reason),
        });
    }

    fn enqueue_event(&self, event: MetricEvent) {
        if self.event_tx.try_send(event).is_err() {
            self.pending_queue_drops
                .fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    fn flush_pending(&self) {
        let mut events = Vec::new();
        {
            let mut receiver = self
                .event_rx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.apply_event(event);
        }
        let queue_drops = self.pending_queue_drops.swap(0, AtomicOrdering::AcqRel);
        if queue_drops > 0 {
            self.observability_dropped
                .with_label_values(&["queue"])
                .inc_by(queue_drops);
        }
    }

    fn apply_event(&self, event: MetricEvent) {
        match event {
            MetricEvent::Http {
                route_family,
                method,
                status_class,
                outcome,
                elapsed_seconds,
            } => {
                self.http_requests
                    .with_label_values(&[route_family, method, status_class, outcome])
                    .inc();
                self.http_duration
                    .with_label_values(&[route_family, method, outcome, "none"])
                    .observe(elapsed_seconds);
            }
            MetricEvent::HttpPhase {
                route_family,
                method,
                outcome,
                phase,
                elapsed_seconds,
            } => {
                self.http_duration
                    .with_label_values(&[route_family, method, outcome, phase])
                    .observe(elapsed_seconds);
            }
            MetricEvent::HttpRequestTiming {
                route_family,
                method,
                status_class,
                operation,
                outcome,
                total_seconds,
                phase_seconds,
            } => {
                self.http_requests
                    .with_label_values(&[route_family, method, status_class, outcome])
                    .inc();
                self.http_duration
                    .with_label_values(&[route_family, method, outcome, "none"])
                    .observe(total_seconds);
                self.http_request_duration
                    .with_label_values(&[route_family, method, operation, outcome, "total"])
                    .observe(total_seconds);
                for (phase, elapsed_seconds) in REQUEST_PHASES.iter().zip(phase_seconds) {
                    self.http_request_duration
                        .with_label_values(&[route_family, method, operation, outcome, phase])
                        .observe(elapsed_seconds);
                }
            }
            MetricEvent::UpstreamResponse {
                upstream_class,
                operation,
                outcome,
                status_class,
                elapsed_seconds,
            } => {
                self.upstream_requests
                    .with_label_values(&[upstream_class, operation, outcome, status_class])
                    .inc();
                self.upstream_duration
                    .with_label_values(&[upstream_class, operation, outcome, "upstream"])
                    .observe(elapsed_seconds);
            }
            MetricEvent::UpstreamFailure {
                upstream_class,
                operation,
                outcome,
                elapsed_seconds,
            } => {
                self.upstream_duration
                    .with_label_values(&[upstream_class, operation, outcome, "upstream"])
                    .observe(elapsed_seconds);
            }
            MetricEvent::Sqlite {
                workload_class,
                operation,
                outcome,
                reason,
                pool_wait_seconds,
                begin_wait_seconds,
                transaction_admitted,
                admission_metric_admitted,
            } => {
                if transaction_admitted {
                    self.sqlite_transactions
                        .with_label_values(&[workload_class, operation, outcome])
                        .inc();
                } else if admission_metric_admitted {
                    self.sqlite_admission_defers
                        .with_label_values(&[workload_class, operation, reason])
                        .inc();
                }
                if reason == "none" {
                    if pool_wait_seconds > 0.0 {
                        self.sqlite_wait_duration
                            .with_label_values(&[workload_class, operation, "pool"])
                            .observe(pool_wait_seconds);
                    }
                    if begin_wait_seconds > 0.0 {
                        self.sqlite_wait_duration
                            .with_label_values(&[workload_class, operation, "begin"])
                            .observe(begin_wait_seconds);
                    }
                } else {
                    let wait_seconds = pool_wait_seconds + begin_wait_seconds;
                    if wait_seconds > 0.0 {
                        self.sqlite_wait_duration
                            .with_label_values(&[workload_class, operation, reason])
                            .observe(wait_seconds);
                    }
                }
            }
            MetricEvent::Background {
                operation,
                outcome,
                reason,
            } => {
                self.background_tasks
                    .with_label_values(&[operation, outcome, reason])
                    .inc();
            }
            MetricEvent::Drop { reason } => {
                self.observability_dropped
                    .with_label_values(&[reason])
                    .inc();
            }
        }
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

    fn admit_label_tuple(&self, metric: &str, labels: &[&str]) -> bool {
        // All callers normalize labels to fixed enums before reaching this gate. A
        // fixed open-addressing table admits known tuples without allocation or a
        // process-wide mutex; a saturated probe window conservatively drops new ones.
        let fingerprint = label_tuple_fingerprint(metric, labels);
        let start = (fingerprint as usize) % MAX_CARDINALITY_TUPLES;
        for offset in 0..CARDINALITY_PROBES {
            let slot = (start + offset) % MAX_CARDINALITY_TUPLES;
            let entry = &self.cardinality_slots[slot];
            let observed = entry.load(AtomicOrdering::Acquire);
            if observed == fingerprint {
                return true;
            }
            if observed == 0 {
                match entry.compare_exchange(
                    0,
                    fingerprint,
                    AtomicOrdering::AcqRel,
                    AtomicOrdering::Acquire,
                ) {
                    Ok(_) => return true,
                    Err(raced) if raced == fingerprint => return true,
                    Err(_) => continue,
                }
            }
        }
        false
    }

    fn gather(&self) -> Result<Vec<u8>, prometheus::Error> {
        self.flush_pending();
        let families = self.registry.gather();
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&families, &mut output)
            .map(|_| output)
    }

    #[doc(hidden)]
    pub fn gather_test_metrics(&self) -> String {
        String::from_utf8(self.gather().expect("gather test metrics")).expect("metrics are utf8")
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
        let mut server = self.server;
        if tokio::time::timeout(EXPORTER_SHUTDOWN_TIMEOUT, &mut server)
            .await
            .is_err()
        {
            server.abort();
            let _ = server.await;
            tracing::warn!(
                component = "observability",
                event = "exporter_shutdown_timeout",
                task = "server",
                "metrics exporter server task did not stop before the shutdown deadline"
            );
        }
        let mut sampler = self.sampler;
        if tokio::time::timeout(EXPORTER_SHUTDOWN_TIMEOUT, &mut sampler)
            .await
            .is_err()
        {
            sampler.abort();
            let _ = sampler.await;
            tracing::warn!(
                component = "observability",
                event = "exporter_shutdown_timeout",
                task = "sampler",
                "metrics runtime sampler did not stop before the shutdown deadline"
            );
        }
    }
}

static GLOBAL_METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();

pub fn global_metrics() -> Arc<Metrics> {
    GLOBAL_METRICS
        .get_or_init(|| {
            Arc::new(
                Metrics::new_with_recording(
                    route_allowlist_from_env(),
                    env_flag("OBSERVABILITY_ENABLED") && observability_identity_is_valid(),
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
    if !observability_identity_is_valid() {
        tracing::warn!(
            component = "observability",
            event = "exporter_disabled",
            reason = "invalid_deployment_identity",
            "metrics exporter disabled because deployment identity is missing or invalid"
        );
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
    if !metrics_bind_allowed(bind, env_flag("METRICS_ALLOW_UNSPECIFIED_BIND")) {
        tracing::warn!(
            component = "observability",
            event = "exporter_disabled",
            reason = "non_private_bind",
            "metrics exporter disabled because METRICS_BIND is not a loopback or private address"
        );
        return None;
    }
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

async fn http_metrics_middleware_core(
    metrics: Arc<Metrics>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !metrics.recording_enabled {
        return next.run(req).await;
    }
    let path = req.uri().path().to_owned();
    let method = req.method().as_str().to_owned();
    let scope = RequestTimingScope::new(metrics, &path, &method);
    if path == "/health" {
        let response = next.run(req).await;
        scope.finish(response.status());
        return response;
    }
    REQUEST_TIMING_SCOPE
        .scope(scope, async move {
            let response = next.run(req).await;
            REQUEST_TIMING_SCOPE.with(|scope| scope.finish(response.status()));
            response
        })
        .await
}

pub async fn http_metrics_middleware(req: Request<Body>, next: Next) -> Response {
    http_metrics_middleware_core(global_metrics(), req, next).await
}

#[doc(hidden)]
pub async fn http_metrics_middleware_with_metrics(
    Extension(metrics): Extension<Arc<Metrics>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    http_metrics_middleware_core(metrics, req, next).await
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
    let authorized =
        provided.is_some_and(|provided| constant_time_token_eq(provided, endpoint.token.as_ref()));
    if !authorized {
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

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    let mut difference = provided.len() ^ expected.len();
    for index in 0..provided.len().max(expected.len()) {
        difference |= usize::from(
            provided.get(index).copied().unwrap_or_default()
                ^ expected.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
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

fn metrics_bind_allowed(bind: SocketAddr, allow_unspecified: bool) -> bool {
    if bind.ip().is_unspecified() {
        return allow_unspecified;
    }
    match bind.ip() {
        IpAddr::V4(address) => address.is_loopback() || address.is_private(),
        IpAddr::V6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

fn label_tuple_fingerprint(metric: &str, labels: &[&str]) -> u64 {
    let mut hash = 14_695_981_039_346_656_037u64;
    for byte in metric.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    for label in labels {
        hash ^= 0xff;
        hash = hash.wrapping_mul(1_099_511_628_211);
        for byte in label.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
    }
    if hash == 0 { 1 } else { hash }
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
            _ => "OTHER",
        }
    }
}

fn default_request_operation(route_family: &str) -> &'static str {
    match route_family {
        "mcp" => "mcp_unknown",
        "http_search" => "search",
        "http_extract" => "extract",
        "http_crawl" => "crawl",
        "http_map" => "map",
        "http_research" => "research",
        "admin" => "admin",
        "user" => "user",
        "public" => "public",
        _ => "internal",
    }
}

fn normalize_request_operation(operation: &str) -> &'static str {
    match operation {
        "search" | "http_search" => "search",
        "extract" | "http_extract" => "extract",
        "crawl" | "http_crawl" => "crawl",
        "map" | "http_map" => "map",
        "research" | "http_research" => "research",
        "admin" => "admin",
        "user" => "user",
        "public" => "public",
        "internal" => "internal",
        "mcp_control" => "mcp_control",
        "mcp_business" => "mcp_business",
        "mcp_unknown" => "mcp_unknown",
        _ => "other",
    }
}

fn request_phase_index(phase: &str) -> Option<usize> {
    Some(match phase {
        "configuration" => 0,
        "billing_lock_wait" => 1,
        "preflight" => 2,
        "key_selection" => 3,
        "upstream" => 4,
        "response_body" => 5,
        "persistence" => 6,
        "settlement" => 7,
        "reconciliation_usage" => 8,
        "reconciliation_schedule" => 9,
        "other" => 10,
        _ => return None,
    })
}

fn legacy_request_phase(phase: &str) -> &'static str {
    match phase {
        "admission" => "preflight",
        "key_selection" => "key_selection",
        "upstream" | "response_headers" => "upstream",
        "response_body" => "response_body",
        "persistence" => "persistence",
        "ack" => "settlement",
        _ => "other",
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

pub(crate) fn outcome_for_status_code(status: StatusCode) -> &'static str {
    outcome_for_status(status)
}

pub(crate) fn outcome_for_proxy_error(error: &crate::ProxyError) -> &'static str {
    if matches!(error, crate::ProxyError::Http(error) if error.is_timeout()) {
        "timeout"
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
        "research" | "research_result" | "http_research" => "research",
        "observability_sampler" => "observability_sampler",
        "background_tasks" => "background_tasks",
        "foreground_job_trigger" => "foreground_job_trigger",
        _ if operation.contains("alert") => "alerts",
        _ if operation.contains("reconcil") => "reconciliation",
        _ if operation.contains("request") => "request",
        _ => "other",
    }
}

pub(crate) fn upstream_class_for_operation(operation: &str) -> &'static str {
    if operation == "mcp" {
        "tavily_mcp"
    } else {
        "tavily_http"
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
        "native_query_deadline" => "native_query_deadline",
        "foreground_pressure" => "foreground_pressure",
        "pool_pressure" => "pool_pressure",
        "recent_contention" => "recent_contention",
        "writer_contention" => "writer_contention",
        "coverage_readiness" => "coverage_readiness",
        "source_baseline" => "source_baseline",
        _ => "other",
    }
}

fn normalize_phase(phase: &str) -> &'static str {
    match phase {
        "none" => "none",
        "admission" => "admission",
        "key_selection" => "key_selection",
        "upstream" => "upstream",
        "response_headers" => "response_headers",
        "response_body" => "response_body",
        "persistence" => "persistence",
        "ack" => "ack",
        _ => "none",
    }
}

fn common_const_labels() -> HashMap<String, String> {
    let mut labels = HashMap::new();
    labels.insert("service".to_owned(), "tavily-hikari".to_owned());
    labels.insert(
        "environment".to_owned(),
        bounded_deployment_identity("OBSERVABILITY_ENVIRONMENT"),
    );
    labels.insert(
        "instance".to_owned(),
        bounded_deployment_identity("OBSERVABILITY_INSTANCE"),
    );
    labels
}

fn observability_identity_is_valid() -> bool {
    let Ok(environment) = env::var("OBSERVABILITY_ENVIRONMENT") else {
        return false;
    };
    let Ok(instance) = env::var("OBSERVABILITY_INSTANCE") else {
        return false;
    };
    valid_environment_value(environment.trim()) && valid_deployment_value(instance.trim())
}

fn bounded_deployment_identity(name: &str) -> String {
    let value = env::var(name).unwrap_or_default();
    match name {
        "OBSERVABILITY_ENVIRONMENT" => bounded_environment_value(value.trim()),
        "OBSERVABILITY_INSTANCE" => bounded_deployment_value(value.trim()),
        _ => "unknown".to_owned(),
    }
}

fn bounded_deployment_value(value: &str) -> String {
    if valid_deployment_value(value) {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn bounded_environment_value(value: &str) -> String {
    if valid_environment_value(value) {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn valid_deployment_value(value: &str) -> bool {
    let safe_slot = matches!(
        value,
        "primary" | "secondary" | "standby" | "local" | "test"
    );
    let safe_ordinal = value.strip_prefix("hikari-").is_some_and(|suffix| {
        (1..=3).contains(&suffix.len()) && suffix.bytes().all(|byte| byte.is_ascii_digit())
    });
    safe_slot || safe_ordinal
}

fn valid_environment_value(value: &str) -> bool {
    matches!(value, "development" | "staging" | "production" | "test")
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
    use tower::ServiceExt;

    fn request_timing_histogram_value(output: &str, metric: &str, phase: &str) -> f64 {
        output
            .lines()
            .find(|line| line.starts_with(metric) && line.contains(&format!("phase=\"{phase}\"")))
            .and_then(|line| line.rsplit_once(' ').map(|(_, value)| value))
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("missing {metric} for phase {phase}: {output}"))
    }

    fn assert_terminal_request_timing(output: &str) -> ([f64; REQUEST_PHASE_COUNT], f64) {
        let mut phase_sums = [0.0; REQUEST_PHASE_COUNT];
        for (index, phase) in REQUEST_PHASES.iter().enumerate() {
            let count = request_timing_histogram_value(
                output,
                "hikari_http_request_duration_seconds_count{",
                phase,
            );
            assert_eq!(count, 1.0, "expected one terminal {phase} sample: {output}");
            phase_sums[index] = request_timing_histogram_value(
                output,
                "hikari_http_request_duration_seconds_sum{",
                phase,
            );
        }
        let total_count = request_timing_histogram_value(
            output,
            "hikari_http_request_duration_seconds_count{",
            "total",
        );
        assert_eq!(
            total_count, 1.0,
            "expected one terminal total sample: {output}"
        );
        let total = request_timing_histogram_value(
            output,
            "hikari_http_request_duration_seconds_sum{",
            "total",
        );
        let leaf_sum: f64 = phase_sums.iter().sum();
        assert!(
            (leaf_sum - total).abs() <= 1e-9,
            "terminal total must equal the leaf phases: {output}"
        );
        (phase_sums, total)
    }

    #[test]
    fn route_and_method_values_are_bounded() {
        assert_eq!(route_family("/api/tavily/search"), "http_search");
        assert_eq!(route_family("/mcp"), "mcp");
        assert_eq!(metric_method("/mcp", "POST"), "MCP");
        assert_eq!(metric_method("/anything", "OPTIONS"), "OTHER");
        assert_eq!(metric_method("/anything", "HEAD"), "OTHER");
    }

    #[test]
    fn instance_identity_rejects_sensitive_tokens() {
        assert_eq!(bounded_environment_value("production"), "production");
        assert_eq!(bounded_deployment_value("primary"), "primary");
        assert_eq!(bounded_deployment_value("hikari-01"), "hikari-01");
        assert_eq!(bounded_deployment_value("hikari-prod-01"), "unknown");
        assert_eq!(bounded_deployment_value("token-123"), "unknown");
        assert_eq!(bounded_deployment_value("tvly-dev-abc123"), "unknown");
        assert_eq!(bounded_environment_value("customer-prod"), "unknown");
    }

    #[test]
    fn recorder_uses_a_fixed_lock_free_cardinality_table() {
        let metrics = Metrics::new(None).expect("metrics");
        for index in 0..MAX_CARDINALITY_TUPLES {
            let value = index.to_string();
            let _ = metrics.admit_label_tuple("test", &[value.as_str()]);
        }
        assert_eq!(metrics.cardinality_slots.len(), MAX_CARDINALITY_TUPLES);
    }

    #[test]
    fn recorder_rejects_a_saturated_probe_window() {
        let metrics = Metrics::new(None).expect("metrics");
        let labels = ["overflow"];
        let fingerprint = label_tuple_fingerprint("test", &labels);
        let start = (fingerprint as usize) % MAX_CARDINALITY_TUPLES;
        for offset in 0..CARDINALITY_PROBES {
            metrics.cardinality_slots[(start + offset) % MAX_CARDINALITY_TUPLES].store(
                fingerprint.wrapping_add(offset as u64 + 1),
                AtomicOrdering::Release,
            );
        }
        assert!(!metrics.admit_label_tuple("test", &labels));
        metrics.record_drop("cardinality");
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("reason=\"cardinality\""));
    }

    #[test]
    fn recorder_queue_is_bounded_and_reports_overflow() {
        let metrics = Metrics::new(None).expect("metrics");
        for _ in 0..(EVENT_QUEUE_CAPACITY + 16) {
            metrics.record_background("background_tasks", "success", "started");
        }
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("hikari_observability_dropped_total"));
        assert!(output.contains("reason=\"queue\""));
    }

    #[tokio::test]
    async fn request_timing_scope_emits_one_terminal_event_and_all_leaf_phases() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/api/tavily/search", "POST");
        REQUEST_TIMING_SCOPE
            .scope(scope, async {
                set_request_operation("search");
                let outer = begin_request_phase("preflight").expect("preflight phase");
                assert!(REQUEST_TIMING_SCOPE.with(|scope| scope.phase_state.get().is_some()));
                let upstream = begin_request_phase("upstream").expect("upstream phase");
                drop(upstream);
                let first_repeated_preflight =
                    begin_request_phase("preflight").expect("first repeated preflight phase");
                tokio::time::sleep(Duration::from_millis(5)).await;
                drop(first_repeated_preflight);
                let second_repeated_preflight =
                    begin_request_phase("preflight").expect("second repeated preflight phase");
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(second_repeated_preflight);
                let billing_lock =
                    begin_request_phase("billing_lock_wait").expect("billing lock phase");
                tokio::time::sleep(Duration::from_millis(1)).await;
                drop(billing_lock);
                let settlement = begin_request_phase("settlement").expect("settlement phase");
                tokio::time::sleep(Duration::from_millis(1)).await;
                drop(settlement);
                drop(outer);
                REQUEST_TIMING_SCOPE.with(|scope| scope.finish(StatusCode::OK));
            })
            .await;

        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert_eq!(
            output.matches("hikari_http_requests_total{").count(),
            1,
            "terminal request must be counted once"
        );
        let (phase_sums, _total_sum) = assert_terminal_request_timing(&output);
        let preflight_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "preflight")
            .expect("preflight phase index");
        assert!(
            phase_sums[preflight_index] >= Duration::from_millis(25).as_secs_f64(),
            "repeated preflight segments must accumulate: {output}"
        );
        for phase in ["billing_lock_wait", "settlement"] {
            let index = REQUEST_PHASES
                .iter()
                .position(|candidate| *candidate == phase)
                .expect("phase index");
            assert!(phase_sums[index] > 0.0, "missing elapsed time for {phase}");
        }
        assert!(output.contains("operation=\"search\""));
        assert!(!output.contains("request_id"));
    }

    #[tokio::test]
    async fn request_timing_scope_health_is_terminal_only() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/health", "GET");
        assert!(scope.phase_state.get().is_none());
        REQUEST_TIMING_SCOPE
            .scope(scope, async {
                assert!(REQUEST_TIMING_SCOPE.with(|scope| scope.phase_state.get().is_none()));
                REQUEST_TIMING_SCOPE.with(|scope| scope.finish(StatusCode::NO_CONTENT));
            })
            .await;

        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert_eq!(
            output.matches("hikari_http_requests_total{").count(),
            1,
            "health request must be counted once"
        );
        assert_eq!(
            output
                .matches("hikari_http_duration_seconds_count{")
                .count(),
            1,
            "health request must retain one legacy total sample"
        );
        assert!(output.contains("route_family=\"public\""));
        assert!(output.contains("method=\"GET\""));
        assert!(output.contains("operation=\"public\""));
        let (phase_sums, total) = assert_terminal_request_timing(&output);
        let other_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "other")
            .expect("other phase index");
        for (index, phase_sum) in phase_sums.iter().enumerate() {
            if index == other_index {
                assert!(*phase_sum > 0.0, "health elapsed time must be retained");
            } else {
                assert_eq!(*phase_sum, 0.0, "health must not emit phase time");
            }
        }
        assert!((phase_sums[other_index] - total).abs() <= 1e-9);
    }

    #[tokio::test]
    async fn http_metrics_middleware_health_fast_path_preserves_response_and_metrics() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let app = Router::new()
            .route(
                "/health",
                get(|| async {
                    assert!(begin_request_phase("preflight").is_none());
                    (StatusCode::OK, "healthy")
                }),
            )
            .layer(axum::middleware::from_fn(
                http_metrics_middleware_with_metrics,
            ))
            .layer(Extension(metrics.clone()));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("health response body");
        assert_eq!(body.as_ref(), b"healthy");

        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert_eq!(
            output.matches("hikari_http_requests_total{").count(),
            1,
            "health request must be counted once"
        );
        let (phase_sums, total) = assert_terminal_request_timing(&output);
        let other_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "other")
            .expect("other phase index");
        assert!((phase_sums[other_index] - total).abs() <= 1e-9);
        assert!(output.contains("route_family=\"public\""));
        assert!(output.contains("method=\"GET\""));
        assert!(output.contains("operation=\"public\""));
    }

    #[tokio::test]
    async fn http_metrics_middleware_disabled_recording_bypasses_timing_scope() {
        let metrics = Arc::new(Metrics::new_with_recording(None, false).expect("metrics"));
        let app = Router::new()
            .route(
                "/disabled",
                get(|| async {
                    assert!(begin_request_phase("preflight").is_none());
                    (StatusCode::OK, "disabled")
                }),
            )
            .layer(axum::middleware::from_fn(
                http_metrics_middleware_with_metrics,
            ))
            .layer(Extension(metrics.clone()));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/disabled")
                    .body(Body::empty())
                    .expect("disabled request"),
            )
            .await
            .expect("disabled response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("disabled response body");
        assert_eq!(body.as_ref(), b"disabled");
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(!output.contains("hikari_http_requests_total{"));
        assert!(!output.contains("hikari_http_request_duration_seconds_count{"));
    }

    #[tokio::test]
    async fn legacy_http_phase_recording_does_not_overlap_real_phase_guards() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/mcp", "POST");
        let mut key_selection_elapsed = Duration::ZERO;
        let mut upstream_elapsed = Duration::ZERO;
        REQUEST_TIMING_SCOPE
            .scope(scope, async {
                let key_selection = begin_request_phase("key_selection").expect("key selection");
                let key_selection_started = Instant::now();
                tokio::time::sleep(Duration::from_millis(80)).await;
                key_selection_elapsed = key_selection_started.elapsed();
                drop(key_selection);
                metrics.record_http_phase_legacy(
                    "/mcp",
                    "POST",
                    "success",
                    "key_selection",
                    key_selection_elapsed,
                );

                let upstream = begin_request_phase("upstream").expect("upstream phase");
                let upstream_started = Instant::now();
                tokio::time::sleep(Duration::from_millis(80)).await;
                upstream_elapsed = upstream_started.elapsed();
                drop(upstream);
                metrics.record_http_phase_legacy(
                    "/mcp",
                    "POST",
                    "success",
                    "upstream",
                    upstream_elapsed,
                );
                REQUEST_TIMING_SCOPE.with(|scope| scope.finish(StatusCode::OK));
            })
            .await;

        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        let (phase_sums, _total) = assert_terminal_request_timing(&output);
        for (phase, expected) in [
            ("key_selection", key_selection_elapsed),
            ("upstream", upstream_elapsed),
        ] {
            let index = REQUEST_PHASES
                .iter()
                .position(|candidate| *candidate == phase)
                .expect("phase index");
            let actual = phase_sums[index];
            let expected = expected.as_secs_f64();
            assert!(
                actual >= expected * 0.7 && actual <= expected * 1.35,
                "{phase} must retain its own elapsed segment: actual={actual:.3}s expected={expected:.3}s output={output}"
            );
        }
        assert!(output.contains("hikari_http_duration_seconds_sum{"));
        assert!(output.contains("phase=\"key_selection\""));
        assert!(output.contains("phase=\"upstream\""));
    }

    #[tokio::test]
    async fn request_timing_scope_handles_completed_nested_phase_guards_without_overlap() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/mcp", "POST");
        let mut upstream_before_nested = Duration::ZERO;
        let mut configuration_elapsed = Duration::ZERO;
        let mut upstream_after_nested = Duration::ZERO;
        REQUEST_TIMING_SCOPE
            .scope(scope, async {
                let upstream = begin_request_phase("upstream").expect("upstream phase");
                let upstream_before_started = Instant::now();
                tokio::time::sleep(Duration::from_millis(60)).await;
                upstream_before_nested = upstream_before_started.elapsed();

                let configuration =
                    begin_request_phase("configuration").expect("configuration phase");
                let configuration_started = Instant::now();
                tokio::time::sleep(Duration::from_millis(60)).await;
                configuration_elapsed = configuration_started.elapsed();
                drop(configuration);

                let upstream_after_started = Instant::now();
                tokio::time::sleep(Duration::from_millis(60)).await;
                upstream_after_nested = upstream_after_started.elapsed();
                drop(upstream);
                REQUEST_TIMING_SCOPE.with(|scope| scope.finish(StatusCode::OK));
            })
            .await;

        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        let (phase_sums, _total) = assert_terminal_request_timing(&output);
        let upstream_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "upstream")
            .expect("upstream phase index");
        let configuration_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "configuration")
            .expect("configuration phase index");
        let upstream_expected = upstream_before_nested + upstream_after_nested;
        for (phase, actual, expected) in [
            (
                "upstream",
                phase_sums[upstream_index],
                upstream_expected.as_secs_f64(),
            ),
            (
                "configuration",
                phase_sums[configuration_index],
                configuration_elapsed.as_secs_f64(),
            ),
        ] {
            assert!(
                actual >= expected * 0.7 && actual <= expected * 1.35,
                "{phase} must retain only its own nested segments: actual={actual:.3}s expected={expected:.3}s output={output}"
            );
        }
    }

    #[tokio::test]
    async fn mcp_request_operation_is_fixed_for_control_business_and_unknown_calls() {
        for (method_is_tool_call, billable, expected) in [
            (false, false, "mcp_control"),
            (true, true, "mcp_business"),
            (true, false, "mcp_unknown"),
        ] {
            let metrics = Arc::new(Metrics::new(None).expect("metrics"));
            let scope = RequestTimingScope::new(metrics.clone(), "/mcp", "POST");
            REQUEST_TIMING_SCOPE
                .scope(scope, async {
                    set_mcp_request_operation_kind(method_is_tool_call, billable);
                    REQUEST_TIMING_SCOPE.with(|scope| scope.finish(StatusCode::OK));
                })
                .await;
            let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
            assert!(output.contains(&format!("operation=\"{expected}\"")));
            assert!(!output.contains("tool-name"));
        }
    }

    #[test]
    fn request_timing_scope_cancellation_is_terminal_once() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/mcp", "POST");
        drop(scope);
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("outcome=\"cancelled\""));
        assert_terminal_request_timing(&output);
    }

    #[tokio::test]
    async fn request_timing_scope_active_future_abort_is_terminal_once() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let entered = Arc::new(Notify::new());
        let task = tokio::spawn({
            let metrics = metrics.clone();
            let entered = entered.clone();
            async move {
                let scope = RequestTimingScope::new(metrics, "/mcp", "POST");
                REQUEST_TIMING_SCOPE
                    .scope(scope, async move {
                        let _phase = begin_request_phase("upstream").expect("upstream phase");
                        entered.notify_one();
                        std::future::pending::<()>().await;
                    })
                    .await;
            }
        });
        entered.notified().await;
        task.abort();
        assert!(task.await.expect_err("aborted task").is_cancelled());

        let output = metrics.gather_test_metrics();
        assert!(output.contains("outcome=\"cancelled\""));
        let (phase_sums, _total) = assert_terminal_request_timing(&output);
        let upstream_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "upstream")
            .expect("upstream phase index");
        assert!(phase_sums[upstream_index] > 0.0);
    }

    #[tokio::test]
    async fn request_timing_scope_health_abort_is_terminal_once() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        let entered = Arc::new(Notify::new());
        let task = tokio::spawn({
            let metrics = metrics.clone();
            let entered = entered.clone();
            async move {
                let scope = RequestTimingScope::new(metrics, "/health", "GET");
                REQUEST_TIMING_SCOPE
                    .scope(scope, async move {
                        entered.notify_one();
                        std::future::pending::<()>().await;
                    })
                    .await;
            }
        });
        entered.notified().await;
        task.abort();
        assert!(task.await.expect_err("aborted task").is_cancelled());

        let output = metrics.gather_test_metrics();
        assert_eq!(
            output.matches("hikari_http_requests_total{").count(),
            1,
            "aborted health request must emit one terminal event"
        );
        assert!(output.contains("status_class=\"none\""));
        assert!(output.contains("outcome=\"cancelled\""));
        let (phase_sums, total) = assert_terminal_request_timing(&output);
        let other_index = REQUEST_PHASES
            .iter()
            .position(|phase| *phase == "other")
            .expect("other phase index");
        assert!((phase_sums[other_index] - total).abs() <= 1e-9);
    }

    #[test]
    fn request_timing_scope_disabled_recording_is_empty() {
        let metrics = Arc::new(Metrics::new_with_recording(None, false).expect("metrics"));
        let scope = RequestTimingScope::new(metrics.clone(), "/api/tavily/search", "POST");
        drop(scope);
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(!output.contains("hikari_http_requests_total{"));
        assert!(!output.contains("hikari_http_request_duration_seconds_count{"));
    }

    #[test]
    fn request_timing_queue_overflow_drops_the_whole_terminal_event() {
        let metrics = Arc::new(Metrics::new(None).expect("metrics"));
        for _ in 0..EVENT_QUEUE_CAPACITY {
            metrics.record_background("background_tasks", "success", "started");
        }
        let scope = RequestTimingScope::new(metrics.clone(), "/health", "GET");
        assert!(scope.phase_state.get().is_none());
        drop(scope);
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(!output.contains("hikari_http_request_duration_seconds_count{"));
        assert!(output.contains("hikari_observability_dropped_total"));
        assert!(output.contains("reason=\"queue\""));
    }

    #[test]
    fn metrics_bind_scope_defaults_to_private_addresses() {
        assert!(metrics_bind_allowed(
            "127.0.0.1:9091".parse().expect("loopback"),
            false
        ));
        assert!(metrics_bind_allowed(
            "10.0.0.10:9091".parse().expect("private"),
            false
        ));
        assert!(!metrics_bind_allowed(
            "0.0.0.0:9091".parse().expect("wildcard"),
            false
        ));
        assert!(metrics_bind_allowed(
            "0.0.0.0:9091".parse().expect("wildcard"),
            true
        ));
        assert!(!metrics_bind_allowed(
            "8.8.8.8:9091".parse().expect("public"),
            true
        ));
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
    fn phase_and_timeout_outcomes_are_exported() {
        let metrics = Metrics::new(None).expect("metrics");
        metrics.record_http_phase(
            "/api/tavily/search",
            "POST",
            "success",
            "upstream",
            Duration::from_millis(10),
        );
        metrics.record_upstream_failure("tavily_http", "search", true, Duration::from_millis(20));
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("phase=\"upstream\""));
        assert!(output.contains("outcome=\"timeout\""));
        assert!(!output.contains("hikari_upstream_requests_total{"));
    }

    #[test]
    fn invalid_deployment_identity_disables_runtime_observability() {
        assert!(valid_environment_value("production"));
        assert!(valid_deployment_value("hikari-01"));
        assert!(!valid_environment_value("customer-prod"));
        assert!(!valid_deployment_value("token-123"));
    }

    #[test]
    fn deferred_sqlite_admission_is_not_a_transaction() {
        let metrics = Metrics::new(None).expect("metrics");
        metrics.record_sqlite(
            "foreground",
            "request_log",
            Duration::ZERO,
            Duration::ZERO,
            "degraded",
            "pool",
        );
        let output = String::from_utf8(metrics.gather().expect("gather")).expect("utf8");
        assert!(output.contains("hikari_sqlite_admission_defers_total"));
        assert!(output.contains("reason=\"pool\""));
        assert!(!output.contains("hikari_sqlite_transactions_total"));
    }

    #[test]
    fn unknown_dimensions_are_mapped_to_fixed_values() {
        assert_eq!(normalize_operation("user supplied operation"), "other");
        assert_eq!(normalize_operation("research_result"), "research");
        assert_eq!(normalize_request_operation("mcp_control"), "mcp_control");
        assert_eq!(normalize_request_operation("arbitrary-tool-name"), "other");
        assert_eq!(normalize_reason("user supplied reason"), "other");
        assert_eq!(
            normalize_reason("native_query_deadline"),
            "native_query_deadline"
        );
        assert_eq!(normalize_reason("writer_contention"), "writer_contention");
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

        let mut wrong_headers = HeaderMap::new();
        wrong_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-token"),
        );
        let wrong = metrics_handler(axum::extract::State(endpoint.clone()), wrong_headers).await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

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
