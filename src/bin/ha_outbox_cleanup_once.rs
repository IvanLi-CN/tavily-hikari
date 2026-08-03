use std::{
    io::{self, Write},
    str::FromStr,
};

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use sqlx::{ConnectOptions, SqlitePool, sqlite::SqliteConnectOptions};
use tavily_hikari::{
    HaOutboxGcChannelReport, HaOutboxGcOptions, HaOutboxGcReport, HaSyncChannel,
    format_ha_outbox_gc_report_message, run_ha_outbox_gc_once,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run bounded HA control outbox GC once, or repeatedly until complete"
)]
struct Cli {
    /// SQLite database path to inspect or mutate.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Maximum ha_outbox rows to delete per batch.
    #[arg(long, default_value_t = HaOutboxGcOptions::default().batch_size, value_parser = positive_i64)]
    batch_size: i64,

    /// Maximum batches per GC pass.
    #[arg(long, default_value_t = HaOutboxGcOptions::default().max_batches, value_parser = positive_i64)]
    max_batches: i64,

    /// Maximum seconds per GC pass.
    #[arg(long, default_value_t = HaOutboxGcOptions::default().max_runtime_secs, value_parser = positive_u64)]
    max_runtime_secs: u64,

    /// Sleep between batches to reduce write pressure.
    #[arg(long, default_value_t = HaOutboxGcOptions::default().inter_batch_sleep_ms)]
    inter_batch_sleep_ms: u64,

    /// Repair HA triggers against the current three-channel contract before cleanup.
    #[arg(long, default_value_t = false)]
    repair_triggers: bool,

    /// HA mode used when repairing triggers.
    #[arg(long, env = "HA_MODE", default_value = "active_standby")]
    ha_mode: String,

    /// Continue running bounded passes until no retained control outbox rows remain.
    #[arg(long, default_value_t = false)]
    run_until_complete: bool,

    /// Emit JSON output. Plain output is retained for interactive use.
    #[arg(long, default_value_t = false)]
    json: bool,

