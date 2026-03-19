use crate::analysis::*;
use crate::models::*;
use crate::tavily_proxy::QuotaSubjectDbLease;
use crate::*;

pub(crate) fn is_transient_sqlite_write_error(err: &ProxyError) -> bool {
    let ProxyError::Database(db_err) = err else {
        return false;
    };
    let sqlx::Error::Database(db_err) = db_err else {
        return false;
    };

    if let Some(code) = db_err.code() {
        match code.as_ref() {
            // SQLite primary and extended codes for lock/busy states.
            "5" | "6" | "261" | "262" | "517" | "518" | "SQLITE_BUSY" | "SQLITE_LOCKED" => {
                return true;
            }
            _ => {}
        }
    }

    let message = db_err.message().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("database is busy")
}

pub(crate) fn is_invalid_current_month_billing_subject_error(err: &ProxyError) -> bool {
    match err {
        ProxyError::QuotaDataMissing { reason } => {
            reason.contains("charged auth_token_logs rows with invalid billing_subject")
        }
        _ => false,
    }
}

pub(crate) async fn open_sqlite_pool(
    database_path: &str,
    create_if_missing: bool,
    read_only: bool,
) -> Result<SqlitePool, ProxyError> {
    let mut options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5));
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(ProxyError::Database)
}

pub(crate) async fn begin_immediate_sqlite_connection(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    Ok(conn)
}

pub(crate) async fn begin_read_snapshot_sqlite_connection(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN").execute(&mut *conn).await?;
    Ok(conn)
}

#[derive(Debug)]
pub(crate) struct KeyStore {
    pub(crate) pool: SqlitePool,
    pub(crate) token_binding_cache: RwLock<HashMap<String, TokenBindingCacheEntry>>,
    pub(crate) account_quota_resolution_cache:
        RwLock<HashMap<String, AccountQuotaResolutionCacheEntry>>,
    #[cfg(test)]
    pub(crate) forced_pending_claim_miss_log_ids: Mutex<HashSet<i64>>,
    // Lightweight failpoint registry used by integration tests to simulate a lost quota
    // subject lease after precheck but before settlement.
    pub(crate) forced_quota_subject_lock_loss_subjects: std::sync::Mutex<HashSet<String>>,
}

