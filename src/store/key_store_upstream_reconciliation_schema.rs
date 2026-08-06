impl KeyStore {
    async fn ensure_upstream_reconciliation_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_reconciliation_usage (
                token_id TEXT NOT NULL,
                key_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                project_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                settlement_mode TEXT NOT NULL DEFAULT 'actual',
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                request_count INTEGER NOT NULL DEFAULT 0,
                first_used_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (token_id, key_id, period_code)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_reconciliation_work (
                work_key TEXT PRIMARY KEY,
                token_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                project_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                settlement_mode TEXT NOT NULL,
                scheduling_key_id TEXT NOT NULL,
                fair_rank INTEGER NOT NULL DEFAULT 0,
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'ready',
                next_attempt_at INTEGER NOT NULL DEFAULT 0,
                reservation_id TEXT,
                reservation_expires_at INTEGER,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                last_error_kind TEXT,
                hydration_cursor_key_id TEXT,
                upstream_usage_total INTEGER NOT NULL DEFAULT 0,
                hydration_complete INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_reconciliation_cursors (
                lane TEXT PRIMARY KEY,
                fair_rank INTEGER NOT NULL DEFAULT 0,
                scheduling_key_id TEXT NOT NULL DEFAULT '',
                period_end INTEGER NOT NULL,
                token_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        for (column, definition) in [
            ("fair_rank", "INTEGER NOT NULL DEFAULT 0"),
            ("hydration_cursor_key_id", "TEXT"),
            ("upstream_usage_total", "INTEGER NOT NULL DEFAULT 0"),
            ("hydration_complete", "INTEGER NOT NULL DEFAULT 0"),
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
        for (column, definition) in [
            ("fair_rank", "INTEGER NOT NULL DEFAULT 0"),
            ("scheduling_key_id", "TEXT NOT NULL DEFAULT ''"),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_cursors", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE upstream_reconciliation_cursors ADD COLUMN {column} {definition}",
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_reconciliation_research (
                request_id TEXT PRIMARY KEY,
                token_id TEXT NOT NULL,
                key_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                terminal_at INTEGER,
                last_polled_at INTEGER,
                next_poll_at INTEGER NOT NULL DEFAULT 0,
                poll_attempt_count INTEGER NOT NULL DEFAULT 0,
                last_poll_outcome TEXT,
                last_poll_error_kind TEXT,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_reconciliation_settlements (
                settlement_key TEXT PRIMARY KEY,
                token_id TEXT NOT NULL,
                period_code TEXT NOT NULL,
                project_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                status TEXT NOT NULL,
                upstream_usage INTEGER,
                local_billed_credits INTEGER,
                delta_credits INTEGER,
                degraded_reason TEXT,
                next_attempt_at INTEGER,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                settled_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS billing_reconciliation_adjustments (
                settlement_key TEXT PRIMARY KEY,
                token_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                period_code TEXT NOT NULL,
                delta_credits INTEGER NOT NULL,
                attributed_at INTEGER NOT NULL,
                degraded_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS billing_reconciliation_shadow_adjustments (
                settlement_key TEXT PRIMARY KEY,
                token_id TEXT NOT NULL,
                billing_subject TEXT NOT NULL,
                period_code TEXT NOT NULL,
                delta_credits INTEGER NOT NULL,
                attributed_at INTEGER NOT NULL,
                degraded_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upstream_usage_rate_attempts (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                attempted_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_column_exists("upstream_reconciliation_usage", "settlement_mode")
            .await?
        {
            sqlx::query(
                "ALTER TABLE upstream_reconciliation_usage ADD COLUMN settlement_mode TEXT NOT NULL DEFAULT 'actual'",
            )
            .execute(&self.pool)
            .await?;
        }
        for (column, definition) in [
            ("last_polled_at", "INTEGER"),
            ("next_poll_at", "INTEGER NOT NULL DEFAULT 0"),
            ("poll_attempt_count", "INTEGER NOT NULL DEFAULT 0"),
            ("last_poll_outcome", "TEXT"),
            ("last_poll_error_kind", "TEXT"),
        ] {
            if !self
                .table_column_exists("upstream_reconciliation_research", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE upstream_reconciliation_research ADD COLUMN {column} {definition}",
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_usage_period ON upstream_reconciliation_usage(period_end, token_id, period_code)").execute(&self.pool).await?;
        for statement in [
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_usage_subject_mode_period ON upstream_reconciliation_usage(billing_subject, settlement_mode, period_start, token_id, period_code)",
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_usage_window_mode ON upstream_reconciliation_usage(token_id, period_code, billing_subject, settlement_mode)",
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_work_ready ON upstream_reconciliation_work(status, next_attempt_at, period_end, token_id, period_code)",
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_work_fair ON upstream_reconciliation_work(status, next_attempt_at, fair_rank, scheduling_key_id, period_end, token_id, period_code)",
            "CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_cursor_updated ON upstream_reconciliation_cursors(updated_at, lane)",
        ] {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_upstream_reconciliation_work_usage_insert
            AFTER INSERT ON upstream_reconciliation_usage
            BEGIN
                INSERT INTO upstream_reconciliation_work (
                    work_key, token_id, period_code, project_id, billing_subject,
                    settlement_mode, scheduling_key_id, fair_rank, period_start, period_end,
                    status, next_attempt_at, created_at, updated_at
                ) VALUES (
                    'v1:' || NEW.token_id || ':' || NEW.period_code,
                    NEW.token_id, NEW.period_code, NEW.project_id, NEW.billing_subject,
                    NEW.settlement_mode, NEW.key_id,
                    (SELECT COUNT(*) + 1 FROM upstream_reconciliation_work
                     WHERE scheduling_key_id = NEW.key_id),
                    NEW.period_start, NEW.period_end,
                    'ready', 0, NEW.updated_at, NEW.updated_at
                )
                ON CONFLICT(work_key) DO UPDATE SET
                    project_id = excluded.project_id,
                    billing_subject = excluded.billing_subject,
                    settlement_mode = excluded.settlement_mode,
                    scheduling_key_id = CASE
                        WHEN excluded.scheduling_key_id < upstream_reconciliation_work.scheduling_key_id
                        THEN excluded.scheduling_key_id
                        ELSE upstream_reconciliation_work.scheduling_key_id
                    END,
                    fair_rank = CASE
                        WHEN excluded.scheduling_key_id < upstream_reconciliation_work.scheduling_key_id
                        THEN (SELECT COUNT(*) + 1 FROM upstream_reconciliation_work
                              WHERE scheduling_key_id = excluded.scheduling_key_id)
                        ELSE upstream_reconciliation_work.fair_rank
                    END,
                    period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                    period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                    updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at)
                ;
            END
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS trg_upstream_reconciliation_work_usage_update
            AFTER UPDATE OF token_id, key_id, period_code, project_id, billing_subject,
                settlement_mode, period_start, period_end, updated_at
                ON upstream_reconciliation_usage
            BEGIN
                INSERT INTO upstream_reconciliation_work (
                    work_key, token_id, period_code, project_id, billing_subject,
                    settlement_mode, scheduling_key_id, fair_rank, period_start, period_end,
                    status, next_attempt_at, created_at, updated_at
                ) VALUES (
                    'v1:' || NEW.token_id || ':' || NEW.period_code,
                    NEW.token_id, NEW.period_code, NEW.project_id, NEW.billing_subject,
                    NEW.settlement_mode, NEW.key_id,
                    (SELECT COUNT(*) + 1 FROM upstream_reconciliation_work
                     WHERE scheduling_key_id = NEW.key_id),
                    NEW.period_start, NEW.period_end,
                    'ready', 0, NEW.updated_at, NEW.updated_at
                )
                ON CONFLICT(work_key) DO UPDATE SET
                    project_id = excluded.project_id,
                    billing_subject = excluded.billing_subject,
                    settlement_mode = excluded.settlement_mode,
                    scheduling_key_id = CASE
                        WHEN excluded.scheduling_key_id < upstream_reconciliation_work.scheduling_key_id
                        THEN excluded.scheduling_key_id
                        ELSE upstream_reconciliation_work.scheduling_key_id
                    END,
                    fair_rank = CASE
                        WHEN excluded.scheduling_key_id < upstream_reconciliation_work.scheduling_key_id
                        THEN (SELECT COUNT(*) + 1 FROM upstream_reconciliation_work
                              WHERE scheduling_key_id = excluded.scheduling_key_id)
                        ELSE upstream_reconciliation_work.fair_rank
                    END,
                    period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                    period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                    updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at)
                ;
            END
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_research_period
               ON upstream_reconciliation_research(token_id, period_code, terminal_at)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_research_poll
               ON upstream_reconciliation_research(terminal_at, next_poll_at, key_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_upstream_reconciliation_settlement_status
               ON upstream_reconciliation_settlements(status, next_attempt_at, period_end)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_upstream_usage_rate_attempts_key_time
               ON upstream_usage_rate_attempts(key_id, attempted_at)"#,
        )
        .execute(&self.pool)
        .await?;
        self.repair_upstream_reconciliation_fair_ranks().await?;
        self.backfill_upstream_reconciliation_work_page().await?;
        Ok(())
    }

    async fn repair_upstream_reconciliation_fair_ranks(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            WITH ranked AS (
                SELECT
                    work_key,
                    ROW_NUMBER() OVER (
                        PARTITION BY scheduling_key_id
                        ORDER BY period_end ASC, token_id ASC, period_code ASC
                    ) AS fair_rank
                FROM upstream_reconciliation_work
            )
            UPDATE upstream_reconciliation_work
            SET fair_rank = (
                SELECT ranked.fair_rank
                FROM ranked
                WHERE ranked.work_key = upstream_reconciliation_work.work_key
            )
            WHERE fair_rank = 0
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn backfill_upstream_reconciliation_work_page(
        &self,
    ) -> Result<i64, ProxyError> {
        let cursor = sqlx::query_as::<_, (i64, String, String)>(
            r#"
            SELECT period_end, token_id, period_code
            FROM upstream_reconciliation_cursors
            WHERE lane = 'backfill'
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT
                u.token_id,
                u.period_code,
                MIN(u.project_id) AS project_id,
                MIN(u.billing_subject) AS billing_subject,
                MIN(u.settlement_mode) AS settlement_mode,
                MIN(u.key_id) AS scheduling_key_id,
                ROW_NUMBER() OVER (
                    PARTITION BY MIN(u.key_id)
                    ORDER BY MAX(u.period_end) ASC, u.token_id ASC, u.period_code ASC
                ) AS fair_rank,
                MIN(u.period_start) AS period_start,
                MAX(u.period_end) AS period_end,
                MIN(u.updated_at) AS created_at,
                MAX(u.updated_at) AS updated_at
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_work w
              ON w.work_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE w.work_key IS NULL
            "#,
        );
        if let Some((period_end, token_id, period_code)) = cursor.as_ref() {
            query
                .push(" GROUP BY u.token_id, u.period_code HAVING (MAX(u.period_end) > ")
                .push_bind(*period_end)
                .push(" OR (MAX(u.period_end) = ")
                .push_bind(*period_end)
                .push(" AND (u.token_id > ")
                .push_bind(token_id)
                .push(" OR (u.token_id = ")
                .push_bind(token_id)
                .push(" AND u.period_code > ")
                .push_bind(period_code)
                .push("))))");
        } else {
            query.push(" GROUP BY u.token_id, u.period_code");
        }
        let rows = query
            .push(" ORDER BY MAX(u.period_end) ASC, u.token_id ASC, u.period_code ASC LIMIT ")
            .push_bind(256_i64)
            .build_query_as::<(
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                i64,
                i64,
                i64,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;
        let Some(last) = rows.last() else {
            return Ok(0);
        };
        let mut insert = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            INSERT INTO upstream_reconciliation_work (
                work_key, token_id, period_code, project_id, billing_subject,
                settlement_mode, scheduling_key_id, fair_rank, period_start, period_end,
                status, next_attempt_at, created_at, updated_at
            ) VALUES "#,
        );
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                insert.push(", ");
            }
            insert
                .push("(")
                .push_bind(format!("v1:{}:{}", row.0, row.1))
                .push(", ")
                .push_bind(&row.0)
                .push(", ")
                .push_bind(&row.1)
                .push(", ")
                .push_bind(&row.2)
                .push(", ")
                .push_bind(&row.3)
                .push(", ")
                .push_bind(&row.4)
                .push(", ")
                .push_bind(&row.5)
                .push(", (SELECT COUNT(*) + 1 FROM upstream_reconciliation_work existing
                            WHERE existing.scheduling_key_id = ")
                .push_bind(&row.5)
                .push(") + ")
                .push_bind(row.6)
                .push(", ")
                .push_bind(row.7)
                .push(", ")
                .push_bind(row.8)
                .push(", 'ready', 0, ")
                .push_bind(row.9)
                .push(", ")
                .push_bind(row.10)
                .push(")");
        }
        insert
            .push(
                r#"
            ON CONFLICT(work_key) DO NOTHING
            "#,
            )
            .build()
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_cursors (
                lane, fair_rank, scheduling_key_id, period_end, token_id, period_code, updated_at
            ) VALUES ('backfill', 0, '', ?, ?, ?, ?)
            ON CONFLICT(lane) DO UPDATE SET
                period_end = excluded.period_end,
                token_id = excluded.token_id,
                period_code = excluded.period_code,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(last.8)
        .bind(&last.0)
        .bind(&last.1)
        .bind(last.10)
        .execute(&self.pool)
        .await?;
        Ok(rows.len() as i64)
    }
}
