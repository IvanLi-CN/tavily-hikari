use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use tavily_hikari::{
    HaOutboxGcChannelReport, HaOutboxGcOptions, HaOutboxGcReport,
    format_ha_outbox_gc_report_message, run_ha_outbox_gc_once,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run bounded HA control outbox GC once, or repeatedly until complete"
)]
struct Cli {
    /// SQLite database path to mutate.
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
    elapsed_ms: u128,
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
        elapsed_ms: report.elapsed_ms,
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
                    }],
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    elapsed_ms: 12,
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
                    }],
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    elapsed_ms: 8,
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
