use std::{
    io::{self, Write},
    process,
};

use chrono::Utc;
use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{BillingLedgerAuditReport, audit_business_quota_ledger};

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

fn write_report(mut writer: impl Write, report: &BillingLedgerAuditReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let report = audit_business_quota_ledger(&cli.db_path, Utc::now()).await?;

    {
        let mut stdout = io::stdout().lock();
        write_report(&mut stdout, &report)?;
    }

    if report.has_mismatches() {
        process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TrackingWriter {
        bytes: Vec<u8>,
        flushed: bool,
    }

    impl Write for TrackingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            Ok(())
        }
    }

    #[test]
    fn write_report_flushes_output() {
        let report = BillingLedgerAuditReport::default();
        let mut writer = TrackingWriter::default();

        write_report(&mut writer, &report).expect("write report");

        assert!(writer.flushed);
        assert!(
            String::from_utf8(writer.bytes)
                .expect("utf8")
                .ends_with('\n')
        );
    }
}
