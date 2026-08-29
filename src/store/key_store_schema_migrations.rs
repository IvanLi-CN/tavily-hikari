const SCHEMA_BASELINE_VERSION: i64 = 1;
const SCHEMA_BASELINE_NAME: &str = "production-schema-baseline-v1";
const SCHEMA_BASELINE_CHECKSUM: &str = "sha256:8c3198e17b914838e07a4938107f3a0f";
const GC_WORK_VERSION: i64 = 2;
const GC_WORK_NAME: &str = "ha-gc-durable-channel-claims-v1";
const GC_WORK_CHECKSUM: &str = "sha256:40e1bc657936ad891830471519d680e2";
const RECONCILIATION_WORK_VERSION: i64 = 3;
const RECONCILIATION_WORK_NAME: &str = "reconciliation-durable-work-projection-v1";
const RECONCILIATION_WORK_CHECKSUM: &str = "sha256:7e94d5620f3e49a0a17587fb4e019a51";
const RECONCILIATION_OUTCOME_VERSION: i64 = 4;
const RECONCILIATION_OUTCOME_NAME: &str = "reconciliation-terminal-outcomes-v1";
const RECONCILIATION_OUTCOME_CHECKSUM: &str = "sha256:4d6fd3e8c7a3a806a5d420ef07fa4f3c";
const RECONCILIATION_TERMINAL_REFRESH_VERSION: i64 = 5;
const RECONCILIATION_TERMINAL_REFRESH_NAME: &str = "reconciliation-terminal-usage-refresh-v1";
const RECONCILIATION_TERMINAL_REFRESH_CHECKSUM: &str = "sha256:8e4f4cc3f832d24d4f7d7dc3d6f2a8c1";
const RECONCILIATION_TERMINAL_SAME_SECOND_VERSION: i64 = 6;
const RECONCILIATION_TERMINAL_SAME_SECOND_NAME: &str =
    "reconciliation-terminal-usage-refresh-v2";
const RECONCILIATION_TERMINAL_SAME_SECOND_CHECKSUM: &str =
    "sha256:83c35ed6d15067fc5c4e830eaf3b520c";
const RECONCILIATION_PROJECTION_LIFECYCLE_VERSION: i64 = 7;
const RECONCILIATION_PROJECTION_LIFECYCLE_NAME: &str =
    "reconciliation-projection-lifecycle-v1";
const RECONCILIATION_PROJECTION_LIFECYCLE_CHECKSUM: &str =
    "sha256:75705f2a93c4a8d6526f13b708490ec1";
const HA_GC_LEGACY_CURSOR_VERSION: i64 = 8;
const HA_GC_LEGACY_CURSOR_NAME: &str = "ha-gc-per-channel-legacy-cursor-v1";
const HA_GC_LEGACY_CURSOR_CHECKSUM: &str = "sha256:ac41cf12d5e848f3a1b9d276e0f4598c";
const RECONCILIATION_ENGINE_STATE_VERSION: i64 = 9;
const RECONCILIATION_ENGINE_STATE_NAME: &str = "reconciliation-engine-state-v1";
const RECONCILIATION_ENGINE_STATE_CHECKSUM: &str =
    "sha256:614b3746410a20742499208d97764b88";
const RECONCILIATION_OUTCOME_REPAIR_VERSION: i64 = 10;
const RECONCILIATION_OUTCOME_REPAIR_NAME: &str = "reconciliation-shadow-outcome-repair-v1";
const RECONCILIATION_OUTCOME_REPAIR_CHECKSUM: &str =
    "sha256:0c3d8491c7064ba6a47fce9bb28c6220";
const RECONCILIATION_CONTROLLER_VERSION: i64 = 11;
const RECONCILIATION_CONTROLLER_NAME: &str = "reconciliation-activation-controller-v1";
const RECONCILIATION_CONTROLLER_CHECKSUM: &str =
    "sha256:dc6ccbc1e746c3be915e576218978879";
const ALERT_PROJECTION_VERSION: i64 = 12;
const ALERT_PROJECTION_NAME: &str = "dashboard-alert-projection-v1";
const ALERT_PROJECTION_CHECKSUM: &str = "sha256:5c781d4f2e85029d5aac2858300d182e";
const ALERT_PROJECTION_RECENT_WINDOW_VERSION: i64 = 13;
const ALERT_PROJECTION_RECENT_WINDOW_NAME: &str = "dashboard-alert-projection-recent-window-v1";
const ALERT_PROJECTION_RECENT_WINDOW_CHECKSUM: &str =
    "sha256:4d620c7e43ae60c4f7067f4b4995c24e";
const ALERT_PROJECTION_RECENT_WINDOW_SECS: i64 = 30 * 24 * 60 * 60;
const ALERT_PROJECTION_ADMIN_HISTORY_VERSION: i64 = 14;
const ALERT_PROJECTION_ADMIN_HISTORY_NAME: &str = "dashboard-alert-projection-admin-history-v1";
const ALERT_PROJECTION_ADMIN_HISTORY_CHECKSUM: &str =
    "sha256:ff6f5901c3a603feac18afbbb04a1cdf";
const ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_VERSION: i64 = 15;
const ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_NAME: &str =
    "dashboard-alert-projection-admin-history-fence-repair-v1";
const ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_CHECKSUM: &str =
    "sha256:8ef5bf8e2b29acd27096657ad0d3d97e";
const ALERT_PROJECTION_SUMMARY_VERSION: i64 = 16;
const ALERT_PROJECTION_SUMMARY_NAME: &str = "dashboard-alert-projection-summary-v1";
const ALERT_PROJECTION_SUMMARY_CHECKSUM: &str = "sha256:b7f45f0e7e5388f9b1f6b0fa75b9c4e1";
const ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_VERSION: i64 = 17;
const ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_NAME: &str =
    "dashboard-alert-projection-exact-boundary-repair-v1";
const ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_CHECKSUM: &str =
    "sha256:4f1246b9e907ed2bc4ce4cda5a66fb83";
const RECONCILIATION_TRANSPORT_OBSERVATION_VERSION: i64 = 18;
const RECONCILIATION_TRANSPORT_OBSERVATION_NAME: &str =
    "reconciliation-transport-observation-v1";
const RECONCILIATION_TRANSPORT_OBSERVATION_CHECKSUM: &str =
    "sha256:8ad321a9b9624b550c29e434d3dfb37d";
const RECONCILIATION_TRANSPORT_STATE_VERSION: i64 = 19;
const RECONCILIATION_TRANSPORT_STATE_NAME: &str = "reconciliation-transport-observation-state-v1";
const RECONCILIATION_TRANSPORT_STATE_CHECKSUM: &str =
    "sha256:3d0e3b1c8a0b1f9d7c1e0a2b4f6d8e90";
