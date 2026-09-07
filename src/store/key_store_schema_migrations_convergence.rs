const ADMIN_ALERT_CANONICAL_GROUPS_VERSION: i64 = 31;
const ADMIN_ALERT_CANONICAL_GROUPS_NAME: &str = "admin-alert-canonical-groups-v1";
const ADMIN_ALERT_CANONICAL_GROUPS_CHECKSUM: &str =
    "sha256:15dc6c4d56ff4d14a71c1af66f086757a1bc0c97c42b1e69e970f0f03c1e4afe";
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION: i64 = 32;
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_NAME: &str =
    "reconciliation-key-observation-source-identity-v1";
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_CHECKSUM: &str =
    "sha256:298f687879854438c25978d82fd424baa0d0da16b2e0e5c9cfefc99d1297e493";

fn convergence_schema_migration_records() -> [(i64, &'static str, &'static str); 2] {
    [
        (
            ADMIN_ALERT_CANONICAL_GROUPS_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_CHECKSUM,
        ),
        (
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION,
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_NAME,
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_CHECKSUM,
        ),
    ]
}

impl KeyStore {
    async fn validate_convergence_schema_migration_objects(&self) -> Result<(), ProxyError> {
        if self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_VERSION)
            .await?
            && (!self
                .schema_object_exists("observability", "admin_alert_canonical_groups")
                .await?
                || !self
                    .schema_object_exists("observability", "admin_alert_canonical_groups_state")
                    .await?
                || !self
                    .schema_named_object_exists(
                        "observability",
                        "index",
                        "idx_admin_alert_canonical_groups_page",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 31".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION)
            .await?
            && (!self
                .table_column_exists(
                    "upstream_reconciliation_key_observations",
                    "candidate_identity",
                )
                .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_key_observations",
                        "key_set_identity",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_key_observations",
                        "key_source_identity",
                    )
                    .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "index",
                        "idx_reconciliation_key_observations_source_identity",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 32".to_string(),
            ));
        }
        Ok(())
    }

    async fn apply_admin_alert_canonical_groups_migration(&self) -> Result<(), ProxyError> {
        // This local read model is derived solely from the complete alert projection.
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_groups_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                active_generation INTEGER NOT NULL DEFAULT 0,
                source_recent_generation INTEGER NOT NULL DEFAULT -1,
                source_history_generation INTEGER NOT NULL DEFAULT -1
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_groups (
                build_generation INTEGER NOT NULL,
                position INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                total_count INTEGER NOT NULL,
                alert_type TEXT NOT NULL,
                group_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY(build_generation, position)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_groups_page \
             ON admin_alert_canonical_groups(\
                 build_generation, last_seen DESC, total_count DESC, alert_type DESC, group_id DESC\
             )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO observability.admin_alert_canonical_groups_state(singleton) \
             VALUES (1) ON CONFLICT(singleton) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_GROUPS_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_key_observation_source_identity_migration(
        &self,
    ) -> Result<(), ProxyError> {
        for (column, definition) in [
            (
                "candidate_identity",
                "ALTER TABLE upstream_reconciliation_key_observations \
                 ADD COLUMN candidate_identity TEXT NOT NULL DEFAULT ''",
            ),
            (
                "key_set_identity",
                "ALTER TABLE upstream_reconciliation_key_observations \
                 ADD COLUMN key_set_identity TEXT NOT NULL DEFAULT ''",
            ),
            (
                "key_source_identity",
                "ALTER TABLE upstream_reconciliation_key_observations \
                 ADD COLUMN key_source_identity TEXT NOT NULL DEFAULT ''",
            ),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_key_observations", column)
                .await?
            {
                sqlx::query(definition).execute(&self.pool).await?;
            }
        }
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_reconciliation_key_observations_source_identity \
             ON upstream_reconciliation_key_observations(\
                 token_id, period_code, candidate_identity, key_set_identity, key_id, \
                 key_source_identity, observed_at DESC\
             )",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION,
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_NAME,
            RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_CHECKSUM,
        )
        .await
    }
}
