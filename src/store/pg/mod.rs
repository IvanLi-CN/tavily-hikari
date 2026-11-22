//! PostgreSQL store plumbing for the SQLite-to-PostgreSQL migration.
//!
//! Scope of this module: connection settings, the embedded baseline migration, and the
//! startup schema gate. No business storage lives here yet — the runtime keeps serving
//! from SQLite until the storage-migration waves port each module.

mod config;
mod runtime;
mod schema;

pub use config::{PgConfigError, PgStartupPlan};
pub use runtime::PgStore;
