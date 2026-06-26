use crate::runtime_logging::RuntimeLogFormat;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

#[derive(Clone, Default)]
struct SharedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl SharedWriter {
    fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                buffer: Arc::clone(&buffer),
            },
            buffer,
        )
    }
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard {
            buffer: Arc::clone(&self.buffer),
        }
    }
}

struct SharedWriterGuard {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl std::io::Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer
            .lock()
            .expect("buffer lock")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_tracing_output<F>(filter: EnvFilter, emit: F) -> String
where
    F: FnOnce(),
{
    let (writer, buffer) = SharedWriter::new();
    let dispatch = match RuntimeLogFormat::Json {
        RuntimeLogFormat::Json => tracing::Dispatch::new(
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
        RuntimeLogFormat::Text => unreachable!("tests use json logs only"),
    };
    tracing::dispatcher::with_default(&dispatch, emit);
    String::from_utf8(buffer.lock().expect("buffer lock").clone()).expect("utf8 log output")
}

#[test]
fn perf_logs_are_info_level_and_include_memory_budget_fields() {
    let output = capture_tracing_output(EnvFilter::new("info"), || {
        emit_perf_log(
            DbLogStatus::Info,
            "admin_read",
            "request_logs_list_completed",
            Duration::from_millis(42),
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some("global"),
                page_size: Some(50),
                row_count: Some(12),
                degraded: Some("full"),
                ..Default::default()
            },
        );
        emit_low_memory_protection_decision(
            "admin_read",
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some("global"),
                page_size: Some(200),
                degraded: Some("protected"),
                ..Default::default()
            },
        );
    });
    let records = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("valid json log line"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);

    let perf = &records[0];
    assert_eq!(perf["level"], "INFO");
    assert_eq!(perf["component"], "admin_read");
    assert_eq!(perf["event"], "request_logs_list_completed");
    assert_eq!(perf["route"], "/api/logs/list");
    assert_eq!(perf["scope"], "global");
    assert_eq!(perf["page_size"], 50);
    assert_eq!(perf["row_count"], 12);
    assert_eq!(perf["degraded"], "full");
    assert_eq!(perf["phase"], "");
    assert_eq!(perf["outbox_row_count"], 0);
    assert_eq!(perf["outbox_oldest_age_secs"], 0);
    assert_eq!(perf["outbox_ack_lag"], 0);
    assert!(perf.get("memory_current_bytes").is_some());
    assert!(perf.get("memory_limit_bytes").is_some());
    assert!(perf.get("headroom_bytes").is_some());

    let decision = &records[1];
    assert_eq!(decision["level"], "INFO");
    assert_eq!(decision["component"], "admin_read");
    assert_eq!(decision["event"], "low_memory_protection_decision");
    assert_eq!(decision["degraded"], "protected");
    assert_eq!(decision["page_size"], 200);
    assert_eq!(decision["elapsed_ms"], 0);
    assert!(decision.get("memory_current_bytes").is_some());
    assert!(decision.get("memory_limit_bytes").is_some());
    assert!(decision.get("headroom_bytes").is_some());
}

#[test]
fn low_memory_protection_duplicate_logs_are_sampled() {
    let output = capture_tracing_output(EnvFilter::new("info"), || {
        emit_low_memory_protection_decision(
            "admin_read",
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some("global"),
                phase: Some("cache_serve"),
                degraded: Some("cache_hit"),
                ..Default::default()
            },
        );
        emit_low_memory_protection_decision(
            "admin_read",
            PerfLogScope {
                route: Some("/api/logs/list"),
                scope: Some("global"),
                phase: Some("cache_serve"),
                degraded: Some("cache_hit"),
                ..Default::default()
            },
        );
    });
    let records = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
}
