impl KeyStore {
    async fn rebuild_request_log_soft_reference_tables_if_needed(
        &self,
    ) -> Result<(), ProxyError> {
        if self
            .table_has_foreign_key_to("auth_token_logs", "request_logs")
            .await?
        {
            self.rebuild_auth_token_logs_table(
                AuthTokenLogsRebuildMode::DropLegacyRequestKindColumns,
            )
            .await?;
            self.ensure_auth_token_logs_indexes().await?;
        }
        if self
            .table_has_foreign_key_to("billing_ledger", "request_logs")
            .await?
        {
            self.rebuild_billing_ledger_without_request_log_foreign_key()
                .await?;
        }
        if self
            .table_has_foreign_key_to("api_key_maintenance_records", "request_logs")
            .await?
        {
            self.rebuild_api_key_maintenance_records_without_request_log_foreign_key()
                .await?;
        }
        if self
            .table_has_foreign_key_to("api_key_transient_backoffs", "request_logs")
            .await?
        {
            self.rebuild_api_key_transient_backoffs_without_request_log_foreign_key()
                .await?;
        }
        Ok(())
    }

    async fn main_table_exists_in_pool(
        pool: &SqlitePool,
        table: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn table_exists_in_pool(pool: &SqlitePool, table: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn table_column_exists_in_pool(
        pool: &SqlitePool,
        table: &str,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ? LIMIT 1"
        ))
        .bind(column)
        .fetch_optional(pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn ensure_request_logs_schema_in_pool(pool: &SqlitePool) -> Result<(), ProxyError> {
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
        .execute(pool)
        .await?;

        for sql in [
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_auth_token_time
               ON request_logs(auth_token_id, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_time
               ON request_logs(created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_visibility_time
               ON request_logs(visibility, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_key_time
               ON request_logs(api_key_id, created_at DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_key_effect_time
               ON request_logs(key_effect_code, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_request_kind_time
               ON request_logs(request_kind_key, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_binding_effect_time
               ON request_logs(binding_effect_code, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_selection_effect_time
               ON request_logs(selection_effect_code, created_at DESC, id DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_user_ip_time
               ON request_logs(request_user_id, client_ip, created_at DESC)"#,
        ] {
            sqlx::query(sql).execute(pool).await?;
        }

        Ok(())
    }

    async fn copy_legacy_request_logs_into_observability_batched_in_pool(
        pool: &SqlitePool,
        batch_size: i64,
    ) -> Result<(i64, i64), ProxyError> {
        if !Self::main_table_exists_in_pool(pool, "request_logs").await? {
            return Ok((0, 0));
        }

        let source_columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('request_logs', 'main')",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
        if source_columns.is_empty() {
            return Ok((0, 0));
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
        let insert_sql = format!(
            r#"
            INSERT INTO observability.request_logs ({})
            SELECT {}
            FROM main.request_logs AS legacy
            WHERE legacy.id > ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM observability.request_logs AS obs
                  WHERE obs.id = legacy.id
              )
            ORDER BY legacy.id ASC
            LIMIT ?
            "#,
            target_columns.join(", "),
            source_exprs.join(", "),
        );

        let mut last_seen_id = 0_i64;
        let mut copied = 0_i64;
        let mut batches = 0_i64;

        loop {
            let next_missing_id = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT legacy.id
                FROM main.request_logs AS legacy
                WHERE legacy.id > ?
                  AND NOT EXISTS (
                      SELECT 1
                      FROM observability.request_logs AS obs
                      WHERE obs.id = legacy.id
                  )
                ORDER BY legacy.id ASC
                LIMIT 1
                "#,
            )
            .bind(last_seen_id)
            .fetch_optional(pool)
            .await?;
            let Some(next_missing_id) = next_missing_id else {
                break;
            };

            let inserted = sqlx::query(&insert_sql)
                .bind(next_missing_id - 1)
                .bind(batch_size)
                .execute(pool)
                .await?
                .rows_affected() as i64;
            if inserted <= 0 {
                break;
            }
            batches += 1;
            copied += inserted;
            last_seen_id = next_missing_id;
        }

        Ok((copied, batches))
    }

    async fn ensure_request_logs_child_reference_integrity_in_pool(
        pool: &SqlitePool,
        table: &str,
        column: &str,
        context: &str,
    ) -> Result<(), ProxyError> {
        if !Self::table_exists_in_pool(pool, table).await? {
            return Ok(());
        }
        if !Self::table_column_exists_in_pool(pool, table, column).await? {
            return Ok(());
        }

        let query = format!(
            "SELECT rowid, {column} AS request_log_id FROM {table} \
             WHERE {column} IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM observability.request_logs WHERE observability.request_logs.id = {table}.{column}) \
             ORDER BY rowid ASC LIMIT 5"
        );
        let rows = sqlx::query(&query).fetch_all(pool).await?;
        if rows.is_empty() {
            return Ok(());
        }

        let details = rows
            .into_iter()
            .map(|row| {
                let rowid = row.try_get::<i64, _>("rowid").unwrap_or_default();
                let request_log_id = row.try_get::<i64, _>("request_log_id").unwrap_or_default();
                format!("{table}[rowid={rowid}] -> request_logs[id={request_log_id}]")
            })
            .collect::<Vec<_>>()
            .join("; ");

        Err(ProxyError::Other(format!("{context}: {details}")))
    }

    async fn ensure_request_logs_rebuild_references_valid_in_pool(
        pool: &SqlitePool,
        context: &str,
    ) -> Result<(), ProxyError> {
        for (table, column) in [
            ("auth_token_logs", "request_log_id"),
            ("billing_ledger", "request_log_id"),
            ("api_key_maintenance_records", "request_log_id"),
            ("api_key_transient_backoffs", "source_request_log_id"),
        ] {
            Self::ensure_request_logs_child_reference_integrity_in_pool(
                pool, table, column, context,
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn open_for_observability_sidecar_migration(
        database_path: &str,
    ) -> Result<Self, ProxyError> {
        let layout = SqliteDatabaseLayout::from_database_path(database_path);
        let pool = open_sqlite_pool_forced_observability(
            &layout.core_database_path,
            layout.observability_database_path.as_deref(),
            true,
            false,
            SQLITE_POOL_MAX_CONNECTIONS_DEFAULT,
        )
        .await?;
        let observability_database_path = attached_database_path(&pool, "observability").await?;
        let store = Self {
            database_path: layout.core_database_path.clone(),
            observability_database_path,
            _observability_lock: None,
            pool,
            backend_time: BackendTime::system(),
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
        store.ensure_meta_schema().await?;
        Self::ensure_request_logs_schema_in_pool(&store.pool).await?;
        store.upgrade_request_logs_schema().await?;
        store.ensure_request_logs_gc_support_indexes().await?;
        Ok(store)
    }

    pub(crate) async fn run_observability_sidecar_migrate(
        database_path: &str,
        batch_size: i64,
        dry_run: bool,
    ) -> Result<ObservabilitySidecarMigrationReport, ProxyError> {
        if batch_size <= 0 {
            return Err(ProxyError::Other(
                "observability sidecar migration requires batch_size > 0".to_string(),
            ));
        }

        let started = Instant::now();
        let layout = SqliteDatabaseLayout::from_database_path(database_path);
        let sidecar_path = layout
            .observability_database_path
            .clone()
            .ok_or_else(|| ProxyError::Other("observability sidecar path is unavailable".to_string()))?;
        let attached_default = detect_observability_attach_path(
            &layout.core_database_path,
            layout.observability_database_path.as_deref(),
            true,
            false,
        )
        .await?;
        let offline_probe = probe_observability_offline_state(&layout.core_database_path).await?;
        if !dry_run && !offline_probe.service_lock_held_exclusively {
            return Err(ProxyError::Other(format!(
                "offline migration requires the service to be stopped; could not acquire exclusive observability lock at {}",
                offline_probe.sibling_lock_path
            )));
        }
        let offline_lock_acquired = offline_probe.service_lock_held_exclusively;

        let core_file_bytes = core_database_file_size(&layout.core_database_path)?;
        let available_bytes_before = available_disk_bytes_for_path(&layout.core_database_path)?;
        let sidecar_file_bytes_before = std::fs::metadata(&sidecar_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let large_legacy_fallback_active = attached_default
            .as_deref()
            .map(|path| sqlite_paths_match(path, &layout.core_database_path))
            .unwrap_or(false);

        let core_probe = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&layout.core_database_path)
                    .create_if_missing(false)
                    .read_only(true)
                    .busy_timeout(Duration::from_secs(5)),
            )
            .await
            .map_err(ProxyError::Database)?;
        let legacy_request_logs_exists = Self::main_table_exists_in_pool(&core_probe, "request_logs").await?;
        let legacy_api_key_usage_buckets_exists =
            Self::main_table_exists_in_pool(&core_probe, "api_key_usage_buckets").await?;
        let legacy_dashboard_request_rollup_buckets_exists =
            Self::main_table_exists_in_pool(&core_probe, "dashboard_request_rollup_buckets").await?;
        let legacy_request_log_catalog_rollups_exists =
            Self::main_table_exists_in_pool(&core_probe, "request_log_catalog_rollups").await?;

        let (source_min_request_log_id, source_max_request_log_id, source_request_log_rows) =
            if legacy_request_logs_exists {
                sqlx::query_as::<_, (Option<i64>, Option<i64>, i64)>(
                    "SELECT MIN(id), MAX(id), COUNT(*) FROM request_logs",
                )
                .fetch_one(&core_probe)
                .await?
            } else {
                (None, None, 0)
            };
        let sidecar_request_log_rows_before_probe: i64 =
            if std::path::Path::new(&sidecar_path).exists() {
                let sidecar_probe = SqlitePoolOptions::new()
                    .min_connections(1)
                    .max_connections(1)
                    .connect_with(
                        SqliteConnectOptions::new()
                            .filename(&sidecar_path)
                            .create_if_missing(false)
                            .read_only(true)
                            .busy_timeout(Duration::from_secs(5)),
                    )
                    .await
                    .map_err(ProxyError::Database)?;
                let sidecar_has_request_logs =
                    Self::table_exists_in_pool(&sidecar_probe, "request_logs").await?;
                if sidecar_has_request_logs {
                    sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
                        .fetch_one(&sidecar_probe)
                        .await?
                } else {
                    0
                }
            } else {
                0
            };
        let resumed_copy = legacy_request_logs_exists && sidecar_request_log_rows_before_probe > 0;
        let already_migrated = !legacy_request_logs_exists
            && !legacy_api_key_usage_buckets_exists
            && !legacy_dashboard_request_rollup_buckets_exists
            && !legacy_request_log_catalog_rollups_exists;

        let (attached_observability_path, sidecar_request_log_rows_before, sidecar_request_log_rows_after, copied_request_logs, batches, dropped_main_request_logs, dropped_legacy_api_key_usage_buckets, dropped_legacy_dashboard_request_rollup_buckets, dropped_legacy_request_log_catalog_rollups, reset_api_key_usage_buckets_meta, reset_dashboard_request_rollup_buckets_meta, reset_request_log_catalog_rollup_meta, child_reference_checks_passed) =
            if dry_run {
                let attached_observability_path = attached_default
                    .clone()
                    .unwrap_or_else(|| sidecar_path.clone());
                (
                    attached_observability_path,
                    sidecar_request_log_rows_before_probe,
                    sidecar_request_log_rows_before_probe,
                    0,
                    0,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                )
            } else {
                let _offline_guard = acquire_observability_offline_guard(&layout.core_database_path)?;
                let store = Self::open_for_observability_sidecar_migration(database_path).await?;
                let attached_observability_path = store
                    .observability_database_path
                    .clone()
                    .unwrap_or_else(|| layout.core_database_path.clone());
                let sidecar_request_log_rows_before: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                        .fetch_one(&store.pool)
                        .await?;

                let mut copied_request_logs = 0_i64;
                let mut batches = 0_i64;
                let mut dropped_main_request_logs = false;
                let mut dropped_legacy_api_key_usage_buckets = false;
                let mut dropped_legacy_dashboard_request_rollup_buckets = false;
                let mut dropped_legacy_request_log_catalog_rollups = false;
                let mut reset_api_key_usage_buckets_meta = false;
                let mut reset_dashboard_request_rollup_buckets_meta = false;
                let mut reset_request_log_catalog_rollup_meta = false;
                let child_reference_checks_passed;

                if legacy_request_logs_exists {
                    store
                        .rebuild_request_log_soft_reference_tables_if_needed()
                        .await?;
                    let (copied, copied_batches) =
                        Self::copy_legacy_request_logs_into_observability_batched_in_pool(
                            &store.pool,
                            batch_size,
                        )
                        .await?;
                    copied_request_logs = copied;
                    batches = copied_batches;
                    Self::ensure_request_logs_rebuild_references_valid_in_pool(
                        &store.pool,
                        "request_logs schema migration produced invalid preserved references",
                    )
                    .await?;
                    sqlx::query("DROP TABLE request_logs")
                        .execute(&store.pool)
                        .await?;
                    dropped_main_request_logs = true;
                    child_reference_checks_passed = true;
                } else {
                    Self::ensure_request_logs_rebuild_references_valid_in_pool(
                        &store.pool,
                        "request_logs schema migration produced invalid preserved references",
                    )
                    .await?;
                    child_reference_checks_passed = true;
                }

                if legacy_api_key_usage_buckets_exists {
                    let mut tx = store.pool.begin().await?;
                    sqlx::query(
                        r#"
                        INSERT INTO meta (key, value)
                        VALUES (?, '0'), (?, '0')
                        ON CONFLICT(key) DO UPDATE SET value = excluded.value
                        "#,
                    )
                    .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
                    .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("DROP TABLE api_key_usage_buckets")
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                    dropped_legacy_api_key_usage_buckets = true;
                    reset_api_key_usage_buckets_meta = true;
                }
                if legacy_dashboard_request_rollup_buckets_exists {
                    let mut tx = store.pool.begin().await?;
                    sqlx::query(
                        r#"
                        INSERT INTO meta (key, value)
                        VALUES (?, '0')
                        ON CONFLICT(key) DO UPDATE SET value = excluded.value
                        "#,
                    )
                    .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("DROP TABLE dashboard_request_rollup_buckets")
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                    dropped_legacy_dashboard_request_rollup_buckets = true;
                    reset_dashboard_request_rollup_buckets_meta = true;
                }
                if legacy_request_log_catalog_rollups_exists {
                    let mut tx = store.pool.begin().await?;
                    sqlx::query(
                        r#"
                        INSERT INTO meta (key, value)
                        VALUES (?, '0'), (?, '0')
                        ON CONFLICT(key) DO UPDATE SET value = excluded.value
                        "#,
                    )
                    .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
                    .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("DROP TABLE request_log_catalog_rollups")
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                    dropped_legacy_request_log_catalog_rollups = true;
                    reset_request_log_catalog_rollup_meta = true;
                }

                let sidecar_request_log_rows_after: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                        .fetch_one(&store.pool)
                        .await?;
                (
                    attached_observability_path,
                    sidecar_request_log_rows_before,
                    sidecar_request_log_rows_after,
                    copied_request_logs,
                    batches,
                    dropped_main_request_logs,
                    dropped_legacy_api_key_usage_buckets,
                    dropped_legacy_dashboard_request_rollup_buckets,
                    dropped_legacy_request_log_catalog_rollups,
                    reset_api_key_usage_buckets_meta,
                    reset_dashboard_request_rollup_buckets_meta,
                    reset_request_log_catalog_rollup_meta,
                    child_reference_checks_passed,
                )
            };
        let available_bytes_after = available_disk_bytes_for_path(&layout.core_database_path)?;
        let sidecar_file_bytes_after = std::fs::metadata(&sidecar_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        Ok(ObservabilitySidecarMigrationReport {
            dry_run,
            offline_lock_acquired,
            sibling_lock_path: offline_probe.sibling_lock_path,
            sqlite_write_probe_ok: offline_probe.sqlite_write_probe_ok,
            core_path: layout.core_database_path.clone(),
            sidecar_path,
            attached_observability_path,
            legacy_request_logs_exists,
            legacy_api_key_usage_buckets_exists,
            legacy_dashboard_request_rollup_buckets_exists,
            legacy_request_log_catalog_rollups_exists,
            large_legacy_fallback_active,
            large_legacy_fallback_threshold_bytes:
                LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES,
            core_file_bytes,
            sidecar_file_bytes_before,
            sidecar_file_bytes_after,
            available_bytes_before,
            available_bytes_after,
            source_min_request_log_id,
            source_max_request_log_id,
            source_request_log_rows,
            sidecar_request_log_rows_before,
            sidecar_request_log_rows_after,
            copied_request_logs,
            resumed_copy,
            already_migrated,
            dropped_main_request_logs,
            dropped_legacy_api_key_usage_buckets,
            dropped_legacy_dashboard_request_rollup_buckets,
            dropped_legacy_request_log_catalog_rollups,
            reset_api_key_usage_buckets_meta,
            reset_dashboard_request_rollup_buckets_meta,
            reset_request_log_catalog_rollup_meta,
            child_reference_checks_passed,
            batch_size,
            batches,
            completed: !dry_run
                && child_reference_checks_passed
                && (!legacy_request_logs_exists || dropped_main_request_logs)
                && (!legacy_api_key_usage_buckets_exists
                    || dropped_legacy_api_key_usage_buckets)
                && (!legacy_dashboard_request_rollup_buckets_exists
                    || dropped_legacy_dashboard_request_rollup_buckets)
                && (!legacy_request_log_catalog_rollups_exists
                    || dropped_legacy_request_log_catalog_rollups),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn migrate_legacy_observability_tables_to_sidecar(&self) -> Result<(), ProxyError> {
        let Some(observability_database_path) = self.observability_database_path.as_deref() else {
            return Ok(());
        };
        if sqlite_paths_match(&self.database_path, observability_database_path) {
            return Ok(());
        }

        let legacy_request_logs = self.main_table_exists("request_logs").await?;
        let legacy_api_key_usage_buckets = self.main_table_exists("api_key_usage_buckets").await?;
        let legacy_dashboard_rollups = self
            .main_table_exists("dashboard_request_rollup_buckets")
            .await?;
        let legacy_catalog_rollups = self
            .main_table_exists("request_log_catalog_rollups")
            .await?;

        if legacy_request_logs {
            self.rebuild_request_log_soft_reference_tables_if_needed()
                .await?;
            self.copy_legacy_request_logs_into_observability().await?;
            let mut conn = self.pool.acquire().await?;
            self.ensure_request_logs_rebuild_references_valid(
                &mut conn,
                "request_logs schema migration produced invalid preserved references",
            )
            .await?;
            drop(conn);
            sqlx::query("DROP TABLE request_logs")
                .execute(&self.pool)
                .await?;
        }

        if legacy_api_key_usage_buckets {
            sqlx::query("DROP TABLE api_key_usage_buckets")
                .execute(&self.pool)
                .await?;
            self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE, 0)
                .await?;
            self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE, 0)
                .await?;
        }
        if legacy_dashboard_rollups {
            sqlx::query("DROP TABLE dashboard_request_rollup_buckets")
                .execute(&self.pool)
                .await?;
            self.set_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE, 0)
                .await?;
        }
        if legacy_catalog_rollups {
            sqlx::query("DROP TABLE request_log_catalog_rollups")
                .execute(&self.pool)
                .await?;
            self.set_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE, 0)
                .await?;
        }

        Ok(())
    }
}
