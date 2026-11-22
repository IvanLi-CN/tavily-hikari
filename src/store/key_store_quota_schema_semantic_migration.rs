impl KeyStore {
    async fn table_columns_set(
        &self,
        table: &str,
    ) -> Result<std::collections::HashSet<String>, ProxyError> {
        if !self.main_table_exists(table).await? {
            return Ok(std::collections::HashSet::new());
        }

        sqlx::query_scalar::<_, String>(&format!(
            "SELECT name FROM pragma_table_info('{table}', 'main')"
        ))
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().collect())
        .map_err(ProxyError::Database)
    }

    async fn rebuild_account_quota_limits_with_semantic_columns(&self) -> Result<(), ProxyError> {
        let source_columns = self.table_columns_set("account_quota_limits").await?;
        let has_legacy = source_columns.contains("hourly_any_limit")
            || source_columns.contains("hourly_limit")
            || source_columns.contains("daily_limit")
            || source_columns.contains("monthly_limit");
        let has_target = source_columns.contains("business_calls_1h_limit")
            && source_columns.contains("daily_credits_limit")
            && source_columns.contains("monthly_credits_limit");
        if source_columns.is_empty() || (!has_legacy && has_target) {
            return Ok(());
        }

        let has = |column: &str| source_columns.contains(column);
        let source = |column: &str| format!("legacy.{column}");
        let select_expr = |primary: &str, legacy: &str, default_sql: &str| {
            if has(primary) {
                source(primary)
            } else if has(legacy) {
                source(legacy)
            } else {
                default_sql.to_string()
            }
        };

        let business_calls_1h_limit =
            select_expr("business_calls_1h_limit", "hourly_limit", "0");
        let daily_credits_limit = select_expr("daily_credits_limit", "daily_limit", "0");
        let monthly_credits_limit = select_expr("monthly_credits_limit", "monthly_limit", "0");
        let monthly_broken_limit = if has("monthly_broken_limit") {
            source("monthly_broken_limit")
        } else {
            USER_MONTHLY_BROKEN_LIMIT_DEFAULT.to_string()
        };
        let monthly_blocked_key_limit_delta = if has("monthly_blocked_key_limit_delta") {
            source("monthly_blocked_key_limit_delta")
        } else if has("monthly_broken_limit") {
            format!(
                "COALESCE(legacy.monthly_broken_limit, {}) - {}",
                USER_MONTHLY_BROKEN_LIMIT_DEFAULT, USER_MONTHLY_BROKEN_LIMIT_DEFAULT
            )
        } else {
            "0".to_string()
        };
        let inherits_defaults = if has("inherits_defaults") {
            "COALESCE(legacy.inherits_defaults, 1)".to_string()
        } else {
            "1".to_string()
        };
        let created_at = if has("created_at") {
            source("created_at")
        } else {
            "legacy.updated_at".to_string()
        };
        let updated_at = if has("updated_at") {
            source("updated_at")
        } else if has("created_at") {
            source("created_at")
        } else {
            "CAST(strftime('%s', 'now') AS INTEGER)".to_string()
        };

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS account_quota_limits_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE account_quota_limits_new (
                    user_id TEXT PRIMARY KEY,
                    business_calls_1h_limit INTEGER NOT NULL,
                    daily_credits_limit INTEGER NOT NULL,
                    monthly_credits_limit INTEGER NOT NULL,
                    monthly_broken_limit INTEGER NOT NULL DEFAULT 5,
                    monthly_blocked_key_limit_delta INTEGER NOT NULL DEFAULT 0,
                    inherits_defaults INTEGER NOT NULL DEFAULT 1,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    FOREIGN KEY (user_id) REFERENCES users(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            let copy_sql = format!(
                r#"
                INSERT INTO account_quota_limits_new (
                    user_id,
                    business_calls_1h_limit,
                    daily_credits_limit,
                    monthly_credits_limit,
                    monthly_broken_limit,
                    monthly_blocked_key_limit_delta,
                    inherits_defaults,
                    created_at,
                    updated_at
                )
                SELECT
                    legacy.user_id,
                    {business_calls_1h_limit},
                    {daily_credits_limit},
                    {monthly_credits_limit},
                    {monthly_broken_limit},
                    {monthly_blocked_key_limit_delta},
                    {inherits_defaults},
                    {created_at},
                    {updated_at}
                FROM account_quota_limits AS legacy
                "#
            );
            sqlx::query(&copy_sql).execute(&mut *conn).await?;
            sqlx::query("DROP TABLE account_quota_limits")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE account_quota_limits_new RENAME TO account_quota_limits")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        if rebuild_result.is_err() {
            sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
        }
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        rebuild_result.map_err(ProxyError::Database)
    }

    async fn rebuild_user_tags_with_semantic_columns(&self) -> Result<(), ProxyError> {
        let source_columns = self.table_columns_set("user_tags").await?;
        let has_legacy = source_columns.contains("hourly_any_delta")
            || source_columns.contains("hourly_delta")
            || source_columns.contains("daily_delta")
            || source_columns.contains("monthly_delta");
        let has_target = source_columns.contains("business_calls_1h_delta")
            && source_columns.contains("daily_credits_delta")
            && source_columns.contains("monthly_credits_delta");
        if source_columns.is_empty() || (!has_legacy && has_target) {
            return Ok(());
        }

        let has = |column: &str| source_columns.contains(column);
        let source = |column: &str| format!("legacy.{column}");
        let select_expr = |primary: &str, legacy: &str| {
            if has(primary) {
                source(primary)
            } else if has(legacy) {
                source(legacy)
            } else {
                "0".to_string()
            }
        };

        let business_calls_1h_delta = select_expr("business_calls_1h_delta", "hourly_delta");
        let daily_credits_delta = select_expr("daily_credits_delta", "daily_delta");
        let monthly_credits_delta = select_expr("monthly_credits_delta", "monthly_delta");

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS user_tags_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE user_tags_new (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    display_name TEXT NOT NULL,
                    icon TEXT,
                    system_key TEXT UNIQUE,
                    effect_kind TEXT NOT NULL DEFAULT 'quota_delta',
                    business_calls_1h_delta INTEGER NOT NULL DEFAULT 0,
                    daily_credits_delta INTEGER NOT NULL DEFAULT 0,
                    monthly_credits_delta INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            let copy_sql = format!(
                r#"
                INSERT INTO user_tags_new (
                    id,
                    name,
                    display_name,
                    icon,
                    system_key,
                    effect_kind,
                    business_calls_1h_delta,
                    daily_credits_delta,
                    monthly_credits_delta,
                    created_at,
                    updated_at
                )
                SELECT
                    legacy.id,
                    legacy.name,
                    legacy.display_name,
                    legacy.icon,
                    legacy.system_key,
                    COALESCE(legacy.effect_kind, 'quota_delta'),
                    {business_calls_1h_delta},
                    {daily_credits_delta},
                    {monthly_credits_delta},
                    legacy.created_at,
                    legacy.updated_at
                FROM user_tags AS legacy
                "#
            );
            sqlx::query(&copy_sql).execute(&mut *conn).await?;
            sqlx::query("DROP TABLE user_tags").execute(&mut *conn).await?;
            sqlx::query("ALTER TABLE user_tags_new RENAME TO user_tags")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        if rebuild_result.is_err() {
            sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
        }
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        rebuild_result.map_err(ProxyError::Database)
    }

    async fn rebuild_account_quota_limit_snapshots_with_semantic_columns(
        &self,
    ) -> Result<(), ProxyError> {
        let source_columns = self.table_columns_set("account_quota_limit_snapshots").await?;
        let has_legacy = source_columns.contains("hourly_any_limit")
            || source_columns.contains("hourly_limit")
            || source_columns.contains("daily_limit")
            || source_columns.contains("monthly_limit");
        let has_target = source_columns.contains("business_calls_1h_limit")
            && source_columns.contains("daily_credits_limit")
            && source_columns.contains("monthly_credits_limit");
        if source_columns.is_empty() || (!has_legacy && has_target) {
            return Ok(());
        }

        let has = |column: &str| source_columns.contains(column);
        let source = |column: &str| format!("legacy.{column}");
        let select_expr = |primary: &str, legacy: &str| {
            if has(primary) {
                source(primary)
            } else if has(legacy) {
                source(legacy)
            } else {
                "0".to_string()
            }
        };

        let business_calls_1h_limit =
            select_expr("business_calls_1h_limit", "hourly_limit");
        let daily_credits_limit = select_expr("daily_credits_limit", "daily_limit");
        let monthly_credits_limit = select_expr("monthly_credits_limit", "monthly_limit");

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS account_quota_limit_snapshots_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE account_quota_limit_snapshots_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL,
                    changed_at INTEGER NOT NULL,
                    business_calls_1h_limit INTEGER NOT NULL,
                    daily_credits_limit INTEGER NOT NULL,
                    monthly_credits_limit INTEGER NOT NULL,
                    FOREIGN KEY (user_id) REFERENCES users(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            let copy_sql = format!(
                r#"
                INSERT INTO account_quota_limit_snapshots_new (
                    id,
                    user_id,
                    changed_at,
                    business_calls_1h_limit,
                    daily_credits_limit,
                    monthly_credits_limit
                )
                SELECT
                    legacy.id,
                    legacy.user_id,
                    legacy.changed_at,
                    {business_calls_1h_limit},
                    {daily_credits_limit},
                    {monthly_credits_limit}
                FROM account_quota_limit_snapshots AS legacy
                "#
            );
            sqlx::query(&copy_sql).execute(&mut *conn).await?;
            sqlx::query("DROP TABLE account_quota_limit_snapshots")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                "ALTER TABLE account_quota_limit_snapshots_new RENAME TO account_quota_limit_snapshots",
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        if rebuild_result.is_err() {
            sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
        }
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        rebuild_result.map_err(ProxyError::Database)
    }

    async fn migrate_quota_schema_to_semantic_columns(&self) -> Result<(), ProxyError> {
        self.rebuild_account_quota_limits_with_semantic_columns()
            .await?;
        self.rebuild_user_tags_with_semantic_columns().await?;
        self.rebuild_account_quota_limit_snapshots_with_semantic_columns()
            .await?;
        Ok(())
    }
}
