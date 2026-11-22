//! Baseline schema gate for the PostgreSQL store.
//!
//! The gate is a cheap startup assertion, not a full schema diff: it proves that the
//! `core` and `observability` schemas carry the tables the server cannot run without.

use sqlx::PgPool;
use tracing::info;

/// A table the server cannot start without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredRelation {
    pub schema: &'static str,
    pub table: &'static str,
}

impl RequiredRelation {
    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.schema, self.table)
    }
}

/// Baseline relations asserted at startup. Keep this list in sync with
/// `src/store/pg/migrations/0001_baseline.sql`; both schemas must be represented.
pub const REQUIRED_RELATIONS: &[RequiredRelation] = &[
    RequiredRelation {
        schema: "core",
        table: "api_keys",
    },
    RequiredRelation {
        schema: "core",
        table: "billing_ledger",
    },
    RequiredRelation {
        schema: "observability",
        table: "request_logs",
    },
    RequiredRelation {
        schema: "observability",
        table: "dashboard_rollup_daily_seals",
    },
];

pub fn required_relation_names() -> Vec<String> {
    REQUIRED_RELATIONS
        .iter()
        .map(RequiredRelation::qualified_name)
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum PgSchemaError {
    #[error(
        "postgresql schema is incomplete: missing {missing_count} required relation(s): \
         {missing}. Start with migrations enabled (omit --no-auto-migrate) or apply \
         src/store/pg/migrations to the target database first"
    )]
    MissingRelations {
        missing_count: usize,
        missing: String,
    },
    #[error("postgresql schema probe failed: {0}")]
    Probe(#[source] sqlx::Error),
}

impl PgSchemaError {
    /// Builds the missing-relation error from the probe result.
    pub fn from_missing(missing: Vec<String>) -> Self {
        Self::MissingRelations {
            missing_count: missing.len(),
            missing: missing.join(", "),
        }
    }
}

/// Returns the required relations absent from the target database, ordered by name.
pub async fn missing_required_relations(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let missing: Vec<String> = sqlx::query_scalar(
        "SELECT required.rel FROM unnest($1::text[]) AS required(rel) \
         WHERE to_regclass(required.rel) IS NULL ORDER BY required.rel",
    )
    .bind(required_relation_names())
    .fetch_all(pool)
    .await?;
    Ok(missing)
}

pub async fn assert_schema_present(pool: &PgPool) -> Result<(), PgSchemaError> {
    let missing = missing_required_relations(pool)
        .await
        .map_err(PgSchemaError::Probe)?;
    if !missing.is_empty() {
        return Err(PgSchemaError::from_missing(missing));
    }
    info!(
        component = "startup",
        event = "pg_schema_gate_passed",
        required_relations = REQUIRED_RELATIONS.len(),
        "postgresql baseline schema present"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PgSchemaError, REQUIRED_RELATIONS, RequiredRelation, required_relation_names};
    use std::collections::BTreeSet;

    #[test]
    fn required_relations_cover_both_baseline_schemas() {
        let names = required_relation_names();
        let schemas: BTreeSet<&str> = REQUIRED_RELATIONS
            .iter()
            .map(|relation| relation.schema)
            .collect();
        assert_eq!(
            schemas,
            BTreeSet::from(["core", "observability"]),
            "both schemas must be asserted"
        );
        for expected in [
            "core.api_keys",
            "core.billing_ledger",
            "observability.request_logs",
            "observability.dashboard_rollup_daily_seals",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing assert for {expected}: {names:?}"
            );
        }
    }

    #[test]
    fn qualified_name_joins_schema_and_table() {
        assert_eq!(
            RequiredRelation {
                schema: "core",
                table: "api_keys",
            }
            .qualified_name(),
            "core.api_keys"
        );
    }

    #[test]
    fn missing_relation_error_names_every_missing_table() {
        let err = PgSchemaError::from_missing(vec![
            "observability.dashboard_rollup_daily_seals".to_owned(),
            "observability.request_logs".to_owned(),
        ]);
        let message = err.to_string();
        assert!(
            message.contains("missing 2 required relation(s)"),
            "{message}"
        );
        assert!(message.contains("observability.request_logs"), "{message}");
        assert!(
            message.contains("observability.dashboard_rollup_daily_seals"),
            "{message}"
        );
        assert!(message.contains("--no-auto-migrate"), "{message}");
    }

    #[test]
    fn missing_relation_error_counts_a_single_table() {
        let message = PgSchemaError::from_missing(vec!["core.api_keys".to_owned()]).to_string();
        assert!(
            message.contains("missing 1 required relation(s)"),
            "{message}"
        );
        assert!(message.contains("core.api_keys"), "{message}");
    }
}
