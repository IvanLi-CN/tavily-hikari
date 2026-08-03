use std::{
    fmt as stdfmt, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};

use clap::ValueEnum;
use tracing::Dispatch;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

const DEFAULT_RUNTIME_LOG_FILTER: &str = "warn,tavily_hikari=info,sqlx::query=warn";

static RUNTIME_LOGGING_INIT: OnceLock<()> = OnceLock::new();
static LOG_TRACER_INIT: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum RuntimeLogFormat {
    #[default]
    Json,
    Text,
}

impl RuntimeLogFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
        }
    }
}

impl stdfmt::Display for RuntimeLogFormat {
    fn fmt(&self, f: &mut stdfmt::Formatter<'_>) -> stdfmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LegacyStdIoLevel {
    Info,
    Warn,
}

pub fn init_runtime_logging(format: RuntimeLogFormat) {
    RUNTIME_LOGGING_INIT.get_or_init(|| {
        let dispatch = build_runtime_log_dispatch(format, runtime_log_env_filter(), io::stderr);
        if tracing::dispatcher::set_global_default(dispatch).is_ok() {
            install_log_tracer();
        }
    });
}

pub fn emit_legacy_stdio_event(
    level: LegacyStdIoLevel,
    module_path: &'static str,
    file: &'static str,
    line: u32,
    args: stdfmt::Arguments<'_>,
) {
    let component = component_for_module_path(module_path);
    match level {
        LegacyStdIoLevel::Info => {
            tracing::info!(
                component,
                event = "legacy_stdio",
                stream = "stdout",
                module_path,
                file,
                line,
                message = %args,
            );
        }
        LegacyStdIoLevel::Warn => {
            tracing::warn!(
                component,
                event = "legacy_stdio",
                stream = "stderr",
                module_path,
                file,
                line,
                message = %args,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMemorySnapshot {
    pub memory_current_bytes: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub cgroup_anon_bytes: Option<u64>,
    pub cgroup_file_bytes: Option<u64>,
    pub cgroup_swap_bytes: Option<u64>,
    pub headroom_bytes: Option<u64>,
    pub process_rss_bytes: Option<u64>,
    pub process_rss_anon_bytes: Option<u64>,
    pub process_rss_file_bytes: Option<u64>,
    pub process_hwm_bytes: Option<u64>,
    pub process_swap_bytes: Option<u64>,
    pub child_process_rss_bytes: Option<u64>,
    pub process_group_rss_bytes: Option<u64>,
}

#[derive(Debug)]
pub struct RuntimePerfScope {
    started_at: Instant,
    memory: OnceLock<RuntimeMemorySnapshot>,
}

impl RuntimePerfScope {
    pub fn start() -> Self {
        Self {
            started_at: Instant::now(),
            memory: OnceLock::new(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    pub fn memory(&self) -> RuntimeMemorySnapshot {
        *self.memory.get_or_init(capture_runtime_memory_snapshot)
    }

    pub fn memory_loaded(&self) -> bool {
        self.memory.get().is_some()
    }
}

pub fn capture_runtime_memory_snapshot() -> RuntimeMemorySnapshot {
    let process_stats = read_process_status("/proc/self/status").unwrap_or_default();
    let cgroup_stats = cgroup_probe_file("memory.stat")
        .and_then(read_cgroup_memory_stat)
        .unwrap_or_default();
    let cgroup_swap_bytes = cgroup_probe_file("memory.swap.current")
        .and_then(read_u64_from_file)
        .or_else(|| read_u64_from_file("/sys/fs/cgroup/memory.swap.current"));
    let child_process_rss_bytes = sum_child_rss_bytes(std::process::id());
    let memory_current_bytes = cgroup_probe_file("memory.current")
        .and_then(read_u64_from_file)
        .or_else(|| read_u64_from_file("/sys/fs/cgroup/memory.current"));
    let memory_limit_bytes = cgroup_probe_file("memory.max")
        .and_then(read_memory_limit_from_file)
        .or_else(|| read_memory_limit_from_file("/sys/fs/cgroup/memory.max"));
    let process_group_rss_bytes = process_stats
        .rss_bytes
        .zip(child_process_rss_bytes)
        .map(|(rss, child)| rss + child);
    let headroom_bytes = match (memory_current_bytes, memory_limit_bytes) {
        (Some(current), Some(limit)) if limit >= current => Some(limit - current),
        _ => None,
    };
    RuntimeMemorySnapshot {
        memory_current_bytes,
        memory_limit_bytes,
        cgroup_anon_bytes: cgroup_stats.anon_bytes,
        cgroup_file_bytes: cgroup_stats.file_bytes,
        cgroup_swap_bytes,
        headroom_bytes,
        process_rss_bytes: process_stats.rss_bytes,
        process_rss_anon_bytes: process_stats.rss_anon_bytes,
        process_rss_file_bytes: process_stats.rss_file_bytes,
        process_hwm_bytes: process_stats.hwm_bytes,
        process_swap_bytes: process_stats.swap_bytes,
        child_process_rss_bytes,
        process_group_rss_bytes,
    }
}

#[derive(Debug, Default)]
struct ProcessStatusSnapshot {
    rss_bytes: Option<u64>,
    rss_anon_bytes: Option<u64>,
    rss_file_bytes: Option<u64>,
    hwm_bytes: Option<u64>,
    swap_bytes: Option<u64>,
}

fn read_process_status(path: impl AsRef<Path>) -> Option<ProcessStatusSnapshot> {
    let content = fs::read_to_string(path).ok()?;
    let mut snapshot = ProcessStatusSnapshot::default();
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            snapshot.rss_bytes = parse_kib_line(value);
        } else if let Some(value) = line.strip_prefix("RssAnon:") {
            snapshot.rss_anon_bytes = parse_kib_line(value);
        } else if let Some(value) = line.strip_prefix("RssFile:") {
            snapshot.rss_file_bytes = parse_kib_line(value);
        } else if let Some(value) = line.strip_prefix("VmHWM:") {
            snapshot.hwm_bytes = parse_kib_line(value);
        } else if let Some(value) = line.strip_prefix("VmSwap:") {
            snapshot.swap_bytes = parse_kib_line(value);
        }
    }
    Some(snapshot)
}

fn parse_kib_line(value: &str) -> Option<u64> {
    value
        .split_whitespace()
        .next()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(|kib| kib.saturating_mul(1024))
}

fn sum_child_rss_bytes(pid: u32) -> Option<u64> {
    let tasks_dir = PathBuf::from(format!("/proc/{pid}/task/{pid}/children"));
    let children = fs::read_to_string(tasks_dir).ok()?;
    Some(
        children
            .split_whitespace()
            .filter_map(|child_pid| {
                let child_path = format!("/proc/{child_pid}/status");
                read_process_status(child_path).and_then(|snapshot| snapshot.rss_bytes)
            })
            .sum(),
    )
}

fn cgroup_probe_file(file_name: &str) -> Option<&'static str> {
    static MEMORY_CURRENT_PATH: OnceLock<Option<String>> = OnceLock::new();
    static MEMORY_MAX_PATH: OnceLock<Option<String>> = OnceLock::new();
    static MEMORY_STAT_PATH: OnceLock<Option<String>> = OnceLock::new();
    static MEMORY_SWAP_CURRENT_PATH: OnceLock<Option<String>> = OnceLock::new();
    let store = match file_name {
        "memory.current" => &MEMORY_CURRENT_PATH,
        "memory.max" => &MEMORY_MAX_PATH,
        "memory.stat" => &MEMORY_STAT_PATH,
        "memory.swap.current" => &MEMORY_SWAP_CURRENT_PATH,
        _ => return None,
    };
    store
        .get_or_init(|| resolve_cgroup_file_path(file_name))
        .as_deref()
}

#[derive(Debug, Default)]
struct CgroupMemoryStat {
    anon_bytes: Option<u64>,
    file_bytes: Option<u64>,
}

fn read_cgroup_memory_stat(path: impl AsRef<Path>) -> Option<CgroupMemoryStat> {
    let content = fs::read_to_string(path).ok()?;
    let mut stats = CgroupMemoryStat::default();
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let value = parts.next().and_then(|raw| raw.parse::<u64>().ok());
        match key {
            "anon" => stats.anon_bytes = value,
            "file" => stats.file_bytes = value,
            _ => {}
        }
    }
    Some(stats)
}

fn resolve_cgroup_file_path(file_name: &str) -> Option<String> {
    let relative = read_relative_cgroup_path()?;
    let candidate = PathBuf::from("/sys/fs/cgroup")
        .join(relative.trim_start_matches('/'))
        .join(file_name);
    if candidate.exists() {
        return Some(candidate.to_string_lossy().to_string());
    }
    let fallback = PathBuf::from("/sys/fs/cgroup").join(file_name);
    fallback
        .exists()
        .then(|| fallback.to_string_lossy().to_string())
}

fn read_relative_cgroup_path() -> Option<String> {
    let content = fs::read_to_string("/proc/self/cgroup").ok()?;
    content.lines().find_map(|line| {
        let mut parts = line.splitn(3, ':');
        let _hierarchy = parts.next()?;
        let controllers = parts.next()?;
        let path = parts.next()?;
        if controllers.is_empty() {
            Some(path.to_string())
        } else {
            None
        }
    })
}

fn read_u64_from_file(path: impl AsRef<Path>) -> Option<u64> {
    fs::read_to_string(path)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<u64>().ok())
}

fn read_memory_limit_from_file(path: impl AsRef<Path>) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "max" {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

pub(crate) fn component_for_module_path(module_path: &str) -> &'static str {
    if module_path.contains("::store") {
        "db"
    } else if module_path.contains("::server::schedulers") {
        "scheduler"
    } else if module_path.contains("::server::proxy") {
        "proxy"
    } else if module_path.contains("::server::handlers") {
        "http_handler"
    } else if module_path.contains("::server::serve") || module_path.contains("::proxy_ha") {
        "ha"
    } else if module_path.contains("::proxy_core")
        || module_path.contains("::proxy_forward_proxy_maintenance")
    {
        "forward_proxy"
    } else if module_path.contains("::tavily_proxy") {
        "proxy_runtime"
    } else {
        "runtime"
    }
}

fn runtime_log_env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(DEFAULT_RUNTIME_LOG_FILTER))
        .unwrap_or_else(|_| EnvFilter::new("warn"))
}

fn build_runtime_log_dispatch<W>(format: RuntimeLogFormat, filter: EnvFilter, writer: W) -> Dispatch
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    match format {
        RuntimeLogFormat::Json => Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_target(true)
                .finish(),
        ),
        RuntimeLogFormat::Text => Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .compact()
                .with_ansi(false)
                .with_target(true)
                .finish(),
        ),
    }
}