    /// Inspect cleanup readiness without repairing triggers or deleting data.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightChannel {
    channel: &'static str,
    retention_secs: i64,
    created_at_index_present: bool,
    oldest_age_secs: Option<i64>,
    high_watermark: i64,
    lowest_peer_ack: Option<i64>,
    pending_cleanup: bool,
    gc_progress: Option<PreflightGcProgress>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PreflightGcProgress {
    last_observed_at: Option<i64>,
    last_high_watermark: i64,
    last_ingress_seq_delta: Option<i64>,
    last_net_rows_delta_estimate: Option<i64>,
    total_deleted_rows: i64,
    last_progress_at: Option<i64>,
    last_defer_reason: Option<String>,
    next_retry_at: Option<i64>,
    batch_size: i64,
    last_continuation_delay_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightReport {
    dry_run: bool,
    channels: Vec<PreflightChannel>,
}

async fn read_only_preflight(db_path: &str) -> Result<PreflightReport, Box<dyn std::error::Error>> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))?
        .read_only(true)
        .disable_statement_logging();
    let pool = SqlitePool::connect_with(options).await?;
    let now = chrono::Utc::now().timestamp();
    let gc_progress_supported: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
               SELECT 1
                 FROM pragma_table_info('ha_outbox_gc_channel_state')
                WHERE name = 'last_net_rows_delta_estimate'
           )"#,
    )
    .fetch_one(&pool)
    .await?;
    let mut channels = Vec::with_capacity(3);
    for (ha_channel, table, index, retention_secs) in [
        (
            HaSyncChannel::Control,
            "ha_outbox",
            "idx_ha_outbox_created",
            72 * 60 * 60,
        ),
        (
            HaSyncChannel::Billing,
            "ha_billing_outbox",
            "idx_ha_billing_outbox_created",
            14 * 24 * 60 * 60,
        ),
        (
            HaSyncChannel::Runtime,
            "ha_runtime_outbox",
            "idx_ha_runtime_outbox_created",
            14 * 24 * 60 * 60,
        ),
    ] {
        let channel = ha_channel.as_str();
        let created_at_index_present: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?)",
        )
        .bind(index)
        .fetch_one(&pool)
        .await?;
        let oldest_created_at: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT created_at FROM {table} ORDER BY created_at ASC, seq ASC LIMIT 1"
        ))
        .fetch_optional(&pool)
        .await?;
        let high_watermark: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT seq FROM {table} ORDER BY seq DESC LIMIT 1"
        ))
        .fetch_optional(&pool)
        .await?;
        let lowest_peer_ack: Option<i64> = sqlx::query_scalar(
            "SELECT acked_seq FROM ha_peer_watermarks WHERE channel = ? ORDER BY acked_seq ASC LIMIT 1",
        )
        .bind(channel)
        .fetch_optional(&pool)
        .await?;
        let allowed_resources = tavily_hikari::ha_outbox_gc_allowed_resources(ha_channel);
        let placeholders = std::iter::repeat_n("?", allowed_resources.len())
            .collect::<Vec<_>>()
            .join(", ");
        let pending_sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {table} WHERE created_at < ? OR resource NOT IN ({placeholders}) LIMIT 1)"
        );
        let mut pending_query =
            sqlx::query_scalar::<_, bool>(&pending_sql).bind(now - retention_secs);
        for resource in allowed_resources {
            pending_query = pending_query.bind(*resource);
        }
        let pending_cleanup = pending_query.fetch_one(&pool).await?;
        let gc_progress = if gc_progress_supported {
            sqlx::query_as::<_, PreflightGcProgress>(
                r#"SELECT last_observed_at, last_high_watermark, last_ingress_seq_delta,
                          last_net_rows_delta_estimate, total_deleted_rows, last_progress_at,
                          last_defer_reason, next_retry_at, batch_size,
                          last_continuation_delay_secs
                   FROM ha_outbox_gc_channel_state
                   WHERE channel = ?"#,
            )
            .bind(channel)
            .fetch_optional(&pool)
            .await?
        } else {
            None
        };
        channels.push(PreflightChannel {
            channel,
            retention_secs,
            created_at_index_present,
            oldest_age_secs: oldest_created_at.map(|value| now.saturating_sub(value).max(0)),
            high_watermark: high_watermark.unwrap_or(0),
            lowest_peer_ack,
            pending_cleanup,
            gc_progress,
        });
    }
    pool.close().await;
    Ok(PreflightReport {
        dry_run: true,
        channels,
    })
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
    repaired_triggers: bool,
    trigger_repair_report: Option<tavily_hikari::HaTriggerRepairReport>,
    run_until_complete: bool,
    passes: usize,
    batch_size: i64,
    max_batches: i64,
    deleted_rows: i64,
    invalid_legacy_deleted_rows: i64,
    retention_deleted_rows: i64,
    batches: i64,
    completed: bool,
    has_more: bool,
    channels: Vec<HaOutboxGcChannelReport>,
    wal_checkpoint_busy: bool,
    wal_checkpoint_log_frames: i64,
    wal_checkpoint_checkpointed_frames: i64,
    active_elapsed_ms: u128,
    max_batch_elapsed_ms: u128,
    elapsed_ms: u128,
    continuation_delay_secs: Option<i64>,
    pass_reports: Vec<HaOutboxGcReport>,
}

