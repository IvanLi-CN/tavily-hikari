const ADMIN_ALERT_CANONICAL_GROUPS_VERSION: i64 = 31;
const ADMIN_ALERT_CANONICAL_GROUPS_NAME: &str = "admin-alert-canonical-groups-v1";
const ADMIN_ALERT_CANONICAL_GROUPS_CHECKSUM: &str =
    "sha256:15dc6c4d56ff4d14a71c1af66f086757a1bc0c97c42b1e69e970f0f03c1e4afe";
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION: i64 = 32;
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_NAME: &str =
    "reconciliation-key-observation-source-identity-v1";
const RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_CHECKSUM: &str =
    "sha256:298f687879854438c25978d82fd424baa0d0da16b2e0e5c9cfefc99d1297e493";
const ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_VERSION: i64 = 33;
const ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_NAME: &str =
    "admin-alert-canonical-groups-slot-state-v1";
const ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_CHECKSUM: &str =
    "sha256:cb1bb8cc4d9d50b1812430082bff5dfb4d49b18d9a669d7f10e91388c782cff5";
const ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_VERSION: i64 = 34;
const ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_NAME: &str =
    "admin-alert-canonical-groups-snapshot-v1";
const ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_CHECKSUM: &str =
    "sha256:1619f39f9263aeb4bd42223d75a74a8137c0041e8cc51155d96792fa12bea9ce";
const ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_VERSION: i64 = 35;
const ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_NAME: &str =
    "admin-alert-canonical-bounded-build-v1";
const ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_CHECKSUM: &str =
    "sha256:3f421f1ca15a91c4e1731c8331cd330dc12b38aab08772e3b488a74bef679b01";
const ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_VERSION: i64 = 36;
const ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_NAME: &str =
    "admin-alert-canonical-groups-finalize-cursor-v1";
const ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_CHECKSUM: &str =
    "sha256:ec4d03331712ed690a6aab3d9f20b3e4b82e581d656ff132fb7f959535314f94";
const ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_VERSION: i64 = 37;
const ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_NAME: &str = "admin-alert-canonical-payload-resume-v1";
const ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_CHECKSUM: &str =
    "sha256:50e585f7f8a26b5ea2a7a795a4f81f272d8a34e2bb21c4dce3cb17a66c8395c1";

fn convergence_schema_migration_records() -> [(i64, &'static str, &'static str); 7] {
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
        (
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_CHECKSUM,
        ),
        (
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_CHECKSUM,
        ),
        (
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_VERSION,
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_NAME,
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_CHECKSUM,
        ),
        (
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_CHECKSUM,
        ),
        (
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_VERSION,
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_NAME,
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_CHECKSUM,
        ),
    ]
}

