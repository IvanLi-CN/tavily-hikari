//! `s2pg` moves one SQLite snapshot into PostgreSQL and proves the copy stayed faithful.
//!
//! ```text
//! s2pg snapshot --sqlite <path> --out <path>
//! s2pg load     --snapshot <path> --database-url <url> [--reset]
//! s2pg verify   --snapshot <path> --database-url <url> --report <path>
//! s2pg report   --report <path>
//! ```
//!
//! The tool is offline and one-way: it never writes to the SQLite source, never needs the
//! running server, and refuses to coerce values that do not match the target column kind.

#[path = "s2pg/aggregate.rs"]
mod aggregate;
#[path = "s2pg/catalog.rs"]
mod catalog;
#[path = "s2pg/cli.rs"]
mod cli;
#[path = "s2pg/decimal.rs"]
mod decimal;
#[path = "s2pg/error.rs"]
mod error;
#[path = "s2pg/hash.rs"]
mod hash;
#[path = "s2pg/load.rs"]
mod load;
#[path = "s2pg/manifest.rs"]
mod manifest;
#[path = "s2pg/report.rs"]
mod report;
#[path = "s2pg/snapshot.rs"]
mod snapshot;
#[path = "s2pg/sqlite.rs"]
mod sqlite;
#[path = "s2pg/types.rs"]
mod types;
#[path = "s2pg/verify.rs"]
mod verify;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::Result;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli.command).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("s2pg: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(command: Command) -> Result<bool> {
    match command {
        Command::Snapshot(args) => {
            let summary = snapshot::run(&args).await?;
            let manifest = &summary.manifest;
            println!(
                "snapshot: path={} sha256={} bytes={} tables={} rows={} userVersion={} manifest={}",
                manifest.path,
                manifest.snapshot_id,
                manifest.bytes,
                manifest.tables.len(),
                manifest.total_rows,
                manifest.source.user_version,
                summary.manifest_path.display()
            );
            Ok(true)
        }
        Command::Load(args) => {
            let summary = load::run(&args).await?;
            println!(
                "load: tables={} rows={} sequences={}",
                summary.tables, summary.rows, summary.sequences
            );
            Ok(true)
        }
        Command::Verify(args) => {
            let verification = verify::run(&args).await?;
            println!("{}", report::render_human(&verification));
            println!("report: {}", args.report.display());
            Ok(verification.passed())
        }
        Command::Report(args) => {
            let verification = report::VerificationReport::read(&args.report)?;
            println!("{}", report::render_human(&verification));
            Ok(true)
        }
    }
}