impl KeyStore {
    pub(crate) async fn new(database_path: &str) -> Result<Self, ProxyError> {
        let store = Self {
            pool: open_sqlite_pool(database_path, true, false).await?,
            token_binding_cache: RwLock::new(HashMap::new()),
            account_quota_resolution_cache: RwLock::new(HashMap::new()),
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT NOT NULL,
                auth_token_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                request_body BLOB,
                response_body BLOB,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.upgrade_request_logs_schema().await?;

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
            r#"CREATE INDEX IF NOT EXISTS idx_request_logs_key_time
               ON request_logs(api_key_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

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
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (api_key_id, bucket_start, bucket_secs),
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_usage_buckets_time
               ON api_key_usage_buckets(bucket_start DESC)"#,
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
                counts_business_quota INTEGER NOT NULL DEFAULT 1,
                business_credits INTEGER,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                api_key_id TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_token_time ON auth_token_logs(token_id, created_at DESC, id DESC)"#,
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

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_billing_pending
               ON auth_token_logs(billing_state, billing_subject, id)"#,
        )
        .execute(&self.pool)
        .await?;

        if !self
            .table_column_exists("auth_token_logs", "api_key_id")
            .await?
        {
            sqlx::query("ALTER TABLE auth_token_logs ADD COLUMN api_key_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_token_logs_api_key_time
               ON auth_token_logs(api_key_id, created_at DESC, id DESC)"#,
        )
        .execute(&self.pool)
        .await?;

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

        if request_kind_schema_changed
            || self
                .get_meta_i64(META_KEY_AUTH_TOKEN_LOG_REQUEST_KIND_BACKFILL_V1)
                .await?
                .is_none()
        {
            self.backfill_auth_token_log_request_kinds().await?;
            self.set_meta_i64(META_KEY_AUTH_TOKEN_LOG_REQUEST_KIND_BACKFILL_V1, 1)
                .await?;
        }

        // Backfill API key usage buckets exactly once. This enables safe request_logs retention
        // without changing the meaning of cumulative statistics.
        if self
            .get_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
            .await?
            .is_none()
        {
            self.migrate_api_key_usage_buckets_v1().await?;
            self.set_meta_i64(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE, 1)
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

    pub(crate) async fn ensure_dev_open_admin_token(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            ) VALUES (?, ?, 0, ?, NULL, 0, ?, NULL, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(DEV_OPEN_ADMIN_TOKEN_ID)
        .bind(DEV_OPEN_ADMIN_TOKEN_SECRET)
        .bind(DEV_OPEN_ADMIN_TOKEN_NOTE)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn user_token_bindings_uses_single_binding_primary_key(
        &self,
    ) -> Result<bool, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT name, pk FROM pragma_table_info('user_token_bindings')",
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(false);
        }

        let mut user_id_pk = 0;
        let mut token_id_pk = 0;
        for (name, pk) in rows {
            if name == "user_id" {
                user_id_pk = pk;
            } else if name == "token_id" {
                token_id_pk = pk;
            }
        }

        Ok(user_id_pk == 1 && token_id_pk == 0)
    }

    pub(crate) async fn migrate_user_token_bindings_to_multi_binding(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .user_token_bindings_uses_single_binding_primary_key()
            .await?
        {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            CREATE TABLE user_token_bindings_v2 (
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
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_token_bindings_v2 (user_id, token_id, created_at, updated_at)
               SELECT user_id, token_id, created_at, updated_at
               FROM user_token_bindings"#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE user_token_bindings")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE user_token_bindings_v2 RENAME TO user_token_bindings")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn force_user_relogin_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE user_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn migrate_api_key_usage_buckets_v1(&self) -> Result<(), ProxyError> {
        // Rebuild buckets from request_logs to preserve cumulative statistics after retention.
        // This is safe to rerun because we clear and recompute deterministically.
        let now_ts = Utc::now().timestamp();
        let mut read_conn = self.pool.acquire().await?;
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM api_key_usage_buckets")
            .execute(&mut *tx)
            .await?;

        let mut rows = sqlx::query(
            r#"
            SELECT api_key_id, created_at, result_status
            FROM request_logs
            ORDER BY api_key_id ASC, created_at ASC, id ASC
            "#,
        )
        .fetch(&mut *read_conn);

        #[derive(Clone, Copy, Default)]
        struct BucketCounts {
            total_requests: i64,
            success_count: i64,
            error_count: i64,
            quota_exhausted_count: i64,
        }

        async fn flush_bucket(
            tx: &mut Transaction<'_, Sqlite>,
            now_ts: i64,
            key: &str,
            bucket_start: i64,
            counts: BucketCounts,
        ) -> Result<(), ProxyError> {
            if counts.total_requests <= 0 {
                return Ok(());
            }
            sqlx::query(
                r#"
                INSERT INTO api_key_usage_buckets (
                    api_key_id,
                    bucket_start,
                    bucket_secs,
                    total_requests,
                    success_count,
                    error_count,
                    quota_exhausted_count,
                    updated_at
                ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(key)
            .bind(bucket_start)
            .bind(counts.total_requests)
            .bind(counts.success_count)
            .bind(counts.error_count)
            .bind(counts.quota_exhausted_count)
            .bind(now_ts)
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut current_key: Option<String> = None;
        let mut current_bucket_start: i64 = 0;
        let mut counts = BucketCounts::default();

        while let Some(row) = rows.try_next().await? {
            let key_id: String = row.try_get("api_key_id")?;
            let created_at: i64 = row.try_get("created_at")?;
            let status: String = row.try_get("result_status")?;

            let bucket_start = local_day_bucket_start_utc_ts(created_at);

            let needs_flush = match current_key.as_deref() {
                None => false,
                Some(k) if k != key_id.as_str() => true,
                Some(_) if current_bucket_start != bucket_start => true,
                _ => false,
            };

            if needs_flush {
                let key = current_key.as_deref().expect("flush key present");
                flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;

                counts = BucketCounts::default();
            }

            current_key = Some(key_id);
            current_bucket_start = bucket_start;

            counts.total_requests += 1;
            match status.as_str() {
                OUTCOME_SUCCESS => counts.success_count += 1,
                OUTCOME_ERROR => counts.error_count += 1,
                OUTCOME_QUOTA_EXHAUSTED => counts.quota_exhausted_count += 1,
                _ => {}
            }
        }

        if let Some(key) = current_key.as_deref() {
            flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Reconcile derived fields to ensure cross-table consistency.
    /// This migration is idempotent and safe to run on every startup.
    pub(crate) async fn migrate_data_consistency(&self) -> Result<(), ProxyError> {
        // 1) Access tokens: recompute total_requests and last_used_at from auth_token_logs
        //    Older versions incremented total_requests during validation, which
        //    inflated counters. The canonical source of truth is auth_token_logs.
        sqlx::query(
            r#"
            UPDATE auth_tokens
            SET total_requests = COALESCE((
                    SELECT COUNT(*) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                ), 0),
                last_used_at = (
                    SELECT MAX(created_at) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 2) API keys: refresh last_used_at from request_logs to avoid stale values
        //    (This is a best-effort consistency update; it's safe and general.)
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = COALESCE((
                SELECT MAX(created_at) FROM request_logs r WHERE r.api_key_id = api_keys.id
            ), last_used_at)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Ensure that every token_id referenced in auth_token_logs has a corresponding
    /// auth_tokens row. Missing rows are backfilled as disabled, soft-deleted tokens
    /// so that downstream usage aggregation into token_usage_stats (with FOREIGN KEYs)
    /// does not fail for legacy data.
    pub(crate) async fn heal_orphan_auth_tokens_from_logs(&self) -> Result<(), ProxyError> {
        // Skip if auth_token_logs table does not exist (very old databases).
        let has_logs_table = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'auth_token_logs' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        if has_logs_table.is_none() {
            self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, 0).await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            )
            SELECT
                l.token_id,
                'restored-from-logs',
                0,
                '[auto-restored from logs]',
                NULL,
                COUNT(*) AS total_requests,
                MIN(l.created_at) AS created_at,
                MAX(l.created_at) AS last_used_at,
                ?
            FROM auth_token_logs l
            LEFT JOIN auth_tokens t ON t.id = l.token_id
            WHERE t.id IS NULL
            GROUP BY l.token_id
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Record completion so this healer is only ever run once per database.
        self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, now)
            .await?;

        Ok(())
    }

    pub(crate) async fn backfill_account_quota_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let hourly_any_limit = effective_token_hourly_request_limit();
        let hourly_limit = effective_token_hourly_limit();
        let daily_limit = effective_token_daily_limit();
        let monthly_limit = effective_token_monthly_limit();

        // Ensure every bound account has a default limits row.
        sqlx::query(
            r#"
            INSERT INTO account_quota_limits (
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
                created_at,
                updated_at
            )
            SELECT
                b.user_id,
                ?,
                ?,
                ?,
                ?,
                ?,
                ?
            FROM user_token_bindings b
            GROUP BY b.user_id
            ON CONFLICT(user_id) DO NOTHING
            "#,
        )
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Copy existing token rolling buckets to account scope.
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            SELECT
                b.user_id,
                u.bucket_start,
                u.granularity,
                SUM(u.count) AS count
            FROM user_token_bindings b
            JOIN token_usage_buckets u ON u.token_id = b.token_id
            GROUP BY b.user_id, u.bucket_start, u.granularity
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = excluded.count
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Copy monthly counters to account scope. If multiple tokens map to one account,
        // keep the latest month_start and aggregate counts in that month.
        sqlx::query(
            r#"
            WITH mapped AS (
                SELECT b.user_id AS user_id, q.month_start AS month_start, q.month_count AS month_count
                FROM user_token_bindings b
                JOIN auth_token_quota q ON q.token_id = b.token_id
            ),
            latest AS (
                SELECT user_id, MAX(month_start) AS latest_month_start
                FROM mapped
                GROUP BY user_id
            )
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            SELECT
                l.user_id,
                l.latest_month_start,
                COALESCE(SUM(CASE WHEN m.month_start = l.latest_month_start THEN m.month_count ELSE 0 END), 0)
            FROM latest l
            LEFT JOIN mapped m ON m.user_id = l.user_id
            GROUP BY l.user_id, l.latest_month_start
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn increment_usage_bucket_by(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(token_id, bucket_start, granularity)
            DO UPDATE SET count = token_usage_buckets.count + excluded.count
            "#,
        )
        .bind(token_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn increment_usage_bucket(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_usage_bucket_by(token_id, bucket_start, granularity, 1)
            .await
    }

    pub(crate) async fn increment_account_usage_bucket_by(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = account_usage_buckets.count + excluded.count
            "#,
        )
        .bind(user_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn increment_account_usage_bucket(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_account_usage_bucket_by(user_id, bucket_start, granularity, 1)
            .await
    }

    pub(crate) async fn increment_api_key_user_usage_bucket(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        api_key_id: &str,
        user_id: &str,
        bucket_start: i64,
        credits: i64,
        result_status: &str,
    ) -> Result<(), ProxyError> {
        if credits <= 0 {
            return Ok(());
        }
        let (success_credits, failure_credits) = if result_status == OUTCOME_SUCCESS {
            (credits, 0_i64)
        } else {
            (0_i64, credits)
        };
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO api_key_user_usage_buckets (
                api_key_id,
                user_id,
                bucket_start,
                bucket_secs,
                success_credits,
                failure_credits,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, user_id, bucket_start, bucket_secs)
            DO UPDATE SET
                success_credits = api_key_user_usage_buckets.success_credits + excluded.success_credits,
                failure_credits = api_key_user_usage_buckets.failure_credits + excluded.failure_credits,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(api_key_id)
        .bind(user_id)
        .bind(bucket_start)
        .bind(SECS_PER_DAY)
        .bind(success_credits)
        .bind(failure_credits)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn refresh_user_api_key_binding(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        api_key_id: &str,
        success_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO user_api_key_bindings (
                user_id,
                api_key_id,
                created_at,
                updated_at,
                last_success_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(user_id, api_key_id)
            DO UPDATE SET
                updated_at = CASE
                    WHEN excluded.last_success_at >= user_api_key_bindings.last_success_at THEN excluded.updated_at
                    ELSE user_api_key_bindings.updated_at
                END,
                last_success_at = MAX(user_api_key_bindings.last_success_at, excluded.last_success_at)
            "#,
        )
        .bind(user_id)
        .bind(api_key_id)
        .bind(success_at)
        .bind(success_at)
        .bind(success_at)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM user_api_key_bindings
            WHERE user_id = ?
              AND api_key_id IN (
                  SELECT api_key_id
                  FROM user_api_key_bindings
                  WHERE user_id = ?
                  ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
                  LIMIT -1 OFFSET ?
              )
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(USER_API_KEY_BINDING_RECENT_LIMIT)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub(crate) async fn sum_usage_buckets(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn sum_usage_buckets_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, SUM(count) as total FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, total) in rows {
            map.insert(token_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn earliest_usage_bucket_since_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, MIN(bucket_start) as earliest FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, bucket_start) in rows {
            map.insert(token_id, bucket_start);
        }
        Ok(map)
    }

    pub(crate) async fn earliest_account_usage_bucket_since_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, MIN(bucket_start) as earliest FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, bucket_start) in rows {
            map.insert(user_id, bucket_start);
        }
        Ok(map)
    }

    pub(crate) async fn fetch_monthly_counts(
        &self,
        token_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT token_id, month_start, month_count FROM auth_token_quota WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (token_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(token_id.clone(), 0);
                stale_ids.push(token_id);
            } else {
                map.insert(token_id, stored_count);
            }
        }

        for token_id in stale_ids {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(&token_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    pub(crate) async fn fetch_monthly_count(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(token_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_count(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_counts(
        &self,
        user_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT user_id, month_start, month_count FROM account_monthly_quota WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (user_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(user_id.clone(), 0);
                stale_ids.push(user_id);
            } else {
                map.insert(user_id, stored_count);
            }
        }

        for user_id in stale_ids {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(&user_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    pub(crate) async fn delete_old_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM token_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_old_account_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM account_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete per-token usage logs older than the given threshold.
    /// This is strictly time-based and deliberately independent of token status,
    /// so that audit trails are not coupled to enable/disable/delete operations.
    pub(crate) async fn delete_old_auth_token_logs(
        &self,
        threshold: i64,
    ) -> Result<i64, ProxyError> {
        let result = sqlx::query(
            r#"
            DELETE FROM auth_token_logs
            WHERE created_at < ?
            "#,
        )
        .bind(threshold)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    pub(crate) async fn delete_old_request_logs(&self, threshold: i64) -> Result<i64, ProxyError> {
        // Batched deletes reduce long-running write locks on large tables.
        const BATCH_SIZE: i64 = 5_000;
        let mut total_deleted = 0_i64;
        loop {
            let result = sqlx::query(
                r#"
                DELETE FROM request_logs
                WHERE id IN (
                    SELECT id
                    FROM request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            )
            .bind(threshold)
            .bind(BATCH_SIZE)
            .execute(&self.pool)
            .await?;
            let deleted = result.rows_affected() as i64;
            total_deleted += deleted;
            if deleted == 0 {
                break;
            }
        }
        Ok(total_deleted)
    }

    /// Aggregate per-token usage logs into hourly buckets in token_usage_stats.
    /// Returns (rows_affected, new_last_rollup_ts). When there are no new logs,
    /// rows_affected is 0 and new_last_rollup_ts is None.
    pub(crate) async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        async fn read_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
        ) -> Result<Option<i64>, ProxyError> {
            let value =
                sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
                    .bind(key)
                    .fetch_optional(&mut **tx)
                    .await?;
            Ok(value.and_then(|v| v.parse::<i64>().ok()))
        }

        async fn write_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
            value: i64,
        ) -> Result<(), ProxyError> {
            sqlx::query(
                r#"
                INSERT INTO meta (key, value)
                VALUES (?, ?)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(key)
            .bind(value.to_string())
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut tx = self.pool.begin().await?;

        // v2 cursor: strictly monotonic auth_token_logs.id to guarantee idempotent rollup.
        // Backward compatibility: on first v2 run, legacy timestamp is used only to filter
        // the migration batch, then the cursor permanently switches to id-based mode.
        let v2_cursor = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2).await?;
        let (last_log_id, migration_legacy_ts) = if let Some(id) = v2_cursor {
            (id, None)
        } else {
            (
                0,
                read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?,
            )
        };

        let (max_log_id, max_created_at): (Option<i64>, Option<i64>) =
            if let Some(legacy_ts) = migration_legacy_ts {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(CASE WHEN created_at >= ? THEN created_at END) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                    "#,
                )
                .bind(legacy_ts)
                .fetch_one(&mut *tx)
                .await?
            } else {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(created_at) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                      AND id > ?
                    "#,
                )
                .bind(last_log_id)
                .fetch_one(&mut *tx)
                .await?
            };

        let Some(max_log_id) = max_log_id else {
            if migration_legacy_ts.is_some() {
                // No billable logs yet: initialize v2 cursor to complete migration.
                write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, 0).await?;
            }
            tx.commit().await?;
            return Ok((0, None));
        };

        let bucket_secs = TOKEN_USAGE_STATS_BUCKET_SECS;

        let result = if let Some(legacy_ts) = migration_legacy_ts {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND created_at >= ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(legacy_ts)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND id > ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(last_log_id)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        };

        let affected = result.rows_affected() as i64;
        let mut new_last_rollup_ts = max_created_at;

        write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, max_log_id).await?;
        if let Some(ts) = max_created_at {
            // Keep legacy timestamp cursor monotonic for observability and downgrade compatibility.
            // This prevents accidental timestamp regression when newer log ids carry older created_at.
            let legacy_ts = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?;
            let clamped_ts = legacy_ts.map_or(ts, |old| old.max(ts));
            write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS, clamped_ts).await?;
            new_last_rollup_ts = Some(clamped_ts);
        }

        tx.commit().await?;
        Ok((affected, new_last_rollup_ts))
    }

    pub(crate) async fn increment_monthly_quota_by(
        &self,
        token_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_monthly_count(token_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                    ELSE auth_token_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                    ELSE auth_token_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(token_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;

        Ok(month_count)
    }

    pub(crate) async fn increment_monthly_quota(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_monthly_quota_by(token_id, current_month_start, 1)
            .await
    }

    pub(crate) async fn increment_account_monthly_quota_by(
        &self,
        user_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_account_monthly_count(user_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                    ELSE account_monthly_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                    ELSE account_monthly_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(user_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;
        Ok(month_count)
    }

    pub(crate) async fn increment_account_monthly_quota(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_account_monthly_quota_by(user_id, current_month_start, 1)
            .await
    }

    pub(crate) async fn upgrade_auth_tokens_schema(&self) -> Result<(), ProxyError> {
        // Future-proof placeholder for migrations
        // Ensure required columns exist if table is from older version
        // enabled
        if !self.auth_tokens_column_exists("enabled").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1")
                .execute(&self.pool)
                .await?;
        }

        if !self.auth_tokens_column_exists("note").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN note TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("total_requests").await? {
            sqlx::query(
                "ALTER TABLE auth_tokens ADD COLUMN total_requests INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self.auth_tokens_column_exists("created_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("last_used_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN last_used_at INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("group_name").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN group_name TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("deleted_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN deleted_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    pub(crate) async fn auth_tokens_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('auth_tokens') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    pub(crate) async fn table_column_exists(
        &self,
        table: &str,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
        )
        .bind(table)
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
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

    pub(crate) async fn upgrade_request_logs_schema(&self) -> Result<(), ProxyError> {
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

        self.ensure_request_logs_key_ids().await?;

        Ok(())
    }

    pub(crate) async fn ensure_request_logs_key_ids(&self) -> Result<(), ProxyError> {
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
            let mut tx = self.pool.begin().await?;

            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS request_logs_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    api_key_id TEXT NOT NULL,
                    auth_token_id TEXT,
                    method TEXT NOT NULL,
                    path TEXT NOT NULL,
                    query TEXT,
                    status_code INTEGER,
                    tavily_status_code INTEGER,
                    error_message TEXT,
                    result_status TEXT NOT NULL DEFAULT 'unknown',
                    failure_kind TEXT,
                    key_effect_code TEXT NOT NULL DEFAULT 'none',
                    key_effect_summary TEXT,
                    request_body BLOB,
                    response_body BLOB,
                    forwarded_headers TEXT,
                    dropped_headers TEXT,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
                )
                "#,
            )
            .execute(&mut *tx)
            .await?;

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
                    NULL AS failure_kind,
                    'none' AS key_effect_code,
                    NULL AS key_effect_summary,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
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
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                "#,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query("DROP TABLE request_logs")
                .execute(&mut *tx)
                .await?;
            sqlx::query("ALTER TABLE request_logs_new RENAME TO request_logs")
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
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

        Ok(())
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

    pub async fn fetch_key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        // `api_key_usage_buckets.bucket_start` is aligned to *server-local midnight* (stored as UTC ts).
        // Callers might pass `since` aligned to UTC midnight (e.g. from browser). Normalize so daily
        // bucket queries remain correct under non-UTC server timezones.
        let since_bucket_start = local_day_bucket_start_utc_ts(since);

        let totals_row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(total_requests), 0) AS total_requests,
              COALESCE(SUM(success_count), 0) AS success_count,
              COALESCE(SUM(error_count), 0) AS error_count,
              COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE api_key_id = ? AND bucket_secs = 86400 AND bucket_start >= ?
            "#,
        )
        .bind(key_id)
        .bind(since_bucket_start)
        .fetch_one(&self.pool)
        .await?;

        // Active/exhausted counts in this scope are not meaningful per single key; expose 1/0 for convenience
        // We will compute based on current key status
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        let key_last_used_at: Option<i64> =
            sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;
        let last_activity = key_last_used_at
            .and_then(normalize_timestamp)
            .filter(|ts| *ts >= since_bucket_start);

        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_quarantines
            WHERE key_id = ? AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        let (active_keys, exhausted_keys, quarantined_keys) = if quarantined {
            (0, 0, 1)
        } else {
            match status.as_deref() {
                Some(STATUS_EXHAUSTED) => (0, 1, 0),
                _ => (1, 0, 0),
            }
        };

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys,
            exhausted_keys,
            quarantined_keys,
            last_activity,
            total_quota_limit: 0,
            total_quota_remaining: 0,
        })
    }

    pub async fn fetch_key_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = if let Some(since_ts) = since {
            sqlx::query(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, failure_kind, key_effect_code, key_effect_summary,
                       request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ? AND created_at >= ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(since_ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, failure_kind, key_effect_code, key_effect_summary,
                       request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| -> Result<RequestLogRecord, sqlx::Error> {
                Ok(RequestLogRecord {
                    id: row.try_get("id")?,
                    key_id: row.try_get("api_key_id")?,
                    auth_token_id: row.try_get("auth_token_id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    status_code: row.try_get("status_code")?,
                    tavily_status_code: row.try_get("tavily_status_code")?,
                    error_message: row.try_get("error_message")?,
                    result_status: row.try_get("result_status")?,
                    failure_kind: row.try_get("failure_kind")?,
                    key_effect_code: row.try_get("key_effect_code")?,
                    key_effect_summary: row.try_get("key_effect_summary")?,
                    request_body: row
                        .try_get::<Option<Vec<u8>>, _>("request_body")?
                        .unwrap_or_default(),
                    response_body: row
                        .try_get::<Option<Vec<u8>>, _>("response_body")?
                        .unwrap_or_default(),
                    created_at: row.try_get("created_at")?,
                    forwarded_headers: parse_header_list(
                        row.try_get::<Option<String>, _>("forwarded_headers")?,
                    ),
                    dropped_headers: parse_header_list(
                        row.try_get::<Option<String>, _>("dropped_headers")?,
                    ),
                })
            })
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) async fn sync_keys(&self, keys: &[String]) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;

        let now = Utc::now().timestamp();

        for key in keys {
            // If key exists, undelete by clearing deleted_at
            if let Some((id, deleted_at)) = sqlx::query_as::<_, (String, Option<i64>)>(
                "SELECT id, deleted_at FROM api_keys WHERE api_key = ? LIMIT 1",
            )
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            {
                if deleted_at.is_some() {
                    sqlx::query("UPDATE api_keys SET deleted_at = NULL WHERE id = ?")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (id, api_key, status, created_at, status_changed_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(key)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        // Soft delete any keys not present in the provided set
        if keys.is_empty() {
            sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE deleted_at IS NULL")
                .bind(now)
                .execute(&mut *tx)
                .await?;
        } else {
            let mut builder = QueryBuilder::new("UPDATE api_keys SET deleted_at = ");
            builder.push_bind(now);
            builder.push(" WHERE deleted_at IS NULL AND api_key NOT IN (");
            {
                let mut separated = builder.separated(", ");
                for key in keys {
                    separated.push_bind(key);
                }
            }
            builder.push(")");
            builder.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn acquire_key(&self) -> Result<ApiKeyLease, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY last_used_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        Err(ProxyError::NoAvailableKeys)
    }

    pub(crate) async fn try_acquire_specific_key(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyLease>, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE id = ? AND status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(STATUS_ACTIVE)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(Some(ApiKeyLease {
                id,
                secret: api_key,
            }));
        }

        Ok(None)
    }

    pub(crate) async fn save_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO research_requests (
                request_id,
                key_id,
                token_id,
                expires_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(request_id) DO UPDATE SET
                key_id = excluded.key_id,
                token_id = excluded.token_id,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(request_id)
        .bind(key_id)
        .bind(token_id)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Opportunistic cleanup to keep this small over time.
        sqlx::query("DELETE FROM research_requests WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn ensure_api_key_maintenance_records_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_maintenance_records (
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
                FOREIGN KEY (request_log_id) REFERENCES request_logs(id),
                FOREIGN KEY (auth_token_log_id) REFERENCES auth_token_logs(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_key_created
               ON api_key_maintenance_records(key_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_request_log
               ON api_key_maintenance_records(request_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_auth_token_log
               ON api_key_maintenance_records(auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn get_research_request_affinity(
        &self,
        request_id: &str,
        now: i64,
    ) -> Result<Option<(String, String)>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT key_id, token_id
            FROM research_requests
            WHERE request_id = ? AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(request_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        if row.is_none() {
            sqlx::query(
                r#"
                DELETE FROM research_requests
                WHERE request_id = ? AND expires_at <= ?
                "#,
            )
            .bind(request_id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(row)
    }

    // ----- Access token helpers -----

    pub(crate) fn compose_full_token(id: &str, secret: &str) -> String {
        format!("th-{}-{}", id, secret)
    }

    pub(crate) async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        // Expect format th-<id>-<secret>
        let Some(rest) = token.strip_prefix("th-") else {
            return Ok(false);
        };
        let parts: Vec<&str> = rest.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Ok(false);
        }
        let id = parts[0];
        let secret = parts[1];
        // Keep short, human-friendly id; strengthen total entropy by lengthening secret.
        // Backward-compatible: accept legacy 12-char secrets and new longer secrets.
        const LEGACY_SECRET_LEN: usize = 12;
        const NEW_SECRET_LEN: usize = 24; // chosen to significantly raise entropy
        let secret_len_ok = secret.len() == LEGACY_SECRET_LEN || secret.len() == NEW_SECRET_LEN;
        if id.len() != 4 || !secret_len_ok {
            return Ok(false);
        }

        // Validation should be a pure check. Do NOT mutate usage counters here,
        // otherwise the token's total_requests will be double-counted (once here,
        // and once when we actually record the attempt). Only return whether the
        // token exists and is enabled.
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COUNT(1) as cnt, enabled FROM auth_tokens WHERE id = ? AND secret = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .bind(secret)
        .fetch_optional(&self.pool)
        .await?;

        Ok(matches!(row, Some((cnt, enabled)) if cnt > 0 && enabled == 1))
    }

    pub(crate) async fn create_access_token(
        &self,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        loop {
            let id = random_string(ALPHABET, 4);
            // Increase secret length to strengthen token entropy while keeping id short.
            let secret = random_string(ALPHABET, 24);
            let res = sqlx::query(
                r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                   VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)"#,
            )
            .bind(&id)
            .bind(&secret)
            .bind(note.unwrap_or(""))
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    let token_str = Self::compose_full_token(&id, &secret);
                    return Ok(AuthTokenSecret {
                        id,
                        token: token_str,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    // Retry on rare id collision
                    continue;
                }
                Err(e) => return Err(ProxyError::Database(e)),
            }
        }
    }

    /// Batch-create access tokens with required group name. Optional note applied to each row.
    pub(crate) async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut tx = self.pool.begin().await?;
        let mut out: Vec<AuthTokenSecret> = Vec::with_capacity(count);
        for _ in 0..count {
            loop {
                let id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);
                let res = sqlx::query(
                    r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES (?, ?, 1, ?, ?, 0, ?, NULL, NULL)"#,
                )
                .bind(&id)
                .bind(&secret)
                .bind(note.unwrap_or(""))
                .bind(group)
                .bind(Utc::now().timestamp())
                .execute(&mut *tx)
                .await;

                match res {
                    Ok(_) => {
                        let token = Self::compose_full_token(&id, &secret);
                        out.push(AuthTokenSecret { id, token });
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        continue;
                    }
                    Err(e) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(e));
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(out)
    }
    // Generate random string of given length from provided alphabet
    // Alphabet is a byte slice of ASCII alphanumerics
    // Using ThreadRng for simplicity

    pub(crate) async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    /// Paginated list of access tokens ordered by created_at desc. Returns (items, total)
    pub(crate) async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;

        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM auth_tokens WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC
               LIMIT ? OFFSET ?"#,
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect();
        Ok((items, total))
    }

    pub(crate) async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE auth_tokens SET enabled = 0, deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_access_token_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET enabled = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(if enabled { 1 } else { 0 })
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn update_access_token_note(
        &self,
        id: &str,
        note: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET note = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(note)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT secret FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &secret),
        }))
    }

    /// Update the secret for an existing token id and return the new full token string.
    pub(crate) async fn rotate_access_token_secret(
        &self,
        id: &str,
    ) -> Result<AuthTokenSecret, ProxyError> {
        // Ensure token exists first to provide a clearer error on missing id
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        }

        // Generate a new secret with the current strong length
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let new_secret = random_string(ALPHABET, 24);

        sqlx::query("UPDATE auth_tokens SET secret = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(&new_secret)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &new_secret),
        })
    }

    pub(crate) async fn list_user_tokens(
        &self,
        user_id: &str,
    ) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT t.id, t.enabled, t.note, t.group_name, t.total_requests, t.created_at, t.last_used_at
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND t.deleted_at IS NULL
               ORDER BY b.updated_at DESC, b.created_at DESC, t.id DESC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used_at)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    pub(crate) async fn is_user_token_bound(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT 1
               FROM user_token_bindings
               WHERE user_id = ? AND token_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    pub(crate) async fn list_user_bindings_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, String>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, user_id FROM user_token_bindings WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");
        let rows = builder
            .build_query_as::<(String, String)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, user_id) in rows {
            map.insert(token_id, user_id);
        }
        Ok(map)
    }

    pub(crate) async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT t.secret
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND b.token_id = ? AND t.deleted_at IS NULL AND t.enabled = 1
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    #[allow(dead_code)]
    pub(crate) async fn find_user_id_by_token(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.token_binding_cache.read().await;
            cache.get(token_id).cloned()
        } && cached.expires_at > now
        {
            return Ok(cached.user_id);
        }

        self.find_user_id_by_token_fresh(token_id).await
    }

    pub(crate) async fn find_user_id_by_token_fresh(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let user_id = row.map(|(id,)| id);
        self.cache_token_binding(token_id, user_id.as_deref()).await;
        Ok(user_id)
    }

    pub(crate) async fn cache_token_binding(&self, token_id: &str, user_id: Option<&str>) {
        let mut cache = self.token_binding_cache.write().await;
        cache.insert(
            token_id.to_string(),
            TokenBindingCacheEntry {
                user_id: user_id.map(str::to_string),
                expires_at: Instant::now() + Duration::from_secs(TOKEN_BINDING_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - TOKEN_BINDING_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    pub(crate) async fn cached_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Option<AccountQuotaResolution> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.account_quota_resolution_cache.read().await;
            cache.get(user_id).cloned()
        } && cached.expires_at > now
        {
            return Some(cached.resolution);
        }
        None
    }

    pub(crate) async fn cache_account_quota_resolution(
        &self,
        user_id: &str,
        resolution: &AccountQuotaResolution,
    ) {
        let mut cache = self.account_quota_resolution_cache.write().await;
        cache.insert(
            user_id.to_string(),
            AccountQuotaResolutionCacheEntry {
                resolution: resolution.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(ACCOUNT_QUOTA_RESOLUTION_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    pub(crate) async fn invalidate_account_quota_resolution(&self, user_id: &str) {
        self.account_quota_resolution_cache
            .write()
            .await
            .remove(user_id);
    }

    pub(crate) async fn invalidate_account_quota_resolutions<I, S>(&self, user_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cache = self.account_quota_resolution_cache.write().await;
        for user_id in user_ids {
            cache.remove(user_id.as_ref());
        }
    }

    pub(crate) async fn invalidate_all_account_quota_resolutions(&self) {
        self.account_quota_resolution_cache.write().await.clear();
    }

    pub(crate) async fn list_user_ids_for_tag(
        &self,
        tag_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT user_id FROM user_tag_bindings WHERE tag_id = ?",
        )
        .bind(tag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let search = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tag_id = tag_id.map(str::trim).filter(|value| !value.is_empty());

        let total = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )"#,
                )
                .bind(tag_id)
                .fetch_one(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
                    .fetch_one(&self.pool)
                    .await?
            }
        };

        let rows = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let items = rows
            .into_iter()
            .map(
                |(user_id, display_name, username, active, last_login_at, token_count)| {
                    AdminUserIdentity {
                        user_id,
                        display_name,
                        username,
                        active: active == 1,
                        last_login_at,
                        token_count,
                    }
                },
            )
            .collect();
        Ok((items, total))
    }

    pub(crate) async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            ),
        >(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id = ?
               GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(user_id, display_name, username, active, last_login_at, token_count)| {
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                }
            },
        ))
    }

    pub(crate) async fn get_admin_user_identities(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at");

        let rows = builder
            .build_query_as::<(
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;

        let mut items = HashMap::with_capacity(rows.len());
        for (user_id, display_name, username, active, last_login_at, token_count) in rows {
            items.insert(
                user_id.clone(),
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                },
            );
        }
        Ok(items)
    }

    pub(crate) async fn list_recent_api_key_bindings_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT api_key_id
            FROM user_api_key_bindings
            WHERE user_id = ?
            ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
            LIMIT ?
            "#,
        )
        .bind(user_id)
        .bind(USER_API_KEY_BINDING_RECENT_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_sticky_users_page(
        &self,
        key_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedApiKeyStickyUsers, ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let now = Local::now();
        let today_start = start_of_local_day_utc_ts(now);
        let yesterday_start = previous_local_day_start_utc_ts(now);
        let month_start = start_of_local_month_utc_ts(now);
        let oldest_daily_date = now
            .date_naive()
            .checked_sub_days(chrono::Days::new(6))
            .unwrap_or_else(|| now.date_naive());
        let oldest_daily_start = local_date_start_utc_ts(oldest_daily_date, now);
        let usage_since = month_start.min(oldest_daily_start);

        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_api_key_bindings WHERE api_key_id = ?",
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
            ),
        >(
            r#"
            SELECT
                b.user_id,
                b.last_success_at,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0) AS yesterday_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0) AS yesterday_failure_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0) AS today_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0) AS today_failure_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start >= ? THEN u.success_credits ELSE 0 END), 0) AS month_success_credits,
                COALESCE(SUM(CASE WHEN u.bucket_start >= ? THEN u.failure_credits ELSE 0 END), 0) AS month_failure_credits
            FROM user_api_key_bindings b
            LEFT JOIN api_key_user_usage_buckets u
              ON u.api_key_id = b.api_key_id
             AND u.user_id = b.user_id
             AND u.bucket_secs = ?
             AND u.bucket_start >= ?
            WHERE b.api_key_id = ?
            GROUP BY b.user_id, b.last_success_at
            ORDER BY
                (COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.success_credits ELSE 0 END), 0)
                + COALESCE(SUM(CASE WHEN u.bucket_start = ? THEN u.failure_credits ELSE 0 END), 0)) DESC,
                b.last_success_at DESC,
                b.user_id ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(yesterday_start)
        .bind(yesterday_start)
        .bind(today_start)
        .bind(today_start)
        .bind(month_start)
        .bind(month_start)
        .bind(SECS_PER_DAY)
        .bind(usage_since)
        .bind(key_id)
        .bind(today_start)
        .bind(today_start)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let user_ids = rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>();
        let identities = self.get_admin_user_identities(&user_ids).await?;

        let bucket_starts = (0..7_i64)
            .map(|index| oldest_daily_start + index * SECS_PER_DAY)
            .collect::<Vec<_>>();
        let daily_rows = if user_ids.is_empty() {
            Vec::new()
        } else {
            let mut builder = QueryBuilder::new(
                "SELECT user_id, bucket_start, success_credits, failure_credits \
                 FROM api_key_user_usage_buckets \
                 WHERE api_key_id = ",
            );
            builder.push_bind(key_id);
            builder.push(" AND bucket_secs = ");
            builder.push_bind(SECS_PER_DAY);
            builder.push(" AND bucket_start >= ");
            builder.push_bind(oldest_daily_start);
            builder.push(" AND user_id IN (");
            {
                let mut separated = builder.separated(", ");
                for user_id in &user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(") ORDER BY user_id ASC, bucket_start ASC");
            builder
                .build_query_as::<(String, i64, i64, i64)>()
                .fetch_all(&self.pool)
                .await?
        };

        let mut daily_map = HashMap::<String, HashMap<i64, StickyCreditsWindow>>::new();
        for (user_id, bucket_start, success_credits, failure_credits) in daily_rows {
            daily_map.entry(user_id).or_default().insert(
                bucket_start,
                StickyCreditsWindow {
                    success_credits,
                    failure_credits,
                },
            );
        }

        let mut items = Vec::with_capacity(rows.len());
        for (
            user_id,
            last_success_at,
            yesterday_success_credits,
            yesterday_failure_credits,
            today_success_credits,
            today_failure_credits,
            month_success_credits,
            month_failure_credits,
        ) in rows
        {
            let Some(user) = identities.get(&user_id).cloned() else {
                continue;
            };
            let user_daily = daily_map.get(&user_id);
            let daily_buckets = bucket_starts
                .iter()
                .map(|bucket_start| {
                    let bucket = user_daily
                        .and_then(|items| items.get(bucket_start))
                        .cloned()
                        .unwrap_or_default();
                    ApiKeyUserUsageBucket {
                        bucket_start: *bucket_start,
                        bucket_end: bucket_start.saturating_add(SECS_PER_DAY),
                        success_credits: bucket.success_credits,
                        failure_credits: bucket.failure_credits,
                    }
                })
                .collect::<Vec<_>>();

            items.push(ApiKeyStickyUser {
                user,
                last_success_at,
                yesterday: StickyCreditsWindow {
                    success_credits: yesterday_success_credits,
                    failure_credits: yesterday_failure_credits,
                },
                today: StickyCreditsWindow {
                    success_credits: today_success_credits,
                    failure_credits: today_failure_credits,
                },
                month: StickyCreditsWindow {
                    success_credits: month_success_credits,
                    failure_credits: month_failure_credits,
                },
                daily_buckets,
            });
        }

        Ok(PaginatedApiKeyStickyUsers {
            items,
            total,
            page,
            per_page,
        })
    }

    pub(crate) async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Ok(false);
        }

        let defaults = AccountQuotaLimits::legacy_defaults();
        let current = self.ensure_account_quota_limits(user_id).await?;
        let requested = AccountQuotaLimits {
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults: false,
        };
        let inherits_defaults = if current.inherits_defaults && requested.same_limits_as(&defaults)
        {
            1
        } else {
            0
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    hourly_any_limit = excluded.hourly_any_limit,
                    hourly_limit = excluded.hourly_limit,
                    daily_limit = excluded.daily_limit,
                    monthly_limit = excluded.monthly_limit,
                    inherits_defaults = excluded.inherits_defaults,
                    updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(inherits_defaults)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    pub(crate) async fn backfill_account_quota_inherits_defaults_v1(
        &self,
    ) -> Result<(), ProxyError> {
        let defaults = AccountQuotaLimits::legacy_defaults();
        // Legacy rows do not record whether they were following defaults or manually customized.
        // Only rows that already match the current env tuple are safe to keep on the default-track;
        // every other tuple is conservatively treated as a custom baseline so upgrades never clobber
        // admin-set quotas.
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET inherits_defaults = CASE
                   WHEN hourly_any_limit = ?
                    AND hourly_limit = ?
                    AND daily_limit = ?
                    AND monthly_limit = ?
                   THEN 1
                   ELSE 0
               END"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn sync_account_quota_limits_with_defaults(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let defaults = AccountQuotaLimits::legacy_defaults();
        sqlx::query(
            r#"UPDATE account_quota_limits
               SET hourly_any_limit = ?,
                   hourly_limit = ?,
                   daily_limit = ?,
                   monthly_limit = ?,
                   updated_at = ?
               WHERE inherits_defaults = 1"#,
        )
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_all_account_quota_resolutions().await;
        Ok(())
    }

    pub(crate) async fn account_quota_zero_base_cutover_at(&self) -> Result<i64, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1)
            .await?
            .unwrap_or(i64::MAX))
    }

    pub(crate) async fn default_account_quota_limits_for_user(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        let user_created_at =
            sqlx::query_scalar::<_, i64>("SELECT created_at FROM users WHERE id = ? LIMIT 1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        Ok(default_account_quota_limits_for_created_at(
            user_created_at,
            cutover_at,
        ))
    }

    pub(crate) async fn default_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        let mut builder = QueryBuilder::new("SELECT id, created_at FROM users WHERE id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(user_id, created_at)| {
                (
                    user_id,
                    default_account_quota_limits_for_created_at(created_at, cutover_at),
                )
            })
            .collect())
    }

