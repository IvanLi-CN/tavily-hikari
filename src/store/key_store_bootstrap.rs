impl KeyStore {
    #[cfg(test)]
    async fn acquire_test_schema_init_guard() -> tokio::sync::OwnedSemaphorePermit {
        static LOCK: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
            std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)))
            .clone()
            .acquire_owned()
            .await
            .expect("test schema init semaphore closed")
    }

    async fn main_table_exists(&self, table: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn table_has_foreign_key_to(
        &self,
        table: &str,
        parent_table: &str,
    ) -> Result<bool, ProxyError> {
        if !self.main_table_exists(table).await? {
            return Ok(false);
        }

        let rows = sqlx::query(&format!("PRAGMA foreign_key_list({table})"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().any(|row| {
            row.try_get::<String, _>("table")
                .map(|value| value == parent_table)
                .unwrap_or(false)
        }))
    }

    fn legacy_request_logs_select_exprs(source_columns: &HashSet<String>) -> Vec<(&'static str, String)> {
        let has = |column: &str| source_columns.contains(column);
        let source = |column: &str| format!("legacy.{column}");
        let nullable_source = |column: &str| {
            if has(column) {
                source(column)
            } else {
                "NULL".to_string()
            }
        };

        vec![
            ("id", "legacy.id".to_string()),
            (
                "api_key_id",
                if has("api_key_id") {
                    source("api_key_id")
                } else if has("api_key") {
                    "(SELECT id FROM api_keys WHERE api_keys.api_key = legacy.api_key LIMIT 1)"
                        .to_string()
                } else {
                    "NULL".to_string()
                },
            ),
            ("auth_token_id", nullable_source("auth_token_id")),
            ("request_user_id", nullable_source("request_user_id")),
            ("method", source("method")),
            ("path", source("path")),
            ("query", nullable_source("query")),
            ("status_code", nullable_source("status_code")),
            ("tavily_status_code", nullable_source("tavily_status_code")),
            ("error_message", nullable_source("error_message")),
            (
                "result_status",
                if has("result_status") {
                    source("result_status")
                } else {
                    "'unknown'".to_string()
                },
            ),
            ("request_kind_key", nullable_source("request_kind_key")),
            ("request_kind_label", nullable_source("request_kind_label")),
            ("request_kind_detail", nullable_source("request_kind_detail")),
            ("counts_business_quota", nullable_source("counts_business_quota")),
            ("business_credits", nullable_source("business_credits")),
            ("failure_kind", nullable_source("failure_kind")),
            (
                "key_effect_code",
                if has("key_effect_code") {
                    source("key_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            ("key_effect_summary", nullable_source("key_effect_summary")),
            (
                "binding_effect_code",
                if has("binding_effect_code") {
                    source("binding_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            (
                "binding_effect_summary",
                nullable_source("binding_effect_summary"),
            ),
            (
                "selection_effect_code",
                if has("selection_effect_code") {
                    source("selection_effect_code")
                } else {
                    "'none'".to_string()
                },
            ),
            (
                "selection_effect_summary",
                nullable_source("selection_effect_summary"),
            ),
            ("gateway_mode", nullable_source("gateway_mode")),
            ("experiment_variant", nullable_source("experiment_variant")),
            ("proxy_session_id", nullable_source("proxy_session_id")),
            ("routing_subject_hash", nullable_source("routing_subject_hash")),
            ("upstream_operation", nullable_source("upstream_operation")),
            ("fallback_reason", nullable_source("fallback_reason")),
            ("request_body", nullable_source("request_body")),
            ("response_body", nullable_source("response_body")),
            ("request_body_bytes", nullable_source("request_body_bytes")),
            ("response_body_bytes", nullable_source("response_body_bytes")),
            ("request_body_sha256", nullable_source("request_body_sha256")),
            ("response_body_sha256", nullable_source("response_body_sha256")),
            ("body_retention_days", nullable_source("body_retention_days")),
            ("body_retention_profile", nullable_source("body_retention_profile")),
            ("body_cleaned_reason", nullable_source("body_cleaned_reason")),
            ("body_cleaned_at", nullable_source("body_cleaned_at")),
            ("forwarded_headers", nullable_source("forwarded_headers")),
            ("dropped_headers", nullable_source("dropped_headers")),
            ("remote_addr", nullable_source("remote_addr")),
            ("client_ip", nullable_source("client_ip")),
            ("client_ip_source", nullable_source("client_ip_source")),
            (
                "client_ip_trusted",
                if has("client_ip_trusted") {
                    source("client_ip_trusted")
                } else {
                    "0".to_string()
                },
            ),
            ("ip_headers", nullable_source("ip_headers")),
            (
                "visibility",
                if has("visibility") {
                    source("visibility")
                } else {
                    format!("'{}'", REQUEST_LOG_VISIBILITY_VISIBLE)
                },
            ),
            ("created_at", source("created_at")),
        ]
    }

    async fn copy_legacy_request_logs_into_observability(&self) -> Result<(), ProxyError> {
        if !self.main_table_exists("request_logs").await? {
            return Ok(());
        }

        let source_columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('request_logs', 'main')",
        )
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        if source_columns.is_empty() {
            return Ok(());
        }

        let select_exprs = Self::legacy_request_logs_select_exprs(&source_columns);
        let target_columns = select_exprs
            .iter()
            .map(|(column, _)| *column)
            .collect::<Vec<_>>();
        let source_exprs = select_exprs
            .iter()
            .map(|(_, expr)| expr.as_str())
            .collect::<Vec<_>>();

        let copy_sql = format!(
            r#"
            INSERT INTO observability.request_logs ({})
            SELECT {}
            FROM main.request_logs AS legacy
            WHERE NOT EXISTS (
                SELECT 1
                FROM observability.request_logs AS obs
                WHERE obs.id = legacy.id
            )
            ORDER BY legacy.id ASC
            "#,
            target_columns.join(", "),
            source_exprs.join(", "),
        );
        sqlx::query(&copy_sql).execute(&self.pool).await?;
        Ok(())
    }

    async fn rebuild_billing_ledger_without_request_log_foreign_key(
        &self,
    ) -> Result<(), ProxyError> {
        if !self.main_table_exists("billing_ledger").await? {
            return Ok(());
        }
        let has_updated_at = self.table_column_exists("billing_ledger", "updated_at").await?;
        let updated_at_select_expr = if has_updated_at {
            "updated_at"
        } else {
            "created_at"
        };

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS billing_ledger_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE billing_ledger_new (
                    auth_token_log_id INTEGER PRIMARY KEY,
                    token_id TEXT NOT NULL,
                    billing_subject TEXT,
                    billing_state TEXT NOT NULL DEFAULT 'none',
                    business_credits INTEGER,
                    request_user_id TEXT,
                    api_key_id TEXT,
                    request_log_id INTEGER,
                    result_status TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    settled_at INTEGER,
                    error_message TEXT
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            let copy_sql = format!(
                r#"
                INSERT INTO billing_ledger_new (
                    auth_token_log_id,
                    token_id,
                    billing_subject,
                    billing_state,
                    business_credits,
                    request_user_id,
                    api_key_id,
                    request_log_id,
                    result_status,
                    created_at,
                    updated_at,
                    settled_at,
                    error_message
                )
                SELECT
                    auth_token_log_id,
                    token_id,
                    billing_subject,
                    billing_state,
                    business_credits,
                    request_user_id,
                    api_key_id,
                    request_log_id,
                    result_status,
                    created_at,
                    {updated_at_select_expr},
                    settled_at,
                    error_message
                FROM billing_ledger
                "#
            );
            sqlx::query(&copy_sql).execute(&mut *conn).await?;
            sqlx::query("DROP TABLE billing_ledger")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE billing_ledger_new RENAME TO billing_ledger")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        let reenable = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        rebuild_result?;
        reenable?;
        self.ensure_billing_ledger_indexes().await?;
        Ok(())
    }

    async fn ensure_billing_ledger_ha_shape(&self) -> Result<(), ProxyError> {
        let missing_updated_at = self.main_table_exists("billing_ledger").await?
            && !self.table_column_exists("billing_ledger", "updated_at").await?;
        let has_auth_token_fk = self
            .table_has_foreign_key_to("billing_ledger", "auth_token_logs")
            .await?;
        if !missing_updated_at && !has_auth_token_fk {
            return Ok(());
        }
        self.rebuild_billing_ledger_without_request_log_foreign_key()
            .await
    }

    async fn rebuild_ha_peer_watermarks_for_channels(&self) -> Result<(), ProxyError> {
        if !self.main_table_exists("ha_peer_watermarks").await? {
            return Ok(());
        }

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS ha_peer_watermarks_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE ha_peer_watermarks_new (
                    peer_node_id TEXT NOT NULL,
                    channel TEXT NOT NULL DEFAULT 'control',
                    acked_seq INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (peer_node_id, channel)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO ha_peer_watermarks_new (peer_node_id, channel, acked_seq, updated_at)
                SELECT
                    peer_node_id,
                    COALESCE(channel, 'control'),
                    acked_seq,
                    updated_at
                FROM ha_peer_watermarks
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE ha_peer_watermarks")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE ha_peer_watermarks_new RENAME TO ha_peer_watermarks")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        let reenable = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        rebuild_result?;
        reenable?;
        Ok(())
    }

    async fn ensure_ha_peer_watermarks_shape(&self) -> Result<(), ProxyError> {
        if !self.main_table_exists("ha_peer_watermarks").await? {
            return Ok(());
        }
        let missing_channel = !self.table_column_exists("ha_peer_watermarks", "channel").await?;
        if missing_channel {
            return self.rebuild_ha_peer_watermarks_for_channels().await;
        }

        let pk_rows = sqlx::query(
            "SELECT name, pk FROM pragma_table_info('ha_peer_watermarks') WHERE pk > 0 ORDER BY pk ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        let pk_columns = pk_rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("name"))
            .collect::<Result<Vec<_>, _>>()?;
        if pk_columns != vec!["peer_node_id".to_string(), "channel".to_string()] {
            return self.rebuild_ha_peer_watermarks_for_channels().await;
        }
        Ok(())
    }

    async fn rebuild_api_key_maintenance_records_without_request_log_foreign_key(
        &self,
    ) -> Result<(), ProxyError> {
        if !self.main_table_exists("api_key_maintenance_records").await? {
            return Ok(());
        }

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS api_key_maintenance_records_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE api_key_maintenance_records_new (
                    id TEXT PRIMARY KEY,
                    key_id TEXT NOT NULL,
                    source TEXT NOT NULL,
                    operation_code TEXT NOT NULL,
                    operation_summary TEXT NOT NULL,
                    reason_code TEXT,
                    reason_summary TEXT,
                    reason_detail TEXT,
                    request_log_id INTEGER,
                    auth_token_log_id INTEGER,
                    auth_token_id TEXT,
                    actor_user_id TEXT,
                    actor_display_name TEXT,
                    status_before TEXT,
                    status_after TEXT,
                    quarantine_before INTEGER NOT NULL DEFAULT 0,
                    quarantine_after INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY (key_id) REFERENCES api_keys(id),
                    FOREIGN KEY (auth_token_id) REFERENCES auth_tokens(id),
                    FOREIGN KEY (auth_token_log_id) REFERENCES auth_token_logs(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO api_key_maintenance_records_new (
                    id,
                    key_id,
                    source,
                    operation_code,
                    operation_summary,
                    reason_code,
                    reason_summary,
                    reason_detail,
                    request_log_id,
                    auth_token_log_id,
                    auth_token_id,
                    actor_user_id,
                    actor_display_name,
                    status_before,
                    status_after,
                    quarantine_before,
                    quarantine_after,
                    created_at
                )
                SELECT
                    id,
                    key_id,
                    source,
                    operation_code,
                    operation_summary,
                    reason_code,
                    reason_summary,
                    reason_detail,
                    request_log_id,
                    auth_token_log_id,
                    auth_token_id,
                    actor_user_id,
                    actor_display_name,
                    status_before,
                    status_after,
                    quarantine_before,
                    quarantine_after,
                    created_at
                FROM api_key_maintenance_records
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE api_key_maintenance_records")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                "ALTER TABLE api_key_maintenance_records_new RENAME TO api_key_maintenance_records",
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        let reenable = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        rebuild_result?;
        reenable?;
        self.ensure_api_key_maintenance_records_schema().await?;
        Ok(())
    }

    async fn rebuild_api_key_transient_backoffs_without_request_log_foreign_key(
        &self,
    ) -> Result<(), ProxyError> {
        if !self.main_table_exists("api_key_transient_backoffs").await? {
            return Ok(());
        }

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS api_key_transient_backoffs_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE api_key_transient_backoffs_new (
                    key_id TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    cooldown_until INTEGER NOT NULL,
                    retry_after_secs INTEGER NOT NULL,
                    reason_code TEXT,
                    source_request_log_id INTEGER,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (key_id, scope),
                    FOREIGN KEY (key_id) REFERENCES api_keys(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO api_key_transient_backoffs_new (
                    key_id,
                    scope,
                    cooldown_until,
                    retry_after_secs,
                    reason_code,
                    source_request_log_id,
                    created_at,
                    updated_at
                )
                SELECT
                    key_id,
                    scope,
                    cooldown_until,
                    retry_after_secs,
                    reason_code,
                    source_request_log_id,
                    created_at,
                    updated_at
                FROM api_key_transient_backoffs
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE api_key_transient_backoffs")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                "ALTER TABLE api_key_transient_backoffs_new RENAME TO api_key_transient_backoffs",
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        let reenable = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        rebuild_result?;
        reenable?;
        self.ensure_api_key_transient_backoffs_schema().await?;
        Ok(())
    }

    fn uses_legacy_single_db_observability_compatibility(&self) -> bool {
        let Some(observability_database_path) = self.observability_database_path.as_deref() else {
            return false;
        };
        sqlite_paths_match(&self.database_path, observability_database_path)
    }

    async fn ensure_billing_ledger_indexes(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_billing_ledger_state_subject
               ON billing_ledger(billing_state, billing_subject, auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_billing_ledger_token_state
               ON billing_ledger(token_id, billing_state, auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_billing_ledger_charged_window
               ON billing_ledger(billing_state, created_at, auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_billing_ledger_updated_at
               ON billing_ledger(updated_at, auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_billing_ledger_request_log
               ON billing_ledger(request_log_id, auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn billing_ledger_startup_repair_upper_bound(&self) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT MAX(id)
            FROM auth_token_logs
            WHERE billing_state <> 'none'
               OR billing_subject IS NOT NULL
               OR business_credits IS NOT NULL
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    async fn billing_ledger_startup_repair_needs_reconcile(
        &self,
        upper_bound: i64,
    ) -> Result<bool, ProxyError> {
        let persisted_high_watermark = self
            .get_meta_i64(META_KEY_BILLING_LEDGER_STARTUP_HIGH_WATERMARK_V1)
            .await?
            .unwrap_or_default();
        if persisted_high_watermark >= upper_bound {
            let has_gap = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT 1
                FROM auth_token_logs atl
                WHERE atl.id <= ?
                  AND (
                    atl.billing_state <> 'none'
                    OR atl.billing_subject IS NOT NULL
                    OR atl.business_credits IS NOT NULL
                  )
                  AND NOT EXISTS (
                    SELECT 1
                    FROM billing_ledger bl
                    WHERE bl.auth_token_log_id = atl.id
                  )
                LIMIT 1
                "#,
            )
            .bind(upper_bound)
            .fetch_optional(&self.pool)
            .await?;
            if has_gap.is_none() {
                return Ok(false);
            }
        }

        let mismatch = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM auth_token_logs atl
            LEFT JOIN billing_ledger bl ON bl.auth_token_log_id = atl.id
            WHERE atl.id <= ?
              AND (
                atl.billing_state <> 'none'
                OR atl.billing_subject IS NOT NULL
                OR atl.business_credits IS NOT NULL
              )
              AND (
                bl.auth_token_log_id IS NULL
                OR bl.token_id <> atl.token_id
                OR COALESCE(bl.billing_subject, '') <> COALESCE(atl.billing_subject, '')
                OR bl.billing_state <> atl.billing_state
                OR COALESCE(bl.business_credits, -1) <> COALESCE(atl.business_credits, -1)
                OR COALESCE(bl.request_user_id, '') <> COALESCE(atl.request_user_id, '')
                OR COALESCE(bl.api_key_id, '') <> COALESCE(atl.api_key_id, '')
                OR bl.result_status <> atl.result_status
                OR bl.created_at <> atl.created_at
                OR COALESCE(bl.error_message, '') <> COALESCE(atl.error_message, '')
              )
            LIMIT 1
            "#,
        )
        .bind(upper_bound)
        .fetch_optional(&self.pool)
        .await?;
        Ok(mismatch.is_some())
    }

    async fn maybe_repair_billing_ledger_from_auth_token_logs(&self) -> Result<(), ProxyError> {
        let Some(upper_bound) = self.billing_ledger_startup_repair_upper_bound().await? else {
            self.set_meta_i64(META_KEY_BILLING_LEDGER_STARTUP_HIGH_WATERMARK_V1, 0)
                .await?;
            eprintln!("billing ledger startup precheck skipped: upper_bound=0, reason=no_billable_logs");
            return Ok(());
        };

        let precheck_started = Instant::now();
        let needs_reconcile = self
            .billing_ledger_startup_repair_needs_reconcile(upper_bound)
            .await?;
        log_slow_db_operation(
            "billing ledger startup precheck",
            precheck_started.elapsed(),
            Some("component=startup"),
        );

        if !needs_reconcile {
            self.set_meta_i64(META_KEY_BILLING_LEDGER_STARTUP_HIGH_WATERMARK_V1, upper_bound)
                .await?;
            eprintln!(
                "billing ledger startup precheck skipped: upper_bound={upper_bound}, reason=no_gap"
            );
            return Ok(());
        }

        eprintln!("billing ledger startup repair started: upper_bound={upper_bound}");
        let repair_started = Instant::now();
        self.backfill_billing_ledger_from_auth_token_logs().await?;
        self.set_meta_i64(META_KEY_BILLING_LEDGER_STARTUP_HIGH_WATERMARK_V1, upper_bound)
            .await?;
        let elapsed = repair_started.elapsed();
        log_slow_db_operation(
            "billing ledger startup repair",
            elapsed,
            Some("component=startup"),
        );
        eprintln!(
            "billing ledger startup repair completed: upper_bound={upper_bound}, elapsed_ms={}",
            elapsed.as_millis()
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn new(database_path: &str) -> Result<Self, ProxyError> {
        Self::new_with_time(database_path, BackendTime::system()).await
    }

    pub(crate) async fn new_with_time(
        database_path: &str,
        backend_time: BackendTime,
    ) -> Result<Self, ProxyError> {
        #[cfg(test)]
        let _schema_init_guard = Self::acquire_test_schema_init_guard().await;
        let startup_started = Instant::now();
        let layout = SqliteDatabaseLayout::from_database_path(database_path);
        let observability_lock =
            acquire_observability_service_shared_lock(&layout.core_database_path)?;
        let startup_context =
            sqlite_runtime_log_context(&layout.core_database_path, "startup", false, true);
        let pool = instrument_db_operation(
            "sqlite startup open pool",
            Some(startup_context.as_str()),
            open_sqlite_pool_with_observability(
                &layout.core_database_path,
                layout.observability_database_path.as_deref(),
                true,
                false,
            ),
        )
        .await?;
        let observability_database_path = attached_database_path(&pool, "observability").await?;
        if let Some(attached_path) = observability_database_path.as_deref()
            && sqlite_paths_match(&layout.core_database_path, attached_path)
            && layout.observability_database_path.as_deref() != Some(attached_path)
        {
            eprintln!(
                "observability startup migration deferred: using legacy single-db request_logs for {} because inline sidecar migration exceeded the startup budget",
                layout.core_database_path
            );
        }
        let store = Self {
            database_path: layout.core_database_path.clone(),
            observability_database_path,
            _observability_lock: Some(observability_lock),
            pool,
            backend_time,
            token_binding_cache: RwLock::new(HashMap::new()),
            account_quota_resolution_cache: RwLock::new(HashMap::new()),
            request_logs_catalog_cache: RwLock::new(HashMap::new()),
            request_log_retention_cache: RwLock::new(None),
            user_debug_info_shared_cache: RwLock::new(HashMap::new()),
            request_stats_coalescer: RequestStatsCoalescer::default(),
            admin_heavy_read_semaphore: Semaphore::new(ADMIN_HEAVY_READ_CONCURRENCY),
            #[cfg(test)]
            forced_pending_claim_miss_log_ids: Mutex::new(HashSet::new()),
            forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(HashSet::new()),
        };
        instrument_db_operation(
            "sqlite startup initialize schema",
            Some(startup_context.as_str()),
            store.initialize_schema(),
        )
        .await?;
        log_slow_db_operation(
            "sqlite startup total",
            startup_started.elapsed(),
            Some(startup_context.as_str()),
        );
        Ok(store)
    }

    pub(crate) async fn open_for_request_logs_gc(
        database_path: &str,
    ) -> Result<Self, ProxyError> {
        Self::open_for_request_logs_gc_with_time(database_path, BackendTime::system()).await
    }

    pub(crate) async fn open_for_request_logs_gc_with_time(
        database_path: &str,
        backend_time: BackendTime,
    ) -> Result<Self, ProxyError> {
        #[cfg(test)]
        let _schema_init_guard = Self::acquire_test_schema_init_guard().await;
        let layout = SqliteDatabaseLayout::from_database_path(database_path);
        let observability_lock =
            acquire_observability_service_shared_lock(&layout.core_database_path)?;
        let gc_context =
            sqlite_runtime_log_context(&layout.core_database_path, "request-logs-gc", false, true);
        let pool = instrument_db_operation(
            "sqlite request logs gc open pool",
            Some(gc_context.as_str()),
            open_sqlite_pool_with_observability(
                &layout.core_database_path,
                layout.observability_database_path.as_deref(),
                true,
                false,
            ),
        )
        .await?;
        let observability_database_path = attached_database_path(&pool, "observability").await?;
        if let Some(attached_path) = observability_database_path.as_deref()
            && sqlite_paths_match(&layout.core_database_path, attached_path)
            && layout.observability_database_path.as_deref() != Some(attached_path)
        {
            eprintln!(
                "observability startup migration deferred: running request_logs GC against the legacy single-db layout for {}",
                layout.core_database_path
            );
        }
        let store = Self {
            database_path: layout.core_database_path.clone(),
            observability_database_path,
            _observability_lock: Some(observability_lock),
            pool,
            backend_time,
            token_binding_cache: RwLock::new(HashMap::new()),
            account_quota_resolution_cache: RwLock::new(HashMap::new()),
            request_logs_catalog_cache: RwLock::new(HashMap::new()),
            request_log_retention_cache: RwLock::new(None),
            user_debug_info_shared_cache: RwLock::new(HashMap::new()),
            request_stats_coalescer: RequestStatsCoalescer::default(),
            admin_heavy_read_semaphore: Semaphore::new(ADMIN_HEAVY_READ_CONCURRENCY),
            #[cfg(test)]
            forced_pending_claim_miss_log_ids: Mutex::new(HashSet::new()),
            forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(HashSet::new()),
        };
        instrument_db_operation(
            "sqlite request logs gc bootstrap schema",
            Some(gc_context.as_str()),
            async {
                sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT,
                auth_token_id TEXT,
                request_user_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                counts_business_quota INTEGER,
                business_credits INTEGER,
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                binding_effect_code TEXT NOT NULL DEFAULT 'none',
                binding_effect_summary TEXT,
                selection_effect_code TEXT NOT NULL DEFAULT 'none',
                selection_effect_summary TEXT,
                gateway_mode TEXT,
                experiment_variant TEXT,
                proxy_session_id TEXT,
                routing_subject_hash TEXT,
                upstream_operation TEXT,
                fallback_reason TEXT,
                request_body BLOB,
                response_body BLOB,
                request_body_bytes INTEGER,
                response_body_bytes INTEGER,
                request_body_sha256 TEXT,
                response_body_sha256 TEXT,
                body_retention_days INTEGER,
                body_retention_profile TEXT,
                body_cleaned_reason TEXT,
                body_cleaned_at INTEGER,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                remote_addr TEXT,
                client_ip TEXT,
                client_ip_source TEXT,
                client_ip_trusted INTEGER NOT NULL DEFAULT 0,
                ip_headers TEXT,
                visibility TEXT NOT NULL DEFAULT 'visible',
                created_at INTEGER NOT NULL
            )
            "#,
                )
                .execute(&store.pool)
                .await?;
                store.ensure_meta_schema().await?;
                store.ensure_users_debug_info_shared_column().await?;
                store.migrate_legacy_observability_tables_to_sidecar().await?;
                store.upgrade_request_logs_schema().await?;
                store.ensure_request_logs_gc_support_indexes().await?;
                Ok(())
            },
        )
        .await?;
        Ok(store)
    }

    pub(crate) async fn initialize_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                group_name TEXT,
                registration_ip TEXT,
                registration_region TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL DEFAULT 0,
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        self.upgrade_api_keys_schema().await?;
        self.ensure_api_key_quarantines_schema().await?;
        self.ensure_api_key_maintenance_records_schema().await?;
        self.ensure_api_key_quota_sync_samples_schema().await?;
        self.ensure_api_key_low_quota_depletions_schema().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT,
                auth_token_id TEXT,
                request_user_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                counts_business_quota INTEGER,
                business_credits INTEGER,
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                binding_effect_code TEXT NOT NULL DEFAULT 'none',
                binding_effect_summary TEXT,
                selection_effect_code TEXT NOT NULL DEFAULT 'none',
                selection_effect_summary TEXT,
                gateway_mode TEXT,
                experiment_variant TEXT,
                proxy_session_id TEXT,
                routing_subject_hash TEXT,
                upstream_operation TEXT,
                fallback_reason TEXT,
                request_body BLOB,
                response_body BLOB,
                request_body_bytes INTEGER,
                response_body_bytes INTEGER,
                request_body_sha256 TEXT,
                response_body_sha256 TEXT,
                body_retention_days INTEGER,
                body_retention_profile TEXT,
                body_cleaned_reason TEXT,
                body_cleaned_at INTEGER,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                remote_addr TEXT,
                client_ip TEXT,
                client_ip_source TEXT,
                client_ip_trusted INTEGER NOT NULL DEFAULT 0,
                ip_headers TEXT,
                visibility TEXT NOT NULL DEFAULT 'visible',
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        let mut request_kind_schema_changed = false;
        request_kind_schema_changed |= self.upgrade_request_logs_schema().await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_auth_token_time
               ON request_logs(auth_token_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_time
               ON request_logs(created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_visibility_time
               ON request_logs(visibility, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_key_time
               ON request_logs(api_key_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_key_effect_time
               ON request_logs(key_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_api_key_transient_backoffs_schema().await?;

        // API key usage rollups (for statistics that must not depend on request_logs retention).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.api_key_usage_buckets (
                api_key_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                total_requests INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                error_count INTEGER NOT NULL,
                quota_exhausted_count INTEGER NOT NULL,
                valuable_success_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_429_count INTEGER NOT NULL DEFAULT 0,
                other_success_count INTEGER NOT NULL DEFAULT 0,
                other_failure_count INTEGER NOT NULL DEFAULT 0,
                unknown_count INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (api_key_id, bucket_start, bucket_secs)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        let api_key_usage_buckets_schema_changed = self
            .ensure_api_key_usage_bucket_request_value_columns()
            .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_api_key_usage_buckets_time
               ON api_key_usage_buckets(bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.dashboard_request_rollup_buckets (
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                total_requests INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                error_count INTEGER NOT NULL,
                quota_exhausted_count INTEGER NOT NULL,
                valuable_success_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_429_count INTEGER NOT NULL DEFAULT 0,
                other_success_count INTEGER NOT NULL DEFAULT 0,
                other_failure_count INTEGER NOT NULL DEFAULT 0,
                unknown_count INTEGER NOT NULL DEFAULT 0,
                mcp_non_billable INTEGER NOT NULL DEFAULT 0,
                mcp_billable INTEGER NOT NULL DEFAULT 0,
                api_non_billable INTEGER NOT NULL DEFAULT 0,
                api_billable INTEGER NOT NULL DEFAULT 0,
                local_estimated_credits INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (bucket_start, bucket_secs)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        let dashboard_request_rollup_buckets_schema_changed = self
            .ensure_dashboard_request_rollup_bucket_columns()
            .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_dashboard_request_rollup_buckets_scope_time
               ON dashboard_request_rollup_buckets(bucket_secs, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // Access tokens for /mcp authentication
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_tokens (
                id TEXT PRIMARY KEY,           -- 4-char id code
                secret TEXT NOT NULL,          -- 12-char secret
                enabled INTEGER NOT NULL DEFAULT 1,
                note TEXT,
                group_name TEXT,
                total_requests INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_auth_tokens_schema().await?;

        // Persist research request ownership/key affinity so result polling survives
        // process restarts and multi-instance routing.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS research_requests (
                request_id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                token_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_research_requests_expires_at
               ON research_requests(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_announcements_schema().await?;
        forward_proxy::ensure_forward_proxy_schema(&self.pool).await?;

        // User identity model (separated from admin auth):
        // - users: local user records
        // - oauth_accounts: third-party account bindings (provider + provider_user_id unique)
        // - user_sessions: persisted user sessions for browser auth
        // - user_token_bindings: one user may bind multiple auth tokens
        // - oauth_login_states: one-time OAuth state tokens for CSRF/replay protection
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                username TEXT,
                avatar_template TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                debug_info_shared INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_login_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_users_debug_info_shared_column().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS oauth_accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                provider_user_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                username TEXT,
                name TEXT,
                avatar_template TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                trust_level INTEGER,
                raw_payload TEXT,
                refresh_token_ciphertext TEXT,
                refresh_token_nonce TEXT,
                last_profile_sync_attempt_at INTEGER,
                last_profile_sync_success_at INTEGER,
                last_profile_sync_error TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(provider, provider_user_id),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_oauth_accounts_user ON oauth_accounts(user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("oauth_accounts", "refresh_token_ciphertext")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_accounts ADD COLUMN refresh_token_ciphertext TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_column_exists("oauth_accounts", "refresh_token_nonce")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_accounts ADD COLUMN refresh_token_nonce TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_column_exists("oauth_accounts", "last_profile_sync_attempt_at")
            .await?
        {
            sqlx::query(
                "ALTER TABLE oauth_accounts ADD COLUMN last_profile_sync_attempt_at INTEGER",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self
            .table_column_exists("oauth_accounts", "last_profile_sync_success_at")
            .await?
        {
            sqlx::query(
                "ALTER TABLE oauth_accounts ADD COLUMN last_profile_sync_success_at INTEGER",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self
            .table_column_exists("oauth_accounts", "last_profile_sync_error")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_accounts ADD COLUMN last_profile_sync_error TEXT")
                .execute(&self.pool)
                .await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_sessions (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked_at INTEGER,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_sessions_user ON user_sessions(user_id, expires_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_token_bindings (
                user_id TEXT NOT NULL,
                token_id TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, token_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.migrate_user_token_bindings_to_multi_binding().await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_token_bindings_user_updated
               ON user_token_bindings(user_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_api_key_bindings (
                user_id TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_success_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, api_key_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_api_key_bindings_user_recent
               ON user_api_key_bindings(user_id, last_success_at DESC, api_key_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_api_key_bindings_key_recent
               ON user_api_key_bindings(api_key_id, last_success_at DESC, user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_api_key_bindings (
                token_id TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_success_at INTEGER NOT NULL,
                PRIMARY KEY (token_id, api_key_id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_api_key_bindings_token_recent
               ON token_api_key_bindings(token_id, last_success_at DESC, api_key_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_api_key_bindings_key_recent
               ON token_api_key_bindings(api_key_id, last_success_at DESC, token_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS subject_key_breakages (
                subject_kind TEXT NOT NULL,
                subject_id TEXT NOT NULL,
                key_id TEXT NOT NULL,
                month_start INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                latest_break_at INTEGER NOT NULL,
                key_status TEXT NOT NULL,
                reason_code TEXT,
                reason_summary TEXT,
                source TEXT NOT NULL,
                breaker_token_id TEXT,
                breaker_user_id TEXT,
                breaker_user_display_name TEXT,
                manual_actor_display_name TEXT,
                PRIMARY KEY (subject_kind, subject_id, key_id, month_start),
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_subject_key_breakages_subject_month
               ON subject_key_breakages(subject_kind, subject_id, month_start DESC, latest_break_at DESC, key_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_subject_key_breakages_key_month
               ON subject_key_breakages(key_id, month_start DESC, latest_break_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_primary_api_key_affinity (
                user_id TEXT PRIMARY KEY,
                api_key_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_primary_api_key_affinity_key
               ON user_primary_api_key_affinity(api_key_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_primary_api_key_affinity (
                token_id TEXT PRIMARY KEY,
                user_id TEXT,
                api_key_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_primary_api_key_affinity_user
               ON token_primary_api_key_affinity(user_id, updated_at DESC, token_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_primary_api_key_affinity_key
               ON token_primary_api_key_affinity(api_key_id, updated_at DESC, token_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS http_project_api_key_affinity (
                owner_subject TEXT NOT NULL,
                project_id_hash TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (owner_subject, project_id_hash),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_http_project_api_key_affinity_key
               ON http_project_api_key_affinity(api_key_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_sessions (
                proxy_session_id TEXT PRIMARY KEY,
                upstream_session_id TEXT,
                upstream_key_id TEXT,
                auth_token_id TEXT,
                user_id TEXT,
                protocol_version TEXT,
                last_event_id TEXT,
                gateway_mode TEXT NOT NULL DEFAULT 'upstream_mcp',
                experiment_variant TEXT NOT NULL DEFAULT 'control',
                ab_bucket INTEGER,
                routing_subject_hash TEXT,
                fallback_reason TEXT,
                rate_limited_until INTEGER,
                last_rate_limited_at INTEGER,
                last_rate_limit_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked_at INTEGER,
                revoke_reason TEXT,
                FOREIGN KEY (upstream_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_user_active
               ON mcp_sessions(user_id, revoked_at, expires_at DESC, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_token_active
               ON mcp_sessions(auth_token_id, revoked_at, expires_at DESC, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_expires_at
               ON mcp_sessions(expires_at, revoked_at)"#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_mcp_sessions_schema().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS oauth_login_states (
                state TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                redirect_to TEXT,
                binding_hash TEXT,
                bind_token_id TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                consumed_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_oauth_login_states_expire ON oauth_login_states(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("oauth_login_states", "binding_hash")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_login_states ADD COLUMN binding_hash TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_column_exists("oauth_login_states", "bind_token_id")
            .await?
        {
            sqlx::query("ALTER TABLE oauth_login_states ADD COLUMN bind_token_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        self.ensure_dev_open_admin_token().await?;

        // Ensure per-token usage logs table exists BEFORE running data consistency migration
        // because the migration queries auth_token_logs.
        // Per-token usage logs for detail page (auth_token_logs)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_token_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_id TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                http_status INTEGER,
                mcp_status INTEGER,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                result_status TEXT NOT NULL,
                error_message TEXT,
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                binding_effect_code TEXT NOT NULL DEFAULT 'none',
                binding_effect_summary TEXT,
                selection_effect_code TEXT NOT NULL DEFAULT 'none',
                selection_effect_summary TEXT,
                gateway_mode TEXT,
                experiment_variant TEXT,
                proxy_session_id TEXT,
                routing_subject_hash TEXT,
                upstream_operation TEXT,
                fallback_reason TEXT,
                counts_business_quota INTEGER NOT NULL DEFAULT 1,
                business_credits INTEGER,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                request_user_id TEXT,
                api_key_id TEXT,
                request_log_id INTEGER,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS billing_ledger (
                auth_token_log_id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                business_credits INTEGER,
                request_user_id TEXT,
                api_key_id TEXT,
                request_log_id INTEGER,
                result_status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                settled_at INTEGER,
                error_message TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        self.ensure_billing_ledger_ha_shape().await?;
        self.ensure_billing_ledger_indexes().await?;

        // Upgrade: add mcp_status column if missing
        if !self
            .table_column_exists("auth_token_logs", "mcp_status")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN mcp_status INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "failure_kind")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN failure_kind TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "key_effect_code")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN key_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "key_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN key_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "binding_effect_code")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN binding_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "binding_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN binding_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "selection_effect_code")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN selection_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "selection_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN selection_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        for (column, sql) in [
            (
                "gateway_mode",
                "ALTER TABLE auth_token_logs ADD COLUMN gateway_mode TEXT",
            ),
            (
                "experiment_variant",
                "ALTER TABLE auth_token_logs ADD COLUMN experiment_variant TEXT",
            ),
            (
                "proxy_session_id",
                "ALTER TABLE auth_token_logs ADD COLUMN proxy_session_id TEXT",
            ),
            (
                "routing_subject_hash",
                "ALTER TABLE auth_token_logs ADD COLUMN routing_subject_hash TEXT",
            ),
            (
                "upstream_operation",
                "ALTER TABLE auth_token_logs ADD COLUMN upstream_operation TEXT",
            ),
            (
                "fallback_reason",
                "ALTER TABLE auth_token_logs ADD COLUMN fallback_reason TEXT",
            ),
        ] {
            if !self.table_column_exists("auth_token_logs", column).await? {
                sqlx::query(sql).execute(&self.pool).await?;
            }
        }

        request_kind_schema_changed |= self.ensure_auth_token_logs_request_kind_columns().await?;

        // Upgrade: add counts_business_quota column if missing
        if !self
            .table_column_exists("auth_token_logs", "counts_business_quota")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN counts_business_quota INTEGER NOT NULL DEFAULT 1",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "business_credits")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN business_credits INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "billing_subject")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN billing_subject TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "billing_state")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN billing_state TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self
            .table_column_exists("auth_token_logs", "request_user_id")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_user_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "api_key_id")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN api_key_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .table_column_exists("auth_token_logs", "request_log_id")
            .await?
        {
            sqlx::query(
                "ALTER TABLE auth_token_logs ADD COLUMN request_log_id INTEGER",
            )
            .execute(&self.pool)
            .await?;
        }

        if self
            .auth_token_logs_have_legacy_request_kind_columns()
            .await?
        {
            self.rebuild_auth_token_logs_table(
                AuthTokenLogsRebuildMode::DropLegacyRequestKindColumns,
            )
            .await?;
            request_kind_schema_changed = true;
        }

        self.ensure_meta_schema().await?;
        self.maybe_repair_billing_ledger_from_auth_token_logs()
            .await?;

        self.ensure_auth_token_logs_indexes().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_user_usage_buckets (
                api_key_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                success_credits INTEGER NOT NULL,
                failure_credits INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (api_key_id, user_id, bucket_start, bucket_secs),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_user_usage_buckets_key_bucket
               ON api_key_user_usage_buckets(api_key_id, bucket_secs, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_user_usage_buckets_user_bucket
               ON api_key_user_usage_buckets(user_id, bucket_secs, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        self.migrate_legacy_observability_tables_to_sidecar().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS quota_subject_locks (
                subject TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_quota_subject_locks_expires_at
               ON quota_subject_locks(expires_at)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_usage_buckets (
                token_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                granularity TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (token_id, bucket_start, granularity),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_usage_lookup ON token_usage_buckets(token_id, granularity, bucket_start)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_token_quota (
                token_id TEXT PRIMARY KEY,
                month_start INTEGER NOT NULL,
                month_count INTEGER NOT NULL,
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_quota_limits (
                user_id TEXT PRIMARY KEY,
                hourly_any_limit INTEGER NOT NULL,
                hourly_limit INTEGER NOT NULL,
                daily_limit INTEGER NOT NULL,
                monthly_limit INTEGER NOT NULL,
                monthly_broken_limit INTEGER NOT NULL DEFAULT 5,
                monthly_blocked_key_limit_delta INTEGER NOT NULL DEFAULT 0,
                inherits_defaults INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("account_quota_limits", "inherits_defaults")
            .await?
        {
            sqlx::query(
                "ALTER TABLE account_quota_limits ADD COLUMN inherits_defaults INTEGER NOT NULL DEFAULT 1",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("account_quota_limits", "monthly_broken_limit")
            .await?
        {
            sqlx::query(
                "ALTER TABLE account_quota_limits ADD COLUMN monthly_broken_limit INTEGER NOT NULL DEFAULT 5",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .table_column_exists("account_quota_limits", "monthly_blocked_key_limit_delta")
            .await?
        {
            sqlx::query(
                "ALTER TABLE account_quota_limits ADD COLUMN monthly_blocked_key_limit_delta INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
            sqlx::query(
                "UPDATE account_quota_limits SET monthly_blocked_key_limit_delta = monthly_broken_limit - ?",
            )
            .bind(USER_MONTHLY_BROKEN_LIMIT_DEFAULT)
            .execute(&self.pool)
            .await?;
        }

        self.ensure_linuxdo_credit_recharge_schema().await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                icon TEXT,
                system_key TEXT UNIQUE,
                effect_kind TEXT NOT NULL DEFAULT 'quota_delta',
                hourly_any_delta INTEGER NOT NULL DEFAULT 0,
                hourly_delta INTEGER NOT NULL DEFAULT 0,
                daily_delta INTEGER NOT NULL DEFAULT 0,
                monthly_delta INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_tag_bindings (
                user_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, tag_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (tag_id) REFERENCES user_tags(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_tag_bindings_user_updated
               ON user_tag_bindings(user_id, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_user_tag_bindings_tag_user
               ON user_tag_bindings(tag_id, user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_usage_buckets (
                user_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                granularity TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (user_id, bucket_start, granularity),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_account_usage_lookup
               ON account_usage_buckets(user_id, granularity, bucket_start)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_monthly_quota (
                user_id TEXT PRIMARY KEY,
                month_start INTEGER NOT NULL,
                month_count INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_usage_rollup_buckets (
                user_id TEXT NOT NULL,
                metric_kind TEXT NOT NULL,
                bucket_kind TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                value INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, metric_kind, bucket_kind, bucket_start),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_account_usage_rollup_lookup
               ON account_usage_rollup_buckets(user_id, metric_kind, bucket_kind, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_account_usage_rollup_metric_lookup
               ON account_usage_rollup_buckets(metric_kind, bucket_kind, bucket_start DESC, user_id)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_rate_limit_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                changed_at INTEGER NOT NULL,
                limit_value INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_rate_limit_snapshots_changed
               ON request_rate_limit_snapshots(changed_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS account_quota_limit_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                changed_at INTEGER NOT NULL,
                hourly_any_limit INTEGER NOT NULL,
                hourly_limit INTEGER NOT NULL,
                daily_limit INTEGER NOT NULL,
                monthly_limit INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_account_quota_limit_snapshots_user_changed
               ON account_quota_limit_snapshots(user_id, changed_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS token_usage_stats (
                token_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                system_failure_count INTEGER NOT NULL,
                external_failure_count INTEGER NOT NULL,
                quota_exhausted_count INTEGER NOT NULL,
                PRIMARY KEY (token_id, bucket_start, bucket_secs),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_usage_stats_token_time
               ON token_usage_stats(token_id, bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        // Scheduled jobs table for background tasks (e.g., quota/usage sync)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS scheduled_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_type TEXT NOT NULL,
                trigger_source TEXT NOT NULL DEFAULT 'scheduler',
                key_id TEXT,
                status TEXT NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 1,
                message TEXT,
                queued_at INTEGER NOT NULL,
                started_at INTEGER,
                finished_at INTEGER,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_scheduled_jobs_queue_schema().await?;
        self.ensure_meta_schema().await?;
        self.ensure_request_logs_read_indexes().await?;
        self.migrate_log_effect_buckets().await?;

        let uses_legacy_single_db_observability_compatibility =
            self.uses_legacy_single_db_observability_compatibility();

        if request_kind_schema_changed && !uses_legacy_single_db_observability_compatibility {
            self.reset_request_kind_canonical_migration_v1_markers()
                .await?;
        }

        if !uses_legacy_single_db_observability_compatibility {
            self.ensure_request_kind_canonical_migration_v1().await?;
        }
        self.ensure_request_log_catalog_rollup_schema().await?;
        self.ensure_ha_schema().await?;
        self.ensure_observability_sidecar_startup_rebuild_not_required()
            .await?;
        if !uses_legacy_single_db_observability_compatibility
            && core_database_file_size(&self.database_path).unwrap_or(u64::MAX)
                <= LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES
            && self
                .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
                .await?
                .is_none()
        {
            self.rebuild_observability_sidecar_derived_tables_offline(false)
                .await?;
        }

        if !uses_legacy_single_db_observability_compatibility
            && self
                .get_meta_i64(META_KEY_API_KEY_CREATED_AT_BACKFILL_V1)
                .await?
                .is_none()
        {
            self.backfill_api_key_created_at().await?;
            self.set_meta_i64(
                META_KEY_API_KEY_CREATED_AT_BACKFILL_V1,
                self.backend_time.now_ts(),
            )
            .await?;
        }

        // Backfill API key usage buckets exactly once. This enables safe request_logs retention
        // without changing the meaning of cumulative statistics.
        if !uses_legacy_single_db_observability_compatibility {
            let api_key_usage_buckets_v1_done = self
                .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
                .await?
                .is_some();
            if !api_key_usage_buckets_v1_done {
                self.reject_large_sidecar_startup_rebuild("api key usage bucket rebuild")
                    .await?;
                self.migrate_api_key_usage_buckets_v1().await?;
                self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE, 1)
                    .await?;
                self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE, 1)
                    .await?;
            } else if api_key_usage_buckets_schema_changed
                || self
                    .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
                    .await?
                    .is_none()
            {
                self.reject_large_sidecar_startup_rebuild("api key usage bucket request-value backfill")
                    .await?;
                self.backfill_api_key_usage_bucket_request_value_counts_v2()
                    .await?;
                self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE, 1)
                    .await?;
            }
        }

        if !uses_legacy_single_db_observability_compatibility
            && dashboard_request_rollup_buckets_schema_changed
        {
            self.reject_large_sidecar_startup_rebuild("dashboard request rollup schema rebuild")
                .await?;
            self.set_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE, 0)
                .await?;
        }

        if !uses_legacy_single_db_observability_compatibility
            && self
                .get_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
                .await?
                != Some(1)
        {
            self.reject_large_sidecar_startup_rebuild("dashboard request rollup rebuild")
                .await?;
            self.rebuild_dashboard_request_rollup_buckets().await?;
            self.set_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE, 1)
                .await?;
        }

        if !uses_legacy_single_db_observability_compatibility {
            let request_log_catalog_rollup_retention_days = self
                .get_system_settings()
                .await?
                .request_log_retention
                .max_log_retention_days;
            let request_log_catalog_rollup_needs_rebuild = self
                .get_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
                .await?
                != Some(1)
                || self
                    .get_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
                    .await?
                    != Some(request_log_catalog_rollup_retention_days);
            if request_log_catalog_rollup_needs_rebuild {
                self.reject_large_sidecar_startup_rebuild("request log catalog rollup rebuild")
                    .await?;
                self.rebuild_request_log_catalog_rollups().await?;
                self.set_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE, 1)
                    .await?;
                self.set_meta_i64(
                    META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS,
                    request_log_catalog_rollup_retention_days,
                )
                .await?;
            }
        }

        // After ensuring schemas, run the data consistency migration at most once.
        // Older versions incremented auth_tokens.total_requests during validation; this
        // migration reconciles those counters using auth_token_logs, then marks itself
        // as completed in the meta table so that future startups do not depend on
        // potentially truncated logs.
        if !uses_legacy_single_db_observability_compatibility
            && self
                .get_meta_i64(META_KEY_DATA_CONSISTENCY_DONE)
                .await?
                .is_none()
        {
            self.migrate_data_consistency().await?;
            self.set_meta_i64(META_KEY_DATA_CONSISTENCY_DONE, 1).await?;
        }

        // One-time healer: backfill soft-deleted auth_tokens rows for any token_id
        // that only exists in auth_token_logs. This ensures that downstream usage
        // rollups into token_usage_stats (which reference auth_tokens via FOREIGN KEY)
        // will not fail with constraint errors for legacy data.
        if !uses_legacy_single_db_observability_compatibility
            && self
                .get_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1)
                .await?
                .is_none()
        {
            self.heal_orphan_auth_tokens_from_logs().await?;
        }

        // Cut over business quota counters from legacy "requests" units to "credits".
        // Historical request counts cannot be converted safely, but clearing them would silently
        // grant fresh quota to every active subject on upgrade. Preserve existing windows and let
        // them age out naturally; new charges written after the cutover are already credits-based.
        if !uses_legacy_single_db_observability_compatibility {
            if self
                .get_meta_i64(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
                .await?
                .is_none()
            {
                self.set_meta_i64(
                    META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1,
                    self.backend_time.now_ts(),
                )
                .await?;
            }

            if self
                .get_meta_i64(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1)
                .await?
                .is_none()
            {
                self.backfill_account_quota_v1().await?;
                self.set_meta_i64(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1, 1)
                    .await?;
            }
            if self
                .get_meta_i64(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
                .await?
                .is_none()
            {
                self.backfill_account_quota_inherits_defaults_v1().await?;
                self.set_meta_i64(
                    META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1,
                    self.backend_time.now_ts(),
                )
                .await?;
            }
            if self
                .get_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1)
                .await?
                .is_none()
            {
                self.set_meta_i64(
                    META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1,
                    self.backend_time.now_ts(),
                )
                .await?;
            }
            let account_usage_rollup_v1_done = self
                .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_V1_DONE)
                .await?
                .unwrap_or_default();
            if account_usage_rollup_v1_done <= 0
                || self.account_usage_rollup_request_day_rebuild_needed().await?
            {
                self.rebuild_account_usage_rollup_buckets_v1().await?;
            }
            self.backfill_account_limit_snapshot_history_v1().await?;
            if self
                .get_meta_i64(META_KEY_FORCE_USER_RELOGIN_V1)
                .await?
                .is_none()
            {
                self.force_user_relogin_v1().await?;
                self.set_meta_i64(
                    META_KEY_FORCE_USER_RELOGIN_V1,
                    self.backend_time.now_ts(),
                )
                    .await?;
            }
            self.seed_linuxdo_system_tags().await?;
            if self
                .get_meta_i64(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
                .await?
                .is_none()
            {
                self.backfill_linuxdo_system_tag_default_deltas_v1().await?;
                self.set_meta_i64(
                    META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1,
                    self.backend_time.now_ts(),
                )
                .await?;
            }
            self.sync_linuxdo_system_tag_default_deltas_with_env()
                .await?;
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
                Ok(_) => {}
                Err(err) if is_invalid_current_month_billing_subject_error(&err) => {
                    eprintln!("startup monthly quota rebase skipped: {err}");
                }
                Err(err) => return Err(err),
            }
        }

        Ok(())
    }

    async fn backfill_billing_ledger_from_auth_token_logs(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO billing_ledger (
                auth_token_log_id,
                token_id,
                billing_subject,
                billing_state,
                business_credits,
                request_user_id,
                api_key_id,
                request_log_id,
                result_status,
                created_at,
                updated_at,
                settled_at,
                error_message
            )
            SELECT
                atl.id,
                atl.token_id,
                atl.billing_subject,
                atl.billing_state,
                atl.business_credits,
                atl.request_user_id,
                atl.api_key_id,
                atl.request_log_id,
                atl.result_status,
                atl.created_at,
                atl.created_at,
                CASE
                    WHEN atl.billing_state = 'charged' THEN atl.created_at
                    ELSE NULL
                END,
                atl.error_message
            FROM auth_token_logs atl
            WHERE atl.billing_state <> 'none'
               OR atl.billing_subject IS NOT NULL
               OR atl.business_credits IS NOT NULL
            ON CONFLICT(auth_token_log_id) DO UPDATE SET
                token_id = excluded.token_id,
                billing_subject = excluded.billing_subject,
                billing_state = excluded.billing_state,
                business_credits = excluded.business_credits,
                request_user_id = excluded.request_user_id,
                api_key_id = excluded.api_key_id,
                request_log_id = excluded.request_log_id,
                result_status = excluded.result_status,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                settled_at = excluded.settled_at,
                error_message = excluded.error_message
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_ha_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_node_state (
                id TEXT PRIMARY KEY CHECK (id = 'local'),
                node_id TEXT NOT NULL,
                role TEXT NOT NULL,
                edgeone_origin TEXT,
                ha_source_kind TEXT,
                ha_direct_origin_scheme TEXT,
                ha_direct_origin_host TEXT,
                ha_direct_origin_port INTEGER,
                ha_origin_group_id TEXT,
                message TEXT,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self.table_column_exists("ha_node_state", "message").await? {
            sqlx::query("ALTER TABLE ha_node_state ADD COLUMN message TEXT")
                .execute(&self.pool)
                .await?;
        }
        for (column, ddl) in [
            ("ha_source_kind", "ALTER TABLE ha_node_state ADD COLUMN ha_source_kind TEXT"),
            (
                "ha_direct_origin_scheme",
                "ALTER TABLE ha_node_state ADD COLUMN ha_direct_origin_scheme TEXT",
            ),
            (
                "ha_direct_origin_host",
                "ALTER TABLE ha_node_state ADD COLUMN ha_direct_origin_host TEXT",
            ),
            (
                "ha_direct_origin_port",
                "ALTER TABLE ha_node_state ADD COLUMN ha_direct_origin_port INTEGER",
            ),
            (
                "ha_origin_group_id",
                "ALTER TABLE ha_node_state ADD COLUMN ha_origin_group_id TEXT",
            ),
        ] {
            if !self.table_column_exists("ha_node_state", column).await? {
                sqlx::query(ddl).execute(&self.pool).await?;
            }
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_sync_watermarks (
                name TEXT PRIMARY KEY,
                source_node_id TEXT,
                target_node_id TEXT,
                watermark INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                detail TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_failover_operations (
                id TEXT PRIMARY KEY,
                operation_kind TEXT NOT NULL,
                target_node_id TEXT,
                from_origin TEXT,
                to_origin TEXT,
                status TEXT NOT NULL,
                message TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_recovery_batches (
                id TEXT PRIMARY KEY,
                source_node_id TEXT NOT NULL,
                status TEXT NOT NULL,
                event_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                imported_at INTEGER,
                checksum TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_edgeone_audit_logs (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                request_json TEXT,
                response_json TEXT,
                status TEXT NOT NULL,
                message TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_control_plane_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_kind TEXT NOT NULL,
                category TEXT NOT NULL,
                status TEXT NOT NULL,
                node_id TEXT,
                operation_id TEXT,
                summary TEXT NOT NULL,
                detail TEXT,
                technical_details_json TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_ha_control_plane_events_created_at ON ha_control_plane_events(created_at DESC)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_ha_control_plane_events_category_node ON ha_control_plane_events(category, node_id, created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_outbox (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                resource TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                op TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                checksum TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_billing_outbox (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                resource TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                op TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                checksum TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_runtime_outbox (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                resource TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                op TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                checksum TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_outbox_suppression (
                id TEXT PRIMARY KEY CHECK (id = 'local')
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_ha_outbox_resource
               ON ha_outbox(resource, resource_id, seq)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_ha_outbox_created
               ON ha_outbox(created_at, seq)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_ha_billing_outbox_created
               ON ha_billing_outbox(created_at, seq)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_ha_runtime_outbox_created
               ON ha_runtime_outbox(created_at, seq)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ha_peer_watermarks (
                peer_node_id TEXT NOT NULL,
                channel TEXT NOT NULL DEFAULT 'control',
                acked_seq INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (peer_node_id, channel)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        self.ensure_ha_peer_watermarks_shape().await?;

        Ok(())
    }

    pub(crate) async fn try_claim_request_kind_canonical_migration_v1(
        &self,
        now_ts: i64,
    ) -> Result<RequestKindCanonicalMigrationClaim, ProxyError> {
        match read_request_kind_canonical_migration_status(&self.pool).await? {
            Some(RequestKindCanonicalMigrationState::Done(done_at)) => {
                return Ok(RequestKindCanonicalMigrationClaim::AlreadyDone(done_at));
            }
            Some(state)
                if request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                    .is_some() =>
            {
                return Ok(RequestKindCanonicalMigrationClaim::RunningElsewhere(
                    request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                        .expect("running state should expose heartbeat"),
                ));
            }
            _ => {}
        }

        let mut conn = match begin_immediate_sqlite_connection(&self.pool).await {
            Ok(conn) => conn,
            Err(err) if is_transient_sqlite_write_error(&err) => {
                return match read_request_kind_canonical_migration_status(&self.pool).await? {
                    Some(RequestKindCanonicalMigrationState::Done(done_at)) => {
                        Ok(RequestKindCanonicalMigrationClaim::AlreadyDone(done_at))
                    }
                    Some(state)
                        if request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                            .is_some() =>
                    {
                        Ok(RequestKindCanonicalMigrationClaim::RunningElsewhere(
                            request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                                .expect("running state should expose heartbeat"),
                        ))
                    }
                    _ => Ok(RequestKindCanonicalMigrationClaim::RetryLater),
                };
            }
            Err(err) => return Err(err),
        };

        let state = read_request_kind_canonical_migration_status_with_connection(&mut conn).await?;
        match state {
            Some(RequestKindCanonicalMigrationState::Done(done_at)) => {
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
                    &done_at.to_string(),
                )
                .await?;
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
                    &RequestKindCanonicalMigrationState::Done(done_at).as_meta_value(),
                )
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(RequestKindCanonicalMigrationClaim::AlreadyDone(done_at))
            }
            Some(state)
                if request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                    .is_some() =>
            {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(RequestKindCanonicalMigrationClaim::RunningElsewhere(
                    request_kind_canonical_migration_state_blocks_reentry(now_ts, state)
                        .expect("running state should expose heartbeat"),
                ))
            }
            _ => {
                let upper_bounds = match state {
                    Some(RequestKindCanonicalMigrationState::Running { .. })
                    | Some(RequestKindCanonicalMigrationState::Failed(_)) => {
                        match read_request_kind_canonical_backfill_upper_bounds_with_connection(
                            &mut conn,
                        )
                        .await?
                        {
                            Some(upper_bounds) => upper_bounds,
                            None => {
                                capture_request_kind_canonical_backfill_upper_bounds_with_connection(
                                    &mut conn,
                                )
                                .await?
                            }
                        }
                    }
                    _ => {
                        capture_request_kind_canonical_backfill_upper_bounds_with_connection(
                            &mut conn,
                        )
                        .await?
                    }
                };
                write_request_kind_canonical_backfill_upper_bounds_with_connection(
                    &mut conn,
                    upper_bounds,
                )
                .await?;
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
                    &current_request_kind_canonical_migration_running_state(now_ts).as_meta_value(),
                )
                .await?;
                delete_meta_key_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
                )
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(RequestKindCanonicalMigrationClaim::Claimed)
            }
        }
    }

    pub(crate) async fn finish_request_kind_canonical_migration_v1(
        &self,
        state: RequestKindCanonicalMigrationState,
    ) -> Result<(), ProxyError> {
        let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
        let done_at = read_meta_string_with_connection(
            &mut conn,
            META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
        )
        .await?
        .and_then(|value| value.parse::<i64>().ok());

        if let Some(done_at) = done_at {
            let done_state = RequestKindCanonicalMigrationState::Done(done_at);
            write_meta_string_with_connection(
                &mut conn,
                META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
                &done_state.as_meta_value(),
            )
            .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            return Ok(());
        }

        match state {
            RequestKindCanonicalMigrationState::Done(done_at) => {
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
                    &done_at.to_string(),
                )
                .await?;
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
                    &state.as_meta_value(),
                )
                .await?;
            }
            RequestKindCanonicalMigrationState::Running { .. } => {}
            RequestKindCanonicalMigrationState::Failed(_) => {
                write_meta_string_with_connection(
                    &mut conn,
                    META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
                    &state.as_meta_value(),
                )
                .await?;
            }
        }

        sqlx::query("COMMIT").execute(&mut *conn).await?;
        Ok(())
    }

    async fn ensure_users_debug_info_shared_column(&self) -> Result<(), ProxyError> {
        if !self.table_exists("users").await? {
            return Ok(());
        }
        if !self
            .table_column_exists("users", "debug_info_shared")
            .await?
        {
            sqlx::query(
                "ALTER TABLE users ADD COLUMN debug_info_shared INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn ensure_meta_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_request_logs_read_indexes(&self) -> Result<(), ProxyError> {
        for sql in [
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_request_kind_time
               ON request_logs(request_kind_key, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_binding_effect_time
               ON request_logs(binding_effect_code, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_selection_effect_time
               ON request_logs(selection_effect_code, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_user_ip_time
               ON request_logs(request_user_id, client_ip, created_at DESC)"#,
        ] {
            sqlx::query(sql).execute(&self.pool).await?;
        }

        Ok(())
    }
}