const RECONCILIATION_RESEARCH_PROGRESS_WINDOW_VERSION: i64 = 20;
const RECONCILIATION_RESEARCH_PROGRESS_WINDOW_NAME: &str =
    "reconciliation-research-progress-window-v1";
const RECONCILIATION_RESEARCH_PROGRESS_WINDOW_CHECKSUM: &str =
    "sha256:6431dd87e790811d9b05f32d7a2c54de";
const RECONCILIATION_RESEARCH_SELECTION_VERSION: i64 = 21;
const RECONCILIATION_RESEARCH_SELECTION_NAME: &str = "reconciliation-research-selection-v1";
const RECONCILIATION_RESEARCH_SELECTION_CHECKSUM: &str =
    "sha256:2b9b3c74d8a1e5f6c7b8d9e0f1a2b3c4";
// Version 21 is already occupied by the research scan cursor on deployed
// databases. Keep the key-observation state additive at the next ledger slot
// so rolling upgrades never rewrite an existing migration record.
const RECONCILIATION_KEY_OBSERVATION_VERSION: i64 = 22;
const RECONCILIATION_KEY_OBSERVATION_NAME: &str = "reconciliation-key-observation-v1";
const RECONCILIATION_KEY_OBSERVATION_CHECKSUM: &str =
    "sha256:9c8d8f2d3c75a9a0f0c6b2e19d6b7a11";
const RECONCILIATION_OBSERVATION_METRICS_VERSION: i64 = 23;
const RECONCILIATION_OBSERVATION_METRICS_NAME: &str =
    "reconciliation-key-observation-metrics-v1";
const RECONCILIATION_OBSERVATION_METRICS_CHECKSUM: &str =
    "sha256:2f7c6b8d9e0a1b2c3d4e5f60718293a4";
