use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use tavily_hikari::{RequestLogsGcOptions, RequestLogsGcReport, run_request_logs_gc_once};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run bounded request_logs retention GC once, or repeatedly until complete"
)]
struct Cli {
    /// SQLite database path to mutate.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Maximum request_logs and rollup rows to delete per batch.
    #[arg(long, default_value_t = RequestLogsGcOptions::default().batch_size, value_parser = positive_i64)]
    batch_size: i64,

    /// Maximum batches per GC pass.
    #[arg(long, default_value_t = RequestLogsGcOptions::default().max_batches, value_parser = positive_i64)]
    max_batches: i64,

    /// Maximum seconds per GC pass.
    #[arg(long, default_value_t = RequestLogsGcOptions::default().max_runtime_secs, value_parser = positive_u64)]
    max_runtime_secs: u64,

    /// Sleep between batches to reduce write-lock pressure.
    #[arg(long, default_value_t = RequestLogsGcOptions::default().inter_batch_sleep_ms)]
    inter_batch_sleep_ms: u64,

    /// Continue running bounded passes until no old request log or rollup rows remain.
    #[arg(long, default_value_t = false)]
    run_until_complete: bool,

    /// Emit JSON output. Plain output is retained for interactive use.
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
#[serde(rename_all = "camelCase")]
struct CliReport {
    run_until_complete: bool,
    passes: usize,
    retention_days: i64,
    threshold: i64,
    batch_size: i64,
    max_batches: i64,
    deleted_request_logs: i64,
    deleted_rollups: i64,
    batches: i64,
    completed: bool,
    has_more: bool,
    elapsed_ms: u128,
    pass_reports: Vec<RequestLogsGcReport>,
}

impl CliReport {
    fn from_passes(run_until_complete: bool, reports: Vec<RequestLogsGcReport>) -> Self {
        let last = reports
            .last()
            .expect("request logs gc cli always records at least one pass");
        Self {
            run_until_complete,
            passes: reports.len(),
            retention_days: last.retention_days,
            threshold: last.threshold,
            batch_size: last.batch_size,
            max_batches: last.max_batches,
            deleted_request_logs: reports
                .iter()
                .map(|report| report.deleted_request_logs)
                .sum(),
            deleted_rollups: reports.iter().map(|report| report.deleted_rollups).sum(),
            batches: reports.iter().map(|report| report.batches).sum(),
            completed: last.completed,
            has_more: last.has_more,
            elapsed_ms: reports.iter().map(|report| report.elapsed_ms).sum(),
            pass_reports: reports,
        }
    }
}

fn write_json_report(mut writer: impl Write, report: &CliReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn write_plain_report(mut writer: impl Write, report: &CliReport) -> io::Result<()> {
    writeln!(
        writer,
        "request_logs_gc: completed={} has_more={} passes={} deleted_request_logs={} deleted_rollups={} batches={} elapsed_ms={} retention_days={}",
        report.completed,
        report.has_more,
        report.passes,
        report.deleted_request_logs,
        report.deleted_rollups,
        report.batches,
        report.elapsed_ms,
        report.retention_days
    )?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let options = RequestLogsGcOptions {
        batch_size: cli.batch_size,
        max_batches: cli.max_batches,
        max_runtime_secs: cli.max_runtime_secs,
        inter_batch_sleep_ms: cli.inter_batch_sleep_ms,
    };
    let mut reports = Vec::new();

    loop {
        let report = run_request_logs_gc_once(&cli.db_path, options).await?;
        let completed = report.completed;
        reports.push(report);
        if completed || !cli.run_until_complete {
            break;
        }
    }

    let cli_report = CliReport::from_passes(cli.run_until_complete, reports);
    if cli.json {
        write_json_report(io::stdout().lock(), &cli_report)?;
    } else {
        write_plain_report(io::stdout().lock(), &cli_report)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_report_sums_passes() {
        let report = CliReport::from_passes(
            true,
            vec![
                RequestLogsGcReport {
                    retention_days: 32,
                    threshold: 100,
                    batch_size: 10,
                    max_batches: 1,
                    deleted_request_logs: 10,
                    deleted_rollups: 4,
                    batches: 1,
                    completed: false,
                    has_more: true,
                    elapsed_ms: 12,
                },
                RequestLogsGcReport {
                    retention_days: 32,
                    threshold: 100,
                    batch_size: 10,
                    max_batches: 1,
                    deleted_request_logs: 2,
                    deleted_rollups: 1,
                    batches: 1,
                    completed: true,
                    has_more: false,
                    elapsed_ms: 8,
                },
            ],
        );

        assert_eq!(report.deleted_request_logs, 12);
        assert_eq!(report.deleted_rollups, 5);
        assert!(report.completed);
        assert_eq!(report.elapsed_ms, 20);
    }

    #[test]
    fn cli_rejects_zero_runtime() {
        let err = Cli::try_parse_from([
            "request_logs_gc_once",
            "--max-runtime-secs",
            "0",
            "--run-until-complete",
        ])
        .expect_err("zero runtime must be rejected");

        assert!(err.to_string().contains("expected a positive integer"));
    }
}
