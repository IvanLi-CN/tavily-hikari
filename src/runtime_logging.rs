use std::{fmt as stdfmt, io, sync::OnceLock};

use clap::ValueEnum;
use tracing::Dispatch;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

const DEFAULT_RUNTIME_LOG_FILTER: &str = "warn,sqlx::query=warn";

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
        io::Write,
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
}
