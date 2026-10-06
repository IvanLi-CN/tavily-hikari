use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use tavily_hikari::{
    RequestStatisticsRecoveryOptions, RequestStatisticsRecoveryReport,
    run_request_statistics_recovery_once,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run one bounded, exclusive request statistics integrity and retention recovery pass"
)]
struct Cli {
    /// SQLite database path to recover.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Explicit local-day start in UTC seconds for the fixed recovery target.
    #[arg(long, value_parser = positive_i64)]
    target_day_start: i64,

    /// Maximum wall-clock seconds for this resumable pass.
    #[arg(long, default_value_t = RequestStatisticsRecoveryOptions::default().max_runtime_secs, value_parser = positive_u64)]
    max_runtime_secs: u64,

    /// Maximum request-log rows or bodies handled per GC batch.
    #[arg(long, default_value_t = RequestStatisticsRecoveryOptions::default().gc_batch_size, value_parser = positive_i64)]
    gc_batch_size: i64,

    /// Maximum GC batches per recovery loop iteration.
    #[arg(long, default_value_t = RequestStatisticsRecoveryOptions::default().gc_max_batches, value_parser = positive_i64)]
    gc_max_batches: i64,

    /// Sleep between GC batches to reduce writer pressure.
    #[arg(long, default_value_t = RequestStatisticsRecoveryOptions::default().gc_inter_batch_sleep_ms)]
    gc_inter_batch_sleep_ms: u64,

    /// Emit a machine-readable report.
    #[arg(long, default_value_t = false)]
    json: bool,
}

fn positive_i64(value: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<i64>()
        .map_err(|err| format!("expected a positive integer: {err}"))?;
    if parsed > 0 {
        Ok(parsed)
    } else {
        Err("expected a positive integer".to_string())
    }
}

fn positive_u64(value: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|err| format!("expected a positive integer: {err}"))?;
    if parsed > 0 {
        Ok(parsed)
    } else {
        Err("expected a positive integer".to_string())
    }
}

#[derive(Debug, Serialize)]
struct FailureReport {
    outcome: &'static str,
    error: String,
}

fn write_json(mut writer: impl Write, report: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn write_plain_report(
    mut writer: impl Write,
    report: &RequestStatisticsRecoveryReport,
) -> io::Result<()> {
    writeln!(
        writer,
        "request_statistics_recovery: outcome={} target_day_start={} target_day_end={} source_fence={} checkpoint={} accepted_checkpoints={} target_complete={} sealed_day={} expired_rows_remaining={} integrity_slices={} gc_passes={} cleaned_bodies={} deleted_rows={} deleted_rollups={} defer_reason={} error={} elapsed_ms={} time_since_effective_progress_ms={}",
        report.outcome,
        report.target_day_start,
        report.target_day_end,
        report.source_fence,
        report.checkpoint,
        report.accepted_checkpoints,
        report.target_complete,
        report.sealed_day,
        report.target_expired_rows_remaining,
        report.integrity_slices,
        report.gc_passes,
        report.cleaned_request_log_bodies,
        report.deleted_request_logs,
        report.deleted_rollups,
        report.last_defer_reason.as_deref().unwrap_or("none"),
        report.error.as_deref().unwrap_or("none"),
        report.elapsed_ms,
        report.time_since_effective_progress_ms,
    )?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let options = RequestStatisticsRecoveryOptions {
        target_day_start: cli.target_day_start,
        max_runtime_secs: cli.max_runtime_secs,
        gc_batch_size: cli.gc_batch_size,
        gc_max_batches: cli.gc_max_batches,
        gc_inter_batch_sleep_ms: cli.gc_inter_batch_sleep_ms,
    };
    match run_request_statistics_recovery_once(&cli.db_path, options).await {
        Ok(report) => {
            if cli.json {
                write_json(io::stdout().lock(), &report)?;
            } else {
                write_plain_report(io::stdout().lock(), &report)?;
            }
        }
        Err(err) => {
            let outcome = if err.to_string().contains("exclusive database ownership") {
                "deferred"
            } else {
                "failed"
            };
            let report = FailureReport {
                outcome,
                error: err.to_string(),
            };
            if cli.json {
                write_json(io::stdout().lock(), &report)?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "request_statistics_recovery: outcome={} error={}",
                    report.outcome,
                    report.error
                )?;
            }
        }
    }
    Ok(())
}
