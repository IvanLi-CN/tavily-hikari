//! Column type mapping and strict value handling shared by `load` and `verify`.
//!
//! The mapping is affinity preserving (SPEC REQ-PG-006): SQLite INTEGER/INT/BOOLEAN become
//! `bigint`, REAL becomes `double precision`, TEXT becomes `text`, BLOB becomes `bytea` and
//! NUMERIC becomes `numeric`. Time columns keep integer epoch values. Anything that cannot be
//! represented exactly by the target column fails instead of being coerced silently.

use crate::decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Integer,
    Double,
    Text,
    Bytes,
    Numeric,
}

impl ColumnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Double => "double",
            Self::Text => "text",
            Self::Bytes => "bytes",
            Self::Numeric => "numeric",
        }
    }

    pub fn pg_type_name(self) -> &'static str {
        match self {
            Self::Integer => "bigint",
            Self::Double => "double precision",
            Self::Text => "text",
            Self::Bytes => "bytea",
            Self::Numeric => "numeric",
        }
    }

    /// Maps a SQLite declared column type to the PG column kind it must land in.
    pub fn from_sqlite_declared(declared: &str) -> Result<Self, String> {
        let base = declared
            .trim()
            .split(|ch: char| ch == '(' || ch.is_whitespace())
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        match base.as_str() {
            "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "BOOLEAN" => Ok(Self::Integer),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(Self::Double),
            "TEXT" | "VARCHAR" | "CHAR" | "CLOB" => Ok(Self::Text),
            "BLOB" => Ok(Self::Bytes),
            "NUMERIC" | "DECIMAL" => Ok(Self::Numeric),
            "" => Err(format!(
                "SQLite column has no declared type (declared {declared:?})"
            )),
            other => Err(format!(
                "SQLite declared type {other:?} has no affinity-preserving PostgreSQL mapping"
            )),
        }
    }

    /// Maps a `information_schema.columns.data_type` value to the kind this tool accepts.
    ///
    /// Only types that cannot silently narrow or pad a migrated value are accepted: `real`,
    /// `character varying(n)` and `character(n)` are rejected even though they are nominally
    /// numeric or textual, because copying into them rounds or truncates values.
    pub fn from_pg_data_type(data_type: &str) -> Result<Self, String> {
        match data_type {
            "bigint" | "integer" => Ok(Self::Integer),
            "double precision" => Ok(Self::Double),
            "text" => Ok(Self::Text),
            "bytea" => Ok(Self::Bytes),
            "numeric" => Ok(Self::Numeric),
            other => Err(format!(
                "PostgreSQL data type {other:?} has no affinity-preserving SQLite mapping"
            )),
        }
    }
}

/// A runtime value read from either database, keeping the storage class instead of coercing.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl SqlValue {
    pub fn runtime_kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Integer(_) => "integer",
            Self::Real(_) => "real",
            Self::Text(_) => "text",
            Self::Bytes(_) => "blob",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Renders the value for diagnostics (primary key reporting and report details).
    pub fn render_debug(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Real(value) => format_float(*value),
            Self::Text(value) => format!("{value:?}"),
            Self::Bytes(value) => {
                let mut rendered = String::with_capacity(3 + value.len() * 2);
                rendered.push_str("x'");
                for byte in value {
                    rendered.push_str(&format!("{byte:02x}"));
                }
                rendered.push('\'');
                rendered
            }
        }
    }
}

/// Renders a COPY text field for `value`, failing when the storage class contradicts `kind`.
pub fn copy_field(kind: ColumnKind, value: &SqlValue) -> Result<String, String> {
    match (kind, value) {
        (_, SqlValue::Null) => Ok("\\N".to_string()),
        (ColumnKind::Integer, SqlValue::Integer(raw)) => Ok(raw.to_string()),
        (ColumnKind::Double, SqlValue::Real(raw)) => Ok(format_float(*raw)),
        (ColumnKind::Double, SqlValue::Integer(raw)) => {
            // An integer is exact in a double column only while it fits the 53-bit mantissa.
            if raw.unsigned_abs() <= (1u64 << 53) {
                Ok(raw.to_string())
            } else {
                Err(format!(
                    "integer {raw} cannot be represented exactly as double precision"
                ))
            }
        }
        (ColumnKind::Text, SqlValue::Text(raw)) => escape_copy_text(raw),
        (ColumnKind::Bytes, SqlValue::Bytes(raw)) => Ok(render_bytea(raw)),
        (ColumnKind::Numeric, SqlValue::Integer(raw)) => Ok(raw.to_string()),
        (ColumnKind::Numeric, SqlValue::Real(raw)) => Decimal::from_f64(*raw).map(|v| v.render()),
        (ColumnKind::Numeric, SqlValue::Text(raw)) => {
            Decimal::parse(raw).map(|value| value.render())
        }
        (kind, value) => Err(format!(
            "target column maps to {} but the SQLite value is {}",
            kind.pg_type_name(),
            value.runtime_kind()
        )),
    }
}

