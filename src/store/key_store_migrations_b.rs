impl KeyStore {
    async fn rebuild_request_logs_table(
        &self,
        mode: RequestLogsRebuildMode,
    ) -> Result<(), ProxyError> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;

        let rebuild_result = self
            .rebuild_request_logs_table_with_foreign_keys_disabled(&mut conn, mode)
            .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }

        let reenable_result = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;

        match (rebuild_result, reenable_result) {
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err.into()),
            (Ok(_), Ok(_)) => Ok(()),
        }
    }

    async fn rebuild_request_logs_table_with_foreign_keys_disabled(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        mode: RequestLogsRebuildMode,
    ) -> Result<(), ProxyError> {
        sqlx::query("BEGIN IMMEDIATE").execute(&mut **conn).await?;
        sqlx::query("DROP TABLE IF EXISTS request_logs_new")
            .execute(&mut **conn)
            .await?;
        sqlx::query(REQUEST_LOGS_REBUILT_SCHEMA_SQL)
            .execute(&mut **conn)
            .await?;

        match mode {
            RequestLogsRebuildMode::DropLegacyApiKeyColumn => {
                sqlx::query(
                    r#"
                    INSERT INTO request_logs_new (
                        id,
                        api_key_id,
                        auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        business_credits,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        binding_effect_code,
                        binding_effect_summary,
                        selection_effect_code,
                        selection_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        visibility,
                        created_at
                    )
                    SELECT
                        id,
                        api_key_id,
                        NULL as auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        NULL AS request_kind_key,
                        NULL AS request_kind_label,
                        NULL AS request_kind_detail,
                        NULL AS business_credits,
                        NULL AS failure_kind,
                        'none' AS key_effect_code,
                        NULL AS key_effect_summary,
                        'none' AS binding_effect_code,
                        NULL AS binding_effect_summary,
                        'none' AS selection_effect_code,
                        NULL AS selection_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        ? AS visibility,
                        created_at
                    FROM request_logs
                    "#,
                )
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .execute(&mut **conn)
                .await?;
            }
            RequestLogsRebuildMode::RelaxApiKeyIdNullability => {
                sqlx::query(
                    r#"
                    INSERT INTO request_logs_new (
                        id,
                        api_key_id,
                        auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        business_credits,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        binding_effect_code,
                        binding_effect_summary,
                        selection_effect_code,
                        selection_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        visibility,
                        created_at
                    )
                    SELECT
                        id,
                        api_key_id,
                        auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        business_credits,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        binding_effect_code,
                        binding_effect_summary,
                        selection_effect_code,
                        selection_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        visibility,
                        created_at
                    FROM request_logs
                    "#,
                )
                .execute(&mut **conn)
                .await?;
            }
            RequestLogsRebuildMode::DropLegacyRequestKindColumns => {
                sqlx::query(
                    r#"
                    INSERT INTO request_logs_new (
                        id,
                        api_key_id,
                        auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        business_credits,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        visibility,
                        created_at
                    )
                    SELECT
                        id,
                        api_key_id,
                        auth_token_id,
                        method,
                        path,
                        query,
                        status_code,
                        tavily_status_code,
                        error_message,
                        result_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        business_credits,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        request_body,
                        response_body,
                        forwarded_headers,
                        dropped_headers,
                        visibility,
                        created_at
                    FROM request_logs
                    "#,
                )
                .execute(&mut **conn)
                .await?;
            }
        }

        sqlx::query("DROP TABLE request_logs")
            .execute(&mut **conn)
            .await?;
        sqlx::query("ALTER TABLE request_logs_new RENAME TO request_logs")
            .execute(&mut **conn)
            .await?;

        self.ensure_request_logs_rebuild_references_valid(
            conn,
            "request_logs schema migration produced invalid preserved references",
        )
        .await?;

        sqlx::query("COMMIT").execute(&mut **conn).await?;

        Ok(())
    }

    async fn rebuild_auth_token_logs_table(
        &self,
        mode: AuthTokenLogsRebuildMode,
    ) -> Result<(), ProxyError> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;

        let rebuild_result = self
            .rebuild_auth_token_logs_table_with_foreign_keys_disabled(&mut conn, mode)
            .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }

        let reenable_result = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;

        match (rebuild_result, reenable_result) {
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err.into()),
            (Ok(_), Ok(_)) => Ok(()),
        }
    }

    async fn rebuild_auth_token_logs_table_with_foreign_keys_disabled(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        mode: AuthTokenLogsRebuildMode,
    ) -> Result<(), ProxyError> {
        sqlx::query("BEGIN IMMEDIATE").execute(&mut **conn).await?;
        sqlx::query("DROP TABLE IF EXISTS auth_token_logs_new")
            .execute(&mut **conn)
            .await?;
        sqlx::query(AUTH_TOKEN_LOGS_REBUILT_SCHEMA_SQL)
            .execute(&mut **conn)
            .await?;

        match mode {
            AuthTokenLogsRebuildMode::DropLegacyRequestKindColumns => {
                sqlx::query(
                    r#"
                    INSERT INTO auth_token_logs_new (
                        id,
                        token_id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        result_status,
                        error_message,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        binding_effect_code,
                        binding_effect_summary,
                        selection_effect_code,
                        selection_effect_summary,
                        counts_business_quota,
                        business_credits,
                        billing_subject,
                        billing_state,
                        request_user_id,
                        api_key_id,
                        request_log_id,
                        created_at
                    )
                    SELECT
                        id,
                        token_id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                        result_status,
                        error_message,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        binding_effect_code,
                        binding_effect_summary,
                        selection_effect_code,
                        selection_effect_summary,
                        counts_business_quota,
                        business_credits,
                        billing_subject,
                        billing_state,
                        request_user_id,
                        api_key_id,
                        request_log_id,
                        created_at
                    FROM auth_token_logs
                    "#,
                )
                .execute(&mut **conn)
                .await?;
            }
        }

        sqlx::query("DROP TABLE auth_token_logs")
            .execute(&mut **conn)
            .await?;
        sqlx::query("ALTER TABLE auth_token_logs_new RENAME TO auth_token_logs")
            .execute(&mut **conn)
            .await?;

        self.ensure_auth_token_logs_rebuild_references_valid(
            conn,
            "auth_token_logs schema migration produced invalid preserved references",
        )
        .await?;

        sqlx::query("COMMIT").execute(&mut **conn).await?;
        Ok(())
    }

    async fn ensure_auth_token_logs_rebuild_references_valid(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        context: &str,
    ) -> Result<(), ProxyError> {
        let rows = sqlx::query("PRAGMA foreign_key_check('auth_token_logs')")
            .fetch_all(&mut **conn)
            .await?;
        if !rows.is_empty() {
            let details = rows
                .into_iter()
                .take(5)
                .map(|row| {
                    let table = row
                        .try_get::<String, _>(0)
                        .unwrap_or_else(|_| "<unknown-table>".to_string());
                    let rowid = row.try_get::<i64, _>(1).unwrap_or_default();
                    let parent = row
                        .try_get::<String, _>(2)
                        .unwrap_or_else(|_| "<unknown-parent>".to_string());
                    let fk_index = row.try_get::<i64, _>(3).unwrap_or_default();
                    format!("{table}[rowid={rowid}] -> {parent} (fk#{fk_index})")
                })
                .collect::<Vec<_>>()
                .join("; ");

            return Err(ProxyError::Other(format!("{context}: {details}")));
        }

        self.ensure_auth_token_logs_child_reference_integrity(
            conn,
            "api_key_maintenance_records",
            context,
        )
        .await
    }

    async fn ensure_auth_token_logs_child_reference_integrity(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        table: &str,
        context: &str,
    ) -> Result<(), ProxyError> {
        let table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&mut **conn)
        .await?;
        if table_exists.is_none() {
            return Ok(());
        }

        let has_auth_token_log_id = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info(?) WHERE name = 'auth_token_log_id' LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&mut **conn)
        .await?;
        if has_auth_token_log_id.is_none() {
            return Ok(());
        }

        let query = format!(
            "SELECT rowid, auth_token_log_id FROM {table} \
             WHERE auth_token_log_id IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM auth_token_logs WHERE auth_token_logs.id = {table}.auth_token_log_id) \
             ORDER BY rowid ASC LIMIT 5"
        );
        let rows = sqlx::query(&query).fetch_all(&mut **conn).await?;
        if rows.is_empty() {
            return Ok(());
        }

        let details = rows
            .into_iter()
            .map(|row| {
                let rowid = row.try_get::<i64, _>("rowid").unwrap_or_default();
                let auth_token_log_id = row
                    .try_get::<i64, _>("auth_token_log_id")
                    .unwrap_or_default();
                format!("{table}[rowid={rowid}] -> auth_token_logs[id={auth_token_log_id}]")
            })
            .collect::<Vec<_>>()
            .join("; ");

        Err(ProxyError::Other(format!("{context}: {details}")))
    }

    async fn ensure_auth_token_logs_indexes(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_token_time ON auth_token_logs(token_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_billable_id
               ON auth_token_logs(counts_business_quota, id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_token_request_kind_time
               ON auth_token_logs(token_id, request_kind_key, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_billing_pending
               ON auth_token_logs(billing_state, billing_subject, id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_api_key_time
               ON auth_token_logs(api_key_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_request_log_id
               ON auth_token_logs(request_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_binding_effect_time
               ON auth_token_logs(token_id, binding_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_request_user_time
               ON auth_token_logs(request_user_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_selection_effect_time
               ON auth_token_logs(token_id, selection_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_request_logs_rebuild_references_valid(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        context: &str,
    ) -> Result<(), ProxyError> {
        let rows = sqlx::query("PRAGMA foreign_key_check('request_logs')")
            .fetch_all(&mut **conn)
            .await?;
        if !rows.is_empty() {
            let details = rows
                .into_iter()
                .take(5)
                .map(|row| {
                    let table = row
                        .try_get::<String, _>(0)
                        .unwrap_or_else(|_| "<unknown-table>".to_string());
                    let rowid = row.try_get::<i64, _>(1).unwrap_or_default();
                    let parent = row
                        .try_get::<String, _>(2)
                        .unwrap_or_else(|_| "<unknown-parent>".to_string());
                    let fk_index = row.try_get::<i64, _>(3).unwrap_or_default();
                    format!("{table}[rowid={rowid}] -> {parent} (fk#{fk_index})")
                })
                .collect::<Vec<_>>()
                .join("; ");

            return Err(ProxyError::Other(format!("{context}: {details}")));
        }

        self.ensure_request_logs_child_reference_integrity(conn, "auth_token_logs", context)
            .await?;
        self.ensure_request_logs_child_reference_integrity(
            conn,
            "api_key_maintenance_records",
            context,
        )
        .await
    }

    async fn ensure_request_logs_child_reference_integrity(
        &self,
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
        table: &str,
        context: &str,
    ) -> Result<(), ProxyError> {
        let table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&mut **conn)
        .await?;
        if table_exists.is_none() {
            return Ok(());
        }

        let has_request_log_id = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info(?) WHERE name = 'request_log_id' LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&mut **conn)
        .await?;
        if has_request_log_id.is_none() {
            return Ok(());
        }

        let query = format!(
            "SELECT rowid, request_log_id FROM {table} \
             WHERE request_log_id IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM request_logs WHERE request_logs.id = {table}.request_log_id) \
             ORDER BY rowid ASC LIMIT 5"
        );
        let rows = sqlx::query(&query).fetch_all(&mut **conn).await?;
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

    pub(crate) async fn upgrade_api_keys_schema(&self) -> Result<(), ProxyError> {
        // Track whether legacy column existed to gate one-time migration logic
        let had_disabled_at = self.api_keys_column_exists("disabled_at").await?;
        if had_disabled_at {
            sqlx::query("ALTER TABLE api_keys RENAME COLUMN disabled_at TO status_changed_at")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("status").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN status TEXT NOT NULL DEFAULT 'active'")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("status_changed_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN status_changed_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("group_name").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN group_name TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("registration_ip").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN registration_ip TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("registration_region").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN registration_region TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.api_keys_column_exists("created_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Add deleted_at for soft delete marker (timestamp)
        if !self.api_keys_column_exists("deleted_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN deleted_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        // Quota tracking columns for Tavily usage
        if !self.api_keys_column_exists("quota_limit").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_limit INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.api_keys_column_exists("quota_remaining").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_remaining INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.api_keys_column_exists("quota_synced_at").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN quota_synced_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        // Migrate legacy status='deleted' into deleted_at and normalize status
        let legacy_deleted = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM api_keys WHERE status = 'deleted' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        if legacy_deleted.is_some() {
            let now = Utc::now().timestamp();
            sqlx::query(
                r#"UPDATE api_keys
                   SET deleted_at = COALESCE(status_changed_at, ?)
                   WHERE status = 'deleted' AND (deleted_at IS NULL OR deleted_at = 0)"#,
            )
            .bind(now)
            .execute(&self.pool)
            .await?;

            sqlx::query("UPDATE api_keys SET status = 'active' WHERE status = 'deleted'")
                .execute(&self.pool)
                .await?;
        }

        // Only when migrating from legacy 'disabled_at' do we mark keys as exhausted.
        if had_disabled_at {
            sqlx::query(
                r#"
                UPDATE api_keys
                SET status = ?
                WHERE status_changed_at IS NOT NULL
                  AND status_changed_at != 0
                  AND status <> ?
                "#,
            )
            .bind(STATUS_EXHAUSTED)
            .bind(STATUS_EXHAUSTED)
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?
            WHERE status IS NULL
               OR status = ''
            "#,
        )
        .bind(STATUS_ACTIVE)
        .execute(&self.pool)
        .await?;

        self.ensure_api_key_ids().await?;
        self.ensure_api_keys_primary_key().await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_api_keys_created_at ON api_keys(created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn backfill_api_key_created_at(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET created_at = COALESCE(
                (
                    SELECT MIN(candidate_ts)
                    FROM (
                        SELECT MIN(r.created_at) AS candidate_ts
                        FROM request_logs r
                        WHERE r.api_key_id = api_keys.id
                        UNION ALL
                        SELECT MIN(q.created_at) AS candidate_ts
                        FROM api_key_quarantines q
                        WHERE q.key_id = api_keys.id
                    ) candidates
                    WHERE candidate_ts IS NOT NULL
                      AND candidate_ts > 0
                ),
                0
            )
            WHERE created_at IS NULL OR created_at <= 0
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn ensure_api_key_quarantines_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_quarantines (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                source TEXT NOT NULL,
                reason_code TEXT NOT NULL,
                reason_summary TEXT NOT NULL,
                reason_detail TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                cleared_at INTEGER,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_quarantines_active ON api_key_quarantines(key_id) WHERE cleared_at IS NULL",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_api_key_quarantines_key_created ON api_key_quarantines(key_id, created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_api_key_quarantines_created_at ON api_key_quarantines(created_at DESC, key_id)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn ensure_api_key_quota_sync_samples_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_quota_sync_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key_id TEXT NOT NULL,
                quota_limit INTEGER NOT NULL,
                quota_remaining INTEGER NOT NULL,
                captured_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_quota_sync_samples_key_captured
               ON api_key_quota_sync_samples(key_id, captured_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_quota_sync_samples_captured
               ON api_key_quota_sync_samples(captured_at DESC, key_id)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn ensure_api_key_low_quota_depletions_schema(
        &self,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_low_quota_depletions (
                key_id TEXT NOT NULL,
                month_start INTEGER NOT NULL,
                threshold INTEGER NOT NULL,
                quota_remaining INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (key_id, month_start),
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_low_quota_depletions_month
               ON api_key_low_quota_depletions(month_start, key_id)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn ensure_api_key_ids(&self) -> Result<(), ProxyError> {
        if !self.api_keys_column_exists("id").await? {
            sqlx::query("ALTER TABLE api_keys ADD COLUMN id TEXT")
                .execute(&self.pool)
                .await?;
        }

        let mut tx = self.pool.begin().await?;
        let keys = sqlx::query_scalar::<_, String>(
            "SELECT api_key FROM api_keys WHERE id IS NULL OR id = ''",
        )
        .fetch_all(&mut *tx)
        .await?;

        for api_key in keys {
            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query("UPDATE api_keys SET id = ? WHERE api_key = ?")
                .bind(&id)
                .bind(&api_key)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn ensure_api_keys_primary_key(&self) -> Result<(), ProxyError> {
        if self.api_keys_primary_key_is_id().await? {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        // Ensure the temp table schema is up-to-date even if a previous migration attempt left it behind.
        sqlx::query("DROP TABLE IF EXISTS api_keys_new")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE api_keys_new (
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
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO api_keys_new (
                id,
                api_key,
                group_name,
                registration_ip,
                registration_region,
                status,
                created_at,
                status_changed_at,
                last_used_at,
                quota_limit,
                quota_remaining,
                quota_synced_at,
                deleted_at
            )
            SELECT
                id,
                api_key,
                group_name,
                registration_ip,
                registration_region,
                status,
                created_at,
                status_changed_at,
                last_used_at,
                quota_limit,
                quota_remaining,
                quota_synced_at,
                deleted_at
            FROM api_keys
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("DROP TABLE api_keys").execute(&mut *tx).await?;
        sqlx::query("ALTER TABLE api_keys_new RENAME TO api_keys")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn api_keys_primary_key_is_id(&self) -> Result<bool, ProxyError> {
        let rows = sqlx::query("SELECT name, pk FROM pragma_table_info('api_keys')")
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let name: String = row.try_get("name")?;
            let pk: i64 = row.try_get("pk")?;
            if name == "id" {
                return Ok(pk > 0);
            }
        }

        Ok(false)
    }

    pub(crate) async fn generate_unique_key_id(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<String, ProxyError> {
        loop {
            let candidate = nanoid!(4);
            let exists = sqlx::query_scalar::<_, Option<String>>(
                "SELECT id FROM api_keys WHERE id = ? LIMIT 1",
            )
            .bind(&candidate)
            .fetch_optional(&mut **tx)
            .await?;

            if exists.is_none() {
                return Ok(candidate);
            }
        }
    }

    pub(crate) async fn api_keys_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('api_keys') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;

        Ok(exists.is_some())
    }

    pub(crate) async fn upgrade_request_logs_schema(&self) -> Result<bool, ProxyError> {
        if !self.request_logs_column_exists("result_status").await? {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN result_status TEXT NOT NULL DEFAULT 'unknown'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .request_logs_column_exists("tavily_status_code")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN tavily_status_code INTEGER")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("forwarded_headers").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN forwarded_headers TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("dropped_headers").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN dropped_headers TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("failure_kind").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN failure_kind TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("visibility").await? {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN visibility TEXT NOT NULL DEFAULT 'visible'",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            "UPDATE request_logs
             SET visibility = ?
             WHERE visibility IS NULL OR TRIM(visibility) = ''",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .execute(&self.pool)
        .await?;

        if !self.request_logs_column_exists("key_effect_code").await? {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN key_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .request_logs_column_exists("key_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN key_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .request_logs_column_exists("binding_effect_code")
            .await?
        {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN binding_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .request_logs_column_exists("binding_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN binding_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        if !self
            .request_logs_column_exists("selection_effect_code")
            .await?
        {
            sqlx::query(
                "ALTER TABLE request_logs ADD COLUMN selection_effect_code TEXT NOT NULL DEFAULT 'none'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self
            .request_logs_column_exists("selection_effect_summary")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN selection_effect_summary TEXT")
                .execute(&self.pool)
                .await?;
        }

        for (column, sql) in [
            (
                "gateway_mode",
                "ALTER TABLE request_logs ADD COLUMN gateway_mode TEXT",
            ),
            (
                "experiment_variant",
                "ALTER TABLE request_logs ADD COLUMN experiment_variant TEXT",
            ),
            (
                "proxy_session_id",
                "ALTER TABLE request_logs ADD COLUMN proxy_session_id TEXT",
            ),
            (
                "routing_subject_hash",
                "ALTER TABLE request_logs ADD COLUMN routing_subject_hash TEXT",
            ),
            (
                "upstream_operation",
                "ALTER TABLE request_logs ADD COLUMN upstream_operation TEXT",
            ),
            (
                "fallback_reason",
                "ALTER TABLE request_logs ADD COLUMN fallback_reason TEXT",
            ),
        ] {
            if !self.request_logs_column_exists(column).await? {
                sqlx::query(sql).execute(&self.pool).await?;
            }
        }

        let mut request_kind_schema_changed =
            self.ensure_request_logs_request_kind_columns().await?;

        request_kind_schema_changed |= self.ensure_request_logs_key_ids().await?;

        Ok(request_kind_schema_changed)
    }

    pub(crate) async fn ensure_request_logs_key_ids(&self) -> Result<bool, ProxyError> {
        let mut request_kind_schema_changed = false;

        if !self.request_logs_column_exists("api_key_id").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN api_key_id TEXT")
                .execute(&self.pool)
                .await?;

            sqlx::query(
                r#"
                UPDATE request_logs
                SET api_key_id = (
                    SELECT id FROM api_keys WHERE api_keys.api_key = request_logs.api_key
                )
                "#,
            )
            .execute(&self.pool)
            .await?;
        }

        if self.request_logs_column_exists("api_key").await? {
            self.rebuild_request_logs_table(RequestLogsRebuildMode::DropLegacyApiKeyColumn)
                .await?;
            request_kind_schema_changed = true;
        }

        if self
            .table_column_not_null("request_logs", "api_key_id")
            .await?
        {
            self.rebuild_request_logs_table(RequestLogsRebuildMode::RelaxApiKeyIdNullability)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self.request_logs_column_exists("request_body").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN request_body BLOB")
                .execute(&self.pool)
                .await?;
        }

        if !self.request_logs_column_exists("auth_token_id").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN auth_token_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        if self.request_logs_have_legacy_request_kind_columns().await? {
            self.rebuild_request_logs_table(RequestLogsRebuildMode::DropLegacyRequestKindColumns)
                .await?;
            request_kind_schema_changed = true;
        }

        request_kind_schema_changed |= self.ensure_request_logs_request_kind_columns().await?;

        Ok(request_kind_schema_changed)
    }

    async fn ensure_request_logs_request_kind_columns(&self) -> Result<bool, ProxyError> {
        let mut request_kind_schema_changed = false;

        if !self.request_logs_column_exists("request_kind_key").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN request_kind_key TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .request_logs_column_exists("request_kind_label")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN request_kind_label TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .request_logs_column_exists("request_kind_detail")
            .await?
        {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN request_kind_detail TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self.request_logs_column_exists("business_credits").await? {
            sqlx::query("ALTER TABLE request_logs ADD COLUMN business_credits INTEGER")
                .execute(&self.pool)
                .await?;
        }

        Ok(request_kind_schema_changed)
    }

    async fn request_logs_have_legacy_request_kind_columns(&self) -> Result<bool, ProxyError> {
        Ok(self
            .request_logs_column_exists("legacy_request_kind_key")
            .await?
            || self
                .request_logs_column_exists("legacy_request_kind_label")
                .await?
            || self
                .request_logs_column_exists("legacy_request_kind_detail")
                .await?)
    }

    async fn ensure_auth_token_logs_request_kind_columns(&self) -> Result<bool, ProxyError> {
        let mut request_kind_schema_changed = false;

        if !self
            .table_column_exists("auth_token_logs", "request_kind_key")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_key TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .table_column_exists("auth_token_logs", "request_kind_label")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_label TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        if !self
            .table_column_exists("auth_token_logs", "request_kind_detail")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN request_kind_detail TEXT")
                .execute(&self.pool)
                .await?;
            request_kind_schema_changed = true;
        }

        Ok(request_kind_schema_changed)
    }

    async fn auth_token_logs_have_legacy_request_kind_columns(&self) -> Result<bool, ProxyError> {
        Ok(self
            .table_column_exists("auth_token_logs", "legacy_request_kind_key")
            .await?
            || self
                .table_column_exists("auth_token_logs", "legacy_request_kind_label")
                .await?
            || self
                .table_column_exists("auth_token_logs", "legacy_request_kind_detail")
                .await?)
    }

    pub(crate) async fn request_logs_column_exists(
        &self,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('request_logs') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;

        Ok(exists.is_some())
    }

}