impl CliReport {
    fn from_passes(
        run_until_complete: bool,
        repaired_triggers: bool,
        trigger_repair_report: Option<tavily_hikari::HaTriggerRepairReport>,
        reports: Vec<HaOutboxGcReport>,
    ) -> Self {
        let last = reports
            .last()
            .expect("ha outbox cleanup cli always records at least one pass");
        Self {
            repaired_triggers,
            trigger_repair_report,
            run_until_complete,
            passes: reports.len(),
            batch_size: last.batch_size,
            max_batches: last.max_batches,
            deleted_rows: reports.iter().map(|report| report.deleted_rows).sum(),
            invalid_legacy_deleted_rows: reports
                .iter()
                .flat_map(|report| report.channels.iter())
                .map(|channel| channel.invalid_legacy_deleted_rows)
                .sum(),
            retention_deleted_rows: reports
                .iter()
                .flat_map(|report| report.channels.iter())
                .map(|channel| channel.retention_deleted_rows)
                .sum(),
            batches: reports.iter().map(|report| report.batches).sum(),
            completed: last.completed,
            has_more: last.has_more,
            channels: last.channels.clone(),
            wal_checkpoint_busy: last.wal_checkpoint_busy,
            wal_checkpoint_log_frames: last.wal_checkpoint_log_frames,
            wal_checkpoint_checkpointed_frames: last.wal_checkpoint_checkpointed_frames,
            active_elapsed_ms: reports.iter().map(|report| report.active_elapsed_ms).sum(),
            max_batch_elapsed_ms: reports
                .iter()
                .map(|report| report.max_batch_elapsed_ms)
                .max()
                .unwrap_or(0),
            elapsed_ms: reports.iter().map(|report| report.elapsed_ms).sum(),
            continuation_delay_secs: last.continuation_delay_secs,
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
    let aggregate = HaOutboxGcReport {
        batch_size: report.batch_size,
        max_batches: report.max_batches,
        deleted_rows: report.deleted_rows,
        batches: report.batches,
        completed: report.completed,
        has_more: report.has_more,
        channels: report.channels.clone(),
        wal_checkpoint_busy: report.wal_checkpoint_busy,
        wal_checkpoint_log_frames: report.wal_checkpoint_log_frames,
        wal_checkpoint_checkpointed_frames: report.wal_checkpoint_checkpointed_frames,
        active_elapsed_ms: report.active_elapsed_ms,
        max_batch_elapsed_ms: report.max_batch_elapsed_ms,
        elapsed_ms: report.elapsed_ms,
        continuation_delay_secs: report.continuation_delay_secs,
    };
    writeln!(
        writer,
        "ha_outbox_gc: repaired_triggers={} invalid_legacy_deleted_rows={} retention_deleted_rows={} {}",
        report.repaired_triggers,
        report.invalid_legacy_deleted_rows,
        report.retention_deleted_rows,
        format_ha_outbox_gc_report_message(&aggregate, report.passes)
    )?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    if cli.dry_run {
        let report = read_only_preflight(&cli.db_path).await?;
        if cli.json {
            serde_json::to_writer_pretty(io::stdout().lock(), &report)?;
            println!();
        } else {
            for channel in report.channels {
                let gc_progress = channel.gc_progress.as_ref();
                println!(
                    "{}: index_pending={} retention_secs={} oldest_age_secs={:?} high_watermark={} lowest_peer_ack={:?} pending_cleanup={} gc_total_deleted_rows={:?} gc_last_net_rows_delta_estimate={:?} gc_last_progress_at={:?} gc_next_retry_at={:?}",
                    channel.channel,
                    !channel.created_at_index_present,
                    channel.retention_secs,
                    channel.oldest_age_secs,
                    channel.high_watermark,
                    channel.lowest_peer_ack,
                    channel.pending_cleanup,
                    gc_progress.map(|progress| progress.total_deleted_rows),
                    gc_progress.and_then(|progress| progress.last_net_rows_delta_estimate),
                    gc_progress.and_then(|progress| progress.last_progress_at),
                    gc_progress.and_then(|progress| progress.next_retry_at),
                );
            }
        }
        return Ok(());
    }
    let mode = tavily_hikari::HaMode::parse(&cli.ha_mode);
    let options = HaOutboxGcOptions {
        batch_size: cli.batch_size,
        max_batches: cli.max_batches,
        max_runtime_secs: cli.max_runtime_secs,
        inter_batch_sleep_ms: cli.inter_batch_sleep_ms,
    };
    let mut reports = Vec::new();
    let trigger_repair_report = if cli.repair_triggers {
        Some(tavily_hikari::repair_ha_triggers_once(&cli.db_path, mode).await?)
    } else {
        None
    };

    loop {
        let report = run_ha_outbox_gc_once(&cli.db_path, options).await?;
        let completed = report.completed;
        reports.push(report);
        if completed || !cli.run_until_complete {
            break;
        }
    }

    let cli_report = CliReport::from_passes(
        cli.run_until_complete,
        cli.repair_triggers,
        trigger_repair_report,
        reports,
    );
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

    #[tokio::test]
    async fn read_only_preflight_reports_recent_invalid_legacy_rows_as_pending_cleanup() {
        let directory = std::env::temp_dir().join(format!(
            "tavily-hikari-ha-outbox-preflight-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("create preflight test directory");
        let db_path = directory.join("test.db");
        let db_string = db_path.to_string_lossy().to_string();
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{db_string}"))
            .expect("parse sqlite path")
            .create_if_missing(true)
            .disable_statement_logging();
        let pool = SqlitePool::connect_with(options)
            .await
            .expect("open test database");
        for table in ["ha_outbox", "ha_billing_outbox", "ha_runtime_outbox"] {
            sqlx::query(&format!(
                "CREATE TABLE {table} (seq INTEGER PRIMARY KEY, created_at INTEGER NOT NULL, resource TEXT NOT NULL)"
            ))
            .execute(&pool)
            .await
            .expect("create outbox table");
        }
        sqlx::query(
            "CREATE TABLE ha_peer_watermarks (channel TEXT NOT NULL, acked_seq INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create peer watermarks table");
        sqlx::query(
            "INSERT INTO ha_outbox (seq, created_at, resource) VALUES (1, ?, 'scheduled_jobs')",
        )
        .bind(chrono::Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("insert recent invalid legacy event");
        pool.close().await;

        let report = read_only_preflight(&db_string)
            .await
            .expect("run read-only preflight");
        let control = report
            .channels
            .iter()
            .find(|channel| channel.channel == "control")
            .expect("control preflight channel");
        assert!(control.pending_cleanup);

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn cli_report_sums_invalid_and_retention_passes() {
        let report = CliReport::from_passes(
            true,
            true,
            None,
            vec![
                HaOutboxGcReport {
                    batch_size: 10,
                    max_batches: 2,
                    deleted_rows: 5,
                    batches: 1,
                    completed: false,
                    has_more: true,
                    channels: vec![HaOutboxGcChannelReport {
                        channel: tavily_hikari::HaSyncChannel::Control,
                        retention_secs: 72,
                        threshold: 100,
                        invalid_legacy_deleted_rows: 2,
                        retention_deleted_rows: 3,
                        deleted_rows: 5,
                        batches: 1,
                        has_more: true,
                        debt_mode: "offline".to_string(),
                        oldest_deletable_age_secs: None,
                        deleted_rows_per_minute: 0.0,
                        recovery_deadline_at: None,
                        slo_state: "not_applicable".to_string(),
                        slo_state_transition: None,
                        foreground_rps: 0,
                        observed_at: 0,
                    }],
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    active_elapsed_ms: 9,
                    max_batch_elapsed_ms: 5,
                    elapsed_ms: 12,
                    continuation_delay_secs: Some(5),
                },
                HaOutboxGcReport {
                    batch_size: 10,
                    max_batches: 2,
                    deleted_rows: 2,
                    batches: 1,
                    completed: true,
                    has_more: false,
                    channels: vec![HaOutboxGcChannelReport {
                        channel: tavily_hikari::HaSyncChannel::Control,
                        retention_secs: 72,
                        threshold: 100,
                        invalid_legacy_deleted_rows: 1,
                        retention_deleted_rows: 1,
                        deleted_rows: 2,
                        batches: 1,
                        has_more: false,
                        debt_mode: "offline".to_string(),
                        oldest_deletable_age_secs: None,
                        deleted_rows_per_minute: 0.0,
                        recovery_deadline_at: None,
                        slo_state: "not_applicable".to_string(),
                        slo_state_transition: None,
                        foreground_rps: 0,
                        observed_at: 0,
                    }],
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    active_elapsed_ms: 7,
                    max_batch_elapsed_ms: 4,
                    elapsed_ms: 8,
                    continuation_delay_secs: None,
                },
            ],
        );

        assert!(report.repaired_triggers);
        assert_eq!(report.deleted_rows, 7);
        assert_eq!(report.invalid_legacy_deleted_rows, 3);
        assert_eq!(report.retention_deleted_rows, 4);
        assert_eq!(report.batches, 2);
        assert!(report.completed);
        assert_eq!(report.elapsed_ms, 20);
    }
}
