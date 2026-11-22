//! Verification report model, JSON serialization and the human-readable `report` rendering.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::aggregate::{AggregateView, FLOAT_RELATIVE_TOLERANCE};
use crate::error::{Error, Result};
use crate::types::ColumnKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Ok,
    Mismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub snapshot_id: String,
    pub snapshot_path: String,
    pub tool_version: String,
    pub generated_at: String,
    pub overall: Verdict,
    pub float_tolerance: f64,
    pub summary: ReportSummary,
    pub tables: Vec<TableReport>,
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub tables: u64,
    pub rows_sqlite: u64,
    pub rows_postgres: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableReport {
    pub schema: String,
    pub table: String,
    pub rows: RowReport,
    pub primary_key: PrimaryKeyReport,
    pub columns: Vec<ColumnReport>,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowReport {
    pub sqlite: u64,
    pub postgres: u64,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryKeyReport {
    pub columns: Vec<String>,
    pub sqlite: String,
    pub postgres: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnReport {
    pub column: String,
    pub kind: ColumnKind,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Present only for `double precision` columns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    pub sqlite: AggregateView,
    pub postgres: AggregateView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub schema: String,
    pub table: String,
    /// `rows`, `primaryKey`, `column` or `table`.
    pub check: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    pub detail: String,
}

impl VerificationReport {
    pub fn new(snapshot_id: String, snapshot_path: String) -> Self {
        Self {
            snapshot_id,
            snapshot_path,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            overall: Verdict::Ok,
            float_tolerance: FLOAT_RELATIVE_TOLERANCE,
            summary: ReportSummary {
                tables: 0,
                rows_sqlite: 0,
                rows_postgres: 0,
            },
            tables: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub fn passed(&self) -> bool {
        self.overall == Verdict::Ok
    }

    pub fn finalize(&mut self) {
        self.summary.tables = self.tables.len() as u64;
        self.summary.rows_sqlite = self.tables.iter().map(|table| table.rows.sqlite).sum();
        self.summary.rows_postgres = self.tables.iter().map(|table| table.rows.postgres).sum();
        self.overall = if self.failures.is_empty() {
            Verdict::Ok
        } else {
            Verdict::Mismatch
        };
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, format!("{text}\n"))
            .map_err(|err| Error::msg(format!("write report {}: {err}", path.display())))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| Error::msg(format!("read report {}: {err}", path.display())))?;
        serde_json::from_str(&text).map_err(Error::from)
    }
}

/// Renders the machine report as a compact human summary.
pub fn render_human(report: &VerificationReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "s2pg verification report (tool {})",
        report.tool_version
    ));
    lines.push(format!("snapshot: {}", report.snapshot_id));
    lines.push(format!("path: {}", report.snapshot_path));
    lines.push(format!("generated: {}", report.generated_at));
    lines.push(format!(
        "overall: {}",
        match report.overall {
            Verdict::Ok => "OK",
            Verdict::Mismatch => "FAILED",
        }
    ));
    lines.push(format!(
        "tables: {} (rows sqlite={}, postgres={})",
        report.summary.tables, report.summary.rows_sqlite, report.summary.rows_postgres
    ));
    lines.push(format!(
        "double precision tolerance: relative {}",
        report.float_tolerance
    ));
    lines.push(format!("failures: {}", report.failures.len()));
    for failure in &report.failures {
        let column = failure
            .column
            .as_deref()
            .map(|column| format!(" column={column}"))
            .unwrap_or_default();
        lines.push(format!(
            "  - {}.{} check={}{column}: {}",
            failure.schema, failure.table, failure.check, failure.detail
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        ColumnReport, Failure, PrimaryKeyReport, ReportSummary, RowReport, TableReport, Verdict,
        VerificationReport, render_human,
    };
    use crate::aggregate::AggregateView;
    use crate::types::ColumnKind;

    fn empty_view() -> AggregateView {
        AggregateView {
            count: 0,
            nulls: 0,
            sum: None,
            min: None,
            max: None,
            min_length: None,
            max_length: None,
            total_length: None,
            value_digest: None,
        }
    }

    #[test]
    fn renders_failures_and_totals() {
        let mut report = VerificationReport::new("abc123".to_string(), "/tmp/s.db".to_string());
        report.tables.push(TableReport {
            schema: "core".to_string(),
            table: "api_keys".to_string(),
            rows: RowReport {
                sqlite: 5,
                postgres: 4,
                ok: false,
            },
            primary_key: PrimaryKeyReport {
                columns: vec!["id".to_string()],
                sqlite: "a".to_string(),
                postgres: "b".to_string(),
                ok: false,
            },
            columns: vec![ColumnReport {
                column: "weight".to_string(),
                kind: ColumnKind::Double,
                ok: true,
                detail: None,
                tolerance: Some(1e-12),
                sqlite: empty_view(),
                postgres: empty_view(),
            }],
            ok: false,
        });
        report.failures.push(Failure {
            schema: "core".to_string(),
            table: "api_keys".to_string(),
            check: "rows".to_string(),
            column: None,
            detail: "sqlite=5 postgres=4".to_string(),
        });
        report.finalize();

        assert_eq!(report.overall, Verdict::Mismatch);
        assert!(!report.passed());
        assert_eq!(report.summary.tables, 1);
        assert_eq!(report.summary.rows_sqlite, 5);
        let rendered = render_human(&report);
        assert!(rendered.contains("overall: FAILED"));
        assert!(rendered.contains("tables: 1 (rows sqlite=5, postgres=4)"));
        assert!(rendered.contains("core.api_keys check=rows: sqlite=5 postgres=4"));
    }

    #[test]
    fn empty_report_is_ok() {
        let mut report = VerificationReport::new("id".to_string(), "/tmp/s.db".to_string());
        report.finalize();
        assert!(report.passed());
        assert!(render_human(&report).contains("failures: 0"));
        assert_eq!(
            report.summary,
            ReportSummary {
                tables: 0,
                rows_sqlite: 0,
                rows_postgres: 0,
            }
        );
    }
}