const NEW_DATABASE_BOOTSTRAP_MARKER: &str = "tavily-hikari-schema-bootstrap-v1";

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

    async fn schema_has_domain_rows_without_meta(&self) -> Result<bool, ProxyError> {
        for schema in ["main", "observability"] {
            let tables: Vec<String> = sqlx::query_scalar(&format!(
                "SELECT name FROM {schema}.sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'"
            ))
            .fetch_all(&self.pool)
            .await?;
            for table in tables {
                let quoted_table = table.replace('"', "\"\"");
                let sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM {schema}.\"{quoted_table}\" LIMIT 1)"
                );
                if sqlx::query_scalar::<_, i64>(&sql)
                    .fetch_one(&self.pool)
                    .await?
                    != 0
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn new_database_bootstrap_in_progress(&self) -> Result<bool, ProxyError> {
        if !self
            .schema_object_exists("main", "schema_bootstrap_state")
            .await?
        {
            return Ok(false);
        }
        let markers: Vec<String> =
            sqlx::query_scalar("SELECT marker FROM schema_bootstrap_state")
                .fetch_all(&self.pool)
                .await?;
        if markers == [NEW_DATABASE_BOOTSTRAP_MARKER] {
            return Ok(true);
        }
        Err(ProxyError::Other(
            "schema migration adoption rejected: invalid bootstrap marker".to_string(),
        ))
    }

    async fn begin_new_database_bootstrap(&self) -> Result<(), ProxyError> {
        let mut transaction = begin_immediate_sqlite_connection(&self.pool).await?;
        sqlx::query(
            "CREATE TABLE schema_bootstrap_state (marker TEXT PRIMARY KEY NOT NULL)",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO schema_bootstrap_state (marker) VALUES (?)")
            .bind(NEW_DATABASE_BOOTSTRAP_MARKER)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await
    }

    async fn clear_new_database_bootstrap_marker(&self) -> Result<(), ProxyError> {
        if self
            .schema_object_exists("main", "schema_bootstrap_state")
            .await?
        {
            sqlx::query("DROP TABLE schema_bootstrap_state")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
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
            "dashboard_rollup_rebalance_recovery",
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
        if !self
            .schema_named_object_exists("observability", "index", "idx_request_logs_time")
            .await?
        {
            return Err(ProxyError::Other(
                "schema migration baseline rejected: missing observability.idx_request_logs_time"
                    .to_string(),
            ));
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
        ] {
            for column in columns {
                if !self.schema_table_has_column(schema, table, column).await? {
                    return Err(ProxyError::Other(format!(
                        "schema migration adoption rejected: missing source column {schema}.{table}.{column}"
                    )));
                }
            }
        }
        let request_logs_schema = if self
            .schema_object_exists("observability", "request_logs")
            .await?
        {
            "observability"
        } else if self.schema_object_exists("main", "request_logs").await? {
            "main"
        } else {
            return Err(ProxyError::Other(
                "schema migration adoption rejected: missing source request_logs table".to_string(),
            ));
        };
        for column in ["id", "method", "path", "created_at"] {
            if !self
                .schema_table_has_column(request_logs_schema, "request_logs", column)
                .await?
            {
                return Err(ProxyError::Other(format!(
                    "schema migration adoption rejected: missing source column {request_logs_schema}.request_logs.{column}"
                )));
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
            (
                RECONCILIATION_OUTCOME_VERSION,
                RECONCILIATION_OUTCOME_NAME,
                RECONCILIATION_OUTCOME_CHECKSUM,
            ),
            (
                RECONCILIATION_TERMINAL_REFRESH_VERSION,
                RECONCILIATION_TERMINAL_REFRESH_NAME,
                RECONCILIATION_TERMINAL_REFRESH_CHECKSUM,
            ),
            (
                RECONCILIATION_TERMINAL_SAME_SECOND_VERSION,
                RECONCILIATION_TERMINAL_SAME_SECOND_NAME,
                RECONCILIATION_TERMINAL_SAME_SECOND_CHECKSUM,
            ),
            (
                RECONCILIATION_PROJECTION_LIFECYCLE_VERSION,
                RECONCILIATION_PROJECTION_LIFECYCLE_NAME,
                RECONCILIATION_PROJECTION_LIFECYCLE_CHECKSUM,
            ),
            (
                HA_GC_LEGACY_CURSOR_VERSION,
                HA_GC_LEGACY_CURSOR_NAME,
                HA_GC_LEGACY_CURSOR_CHECKSUM,
            ),
            (
                RECONCILIATION_ENGINE_STATE_VERSION,
                RECONCILIATION_ENGINE_STATE_NAME,
                RECONCILIATION_ENGINE_STATE_CHECKSUM,
            ),
            (
                RECONCILIATION_OUTCOME_REPAIR_VERSION,
                RECONCILIATION_OUTCOME_REPAIR_NAME,
                RECONCILIATION_OUTCOME_REPAIR_CHECKSUM,
            ),
            (
                RECONCILIATION_CONTROLLER_VERSION,
                RECONCILIATION_CONTROLLER_NAME,
                RECONCILIATION_CONTROLLER_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_VERSION,
                ALERT_PROJECTION_NAME,
                ALERT_PROJECTION_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_RECENT_WINDOW_VERSION,
                ALERT_PROJECTION_RECENT_WINDOW_NAME,
                ALERT_PROJECTION_RECENT_WINDOW_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_ADMIN_HISTORY_VERSION,
                ALERT_PROJECTION_ADMIN_HISTORY_NAME,
                ALERT_PROJECTION_ADMIN_HISTORY_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_VERSION,
                ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_NAME,
                ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_SUMMARY_VERSION,
                ALERT_PROJECTION_SUMMARY_NAME,
                ALERT_PROJECTION_SUMMARY_CHECKSUM,
            ),
            (
                ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_VERSION,
                ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_NAME,
                ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_CHECKSUM,
            ),
            (
                RECONCILIATION_TRANSPORT_OBSERVATION_VERSION,
                RECONCILIATION_TRANSPORT_OBSERVATION_NAME,
                RECONCILIATION_TRANSPORT_OBSERVATION_CHECKSUM,
            ),
            (
                RECONCILIATION_TRANSPORT_STATE_VERSION,
                RECONCILIATION_TRANSPORT_STATE_NAME,
                RECONCILIATION_TRANSPORT_STATE_CHECKSUM,
            ),
            (
                RECONCILIATION_RESEARCH_PROGRESS_WINDOW_VERSION,
                RECONCILIATION_RESEARCH_PROGRESS_WINDOW_NAME,
                RECONCILIATION_RESEARCH_PROGRESS_WINDOW_CHECKSUM,
            ),
            (
                RECONCILIATION_RESEARCH_SELECTION_VERSION,
                RECONCILIATION_RESEARCH_SELECTION_NAME,
                RECONCILIATION_RESEARCH_SELECTION_CHECKSUM,
            ),
            (
                RECONCILIATION_KEY_OBSERVATION_VERSION,
                RECONCILIATION_KEY_OBSERVATION_NAME,
                RECONCILIATION_KEY_OBSERVATION_CHECKSUM,
            ),
            (
                RECONCILIATION_OBSERVATION_METRICS_VERSION,
                RECONCILIATION_OBSERVATION_METRICS_NAME,
                RECONCILIATION_OBSERVATION_METRICS_CHECKSUM,
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
            .schema_migration_applied(HA_GC_LEGACY_CURSOR_VERSION)
            .await?
            && !self
                .table_column_exists("ha_outbox_gc_channel_state", "legacy_cursor_seq")
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 8".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_ENGINE_STATE_VERSION)
            .await?
            && (!self
                .schema_object_exists("main", "upstream_reconciliation_projection_state")
                .await?
                || !self
                    .schema_object_exists("main", "upstream_reconciliation_run_observation")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "transport_failure_streak")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "transport_retry_at")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "semantic_failure_streak")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "semantic_retry_at")
                    .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "trigger",
                        "trg_upstream_reconciliation_work_failure_reset_insert",
                    )
                    .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "trigger",
                        "trg_upstream_reconciliation_work_failure_reset_update",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 9".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_TRANSPORT_OBSERVATION_VERSION)
            .await?
            && !self
                .table_column_exists(
                    "upstream_reconciliation_run_observation",
                    "last_transport_kind",
                )
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 18".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_TRANSPORT_STATE_VERSION)
            .await?
            && (!self
                .table_column_exists(
                    "upstream_reconciliation_run_observation",
                    "last_transport_kind_at",
                )
                .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_run_observation",
                        "last_retryable_outcome",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 19".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_RESEARCH_PROGRESS_WINDOW_VERSION)
            .await?
            && !self
                .schema_object_exists("main", "upstream_reconciliation_research_progress_window")
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 20".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_RESEARCH_SELECTION_VERSION)
            .await?
            && (!self
                .schema_object_exists("main", "upstream_reconciliation_research_scan_state")
                .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "index",
                        "idx_upstream_reconciliation_research_due_scan",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 21".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_KEY_OBSERVATION_VERSION)
            .await?
            && (!self
                .schema_object_exists("main", "upstream_reconciliation_key_observations")
                .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "index",
                        "idx_reconciliation_key_observations_generation",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 22".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_OBSERVATION_METRICS_VERSION)
            .await?
            && (!self
                .table_column_exists(
                    "upstream_reconciliation_run_observation",
                    "partial_key_observation_count",
                )
                .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_run_observation",
                        "multi_key_pending_count",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_run_observation",
                        "remote_attempt_budget_defer_count",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_run_observation",
                        "resumed_run_count",
                    )
                    .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_run_observation",
                        "terminal_run_count",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 23".to_string(),
            ));
        }
        if self
            .schema_migration_applied(RECONCILIATION_CONTROLLER_VERSION)
            .await?
            && (!self
                .schema_object_exists("main", "upstream_reconciliation_control_state")
                .await?
                || !self
                    .schema_object_exists("main", "upstream_reconciliation_control_transitions")
                    .await?
                || !self
                    .table_column_exists(
                        "upstream_reconciliation_control_state",
                        "activation_period_start",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 11".to_string(),
            ));
        }
        if self.schema_migration_applied(ALERT_PROJECTION_VERSION).await?
            && (!self
                .schema_object_exists("observability", "dashboard_alert_projection_state")
                .await?
                || !self
                    .schema_object_exists("observability", "dashboard_alert_projection_events")
                    .await?
                || !self
                    .schema_named_object_exists(
                        "observability",
                        "index",
                        "idx_dashboard_alert_projection_events_time",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 12".to_string(),
            ));
        }
        if self
            .schema_migration_applied(ALERT_PROJECTION_ADMIN_HISTORY_VERSION)
            .await?
            && !self
                .schema_object_exists("observability", "dashboard_alert_projection_history_state")
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 14".to_string(),
            ));
        }
        if self
            .schema_migration_applied(ALERT_PROJECTION_SUMMARY_VERSION)
            .await?
            && !self
                .schema_object_exists(
                    "observability",
                    "dashboard_alert_projection_recent_summaries",
                )
                .await?
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 16".to_string(),
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
        if self
            .schema_migration_applied(RECONCILIATION_OUTCOME_VERSION)
            .await?
            && (!self
                .table_column_exists("upstream_reconciliation_work", "work_generation")
                .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "completed_generation")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "next_attempt_at")
                    .await?
                || !self
                    .table_column_exists("upstream_reconciliation_work", "last_outcome")
                    .await?
                || !self
                    .schema_named_object_exists(
                        "main",
                        "trigger",
                        "trg_upstream_reconciliation_usage_work_update",
                    )
                    .await?)
        {
            return Err(ProxyError::Other(
                "schema migration object validation failed at version 4".to_string(),
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

    async fn apply_ha_gc_legacy_cursor_migration(&self) -> Result<(), ProxyError> {
        if !self
            .table_column_exists("ha_outbox_gc_channel_state", "legacy_cursor_seq")
            .await?
        {
            sqlx::query(
                "ALTER TABLE ha_outbox_gc_channel_state ADD COLUMN legacy_cursor_seq INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        sqlx::query(
            r#"
            UPDATE ha_outbox_gc_channel_state
               SET legacy_cursor_seq = CASE channel
                   WHEN 'control' THEN (SELECT last_legacy_control_seq FROM ha_outbox_gc_state WHERE id = 'local')
                   WHEN 'billing' THEN (SELECT last_legacy_billing_seq FROM ha_outbox_gc_state WHERE id = 'local')
                   WHEN 'runtime' THEN (SELECT last_legacy_runtime_seq FROM ha_outbox_gc_state WHERE id = 'local')
                   ELSE legacy_cursor_seq
               END
             WHERE legacy_cursor_seq = 0
            "#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            HA_GC_LEGACY_CURSOR_VERSION,
            HA_GC_LEGACY_CURSOR_NAME,
            HA_GC_LEGACY_CURSOR_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_engine_state_migration(&self) -> Result<(), ProxyError> {
        for (column, definition) in [
            ("transport_failure_streak", "INTEGER NOT NULL DEFAULT 0"),
            ("transport_retry_at", "INTEGER NOT NULL DEFAULT 0"),
            ("semantic_failure_streak", "INTEGER NOT NULL DEFAULT 0"),
            ("semantic_retry_at", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_work", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE upstream_reconciliation_work ADD COLUMN {column} {definition}"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_projection_state (
                id TEXT PRIMARY KEY NOT NULL CHECK(id = 'local'),
                cursor_token_id TEXT NOT NULL DEFAULT '',
                cursor_key_id TEXT NOT NULL DEFAULT '',
                cursor_period_code TEXT NOT NULL DEFAULT '',
                batch_size INTEGER NOT NULL DEFAULT 25,
                fast_slice_streak INTEGER NOT NULL DEFAULT 0,
                scanned_rows INTEGER NOT NULL DEFAULT 0,
                transaction_p95_ms INTEGER NOT NULL DEFAULT 0,
                tx_hold_le_10 INTEGER NOT NULL DEFAULT 0,
                tx_hold_le_25 INTEGER NOT NULL DEFAULT 0,
                tx_hold_le_50 INTEGER NOT NULL DEFAULT 0,
                tx_hold_le_100 INTEGER NOT NULL DEFAULT 0,
                tx_hold_le_250 INTEGER NOT NULL DEFAULT 0,
                tx_hold_over_250 INTEGER NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0,
                next_retry_at INTEGER NOT NULL DEFAULT 0,
                last_defer_reason TEXT,
                updated_at INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        let legacy_complete = self
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1)
            .await?
            .unwrap_or(0);
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_projection_state (id, completed)
               VALUES ('local', ?)
               ON CONFLICT(id) DO NOTHING"#,
        )
        .bind(i64::from(legacy_complete != 0))
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_run_observation (
                id TEXT PRIMARY KEY NOT NULL CHECK(id = 'local'),
                mode TEXT NOT NULL DEFAULT 'disabled',
                projection_state TEXT NOT NULL DEFAULT 'unknown',
                projection_scanned_rows INTEGER NOT NULL DEFAULT 0,
                projection_batch_size INTEGER NOT NULL DEFAULT 25,
                projection_transaction_p95_ms INTEGER NOT NULL DEFAULT 0,
                cursor_advanced INTEGER NOT NULL DEFAULT 0,
                hydrate_ms INTEGER NOT NULL DEFAULT 0,
                first_remote_ms INTEGER,
                remote_ms INTEGER NOT NULL DEFAULT 0,
                finalization_ms INTEGER NOT NULL DEFAULT 0,
                research_ms INTEGER NOT NULL DEFAULT 0,
                settled_count INTEGER NOT NULL DEFAULT 0,
                no_adjustment_count INTEGER NOT NULL DEFAULT 0,
                observed_count INTEGER NOT NULL DEFAULT 0,
                upstream_429_count INTEGER NOT NULL DEFAULT 0,
                transport_failure_count INTEGER NOT NULL DEFAULT 0,
                semantic_failure_count INTEGER NOT NULL DEFAULT 0,
                local_pressure_count INTEGER NOT NULL DEFAULT 0,
                partial_key_observation_count INTEGER NOT NULL DEFAULT 0,
                multi_key_pending_count INTEGER NOT NULL DEFAULT 0,
                remote_attempt_budget_defer_count INTEGER NOT NULL DEFAULT 0,
                resumed_run_count INTEGER NOT NULL DEFAULT 0,
                terminal_run_count INTEGER NOT NULL DEFAULT 0,
                last_transport_kind TEXT,
                continuation_reason TEXT,
                next_retry_at INTEGER,
                observed_at INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("INSERT INTO upstream_reconciliation_run_observation (id) VALUES ('local') ON CONFLICT(id) DO NOTHING")
            .execute(&self.pool)
            .await?;
        for (name, event) in [
            ("trg_upstream_reconciliation_work_failure_reset_insert", "INSERT"),
            ("trg_upstream_reconciliation_work_failure_reset_update", "UPDATE"),
        ] {
            sqlx::query(&format!(
                r#"CREATE TRIGGER IF NOT EXISTS {name}
                   AFTER {event} ON upstream_reconciliation_usage
                   BEGIN
                     UPDATE upstream_reconciliation_work
                        SET transport_failure_streak = 0,
                            transport_retry_at = 0,
                            semantic_failure_streak = 0,
                            semantic_retry_at = 0
                      WHERE token_id = NEW.token_id AND period_code = NEW.period_code;
                   END"#
            ))
            .execute(&self.pool)
            .await?;
        }
        self.record_schema_migration(
            RECONCILIATION_ENGINE_STATE_VERSION,
            RECONCILIATION_ENGINE_STATE_NAME,
            RECONCILIATION_ENGINE_STATE_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_outcome_repair_migration(&self) -> Result<(), ProxyError> {
        // Historical terminal outcomes are repaired by the existing bounded
        // projection slices. Startup only resets the local derived cursor.
        sqlx::query(
            r#"UPDATE upstream_reconciliation_projection_state
               SET cursor_token_id = '', cursor_key_id = '', cursor_period_code = '',
                   completed = 0, next_retry_at = 0,
                   last_defer_reason = 'outcome_repair_pending', updated_at = ?
               WHERE id = 'local'"#,
        )
        .bind(self.backend_time.now_ts())
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_OUTCOME_REPAIR_VERSION,
            RECONCILIATION_OUTCOME_REPAIR_NAME,
            RECONCILIATION_OUTCOME_REPAIR_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_controller_migration(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_control_state (
                id TEXT PRIMARY KEY NOT NULL CHECK(id = 'local'),
                mode TEXT NOT NULL DEFAULT 'compare'
                    CHECK(mode IN ('compare', 'active', 'active_paused')),
                activation_period_code TEXT,
                activation_period_start INTEGER,
                legacy_active INTEGER NOT NULL DEFAULT 0,
                paused_reason TEXT,
                transitioned_at INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_control_transitions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                mode TEXT NOT NULL CHECK(mode IN ('compare', 'active', 'active_paused')),
                action TEXT NOT NULL,
                activation_period_code TEXT,
                transitioned_at INTEGER NOT NULL,
                detail TEXT
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_control_transitions_time
               ON upstream_reconciliation_control_transitions(transitioned_at, id)"#,
        )
        .execute(&self.pool)
        .await?;
        let legacy_enabled = self
            .get_meta_i64(META_KEY_UPSTREAM_PRECISE_RECONCILIATION_ENABLED_V1)
            .await?
            .unwrap_or(0)
            != 0;
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_control_state
                    (id, mode, legacy_active, transitioned_at)
               VALUES ('local', ?, ?, ?)
               ON CONFLICT(id) DO NOTHING"#,
        )
        .bind(if legacy_enabled { "active" } else { "compare" })
        .bind(i64::from(legacy_enabled))
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_control_transitions
                    (mode, action, activation_period_code, transitioned_at, detail)
               SELECT mode, 'legacy_adopted', NULL, transitioned_at, NULL
                 FROM upstream_reconciliation_control_state
                WHERE id = 'local'
                  AND NOT EXISTS (
                      SELECT 1 FROM upstream_reconciliation_control_transitions
                  )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_CONTROLLER_VERSION,
            RECONCILIATION_CONTROLLER_NAME,
            RECONCILIATION_CONTROLLER_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_migration(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.dashboard_alert_projection_state (
                source_kind TEXT PRIMARY KEY NOT NULL,
                cursor_occurred_at INTEGER NOT NULL DEFAULT 0,
                cursor_row_sort_id TEXT NOT NULL DEFAULT '',
                fence_occurred_at INTEGER,
                fence_row_sort_id TEXT,
                generation INTEGER NOT NULL DEFAULT 0,
                phase TEXT NOT NULL DEFAULT 'catching_up'
                    CHECK(phase IN ('catching_up', 'idle')),
                observed_at INTEGER,
                stale_reason TEXT
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.dashboard_alert_projection_events (
                source_kind TEXT NOT NULL,
                source_id TEXT NOT NULL,
                occurred_at INTEGER NOT NULL,
                row_sort_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                projected_at INTEGER NOT NULL,
                PRIMARY KEY(source_kind, source_id)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_dashboard_alert_projection_events_time
               ON dashboard_alert_projection_events(occurred_at DESC, row_sort_id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        for source_kind in ALERT_PROJECTION_SOURCES {
            sqlx::query(
                r#"INSERT INTO observability.dashboard_alert_projection_state (source_kind)
                   VALUES (?) ON CONFLICT(source_kind) DO NOTHING"#,
            )
            .bind(source_kind)
            .execute(&self.pool)
            .await?;
        }
        self.record_schema_migration(
            ALERT_PROJECTION_VERSION,
            ALERT_PROJECTION_NAME,
            ALERT_PROJECTION_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_recent_window_migration(&self) -> Result<(), ProxyError> {
        // The first rollout consumed a bounded Dashboard window. The later
        // administrator-history migration deliberately resets this cursor to
        // zero so Events and Groups can switch only after full coverage. A
        // nonzero cursor is already owned by a prior projection and must not
        // be moved by this compatibility migration itself.
        let cursor_start = self
            .backend_time
            .now_ts()
            .saturating_sub(ALERT_PROJECTION_RECENT_WINDOW_SECS);
        sqlx::query(
            r#"UPDATE observability.dashboard_alert_projection_state
                   SET cursor_occurred_at = ?, cursor_row_sort_id = '',
                       fence_occurred_at = NULL, fence_row_sort_id = NULL,
                       phase = 'catching_up', observed_at = NULL, stale_reason = NULL
                 WHERE cursor_occurred_at = 0
                   AND cursor_row_sort_id = ''
                   AND generation = 0"#,
        )
        .bind(cursor_start)
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ALERT_PROJECTION_RECENT_WINDOW_VERSION,
            ALERT_PROJECTION_RECENT_WINDOW_NAME,
            ALERT_PROJECTION_RECENT_WINDOW_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_admin_history_migration(&self) -> Result<(), ProxyError> {
        // Dashboard freshness and administrator completeness have different
        // progress contracts. Preserve the recent tail cursor and start an
        // independent historical backfill below its current boundary. That
        // keeps the last 30-day read model complete while Events and Groups
        // catch up older rows without a startup scan or sidecar rewrite.
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.dashboard_alert_projection_history_state (
                source_kind TEXT PRIMARY KEY NOT NULL,
                cursor_occurred_at INTEGER NOT NULL DEFAULT 0,
                cursor_row_sort_id TEXT NOT NULL DEFAULT '',
                fence_occurred_at INTEGER,
                fence_row_sort_id TEXT,
                generation INTEGER NOT NULL DEFAULT 0,
                phase TEXT NOT NULL DEFAULT 'catching_up'
                    CHECK(phase IN ('catching_up', 'idle'))
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"INSERT INTO observability.dashboard_alert_projection_history_state
                    (source_kind, cursor_occurred_at, cursor_row_sort_id,
                     fence_occurred_at, fence_row_sort_id, generation, phase)
               SELECT source_kind,
                      0,
                      '',
                      CASE WHEN cursor_occurred_at > 0 THEN cursor_occurred_at - 1 ELSE NULL END,
                      CASE WHEN cursor_occurred_at > 0 THEN '' ELSE NULL END,
                      0,
                      CASE WHEN cursor_occurred_at > 0 THEN 'catching_up' ELSE 'idle' END
                 FROM observability.dashboard_alert_projection_state
                WHERE 1 = 1
               ON CONFLICT(source_kind) DO NOTHING"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ALERT_PROJECTION_ADMIN_HISTORY_VERSION,
            ALERT_PROJECTION_ADMIN_HISTORY_NAME,
            ALERT_PROJECTION_ADMIN_HISTORY_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_admin_history_fence_repair_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // Version 14 gave the second immediately before the recent-tail
        // boundary to neither lane. The admin sidecar is derived state, so
        // reset only its durable history cursors. Replaying in background
        // micro-slices is idempotent and avoids any startup source scan.
        sqlx::query(
            r#"UPDATE observability.dashboard_alert_projection_history_state AS history
                   SET cursor_occurred_at = 0,
                       cursor_row_sort_id = '',
                       fence_occurred_at = CASE
                           WHEN tail.cursor_occurred_at > 0 THEN tail.cursor_occurred_at
                           ELSE NULL
                       END,
                       fence_row_sort_id = CASE
                           WHEN tail.cursor_occurred_at > 0 THEN ''
                           ELSE NULL
                       END,
                       generation = history.generation + 1,
                       phase = CASE
                           WHEN tail.cursor_occurred_at > 0 THEN 'catching_up'
                           ELSE 'idle'
                       END
                  FROM observability.dashboard_alert_projection_state AS tail
                 WHERE tail.source_kind = history.source_kind"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_VERSION,
            ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_NAME,
            ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_summary_migration(&self) -> Result<(), ProxyError> {
        // Recent-alert summaries are derived, bounded Dashboard read-model data.
        // The projection worker fills the table after startup; migration never
        // scans alert sources or performs a sidecar backfill.
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS observability.dashboard_alert_projection_recent_summaries (
                window_hours INTEGER PRIMARY KEY NOT NULL,
                source_generation INTEGER NOT NULL,
                computed_at INTEGER NOT NULL,
                summary_json TEXT NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ALERT_PROJECTION_SUMMARY_VERSION,
            ALERT_PROJECTION_SUMMARY_NAME,
            ALERT_PROJECTION_SUMMARY_CHECKSUM,
        )
        .await
    }

    async fn apply_alert_projection_exact_boundary_repair_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // The recent tail owns all source ids at its cursor second because the
        // tail cursor is the low sentinel. History must therefore stop at the
        // previous second; a fence at the cursor second with an empty id loses
        // the exact-boundary ids from both lanes.
        sqlx::query(
            r#"UPDATE observability.dashboard_alert_projection_history_state AS history
                   SET cursor_occurred_at = 0,
                       cursor_row_sort_id = '',
                       fence_occurred_at = CASE
                           WHEN tail.cursor_occurred_at > 0
                               THEN tail.cursor_occurred_at - 1
                           ELSE NULL
                       END,
                       fence_row_sort_id = CASE
                           WHEN tail.cursor_occurred_at > 0 THEN ''
                           ELSE NULL
                       END,
                       generation = history.generation + 1,
                       phase = CASE
                           WHEN tail.cursor_occurred_at > 0 THEN 'catching_up'
                           ELSE 'idle'
                       END
                  FROM observability.dashboard_alert_projection_state AS tail
                 WHERE tail.source_kind = history.source_kind"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_VERSION,
            ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_NAME,
            ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_transport_observation_migration(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .table_column_exists(
                "upstream_reconciliation_run_observation",
                "last_transport_kind",
            )
            .await?
        {
            sqlx::query(
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN last_transport_kind TEXT",
            )
            .execute(&self.pool)
            .await?;
        }
        self.record_schema_migration(
            RECONCILIATION_TRANSPORT_OBSERVATION_VERSION,
            RECONCILIATION_TRANSPORT_OBSERVATION_NAME,
            RECONCILIATION_TRANSPORT_OBSERVATION_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_transport_state_migration(&self) -> Result<(), ProxyError> {
        if !self
            .table_column_exists(
                "upstream_reconciliation_run_observation",
                "last_transport_kind_at",
            )
            .await?
        {
            sqlx::query(
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN last_transport_kind_at INTEGER",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self
            .table_column_exists(
                "upstream_reconciliation_run_observation",
                "last_retryable_outcome",
            )
            .await?
        {
            sqlx::query(
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN last_retryable_outcome TEXT",
            )
            .execute(&self.pool)
            .await?;
        }
        self.record_schema_migration(
            RECONCILIATION_TRANSPORT_STATE_VERSION,
            RECONCILIATION_TRANSPORT_STATE_NAME,
            RECONCILIATION_TRANSPORT_STATE_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_research_progress_window_migration(
        &self,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_research_progress_window (
                id TEXT PRIMARY KEY NOT NULL CHECK(id = 'local'),
                active_period_start INTEGER NOT NULL DEFAULT 0,
                active_started_at INTEGER NOT NULL DEFAULT 0,
                baseline_terminal_count INTEGER NOT NULL DEFAULT 0,
                baseline_pending_count INTEGER NOT NULL DEFAULT 0,
                last_window_started_at INTEGER,
                last_window_ended_at INTEGER,
                last_window_terminal_delta INTEGER NOT NULL DEFAULT 0,
                last_window_pending_delta INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO upstream_reconciliation_research_progress_window (id) VALUES ('local') ON CONFLICT(id) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_RESEARCH_PROGRESS_WINDOW_VERSION,
            RECONCILIATION_RESEARCH_PROGRESS_WINDOW_NAME,
            RECONCILIATION_RESEARCH_PROGRESS_WINDOW_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_research_selection_migration(
        &self,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_research_scan_state (
                id TEXT PRIMARY KEY NOT NULL CHECK(id = 'local'),
                cursor_next_poll_at INTEGER NOT NULL DEFAULT -1,
                cursor_key_id TEXT NOT NULL DEFAULT '',
                cursor_request_id TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO upstream_reconciliation_research_scan_state (id) VALUES ('local') ON CONFLICT(id) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_research_due_scan ON upstream_reconciliation_research (terminal_at, next_poll_at, key_id, request_id)",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_RESEARCH_SELECTION_VERSION,
            RECONCILIATION_RESEARCH_SELECTION_NAME,
            RECONCILIATION_RESEARCH_SELECTION_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_key_observation_migration(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS upstream_reconciliation_key_observations (
                token_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                work_generation INTEGER NOT NULL,
                key_id TEXT NOT NULL,
                upstream_usage INTEGER NOT NULL,
                observed_at INTEGER NOT NULL,
                PRIMARY KEY (token_id, period_code, work_generation, key_id)
            )"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_reconciliation_key_observations_generation \
             ON upstream_reconciliation_key_observations(token_id, period_code, work_generation, key_id)",
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_KEY_OBSERVATION_VERSION,
            RECONCILIATION_KEY_OBSERVATION_NAME,
            RECONCILIATION_KEY_OBSERVATION_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_observation_metrics_migration(&self) -> Result<(), ProxyError> {
        for (column, definition) in [
            (
                "partial_key_observation_count",
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN partial_key_observation_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "multi_key_pending_count",
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN multi_key_pending_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "remote_attempt_budget_defer_count",
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN remote_attempt_budget_defer_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "resumed_run_count",
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN resumed_run_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "terminal_run_count",
                "ALTER TABLE upstream_reconciliation_run_observation ADD COLUMN terminal_run_count INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_run_observation", column)
                .await?
            {
                sqlx::query(definition).execute(&self.pool).await?;
            }
        }
        self.record_schema_migration(
            RECONCILIATION_OBSERVATION_METRICS_VERSION,
            RECONCILIATION_OBSERVATION_METRICS_NAME,
            RECONCILIATION_OBSERVATION_METRICS_CHECKSUM,
        )
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

    async fn apply_reconciliation_outcome_migration(&self) -> Result<(), ProxyError> {
        for (column, definition) in [
            ("work_generation", "INTEGER NOT NULL DEFAULT 1"),
            ("completed_generation", "INTEGER NOT NULL DEFAULT 0"),
            ("next_attempt_at", "INTEGER NOT NULL DEFAULT 0"),
            ("last_outcome", "TEXT"),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_work", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE upstream_reconciliation_work ADD COLUMN {column} {definition}",
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_work_pending ON upstream_reconciliation_work(next_attempt_at, period_end, scheduling_key_id, token_id, period_code)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("DROP TRIGGER IF EXISTS trg_upstream_reconciliation_usage_work_insert")
            .execute(&self.pool)
            .await?;
        sqlx::query("DROP TRIGGER IF EXISTS trg_upstream_reconciliation_usage_work_update")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"CREATE TRIGGER IF NOT EXISTS trg_upstream_reconciliation_usage_work_insert
               AFTER INSERT ON upstream_reconciliation_usage
               BEGIN
                 INSERT INTO upstream_reconciliation_work (
                   token_id, period_code, project_id, billing_subject, settlement_mode,
                   period_start, period_end, scheduling_key_id, updated_at,
                   work_generation, completed_generation, next_attempt_at, last_outcome
                 ) VALUES (
                   NEW.token_id, NEW.period_code, NEW.project_id, NEW.billing_subject,
                   NEW.settlement_mode, NEW.period_start, NEW.period_end, NEW.key_id, NEW.updated_at,
                   1, 0, 0, NULL
                 )
                 ON CONFLICT(token_id, period_code) DO UPDATE SET
                   project_id = MIN(upstream_reconciliation_work.project_id, excluded.project_id),
                   billing_subject = MIN(upstream_reconciliation_work.billing_subject, excluded.billing_subject),
                   settlement_mode = MIN(upstream_reconciliation_work.settlement_mode, excluded.settlement_mode),
                   period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                   period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                   scheduling_key_id = MIN(upstream_reconciliation_work.scheduling_key_id, excluded.scheduling_key_id),
                   updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at),
                   work_generation = upstream_reconciliation_work.work_generation + 1,
                   next_attempt_at = 0,
                   last_outcome = NULL;
               END"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE TRIGGER IF NOT EXISTS trg_upstream_reconciliation_usage_work_update
               AFTER UPDATE ON upstream_reconciliation_usage
               BEGIN
                 INSERT INTO upstream_reconciliation_work (
                   token_id, period_code, project_id, billing_subject, settlement_mode,
                   period_start, period_end, scheduling_key_id, updated_at,
                   work_generation, completed_generation, next_attempt_at, last_outcome
                 ) VALUES (
                   NEW.token_id, NEW.period_code, NEW.project_id, NEW.billing_subject,
                   NEW.settlement_mode, NEW.period_start, NEW.period_end, NEW.key_id, NEW.updated_at,
                   1, 0, 0, NULL
                 )
                 ON CONFLICT(token_id, period_code) DO UPDATE SET
                   project_id = MIN(upstream_reconciliation_work.project_id, excluded.project_id),
                   billing_subject = MIN(upstream_reconciliation_work.billing_subject, excluded.billing_subject),
                   settlement_mode = MIN(upstream_reconciliation_work.settlement_mode, excluded.settlement_mode),
                   period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                   period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                   scheduling_key_id = MIN(upstream_reconciliation_work.scheduling_key_id, excluded.scheduling_key_id),
                   updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at),
                   work_generation = upstream_reconciliation_work.work_generation + 1,
                   next_attempt_at = 0,
                   last_outcome = NULL;
               END"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"UPDATE upstream_reconciliation_work
               SET completed_generation = work_generation,
                   last_outcome = CASE
                       WHEN EXISTS (
                           SELECT 1 FROM upstream_reconciliation_settlements s
                           WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                             AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                       ) THEN 'settled'
                       ELSE last_outcome
                   END
               WHERE EXISTS (
                   SELECT 1 FROM upstream_reconciliation_settlements s
                   WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                     AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
               )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_OUTCOME_VERSION,
            RECONCILIATION_OUTCOME_NAME,
            RECONCILIATION_OUTCOME_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_terminal_refresh_migration(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE upstream_reconciliation_work
               SET completed_generation = 0,
                   last_outcome = NULL
               WHERE completed_generation >= work_generation
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_settlements s
                     WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                       AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                 )
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_usage u
                     WHERE u.token_id = upstream_reconciliation_work.token_id
                       AND u.period_code = upstream_reconciliation_work.period_code
                       AND u.updated_at > COALESCE((
                           SELECT MAX(s.settled_at)
                           FROM upstream_reconciliation_settlements s
                           WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                             AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                       ), 0)
                 )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_TERMINAL_REFRESH_VERSION,
            RECONCILIATION_TERMINAL_REFRESH_NAME,
            RECONCILIATION_TERMINAL_REFRESH_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_terminal_same_second_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // Historical usage and settlement timestamps have only second precision. Reopen equal
        // timestamps conservatively: a duplicate verification is safer than losing a usage
        // update that happened in the same second as settlement.
        sqlx::query(
            r#"UPDATE upstream_reconciliation_work
               SET completed_generation = 0,
                   last_outcome = NULL
               WHERE completed_generation >= work_generation
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_settlements s
                     WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                       AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                 )
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_usage u
                     WHERE u.token_id = upstream_reconciliation_work.token_id
                       AND u.period_code = upstream_reconciliation_work.period_code
                       AND u.updated_at = COALESCE((
                           SELECT MAX(s.settled_at)
                           FROM upstream_reconciliation_settlements s
                           WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                             AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                       ), 0)
                 )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_TERMINAL_SAME_SECOND_VERSION,
            RECONCILIATION_TERMINAL_SAME_SECOND_NAME,
            RECONCILIATION_TERMINAL_SAME_SECOND_CHECKSUM,
        )
        .await
    }

    async fn apply_reconciliation_projection_lifecycle_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // This is an O(1) startup classification. Existing usage is projected in bounded
        // post-start slices; an empty new database needs no historical projection at all.
        let has_usage: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM upstream_reconciliation_usage LIMIT 1)",
        )
        .fetch_one(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO NOTHING",
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1)
        .bind(if has_usage { "0" } else { "1" })
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_PROJECTION_LIFECYCLE_VERSION,
            RECONCILIATION_PROJECTION_LIFECYCLE_NAME,
            RECONCILIATION_PROJECTION_LIFECYCLE_CHECKSUM,
        )
        .await
    }

    pub(crate) async fn prepare_versioned_schema(&self) -> Result<bool, ProxyError> {
        let started = std::time::Instant::now();
        let existing_database = self.schema_object_exists("main", "meta").await?;
        if !existing_database {
            if self
                .schema_object_exists("main", "schema_migrations")
                .await?
            {
                return Err(ProxyError::Other(
                    "schema migration adoption rejected: schema_migrations exists without main.meta"
                        .to_string(),
                ));
            }
            if self.new_database_bootstrap_in_progress().await? {
                return Ok(true);
            }
            if self.schema_has_domain_rows_without_meta().await? {
                return Err(ProxyError::Other(
                    "schema migration adoption rejected: domain data exists without main.meta"
                        .to_string(),
                ));
            }
            self.begin_new_database_bootstrap().await?;
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
        if !ledger_preexisting {
            self.validate_schema_adoption_source().await?;
            return Ok(true);
        }
        if interrupted_adoption {
            self.validate_schema_baseline().await?;
            return Ok(true);
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
        if !self
            .schema_migration_applied(RECONCILIATION_OUTCOME_VERSION)
            .await?
        {
            self.apply_reconciliation_outcome_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_TERMINAL_REFRESH_VERSION)
            .await?
        {
            self.apply_reconciliation_terminal_refresh_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_TERMINAL_SAME_SECOND_VERSION)
            .await?
        {
            self.apply_reconciliation_terminal_same_second_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_PROJECTION_LIFECYCLE_VERSION)
            .await?
        {
            self.apply_reconciliation_projection_lifecycle_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(HA_GC_LEGACY_CURSOR_VERSION)
            .await?
        {
            self.apply_ha_gc_legacy_cursor_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_ENGINE_STATE_VERSION)
            .await?
        {
            self.apply_reconciliation_engine_state_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_OUTCOME_REPAIR_VERSION)
            .await?
        {
            self.apply_reconciliation_outcome_repair_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_CONTROLLER_VERSION)
            .await?
        {
            self.apply_reconciliation_controller_migration().await?;
        }
        if !self.schema_migration_applied(ALERT_PROJECTION_VERSION).await? {
            self.apply_alert_projection_migration().await?;
        }
        if !self
            .schema_migration_applied(ALERT_PROJECTION_RECENT_WINDOW_VERSION)
            .await?
        {
            self.apply_alert_projection_recent_window_migration().await?;
        }
        if !self
            .schema_migration_applied(ALERT_PROJECTION_ADMIN_HISTORY_VERSION)
            .await?
        {
            self.apply_alert_projection_admin_history_migration().await?;
        }
        if !self
            .schema_migration_applied(ALERT_PROJECTION_ADMIN_HISTORY_FENCE_REPAIR_VERSION)
            .await?
        {
            self.apply_alert_projection_admin_history_fence_repair_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(ALERT_PROJECTION_SUMMARY_VERSION)
            .await?
        {
            self.apply_alert_projection_summary_migration().await?;
        }
        if !self
            .schema_migration_applied(ALERT_PROJECTION_EXACT_BOUNDARY_REPAIR_VERSION)
            .await?
        {
            self.apply_alert_projection_exact_boundary_repair_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_TRANSPORT_OBSERVATION_VERSION)
            .await?
        {
            self.apply_reconciliation_transport_observation_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_TRANSPORT_STATE_VERSION)
            .await?
        {
            self.apply_reconciliation_transport_state_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_RESEARCH_PROGRESS_WINDOW_VERSION)
            .await?
        {
            self.apply_reconciliation_research_progress_window_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_RESEARCH_SELECTION_VERSION)
            .await?
        {
            self.apply_reconciliation_research_selection_migration()
                .await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_KEY_OBSERVATION_VERSION)
            .await?
        {
            self.apply_reconciliation_key_observation_migration().await?;
        }
        if !self
            .schema_migration_applied(RECONCILIATION_OBSERVATION_METRICS_VERSION)
            .await?
        {
            self.apply_reconciliation_observation_metrics_migration()
                .await?;
        }
        self.validate_applied_migration_objects().await?;
        self.clear_new_database_bootstrap_marker().await?;
        tracing::debug!(
            component = "schema_migration",
            event = "verified",
            outcome = "current",
            elapsed_ms = started.elapsed().as_millis() as u64,
        );
        Ok(false)
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
        self.apply_reconciliation_outcome_migration().await?;
        self.apply_reconciliation_terminal_refresh_migration().await?;
        self.apply_reconciliation_terminal_same_second_migration()
            .await?;
        self.apply_reconciliation_projection_lifecycle_migration()
            .await?;
        self.apply_ha_gc_legacy_cursor_migration().await?;
        self.apply_reconciliation_engine_state_migration().await?;
        // A freshly bootstrapped database has no historical shadow outcomes
        // to repair, so do not manufacture projection debt on first startup.
        self.record_schema_migration(
            RECONCILIATION_OUTCOME_REPAIR_VERSION,
            RECONCILIATION_OUTCOME_REPAIR_NAME,
            RECONCILIATION_OUTCOME_REPAIR_CHECKSUM,
        )
        .await?;
        self.apply_reconciliation_controller_migration().await?;
        self.apply_alert_projection_migration().await?;
        self.apply_alert_projection_recent_window_migration().await?;
        self.apply_alert_projection_admin_history_migration().await?;
        self.apply_alert_projection_admin_history_fence_repair_migration()
            .await?;
        self.apply_alert_projection_summary_migration().await?;
        self.apply_alert_projection_exact_boundary_repair_migration()
            .await?;
        self.apply_reconciliation_transport_observation_migration()
            .await?;
        self.apply_reconciliation_transport_state_migration().await?;
        self.apply_reconciliation_research_progress_window_migration()
            .await?;
        self.apply_reconciliation_research_selection_migration()
            .await?;
        self.apply_reconciliation_key_observation_migration().await?;
        self.apply_reconciliation_observation_metrics_migration()
            .await?;
        self.validate_applied_migration_objects().await?;
        self.clear_new_database_bootstrap_marker().await?;
        tracing::info!(
            component = "schema_migration",
            event = "baseline_adopted",
            outcome = "applied",
            elapsed_ms = started.elapsed().as_millis() as u64,
            migration_count = 23_i64,
        );
        Ok(())
    }
}
