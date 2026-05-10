use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{REQUEST_USER_ID_BACKFILL_BATCH_SIZE, run_request_user_id_backfill};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Backfill request_user_id in historical request logs"
)]
struct Cli {
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    #[arg(long, default_value_t = REQUEST_USER_ID_BACKFILL_BATCH_SIZE)]
    batch_size: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report = run_request_user_id_backfill(&cli.db_path, cli.batch_size).await?;
    serde_json::to_writer_pretty(io::stdout(), &report)?;
    io::stdout().write_all(b"\n")?;
    Ok(())
}
