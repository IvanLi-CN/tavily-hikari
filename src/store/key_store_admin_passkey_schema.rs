impl KeyStore {
    async fn admin_passkey_credentials_have_scoped_primary_key(&self) -> Result<bool, ProxyError> {
        if !self.main_table_exists("admin_passkey_credentials").await? {
            return Ok(false);
        }

        let rows = sqlx::query("PRAGMA table_info(admin_passkey_credentials)")
            .fetch_all(&self.pool)
            .await?;
        let scope_is_first = rows.iter().any(|row| {
            row.try_get::<String, _>("name").ok().as_deref() == Some("scope_id")
                && row.try_get::<i64, _>("pk").ok() == Some(1)
        });
        let credential_is_second = rows.iter().any(|row| {
            row.try_get::<String, _>("name").ok().as_deref() == Some("credential_id")
                && row.try_get::<i64, _>("pk").ok() == Some(2)
        });
        Ok(scope_is_first && credential_is_second)
    }

    async fn rebuild_admin_passkey_tables_for_scopes(&self) -> Result<(), ProxyError> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS admin_passkey_credentials_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query("DROP TABLE IF EXISTS admin_passkey_sessions_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE admin_passkey_credentials_new (
                    scope_id TEXT NOT NULL,
                    credential_id TEXT NOT NULL,
                    passkey_json TEXT NOT NULL,
                    label TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    last_used_at INTEGER,
                    revoked_at INTEGER,
                    PRIMARY KEY (scope_id, credential_id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO admin_passkey_credentials_new
                    (scope_id, credential_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at)
                SELECT scope_id, credential_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at
                FROM admin_passkey_credentials
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                CREATE TABLE admin_passkey_sessions_new (
                    token TEXT PRIMARY KEY,
                    scope_id TEXT NOT NULL,
                    credential_id TEXT,
                    created_at INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL,
                    revoked_at INTEGER
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO admin_passkey_sessions_new
                    (token, scope_id, credential_id, created_at, expires_at, revoked_at)
                SELECT token, scope_id, credential_id, created_at, expires_at, revoked_at
                FROM admin_passkey_sessions
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE admin_passkey_sessions")
                .execute(&mut *conn)
                .await?;
            sqlx::query("DROP TABLE admin_passkey_credentials")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE admin_passkey_credentials_new RENAME TO admin_passkey_credentials")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE admin_passkey_sessions_new RENAME TO admin_passkey_sessions")
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

    pub(crate) async fn ensure_admin_passkey_schema(&self) -> Result<(), ProxyError> {
        // The PostgreSQL baseline ships the scoped passkey tables; only SQLite needs the upgrade
        // path that rewrites pre-scope primary keys.
        if self.pg_pool.is_some() {
            return Ok(());
        }
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS admin_passkey_scopes (
                scope_id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                rp_id TEXT NOT NULL,
                rp_origin TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_seen_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS admin_passkey_credentials (
                scope_id TEXT NOT NULL DEFAULT 'legacy',
                credential_id TEXT NOT NULL,
                passkey_json TEXT NOT NULL,
                label TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_used_at INTEGER,
                revoked_at INTEGER,
                PRIMARY KEY (scope_id, credential_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("admin_passkey_credentials", "scope_id")
            .await?
        {
            sqlx::query(
                "ALTER TABLE admin_passkey_credentials ADD COLUMN scope_id TEXT NOT NULL DEFAULT 'legacy'",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS admin_passkey_reset_tokens (
                token_hash TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL DEFAULT 'legacy',
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                consumed_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("admin_passkey_reset_tokens", "scope_id")
            .await?
        {
            sqlx::query(
                "ALTER TABLE admin_passkey_reset_tokens ADD COLUMN scope_id TEXT NOT NULL DEFAULT 'legacy'",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS admin_passkey_challenges (
                id TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL DEFAULT 'legacy',
                kind TEXT NOT NULL,
                reset_token TEXT,
                state_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                consumed_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("admin_passkey_challenges", "scope_id")
            .await?
        {
            sqlx::query(
                "ALTER TABLE admin_passkey_challenges ADD COLUMN scope_id TEXT NOT NULL DEFAULT 'legacy'",
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS admin_passkey_sessions (
                token TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL DEFAULT 'legacy',
                credential_id TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("admin_passkey_sessions", "scope_id")
            .await?
        {
            sqlx::query(
                "ALTER TABLE admin_passkey_sessions ADD COLUMN scope_id TEXT NOT NULL DEFAULT 'legacy'",
            )
            .execute(&self.pool)
            .await?;
        }

        if !self.admin_passkey_credentials_have_scoped_primary_key().await? {
            self.rebuild_admin_passkey_tables_for_scopes().await?;
        }

        for sql in [
            r#"CREATE INDEX IF NOT EXISTS idx_admin_passkey_credentials_scope_active
               ON admin_passkey_credentials(scope_id, revoked_at, created_at)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_admin_passkey_reset_tokens_scope_active
               ON admin_passkey_reset_tokens(scope_id, expires_at, consumed_at)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_admin_passkey_challenges_scope_expiry
               ON admin_passkey_challenges(scope_id, kind, expires_at, consumed_at)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_admin_passkey_sessions_scope_active
               ON admin_passkey_sessions(scope_id, expires_at, revoked_at)"#,
        ] {
            sqlx::query(sql).execute(&self.pool).await?;
        }
        Ok(())
    }
}