impl KeyStore {
    async fn apply_pending_convergence_schema_migrations(&self) -> Result<(), ProxyError> {
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_groups_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_KEY_OBSERVATION_SOURCE_IDENTITY_VERSION)
            .await?
        {
            self.apply_reconciliation_key_observation_source_identity_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_groups_slot_state_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_groups_snapshot_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_bounded_build_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_groups_finalize_cursor_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_VERSION)
            .await?
        {
            self.apply_admin_alert_canonical_payload_resume_migration()
                .await?;
        }
        Ok(())
    }

    async fn apply_all_convergence_schema_migrations(&self) -> Result<(), ProxyError> {
        self.apply_admin_alert_canonical_groups_migration().await?;
        self.apply_reconciliation_key_observation_source_identity_migration()
            .await?;
        self.apply_admin_alert_canonical_groups_slot_state_migration()
            .await?;
        self.apply_admin_alert_canonical_groups_snapshot_migration()
            .await?;
        self.apply_admin_alert_canonical_bounded_build_migration().await?;
        self.apply_admin_alert_canonical_groups_finalize_cursor_migration()
            .await?;
        self.apply_admin_alert_canonical_payload_resume_migration()
            .await
    }

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
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_VERSION)
            .await?
            && !self
                .table_column_exists("admin_alert_canonical_groups_state", "active_row_count")
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 33".to_string(),
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
        if self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_VERSION)
            .await?
            && (!self
                .schema_object_exists("observability", "admin_alert_canonical_group_events")
                .await?
                || !self
                    .schema_object_exists("observability", "admin_alert_canonical_group_overrides")
                    .await?
                || !self
                    .schema_object_exists("observability", "dashboard_alert_projection_revision_state")
                    .await?
                || !self
                    .table_column_exists("dashboard_alert_projection_events", "projection_revision")
                    .await?
                || !self
                    .table_column_exists("admin_alert_canonical_groups_state", "build_generation")
                    .await?
                || !self
                    .table_column_exists(
                        "admin_alert_canonical_groups_state",
                        "build_source_rowid_upper_bound",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "admin_alert_canonical_groups_state",
                        "build_partition_events_json",
                    )
                    .await?
                || !self
                    .schema_named_object_exists(
                        "observability",
                        "index",
                        "idx_admin_alert_canonical_group_events_partition_scan",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 34".to_string(),
            ));
        }
        if self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_VERSION)
            .await?
            && (!self
                .schema_object_exists(
                    "observability",
                    "admin_alert_canonical_group_fragments",
                )
                .await?
                || !self
                    .schema_object_exists("observability", "admin_alert_canonical_catalog_state")
                    .await?
                || !self
                    .schema_object_exists("observability", "admin_alert_canonical_catalog_facets")
                    .await?
                || !self
                    .table_column_exists(
                        "admin_alert_canonical_groups_state",
                        "build_partition_source_complete",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "admin_alert_canonical_groups_state",
                        "build_partition_fragment_next_position",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 35".to_string(),
            ));
        }
        if self
            .schema_migration_applied(ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_VERSION)
            .await?
            && (!self
                .table_column_exists(
                    "admin_alert_canonical_groups_state",
                    "build_partition_finalize_fragment_position",
                )
                .await?
                || !self
                    .schema_object_exists(
                        "observability",
                        "admin_alert_canonical_catalog_payloads",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "admin_alert_canonical_catalog_payloads",
                        "payload_status",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 36".to_string(),
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

    async fn apply_admin_alert_canonical_groups_slot_state_migration(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .table_column_exists("admin_alert_canonical_groups_state", "active_row_count")
            .await?
        {
            sqlx::query(
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN active_row_count INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET active_row_count = (
                      SELECT COUNT(*)
                        FROM observability.admin_alert_canonical_groups
                       WHERE build_generation =
                           observability.admin_alert_canonical_groups_state.active_generation
                  )
                WHERE singleton = 1"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_SLOT_STATE_CHECKSUM,
        )
        .await
    }

    async fn apply_admin_alert_canonical_groups_snapshot_migration(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .table_column_exists("dashboard_alert_projection_events", "projection_revision")
            .await?
        {
            sqlx::query(
                "ALTER TABLE observability.dashboard_alert_projection_events \
                 ADD COLUMN projection_revision INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.dashboard_alert_projection_revision_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                revision INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO observability.dashboard_alert_projection_revision_state(singleton) \
             VALUES (1) ON CONFLICT(singleton) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        for (column, definition) in [
            (
                "active_projection_revision",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN active_projection_revision INTEGER NOT NULL DEFAULT -1",
            ),
            (
                "build_generation",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_generation INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "build_projection_revision",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_projection_revision INTEGER NOT NULL DEFAULT -1",
            ),
            (
                "build_source_recent_generation",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_source_recent_generation INTEGER NOT NULL DEFAULT -1",
            ),
            (
                "build_source_history_generation",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_source_history_generation INTEGER NOT NULL DEFAULT -1",
            ),
            (
                "build_cursor_occurred_at",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_cursor_occurred_at INTEGER NOT NULL DEFAULT -9223372036854775808",
            ),
            (
                "build_cursor_row_sort_id",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_cursor_row_sort_id TEXT NOT NULL DEFAULT ''",
            ),
            (
                "build_source_rowid_upper_bound",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_source_rowid_upper_bound INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "build_cursor_source_rowid",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_cursor_source_rowid INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "build_phase",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_phase TEXT NOT NULL DEFAULT 'idle'",
            ),
            (
                "build_partition_key",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_key TEXT NOT NULL DEFAULT ''",
            ),
            (
                "build_partition_after_key",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_after_key TEXT NOT NULL DEFAULT ''",
            ),
            (
                "build_partition_cursor_occurred_at",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_cursor_occurred_at INTEGER NOT NULL DEFAULT -9223372036854775808",
            ),
            (
                "build_partition_cursor_row_sort_id",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_cursor_row_sort_id TEXT NOT NULL DEFAULT ''",
            ),
            (
                "build_partition_events_json",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_events_json TEXT NOT NULL DEFAULT '[]'",
            ),
            (
                "build_next_position",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_next_position INTEGER NOT NULL DEFAULT 1",
            ),
        ] {
            if !self
                .table_column_exists("admin_alert_canonical_groups_state", column)
                .await?
            {
                sqlx::query(definition).execute(&self.pool).await?;
            }
        }
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_group_events (
                build_generation INTEGER NOT NULL,
                source_kind TEXT NOT NULL,
                source_id TEXT NOT NULL,
                occurred_at INTEGER NOT NULL,
                row_sort_id TEXT NOT NULL,
                partition_key TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY(build_generation, source_kind, source_id)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_group_events_time \
             ON admin_alert_canonical_group_events(build_generation, occurred_at, row_sort_id)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_group_events_partition \
             ON admin_alert_canonical_group_events(build_generation, partition_key, occurred_at DESC, row_sort_id DESC)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_group_events_partition_scan \
             ON admin_alert_canonical_group_events(build_generation, partition_key, occurred_at, row_sort_id)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_group_overrides (
                build_generation INTEGER NOT NULL,
                source_kind TEXT NOT NULL,
                source_id TEXT NOT NULL,
                occurred_at INTEGER NOT NULL,
                row_sort_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY(build_generation, source_kind, source_id)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        // v31-v33 stored only serialized Groups rows. Those rows do not have a
        // corresponding immutable event snapshot, so treating their source
        // fence as current would let Catalog/Events and Groups publish mixed
        // generations after this additive upgrade. The model is derived and
        // rebuildable; invalidate it once when v34 is first recorded.
        sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET active_generation = 0, active_row_count = 0,
                      active_projection_revision = -1,
                      source_recent_generation = -1, source_history_generation = -1,
                      build_generation = 0, build_projection_revision = -1,
                      build_source_recent_generation = -1,
                      build_source_history_generation = -1,
                      build_cursor_occurred_at = -9223372036854775808,
                      build_cursor_row_sort_id = '', build_source_rowid_upper_bound = 0,
                      build_cursor_source_rowid = 0, build_phase = 'idle',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]', build_next_position = 1
                WHERE singleton = 1"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_SNAPSHOT_CHECKSUM,
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

    async fn apply_admin_alert_canonical_bounded_build_migration(
        &self,
    ) -> Result<(), ProxyError> {
        for (column, definition) in [
            (
                "build_partition_source_complete",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_source_complete INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "build_partition_fragment_next_position",
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_fragment_next_position INTEGER NOT NULL DEFAULT 1",
            ),
        ] {
            if !self
                .table_column_exists("admin_alert_canonical_groups_state", column)
                .await?
            {
                sqlx::query(definition).execute(&self.pool).await?;
            }
        }
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_group_fragments (
                build_generation INTEGER NOT NULL,
                partition_key TEXT NOT NULL,
                position INTEGER NOT NULL,
                events_json TEXT NOT NULL,
                PRIMARY KEY(build_generation, partition_key, position)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_group_fragments_scan \
             ON admin_alert_canonical_group_fragments(build_generation, partition_key, position)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_catalog_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                build_generation INTEGER NOT NULL DEFAULT 0,
                cursor_occurred_at INTEGER NOT NULL DEFAULT -9223372036854775808,
                cursor_row_sort_id TEXT NOT NULL DEFAULT '',
                source_complete INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO observability.admin_alert_canonical_catalog_state(singleton) \
             VALUES (1) ON CONFLICT(singleton) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_catalog_facets (
                build_generation INTEGER NOT NULL,
                facet_kind TEXT NOT NULL,
                facet_identity TEXT NOT NULL,
                facet_value TEXT NOT NULL,
                facet_label TEXT NOT NULL,
                item_count INTEGER NOT NULL,
                PRIMARY KEY(build_generation, facet_kind, facet_identity)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS observability.idx_admin_alert_canonical_catalog_facets_read \
             ON admin_alert_canonical_catalog_facets(build_generation, facet_kind, \
                 item_count DESC, facet_label, facet_value)",
        )
        .execute(&self.pool)
        .await?;
        // Existing v34 snapshots lack durable fragment and catalog progress.
        // They remain derived state, so invalidate them once instead of trying
        // to infer a cursor from a partially built payload.
        sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET active_generation = 0, active_row_count = 0,
                      active_projection_revision = -1,
                      source_recent_generation = -1, source_history_generation = -1,
                      build_generation = 0, build_projection_revision = -1,
                      build_source_recent_generation = -1,
                      build_source_history_generation = -1,
                      build_source_rowid_upper_bound = 0,
                      build_cursor_source_rowid = 0, build_phase = 'idle',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_next_position = 1
                WHERE singleton = 1"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_VERSION,
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_NAME,
            ADMIN_ALERT_CANONICAL_BOUNDED_BUILD_CHECKSUM,
        )
        .await
    }

    async fn apply_admin_alert_canonical_groups_finalize_cursor_migration(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .table_column_exists(
                "admin_alert_canonical_groups_state",
                "build_partition_finalize_fragment_position",
            )
            .await?
        {
            sqlx::query(
                "ALTER TABLE observability.admin_alert_canonical_groups_state \
                 ADD COLUMN build_partition_finalize_fragment_position INTEGER NOT NULL DEFAULT 1",
            )
            .execute(&self.pool)
            .await?;
        }
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.admin_alert_canonical_catalog_payloads (
                build_generation INTEGER NOT NULL,
                facet_kind TEXT NOT NULL,
                cursor_item_count INTEGER NOT NULL DEFAULT 9223372036854775807,
                cursor_label TEXT NOT NULL DEFAULT '',
                cursor_value TEXT NOT NULL DEFAULT '',
                payload_json TEXT NOT NULL DEFAULT '[]',
                payload_status TEXT NOT NULL DEFAULT 'building',
                PRIMARY KEY(build_generation, facet_kind)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("admin_alert_canonical_catalog_payloads", "payload_status")
            .await?
        {
            sqlx::query(
                "ALTER TABLE observability.admin_alert_canonical_catalog_payloads \
                 ADD COLUMN payload_status TEXT NOT NULL DEFAULT 'building'",
            )
            .execute(&self.pool)
            .await?;
        }
        // v35 only persisted source capture. It has no accepted finalization
        // cursor, so discard an in-flight derived build rather than attempting
        // to infer a reduction accumulator. The active generation remains valid.
        sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_generation = 0, build_projection_revision = -1,
                      build_source_recent_generation = -1,
                      build_source_history_generation = -1,
                      build_source_rowid_upper_bound = 0,
                      build_cursor_source_rowid = 0, build_phase = 'idle',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1,
                      build_next_position = 1
                WHERE singleton = 1 AND build_generation > 0"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_VERSION,
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_NAME,
            ADMIN_ALERT_CANONICAL_GROUPS_FINALIZE_CURSOR_CHECKSUM,
        )
        .await
    }

    async fn apply_admin_alert_canonical_payload_resume_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // A canonical payload is an exact administrator response, not an
        // admission budget. Resume the cursor used by older candidates rather
        // than permanently withholding a last-good snapshot for large data.
        sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
             SET build_phase = 'aggregating' \
             WHERE singleton = 1 AND build_phase = 'payload_budget_exceeded'",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE observability.admin_alert_canonical_catalog_payloads \
             SET payload_status = 'building' \
             WHERE payload_status = 'payload_budget_exceeded'",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_VERSION,
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_NAME,
            ADMIN_ALERT_CANONICAL_PAYLOAD_RESUME_CHECKSUM,
        )
        .await
    }
}
