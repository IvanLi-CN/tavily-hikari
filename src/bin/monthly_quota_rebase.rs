use std::io::{self, Write};

use chrono::Utc;
use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::rebase_current_month_business_quota;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Rebase current UTC month quota counters from charged credits"
)]
struct Cli {
    /// SQLite database path to rewrite.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report = rebase_current_month_business_quota(&cli.db_path, Utc::now()).await?;

    serde_json::to_writer_pretty(io::stdout(), &report)?;
    io::stdout().write_all(b"\n")?;

    Ok(())
}
