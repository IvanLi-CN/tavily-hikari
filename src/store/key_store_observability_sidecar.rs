struct ObservabilitySidecarDerivedRebuildReport {
    dropped_main_request_logs: bool,
    dropped_legacy_api_key_usage_buckets: bool,
    dropped_legacy_dashboard_request_rollup_buckets: bool,
    dropped_legacy_request_log_catalog_rollups: bool,
    rebuilt_api_key_usage_buckets: bool,
    rebuilt_dashboard_request_rollup_buckets: bool,
    rebuilt_request_log_catalog_rollups: bool,
    marked_api_key_usage_buckets_meta_complete: bool,
    marked_dashboard_request_rollup_buckets_meta_complete: bool,
    marked_request_log_catalog_rollup_meta_complete: bool,
    elapsed_ms: u128,
}

#[derive(Debug, Default)]
struct ObservabilitySidecarMigrationState {
    attached_observability_path: String,
    sidecar_request_log_rows_before: i64,
    sidecar_request_log_rows_after: i64,
    copied_request_logs: i64,
    batches: i64,
    dropped_main_request_logs: bool,
    dropped_legacy_api_key_usage_buckets: bool,
    dropped_legacy_dashboard_request_rollup_buckets: bool,
    dropped_legacy_request_log_catalog_rollups: bool,
    reset_api_key_usage_buckets_meta: bool,
    reset_dashboard_request_rollup_buckets_meta: bool,
    reset_request_log_catalog_rollup_meta: bool,
    rebuilt_api_key_usage_buckets: bool,
    rebuilt_dashboard_request_rollup_buckets: bool,
    rebuilt_request_log_catalog_rollups: bool,
    marked_api_key_usage_buckets_meta_complete: bool,
    marked_dashboard_request_rollup_buckets_meta_complete: bool,
    marked_request_log_catalog_rollup_meta_complete: bool,
    startup_reopen_verified: bool,
    startup_rebuild_required: bool,
    derived_rebuild_elapsed_ms: u128,
    child_reference_checks_passed: bool,
}

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

    async fn meta_i64_in_pool(pool: &SqlitePool, key: &str) -> Result<Option<i64>, ProxyError> {
        if !Self::main_table_exists_in_pool(pool, "meta").await? {
            return Ok(None);
        }
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value FROM main.meta WHERE key = ? LIMIT 1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?;
        Ok(value.and_then(|raw| raw.parse::<i64>().ok()))
    }

    async fn explicit_sidecar_cutover_meta_complete_in_pool(
        pool: &SqlitePool,
        request_log_catalog_rollup_retention_days: i64,
    ) -> Result<bool, ProxyError> {
        let explicit_cutover_done =
            Self::meta_i64_in_pool(pool, META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
                .await?
                == Some(1);
        let api_key_usage_done =
            Self::meta_i64_in_pool(pool, META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
                .await?
                == Some(1)
                && Self::meta_i64_in_pool(
                    pool,
                    META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE,
                )
                .await?
                    == Some(1);
        let dashboard_done =
            Self::meta_i64_in_pool(pool, META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
                .await?
                == Some(1);
        let catalog_done =
            Self::meta_i64_in_pool(pool, META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE).await?
                == Some(1)
                && Self::meta_i64_in_pool(
                    pool,
                    META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS,
                )
                .await?
                    == Some(request_log_catalog_rollup_retention_days);

        Ok(explicit_cutover_done && api_key_usage_done && dashboard_done && catalog_done)
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

    async fn ensure_observability_sidecar_derived_schema_in_pool(
        pool: &SqlitePool,
    ) -> Result<(), ProxyError> {
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
        .execute(pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_api_key_usage_buckets_time
               ON api_key_usage_buckets(bucket_start DESC)"#,
        )
        .execute(pool)
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
        .execute(pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS observability.idx_dashboard_request_rollup_buckets_scope_time
               ON dashboard_request_rollup_buckets(bucket_secs, bucket_start DESC)"#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.request_log_catalog_rollups (
                bucket_start INTEGER NOT NULL,
                request_kind_key TEXT NOT NULL,
                request_kind_label TEXT NOT NULL,
                result_bucket TEXT NOT NULL,
                key_effect_code TEXT NOT NULL,
                binding_effect_code TEXT NOT NULL,
                selection_effect_code TEXT NOT NULL,
                auth_token_id TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                operational_class TEXT NOT NULL,
                request_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (
                    bucket_start,
                    request_kind_key,
                    request_kind_label,
                    result_bucket,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    auth_token_id,
                    api_key_id,
                    operational_class
                )
            )
            "#,
        )
        .execute(pool)
        .await?;
        for sql in [
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_log_catalog_rollups_kind_time
               ON request_log_catalog_rollups(request_kind_key, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_log_catalog_rollups_result_time
               ON request_log_catalog_rollups(result_bucket, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_log_catalog_rollups_token_time
               ON request_log_catalog_rollups(auth_token_id, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_log_catalog_rollups_key_time
               ON request_log_catalog_rollups(api_key_id, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_request_log_catalog_rollups_operational_time
               ON request_log_catalog_rollups(operational_class, bucket_start DESC)"#,
        ] {
            sqlx::query(sql).execute(pool).await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.server_pressure_buckets (
                bucket_kind TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                failure_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (bucket_kind, bucket_start)
            )
            "#,
        )
        .execute(pool)
        .await?;
        for sql in [
            r#"CREATE INDEX IF NOT EXISTS observability.idx_server_pressure_buckets_kind_time
               ON server_pressure_buckets(bucket_kind, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_server_pressure_buckets_time
               ON server_pressure_buckets(bucket_start DESC)"#,
        ] {
            sqlx::query(sql).execute(pool).await?;
        }

        Ok(())
    }

    async fn mark_observability_sidecar_derived_meta_complete_in_pool(
        pool: &SqlitePool,
        request_log_catalog_rollup_retention_days: i64,
        mark_explicit_cutover: bool,
    ) -> Result<(), ProxyError> {
        let query = sqlx::query(
            r#"
            INSERT INTO main.meta (key, value)
            VALUES
                (?, '1'),
                (?, '1'),
                (?, '1'),
                (?, '1'),
                (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .bind(request_log_catalog_rollup_retention_days.to_string());
        query.execute(pool).await?;
        if mark_explicit_cutover {
            sqlx::query(
                r#"
                INSERT INTO main.meta (key, value)
                VALUES (?, '1')
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    fn sidecar_local_day_bucket_start_sql(created_at_sql: &str) -> String {
        format!(
            "CAST(strftime('%s', date({created_at_sql}, 'unixepoch', 'localtime'), 'utc') AS INTEGER)"
        )
    }

    fn sidecar_counts_business_quota_sql(alias: &str, request_kind_sql: &str) -> String {
        format!(
            "COALESCE({alias}.counts_business_quota, {})",
            request_log_counts_business_quota_sql(request_kind_sql, &format!("{alias}.request_body"))
        )
    }

    fn sidecar_billing_group_sql(request_kind_sql: &str, counts_business_quota_sql: &str) -> String {
        let normalized = format!("LOWER(TRIM(COALESCE({request_kind_sql}, '')))");
        let non_billable_mcp = token_request_kind_non_billable_mcp_sql(request_kind_sql);
        format!(
            r#"
            CASE
                WHEN {normalized} IN ('api:research-result', 'api:usage', 'api:unknown-path')
                    THEN 'non_billable'
                WHEN {normalized} = 'mcp:batch' AND {counts_business_quota_sql} = 0
                    THEN 'non_billable'
                WHEN {non_billable_mcp}
                    THEN 'non_billable'
                ELSE 'billable'
            END
            "#
        )
    }

    async fn rebuild_observability_sidecar_api_key_usage_buckets_sql(
        pool: &SqlitePool,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        let request_kind_sql = "rl.request_kind_key";
        let request_value_bucket_sql =
            request_value_bucket_sql(request_kind_sql, "rl.request_body");
        let bucket_start_sql = Self::sidecar_local_day_bucket_start_sql("rl.created_at");
        let insert_sql = format!(
            r#"
            INSERT INTO observability.api_key_usage_buckets (
                api_key_id,
                bucket_start,
                bucket_secs,
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                valuable_success_count,
                valuable_failure_count,
                valuable_failure_429_count,
                other_success_count,
                other_failure_count,
                unknown_count,
                updated_at
            )
            SELECT
                rl.api_key_id,
                {bucket_start_sql} AS bucket_start,
                86400 AS bucket_secs,
                COUNT(*) AS total_requests,
                SUM(CASE WHEN rl.result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN rl.result_status = 'error' THEN 1 ELSE 0 END) AS error_count,
                SUM(CASE WHEN rl.result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status = 'success' THEN 1 ELSE 0 END) AS valuable_success_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status IN ('error', 'quota_exhausted') THEN 1 ELSE 0 END) AS valuable_failure_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status IN ('error', 'quota_exhausted') AND COALESCE(rl.failure_kind, '') = '{upstream_rate_limited_429}' THEN 1 ELSE 0 END) AS valuable_failure_429_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'other' AND rl.result_status = 'success' THEN 1 ELSE 0 END) AS other_success_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'other' AND rl.result_status IN ('error', 'quota_exhausted') THEN 1 ELSE 0 END) AS other_failure_count,
                SUM(CASE WHEN ({request_value_bucket_sql}) = 'unknown' THEN 1 ELSE 0 END) AS unknown_count,
                ? AS updated_at
            FROM observability.request_logs AS rl
            WHERE rl.visibility = 'visible'
              AND rl.api_key_id IS NOT NULL
            GROUP BY rl.api_key_id, bucket_start
            "#,
            upstream_rate_limited_429 = FAILURE_KIND_UPSTREAM_RATE_LIMITED_429
        );
        let mut conn = pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        let result = async {
            sqlx::query("DELETE FROM observability.api_key_usage_buckets")
                .execute(&mut *conn)
                .await?;
            sqlx::query(&insert_sql)
                .bind(updated_at)
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        if result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        result
    }

    async fn rebuild_observability_sidecar_dashboard_request_rollup_buckets_sql(
        pool: &SqlitePool,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        let request_kind_sql = "rl.request_kind_key";
        let counts_business_quota_sql =
            Self::sidecar_counts_business_quota_sql("rl", request_kind_sql);
        let request_value_bucket_sql =
            request_value_bucket_sql(request_kind_sql, "rl.request_body");
        let protocol_group_sql = format!(
            "CASE WHEN LOWER(TRIM(COALESCE({request_kind_sql}, ''))) LIKE 'mcp:%' THEN 'mcp' ELSE 'api' END"
        );
        let billing_group_sql =
            Self::sidecar_billing_group_sql(request_kind_sql, &counts_business_quota_sql);
        let select_sql = |bucket_start_sql: &str, bucket_secs: i64| {
            format!(
                r#"
                SELECT
                    {bucket_start_sql} AS bucket_start,
                    {bucket_secs} AS bucket_secs,
                    COUNT(*) AS total_requests,
                    SUM(CASE WHEN rl.result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(CASE WHEN rl.result_status = 'error' THEN 1 ELSE 0 END) AS error_count,
                    SUM(CASE WHEN rl.result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status = 'success' THEN 1 ELSE 0 END) AS valuable_success_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status IN ('error', 'quota_exhausted') THEN 1 ELSE 0 END) AS valuable_failure_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'valuable' AND rl.result_status IN ('error', 'quota_exhausted') AND COALESCE(rl.failure_kind, '') = '{upstream_rate_limited_429}' THEN 1 ELSE 0 END) AS valuable_failure_429_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'other' AND rl.result_status = 'success' THEN 1 ELSE 0 END) AS other_success_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'other' AND rl.result_status IN ('error', 'quota_exhausted') THEN 1 ELSE 0 END) AS other_failure_count,
                    SUM(CASE WHEN ({request_value_bucket_sql}) = 'unknown' THEN 1 ELSE 0 END) AS unknown_count,
                    SUM(CASE WHEN ({protocol_group_sql}) = 'mcp' AND ({billing_group_sql}) = 'non_billable' THEN 1 ELSE 0 END) AS mcp_non_billable,
                    SUM(CASE WHEN ({protocol_group_sql}) = 'mcp' AND ({billing_group_sql}) = 'billable' THEN 1 ELSE 0 END) AS mcp_billable,
                    SUM(CASE WHEN ({protocol_group_sql}) = 'api' AND ({billing_group_sql}) = 'non_billable' THEN 1 ELSE 0 END) AS api_non_billable,
                    SUM(CASE WHEN NOT (({protocol_group_sql}) = 'mcp' AND ({billing_group_sql}) = 'non_billable')
                              AND NOT (({protocol_group_sql}) = 'mcp' AND ({billing_group_sql}) = 'billable')
                              AND NOT (({protocol_group_sql}) = 'api' AND ({billing_group_sql}) = 'non_billable')
                             THEN 1 ELSE 0 END) AS api_billable,
                    SUM(MAX(COALESCE(rl.business_credits, 0), 0)) AS local_estimated_credits,
                    ? AS updated_at
                FROM observability.request_logs AS rl
                WHERE rl.visibility = 'visible'
                GROUP BY bucket_start
                "#,
                upstream_rate_limited_429 = FAILURE_KIND_UPSTREAM_RATE_LIMITED_429
            )
        };
        let minute_select = select_sql("(rl.created_at / 60) * 60", SECS_PER_MINUTE);
        let day_select = select_sql(
            &Self::sidecar_local_day_bucket_start_sql("rl.created_at"),
            SECS_PER_DAY,
        );
        let insert_sql = format!(
            r#"
            INSERT INTO observability.dashboard_request_rollup_buckets (
                bucket_start,
                bucket_secs,
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                valuable_success_count,
                valuable_failure_count,
                valuable_failure_429_count,
                other_success_count,
                other_failure_count,
                unknown_count,
                mcp_non_billable,
                mcp_billable,
                api_non_billable,
                api_billable,
                local_estimated_credits,
                updated_at
            )
            {minute_select}
            UNION ALL
            {day_select}
            "#
        );
        let mut conn = pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        let result = async {
            sqlx::query("DELETE FROM observability.dashboard_request_rollup_buckets")
                .execute(&mut *conn)
                .await?;
            sqlx::query(&insert_sql)
                .bind(updated_at)
                .bind(updated_at)
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        if result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        result
    }

    async fn rename_main_table_if_exists_in_pool(
        pool: &SqlitePool,
        table: &str,
        temporary_table: &str,
    ) -> Result<bool, ProxyError> {
        if !Self::main_table_exists_in_pool(pool, table).await? {
            if Self::main_table_exists_in_pool(pool, temporary_table).await? {
                return Ok(true);
            }
            return Ok(false);
        }
        if Self::main_table_exists_in_pool(pool, temporary_table).await? {
            return Err(ProxyError::Other(format!(
                "temporary migration table {temporary_table} already exists"
            )));
        }
        sqlx::query(&format!(
            r#"ALTER TABLE main."{table}" RENAME TO "{temporary_table}""#
        ))
        .execute(pool)
        .await?;
        Ok(true)
    }

    async fn restore_renamed_main_table_if_needed_in_pool(
        pool: &SqlitePool,
        table: &str,
        temporary_table: &str,
        renamed: bool,
    ) -> Result<(), ProxyError> {
        if !renamed {
            return Ok(());
        }
        if Self::main_table_exists_in_pool(pool, table).await? {
            return Err(ProxyError::Other(format!(
                "cannot restore {temporary_table}: main table {table} already exists"
            )));
        }
        sqlx::query(&format!(
            r#"ALTER TABLE main."{temporary_table}" RENAME TO "{table}""#
        ))
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn drop_renamed_main_table_if_needed_in_pool(
        pool: &SqlitePool,
        temporary_table: &str,
        renamed: bool,
    ) -> Result<bool, ProxyError> {
        if !renamed {
            return Ok(false);
        }
        sqlx::query(&format!(r#"DROP TABLE main."{temporary_table}""#))
            .execute(pool)
            .await?;
        Ok(true)
    }

    async fn rebuild_observability_sidecar_derived_tables_offline(
        &self,
        mark_explicit_cutover: bool,
    ) -> Result<ObservabilitySidecarDerivedRebuildReport, ProxyError> {
        self.ensure_meta_schema().await?;
        Self::ensure_observability_sidecar_derived_schema_in_pool(&self.pool).await?;
        let started = Instant::now();
        let temp_request_logs = "__observability_sidecar_legacy_request_logs";
        let temp_api_key_usage = "__observability_sidecar_legacy_api_key_usage_buckets";
        let temp_dashboard = "__observability_sidecar_legacy_dashboard_request_rollup_buckets";
        let temp_catalog = "__observability_sidecar_legacy_request_log_catalog_rollups";

        let request_logs_renamed =
            Self::rename_main_table_if_exists_in_pool(&self.pool, "request_logs", temp_request_logs)
                .await?;
        let api_key_usage_renamed = Self::rename_main_table_if_exists_in_pool(
            &self.pool,
            "api_key_usage_buckets",
            temp_api_key_usage,
        )
        .await?;
        let dashboard_renamed = Self::rename_main_table_if_exists_in_pool(
            &self.pool,
            "dashboard_request_rollup_buckets",
            temp_dashboard,
        )
        .await?;
        let catalog_renamed = Self::rename_main_table_if_exists_in_pool(
            &self.pool,
            "request_log_catalog_rollups",
            temp_catalog,
        )
        .await?;

        let rebuild_result = async {
            Self::rebuild_observability_sidecar_api_key_usage_buckets_sql(
                &self.pool,
                self.backend_time.now_ts(),
            )
            .await?;
            Self::rebuild_observability_sidecar_dashboard_request_rollup_buckets_sql(
                &self.pool,
                self.backend_time.now_ts(),
            )
            .await?;
            self.rebuild_server_pressure_buckets().await?;
            self.rebuild_request_log_catalog_rollups().await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if let Err(err) = rebuild_result {
            let _ = Self::restore_renamed_main_table_if_needed_in_pool(
                &self.pool,
                "request_log_catalog_rollups",
                temp_catalog,
                catalog_renamed,
            )
            .await;
            let _ = Self::restore_renamed_main_table_if_needed_in_pool(
                &self.pool,
                "dashboard_request_rollup_buckets",
                temp_dashboard,
                dashboard_renamed,
            )
            .await;
            let _ = Self::restore_renamed_main_table_if_needed_in_pool(
                &self.pool,
                "api_key_usage_buckets",
                temp_api_key_usage,
                api_key_usage_renamed,
            )
            .await;
            let _ = Self::restore_renamed_main_table_if_needed_in_pool(
                &self.pool,
                "request_logs",
                temp_request_logs,
                request_logs_renamed,
            )
            .await;
            return Err(err);
        }

        let dropped_main_request_logs =
            Self::drop_renamed_main_table_if_needed_in_pool(&self.pool, temp_request_logs, request_logs_renamed)
                .await?;
        let dropped_legacy_api_key_usage_buckets =
            Self::drop_renamed_main_table_if_needed_in_pool(&self.pool, temp_api_key_usage, api_key_usage_renamed)
                .await?;
        let dropped_legacy_dashboard_request_rollup_buckets =
            Self::drop_renamed_main_table_if_needed_in_pool(&self.pool, temp_dashboard, dashboard_renamed)
                .await?;
        let dropped_legacy_request_log_catalog_rollups =
            Self::drop_renamed_main_table_if_needed_in_pool(&self.pool, temp_catalog, catalog_renamed)
                .await?;

        let request_log_catalog_rollup_retention_days = self
            .get_system_settings()
            .await?
            .request_log_retention
            .max_log_retention_days;
        Self::mark_observability_sidecar_derived_meta_complete_in_pool(
            &self.pool,
            request_log_catalog_rollup_retention_days,
            mark_explicit_cutover,
        )
        .await?;

        Ok(ObservabilitySidecarDerivedRebuildReport {
            dropped_main_request_logs,
            dropped_legacy_api_key_usage_buckets,
            dropped_legacy_dashboard_request_rollup_buckets,
            dropped_legacy_request_log_catalog_rollups,
            rebuilt_api_key_usage_buckets: true,
            rebuilt_dashboard_request_rollup_buckets: true,
            rebuilt_request_log_catalog_rollups: true,
            marked_api_key_usage_buckets_meta_complete: true,
            marked_dashboard_request_rollup_buckets_meta_complete: true,
            marked_request_log_catalog_rollup_meta_complete: true,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn ensure_observability_sidecar_startup_rebuild_not_required(
        &self,
    ) -> Result<(), ProxyError> {
        if self.uses_legacy_single_db_observability_compatibility() {
            return Ok(());
        }
        if !self.observability_sidecar_has_cutover_request_logs().await? {
            return Ok(());
        }
        let explicit_cutover_done = self
            .get_meta_i64(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
            .await?
            == Some(1);
        let require_explicit_cutover_marker = core_database_file_size(&self.database_path)
            .unwrap_or(u64::MAX)
            > LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES;
        if require_explicit_cutover_marker && !explicit_cutover_done {
            return Err(ProxyError::Other(
                "observability sidecar derived tables are incomplete; stop the service and run observability_sidecar_migrate before startup".to_string(),
            ));
        }
        let sidecar_has_request_logs = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        if !sidecar_has_request_logs {
            return Ok(());
        }
        let explicit_cutover_done = self
            .get_meta_i64(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
            .await?
            == Some(1);
        if !explicit_cutover_done {
            return Ok(());
        }
        let sidecar_request_log_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                .fetch_one(&self.pool)
                .await?;
        if sidecar_request_log_rows == 0 {
            return Ok(());
        }
        let request_log_catalog_rollup_retention_days = self
            .get_system_settings()
            .await?
            .request_log_retention
            .max_log_retention_days;
        let api_key_usage_done = self
            .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
            .await?
            == Some(1)
            && self
                .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
                .await?
                == Some(1);
        let dashboard_done = self
            .get_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
            .await?
            == Some(1);
        let catalog_done = self
            .get_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
            .await?
            == Some(1)
            && self
                .get_meta_i64(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
                .await?
                == Some(request_log_catalog_rollup_retention_days);
        if api_key_usage_done && dashboard_done && catalog_done {
            return Ok(());
        }
        Err(ProxyError::Other(
            "observability sidecar derived tables are incomplete; stop the service and run observability_sidecar_migrate before startup".to_string(),
        ))
    }

    async fn observability_sidecar_has_cutover_request_logs(
        &self,
    ) -> Result<bool, ProxyError> {
        if self.uses_legacy_single_db_observability_compatibility() {
            return Ok(false);
        }
        if self.main_table_exists("request_logs").await? {
            return Ok(false);
        }
        let sidecar_has_request_logs = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        if !sidecar_has_request_logs {
            return Ok(false);
        }
        let sidecar_request_log_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                .fetch_one(&self.pool)
                .await?;
        Ok(sidecar_request_log_rows > 0)
    }

    async fn reject_large_sidecar_startup_rebuild(
        &self,
        reason: &str,
    ) -> Result<(), ProxyError> {
        if self
            .get_meta_i64(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
            .await?
            != Some(1)
        {
            return Ok(());
        }
        if core_database_file_size(&self.database_path).unwrap_or(u64::MAX)
            <= LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES
        {
            return Ok(());
        }
        if self.observability_sidecar_has_cutover_request_logs().await? {
            return Err(ProxyError::Other(format!(
                "observability sidecar derived tables require {reason}; stop the service and run observability_sidecar_migrate before startup"
            )));
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
        Self::ensure_observability_sidecar_derived_schema_in_pool(&store.pool).await?;
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
        if !std::path::Path::new(&layout.core_database_path).exists() {
            return Err(ProxyError::Other(format!(
                "missing core database {}",
                layout.core_database_path
            )));
        }
        let offline_probe = probe_observability_offline_state(&layout.core_database_path).await?;
        let offline_lock_acquired = if dry_run {
            offline_probe.service_lock_held_exclusively
        } else {
            true
        };

        let core_file_bytes = core_database_file_size(&layout.core_database_path)?;
        let available_bytes_before = available_disk_bytes_for_path(&layout.core_database_path)?;
        let sidecar_file_bytes_before = std::fs::metadata(&sidecar_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);

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
        let request_log_catalog_rollup_retention_days = Self::meta_i64_in_pool(
            &core_probe,
            META_KEY_REQUEST_LOG_RETENTION_MAX_DAYS_V1,
        )
        .await?
        .unwrap_or_else(|| {
            effective_request_logs_retention_days().min(REQUEST_LOG_RETENTION_DAYS_MAX)
        })
        .clamp(REQUEST_LOG_RETENTION_DAYS_MIN, REQUEST_LOG_RETENTION_DAYS_MAX);
        let explicit_cutover_meta_complete = Self::explicit_sidecar_cutover_meta_complete_in_pool(
            &core_probe,
            request_log_catalog_rollup_retention_days,
        )
        .await?;
        let temporary_legacy_request_logs_exists =
            Self::main_table_exists_in_pool(&core_probe, "__observability_sidecar_legacy_request_logs").await?;
        let temporary_legacy_api_key_usage_buckets_exists = Self::main_table_exists_in_pool(
            &core_probe,
            "__observability_sidecar_legacy_api_key_usage_buckets",
        )
        .await?;
        let temporary_legacy_dashboard_request_rollup_buckets_exists =
            Self::main_table_exists_in_pool(
                &core_probe,
                "__observability_sidecar_legacy_dashboard_request_rollup_buckets",
            )
            .await?;
        let temporary_legacy_request_log_catalog_rollups_exists =
            Self::main_table_exists_in_pool(
                &core_probe,
                "__observability_sidecar_legacy_request_log_catalog_rollups",
            )
            .await?;
        let attached_default = planned_observability_attach_path(
            &layout.core_database_path,
            layout.observability_database_path.as_deref(),
            legacy_request_logs_exists,
            true,
            false,
        );
        let large_legacy_fallback_active = attached_default
            .as_deref()
            .map(|path| sqlite_paths_match(path, &layout.core_database_path))
            .unwrap_or(false);

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
            && !legacy_request_log_catalog_rollups_exists
            && !temporary_legacy_request_logs_exists
            && !temporary_legacy_api_key_usage_buckets_exists
            && !temporary_legacy_dashboard_request_rollup_buckets_exists
            && !temporary_legacy_request_log_catalog_rollups_exists
            && explicit_cutover_meta_complete;

        let mut migration_state = ObservabilitySidecarMigrationState::default();
        if dry_run {
            migration_state.attached_observability_path = attached_default
                .clone()
                .unwrap_or_else(|| sidecar_path.clone());
            migration_state.sidecar_request_log_rows_before = sidecar_request_log_rows_before_probe;
            migration_state.sidecar_request_log_rows_after = sidecar_request_log_rows_before_probe;
            migration_state.startup_rebuild_required =
                !already_migrated && sidecar_request_log_rows_before_probe > 0;
        } else {
            let _offline_guard = acquire_observability_offline_guard(&layout.core_database_path)?;
            let store = Self::open_for_observability_sidecar_migration(database_path).await?;
            let result = async {
                migration_state.attached_observability_path = store
                    .observability_database_path
                    .clone()
                    .unwrap_or_else(|| layout.core_database_path.clone());
                migration_state.sidecar_request_log_rows_before =
                    sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                        .fetch_one(&store.pool)
                        .await?;

                if already_migrated {
                    store
                        .ensure_observability_sidecar_startup_rebuild_not_required()
                        .await?;
                    migration_state.sidecar_request_log_rows_after =
                        sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                            .fetch_one(&store.pool)
                            .await?;
                    migration_state.startup_reopen_verified = true;
                    migration_state.child_reference_checks_passed = true;
                    Ok::<(), ProxyError>(())
                } else {
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
                        migration_state.copied_request_logs = copied;
                        migration_state.batches = copied_batches;
                    }
                    Self::ensure_request_logs_rebuild_references_valid_in_pool(
                        &store.pool,
                        "request_logs schema migration produced invalid preserved references",
                    )
                    .await?;
                    migration_state.child_reference_checks_passed = true;

                        let derived_report = store
                            .rebuild_observability_sidecar_derived_tables_offline(true)
                            .await?;

                        migration_state.sidecar_request_log_rows_after =
                            sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
                                .fetch_one(&store.pool)
                                .await?;
                    migration_state.dropped_main_request_logs =
                        derived_report.dropped_main_request_logs;
                    migration_state.dropped_legacy_api_key_usage_buckets =
                        derived_report.dropped_legacy_api_key_usage_buckets;
                    migration_state.dropped_legacy_dashboard_request_rollup_buckets =
                        derived_report.dropped_legacy_dashboard_request_rollup_buckets;
                    migration_state.dropped_legacy_request_log_catalog_rollups =
                        derived_report.dropped_legacy_request_log_catalog_rollups;
                    migration_state.rebuilt_api_key_usage_buckets =
                        derived_report.rebuilt_api_key_usage_buckets;
                    migration_state.rebuilt_dashboard_request_rollup_buckets =
                        derived_report.rebuilt_dashboard_request_rollup_buckets;
                    migration_state.rebuilt_request_log_catalog_rollups =
                        derived_report.rebuilt_request_log_catalog_rollups;
                    migration_state.marked_api_key_usage_buckets_meta_complete =
                        derived_report.marked_api_key_usage_buckets_meta_complete;
                    migration_state.marked_dashboard_request_rollup_buckets_meta_complete =
                        derived_report.marked_dashboard_request_rollup_buckets_meta_complete;
                    migration_state.marked_request_log_catalog_rollup_meta_complete =
                        derived_report.marked_request_log_catalog_rollup_meta_complete;
                    migration_state.startup_reopen_verified = true;
                    migration_state.derived_rebuild_elapsed_ms = derived_report.elapsed_ms;
                    Ok::<(), ProxyError>(())
                }
            }
            .await;
            store.pool.close().await;
            drop(_offline_guard);
            if result.is_ok() && !already_migrated {
                crate::verify_observability_sidecar_reopen(&layout.core_database_path).await?;
            }
            result?;
        }
        let ObservabilitySidecarMigrationState {
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
            rebuilt_api_key_usage_buckets,
            rebuilt_dashboard_request_rollup_buckets,
            rebuilt_request_log_catalog_rollups,
            marked_api_key_usage_buckets_meta_complete,
            marked_dashboard_request_rollup_buckets_meta_complete,
            marked_request_log_catalog_rollup_meta_complete,
            startup_reopen_verified,
            startup_rebuild_required,
            derived_rebuild_elapsed_ms,
            child_reference_checks_passed,
        } = migration_state;
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
            rebuilt_api_key_usage_buckets,
            rebuilt_dashboard_request_rollup_buckets,
            rebuilt_request_log_catalog_rollups,
            marked_api_key_usage_buckets_meta_complete,
            marked_dashboard_request_rollup_buckets_meta_complete,
            marked_request_log_catalog_rollup_meta_complete,
            startup_reopen_verified,
            startup_rebuild_required,
            derived_rebuild_elapsed_ms,
            child_reference_checks_passed,
            batch_size,
            batches,
            completed: !dry_run
                && child_reference_checks_passed
                && startup_reopen_verified
                && !startup_rebuild_required
                && (already_migrated
                    || (rebuilt_api_key_usage_buckets
                        && rebuilt_dashboard_request_rollup_buckets
                        && rebuilt_request_log_catalog_rollups
                        && marked_api_key_usage_buckets_meta_complete
                        && marked_dashboard_request_rollup_buckets_meta_complete
                        && marked_request_log_catalog_rollup_meta_complete
                        && (!legacy_request_logs_exists || dropped_main_request_logs)
                        && (!legacy_api_key_usage_buckets_exists
                            || dropped_legacy_api_key_usage_buckets)
                        && (!legacy_dashboard_request_rollup_buckets_exists
                            || dropped_legacy_dashboard_request_rollup_buckets)
                        && (!legacy_request_log_catalog_rollups_exists
                            || dropped_legacy_request_log_catalog_rollups))),
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

        if legacy_api_key_usage_buckets || legacy_dashboard_rollups || legacy_catalog_rollups {
            self.rebuild_observability_sidecar_derived_tables_offline(false)
                .await?;
        }

        Ok(())
    }
}