fn install_log_tracer() {
    LOG_TRACER_INIT.get_or_init(|| {
        let _ = tracing_log::LogTracer::builder()
            .with_max_level(log::LevelFilter::Trace)
            .init();
    });
}

#[allow(unused_macros)]
macro_rules! println {
    ($($arg:tt)*) => {{
        $crate::runtime_logging::emit_legacy_stdio_event(
            $crate::runtime_logging::LegacyStdIoLevel::Info,
            module_path!(),
            file!(),
            line!(),
            format_args!($($arg)*),
        )
    }};
}

#[allow(unused_macros)]
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        $crate::runtime_logging::emit_legacy_stdio_event(
            $crate::runtime_logging::LegacyStdIoLevel::Warn,
            module_path!(),
            file!(),
            line!(),
            format_args!($($arg)*),
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());
    static LOG_BRIDGE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Clone)]
    struct SharedWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedWriter {
        fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
            let buffer = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    buffer: buffer.clone(),
                },
                buffer,
            )
        }
    }

    struct SharedWriterGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = SharedWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriterGuard {
                buffer: self.buffer.clone(),
            }
        }
    }

    impl Write for SharedWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer
                .lock()
                .expect("writer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn captured_output(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buffer.lock().expect("buffer lock").clone()).expect("utf8 log output")
    }

    fn capture_tracing_output<F>(format: RuntimeLogFormat, filter: EnvFilter, emit: F) -> String
    where
        F: FnOnce(),
    {
        let (writer, buffer) = SharedWriter::new();
        let dispatch = build_runtime_log_dispatch(format, filter, writer);
        tracing::dispatcher::with_default(&dispatch, emit);
        captured_output(&buffer)
    }

    fn temp_proc_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tavily-hikari-runtime-logging-{prefix}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp proc dir");
        dir
    }

    #[test]
    fn default_runtime_log_format_is_json() {
        let output =
            capture_tracing_output(RuntimeLogFormat::default(), EnvFilter::new("info"), || {
                tracing::info!(
                    component = "test",
                    event = "startup",
                    message = "json-ready"
                );
            });
        let line = output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let payload: Value = serde_json::from_str(line).expect("valid json log line");
        assert_eq!(payload["component"], "test");
        assert_eq!(payload["event"], "startup");
        assert_eq!(payload["message"], "json-ready");
        assert!(payload.get("level").is_some());
    }

    #[test]
    fn default_runtime_log_filter_keeps_crate_info_visible() {
        let output = capture_tracing_output(
            RuntimeLogFormat::Json,
            EnvFilter::new(DEFAULT_RUNTIME_LOG_FILTER),
            || {
                emit_legacy_stdio_event(
                    LegacyStdIoLevel::Info,
                    "tavily_hikari::runtime_logging::tests",
                    file!(),
                    line!(),
                    format_args!("startup-visible"),
                );
                tracing::info!(
                    target: "external_lib",
                    component = "foreign",
                    event = "filtered",
                    message = "noise"
                );
            },
        );
        assert!(output.contains("startup-visible"));
        assert!(!output.contains("\"target\":\"external_lib\""));
    }

    #[test]
    fn sql_statement_logging_requires_explicit_sqlx_debug() {
        let emit_statement = || {
            tracing::debug!(
                target: "sqlx::query",
                statement = "SELECT sensitive_value FROM request_logs",
                rows_affected = 1_u64,
                "query completed"
            );
        };

        let default_output = capture_tracing_output(
            RuntimeLogFormat::Json,
            EnvFilter::new(DEFAULT_RUNTIME_LOG_FILTER),
            emit_statement,
        );
        assert!(!default_output.contains("sensitive_value"));

        let debug_output = capture_tracing_output(
            RuntimeLogFormat::Json,
            EnvFilter::new("warn,tavily_hikari=info,sqlx::query=debug"),
            emit_statement,
        );
        assert!(debug_output.contains("sensitive_value"));
    }

    #[test]
    fn explicit_text_fallback_format_is_supported() {
        let output = capture_tracing_output(RuntimeLogFormat::Text, EnvFilter::new("info"), || {
            tracing::info!(
                component = "test",
                event = "text-fallback",
                message = "plain"
            );
        });
        assert!(serde_json::from_str::<Value>(output.trim()).is_err());
        assert!(output.contains("text-fallback"));
        assert!(output.contains("plain"));
    }

    #[test]
    fn rust_log_env_filter_still_controls_runtime_logs() {
        let _guard = ENV_TEST_LOCK.lock().expect("env test lock");
        let previous = std::env::var("RUST_LOG").ok();
        unsafe {
            std::env::set_var("RUST_LOG", "error");
        }
        let output =
            capture_tracing_output(RuntimeLogFormat::Json, runtime_log_env_filter(), || {
                tracing::warn!(component = "test", event = "filtered-out", message = "warn");
                tracing::error!(component = "test", event = "kept", message = "error");
            });
        match previous {
            Some(value) => unsafe { std::env::set_var("RUST_LOG", value) },
            None => unsafe { std::env::remove_var("RUST_LOG") },
        }
        assert!(!output.contains("filtered-out"));
        assert!(output.contains("\"event\":\"kept\""));
    }

    #[test]
    fn log_crate_records_are_bridged_into_runtime_subscriber() {
        let _guard = LOG_BRIDGE_TEST_LOCK.lock().expect("log bridge lock");
        install_log_tracer();
        let (writer, buffer) = SharedWriter::new();
        let dispatch =
            build_runtime_log_dispatch(RuntimeLogFormat::Json, EnvFilter::new("warn"), writer);
        tracing::dispatcher::with_default(&dispatch, || {
            log::warn!("log-bridge-ok");
        });
        let output = captured_output(&buffer);
        let line = output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let payload: Value = serde_json::from_str(line).expect("valid json log line");
        assert_eq!(payload["message"], "log-bridge-ok");
    }

    #[test]
    fn runtime_memory_helpers_parse_status_and_cgroup_values() {
        let proc_dir = temp_proc_dir("memory-snapshot");
        let cgroup_dir = proc_dir.join("cg");
        fs::create_dir_all(&cgroup_dir).expect("create cgroup dir");
        fs::write(
            proc_dir.join("status"),
            "VmRSS:\t128 kB\nVmHWM:\t256 kB\nVmSwap:\t64 kB\n",
        )
        .expect("write status");
        fs::write(proc_dir.join("cgroup"), "0::/test.scope\n").expect("write cgroup");
        fs::write(cgroup_dir.join("memory.current"), "4096\n").expect("write memory.current");
        fs::write(cgroup_dir.join("memory.max"), "8192\n").expect("write memory.max");

        let process = read_process_status(proc_dir.join("status")).expect("status parsed");
        assert_eq!(process.rss_bytes, Some(128 * 1024));
        assert_eq!(process.hwm_bytes, Some(256 * 1024));
        assert_eq!(process.swap_bytes, Some(64 * 1024));
        assert_eq!(
            read_u64_from_file(cgroup_dir.join("memory.current")),
            Some(4096)
        );
        assert_eq!(
            read_memory_limit_from_file(cgroup_dir.join("memory.max")),
            Some(8192)
        );
    }

    #[test]
    fn runtime_perf_scope_exposes_elapsed_and_memory_fields() {
        let output = capture_tracing_output(RuntimeLogFormat::Json, EnvFilter::new("info"), || {
            let perf = RuntimePerfScope::start();
            let memory = perf.memory();
            tracing::info!(
                component = "test",
                event = "perf_scope",
                elapsed_ms = perf.elapsed_ms(),
                memory_current_bytes = memory.memory_current_bytes.unwrap_or_default(),
                memory_limit_bytes = memory.memory_limit_bytes.unwrap_or_default(),
                headroom_bytes = memory.headroom_bytes.unwrap_or_default(),
                process_rss_bytes = memory.process_rss_bytes.unwrap_or_default(),
                child_process_rss_bytes = memory.child_process_rss_bytes.unwrap_or_default(),
                process_group_rss_bytes = memory.process_group_rss_bytes.unwrap_or_default(),
                process_hwm_bytes = memory.process_hwm_bytes.unwrap_or_default(),
                process_swap_bytes = memory.process_swap_bytes.unwrap_or_default(),
                "perf"
            );
        });
        let line = output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let payload: Value = serde_json::from_str(line).expect("valid json log line");
        assert_eq!(payload["event"], "perf_scope");
        assert!(payload.get("elapsed_ms").is_some());
        assert!(payload.get("memory_current_bytes").is_some());
        assert!(payload.get("process_rss_bytes").is_some());
    }

    #[test]
    fn runtime_perf_scope_does_not_probe_memory_until_a_log_needs_it() {
        let perf = RuntimePerfScope::start();
        assert!(!perf.memory_loaded());
        let _ = perf.memory();
        assert!(perf.memory_loaded());
    }
}
