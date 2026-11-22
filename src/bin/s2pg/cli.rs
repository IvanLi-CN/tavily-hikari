//! Command line surface for `s2pg`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about = "SQLite to PostgreSQL migration tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Take a consistent SQLite snapshot and write its manifest.
    Snapshot(SnapshotArgs),
    /// Load a snapshot into PostgreSQL.
    Load(LoadArgs),
    /// Compare a snapshot with the loaded PostgreSQL target and write a JSON report.
    Verify(VerifyArgs),
    /// Render a verification report as a human-readable summary.
    Report(ReportArgs),
}

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// SQLite core database file.
    #[arg(long, value_name = "PATH")]
    pub sqlite: PathBuf,
    /// Destination path for the snapshot database.
    #[arg(long, value_name = "PATH")]
    pub out: PathBuf,
}

#[derive(Debug, Args)]
pub struct LoadArgs {
    /// Snapshot database produced by `snapshot`.
    #[arg(long, value_name = "PATH")]
    pub snapshot: PathBuf,
    /// PostgreSQL connection string.
    #[arg(long, value_name = "URL")]
    pub database_url: String,
    /// Drop the `core` and `observability` schemas before loading.
    #[arg(long, default_value_t = false)]
    pub reset: bool,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// Snapshot database produced by `snapshot`.
    #[arg(long, value_name = "PATH")]
    pub snapshot: PathBuf,
    /// PostgreSQL connection string.
    #[arg(long, value_name = "URL")]
    pub database_url: String,
    /// Destination path for the JSON verification report.
    #[arg(long, value_name = "PATH")]
    pub report: PathBuf,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Verification report JSON produced by `verify`.
    #[arg(long, value_name = "PATH")]
    pub report: PathBuf,
}
