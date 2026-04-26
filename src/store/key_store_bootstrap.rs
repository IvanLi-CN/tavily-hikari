impl KeyStore {
    pub(crate) async fn new(database_path: &str) -> Result<Self, ProxyError> {
        let store = Self {
            pool: open_sqlite_pool(database_path, true, false).await?,
            token_binding_cache: RwLock::new(HashMap::new()),
            account_quota_resolution_cache: RwLock::new(HashMap::new()),
            request_logs_catalog_cache: RwLock::new(HashMap::new()),
            #[cfg(test)]
            forced_pending_claim_miss_log_ids: Mutex::new(HashSet::new()),
            forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(HashSet::new()),
        };
        store.initialize_schema().await?;
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT,
                auth_token_id TEXT,
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
                forwarded_headers TEXT,
                dropped_headers TEXT,
                visibility TEXT NOT NULL DEFAULT 'visible',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        let mut request_kind_schema_changed = self.upgrade_request_logs_schema().await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_auth_token_time
               ON request_logs(auth_token_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_time
               ON request_logs(created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_visibility_time
               ON request_logs(visibility, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_key_time
               ON request_logs(api_key_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_request_kind_time
               ON request_logs(request_kind_key, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_key_effect_time
               ON request_logs(key_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_binding_effect_time
               ON request_logs(binding_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_selection_effect_time
               ON request_logs(selection_effect_code, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        self.ensure_api_key_transient_backoffs_schema().await?;

        // API key usage rollups (for statistics that must not depend on request_logs retention).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_usage_buckets (
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
                PRIMARY KEY (api_key_id, bucket_start, bucket_secs),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        let api_key_usage_buckets_schema_changed = self
            .ensure_api_key_usage_bucket_request_value_columns()
            .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_usage_buckets_time
               ON api_key_usage_buckets(bucket_start DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_request_rollup_buckets (
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
            r#"CREATE INDEX IF NOT EXISTS idx_dashboard_request_rollup_buckets_scope_time
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
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_login_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

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
                request_log_id INTEGER REFERENCES request_logs(id),
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

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
                "ALTER TABLE auth_token_logs ADD COLUMN request_log_id INTEGER REFERENCES request_logs(id)",
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

        self.ensure_auth_token_logs_indexes().await?;
        self.migrate_log_effect_buckets().await?;

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
                key_id TEXT,
                status TEXT NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 1,
                message TEXT,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                FOREIGN KEY (key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Meta table for lightweight global key/value settings (e.g., migrations, rollup state)
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

        if request_kind_schema_changed {
            self.reset_request_kind_canonical_migration_v1_markers()
                .await?;
        }

        self.ensure_request_kind_canonical_migration_v1().await?;

        if self
            .get_meta_i64(META_KEY_API_KEY_CREATED_AT_BACKFILL_V1)
            .await?
            .is_none()
        {
            self.backfill_api_key_created_at().await?;
            self.set_meta_i64(
                META_KEY_API_KEY_CREATED_AT_BACKFILL_V1,
                Utc::now().timestamp(),
            )
            .await?;
        }

        // Backfill API key usage buckets exactly once. This enables safe request_logs retention
        // without changing the meaning of cumulative statistics.
        let api_key_usage_buckets_v1_done = self
            .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
            .await?
            .is_some();
        if !api_key_usage_buckets_v1_done {
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
            self.backfill_api_key_usage_bucket_request_value_counts_v2()
                .await?;
            self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE, 1)
                .await?;
        }

        if dashboard_request_rollup_buckets_schema_changed {
            self.set_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE, 0)
                .await?;
        }

        if self
            .get_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
            .await?
            != Some(1)
        {
            self.rebuild_dashboard_request_rollup_buckets().await?;
            self.set_meta_i64(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE, 1)
                .await?;
        }

        // After ensuring schemas, run the data consistency migration at most once.
        // Older versions incremented auth_tokens.total_requests during validation; this
        // migration reconciles those counters using auth_token_logs, then marks itself
        // as completed in the meta table so that future startups do not depend on
        // potentially truncated logs.
        if self
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
        if self
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
        if self
            .get_meta_i64(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
            .await?
            .is_none()
        {
            self.set_meta_i64(
                META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1,
                Utc::now().timestamp(),
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
                Utc::now().timestamp(),
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
                Utc::now().timestamp(),
            )
            .await?;
        }
        if self
            .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_V1_DONE)
            .await?
            .unwrap_or_default()
            <= 0
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
            self.set_meta_i64(META_KEY_FORCE_USER_RELOGIN_V1, Utc::now().timestamp())
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
                Utc::now().timestamp(),
            )
            .await?;
        }
        self.sync_linuxdo_system_tag_default_deltas_with_env()
            .await?;
        self.backfill_linuxdo_user_tag_bindings().await?;
        self.sync_account_quota_limits_with_defaults().await?;
        if self
            .get_meta_i64(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
            .await?
            != Some(start_of_month(Utc::now()).timestamp())
        {
            match rebase_current_month_business_quota_with_pool(
                &self.pool,
                Utc::now(),
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

}
