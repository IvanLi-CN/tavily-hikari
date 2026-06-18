use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{ObservabilitySidecarMigrationReport, run_observability_sidecar_migrate};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Migrate legacy observability tables into the sibling observability sidecar"
)]
struct Cli {
    /// SQLite database path to inspect or migrate.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Maximum request_logs rows to copy per batch.
    #[arg(long, default_value_t = 5000, value_parser = positive_i64)]
    batch_size: i64,

    /// Inspect the current layout without mutating the database.
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Emit JSON output.
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

fn write_json_report(
    mut writer: impl Write,
    report: &ObservabilitySidecarMigrationReport,
) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn write_plain_report(
    mut writer: impl Write,
    report: &ObservabilitySidecarMigrationReport,
) -> io::Result<()> {
    writeln!(
        writer,
        "observability_sidecar_migrate: dry_run={} completed={} startup_reopen_verified={} offline_lock={} sqlite_write_probe_ok={} copied_request_logs={} batches={} already_migrated={} resumed_copy={} core_path={} sidecar_path={} lock_path={} attached_observability_path={}",
        report.dry_run,
        report.completed,
        report.startup_reopen_verified,
        report.offline_lock_acquired,
        report.sqlite_write_probe_ok,
        report.copied_request_logs,
        report.batches,
        report.already_migrated,
        report.resumed_copy,
        report.core_path,
        report.sidecar_path,
        report.sibling_lock_path,
        report.attached_observability_path,
    )?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report =
        run_observability_sidecar_migrate(&cli.db_path, cli.batch_size, cli.dry_run).await?;

    if cli.json {
        write_json_report(io::stdout().lock(), &report)?;
    } else {
        write_plain_report(io::stdout().lock(), &report)?;
    }

    Ok(())
}