/// Renders a finite or infinite f64 the way PostgreSQL accepts it.
pub fn format_float(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else {
        format!("{value:?}")
    }
}

fn escape_copy_text(value: &str) -> Result<String, String> {
    if value.contains('\0') {
        return Err("text value contains NUL, which PostgreSQL text cannot store".to_string());
    }
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000c}' => escaped.push_str("\\f"),
            '\u{000b}' => escaped.push_str("\\v"),
            other => escaped.push(other),
        }
    }
    Ok(escaped)
}

/// COPY text format for `bytea` is the hex input form with the leading backslash escaped.
fn render_bytea(value: &[u8]) -> String {
    let mut rendered = String::with_capacity(4 + value.len() * 2);
    rendered.push_str("\\\\x");
    for byte in value {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::{ColumnKind, SqlValue, copy_field};

    #[test]
    fn maps_declared_sqlite_types() {
        let cases = [
            ("INTEGER", ColumnKind::Integer),
            ("INT", ColumnKind::Integer),
            ("BOOLEAN", ColumnKind::Integer),
            ("REAL", ColumnKind::Double),
            ("DOUBLE", ColumnKind::Double),
            ("TEXT", ColumnKind::Text),
            ("VARCHAR(32)", ColumnKind::Text),
            ("BLOB", ColumnKind::Bytes),
            ("NUMERIC", ColumnKind::Numeric),
            ("DECIMAL(10,2)", ColumnKind::Numeric),
        ];
        for (declared, expected) in cases {
            assert_eq!(
                ColumnKind::from_sqlite_declared(declared).unwrap(),
                expected,
                "{declared}"
            );
        }
        assert!(ColumnKind::from_sqlite_declared("DATETIME").is_err());
        assert!(ColumnKind::from_sqlite_declared("").is_err());
    }

    #[test]
    fn maps_postgres_data_types() {
        assert_eq!(
            ColumnKind::from_pg_data_type("bigint").unwrap(),
            ColumnKind::Integer
        );
        assert_eq!(
            ColumnKind::from_pg_data_type("double precision").unwrap(),
            ColumnKind::Double
        );
        assert_eq!(
            ColumnKind::from_pg_data_type("bytea").unwrap(),
            ColumnKind::Bytes
        );
        assert!(ColumnKind::from_pg_data_type("jsonb").is_err());
        assert!(ColumnKind::from_pg_data_type("real").is_err());
        assert!(ColumnKind::from_pg_data_type("character varying").is_err());
        assert!(ColumnKind::from_pg_data_type("character").is_err());
    }

    #[test]
    fn rejects_values_that_would_be_coerced() {
        let text = SqlValue::Text("42".to_string());
        assert!(copy_field(ColumnKind::Integer, &text).is_err());
        let real = SqlValue::Real(1.5);
        assert!(copy_field(ColumnKind::Integer, &real).is_err());
        let bytes = SqlValue::Bytes(vec![1, 2]);
        assert!(copy_field(ColumnKind::Text, &bytes).is_err());
        let too_large = SqlValue::Integer(9_007_199_254_740_993);
        assert!(copy_field(ColumnKind::Double, &too_large).is_err());
    }

    #[test]
    fn renders_copy_fields() {
        assert_eq!(
            copy_field(ColumnKind::Integer, &SqlValue::Null).unwrap(),
            "\\N"
        );
        assert_eq!(
            copy_field(ColumnKind::Text, &SqlValue::Text("a\tb\\c".to_string())).unwrap(),
            "a\\tb\\\\c"
        );
        assert_eq!(
            copy_field(ColumnKind::Bytes, &SqlValue::Bytes(vec![0x00, 0xff])).unwrap(),
            "\\\\x00ff"
        );
        assert_eq!(
            copy_field(ColumnKind::Double, &SqlValue::Real(1.5)).unwrap(),
            "1.5"
        );
        assert_eq!(
            copy_field(ColumnKind::Double, &SqlValue::Real(f64::INFINITY)).unwrap(),
            "Infinity"
        );
    }
}
