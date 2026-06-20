use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{HaMode, repair_ha_triggers_once};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Repair HA replication triggers for the current three-channel contract"
)]
struct Cli {
    /// SQLite database path to mutate.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// HA mode to reconcile against.
    #[arg(long, env = "HA_MODE", default_value = "active_standby")]
    ha_mode: String,

    /// Emit JSON output. Plain output is retained for interactive use.
    #[arg(long, default_value_t = false)]
    json: bool,
}

fn format_plain(report: &tavily_hikari::HaTriggerRepairReport) -> String {
    let channels = report
        .channels
        .iter()
        .map(|channel| {
            format!(
                "{}:{}:{}:{}",
                channel.channel.as_str(),
                channel.legacy_triggers_dropped,
                channel.current_triggers_dropped,
                channel.triggers_created
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "ha_trigger_repair: mode={} legacy_triggers_dropped={} current_triggers_dropped={} triggers_created={} channels={} elapsed_ms={}",
        report.mode.as_str(),
        report.legacy_triggers_dropped,
        report.current_triggers_dropped,
        report.triggers_created,
        channels,
        report.elapsed_ms
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let mode = HaMode::parse(&cli.ha_mode);
    let report = repair_ha_triggers_once(&cli.db_path, mode).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", format_plain(&report));
    }
    Ok(())
}
