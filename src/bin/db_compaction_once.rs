use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use tavily_hikari::{DbCompactionReport, SqliteDbStats, run_db_compaction_once};

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

fn total_wal_bytes(stats: &SqliteDbStats) -> u64 {
    stats.wal_bytes + stats.observability_wal_bytes
}

fn total_freelist_count(stats: &SqliteDbStats) -> i64 {
    stats
        .freelist_count
        .saturating_add(stats.observability_freelist_count)
}

fn write_plain(report: &DbCompactionReport) {
    let total_database_bytes_before =
        report.before.database_bytes + report.before.observability_database_bytes;
    let total_database_bytes_after =
        report.after.database_bytes + report.after.observability_database_bytes;
    let total_wal_bytes_before = total_wal_bytes(&report.before);
    let total_wal_bytes_after = total_wal_bytes(&report.after);
    let total_freelist_before = total_freelist_count(&report.before);
    let total_freelist_after = total_freelist_count(&report.after);
    if report.skipped {
        println!(
            "db_compaction: skipped=true forced={} reason={} database_bytes_before={} database_bytes_after={} observability_database_bytes_before={} observability_database_bytes_after={} total_database_bytes_before={} total_database_bytes_after={} wal_bytes_before={} wal_bytes_after={} observability_wal_bytes_before={} observability_wal_bytes_after={} total_wal_bytes_before={} total_wal_bytes_after={} total_reclaimable_bytes_before={} total_reclaimable_bytes_after={} total_reclaimable_ratio_before={:.6} total_reclaimable_ratio_after={:.6} reclaimable_bytes_before={} reclaimable_bytes_after={} reclaimable_ratio_before={:.6} reclaimable_ratio_after={:.6} observability_reclaimable_bytes_before={} observability_reclaimable_bytes_after={} observability_reclaimable_ratio_before={:.6} observability_reclaimable_ratio_after={:.6} freelist_before={} freelist_after={} observability_freelist_before={} observability_freelist_after={} total_freelist_before={} total_freelist_after={} elapsed_ms={}",
            report.forced,
            report.reason.as_deref().unwrap_or("unknown"),
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.observability_database_bytes,
            report.after.observability_database_bytes,
            total_database_bytes_before,
            total_database_bytes_after,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.observability_wal_bytes,
            report.after.observability_wal_bytes,
            total_wal_bytes_before,
            total_wal_bytes_after,
            report.before.total_reclaimable_bytes,
            report.after.total_reclaimable_bytes,
            report.before.total_reclaimable_ratio,
            report.after.total_reclaimable_ratio,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.reclaimable_ratio,
            report.after.reclaimable_ratio,
            report.before.observability_reclaimable_bytes,
            report.after.observability_reclaimable_bytes,
            report.before.observability_reclaimable_ratio,
            report.after.observability_reclaimable_ratio,
            report.before.freelist_count,
            report.after.freelist_count,
            report.before.observability_freelist_count,
            report.after.observability_freelist_count,
            total_freelist_before,
            total_freelist_after,
            report.elapsed_ms
        );
    } else {
        println!(
            "db_compaction: skipped=false forced={} database_bytes_before={} database_bytes_after={} observability_database_bytes_before={} observability_database_bytes_after={} total_database_bytes_before={} total_database_bytes_after={} wal_bytes_before={} wal_bytes_after={} observability_wal_bytes_before={} observability_wal_bytes_after={} total_wal_bytes_before={} total_wal_bytes_after={} total_reclaimable_bytes_before={} total_reclaimable_bytes_after={} total_reclaimable_ratio_before={:.6} total_reclaimable_ratio_after={:.6} reclaimable_bytes_before={} reclaimable_bytes_after={} reclaimable_ratio_before={:.6} reclaimable_ratio_after={:.6} observability_reclaimable_bytes_before={} observability_reclaimable_bytes_after={} observability_reclaimable_ratio_before={:.6} observability_reclaimable_ratio_after={:.6} freelist_before={} freelist_after={} observability_freelist_before={} observability_freelist_after={} total_freelist_before={} total_freelist_after={} elapsed_ms={}",
            report.forced,
            report.before.database_bytes,
            report.after.database_bytes,
            report.before.observability_database_bytes,
            report.after.observability_database_bytes,
            total_database_bytes_before,
            total_database_bytes_after,
            report.before.wal_bytes,
            report.after.wal_bytes,
            report.before.observability_wal_bytes,
            report.after.observability_wal_bytes,
            total_wal_bytes_before,
            total_wal_bytes_after,
            report.before.total_reclaimable_bytes,
            report.after.total_reclaimable_bytes,
            report.before.total_reclaimable_ratio,
            report.after.total_reclaimable_ratio,
            report.before.reclaimable_bytes,
            report.after.reclaimable_bytes,
            report.before.reclaimable_ratio,
            report.after.reclaimable_ratio,
            report.before.observability_reclaimable_bytes,
            report.after.observability_reclaimable_bytes,
            report.before.observability_reclaimable_ratio,
            report.after.observability_reclaimable_ratio,
            report.before.freelist_count,
            report.after.freelist_count,
            report.before.observability_freelist_count,
            report.after.observability_freelist_count,
            total_freelist_before,
            total_freelist_after,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn stats() -> SqliteDbStats {
        SqliteDbStats {
            database_bytes: 10,
            wal_bytes: 20,
            observability_database_bytes: 30,
            observability_wal_bytes: 40,
            page_size: 4096,
            page_count: 10,
            freelist_count: 2,
            reclaimable_bytes: 8192,
            reclaimable_ratio: 0.2,
            observability_page_size: 4096,
            observability_page_count: 20,
            observability_freelist_count: 3,
            observability_reclaimable_bytes: 12288,
            observability_reclaimable_ratio: 0.15,
            total_reclaimable_bytes: 20480,
            total_reclaimable_ratio: 0.166,
        }
    }

    #[test]
    fn compaction_report_json_and_aggregate_plain_metrics_are_sidecar_aware() {
        let report = DbCompactionReport {
            skipped: false,
            forced: true,
            reason: None,
            min_reclaimable_bytes: 512 * 1024 * 1024,
            min_reclaimable_ratio: 0.2,
            before: stats(),
            after: stats(),
            elapsed_ms: 7,
        };
        assert_eq!(total_wal_bytes(&report.before), 60);
        assert_eq!(total_freelist_count(&report.before), 5);

        let json = serde_json::to_value(CliReport { report }).expect("serialize report");
        assert_eq!(json["report"]["before"]["observability_database_bytes"], 30);
        assert_eq!(json["report"]["before"]["total_reclaimable_bytes"], 20480);
        assert_eq!(json["report"]["before"]["observability_freelist_count"], 3);
        assert!(matches!(json, Value::Object(_)));
    }
}