    pub(crate) async fn ensure_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaLimits, ProxyError> {
        if let Some(existing) = self.fetch_account_quota_limits(user_id).await? {
            return Ok(existing);
        }

        let now = Utc::now().timestamp();
        let defaults = self.default_account_quota_limits_for_user(user_id).await?;
        sqlx::query(
            r#"INSERT INTO account_quota_limits (
                    user_id,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id) DO NOTHING"#,
        )
        .bind(user_id)
        .bind(defaults.hourly_any_limit)
        .bind(defaults.hourly_limit)
        .bind(defaults.daily_limit)
        .bind(defaults.monthly_limit)
        .bind(if defaults.inherits_defaults { 1 } else { 0 })
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.fetch_account_quota_limits(user_id)
            .await?
            .ok_or_else(|| {
                ProxyError::Other(format!(
                    "account quota limits missing after ensure for user {user_id}"
                ))
            })
    }

    pub(crate) async fn ensure_account_quota_limits_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<(), ProxyError> {
        if user_ids.is_empty() {
            return Ok(());
        }

        let existing = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let missing_user_ids: Vec<String> = user_ids
            .iter()
            .filter(|user_id| !existing.contains_key(*user_id))
            .cloned()
            .collect();
        if missing_user_ids.is_empty() {
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let defaults_by_user = self
            .default_account_quota_limits_for_users(&missing_user_ids)
            .await?;

        let mut builder = QueryBuilder::new(
            "INSERT INTO account_quota_limits (user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults, created_at, updated_at) ",
        );
        builder.push_values(&missing_user_ids, |mut b, user_id| {
            let defaults = defaults_by_user
                .get(user_id)
                .cloned()
                .unwrap_or_else(AccountQuotaLimits::zero_base);
            b.push_bind(user_id)
                .push_bind(defaults.hourly_any_limit)
                .push_bind(defaults.hourly_limit)
                .push_bind(defaults.daily_limit)
                .push_bind(defaults.monthly_limit)
                .push_bind(if defaults.inherits_defaults { 1 } else { 0 })
                .push_bind(now)
                .push_bind(now);
        });
        builder.push(" ON CONFLICT(user_id) DO NOTHING");
        builder.build().execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn fetch_account_quota_limits(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountQuotaLimits>, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit,
                      COALESCE(inherits_defaults, 1)
               FROM account_quota_limits
               WHERE user_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults)| {
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                )
            },
        ))
    }

    pub(crate) async fn fetch_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, hourly_any_limit, hourly_limit, daily_limit, monthly_limit, COALESCE(inherits_defaults, 1) FROM account_quota_limits WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (
            user_id,
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults,
        ) in rows
        {
            map.insert(
                user_id,
                account_quota_limits_from_row(
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                    inherits_defaults,
                ),
            );
        }
        Ok(map)
    }

    pub(crate) async fn seed_linuxdo_system_tags(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        for level in 0..=4 {
            let system_key = linuxdo_system_key_for_level(level);
            let display_name = format!("L{level}");
            sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(system_key) DO UPDATE SET
                        name = excluded.name,
                        display_name = excluded.display_name,
                        icon = excluded.icon,
                        updated_at = excluded.updated_at"#,
            )
            .bind(&system_key)
            .bind(&system_key)
            .bind(display_name)
            .bind(USER_TAG_ICON_LINUXDO)
            .bind(&system_key)
            .bind(USER_TAG_EFFECT_QUOTA_DELTA)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn infer_linuxdo_system_tag_default_deltas_from_rows(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"SELECT effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
               FROM user_tags
               WHERE system_key LIKE 'linuxdo_l%'
               ORDER BY system_key"#,
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.len() != 5 {
            return Ok(None);
        }
        let mut expected: Option<(i64, i64, i64, i64)> = None;
        for (effect_kind, hourly_any_delta, hourly_delta, daily_delta, monthly_delta) in rows {
            if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA {
                return Ok(None);
            }
            let current = (hourly_any_delta, hourly_delta, daily_delta, monthly_delta);
            match expected {
                Some(previous) if previous != current => return Ok(None),
                Some(_) => {}
                None => expected = Some(current),
            }
        }
        Ok(expected)
    }

    pub(crate) async fn get_linuxdo_system_tag_default_deltas_meta(
        &self,
    ) -> Result<Option<(i64, i64, i64, i64)>, ProxyError> {
        let Some(raw) = self
            .get_meta_string(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
            .await?
        else {
            return Ok(None);
        };
        Ok(parse_linuxdo_system_tag_default_deltas(&raw))
    }

    pub(crate) async fn set_linuxdo_system_tag_default_deltas_meta(
        &self,
        value: (i64, i64, i64, i64),
    ) -> Result<(), ProxyError> {
        self.set_meta_string(
            META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1,
            &format_linuxdo_system_tag_default_deltas(value),
        )
        .await
    }

    pub(crate) async fn allow_registration(&self) -> Result<bool, ProxyError> {
        Ok(self
            .get_meta_i64(META_KEY_ALLOW_REGISTRATION_V1)
            .await?
            .unwrap_or(1)
            != 0)
    }

    pub(crate) async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.set_meta_i64(META_KEY_ALLOW_REGISTRATION_V1, if allow { 1 } else { 0 })
            .await?;
        Ok(allow)
    }

    pub(crate) async fn sync_linuxdo_system_tag_default_deltas_with_env(
        &self,
    ) -> Result<(), ProxyError> {
        let current = linuxdo_system_tag_default_deltas();
        let previous = match self.get_linuxdo_system_tag_default_deltas_meta().await? {
            Some(value) => value,
            None => self
                .infer_linuxdo_system_tag_default_deltas_from_rows()
                .await?
                .unwrap_or(current),
        };
        if previous == current {
            self.set_linuxdo_system_tag_default_deltas_meta(current)
                .await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = ?
                 AND hourly_delta = ?
                 AND daily_delta = ?
                 AND monthly_delta = ?"#,
        )
        .bind(current.0)
        .bind(current.1)
        .bind(current.2)
        .bind(current.3)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .bind(previous.0)
        .bind(previous.1)
        .bind(previous.2)
        .bind(previous.3)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
        }
        self.set_linuxdo_system_tag_default_deltas_meta(current)
            .await?;
        Ok(())
    }

    pub(crate) async fn backfill_linuxdo_system_tag_default_deltas_v1(
        &self,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let (hourly_any_delta, hourly_delta, daily_delta, monthly_delta) =
            linuxdo_system_tag_default_deltas();
        let updated = sqlx::query(
            r#"UPDATE user_tags
               SET hourly_any_delta = ?,
                   hourly_delta = ?,
                   daily_delta = ?,
                   monthly_delta = ?,
                   updated_at = ?
               WHERE system_key LIKE 'linuxdo_l%'
                 AND effect_kind = ?
                 AND hourly_any_delta = 0
                 AND hourly_delta = 0
                 AND daily_delta = 0
                 AND monthly_delta = 0"#,
        )
        .bind(hourly_any_delta)
        .bind(hourly_delta)
        .bind(daily_delta)
        .bind(monthly_delta)
        .bind(now)
        .bind(USER_TAG_EFFECT_QUOTA_DELTA)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() > 0 {
            self.invalidate_all_account_quota_resolutions().await;
        }
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;
        self.sync_linuxdo_system_tag_binding_in_tx(&mut tx, user_id, trust_level)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_in_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        trust_level: Option<i64>,
    ) -> Result<(), ProxyError> {
        let Some(level) = normalize_linuxdo_trust_level(trust_level) else {
            return Ok(());
        };
        let desired_key = linuxdo_system_key_for_level(level);
        let Some((tag_id,)) =
            sqlx::query_as::<_, (String,)>("SELECT id FROM user_tags WHERE system_key = ? LIMIT 1")
                .bind(&desired_key)
                .fetch_optional(&mut **tx)
                .await?
        else {
            eprintln!(
                "linuxdo system tag sync skipped for user {} trust_level {:?}: missing system tag for LinuxDo trust level {}",
                user_id, trust_level, level
            );
            return Ok(());
        };

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"DELETE FROM user_tag_bindings
               WHERE user_id = ?
                 AND tag_id IN (
                     SELECT id FROM user_tags WHERE system_key LIKE 'linuxdo_l%'
                 )
                 AND tag_id <> ?"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .bind(USER_TAG_SOURCE_SYSTEM_LINUXDO)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_best_effort(
        &self,
        user_id: &str,
        trust_level: Option<i64>,
    ) {
        if let Err(err) = self
            .sync_linuxdo_system_tag_binding(user_id, trust_level)
            .await
        {
            eprintln!(
                "linuxdo system tag sync error for user {} trust_level {:?}: {}",
                user_id, trust_level, err
            );
        }
    }

    pub(crate) async fn backfill_linuxdo_user_tag_bindings(&self) -> Result<(), ProxyError> {
        let rows = sqlx::query_as::<_, (String, Option<i64>)>(
            r#"SELECT user_id, trust_level
               FROM oauth_accounts
               WHERE provider = 'linuxdo'"#,
        )
        .fetch_all(&self.pool)
        .await?;
        for (user_id, trust_level) in rows {
            self.sync_linuxdo_system_tag_binding(&user_id, trust_level)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn fetch_user_tag_by_id(
        &self,
        tag_id: &str,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
            ),
        >(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               WHERE t.id = ?
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               LIMIT 1"#,
        )
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            )| UserTagRecord {
                id,
                name,
                display_name,
                icon,
                system_key,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
                user_count,
            },
        ))
    }

    pub(crate) async fn list_user_tags(&self) -> Result<Vec<UserTagRecord>, ProxyError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, String, i64, i64, i64, i64, i64)>(
            r#"SELECT
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta,
                 COALESCE(COUNT(b.user_id), 0) AS user_count
               FROM user_tags t
               LEFT JOIN user_tag_bindings b ON b.tag_id = t.id
               GROUP BY t.id, t.name, t.display_name, t.icon, t.system_key,
                        t.effect_kind, t.hourly_any_delta, t.hourly_delta,
                        t.daily_delta, t.monthly_delta
               ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                )| UserTagRecord {
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    hourly_any_delta,
                    hourly_delta,
                    daily_delta,
                    monthly_delta,
                    user_count,
                },
            )
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<UserTagRecord, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        for _ in 0..8 {
            let id = random_string(ALPHABET, 8);
            let inserted = sqlx::query(
                r#"INSERT INTO user_tags (
                        id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(_) => {
                    return self
                        .fetch_user_tag_by_id(&id)
                        .await?
                        .ok_or_else(|| ProxyError::Other("created user tag missing".to_string()));
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    if db_err.message().contains("user_tags.name") {
                        return Err(ProxyError::Other(
                            "user tag name already exists".to_string(),
                        ));
                    }
                    continue;
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        Err(ProxyError::Other(
            "failed to allocate unique user tag id".to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_user_tag(
        &self,
        tag_id: &str,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<Option<UserTagRecord>, ProxyError> {
        if effect_kind != USER_TAG_EFFECT_QUOTA_DELTA && effect_kind != USER_TAG_EFFECT_BLOCK_ALL {
            return Err(ProxyError::Other(
                "invalid user tag effect kind".to_string(),
            ));
        }
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(None);
        };
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let now = Utc::now().timestamp();
        if existing.is_system() {
            if existing.name != name
                || existing.display_name != display_name
                || existing.icon.as_deref() != icon
            {
                return Err(ProxyError::Other(
                    "system user tags only allow effect updates".to_string(),
                ));
            }
            sqlx::query(
                r#"UPDATE user_tags
                   SET effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        } else {
            let updated = sqlx::query(
                r#"UPDATE user_tags
                   SET name = ?,
                       display_name = ?,
                       icon = ?,
                       effect_kind = ?,
                       hourly_any_delta = ?,
                       hourly_delta = ?,
                       daily_delta = ?,
                       monthly_delta = ?,
                       updated_at = ?
                   WHERE id = ?"#,
            )
            .bind(name)
            .bind(display_name)
            .bind(icon)
            .bind(effect_kind)
            .bind(hourly_any_delta)
            .bind(hourly_delta)
            .bind(daily_delta)
            .bind(monthly_delta)
            .bind(now)
            .bind(tag_id)
            .execute(&self.pool)
            .await;
            match updated {
                Ok(_) => {}
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    return Err(ProxyError::Other(
                        "user tag name already exists".to_string(),
                    ));
                }
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        self.fetch_user_tag_by_id(tag_id).await
    }

    pub(crate) async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        let Some(existing) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if existing.is_system() {
            return Err(ProxyError::Other(
                "system user tags cannot be deleted".to_string(),
            ));
        }
        let affected_user_ids = self.list_user_ids_for_tag(tag_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_tag_bindings WHERE tag_id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM user_tags WHERE id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.invalidate_account_quota_resolutions(&affected_user_ids)
            .await;
        Ok(true)
    }

    pub(crate) async fn bind_user_tag_to_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        let user_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        if user_exists == 0 {
            return Ok(false);
        }
        let Some(tag) = self.fetch_user_tag_by_id(tag_id).await? else {
            return Ok(false);
        };
        if tag.is_system() {
            return Err(ProxyError::Other(
                "system user tags are managed by the server".to_string(),
            ));
        }
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .bind(USER_TAG_SOURCE_MANUAL)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    pub(crate) async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        let binding = sqlx::query_as::<_, (String, Option<String>)>(
            r#"SELECT b.source, t.system_key
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ? AND b.tag_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((source, system_key)) = binding else {
            return Ok(false);
        };
        if source != USER_TAG_SOURCE_MANUAL || system_key.is_some() {
            return Err(ProxyError::Other(
                "system-managed user tag bindings are read-only".to_string(),
            ));
        }
        sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ? AND tag_id = ?")
            .bind(user_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(true)
    }

    pub(crate) async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<UserTagBindingRecord>>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            r#"SELECT
                 b.user_id,
                 b.source,
                 t.id,
                 t.name,
                 t.display_name,
                 t.icon,
                 t.system_key,
                 t.effect_kind,
                 t.hourly_any_delta,
                 t.hourly_delta,
                 t.daily_delta,
                 t.monthly_delta
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") ORDER BY (t.system_key IS NULL) ASC, COALESCE(t.system_key, t.name) ASC, t.display_name ASC");

        let rows = builder
            .build_query_as::<(
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                i64,
                i64,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;
        let mut map: HashMap<String, Vec<UserTagBindingRecord>> = HashMap::new();
        for (
            user_id,
            source,
            tag_id,
            name,
            display_name,
            icon,
            system_key,
            effect_kind,
            hourly_any_delta,
            hourly_delta,
            daily_delta,
            monthly_delta,
        ) in rows
        {
            map.entry(user_id.clone())
                .or_default()
                .push(UserTagBindingRecord {
                    source,
                    tag: UserTagRecord {
                        id: tag_id,
                        name,
                        display_name,
                        icon,
                        system_key,
                        effect_kind,
                        hourly_any_delta,
                        hourly_delta,
                        daily_delta,
                        monthly_delta,
                        user_count: 0,
                    },
                });
        }
        Ok(map)
    }

    pub(crate) async fn list_user_tag_bindings_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserTagBindingRecord>, ProxyError> {
        Ok(self
            .list_user_tag_bindings_for_users(&[user_id.to_string()])
            .await?
            .remove(user_id)
            .unwrap_or_default())
    }

    pub(crate) async fn resolve_account_quota_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountQuotaLimits>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.ensure_account_quota_limits_for_users(user_ids).await?;
        let base_limits = self.fetch_account_quota_limits_bulk(user_ids).await?;
        let tag_bindings = self.list_user_tag_bindings_for_users(user_ids).await?;
        let defaults = AccountQuotaLimits::zero_base();
        let mut map = HashMap::new();
        for user_id in user_ids {
            let base = base_limits
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| defaults.clone());
            let tags = tag_bindings.get(user_id).cloned().unwrap_or_default();
            map.insert(
                user_id.clone(),
                build_account_quota_resolution(base, tags).effective,
            );
        }
        Ok(map)
    }

    pub(crate) async fn resolve_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Result<AccountQuotaResolution, ProxyError> {
        if let Some(cached) = self.cached_account_quota_resolution(user_id).await {
            return Ok(cached);
        }

        let base = self.ensure_account_quota_limits(user_id).await?;
        let tags = self.list_user_tag_bindings_for_user(user_id).await?;
        let resolution = build_account_quota_resolution(base, tags);
        self.cache_account_quota_resolution(user_id, &resolution)
            .await;
        Ok(resolution)
    }

    pub(crate) async fn fetch_user_success_failure(
        &self,
        user_id: &str,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let day_start = start_of_day(now).timestamp();
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN l.result_status = ? AND l.created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs l
            JOIN user_token_bindings b ON b.token_id = l.token_id
            WHERE b.user_id = ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_start)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(OUTCOME_ERROR)
        .bind(day_start)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }

    pub(crate) async fn fetch_user_last_activity(
        &self,
        user_id: &str,
    ) -> Result<Option<i64>, ProxyError> {
        let row = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT MAX(l.created_at)
               FROM auth_token_logs l
               JOIN user_token_bindings b ON b.token_id = l.token_id
               WHERE b.user_id = ?"#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub(crate) async fn insert_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let state = random_string(ALPHABET, 48);
            let res = sqlx::query(
                r#"INSERT INTO oauth_login_states
                   (state, provider, redirect_to, binding_hash, bind_token_id, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&state)
            .bind(provider)
            .bind(redirect_to.map(str::trim).filter(|value| !value.is_empty()))
            .bind(
                binding_hash
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
            .bind(bind_token_id.map(str::trim).filter(|value| !value.is_empty()))
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => return Ok(state),
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM oauth_login_states WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = if let Some(hash) = binding_hash
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash = ?
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                r#"SELECT redirect_to, bind_token_id
                   FROM oauth_login_states
                   WHERE state = ?
                     AND provider = ?
                     AND consumed_at IS NULL
                     AND expires_at >= ?
                     AND binding_hash IS NULL
                   LIMIT 1"#,
            )
            .bind(state)
            .bind(provider)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((redirect_to, bind_token_id)) = row else {
            tx.rollback().await.ok();
            return Ok(None);
        };

        let updated = sqlx::query(
            r#"UPDATE oauth_login_states
               SET consumed_at = ?
               WHERE state = ? AND provider = ? AND consumed_at IS NULL"#,
        )
        .bind(now)
        .bind(state)
        .bind(provider)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(None);
        }

        tx.commit().await?;
        Ok(Some(OAuthLoginStatePayload {
            redirect_to,
            bind_token_id,
        }))
    }

    pub(crate) async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        let display_name = profile
            .name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| {
                profile
                    .username
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        let username = profile
            .username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let avatar = profile
            .avatar_template
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let active = if profile.active { 1 } else { 0 };
        let now = Utc::now().timestamp();

        for _ in 0..4 {
            let mut tx = self.pool.begin().await?;

            let existing = sqlx::query_as::<_, (String,)>(
                r#"SELECT user_id
                   FROM oauth_accounts
                   WHERE provider = ? AND provider_user_id = ?
                   LIMIT 1"#,
            )
            .bind(&profile.provider)
            .bind(&profile.provider_user_id)
            .fetch_optional(&mut *tx)
            .await?;

            let user_id = if let Some((user_id,)) = existing {
                user_id
            } else {
                const ALPHABET: &[u8] =
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
                let mut created_user_id = None;
                for _ in 0..8 {
                    let candidate = random_string(ALPHABET, 12);
                    let inserted = sqlx::query(
                        r#"INSERT INTO users
                           (id, display_name, username, avatar_template, active, created_at, updated_at, last_login_at)
                           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(&candidate)
                    .bind(display_name.clone())
                    .bind(username.clone())
                    .bind(avatar.clone())
                    .bind(active)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await;

                    match inserted {
                        Ok(_) => {
                            created_user_id = Some(candidate);
                            break;
                        }
                        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                            continue;
                        }
                        Err(err) => {
                            tx.rollback().await.ok();
                            return Err(ProxyError::Database(err));
                        }
                    }
                }

                let Some(user_id) = created_user_id else {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(
                        "failed to allocate unique local user id".to_string(),
                    ));
                };

                let zero_base = AccountQuotaLimits::zero_base();
                sqlx::query(
                    r#"INSERT INTO account_quota_limits (
                           user_id,
                           hourly_any_limit,
                           hourly_limit,
                           daily_limit,
                           monthly_limit,
                           inherits_defaults,
                           created_at,
                           updated_at
                       )
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
                )
                .bind(&user_id)
                .bind(zero_base.hourly_any_limit)
                .bind(zero_base.hourly_limit)
                .bind(zero_base.daily_limit)
                .bind(zero_base.monthly_limit)
                .bind(0)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;

                let inserted_account = sqlx::query(
                    r#"INSERT INTO oauth_accounts
                       (provider, provider_user_id, user_id, username, name, avatar_template, active, trust_level, raw_payload, created_at, updated_at)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                )
                .bind(&profile.provider)
                .bind(&profile.provider_user_id)
                .bind(&user_id)
                .bind(username.clone())
                .bind(display_name.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(profile.trust_level)
                .bind(profile.raw_payload_json.clone())
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_account {
                    Ok(_) => user_id,
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        tx.rollback().await.ok();
                        continue;
                    }
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            };

            sqlx::query(
                r#"UPDATE users
                   SET display_name = ?, username = ?, avatar_template = ?, active = ?, updated_at = ?, last_login_at = ?
                   WHERE id = ?"#,
            )
            .bind(display_name.clone())
            .bind(username.clone())
            .bind(avatar.clone())
            .bind(active)
            .bind(now)
            .bind(now)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"UPDATE oauth_accounts
                   SET username = ?, name = ?, avatar_template = ?, active = ?, trust_level = ?, raw_payload = ?, updated_at = ?
                   WHERE provider = ? AND provider_user_id = ?"#,
            )
            .bind(username.clone())
            .bind(display_name.clone())
            .bind(avatar.clone())
            .bind(active)
            .bind(profile.trust_level)
            .bind(profile.raw_payload_json.clone())
            .bind(now)
            .bind(&profile.provider)
            .bind(&profile.provider_user_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            if profile.provider == "linuxdo" {
                self.sync_linuxdo_system_tag_binding_best_effort(&user_id, profile.trust_level)
                    .await;
            }
            return Ok(UserIdentity {
                user_id,
                provider: profile.provider.clone(),
                provider_user_id: profile.provider_user_id.clone(),
                display_name,
                username,
                avatar_template: avatar,
            });
        }

        Err(ProxyError::Other(
            "failed to upsert oauth account after retries".to_string(),
        ))
    }

    pub(crate) async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        let row = sqlx::query_scalar::<_, i64>(
            r#"SELECT 1
               FROM oauth_accounts
               WHERE provider = ? AND provider_user_id = ?
               LIMIT 1"#,
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub(crate) async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.ensure_user_token_binding_with_preferred(user_id, note, None)
            .await
    }

    pub(crate) async fn fetch_active_token_secret_by_id(
        &self,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT secret
               FROM auth_tokens
               WHERE id = ? AND enabled = 1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    pub(crate) async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        let preferred_token_id = preferred_token_id
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(preferred_token_id) = preferred_token_id
            && let Some(preferred_secret) = self
                .fetch_active_token_secret_by_id(preferred_token_id)
                .await?
        {
            for _ in 0..4 {
                let now = Utc::now().timestamp();
                let mut tx = self.pool.begin().await?;

                let owner = sqlx::query_as::<_, (String,)>(
                    r#"SELECT user_id
                       FROM user_token_bindings
                       WHERE token_id = ?
                       LIMIT 1"#,
                )
                .bind(preferred_token_id)
                .fetch_optional(&mut *tx)
                .await?;

                match owner {
                    Some((owner_user_id,)) if owner_user_id != user_id => {
                        tx.rollback().await.ok();
                        break;
                    }
                    Some(_) => {
                        let touch = sqlx::query(
                            r#"UPDATE user_token_bindings
                               SET updated_at = ?
                               WHERE user_id = ? AND token_id = ?"#,
                        )
                        .bind(now)
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .execute(&mut *tx)
                        .await;
                        match touch {
                            Ok(_) => {
                                tx.commit().await?;
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                    None => {
                        let result = sqlx::query(
                            r#"INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at)
                               VALUES (?, ?, ?, ?)
                               ON CONFLICT(user_id, token_id) DO UPDATE SET
                                   updated_at = excluded.updated_at"#,
                        )
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .bind(now)
                        .bind(now)
                        .execute(&mut *tx)
                        .await;

                        match result {
                            Ok(_) => {
                                tx.commit().await?;
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                                tx.rollback().await.ok();
                                continue;
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                }
            }
        }

        if let Some(existing) = self.fetch_user_token_any_status(user_id).await? {
            self.cache_token_binding(&existing.id, Some(user_id)).await;
            return Ok(existing);
        }

        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = Utc::now().timestamp();
        let note = note.unwrap_or("").trim().to_string();

        for _ in 0..4 {
            let mut tx = self.pool.begin().await?;
            if let Some((token_id, secret)) = sqlx::query_as::<_, (String, String)>(
                r#"SELECT b.token_id, t.secret
                   FROM user_token_bindings b
                   JOIN auth_tokens t ON t.id = b.token_id
                   WHERE b.user_id = ?
                   ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
                   LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            {
                tx.rollback().await.ok();
                return Ok(AuthTokenSecret {
                    id: token_id.clone(),
                    token: Self::compose_full_token(&token_id, &secret),
                });
            }

            let mut created: Option<(String, String)> = None;
            for _ in 0..8 {
                let token_id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);

                let inserted_token = sqlx::query(
                    r#"INSERT INTO auth_tokens
                       (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)"#,
                )
                .bind(&token_id)
                .bind(&secret)
                .bind(&note)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_token {
                    Ok(_) => {
                        created = Some((token_id, secret));
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            }

            let Some((token_id, secret)) = created else {
                tx.rollback().await.ok();
                return Err(ProxyError::Other(
                    "failed to create auth token for user binding".to_string(),
                ));
            };

            let inserted_binding = sqlx::query(
                r#"INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at)
                   VALUES (?, ?, ?, ?)"#,
            )
            .bind(user_id)
            .bind(&token_id)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;

            match inserted_binding {
                Ok(_) => {
                    tx.commit().await?;
                    self.cache_token_binding(&token_id, Some(user_id)).await;
                    return Ok(AuthTokenSecret {
                        id: token_id.clone(),
                        token: Self::compose_full_token(&token_id, &secret),
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    tx.rollback().await.ok();
                    continue;
                }
                Err(err) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Database(err));
                }
            }
        }

        Err(ProxyError::Other(
            "failed to ensure user token binding after retries".to_string(),
        ))
    }

    pub(crate) async fn fetch_user_token_any_status(
        &self,
        user_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"SELECT b.token_id, t.secret
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ?
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(token_id, secret)| AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    pub(crate) async fn get_user_token(
        &self,
        user_id: &str,
    ) -> Result<UserTokenLookup, ProxyError> {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<i64>, Option<i64>)>(
            r#"SELECT b.token_id, t.secret, t.enabled, t.deleted_at
               FROM user_token_bindings b
               LEFT JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ?
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((token_id, maybe_secret, maybe_enabled, maybe_deleted_at)) = row else {
            return Ok(UserTokenLookup::MissingBinding);
        };
        let Some(secret) = maybe_secret else {
            return Ok(UserTokenLookup::Unavailable);
        };
        let enabled = maybe_enabled.unwrap_or(0);
        if enabled != 1 || maybe_deleted_at.is_some() {
            return Ok(UserTokenLookup::Unavailable);
        }

        Ok(UserTokenLookup::Found(AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    pub(crate) async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        let now = Utc::now().timestamp();
        let expires_at = now + session_max_age_secs.max(60);

        sqlx::query("DELETE FROM user_sessions WHERE expires_at < ? OR revoked_at IS NOT NULL")
            .bind(now)
            .execute(&self.pool)
            .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let inserted = sqlx::query(
                r#"INSERT INTO user_sessions (token, user_id, provider, created_at, expires_at, revoked_at)
                   VALUES (?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&token)
            .bind(&user.user_id)
            .bind(&user.provider)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(_) => {
                    return Ok(UserSession {
                        token,
                        user: user.clone(),
                        expires_at,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn get_user_session(
        &self,
        token: &str,
    ) -> Result<Option<UserSession>, ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("DELETE FROM user_sessions WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await?;

        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
            ),
        >(
            r#"SELECT
                    s.token,
                    s.user_id,
                    s.provider,
                    oa.provider_user_id,
                    u.display_name,
                    u.username,
                    u.avatar_template,
                    s.expires_at
               FROM user_sessions s
               JOIN users u ON u.id = s.user_id
               LEFT JOIN oauth_accounts oa ON oa.user_id = u.id AND oa.provider = s.provider
               WHERE s.token = ? AND s.revoked_at IS NULL AND s.expires_at > ?
               LIMIT 1"#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                token,
                user_id,
                provider,
                provider_user_id,
                display_name,
                username,
                avatar_template,
                expires_at,
            )| UserSession {
                token,
                user: UserIdentity {
                    user_id,
                    provider,
                    provider_user_id: provider_user_id.unwrap_or_default(),
                    display_name,
                    username,
                    avatar_template,
                },
                expires_at,
            },
        ))
    }

    pub(crate) async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE user_sessions SET revoked_at = ? WHERE token = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ----- Token usage logs & metrics -----
    pub(crate) async fn backfill_auth_token_log_request_kinds(&self) -> Result<(), ProxyError> {
        let fallback_key_sql = token_request_kind_fallback_key_sql();
        let fallback_label_sql = token_request_kind_fallback_label_sql();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        // Normalize legacy rows once at startup so read paths can filter directly on stored request kind columns.
        let query = format!(
            r#"
            UPDATE auth_token_logs
            SET
                request_kind_key = {fallback_key_sql},
                request_kind_label = {fallback_label_sql}
            WHERE {needs_fallback_sql}
            "#,
        );
        sqlx::query(query.as_str()).execute(&self.pool).await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn insert_token_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        let counts_business_quota = if counts_business_quota { 1i64 } else { 0i64 };
        let failure_kind = failure_kind
            .map(str::to_string)
            .or_else(|| classify_failure_kind(path, http_status, mcp_status, error_message, &[]));
        let key_effect_summary = key_effect_summary.map(str::to_string);
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status,
                request_kind_key, request_kind_label, request_kind_detail,
                result_status, error_message, failure_kind, key_effect_code, key_effect_summary,
                counts_business_quota, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(failure_kind)
        .bind(key_effect_code)
        .bind(key_effect_summary)
        .bind(counts_business_quota)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(created_at)
        .bind(token_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn insert_token_log_pending_billing(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        let created_at = Utc::now().timestamp();
        let counts_business_quota = if counts_business_quota { 1i64 } else { 0i64 };
        let failure_kind = failure_kind
            .map(str::to_string)
            .or_else(|| classify_failure_kind(path, http_status, mcp_status, error_message, &[]));
        let key_effect_summary = key_effect_summary.map(str::to_string);
        let log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
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
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(failure_kind)
        .bind(key_effect_code)
        .bind(key_effect_summary)
        .bind(counts_business_quota)
        .bind(business_credits)
        .bind(billing_subject)
        .bind(BILLING_STATE_PENDING)
        .bind(api_key_id)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE auth_tokens SET total_requests = total_requests + 1, last_used_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(created_at)
        .bind(token_id)
        .execute(&self.pool)
        .await?;

        Ok(log_id)
    }

    pub(crate) async fn list_pending_billing_log_ids(
        &self,
        billing_subject: &str,
    ) -> Result<Vec<i64>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT id
            FROM auth_token_logs
            WHERE billing_state = ? AND billing_subject = ? AND COALESCE(business_credits, 0) > 0
            ORDER BY id ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(billing_subject)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn list_pending_billing_subjects_for_token(
        &self,
        token_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT DISTINCT billing_subject
            FROM auth_token_logs
            WHERE billing_state = ?
              AND token_id = ?
              AND billing_subject IS NOT NULL
              AND COALESCE(business_credits, 0) > 0
            ORDER BY billing_subject ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(token_id)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn apply_pending_billing_log(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        let mut tx = self.pool.begin().await?;
        #[cfg(test)]
        let force_claim_miss = {
            let mut forced = self.forced_pending_claim_miss_log_ids.lock().await;
            forced.remove(&log_id)
        };
        #[cfg(not(test))]
        let force_claim_miss = false;

        let claimed = if force_claim_miss {
            None
        } else {
            sqlx::query_as::<_, (i64, Option<String>, i64, Option<String>, String)>(
                r#"
                UPDATE auth_token_logs
                SET billing_state = ?
                WHERE id = ? AND billing_state = ?
                RETURNING COALESCE(business_credits, 0), billing_subject, created_at, api_key_id, result_status
                "#,
            )
            .bind(BILLING_STATE_CHARGED)
            .bind(log_id)
            .bind(BILLING_STATE_PENDING)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((credits, billing_subject, created_at, api_key_id, result_status)) = claimed
        else {
            let billing_state = sqlx::query_scalar::<_, String>(
                "SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1",
            )
            .bind(log_id)
            .fetch_optional(&mut *tx)
            .await?;
            match billing_state.as_deref() {
                Some(BILLING_STATE_CHARGED) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::AlreadySettled);
                }
                Some(BILLING_STATE_PENDING) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::RetryLater);
                }
                Some(other) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::QuotaDataMissing {
                        reason: format!(
                            "invalid billing_state for auth_token_logs.id={log_id}: {other}",
                        ),
                    });
                }
                None => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(format!(
                        "pending billing log not found: {log_id}",
                    )));
                }
            }
        };

        if credits <= 0 {
            tx.commit().await?;
            return Ok(PendingBillingSettleOutcome::Charged);
        }

        let Some(billing_subject) = billing_subject else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!("missing billing_subject for auth_token_logs.id={log_id}"),
            });
        };

        let charge_time = Utc
            .timestamp_opt(created_at, 0)
            .single()
            .unwrap_or_else(Utc::now);
        let charge_ts = charge_time.timestamp();
        let minute_bucket = charge_ts - (charge_ts % SECS_PER_MINUTE);
        let hour_bucket = charge_ts - (charge_ts % SECS_PER_HOUR);
        let month_start = start_of_month(charge_time).timestamp();

        if let Some(user_id) = billing_subject.strip_prefix("account:") {
            sqlx::query(
                r#"
                INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id, bucket_start, granularity)
                DO UPDATE SET count = account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id, bucket_start, granularity)
                DO UPDATE SET count = account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(hour_bucket)
            .bind(GRANULARITY_HOUR)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                        ELSE account_monthly_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                        ELSE account_monthly_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(user_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;

            if let Some(api_key_id) = api_key_id.as_deref() {
                self.increment_api_key_user_usage_bucket(
                    &mut tx,
                    api_key_id,
                    user_id,
                    local_day_bucket_start_utc_ts(charge_ts),
                    credits,
                    result_status.as_str(),
                )
                .await?;

                if result_status == OUTCOME_SUCCESS {
                    self.refresh_user_api_key_binding(&mut tx, user_id, api_key_id, created_at)
                        .await?;
                }
            }
        } else if let Some(token_id) = billing_subject.strip_prefix("token:") {
            sqlx::query(
                r#"
                INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(token_id, bucket_start, granularity)
                DO UPDATE SET count = token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(token_id, bucket_start, granularity)
                DO UPDATE SET count = token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(hour_bucket)
            .bind(GRANULARITY_HOUR)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO auth_token_quota (token_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(token_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                        ELSE auth_token_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                        ELSE auth_token_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(token_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;
        } else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!(
                    "invalid billing_subject for auth_token_logs.id={log_id}: {billing_subject}",
                ),
            });
        }

        tx.commit().await?;
        Ok(PendingBillingSettleOutcome::Charged)
    }

    pub(crate) async fn annotate_pending_billing_log(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET error_message = CASE
                WHEN error_message IS NULL OR error_message = '' THEN ?
                WHEN error_message = ? THEN error_message
                ELSE error_message || ' | ' || ?
            END
            WHERE id = ?
            "#,
        )
        .bind(message)
        .bind(message)
        .bind(message)
        .bind(log_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn acquire_quota_subject_lock(
        &self,
        subject: &str,
        ttl: Duration,
        wait_timeout: Duration,
    ) -> Result<QuotaSubjectDbLease, ProxyError> {
        let owner = format!(
            "{}:{}",
            std::process::id(),
            QUOTA_SUBJECT_LOCK_OWNER_SEQ.fetch_add(1, AtomicOrdering::Relaxed)
        );
        let deadline = Instant::now() + wait_timeout;
        let ttl_secs = ttl.as_secs().max(1) as i64;

        loop {
            let now = Utc::now().timestamp();
            let expires_at = now + ttl_secs;
            let mut tx = self.pool.begin().await?;
            sqlx::query("DELETE FROM quota_subject_locks WHERE subject = ? AND expires_at <= ?")
                .bind(subject)
                .bind(now)
                .execute(&mut *tx)
                .await?;

            let inserted = sqlx::query(
                r#"
                INSERT OR IGNORE INTO quota_subject_locks (subject, owner, expires_at, updated_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(subject)
            .bind(&owner)
            .bind(expires_at)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            if inserted.rows_affected() == 1 {
                tx.commit().await?;
                return Ok(QuotaSubjectDbLease {
                    subject: subject.to_string(),
                    owner,
                    ttl,
                });
            }

            tx.rollback().await.ok();
            if Instant::now() >= deadline {
                return Err(ProxyError::Other(format!(
                    "timed out acquiring quota subject lock for {subject}",
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub(crate) async fn refresh_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let expires_at = now + lease.ttl.as_secs().max(1) as i64;
        let rows = sqlx::query(
            "UPDATE quota_subject_locks SET expires_at = ?, updated_at = ? WHERE subject = ? AND owner = ?",
        )
        .bind(expires_at)
        .bind(now)
        .bind(&lease.subject)
        .bind(&lease.owner)
        .execute(&self.pool)
        .await?;
        if rows.rows_affected() == 0 {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                lease.subject,
            )));
        }
        Ok(())
    }

    pub(crate) async fn release_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        sqlx::query("DELETE FROM quota_subject_locks WHERE subject = ? AND owner = ?")
            .bind(&lease.subject)
            .bind(&lease.owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fetch_token_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        type TokenLogRow = (
            i64,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            i64,
        );
        let rows = if let Some(bid) = before_id {
            sqlx::query_as::<_, TokenLogRow>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                       request_kind_key, request_kind_label, request_kind_detail,
                       result_status, error_message, failure_kind, key_effect_code,
                       key_effect_summary, created_at
                FROM auth_token_logs
                WHERE token_id = ? AND id < ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(bid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, TokenLogRow>(
                r#"
                SELECT id, method, path, query, http_status, mcp_status,
                       CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                       request_kind_key, request_kind_label, request_kind_detail,
                       result_status, error_message, failure_kind, key_effect_code,
                       key_effect_summary, created_at
                FROM auth_token_logs
                WHERE token_id = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(token_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    business_credits,
                    request_kind_key,
                    request_kind_label,
                    request_kind_detail,
                    result_status,
                    error_message,
                    failure_kind,
                    key_effect_code,
                    key_effect_summary,
                    created_at,
                )| {
                    let request_kind = finalize_token_request_kind(
                        method.as_str(),
                        path.as_str(),
                        query.as_deref(),
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                    );
                    TokenLogRecord {
                        id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        business_credits,
                        request_kind_key: request_kind.key,
                        request_kind_label: request_kind.label,
                        request_kind_detail: request_kind.detail,
                        result_status,
                        error_message,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        created_at,
                    }
                },
            )
            .collect())
    }

    pub async fn fetch_token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let end_exclusive = until.unwrap_or(now_ts);
        if end_exclusive <= since {
            return Ok(TokenSummary {
                total_requests: 0,
                success_count: 0,
                error_count: 0,
                quota_exhausted_count: 0,
                last_activity: None,
            });
        }

        let rows = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count,
                quota_exhausted_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(since)
        .bind(end_exclusive)
        .fetch_all(&self.pool)
        .await?;

        let mut total_requests = 0;
        let mut success_count = 0;
        let mut system_failure_count = 0;
        let mut external_failure_count = 0;
        let mut quota_exhausted_count = 0;
        let mut last_activity: Option<i64> = None;

        for (bucket_start, success, system_failure, external_failure, quota_exhausted) in rows {
            success_count += success;
            system_failure_count += system_failure;
            external_failure_count += external_failure;
            quota_exhausted_count += quota_exhausted;
            total_requests += success + system_failure + external_failure + quota_exhausted;
            let bucket_end = bucket_start + TOKEN_USAGE_STATS_BUCKET_SECS;
            last_activity = Some(match last_activity {
                Some(prev) if prev > bucket_end => prev,
                _ => bucket_end,
            });
        }

        let error_count = system_failure_count + external_failure_count;

        Ok(TokenSummary {
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            last_activity,
        })
    }

    pub async fn fetch_token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
    ) -> Result<(Vec<TokenLogRecord>, i64), ProxyError> {
        let per_page = per_page.clamp(1, 200) as i64;
        let page = page.max(1) as i64;
        let offset = (page - 1) * per_page;
        type TokenLogRow = (
            i64,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            i64,
        );
        let filtered_request_kinds: Vec<&str> = request_kinds
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        let fallback_key_sql = token_request_kind_fallback_key_sql();

        let mut total_query =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ");
        total_query.push_bind(token_id);
        total_query.push(" AND created_at >= ");
        total_query.push_bind(since);
        if let Some(until) = until {
            total_query.push(" AND created_at < ");
            total_query.push_bind(until);
        }
        if !filtered_request_kinds.is_empty() {
            total_query.push(" AND (request_kind_key IN (");
            {
                let mut separated = total_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            total_query.push(" OR (");
            total_query.push(needs_fallback_sql);
            total_query.push(" AND ");
            total_query.push(fallback_key_sql);
            total_query.push(" IN (");
            {
                let mut separated = total_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            total_query.push("))");
        }
        let total: i64 = total_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        let mut rows_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT id, method, path, query, http_status, mcp_status,
                   CASE WHEN billing_state = 'charged' THEN business_credits ELSE NULL END,
                   request_kind_key,
                   request_kind_label,
                   request_kind_detail,
                   result_status, error_message, failure_kind, key_effect_code,
                   key_effect_summary, created_at
            FROM auth_token_logs
            WHERE token_id =
            "#
            .to_string(),
        );
        rows_query.push_bind(token_id);
        rows_query.push(" AND created_at >= ");
        rows_query.push_bind(since);
        if let Some(until) = until {
            rows_query.push(" AND created_at < ");
            rows_query.push_bind(until);
        }
        if !filtered_request_kinds.is_empty() {
            rows_query.push(" AND (request_kind_key IN (");
            {
                let mut separated = rows_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            rows_query.push(" OR (");
            rows_query.push(needs_fallback_sql);
            rows_query.push(" AND ");
            rows_query.push(fallback_key_sql);
            rows_query.push(" IN (");
            {
                let mut separated = rows_query.separated(", ");
                for kind in &filtered_request_kinds {
                    separated.push_bind(kind);
                }
                separated.push_unseparated(")");
            }
            rows_query.push("))");
        }
        rows_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        rows_query.push_bind(per_page);
        rows_query.push(" OFFSET ");
        rows_query.push_bind(offset);
        let rows = rows_query
            .build_query_as::<TokenLogRow>()
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(
                |(
                    id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    business_credits,
                    request_kind_key,
                    request_kind_label,
                    request_kind_detail,
                    result_status,
                    error_message,
                    failure_kind,
                    key_effect_code,
                    key_effect_summary,
                    created_at,
                )| {
                    let request_kind = finalize_token_request_kind(
                        method.as_str(),
                        path.as_str(),
                        query.as_deref(),
                        request_kind_key,
                        request_kind_label,
                        request_kind_detail,
                    );
                    TokenLogRecord {
                        id,
                        method,
                        path,
                        query,
                        http_status,
                        mcp_status,
                        business_credits,
                        request_kind_key: request_kind.key,
                        request_kind_label: request_kind.label,
                        request_kind_detail: request_kind.detail,
                        result_status,
                        error_message,
                        failure_kind,
                        key_effect_code,
                        key_effect_summary,
                        created_at,
                    }
                },
            )
            .collect();

        Ok((items, total))
    }

    pub async fn fetch_token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        type RequestKindOptionRow = (String, String, i64, i64);
        let fallback_key_sql = token_request_kind_fallback_key_sql();
        let fallback_label_sql = token_request_kind_fallback_label_sql();
        let needs_fallback_sql = token_request_kind_needs_fallback_sql();
        let mut stored_query = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT
                request_kind_key,
                request_kind_label,
                MAX(CASE WHEN counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                MAX(CASE WHEN counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
            FROM auth_token_logs
            WHERE token_id =
            "#,
        );
        stored_query.push_bind(token_id);
        stored_query.push(" AND created_at >= ");
        stored_query.push_bind(since);
        if let Some(until) = until {
            stored_query.push(" AND created_at < ");
            stored_query.push_bind(until);
        }
        stored_query.push(
            r#"
              AND request_kind_key IS NOT NULL
              AND TRIM(request_kind_key) <> ''
              AND request_kind_label IS NOT NULL
              AND TRIM(request_kind_label) <> ''
              AND NOT (
            "#,
        );
        stored_query.push(token_request_kind_needs_fallback_sql());
        stored_query.push(") GROUP BY request_kind_key, request_kind_label");

        let options = stored_query
            .build_query_as::<RequestKindOptionRow>()
            .fetch_all(&self.pool)
            .await?;

        let legacy_fallback_query = if until.is_some() {
            format!(
                r#"
                SELECT
                    {fallback_key_sql} AS request_kind_key,
                    {fallback_label_sql} AS request_kind_label,
                    MAX(CASE WHEN counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                    MAX(CASE WHEN counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
                FROM auth_token_logs
                WHERE token_id = ?
                  AND created_at >= ?
                  AND created_at < ?
                  AND {needs_fallback_sql}
                GROUP BY 1, 2
                "#
            )
        } else {
            format!(
                r#"
                SELECT
                    {fallback_key_sql} AS request_kind_key,
                    {fallback_label_sql} AS request_kind_label,
                    MAX(CASE WHEN counts_business_quota = 1 THEN 1 ELSE 0 END) AS has_billable,
                    MAX(CASE WHEN counts_business_quota = 0 THEN 1 ELSE 0 END) AS has_non_billable
                FROM auth_token_logs
                WHERE token_id = ?
                  AND created_at >= ?
                  AND {needs_fallback_sql}
                GROUP BY 1, 2
                "#
            )
        };
        let legacy_options = if let Some(until) = until {
            sqlx::query_as::<_, RequestKindOptionRow>(legacy_fallback_query.as_str())
                .bind(token_id)
                .bind(since)
                .bind(until)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query_as::<_, RequestKindOptionRow>(legacy_fallback_query.as_str())
                .bind(token_id)
                .bind(since)
                .fetch_all(&self.pool)
                .await?
        };
        let mut options_by_key = BTreeMap::<String, (String, bool, bool)>::new();
        for (key, label, has_billable, has_non_billable) in
            options.into_iter().chain(legacy_options.into_iter())
        {
            match options_by_key.get_mut(&key) {
                Some((current_label, current_has_billable, current_has_non_billable))
                    if prefer_request_kind_label(current_label, &label) =>
                {
                    *current_label = label;
                    *current_has_billable |= has_billable != 0;
                    *current_has_non_billable |= has_non_billable != 0;
                }
                Some((_, current_has_billable, current_has_non_billable)) => {
                    *current_has_billable |= has_billable != 0;
                    *current_has_non_billable |= has_non_billable != 0;
                }
                None => {
                    options_by_key.insert(key, (label, has_billable != 0, has_non_billable != 0));
                }
            }
        }

        let mut normalized_options = options_by_key
            .into_iter()
            .map(
                |(key, (label, has_billable, has_non_billable))| TokenRequestKindOption {
                    protocol_group: token_request_kind_protocol_group(&key).to_string(),
                    billing_group: token_request_kind_option_billing_group(
                        &key,
                        has_billable,
                        has_non_billable,
                    )
                    .to_string(),
                    key,
                    label,
                },
            )
            .collect::<Vec<_>>();
        normalized_options.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.key.cmp(&right.key))
        });

        Ok(normalized_options)
    }

    pub async fn fetch_token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        let hours = hours.clamp(1, 168); // up to 7 days
        let now_ts = Utc::now().timestamp();
        let current_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let window_start = current_bucket - (hours - 1) * SECS_PER_HOUR;
        let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT
                bucket_start,
                success_count,
                system_failure_count,
                external_failure_count
            FROM token_usage_stats
            WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(token_id)
        .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
        .bind(window_start)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(bucket_start, success_count, system_failure_count, external_failure_count)| {
                    TokenHourlyBucket {
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    }
                },
            )
            .collect())
    }

    pub async fn fetch_token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        if until <= since {
            return Err(ProxyError::Other("invalid usage window".into()));
        }
        if bucket_secs <= 0 {
            return Err(ProxyError::Other("bucket_secs must be positive".into()));
        }
        let bucket_secs = match bucket_secs {
            s if s == SECS_PER_HOUR => SECS_PER_HOUR,
            s if s == SECS_PER_DAY => SECS_PER_DAY,
            _ => {
                return Err(ProxyError::Other(
                    "bucket_secs must be either 3600 (hour) or 86400 (day)".into(),
                ));
            }
        };
        let span = until - since;
        let mut bucket_count = span / bucket_secs;
        if span % bucket_secs != 0 {
            bucket_count += 1;
        }
        if bucket_count > 1000 {
            return Err(ProxyError::Other(
                "requested usage series is too large".into(),
            ));
        }
        if bucket_secs == SECS_PER_HOUR {
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            Ok(rows
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        success_count,
                        system_failure_count,
                        external_failure_count,
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect())
        } else {
            // Aggregate hourly stats into daily buckets.
            let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                r#"
                SELECT
                    bucket_start,
                    success_count,
                    system_failure_count,
                    external_failure_count
                FROM token_usage_stats
                WHERE token_id = ? AND bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
                ORDER BY bucket_start ASC
                "#,
            )
            .bind(token_id)
            .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
            .bind(since)
            .bind(until)
            .fetch_all(&self.pool)
            .await?;

            let mut by_day: HashMap<i64, (i64, i64, i64)> = HashMap::new();
            for (bucket_start, success, system_failure, external_failure) in rows {
                let day_start = bucket_start - (bucket_start % SECS_PER_DAY);
                let entry = by_day.entry(day_start).or_insert((0, 0, 0));
                entry.0 += success;
                entry.1 += system_failure;
                entry.2 += external_failure;
            }

            let mut buckets: Vec<TokenUsageBucket> = by_day
                .into_iter()
                .map(
                    |(
                        bucket_start,
                        (success_count, system_failure_count, external_failure_count),
                    )| {
                        TokenUsageBucket {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        }
                    },
                )
                .collect();
            buckets.sort_by_key(|b| b.bucket_start);
            Ok(buckets)
        }
    }

    pub(crate) async fn reset_monthly(&self) -> Result<(), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();

        let now_ts = now.timestamp();

        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE status = ?
              AND status_changed_at IS NOT NULL
              AND status_changed_at < ?
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now_ts)
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn mark_quota_exhausted(&self, key: &str) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?, last_used_at = ?
            WHERE api_key = ? AND status NOT IN (?, ?) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(now)
        .bind(now)
        .bind(key)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn restore_active_status(&self, key: &str) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE api_key = ? AND status = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub(crate) async fn quarantine_key_by_id(
        &self,
        key_id: &str,
        source: &str,
        reason_code: &str,
        reason_summary: &str,
        reason_detail: &str,
    ) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let quarantine_id = nanoid!(12);
        let insert_result = sqlx::query(
            r#"
            INSERT INTO api_key_quarantines (
                id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
            ON CONFLICT(key_id) WHERE cleared_at IS NULL DO NOTHING
            "#,
        )
        .bind(quarantine_id)
        .bind(key_id)
        .bind(source)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(reason_detail)
        .bind(now)
        .execute(&self.pool)
        .await?;

        if insert_result.rows_affected() == 0 {
            sqlx::query(
                r#"
                UPDATE api_key_quarantines
                SET source = ?, reason_code = ?, reason_summary = ?, reason_detail = ?
                WHERE key_id = ? AND cleared_at IS NULL
                "#,
            )
            .bind(source)
            .bind(reason_code)
            .bind(reason_summary)
            .bind(reason_detail)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
            return Ok(false);
        }

        Ok(true)
    }

    pub(crate) async fn clear_key_quarantine_by_id(
        &self,
        key_id: &str,
    ) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let res = sqlx::query(
            r#"
            UPDATE api_key_quarantines
            SET cleared_at = ?
            WHERE key_id = ? AND cleared_at IS NULL
            "#,
        )
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    // Admin ops: add/undelete key by secret
    pub(crate) async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.add_or_undelete_key_in_group(api_key, None).await
    }

    // Admin ops: add/undelete key by secret and optionally assign a group.
    pub(crate) async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        let (id, _) = self
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key, group, None, None, None, false,
            )
            .await?;
        Ok(id)
    }

    // Admin ops: add/undelete key by secret with status
    pub(crate) async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration(
            api_key, None, None, None, None, false,
        )
        .await
    }

    // Admin ops: add/undelete key by secret with status and optional group assignment.
    //
    // Behavior:
    // - created / undeleted: set group_name when group is provided and non-empty
    // - existed: set group_name only if the stored group is empty (do not override)
    pub(crate) async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration(
            api_key, group, None, None, None, false,
        )
        .await
    }

    pub(crate) async fn add_or_undelete_key_with_status_in_group_and_registration(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        proxy_affinity: Option<&forward_proxy::ForwardProxyAffinityRecord>,
        hint_only_proxy_affinity: bool,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let normalized_group = group
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string);
        let normalized_registration_ip = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_registration_region = registration_region
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mut retry_idx = 0usize;

        loop {
            match self
                .add_or_undelete_key_with_status_in_group_once(
                    api_key,
                    normalized_group.as_deref(),
                    normalized_registration_ip.as_deref(),
                    normalized_registration_region.as_deref(),
                    proxy_affinity,
                    hint_only_proxy_affinity,
                )
                .await
            {
                Ok(result) => return Ok(result),
                Err(err)
                    if is_transient_sqlite_write_error(&err)
                        && retry_idx < API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS.len() =>
                {
                    let backoff_ms = API_KEY_UPSERT_TRANSIENT_RETRY_BACKOFF_MS[retry_idx];
                    retry_idx += 1;
                    let key_preview = preview_key(api_key);
                    eprintln!(
                        "api key upsert transient sqlite error (api_key_preview={}, attempt={}, backoff={}ms): {}",
                        key_preview, retry_idx, backoff_ms, err
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub(crate) async fn add_or_undelete_key_with_status_in_group_once(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        proxy_affinity: Option<&forward_proxy::ForwardProxyAffinityRecord>,
        hint_only_proxy_affinity: bool,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().timestamp();

        let operation_result: Result<(String, ApiKeyUpsertStatus), ProxyError> = async {
            if let Some((id, deleted_at, existing_group, existing_registration_ip, existing_registration_region)) =
                sqlx::query_as::<_, (String, Option<i64>, Option<String>, Option<String>, Option<String>)>(
                    "SELECT id, deleted_at, group_name, registration_ip, registration_region FROM api_keys WHERE api_key = ? LIMIT 1",
                )
                .bind(api_key)
                .fetch_optional(&mut *tx)
                .await?
            {
                let existing_empty = existing_group
                    .as_deref()
                    .map(str::trim)
                    .map(|g| g.is_empty())
                    .unwrap_or(true);
                let existing_has_registration_metadata =
                    existing_registration_ip.is_some() || existing_registration_region.is_some();
                let should_refresh_registration =
                    registration_ip.is_some() || registration_region.is_some();
                let should_persist_proxy_affinity =
                    !hint_only_proxy_affinity || !existing_has_registration_metadata;

                let mut assignments = Vec::new();
                if deleted_at.is_some() {
                    assignments.push("deleted_at = NULL".to_string());
                }
                if group.is_some() && existing_empty {
                    assignments.push("group_name = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_ip = ?".to_string());
                }
                if should_refresh_registration {
                    assignments.push("registration_region = ?".to_string());
                }

                if !assignments.is_empty() {
                    let mut query = String::from("UPDATE api_keys SET ");
                    query.push_str(&assignments.join(", "));
                    query.push_str(" WHERE id = ?");
                    let mut sql = sqlx::query(&query);
                    if let Some(group) = group
                        && existing_empty
                    {
                        sql = sql.bind(group);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(registration_ip);
                    }
                    if should_refresh_registration {
                        sql = sql.bind(registration_region);
                    }
                    sql.bind(&id).execute(&mut *tx).await?;
                }
                if should_persist_proxy_affinity
                    && let Some(proxy_affinity) = proxy_affinity
                {
                    sqlx::query(
                        r#"
                        INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                        VALUES (?1, ?2, ?3, strftime('%s', 'now'))
                        ON CONFLICT(key_id) DO UPDATE SET
                            primary_proxy_key = excluded.primary_proxy_key,
                            secondary_proxy_key = excluded.secondary_proxy_key,
                            updated_at = strftime('%s', 'now')
                        "#,
                    )
                    .bind(&id)
                    .bind(proxy_affinity.primary_proxy_key.as_deref())
                    .bind(proxy_affinity.secondary_proxy_key.as_deref())
                    .execute(&mut *tx)
                    .await?;
                }

                if deleted_at.is_some() {
                    return Ok((id, ApiKeyUpsertStatus::Undeleted));
                }

                return Ok((id, ApiKeyUpsertStatus::Existed));
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (
                    id,
                    api_key,
                    group_name,
                    registration_ip,
                    registration_region,
                    status,
                    created_at,
                    status_changed_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(api_key)
            .bind(group)
            .bind(registration_ip)
            .bind(registration_region)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if let Some(proxy_affinity) = proxy_affinity {
                sqlx::query(
                    r#"
                    INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
                    VALUES (?1, ?2, ?3, strftime('%s', 'now'))
                    ON CONFLICT(key_id) DO UPDATE SET
                        primary_proxy_key = excluded.primary_proxy_key,
                        secondary_proxy_key = excluded.secondary_proxy_key,
                        updated_at = strftime('%s', 'now')
                    "#,
                )
                .bind(&id)
                .bind(proxy_affinity.primary_proxy_key.as_deref())
                .bind(proxy_affinity.secondary_proxy_key.as_deref())
                .execute(&mut *tx)
                .await?;
            }
            Ok((id, ApiKeyUpsertStatus::Created))
        }
        .await;

        match operation_result {
            Ok(result) => {
                tx.commit().await?;
                Ok(result)
            }
            Err(err) => {
                tx.rollback().await.ok();
                Err(err)
            }
        }
    }

    // Admin ops: soft-delete by ID (mark deleted_at)
    pub(crate) async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(key_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE api_keys
            SET status = ?, status_changed_at = ?
            WHERE id = ? AND status IN (?, ?) AND deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(now)
        .bind(key_id)
        .bind(STATUS_DISABLED)
        .bind(STATUS_EXHAUSTED)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn touch_key(&self, key: &str, timestamp: i64) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = ?
            WHERE api_key = ?
            "#,
        )
        .bind(timestamp)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn log_attempt(&self, entry: AttemptLog<'_>) -> Result<(), ProxyError> {
        let created_at = Utc::now().timestamp();
        let status_code = entry.status.map(|code| code.as_u16() as i64);
        let failure_kind = entry.failure_kind.map(str::to_string).or_else(|| {
            classify_failure_kind(
                entry.path,
                status_code,
                entry.tavily_status_code,
                entry.error,
                entry.response_body,
            )
        });
        let key_effect_summary = entry.key_effect_summary.map(str::to_string);

        let forwarded_json =
            serde_json::to_string(entry.forwarded_headers).unwrap_or_else(|_| "[]".to_string());
        let dropped_json =
            serde_json::to_string(entry.dropped_headers).unwrap_or_else(|_| "[]".to_string());

        let bucket_start = local_day_bucket_start_utc_ts(created_at);
        let (bucket_success, bucket_error, bucket_quota_exhausted) = match entry.outcome {
            OUTCOME_SUCCESS => (1_i64, 0_i64, 0_i64),
            OUTCOME_ERROR => (0_i64, 1_i64, 0_i64),
            OUTCOME_QUOTA_EXHAUSTED => (0_i64, 0_i64, 1_i64),
            _ => (0_i64, 0_i64, 0_i64),
        };

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(entry.key_id)
        .bind(entry.auth_token_id)
        .bind(entry.method.as_str())
        .bind(entry.path)
        .bind(entry.query)
        .bind(status_code)
        .bind(entry.tavily_status_code)
        .bind(entry.error)
        .bind(entry.outcome)
        .bind(failure_kind)
        .bind(entry.key_effect_code)
        .bind(key_effect_summary)
        .bind(entry.request_body)
        .bind(entry.response_body)
        .bind(forwarded_json)
        .bind(dropped_json)
        .bind(created_at)
        .execute(&mut *tx)
        .await?;

        // Daily API-key rollup bucket (bucket_secs=86400, aligned to local midnight).
        sqlx::query(
            r#"
            INSERT INTO api_key_usage_buckets (
                api_key_id,
                bucket_start,
                bucket_secs,
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                updated_at
            ) VALUES (?, ?, 86400, 1, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, bucket_start, bucket_secs)
            DO UPDATE SET
                total_requests = total_requests + 1,
                success_count = success_count + excluded.success_count,
                error_count = error_count + excluded.error_count,
                quota_exhausted_count = quota_exhausted_count + excluded.quota_exhausted_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(entry.key_id)
        .bind(bucket_start)
        .bind(bucket_success)
        .bind(bucket_error)
        .bind(bucket_quota_exhausted)
        .bind(created_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    pub(crate) fn api_key_metrics_from_clause() -> &'static str {
        r#"
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
        "#
    }

    pub(crate) fn api_key_metrics_query(include_quarantine_detail: bool) -> String {
        let quarantine_detail_sql = if include_quarantine_detail {
            "aq.reason_detail AS quarantine_reason_detail,"
        } else {
            "NULL AS quarantine_reason_detail,"
        };
        format!(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.registration_ip,
                ak.registration_region,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                {quarantine_detail_sql}
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            {}
            "#,
            Self::api_key_metrics_from_clause(),
        )
    }

    pub(crate) fn map_api_key_metrics_row(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<ApiKeyMetrics, sqlx::Error> {
        let id: String = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let group_name: Option<String> = row.try_get("group_name")?;
        let registration_ip: Option<String> = row.try_get("registration_ip")?;
        let registration_region: Option<String> = row.try_get("registration_region")?;
        let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
        let last_used_at: i64 = row.try_get("last_used_at")?;
        let deleted_at: Option<i64> = row.try_get("deleted_at")?;
        let quota_limit: Option<i64> = row.try_get("quota_limit")?;
        let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
        let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
        let total_requests: i64 = row.try_get("total_requests")?;
        let success_count: i64 = row.try_get("success_count")?;
        let error_count: i64 = row.try_get("error_count")?;
        let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
        let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
        let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
        let quarantine_reason_summary: Option<String> = row.try_get("quarantine_reason_summary")?;
        let quarantine_reason_detail: Option<String> = row.try_get("quarantine_reason_detail")?;
        let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

        Ok(ApiKeyMetrics {
            id,
            status,
            group_name: normalize_optional_api_key_field(group_name),
            registration_ip: normalize_optional_api_key_field(registration_ip),
            registration_region: normalize_optional_api_key_field(registration_region),
            status_changed_at: status_changed_at.and_then(normalize_timestamp),
            last_used_at: normalize_timestamp(last_used_at),
            deleted_at: deleted_at.and_then(normalize_timestamp),
            quota_limit,
            quota_remaining,
            quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                source,
                reason_code: quarantine_reason_code.unwrap_or_default(),
                reason_summary: quarantine_reason_summary.unwrap_or_default(),
                reason_detail: quarantine_reason_detail.unwrap_or_default(),
                created_at: quarantine_created_at.unwrap_or_default(),
            }),
        })
    }

    pub(crate) fn normalize_api_key_groups(groups: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for group in groups {
            let value = group.trim().to_string();
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_regions(regions: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for region in regions {
            let value = region.trim().to_string();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn normalize_api_key_statuses(statuses: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        for status in statuses {
            let value = status.trim().to_ascii_lowercase();
            if value.is_empty() {
                continue;
            }
            if !normalized.iter().any(|existing| existing == &value) {
                normalized.push(value);
            }
        }
        normalized
    }

    pub(crate) fn push_api_key_group_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        groups: &'a [String],
    ) {
        if groups.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if group.is_empty() {
                builder.push("(TRIM(COALESCE(ak.group_name, '')) = '')");
            } else {
                builder
                    .push("(TRIM(COALESCE(ak.group_name, '')) = ")
                    .push_bind(group)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_status_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        statuses: &'a [String],
    ) {
        if statuses.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, status) in statuses.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            if status == "quarantined" {
                builder.push("(aq.key_id IS NOT NULL)");
            } else {
                builder
                    .push("(aq.key_id IS NULL AND ak.status = ")
                    .push_bind(status)
                    .push(")");
            }
        }
        builder.push(")");
    }

    pub(crate) fn push_api_key_registration_ip_filter<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        registration_ip: Option<&'a str>,
    ) {
        let Some(registration_ip) = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        builder
            .push(" AND TRIM(COALESCE(ak.registration_ip, '')) = ")
            .push_bind(registration_ip);
    }

    pub(crate) fn push_api_key_region_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        regions: &'a [String],
    ) {
        if regions.is_empty() {
            return;
        }

        builder.push(" AND (");
        for (index, region) in regions.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            builder
                .push("(TRIM(COALESCE(ak.registration_region, '')) = ")
                .push_bind(region)
                .push(")");
        }
        builder.push(")");
    }

    pub(crate) async fn fetch_api_key_group_facets(
        &self,
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT TRIM(COALESCE(ak.group_name, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_status_filters(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        Self::push_api_key_region_filters(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_status_facets(
        &self,
        groups: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT CASE WHEN aq.key_id IS NOT NULL THEN 'quarantined' ELSE ak.status END AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut builder, groups);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        Self::push_api_key_region_filters(&mut builder, regions);
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_region_facets(
        &self,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
    ) -> Result<Vec<ApiKeyFacetCount>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT TRIM(COALESCE(ak.registration_region, '')) AS value, COUNT(*) AS count",
        );
        builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut builder, groups);
        Self::push_api_key_status_filters(&mut builder, statuses);
        Self::push_api_key_registration_ip_filter(&mut builder, registration_ip);
        builder.push(" AND TRIM(COALESCE(ak.registration_region, '')) != ''");
        builder.push(" GROUP BY value ORDER BY value ASC");

        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ApiKeyFacetCount {
                    value: row.try_get("value")?,
                    count: row.try_get("count")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_metrics(
        &self,
        include_quarantine_detail: bool,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        let query = format!(
            "{} ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
            Self::api_key_metrics_query(include_quarantine_detail),
        );
        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_api_key_metrics_page(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        let requested_page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let groups = Self::normalize_api_key_groups(groups);
        let statuses = Self::normalize_api_key_statuses(statuses);
        let registration_ip = registration_ip
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let regions = Self::normalize_api_key_regions(regions);

        let mut count_builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
        count_builder.push(Self::api_key_metrics_from_clause());
        Self::push_api_key_group_filters(&mut count_builder, &groups);
        Self::push_api_key_status_filters(&mut count_builder, &statuses);
        Self::push_api_key_registration_ip_filter(&mut count_builder, registration_ip);
        Self::push_api_key_region_filters(&mut count_builder, &regions);
        let total = count_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;
        let total_pages = ((total + per_page - 1) / per_page).max(1);
        let page = requested_page.min(total_pages);
        let offset = (page - 1) * per_page;

        let mut items_builder = QueryBuilder::<Sqlite>::new(Self::api_key_metrics_query(false));
        Self::push_api_key_group_filters(&mut items_builder, &groups);
        Self::push_api_key_status_filters(&mut items_builder, &statuses);
        Self::push_api_key_registration_ip_filter(&mut items_builder, registration_ip);
        Self::push_api_key_region_filters(&mut items_builder, &regions);
        items_builder.push(
            " ORDER BY CASE WHEN ak.status = 'active' THEN 0 ELSE 1 END ASC, COALESCE(ak.last_used_at, 0) DESC, ak.id ASC",
        );
        items_builder.push(" LIMIT ").push_bind(per_page);
        items_builder.push(" OFFSET ").push_bind(offset);
        let items = items_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Self::map_api_key_metrics_row)
            .collect::<Result<Vec<_>, _>>()?;

        let group_counts = self
            .fetch_api_key_group_facets(&statuses, registration_ip, &regions)
            .await?;
        let status_counts = self
            .fetch_api_key_status_facets(&groups, registration_ip, &regions)
            .await?;
        let region_counts = self
            .fetch_api_key_region_facets(&groups, &statuses, registration_ip)
            .await?;

        Ok(PaginatedApiKeyMetrics {
            items,
            total,
            page,
            per_page,
            facets: ApiKeyListFacets {
                groups: group_counts,
                statuses: status_counts,
                regions: region_counts,
            },
        })
    }

    pub(crate) async fn fetch_api_key_metric_by_id(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                ak.id,
                ak.status,
                ak.group_name,
                ak.registration_ip,
                ak.registration_region,
                ak.status_changed_at,
                ak.last_used_at,
                ak.deleted_at,
                ak.quota_limit,
                ak.quota_remaining,
                ak.quota_synced_at,
                aq.source AS quarantine_source,
                aq.reason_code AS quarantine_reason_code,
                aq.reason_summary AS quarantine_reason_summary,
                aq.reason_detail AS quarantine_reason_detail,
                aq.created_at AS quarantine_created_at,
                COALESCE(stats.total_requests, 0) AS total_requests,
                COALESCE(stats.success_count, 0) AS success_count,
                COALESCE(stats.error_count, 0) AS error_count,
                COALESCE(stats.quota_exhausted_count, 0) AS quota_exhausted_count
            FROM api_keys ak
            LEFT JOIN (
                SELECT
                    api_key_id,
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                GROUP BY api_key_id
            ) AS stats
            ON stats.api_key_id = ak.id
            LEFT JOIN api_key_quarantines aq
            ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL AND ak.id = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| -> Result<ApiKeyMetrics, sqlx::Error> {
            let id: String = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            let group_name: Option<String> = row.try_get("group_name")?;
            let registration_ip: Option<String> = row.try_get("registration_ip")?;
            let registration_region: Option<String> = row.try_get("registration_region")?;
            let status_changed_at: Option<i64> = row.try_get("status_changed_at")?;
            let last_used_at: i64 = row.try_get("last_used_at")?;
            let deleted_at: Option<i64> = row.try_get("deleted_at")?;
            let quota_limit: Option<i64> = row.try_get("quota_limit")?;
            let quota_remaining: Option<i64> = row.try_get("quota_remaining")?;
            let quota_synced_at: Option<i64> = row.try_get("quota_synced_at")?;
            let total_requests: i64 = row.try_get("total_requests")?;
            let success_count: i64 = row.try_get("success_count")?;
            let error_count: i64 = row.try_get("error_count")?;
            let quota_exhausted_count: i64 = row.try_get("quota_exhausted_count")?;
            let quarantine_source: Option<String> = row.try_get("quarantine_source")?;
            let quarantine_reason_code: Option<String> = row.try_get("quarantine_reason_code")?;
            let quarantine_reason_summary: Option<String> =
                row.try_get("quarantine_reason_summary")?;
            let quarantine_reason_detail: Option<String> =
                row.try_get("quarantine_reason_detail")?;
            let quarantine_created_at: Option<i64> = row.try_get("quarantine_created_at")?;

            Ok(ApiKeyMetrics {
                id,
                status,
                group_name: normalize_optional_api_key_field(group_name),
                registration_ip: normalize_optional_api_key_field(registration_ip),
                registration_region: normalize_optional_api_key_field(registration_region),
                status_changed_at: status_changed_at.and_then(normalize_timestamp),
                last_used_at: normalize_timestamp(last_used_at),
                deleted_at: deleted_at.and_then(normalize_timestamp),
                quota_limit,
                quota_remaining,
                quota_synced_at: quota_synced_at.and_then(normalize_timestamp),
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                quarantine: quarantine_source.map(|source| ApiKeyQuarantine {
                    source,
                    reason_code: quarantine_reason_code.unwrap_or_default(),
                    reason_summary: quarantine_reason_summary.unwrap_or_default(),
                    reason_detail: quarantine_reason_detail.unwrap_or_default(),
                    created_at: quarantine_created_at.unwrap_or_default(),
                }),
            })
        })
        .transpose()
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_recent_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;

        let rows = sqlx::query(
            r#"
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
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            FROM request_logs
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let records = rows
            .into_iter()
            .map(|row| -> Result<RequestLogRecord, sqlx::Error> {
                let forwarded =
                    parse_header_list(row.try_get::<Option<String>, _>("forwarded_headers")?);
                let dropped =
                    parse_header_list(row.try_get::<Option<String>, _>("dropped_headers")?);
                let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                let response_body: Option<Vec<u8>> = row.try_get("response_body")?;
                Ok(RequestLogRecord {
                    id: row.try_get("id")?,
                    key_id: row.try_get("api_key_id")?,
                    auth_token_id: row.try_get("auth_token_id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    status_code: row.try_get("status_code")?,
                    tavily_status_code: row.try_get("tavily_status_code")?,
                    error_message: row.try_get("error_message")?,
                    result_status: row.try_get("result_status")?,
                    failure_kind: row.try_get("failure_kind")?,
                    key_effect_code: row.try_get("key_effect_code")?,
                    key_effect_summary: row.try_get("key_effect_summary")?,
                    created_at: row.try_get("created_at")?,
                    request_body: request_body.unwrap_or_default(),
                    response_body: response_body.unwrap_or_default(),
                    forwarded_headers: forwarded,
                    dropped_headers: dropped,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub(crate) async fn fetch_recent_logs_page(
        &self,
        result_status: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;

        let (rows, total) = if let Some(status) = result_status {
            let total: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*) AS count
                FROM request_logs
                WHERE result_status = ?
                "#,
            )
            .bind(status)
            .fetch_one(&self.pool)
            .await?;

            let rows = sqlx::query(
                r#"
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
                    failure_kind,
                    key_effect_code,
                    key_effect_summary,
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                WHERE result_status = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(status)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

            (rows, total)
        } else {
            let total: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*) AS count
                FROM request_logs
                "#,
            )
            .fetch_one(&self.pool)
            .await?;

            let rows = sqlx::query(
                r#"
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
                    request_body,
                    response_body,
                    forwarded_headers,
                    dropped_headers,
                    created_at
                FROM request_logs
                ORDER BY created_at DESC, id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

            (rows, total)
        };

        let records = rows
            .into_iter()
            .map(|row| -> Result<RequestLogRecord, sqlx::Error> {
                let forwarded =
                    parse_header_list(row.try_get::<Option<String>, _>("forwarded_headers")?);
                let dropped =
                    parse_header_list(row.try_get::<Option<String>, _>("dropped_headers")?);
                let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                let response_body: Option<Vec<u8>> = row.try_get("response_body")?;
                Ok(RequestLogRecord {
                    id: row.try_get("id")?,
                    key_id: row.try_get("api_key_id")?,
                    auth_token_id: row.try_get("auth_token_id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    status_code: row.try_get("status_code")?,
                    tavily_status_code: row.try_get("tavily_status_code")?,
                    error_message: row.try_get("error_message")?,
                    result_status: row.try_get("result_status")?,
                    failure_kind: row.try_get("failure_kind")?,
                    key_effect_code: row.try_get("key_effect_code")?,
                    key_effect_summary: row.try_get("key_effect_summary")?,
                    created_at: row.try_get("created_at")?,
                    request_body: request_body.unwrap_or_default(),
                    response_body: response_body.unwrap_or_default(),
                    forwarded_headers: forwarded,
                    dropped_headers: dropped,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((records, total))
    }

    pub(crate) async fn fetch_api_key_secret(
        &self,
        key_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let secret =
            sqlx::query_scalar::<_, String>("SELECT api_key FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(secret)
    }

    pub(crate) async fn fetch_api_key_id_by_secret(
        &self,
        secret: &str,
    ) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM api_keys WHERE api_key = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(secret)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_key_state_snapshot(
        &self,
        key_id: &str,
    ) -> Result<KeyStateSnapshot, ProxyError> {
        let status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT status FROM api_keys WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_quarantines
            WHERE key_id = ? AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        Ok(KeyStateSnapshot {
            status,
            quarantined,
        })
    }

    pub(crate) async fn insert_api_key_maintenance_record(
        &self,
        record: ApiKeyMaintenanceRecord,
    ) -> Result<(), ProxyError> {
        let auth_token_id = if let Some(auth_token_id) = record.auth_token_id.as_deref() {
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(auth_token_id)
                .fetch_optional(&self.pool)
                .await?
                .map(|_| auth_token_id.to_string())
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO api_key_maintenance_records (
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
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(record.id)
        .bind(record.key_id)
        .bind(record.source)
        .bind(record.operation_code)
        .bind(record.operation_summary)
        .bind(record.reason_code)
        .bind(record.reason_summary)
        .bind(record.reason_detail)
        .bind(record.request_log_id)
        .bind(record.auth_token_log_id)
        .bind(auth_token_id)
        .bind(record.actor_user_id)
        .bind(record.actor_display_name)
        .bind(record.status_before)
        .bind(record.status_after)
        .bind(i64::from(record.quarantine_before))
        .bind(i64::from(record.quarantine_after))
        .bind(record.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_quota_for_key(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        synced_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE api_keys
               SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ?
             WHERE id = ?"#,
        )
        .bind(limit)
        .bind(remaining)
        .bind(synced_at)
        .bind(key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = Utc::now().timestamp();
        let threshold = now - older_than_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
            )
            ORDER BY CASE WHEN quota_synced_at IS NULL OR quota_synced_at = 0 THEN 0 ELSE 1 END, quota_synced_at ASC
            "#,
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        let started_at = Utc::now().timestamp();
        let res = sqlx::query(
            r#"INSERT INTO scheduled_jobs (job_type, key_id, status, attempt, started_at)
               VALUES (?, ?, 'running', ?, ?)"#,
        )
        .bind(job_type)
        .bind(key_id)
        .bind(attempt)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub(crate) async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE scheduled_jobs SET status = ?, message = ?, finished_at = ? WHERE id = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            r#"SELECT
                    j.id,
                    j.job_type,
                    j.key_id,
                    k.group_name AS key_group,
                    j.status,
                    j.attempt,
                    j.message,
                    j.started_at,
                    j.finished_at
                FROM scheduled_jobs j
                LEFT JOIN api_keys k ON k.id = j.key_id
                ORDER BY j.started_at DESC, j.id DESC
                LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub(crate) async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100) as i64;
        let offset = ((page - 1) as i64).saturating_mul(per_page);

        let where_clause = match group {
            "quota" => "WHERE j.job_type = 'quota_sync' OR j.job_type = 'quota_sync/manual'",
            "usage" => "WHERE j.job_type = 'token_usage_rollup'",
            "logs" => "WHERE j.job_type = 'auth_token_logs_gc' OR j.job_type = 'request_logs_gc'",
            "geo" => "WHERE j.job_type = 'forward_proxy_geo_refresh'",
            _ => "",
        };

        let count_where_clause = match group {
            "quota" => "WHERE job_type = 'quota_sync' OR job_type = 'quota_sync/manual'",
            "usage" => "WHERE job_type = 'token_usage_rollup'",
            "logs" => "WHERE job_type = 'auth_token_logs_gc' OR job_type = 'request_logs_gc'",
            "geo" => "WHERE job_type = 'forward_proxy_geo_refresh'",
            _ => "",
        };

        let count_query = format!("SELECT COUNT(*) FROM scheduled_jobs {}", count_where_clause);
        let total: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(&self.pool)
            .await?;

        let select_query = format!(
            r#"
            SELECT
                j.id,
                j.job_type,
                j.key_id,
                k.group_name AS key_group,
                j.status,
                j.attempt,
                j.message,
                j.started_at,
                j.finished_at
            FROM scheduled_jobs j
            LEFT JOIN api_keys k ON k.id = j.key_id
            {}
            ORDER BY j.started_at DESC, j.id DESC
            LIMIT ? OFFSET ?
            "#,
            where_clause
        );

        let rows = sqlx::query(&select_query)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((items, total))
    }

    pub(crate) async fn get_meta_string(&self, key: &str) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn get_meta_i64(&self, key: &str) -> Result<Option<i64>, ProxyError> {
        let value = self.get_meta_string(key).await?;

        if let Some(v) = value {
            match v.parse::<i64>() {
                Ok(parsed) => Ok(Some(parsed)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn set_meta_string(&self, key: &str, value: &str) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_meta_i64(&self, key: &str, value: i64) -> Result<(), ProxyError> {
        let v = value.to_string();
        self.set_meta_string(key, &v).await
    }

    pub(crate) async fn fetch_summary(&self) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(total_requests), 0) AS total_requests,
                COALESCE(SUM(success_count), 0) AS success_count,
                COALESCE(SUM(error_count), 0) AS error_count,
                COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS active_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS exhausted_keys,
                COALESCE(SUM(CASE WHEN aq.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS quarantined_keys
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .fetch_one(&self.pool)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(last_used_at) FROM api_keys WHERE deleted_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?
        .and_then(normalize_timestamp);

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0) AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0) AS total_quota_remaining
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            quarantined_keys: key_counts_row.try_get("quarantined_keys")?,
            last_activity,
            total_quota_limit: quotas_row.try_get("total_quota_limit")?,
            total_quota_remaining: quotas_row.try_get("total_quota_remaining")?,
        })
    }

    pub(crate) async fn fetch_summary_windows(
        &self,
        today_start: i64,
        today_end: i64,
        yesterday_start: i64,
        yesterday_end: i64,
        month_start: i64,
    ) -> Result<SummaryWindows, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let window_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS today_total_requests,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS today_success_count,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS today_error_count,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS today_quota_exhausted_count,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS yesterday_total_requests,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS yesterday_success_count,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS yesterday_error_count,
                COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS yesterday_quota_exhausted_count
            FROM request_logs
            WHERE created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(today_start)
        .bind(today_end)
        .bind(today_start)
        .bind(today_end)
        .bind(OUTCOME_SUCCESS)
        .bind(today_start)
        .bind(today_end)
        .bind(OUTCOME_ERROR)
        .bind(today_start)
        .bind(today_end)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(OUTCOME_SUCCESS)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(OUTCOME_ERROR)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(yesterday_start)
        .bind(today_end)
        .fetch_one(&mut *tx)
        .await?;

        let month_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN bucket_start >= ? THEN total_requests ELSE 0 END), 0) AS month_total_requests,
                COALESCE(SUM(CASE WHEN bucket_start >= ? THEN success_count ELSE 0 END), 0) AS month_success_count,
                COALESCE(SUM(CASE WHEN bucket_start >= ? THEN error_count ELSE 0 END), 0) AS month_error_count,
                COALESCE(SUM(CASE WHEN bucket_start >= ? THEN quota_exhausted_count ELSE 0 END), 0) AS month_quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
              AND bucket_start >= ?
            "#,
        )
        .bind(month_start)
        .bind(month_start)
        .bind(month_start)
        .bind(month_start)
        .bind(month_start)
        .fetch_one(&mut *tx)
        .await?;

        let month_lifecycle_row = sqlx::query(
            r#"
            SELECT
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_keys
                    WHERE created_at >= ?
                ) AS month_new_keys,
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_key_quarantines
                    WHERE created_at >= ?
                ) AS month_new_quarantines
            "#,
        )
        .bind(month_start)
        .bind(month_start)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(SummaryWindows {
            today: SummaryWindowMetrics {
                total_requests: window_row.try_get("today_total_requests")?,
                success_count: window_row.try_get("today_success_count")?,
                error_count: window_row.try_get("today_error_count")?,
                quota_exhausted_count: window_row.try_get("today_quota_exhausted_count")?,
                new_keys: 0,
                new_quarantines: 0,
            },
            yesterday: SummaryWindowMetrics {
                total_requests: window_row.try_get("yesterday_total_requests")?,
                success_count: window_row.try_get("yesterday_success_count")?,
                error_count: window_row.try_get("yesterday_error_count")?,
                quota_exhausted_count: window_row.try_get("yesterday_quota_exhausted_count")?,
                new_keys: 0,
                new_quarantines: 0,
            },
            month: SummaryWindowMetrics {
                total_requests: month_row.try_get("month_total_requests")?,
                success_count: month_row.try_get("month_success_count")?,
                error_count: month_row.try_get("month_error_count")?,
                quota_exhausted_count: month_row.try_get("month_quota_exhausted_count")?,
                new_keys: month_lifecycle_row.try_get("month_new_keys")?,
                new_quarantines: month_lifecycle_row.try_get("month_new_quarantines")?,
            },
        })
    }

    pub(crate) async fn fetch_success_breakdown(
        &self,
        month_since: i64,
        day_since: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN bucket_start >= ? THEN success_count ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN bucket_start >= ? THEN success_count ELSE 0 END), 0) AS daily_success
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .bind(month_since)
        .bind(day_since)
        .fetch_one(&self.pool)
        .await?;

        Ok(SuccessBreakdown {
            monthly_success: row.try_get("monthly_success")?,
            daily_success: row.try_get("daily_success")?,
        })
    }

    pub(crate) async fn fetch_token_success_failure(
        &self,
        token_id: &str,
        month_since: i64,
        day_since: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs
            WHERE token_id = ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_since)
        .bind(OUTCOME_ERROR)
        .bind(day_since)
        .bind(token_id)
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }
}
