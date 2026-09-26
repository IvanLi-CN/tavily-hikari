//! Provider-aware schema migration v45.
//!
//! Extracted from the key_store_schema_migrations include so the migration
//! ledger file stays within the source-line budget. The ledger tuple entry,
//! migration registration flow, and migration_count logging remain in
//! key_store_schema_migrations.rs; this module owns the v45 constants, the
//! guarded apply path, and the v45 object validation. The checksum constant
//! is part of the recorded ledger contract and must stay byte-identical.

use super::{KeyStore, ProxyError};

pub(super) const PROVIDER_AWARE_SCHEMA_VERSION: i64 = 45;
pub(super) const PROVIDER_AWARE_SCHEMA_NAME: &str = "provider-aware-schema-v1";
pub(super) const PROVIDER_AWARE_SCHEMA_CHECKSUM: &str =
    "sha256:77201d3952329ffb3e54be6c2a1bdb08d595e3ecbc3e5279a6fb6f4895f4bd0f";

pub(super) fn provider_aware_schema_migration_record() -> (i64, &'static str, &'static str) {
    (
        PROVIDER_AWARE_SCHEMA_VERSION,
        PROVIDER_AWARE_SCHEMA_NAME,
        PROVIDER_AWARE_SCHEMA_CHECKSUM,
    )
}

impl KeyStore {
    pub(super) async fn ensure_provider_aware_schema_migration(&self) -> Result<(), ProxyError> {
        if !self
            .schema_migration_applied(PROVIDER_AWARE_SCHEMA_VERSION)
            .await?
        {
            self.apply_provider_aware_schema_migration().await?;
        }
        Ok(())
    }

    pub(super) async fn apply_provider_aware_schema_migration(&self) -> Result<(), ProxyError> {
        if !self.table_column_exists("api_keys", "provider").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN provider TEXT NOT NULL DEFAULT 'tavily'")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_column_exists("api_keys", "quota_renews_at")
            .await?
        {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_renews_at INTEGER")
                .execute(&self.pool)
                .await?;
        }
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_api_keys_provider_status \
             ON api_keys(provider, status, deleted_at, last_used_at)",
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("auth_token_logs", "provider")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN provider TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.table_column_exists("mcp_sessions", "provider").await? {
            sqlx::query("ALTER TABLE mcp_sessions ADD COLUMN provider TEXT")
                .execute(&self.pool)
                .await?;
        }
        self.record_schema_migration(
            PROVIDER_AWARE_SCHEMA_VERSION,
            PROVIDER_AWARE_SCHEMA_NAME,
            PROVIDER_AWARE_SCHEMA_CHECKSUM,
        )
        .await
    }

    pub(super) async fn validate_provider_aware_schema_objects(&self) -> Result<(), ProxyError> {
        if self
            .schema_migration_applied(PROVIDER_AWARE_SCHEMA_VERSION)
            .await?
            && (!self.table_column_exists("api_keys", "provider").await?
                || !self
                    .table_column_exists("api_keys", "quota_renews_at")
                    .await?
                || !self
                    .table_column_exists("auth_token_logs", "provider")
                    .await?
                || !self.table_column_exists("mcp_sessions", "provider").await?
                || !self
                    .schema_named_object_exists("main", "index", "idx_api_keys_provider_status")
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 45".to_string(),
            ));
        }
        Ok(())
    }
}
