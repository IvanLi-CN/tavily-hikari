use std::io::{self, Write};

use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{
    REQUEST_KIND_CANONICAL_BACKFILL_BATCH_SIZE, run_request_kind_canonical_backfill,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Canonicalize request_kind fields in historical logs"
)]
struct Cli {
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    #[arg(long, default_value_t = REQUEST_KIND_CANONICAL_BACKFILL_BATCH_SIZE)]
    batch_size: i64,

    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report =
        run_request_kind_canonical_backfill(&cli.db_path, cli.batch_size, cli.dry_run).await?;
    serde_json::to_writer_pretty(io::stdout(), &report)?;
    io::stdout().write_all(b"\n")?;
    Ok(())
}
