const SCHEMA_BASELINE_VERSION: i64 = 1;
const SCHEMA_BASELINE_NAME: &str = "production-schema-baseline-v1";
const SCHEMA_BASELINE_CHECKSUM: &str = "sha256:8c3198e17b914838e07a4938107f3a0f";
const GC_WORK_VERSION: i64 = 2;
const GC_WORK_NAME: &str = "ha-gc-durable-channel-claims-v1";
const GC_WORK_CHECKSUM: &str = "sha256:40e1bc657936ad891830471519d680e2";
const RECONCILIATION_WORK_VERSION: i64 = 3;
const RECONCILIATION_WORK_NAME: &str = "reconciliation-durable-work-projection-v1";
const RECONCILIATION_WORK_CHECKSUM: &str = "sha256:7e94d5620f3e49a0a17587fb4e019a51";

impl KeyStore {
    #[cfg(not(test))]
    async fn run_warm_schema_semantic_maintenance(&self) -> Result<(), ProxyError> {
        sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
            .execute(&self.pool)
            .await?;
        self.seed_linuxdo_system_tags().await?;
        self.sync_linuxdo_system_tag_default_deltas_with_env().await?;
        self.backfill_linuxdo_user_tag_bindings().await?;
        self.sync_account_quota_limits_with_defaults().await?;
        match maybe_rebase_current_month_business_quota_with_pool(
            &self.pool,
            || self.backend_time.now_utc(),
            META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
            true,
        )
        .await
        {
            Ok(_) => Ok(()),
            Err(err) if is_invalid_current_month_billing_subject_error(&err) => {
                tracing::debug!(
                    component = "schema_migration",
                    event = "semantic_maintenance_skipped",
                    reason = "invalid_current_month_billing_subject",
                );
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn schema_named_object_exists(
        &self,
        schema: &str,
        object_type: &str,
        name: &str,
    ) -> Result<bool, ProxyError> {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {schema}.sqlite_master WHERE type = ? AND name = ?)"
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(object_type)
            .bind(name)
            .fetch_one(&self.pool)
            .await?
            != 0)
    }

    async fn schema_object_exists(&self, schema: &str, name: &str) -> Result<bool, ProxyError> {
        self.schema_named_object_exists(schema, "table", name).await
    }

    async fn schema_table_has_column(
        &self,
        schema: &str,
        table: &str,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let rows = sqlx::query(&format!("PRAGMA {schema}.table_info({table})"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .any(|row| row.try_get::<String, _>("name").is_ok_and(|name| name == column)))
    }

    pub(crate) async fn validate_schema_baseline(&self) -> Result<(), ProxyError> {
        const CORE_TABLES: &[&str] = &[
            "account_entitlements",
            "account_monthly_quota",
            "account_quota_limit_snapshots",
            "account_quota_limits",
            "account_usage_buckets",
            "account_usage_rollup_buckets",
            "admin_passkey_challenges",
            "admin_passkey_credentials",
            "admin_passkey_reset_tokens",
            "admin_passkey_scopes",
            "admin_passkey_sessions",
            "admin_password_settings",
            "announcements",
            "api_key_low_quota_depletions",
            "api_key_maintenance_records",
            "api_key_quarantines",
            "api_key_quota_sync_samples",
            "api_key_transient_backoffs",
            "api_key_user_usage_buckets",
            "api_keys",
            "auth_token_logs",
            "auth_token_quota",
            "auth_tokens",
            "billing_ledger",
            "billing_reconciliation_adjustments",
            "billing_reconciliation_shadow_adjustments",
            "ha_billing_ledger_imports",
            "ha_billing_outbox",
            "ha_control_plane_events",
            "ha_edgeone_audit_logs",
            "ha_failover_operations",
            "ha_node_state",
            "ha_outbox",
            "ha_outbox_gc_channel_state",
            "ha_outbox_gc_state",
            "ha_outbox_suppression",
            "ha_peer_watermarks",
            "ha_recovery_batches",
            "ha_runtime_counter_imports",
            "ha_runtime_outbox",
            "ha_sync_watermarks",
            "http_project_api_key_affinity",
            "linuxdo_credit_recharge_entitlements",
            "linuxdo_credit_recharge_orders",
            "mcp_sessions",
            "meta",
            "oauth_accounts",
            "oauth_login_states",
            "quota_subject_locks",
            "request_rate_limit_snapshots",
            "research_requests",
            "scheduled_jobs",
            "subject_key_breakages",
            "token_api_key_bindings",
            "token_primary_api_key_affinity",
            "token_usage_buckets",
            "token_usage_stats",
            "upstream_reconciliation_research",
            "upstream_reconciliation_settlements",
            "upstream_reconciliation_usage",
            "upstream_usage_rate_attempts",
            "user_api_key_bindings",
            "user_primary_api_key_affinity",
            "user_sessions",
            "user_tag_bindings",
            "user_tags",
            "user_token_bindings",
            "users",
        ];
        const OBSERVABILITY_TABLES: &[&str] = &[
            "api_key_usage_buckets",
            "dashboard_request_rollup_buckets",
            "dashboard_rollup_daily_seals",
            "dashboard_rollup_integrity_day_reaudits",
            "dashboard_rollup_integrity_gaps",
            "dashboard_rollup_integrity_state",
            "dashboard_rollup_integrity_work_items",
            "request_log_catalog_rollups",
            "request_logs",
            "server_pressure_buckets",
        ];
        for (schema, table) in CORE_TABLES
            .iter()
            .map(|table| ("main", *table))
            .chain(
                OBSERVABILITY_TABLES
                    .iter()
                    .map(|table| ("observability", *table)),
            )
        {
            if !self.schema_object_exists(schema, table).await? {
                return Err(ProxyError::Other(format!(
                    "schema migration baseline rejected: missing {schema}.{table}"
                )));
            }
        }
        for (schema, table, columns) in [
            (
                "main",
                "scheduled_jobs",
                &[
                    "id",
                    "job_type",
                    "trigger_source",
                    "key_id",
                    "status",
                    "attempt",
                    "message",
                    "queued_at",
                    "available_at",
                    "claim_generation",
                    "started_at",
                    "finished_at",
                ][..],
            ),
            (
                "main",
                "users",
                &[
                    "id",
                    "display_name",
                    "username",
                    "avatar_template",
                    "active",
                    "debug_info_shared",
                    "created_at",
                    "updated_at",
                    "last_login_at",
                ][..],
            ),
            (
                "main",
                "api_keys",
                &[
                    "id",
                    "api_key",
                    "group_name",
                    "registration_ip",
                    "registration_region",
                    "status",
                    "created_at",
                    "status_changed_at",
                    "last_used_at",
                    "quota_limit",
                    "quota_remaining",
                    "quota_synced_at",
                    "deleted_at",
                ][..],
            ),
            (
                "main",
                "auth_tokens",
                &[
                    "id",
                    "secret",
                    "enabled",
                    "note",
                    "group_name",
                    "total_requests",
                    "created_at",
                    "last_used_at",
                    "deleted_at",
                ][..],
            ),
            (
                "main",
                "billing_ledger",
                &[
                    "auth_token_log_id",
                    "token_id",
                    "billing_subject",
                    "billing_state",
                    "business_credits",
                    "request_user_id",
                    "api_key_id",
                    "request_log_id",
                    "result_status",
                    "created_at",
                    "updated_at",
                    "settled_at",
                    "error_message",
                ][..],
            ),
            (
                "main",
                "mcp_sessions",
                &[
                    "proxy_session_id",
                    "upstream_session_id",
                    "upstream_key_id",
                    "auth_token_id",
                    "user_id",
                    "protocol_version",
                    "last_event_id",
                    "gateway_mode",
                    "experiment_variant",
                    "ab_bucket",
                    "routing_subject_hash",
                    "fallback_reason",
                    "rate_limited_until",
                    "last_rate_limited_at",
                    "last_rate_limit_reason",
                    "created_at",
                    "updated_at",
                    "expires_at",
                    "revoked_at",
                    "revoke_reason",
                ][..],
            ),
            (
                "main",
                "ha_outbox",
                &["seq", "resource", "created_at"][..],
            ),
            (
                "main",
                "ha_billing_outbox",
                &["seq", "resource", "created_at"][..],
            ),
            (
                "main",
                "ha_runtime_outbox",
                &["seq", "resource", "created_at"][..],
            ),
            (
                "main",
                "ha_outbox_gc_channel_state",
                &[
                    "channel",
                    "last_attempt_at",
                    "last_progress_at",
                    "last_deleted_rows",
                    "last_defer_reason",
                    "next_retry_at",
                    "consecutive_no_progress",
                    "batch_size",
                    "last_observed_at",
                    "last_high_watermark",
                    "total_deleted_rows",
                    "debt_mode",
                    "oldest_deletable_age_secs",
                    "deleted_rows_per_minute",
                    "slo_state",
                    "foreground_rps",
                ][..],
            ),
            (
                "main",
                "upstream_reconciliation_usage",
                &[
                    "token_id",
                    "period_code",
                    "project_id",
                    "billing_subject",
                    "settlement_mode",
                    "period_start",
                    "period_end",
                    "key_id",
                    "request_count",
                    "first_used_at",
                    "last_used_at",
                    "updated_at",
                ][..],
            ),
            (
                "main",
                "upstream_reconciliation_settlements",
                &[
                    "settlement_key",
                    "token_id",
                    "period_code",
                    "project_id",
                    "billing_subject",
                    "period_start",
                    "period_end",
                    "status",
                    "upstream_usage",
                    "local_billed_credits",
                    "delta_credits",
                    "degraded_reason",
                    "next_attempt_at",
                    "attempt_count",
                    "created_at",
                    "updated_at",
                    "settled_at",
                ][..],
            ),
            (
                "observability",
                "request_logs",
                &[
                    "id",
                    "api_key_id",
                    "auth_token_id",
                    "request_user_id",
                    "method",
                    "path",
                    "query",
                    "status_code",
                    "tavily_status_code",
                    "error_message",
                    "result_status",
                    "request_kind_key",
                    "request_kind_label",
                    "request_kind_detail",
                    "counts_business_quota",
                    "business_credits",
                    "failure_kind",
                    "key_effect_code",
                    "key_effect_summary",
                    "binding_effect_code",
                    "binding_effect_summary",
                    "selection_effect_code",
                    "selection_effect_summary",
                    "gateway_mode",
                    "experiment_variant",
                    "proxy_session_id",
                    "routing_subject_hash",
                    "upstream_operation",
                    "fallback_reason",
                    "request_body",
                    "response_body",
                    "request_body_bytes",
                    "response_body_bytes",
                    "request_body_sha256",
                    "response_body_sha256",
                    "body_retention_days",
                    "body_retention_profile",
                    "body_cleaned_reason",
                    "body_cleaned_at",
                    "forwarded_headers",
                    "dropped_headers",
                    "remote_addr",
                    "client_ip",
                    "client_ip_source",
                    "client_ip_trusted",
                    "ip_headers",
                    "visibility",
                    "created_at",
                ][..],
            ),
        ] {
            for column in columns {
                if !self
                    .schema_table_has_column(schema, table, column)
                    .await?
                {
                    return Err(ProxyError::Other(format!(
                        "schema migration baseline rejected: missing {schema}.{table}.{column}"
                    )));
                }
            }
        }
        for index in [
            "idx_scheduled_jobs_queue_available",
            "idx_ha_outbox_created",
            "idx_ha_billing_outbox_created",
            "idx_ha_runtime_outbox_created",
            "idx_upstream_reconciliation_usage_period",
        ] {
            if !self
                .schema_named_object_exists("main", "index", index)
                .await?
            {
                return Err(ProxyError::Other(format!(
                    "schema migration baseline rejected: missing main index {index}"
                )));
            }
        }
        Ok(())
    }

    async fn validate_schema_adoption_source(&self) -> Result<(), ProxyError> {
        for (schema, table) in [
            ("main", "meta"),
            ("main", "users"),
            ("main", "api_keys"),
            ("main", "auth_tokens"),
            ("main", "billing_ledger"),
            ("observability", "request_logs"),
        ] {
            if !self.schema_object_exists(schema, table).await? {
                return Err(ProxyError::Other(format!(
                    "schema migration adoption rejected: missing source table {schema}.{table}"
                )));
            }
        }
        for (schema, table, columns) in [
            ("main", "meta", &["key", "value"][..]),
            ("main", "users", &["id", "created_at", "updated_at"][..]),
            ("main", "api_keys", &["id", "api_key", "last_used_at"][..]),
            ("main", "auth_tokens", &["id", "secret"][..]),
            (
                "main",
                "billing_ledger",
                &[
                    "auth_token_log_id",
                    "token_id",
                    "result_status",
                    "created_at",
                ][..],
            ),
            (
                "observability",
                "request_logs",
                &["id", "method", "path", "created_at"][..],
            ),
        ] {
            for column in columns {
                if !self.schema_table_has_column(schema, table, column).await? {
                    return Err(ProxyError::Other(format!(
                        "schema migration adoption rejected: missing source column {schema}.{table}.{column}"
                    )));
                }
            }
        }
        Ok(())
    }

    async fn ensure_schema_migration_ledger(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                checksum TEXT NOT NULL,
                applied_at INTEGER NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn verify_recorded_schema_migrations(&self) -> Result<(), ProxyError> {
        let expected = [
            (
                SCHEMA_BASELINE_VERSION,
                SCHEMA_BASELINE_NAME,
                SCHEMA_BASELINE_CHECKSUM,
            ),
            (GC_WORK_VERSION, GC_WORK_NAME, GC_WORK_CHECKSUM),
            (
                RECONCILIATION_WORK_VERSION,
                RECONCILIATION_WORK_NAME,
                RECONCILIATION_WORK_CHECKSUM,
            ),
        ];
        let recorded: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT version, name, checksum FROM schema_migrations ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await?;
        for (version, name, checksum) in recorded {
            let Some((_, expected_name, expected_checksum)) = expected
                .iter()
                .find(|(expected_version, _, _)| *expected_version == version)
            else {
                return Err(ProxyError::Other(format!(
                    "schema migration ledger contains unknown version {version}"
                )));
            };
            if name != *expected_name || checksum != *expected_checksum {
                return Err(ProxyError::Other(format!(
                    "schema migration checksum mismatch at version {version}"
                )));
            }
        }
        Ok(())
    }

    async fn validate_applied_migration_objects(&self) -> Result<(), ProxyError> {
        if self.schema_migration_applied(GC_WORK_VERSION).await?
            && (!self
                .table_column_exists("ha_outbox_gc_channel_state", "claim_generation")
                .await?
                || !self
                    .table_column_exists("ha_outbox_gc_channel_state", "claim_started_at")
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 2".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_WORK_VERSION)
            .await?
            && (!self
                .schema_object_exists("main", "upstream_reconciliation_work")
                .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "index",
                        "idx_upstream_reconciliation_work_period",
                    )
                    .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "trigger",
                        "trg_upstream_reconciliation_usage_work_insert",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 3".to_string(),
            ));
        }
        Ok(())
    }

    async fn record_schema_migration(
        &self,
        version: i64,
        name: &str,
        checksum: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES (?, ?, ?, ?)",
        )
        .bind(version)
        .bind(name)
        .bind(checksum)
        .bind(self.backend_time.now_ts())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn schema_migration_applied(&self, version: i64) -> Result<bool, ProxyError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?)",
        )
        .bind(version)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    async fn apply_gc_work_migration(&self) -> Result<(), ProxyError> {
        if !self
            .table_column_exists("ha_outbox_gc_channel_state", "claim_generation")
            .await?
        {
            sqlx::query(
                "ALTER TABLE ha_outbox_gc_channel_state ADD COLUMN claim_generation INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self
            .table_column_exists("ha_outbox_gc_channel_state", "claim_started_at")
            .await?
        {
            sqlx::query(
                "ALTER TABLE ha_outbox_gc_channel_state ADD COLUMN claim_started_at INTEGER",
            )
            .execute(&self.pool)
            .await?;
        }
        self.record_schema_migration(GC_WORK_VERSION, GC_WORK_NAME, GC_WORK_CHECKSUM)
            .await
    }

    async fn apply_reconciliation_work_migration(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_work (
                token_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                project_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                settlement_mode TEXT NOT NULL,
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                scheduling_key_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (token_id, period_code)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_work_period ON upstream_reconciliation_work(period_end, scheduling_key_id, token_id, period_code)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TRIGGER IF NOT EXISTS trg_upstream_reconciliation_usage_work_insert
               AFTER INSERT ON upstream_reconciliation_usage
               BEGIN
                 INSERT INTO upstream_reconciliation_work (
                   token_id, period_code, project_id, billing_subject, settlement_mode,
                   period_start, period_end, scheduling_key_id, updated_at
                 ) VALUES (
                   NEW.token_id, NEW.period_code, NEW.project_id, NEW.billing_subject,
                   NEW.settlement_mode, NEW.period_start, NEW.period_end, NEW.key_id, NEW.updated_at
                 )
                 ON CONFLICT(token_id, period_code) DO UPDATE SET
                   project_id = MIN(upstream_reconciliation_work.project_id, excluded.project_id),
                   billing_subject = MIN(upstream_reconciliation_work.billing_subject, excluded.billing_subject),
                   settlement_mode = MIN(upstream_reconciliation_work.settlement_mode, excluded.settlement_mode),
                   period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                   period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                   scheduling_key_id = MIN(upstream_reconciliation_work.scheduling_key_id, excluded.scheduling_key_id),
                   updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at);
               END"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_WORK_VERSION,
            RECONCILIATION_WORK_NAME,
            RECONCILIATION_WORK_CHECKSUM,
        )
        .await
    }

    pub(crate) async fn prepare_versioned_schema(&self) -> Result<bool, ProxyError> {
        let started = std::time::Instant::now();
        let existing_database = self.schema_object_exists("main", "meta").await?;
        if !existing_database {
            return Ok(true);
        }
        let ledger_preexisting = self
            .schema_object_exists("main", "schema_migrations")
            .await?;
        let interrupted_adoption = if ledger_preexisting {
            !self.schema_migration_applied(SCHEMA_BASELINE_VERSION).await?
                && sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations")
                    .fetch_one(&self.pool)
                    .await?
                    == 0
        } else {
            false
        };
        if !ledger_preexisting || interrupted_adoption {
            self.validate_schema_adoption_source().await?;
            self.validate_schema_baseline().await?;
            self.finish_existing_database_schema_adoption().await?;
            return Ok(false);
        }
        self.validate_schema_baseline().await?;
        self.verify_recorded_schema_migrations().await?;
        if !self.schema_migration_applied(SCHEMA_BASELINE_VERSION).await? {
            return Err(ProxyError::Other(
                "schema migration ledger is missing the baseline record".to_string(),
            ));
        }
        if !self.schema_migration_applied(GC_WORK_VERSION).await? {
            self.apply_gc_work_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_WORK_VERSION)
            .await?
        {
            self.apply_reconciliation_work_migration().await?;
        }
        self.validate_applied_migration_objects().await?;
        tracing::debug!(
            component = "schema_migration",
            event = "verified",
            outcome = "current",
            elapsed_ms = started.elapsed().as_millis() as u64,
        );
        Ok(false)
    }

    async fn finish_existing_database_schema_adoption(&self) -> Result<(), ProxyError> {
        let started = std::time::Instant::now();
        self.ensure_schema_migration_ledger().await?;
        if !self.schema_migration_applied(SCHEMA_BASELINE_VERSION).await? {
            self.record_schema_migration(
                SCHEMA_BASELINE_VERSION,
                SCHEMA_BASELINE_NAME,
                SCHEMA_BASELINE_CHECKSUM,
            )
            .await?;
        }
        if !self.schema_migration_applied(GC_WORK_VERSION).await? {
            self.apply_gc_work_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_WORK_VERSION)
            .await?
        {
            self.apply_reconciliation_work_migration().await?;
        }
        self.validate_applied_migration_objects().await?;
        tracing::info!(
            component = "schema_migration",
            event = "baseline_adopted",
            outcome = "applied",
            elapsed_ms = started.elapsed().as_millis() as u64,
            migration_count = 3_i64,
        );
        Ok(())
    }

    pub(crate) async fn finish_new_database_schema_migrations(&self) -> Result<(), ProxyError> {
        let started = std::time::Instant::now();
        self.validate_schema_baseline().await?;
        self.ensure_schema_migration_ledger().await?;
        self.record_schema_migration(
            SCHEMA_BASELINE_VERSION,
            SCHEMA_BASELINE_NAME,
            SCHEMA_BASELINE_CHECKSUM,
        )
        .await?;
        self.apply_gc_work_migration().await?;
        self.apply_reconciliation_work_migration().await?;
        self.validate_applied_migration_objects().await?;
        tracing::info!(
            component = "schema_migration",
            event = "baseline_adopted",
            outcome = "applied",
            elapsed_ms = started.elapsed().as_millis() as u64,
            migration_count = 3_i64,
        );
        Ok(())
    }
}
