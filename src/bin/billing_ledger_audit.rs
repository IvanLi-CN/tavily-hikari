use std::{
    io::{self, Write},
    process,
};

use chrono::Utc;
use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::audit_business_quota_ledger;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Audit business quota ledger parity for the current UTC windows"
)]
struct Cli {
    /// SQLite database path to inspect.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report = audit_business_quota_ledger(&cli.db_path, Utc::now()).await?;

    serde_json::to_writer_pretty(io::stdout(), &report)?;
    io::stdout().write_all(b"\n")?;

    if report.has_mismatches() {
        process::exit(1);
    }

    Ok(())
}
