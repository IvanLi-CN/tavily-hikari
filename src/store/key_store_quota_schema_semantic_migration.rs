/// Advisory-lock key that serializes the legacy semantic-column rebuild across instances.
/// Registered in this module because all three rebuild entry points live here.
const SEMANTIC_COLUMN_REBUILD_LOCK_KEY: &str = "core:quota_schema_semantic_columns:rebuild";

impl KeyStore {
    async fn table_columns_set(
        &self,
        table: &str,
    ) -> Result<std::collections::HashSet<String>, ProxyError> {
        if !self.main_table_exists(table).await? {
            return Ok(std::collections::HashSet::new());
        }

        sqlx::query_scalar::<_, String>(
            r#"
            SELECT column_name
              FROM information_schema.columns
             WHERE table_schema = 'core'
               AND table_name = $1
            "#,
        )
        .bind(table)
        .fetch_all(self.pg()?)
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

        let business_calls_1h_limit = select_expr("business_calls_1h_limit", "hourly_limit", "0");
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
            "EXTRACT(EPOCH FROM now())::bigint".to_string()
        };

        // PostgreSQL cannot disable foreign keys per session, so the rebuild runs as one
        // DDL transaction guarded by an advisory lock: concurrent instances serialize on
        // the lock, and a failure rolls the whole swap back.
        let mut tx = self.pg()?.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(SEMANTIC_COLUMN_REBUILD_LOCK_KEY)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS core.account_quota_limits_new")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE core.account_quota_limits_new (
                user_id text PRIMARY KEY,
                business_calls_1h_limit bigint NOT NULL,
                daily_credits_limit bigint NOT NULL,
                monthly_credits_limit bigint NOT NULL,
                monthly_broken_limit bigint NOT NULL DEFAULT 5,
                monthly_blocked_key_limit_delta bigint NOT NULL DEFAULT 0,
                inherits_defaults bigint NOT NULL DEFAULT 1,
                created_at bigint NOT NULL,
                updated_at bigint NOT NULL,
                FOREIGN KEY (user_id) REFERENCES core.users (id)
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        let copy_sql = format!(
            r#"
            INSERT INTO core.account_quota_limits_new (
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
            FROM core.account_quota_limits AS legacy
            "#
        );
        sqlx::query(&copy_sql).execute(&mut *tx).await?;
        sqlx::query("DROP TABLE core.account_quota_limits")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE core.account_quota_limits_new RENAME TO account_quota_limits")
            .execute(&mut *tx)
            .await?;
        tx.commit().await.map_err(ProxyError::Database)
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

        // `core.user_tag_bindings.tag_id` references this table, and PostgreSQL cannot
        // suspend foreign keys, so the dependent constraint is dropped and re-added inside
        // the same DDL transaction. The advisory lock serializes concurrent rebuilds.
        let mut tx = self.pg()?.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(SEMANTIC_COLUMN_REBUILD_LOCK_KEY)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS core.user_tags_new")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE core.user_tags_new (
                id text PRIMARY KEY,
                name text NOT NULL UNIQUE,
                display_name text NOT NULL,
                icon text,
                system_key text UNIQUE,
                effect_kind text NOT NULL DEFAULT 'quota_delta',
                business_calls_1h_delta bigint NOT NULL DEFAULT 0,
                daily_credits_delta bigint NOT NULL DEFAULT 0,
                monthly_credits_delta bigint NOT NULL DEFAULT 0,
                created_at bigint NOT NULL,
                updated_at bigint NOT NULL
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        let copy_sql = format!(
            r#"
            INSERT INTO core.user_tags_new (
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
            FROM core.user_tags AS legacy
            "#
        );
        sqlx::query(&copy_sql).execute(&mut *tx).await?;
        sqlx::query(
            "ALTER TABLE core.user_tag_bindings \
             DROP CONSTRAINT IF EXISTS user_tag_bindings_tag_id_fkey",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE core.user_tags")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE core.user_tags_new RENAME TO user_tags")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "ALTER TABLE core.user_tag_bindings \
             ADD CONSTRAINT user_tag_bindings_tag_id_fkey \
             FOREIGN KEY (tag_id) REFERENCES core.user_tags (id)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(ProxyError::Database)
    }

    async fn rebuild_account_quota_limit_snapshots_with_semantic_columns(
        &self,
    ) -> Result<(), ProxyError> {
        let source_columns = self
            .table_columns_set("account_quota_limit_snapshots")
            .await?;
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

        let business_calls_1h_limit = select_expr("business_calls_1h_limit", "hourly_limit");
        let daily_credits_limit = select_expr("daily_credits_limit", "daily_limit");
        let monthly_credits_limit = select_expr("monthly_credits_limit", "monthly_limit");

        let mut tx = self.pg()?.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(SEMANTIC_COLUMN_REBUILD_LOCK_KEY)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS core.account_quota_limit_snapshots_new")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE core.account_quota_limit_snapshots_new (
                id bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
                user_id text NOT NULL,
                changed_at bigint NOT NULL,
                business_calls_1h_limit bigint NOT NULL,
                daily_credits_limit bigint NOT NULL,
                monthly_credits_limit bigint NOT NULL,
                FOREIGN KEY (user_id) REFERENCES core.users (id)
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        let copy_sql = format!(
            r#"
            INSERT INTO core.account_quota_limit_snapshots_new (
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
            FROM core.account_quota_limit_snapshots AS legacy
            "#
        );
        sqlx::query(&copy_sql).execute(&mut *tx).await?;
        sqlx::query("DROP TABLE core.account_quota_limit_snapshots")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "ALTER TABLE core.account_quota_limit_snapshots_new RENAME TO account_quota_limit_snapshots",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            SELECT setval(
                pg_get_serial_sequence('core.account_quota_limit_snapshots', 'id'),
                COALESCE((SELECT MAX(id) FROM core.account_quota_limit_snapshots), 0) + 1,
                false
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(ProxyError::Database)
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
