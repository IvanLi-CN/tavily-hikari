use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use tavily_hikari::{DbCompactionReport, run_db_compaction_once};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run SQLite DB compaction once, with threshold-aware skip or forced execution"
)]
struct Cli {
    /// SQLite database path to mutate.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Force compaction even when reclaimable space is below the automatic threshold.
    #[arg(long, default_value_t = false)]
    force: bool,

    /// Emit JSON output. Plain output is retained for interactive use.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliReport {
    report: DbCompactionReport,
}

fn write_plain(report: &DbCompactionReport) {
    if report.skipped {
        println!(
            "db_compaction: skipped=true forced={} reason={} database_bytes_before={} database_bytes_after={} wal_bytes_before={} wal_bytes_after={} reclaimable_bytes_before={} reclaimable_bytes_after={} freelist_before={} freelist_after={} elapsed_ms={}",
            report.forced,
            report.reason.as_deref().unwrap_or("unknown"),
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.freelist_count,
            report.after.freelist_count,
            report.elapsed_ms
        );
    } else {
        println!(
            "db_compaction: skipped=false forced={} database_bytes_before={} database_bytes_after={} wal_bytes_before={} wal_bytes_after={} reclaimable_bytes_before={} reclaimable_bytes_after={} freelist_before={} freelist_after={} elapsed_ms={}",
            report.forced,
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.freelist_count,
            report.after.freelist_count,
            report.elapsed_ms
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report = run_db_compaction_once(&cli.db_path, cli.force).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&CliReport { report })?);
    } else {
        write_plain(&report);
    }
    Ok(())
}
