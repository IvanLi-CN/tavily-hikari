impl KeyStore {
    const SCHEDULED_JOB_PRIORITY_AGING_SECS: i64 = 5 * 60;
    const SCHEDULED_JOB_PRIORITY_AGING_FLOOR: i64 = 2;
    fn is_scheduled_job_active_identity_conflict(err: &ProxyError) -> bool {
        let ProxyError::Database(sqlx::Error::Database(db_err)) = err else {
            return false;
        };
        let message = db_err.message();
        message.contains("idx_scheduled_jobs_active_identity")
            || message.contains("scheduled_jobs.job_type")
    }

    fn scheduled_job_stale_group(job_type: &str) -> Option<&'static str> {
        match job_type {
            "quota_sync" | "quota_sync/manual" => Some("quota_sync"),
            "quota_sync/hot" => Some("quota_sync/hot"),
            _ => None,
        }
    }

    fn scheduled_job_priority(job_type: &str, trigger_source: &str) -> i64 {
        match (trigger_source, job_type) {
            ("manual", "request_logs_gc" | "db_compaction") => 0,
            ("manual", _) => 1,
            (_, "request_logs_gc" | "db_compaction") => 2,
            (
                _,
                "auth_token_logs_gc"
                | "ha_outbox_gc"
                | "mcp_sessions_gc"
                | "mcp_session_init_backoffs_gc"
                | "token_usage_rollup"
                | "upstream_reconciliation"
                | "upstream_reconciliation_research_drain"
                | "usage_aggregation",
            ) => 3,
            (
                _,
                "linuxdo_user_tag_binding_refresh"
                | "forward_proxy_geo_refresh"
                | "linuxdo_credit_recharge_lifecycle"
                | "linuxdo_user_status_sync",
            ) => 4,
            (_, "quota_sync" | "quota_sync/manual" | "quota_sync/hot") => 5,
            _ => 6,
        }
    }

    fn should_promote_scheduled_job_trigger_source(
        job_type: &str,
        current_trigger_source: &str,
        next_trigger_source: &str,
    ) -> bool {
        Self::scheduled_job_priority(job_type, next_trigger_source)
            < Self::scheduled_job_priority(job_type, current_trigger_source)
    }

    fn scheduled_job_priority_sql(job_type_column: &str, trigger_source_column: &str) -> String {
        format!(
            "CASE \
                WHEN {trigger_source_column} = 'manual' AND ({job_type_column} = 'request_logs_gc' OR {job_type_column} = 'db_compaction') THEN 0 \
                WHEN {trigger_source_column} = 'manual' THEN 1 \
                WHEN {job_type_column} = 'request_logs_gc' OR {job_type_column} = 'db_compaction' THEN 2 \
                WHEN {job_type_column} = 'auth_token_logs_gc' OR {job_type_column} = 'ha_outbox_gc' OR {job_type_column} = 'mcp_sessions_gc' OR {job_type_column} = 'mcp_session_init_backoffs_gc' OR {job_type_column} = 'token_usage_rollup' OR {job_type_column} = 'upstream_reconciliation' OR {job_type_column} = 'upstream_reconciliation_research_drain' OR {job_type_column} = 'usage_aggregation' THEN 3 \
                WHEN {job_type_column} = 'linuxdo_user_tag_binding_refresh' OR {job_type_column} = 'forward_proxy_geo_refresh' OR {job_type_column} = 'linuxdo_credit_recharge_lifecycle' OR {job_type_column} = 'linuxdo_user_status_sync' THEN 4 \
                WHEN {job_type_column} = 'quota_sync' OR {job_type_column} = 'quota_sync/manual' OR {job_type_column} = 'quota_sync/hot' THEN 5 \
                ELSE 6 \
            END"
        )
    }

    fn scheduled_job_effective_priority_sql(
        job_type_column: &str,
        trigger_source_column: &str,
        queued_at_column: &str,
    ) -> String {
        let priority_sql = Self::scheduled_job_priority_sql(job_type_column, trigger_source_column);
        format!(
            "CASE WHEN {trigger_source_column} = 'manual' THEN ({priority_sql}) \
             ELSE MAX({}, ({priority_sql}) - MAX(0, (? - {queued_at_column}) / {})) END",
            Self::SCHEDULED_JOB_PRIORITY_AGING_FLOOR,
            Self::SCHEDULED_JOB_PRIORITY_AGING_SECS,
        )
    }

    async fn create_scheduled_jobs_indexes(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_recent
            ON scheduled_jobs(COALESCE(started_at, queued_at) DESC, id DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_queue_available
            ON scheduled_jobs(status, available_at ASC, queued_at ASC, id ASC)
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_jobs_active_identity
            ON scheduled_jobs(job_type, IFNULL(key_id, ''))
            WHERE status = 'queued' OR status = 'running'
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_scheduled_jobs_indexes_on_conn(
        conn: &mut sqlx::SqliteConnection,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_recent
            ON scheduled_jobs(COALESCE(started_at, queued_at) DESC, id DESC)
            "#,
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_queue_available
            ON scheduled_jobs(status, available_at ASC, queued_at ASC, id ASC)
            "#,
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_jobs_active_identity
            ON scheduled_jobs(job_type, IFNULL(key_id, ''))
            WHERE status = 'queued' OR status = 'running'
            "#,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    pub(crate) async fn ensure_scheduled_jobs_queue_schema(&self) -> Result<(), ProxyError> {
        if !self.table_column_exists("scheduled_jobs", "queued_at").await? {
            self.rebuild_scheduled_jobs_table().await?;
        }
        if !self.table_column_exists("scheduled_jobs", "available_at").await? {
            sqlx::query(
                "ALTER TABLE scheduled_jobs ADD COLUMN available_at INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self.table_column_exists("scheduled_jobs", "claim_generation").await? {
            sqlx::query(
                "ALTER TABLE scheduled_jobs ADD COLUMN claim_generation INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        self.create_scheduled_jobs_indexes().await
    }

    async fn rebuild_scheduled_jobs_table(&self) -> Result<(), ProxyError> {
        let mut raw_conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *raw_conn)
            .await?;
        let mut conn = ImmediateSqliteTransaction::begin(raw_conn).await?;

        let rebuild_result = async {
            sqlx::query("DROP TABLE IF EXISTS scheduled_jobs_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE scheduled_jobs_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    job_type TEXT NOT NULL,
                    trigger_source TEXT NOT NULL DEFAULT 'scheduler',
                    key_id TEXT,
                    status TEXT NOT NULL,
                    attempt INTEGER NOT NULL DEFAULT 1,
                    message TEXT,
                    queued_at INTEGER NOT NULL,
                    available_at INTEGER NOT NULL DEFAULT 0,
                    claim_generation INTEGER NOT NULL DEFAULT 0,
                    started_at INTEGER,
                    finished_at INTEGER,
                    FOREIGN KEY (key_id) REFERENCES api_keys(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO scheduled_jobs_new (
                    id,
                    job_type,
                    trigger_source,
                    key_id,
                    status,
                    attempt,
                    message,
                    queued_at,
                    available_at,
                    claim_generation,
                    started_at,
                    finished_at
                )
                SELECT
                    id,
                    job_type,
                    COALESCE(trigger_source, 'scheduler'),
                    key_id,
                    status,
                    attempt,
                    message,
                    started_at,
                    0,
                    0,
                    started_at,
                    finished_at
                FROM scheduled_jobs
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE scheduled_jobs")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE scheduled_jobs_new RENAME TO scheduled_jobs")
                .execute(&mut *conn)
                .await?;
            Self::create_scheduled_jobs_indexes_on_conn(&mut conn).await?;

            let foreign_key_check: Vec<(String, i64, String, i64)> =
                sqlx::query_as("PRAGMA foreign_key_check(scheduled_jobs)")
                    .fetch_all(&mut *conn)
                    .await?;
            if !foreign_key_check.is_empty() {
                return Err(ProxyError::Other(
                    "scheduled_jobs rebuild failed foreign_key_check".to_string(),
                ));
            }

            Ok::<(), ProxyError>(())
        }
        .await;

        match rebuild_result {
            Ok(()) => {
                let mut raw_conn = conn.commit_connection().await?;
                sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(&mut *raw_conn)
                    .await?;
                Ok(())
            }
            Err(err) => {
                let _ = conn.rollback().await;
                Err(err)
            }
        }
    }

    fn sqlite_wal_path(&self) -> String {
        format!("{}-wal", self.database_path)
    }

    pub(crate) async fn sqlite_db_stats(&self) -> Result<SqliteDbStats, ProxyError> {
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?;
        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await?;
        let freelist_count: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.pool)
            .await?;
        let database_bytes = std::fs::metadata(&self.database_path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        let wal_bytes = std::fs::metadata(self.sqlite_wal_path())
            .map(|meta| meta.len())
            .unwrap_or(0);
        let reclaimable_bytes = freelist_count.max(0) as u64 * page_size.max(0) as u64;
        let total_pages = page_count.max(1) as f64;
        Ok(SqliteDbStats {
            database_bytes,
            wal_bytes,
            page_size,
            page_count,
            freelist_count,
            reclaimable_bytes,
            reclaimable_ratio: freelist_count.max(0) as f64 / total_pages,
        })
    }

    pub(crate) async fn compact_sqlite_database(&self) -> Result<SqliteDbStats, ProxyError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        sqlx::query("VACUUM").execute(&self.pool).await?;
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        self.sqlite_db_stats().await
    }

    pub(crate) async fn checkpoint_sqlite_wal_passive(&self) -> Result<(i64, i64, i64), ProxyError> {
        sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)")
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        self.scheduled_job_start_with_source(job_type, "scheduler", key_id, attempt)
            .await
    }

    pub(crate) async fn scheduled_job_start_with_source(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        let started_at = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            if let Some((job_id, status, _current_trigger_source)) =
                Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id).await?
                && status == "running"
            {
                return Ok::<i64, ProxyError>(job_id);
            }

            let res = sqlx::query(
                r#"
                INSERT INTO scheduled_jobs (
                    job_type,
                    trigger_source,
                    key_id,
                    status,
                    attempt,
                    queued_at,
                    available_at,
                    started_at
                )
                VALUES (?, ?, ?, 'running', ?, ?, ?, ?)
                "#,
            )
            .bind(job_type)
            .bind(trigger_source)
            .bind(key_id)
            .bind(attempt)
            .bind(started_at)
            .bind(started_at)
            .bind(started_at)
            .execute(&mut *conn)
            .await?;
            Ok(res.last_insert_rowid())
        }
        .await;
        match result {
            Ok(job_id) => {
                conn.finish(Ok(())).await?;
                Ok(job_id)
            }
            Err(err) if Self::is_scheduled_job_active_identity_conflict(&err) => {
                let original_error = match conn.finish(Err(err)).await {
                    Err(err) => err,
                    Ok(()) => unreachable!("failed scheduled job transaction committed"),
                };
                if let Some((job_id, _status, _current_trigger_source)) =
                    self.scheduled_job_lookup_active(
                        job_type,
                        key_id,
                        SqliteOperation::ScheduledJobControl,
                    )
                    .await?
                {
                    Ok(job_id)
                } else {
                    Err(original_error)
                }
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed scheduled job transaction committed"),
            },
        }
    }

    async fn abandon_stale_quota_sync_job_locked(
        conn: &mut sqlx::SqliteConnection,
        job_type: &str,
        key_id: Option<&str>,
        now: i64,
    ) -> Result<(), ProxyError> {
        let stale_before = now.saturating_sub(QUOTA_SYNC_STALE_RUNNING_SECS);
        let Some(stale_group) = Self::scheduled_job_stale_group(job_type) else {
            return Ok(());
        };
        sqlx::query(
            r#"
            UPDATE scheduled_jobs
            SET status = 'abandoned',
                message = COALESCE(message, 'abandoned after quota_sync timeout window'),
                finished_at = ?
            WHERE status = 'running'
              AND finished_at IS NULL
              AND started_at IS NOT NULL
              AND started_at <= ?
              AND (
                    ((job_type = 'quota_sync' OR job_type = 'quota_sync/manual') AND ? = 'quota_sync')
                    OR (job_type = ? AND ? = 'quota_sync/hot')
                  )
              AND ((key_id IS NULL AND ? IS NULL) OR key_id = ?)
            "#,
        )
        .bind(now)
        .bind(stale_before)
        .bind(stale_group)
        .bind(stale_group)
        .bind(stale_group)
        .bind(key_id)
        .bind(key_id)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    async fn scheduled_job_lookup_active_locked(
        conn: &mut sqlx::SqliteConnection,
        job_type: &str,
        key_id: Option<&str>,
    ) -> Result<Option<(i64, String, String)>, ProxyError> {
        sqlx::query_as::<_, (i64, String, String)>(
            r#"
            SELECT id, status, trigger_source
            FROM scheduled_jobs
            WHERE job_type = ?
              AND (status = 'queued' OR status = 'running')
              AND ((key_id IS NULL AND ? IS NULL) OR key_id = ?)
            ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, COALESCE(started_at, queued_at) DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(job_type)
        .bind(key_id)
        .bind(key_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(ProxyError::from)
    }

    async fn scheduled_job_lookup_active(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        operation: SqliteOperation,
    ) -> Result<Option<(i64, String, String)>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(operation)
            .await?;
        let lookup = sqlx::query_as::<_, (i64, String, String)>(
            r#"
            SELECT id, status, trigger_source
            FROM scheduled_jobs
            WHERE job_type = ?
              AND (status = 'queued' OR status = 'running')
              AND ((key_id IS NULL AND ? IS NULL) OR key_id = ?)
            ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, COALESCE(started_at, queued_at) DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(job_type)
        .bind(key_id)
        .bind(key_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(ProxyError::from);
        let close = conn.close().await;
        match (lookup, close) {
            (Ok(row), Ok(())) => Ok(row),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn scheduled_job_enqueue(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        let available_at = self.backend_time.now_ts();
        self.scheduled_job_enqueue_at_with_operation(
            job_type,
            trigger_source,
            key_id,
            attempt,
            available_at,
            SqliteOperation::ScheduledJobControl,
        )
            .await
    }

    pub(crate) async fn scheduled_job_enqueue_foreground(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        let available_at = self.backend_time.now_ts();
        self.scheduled_job_enqueue_at_with_operation(
            job_type,
            trigger_source,
            key_id,
            attempt,
            available_at,
            SqliteOperation::ForegroundJobTrigger,
        )
        .await
    }

    pub(crate) async fn scheduled_job_enqueue_at(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.scheduled_job_enqueue_at_with_operation(
            job_type,
            trigger_source,
            key_id,
            attempt,
            available_at,
            SqliteOperation::ScheduledJobControl,
        )
        .await
    }

    async fn scheduled_job_enqueue_at_with_operation(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
        available_at: i64,
        operation: SqliteOperation,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        // Fast-path repeated coalesce reads so owner-facing manual triggers do not
        // fail behind an unrelated long-lived write window.
        let active_representative = if Self::scheduled_job_stale_group(job_type).is_none() {
            self.scheduled_job_lookup_active(job_type, key_id, operation)
                .await?
        } else {
            None
        };
        if let Some((job_id, status, current_trigger_source)) = active_representative.as_ref() {
            let promoted = Self::should_promote_scheduled_job_trigger_source(
                job_type,
                current_trigger_source,
                trigger_source,
            );
            if !promoted && (trigger_source != "manual" || status != "queued") {
                return Ok(ScheduledJobEnqueueResult {
                    job_id: *job_id,
                    created: false,
                    promoted: false,
                    status: status.clone(),
                    trigger_source: current_trigger_source.clone(),
                });
            }
        }

        let queued_at = self.backend_time.now_ts();
        let mut conn = match self
            .sqlite_runtime
            .begin_immediate(operation)
            .await
        {
            Ok(conn) => conn,
            Err(err) if crate::is_transient_sqlite_write_error(&err) => {
                if let Some((job_id, status, trigger_source)) = active_representative {
                    return Ok(ScheduledJobEnqueueResult {
                        job_id,
                        created: false,
                        promoted: false,
                        status,
                        trigger_source,
                    });
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        let result = async {
            Self::abandon_stale_quota_sync_job_locked(&mut conn, job_type, key_id, queued_at)
                .await?;
            if let Some((job_id, status, current_trigger_source)) =
                Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id).await?
            {
                let promoted = Self::should_promote_scheduled_job_trigger_source(
                    job_type,
                    &current_trigger_source,
                    trigger_source,
                );
                if promoted || (trigger_source == "manual" && status == "queued") {
                    sqlx::query(
                        r#"UPDATE scheduled_jobs
                           SET trigger_source = CASE WHEN ? THEN ? ELSE trigger_source END,
                               available_at = CASE WHEN status = 'queued' THEN MIN(available_at, ?) ELSE available_at END
                           WHERE id = ?"#,
                    )
                    .bind(promoted)
                    .bind(trigger_source)
                    .bind(queued_at)
                    .bind(job_id)
                    .execute(&mut *conn)
                    .await?;
                }
                let effective_trigger_source = if promoted {
                    trigger_source.to_string()
                } else {
                    current_trigger_source
                };
                return Ok::<ScheduledJobEnqueueResult, ProxyError>(ScheduledJobEnqueueResult {
                    job_id,
                    created: false,
                    promoted,
                    status,
                    trigger_source: effective_trigger_source,
                });
            }

            let res = sqlx::query(
                r#"
                INSERT INTO scheduled_jobs (
                    job_type,
                    trigger_source,
                    key_id,
                    status,
                    attempt,
                    queued_at,
                    available_at,
                    started_at,
                    finished_at
                )
                VALUES (?, ?, ?, 'queued', ?, ?, ?, NULL, NULL)
                "#,
            )
            .bind(job_type)
            .bind(trigger_source)
            .bind(key_id)
            .bind(attempt)
            .bind(queued_at)
            .bind(available_at)
            .execute(&mut *conn)
            .await?;
            Ok(ScheduledJobEnqueueResult {
                job_id: res.last_insert_rowid(),
                created: true,
                promoted: false,
                status: "queued".to_string(),
                trigger_source: trigger_source.to_string(),
            })
        }
        .await;
        match result {
            Ok(outcome) => {
                conn.finish(Ok(())).await?;
                Ok(outcome)
            }
            Err(err) if crate::is_transient_sqlite_write_error(&err) => {
                let original_error = match conn.finish(Err(err)).await {
                    Err(err) => err,
                    Ok(()) => unreachable!("failed control transaction committed"),
                };
                // The active representative is already durable. Under a short
                // control write budget, returning it is preferable to turning
                // a coalesced manual wake into a foreground 500 solely to
                // promote metadata that the next unlocked scheduler pass can
                // update safely.
                if let Some((job_id, status, current_trigger_source)) =
                    self.scheduled_job_lookup_active(job_type, key_id, operation).await?
                {
                    return Ok(ScheduledJobEnqueueResult {
                        job_id,
                        created: false,
                        promoted: false,
                        status,
                        trigger_source: current_trigger_source,
                    });
                }
                Err(original_error)
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed scheduled job transaction committed"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn scheduled_job_finish_and_enqueue_auto_at(
        &self,
        job_id: i64,
        claim_generation: i64,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
        message: Option<&str>,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.scheduled_job_finish_and_enqueue_auto_at_with_status(
            job_id,
            claim_generation,
            "success",
            job_type,
            key_id,
            attempt,
            message,
            available_at,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn scheduled_job_finish_and_enqueue_auto_at_with_status(
        &self,
        job_id: i64,
        claim_generation: i64,
        status: &str,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
        message: Option<&str>,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        let finished_at = self.backend_time.now_ts();
        let preserve_research_wait_anchor = job_type == "upstream_reconciliation_research_drain"
            && message
                .and_then(crate::ResearchDrainDeferReason::from_scheduled_job_message)
                .is_some_and(crate::ResearchDrainDeferReason::preserves_research_wait_anchor);
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            // `queued_at` is the durable fairness anchor for a Research drain
            // that is otherwise ready but temporarily yields to foreground work
            // or the request lease. Key cooldowns and accepted polls do not use
            // this path, so their next eligibility begins a new wait interval.
            let continuation_queued_at = if preserve_research_wait_anchor {
                sqlx::query_scalar(
                    "SELECT queued_at FROM scheduled_jobs WHERE id = ? AND status = 'running' AND claim_generation = ?",
                )
                .bind(job_id)
                .bind(claim_generation)
                .fetch_optional(&mut *conn)
                .await?
                .ok_or(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                })?
            } else {
                finished_at
            };
            // A deferred online slice must not turn a short SQLite writer conflict
            // into the scheduler's long retry window.
            let updated = sqlx::query(
                r#"UPDATE scheduled_jobs
                   SET status = ?, message = ?, finished_at = ?
                   WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
            )
            .bind(status)
            .bind(message)
            .bind(finished_at)
            .bind(job_id)
            .bind(claim_generation)
            .execute(&mut *conn)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }

            if let Some((continuation_id, status, current_trigger_source)) =
                Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id).await?
            {
                if status == "queued" {
                    sqlx::query(
                        "UPDATE scheduled_jobs SET available_at = MIN(available_at, ?) WHERE id = ?",
                    )
                    .bind(available_at)
                    .bind(continuation_id)
                    .execute(&mut *conn)
                    .await?;
                }
                return Ok(ScheduledJobEnqueueResult {
                    job_id: continuation_id,
                    created: false,
                    promoted: false,
                    status,
                    trigger_source: current_trigger_source,
                });
            }

            let inserted = sqlx::query(
                r#"INSERT INTO scheduled_jobs (
                       job_type,
                       trigger_source,
                       key_id,
                       status,
                       attempt,
                       queued_at,
                       available_at,
                       started_at,
                       finished_at
                   ) VALUES (?, 'auto', ?, 'queued', ?, ?, ?, NULL, NULL)"#,
            )
            .bind(job_type)
            .bind(key_id)
            .bind(attempt)
            .bind(continuation_queued_at)
            .bind(available_at)
            .execute(&mut *conn)
            .await?;
            Ok(ScheduledJobEnqueueResult {
                job_id: inserted.last_insert_rowid(),
                created: true,
                promoted: false,
                status: "queued".to_string(),
                trigger_source: "auto".to_string(),
            })
        }
        .await;
        match result {
            Ok(result) => {
                conn.finish(Ok(())).await?;
                Ok(result)
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed control transaction cannot commit"),
            },
        }
    }

    pub(crate) async fn fetch_queued_scheduled_jobs(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedScheduledJob>, ProxyError> {
        let limit = limit.clamp(1, 128) as i64;
        let now = self.backend_time.now_ts();
        let priority_sql = Self::scheduled_job_effective_priority_sql(
            "job_type",
            "trigger_source",
            "queued_at",
        );
        let query = format!(
            r#"
            SELECT id, job_type, trigger_source, key_id, attempt, queued_at, available_at,
                   {priority_sql} AS effective_priority
            FROM scheduled_jobs
            WHERE status = 'queued' AND available_at <= ?
            ORDER BY effective_priority ASC, available_at ASC, queued_at ASC, id ASC
            LIMIT ?
            "#
        );
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64, i64, i64)>(&query)
            .bind(now)
            .bind(now)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(id, job_type, trigger_source, key_id, attempt, queued_at, available_at, effective_priority)| {
                            QueuedScheduledJob {
                                id,
                                job_type,
                                trigger_source,
                                key_id,
                                attempt,
                                queued_at,
                                available_at,
                                effective_priority,
                            }
                        },
                    )
                    .collect()
            })
            .map_err(ProxyError::from);
        let close = conn.close().await;
        match (result, close) {
            (Ok(rows), Ok(())) => Ok(rows),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn fetch_next_queued_scheduled_job_excluding_types(
        &self,
        excluded_job_types: &[&str],
    ) -> Result<Option<QueuedScheduledJob>, ProxyError> {
        if excluded_job_types.is_empty() {
            return Ok(self.fetch_queued_scheduled_jobs(1).await?.into_iter().next());
        }

        let now = self.backend_time.now_ts();
        let priority_sql = Self::scheduled_job_effective_priority_sql(
            "job_type",
            "trigger_source",
            "queued_at",
        );
        let placeholders = std::iter::repeat_n("?", excluded_job_types.len())
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            r#"
            SELECT id, job_type, trigger_source, key_id, attempt, queued_at, available_at,
                   {priority_sql} AS effective_priority
            FROM scheduled_jobs
            WHERE status = 'queued'
              AND available_at <= ?
              AND job_type NOT IN ({placeholders})
            ORDER BY effective_priority ASC, available_at ASC, queued_at ASC, id ASC
            LIMIT 1
            "#,
        );
        let mut query = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64, i64, i64)>(&query)
            .bind(now)
            .bind(now);
        for job_type in excluded_job_types {
            query = query.bind(*job_type);
        }

        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = query
            .fetch_optional(&mut *conn)
            .await
            .map(|row| {
                row.map(
                    |(id, job_type, trigger_source, key_id, attempt, queued_at, available_at, effective_priority)| {
                        QueuedScheduledJob {
                            id,
                            job_type,
                            trigger_source,
                            key_id,
                            attempt,
                            queued_at,
                            available_at,
                            effective_priority,
                        }
                    },
                )
            })
            .map_err(ProxyError::from);
        let close = conn.close().await;
        match (result, close) {
            (Ok(job), Ok(())) => Ok(job),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn fetch_aged_queued_scheduled_job_by_type(
        &self,
        job_type: &str,
        minimum_eligible_wait_secs: i64,
    ) -> Result<Option<QueuedScheduledJob>, ProxyError> {
        let now = self.backend_time.now_ts();
        let priority_sql = Self::scheduled_job_effective_priority_sql(
            "job_type",
            "trigger_source",
            "queued_at",
        );
        let wait_anchor = if job_type == "upstream_reconciliation_research_drain" {
            "queued_at"
        } else {
            "available_at"
        };
        let query = format!(
            r#"
            SELECT id, job_type, trigger_source, key_id, attempt, queued_at, available_at,
                   {priority_sql} AS effective_priority
            FROM scheduled_jobs
            WHERE status = 'queued'
              AND job_type = ?
              AND available_at <= ?
              AND ? - {wait_anchor} >= ?
            ORDER BY {wait_anchor} ASC, queued_at ASC, id ASC
            LIMIT 1
            "#,
        );
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64, i64, i64)>(&query)
            .bind(now)
            .bind(job_type)
            .bind(now)
            .bind(now)
            .bind(minimum_eligible_wait_secs.max(0))
            .fetch_optional(&mut *conn)
            .await
            .map(|row| {
                row.map(
                    |(id, job_type, trigger_source, key_id, attempt, queued_at, available_at, effective_priority)| {
                        QueuedScheduledJob {
                            id,
                            job_type,
                            trigger_source,
                            key_id,
                            attempt,
                            queued_at,
                            available_at,
                            effective_priority,
                        }
                    },
                )
            })
            .map_err(ProxyError::from);
        let close = conn.close().await;
        match (result, close) {
            (Ok(job), Ok(())) => Ok(job),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn next_queued_scheduled_job_available_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = sqlx::query_scalar(
            "SELECT MIN(available_at) FROM scheduled_jobs WHERE status = 'queued'",
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(ProxyError::from);
        let close = conn.close().await;
        match (result, close) {
            (Ok(available_at), Ok(())) => Ok(available_at),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn scheduled_job_mark_running(
        &self,
        job_id: i64,
    ) -> Result<Option<JobLog>, ProxyError> {
        let started_at = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
                let updated = sqlx::query(
                    r#"
                    UPDATE scheduled_jobs
                    SET status = 'running',
                        started_at = ?,
                        claim_generation = claim_generation + 1
                    WHERE id = ?
                      AND status = 'queued'
                      AND available_at <= ?
                    "#,
                )
                .bind(started_at)
                .bind(job_id)
                .bind(started_at)
                .execute(&mut *conn)
                .await?;
                if updated.rows_affected() == 0 {
                    return Ok::<Option<JobLog>, ProxyError>(None);
                }
                let row = sqlx::query_as::<
                    _,
                    (
                        i64,
                        String,
                        String,
                        Option<String>,
                        Option<String>,
                        String,
                        i64,
                        Option<String>,
                        i64,
                        Option<i64>,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"
                    SELECT
                        j.id,
                        j.job_type,
                        j.trigger_source,
                        j.key_id,
                        k.group_name AS key_group,
                        j.status,
                        j.attempt,
                        j.message,
                        j.queued_at,
                        j.started_at,
                        j.finished_at,
                        j.claim_generation
                    FROM scheduled_jobs j
                    LEFT JOIN api_keys k ON k.id = j.key_id
                    WHERE j.id = ?
                    LIMIT 1
                    "#,
                )
                .bind(job_id)
                .fetch_optional(&mut *conn)
                .await?;
                Ok(row.map(
                    |(
                        id,
                        job_type,
                        trigger_source,
                        key_id,
                        key_group,
                        status,
                        attempt,
                        message,
                        queued_at,
                        started_at,
                        finished_at,
                        claim_generation,
                    )| JobLog {
                        id,
                        job_type,
                        trigger_source,
                        key_id,
                        key_group,
                        status,
                        attempt,
                        message,
                        queued_at,
                        started_at,
                        finished_at,
                        claim_generation,
                    },
                ))
            }
            .await;
        match result {
            Ok(job) => {
                conn.finish(Ok(())).await?;
                Ok(job)
            }
            Err(err) => {
                match conn.finish(Err(err)).await {
                    Err(err) => Err(err),
                    Ok(()) => unreachable!("failed scheduled job transaction committed"),
                }
            }
        }
    }

    pub(crate) async fn scheduled_job_by_id(
        &self,
        job_id: i64,
    ) -> Result<Option<JobLog>, ProxyError> {
        sqlx::query_as::<
            _,
            (
                i64,
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                i64,
                Option<String>,
                i64,
                Option<i64>,
                Option<i64>,
                i64,
            ),
        >(
            r#"
            SELECT
                j.id,
                j.job_type,
                j.trigger_source,
                j.key_id,
                k.group_name AS key_group,
                j.status,
                j.attempt,
                j.message,
                j.queued_at,
                j.started_at,
                j.finished_at,
                j.claim_generation
            FROM scheduled_jobs j
            LEFT JOIN api_keys k ON k.id = j.key_id
            WHERE j.id = ?
            LIMIT 1
            "#,
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(
                    id,
                    job_type,
                    trigger_source,
                    key_id,
                    key_group,
                    status,
                    attempt,
                    message,
                    queued_at,
                    started_at,
                    finished_at,
                    claim_generation,
                )| JobLog {
                    id,
                    job_type,
                    trigger_source,
                    key_id,
                    key_group,
                    status,
                    attempt,
                    message,
                    queued_at,
                    started_at,
                    finished_at,
                    claim_generation,
                },
            )
        })
        .map_err(ProxyError::from)
    }

    pub(crate) async fn scheduled_job_claim_is_current(
        &self,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<bool, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let current: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM scheduled_jobs
                WHERE id = ? AND status = 'running' AND claim_generation = ?
            )
            "#,
        )
        .bind(job_id)
        .bind(claim_generation)
        .fetch_one(&mut *conn)
        .await?;
        conn.close().await?;
        Ok(current != 0)
    }

    pub(crate) async fn scheduled_job_claim(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<Option<i64>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            Self::abandon_stale_quota_sync_job_locked(&mut conn, job_type, key_id, now).await?;
            if Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id)
                .await?
                .is_some()
            {
                return Ok::<Option<i64>, ProxyError>(None);
            }
            let res = sqlx::query(
                r#"
                INSERT INTO scheduled_jobs (
                    job_type,
                    trigger_source,
                    key_id,
                    status,
                    attempt,
                    queued_at,
                    available_at,
                    claim_generation,
                    started_at,
                    finished_at
                )
                VALUES (?, ?, ?, 'running', ?, ?, ?, 1, ?, NULL)
                "#,
            )
            .bind(job_type)
            .bind(trigger_source)
            .bind(key_id)
            .bind(attempt)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            Ok(Some(res.last_insert_rowid()))
        }
        .await;
        match result {
            Ok(job_id) => {
                conn.finish(Ok(())).await?;
                Ok(job_id)
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed scheduled job transaction committed"),
            },
        }
    }

    pub(crate) async fn abandon_active_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = sqlx::query(
                r#"
                UPDATE scheduled_jobs
                SET status = CASE
                        WHEN status = 'running' AND job_type = 'ha_outbox_gc' THEN 'queued'
                        ELSE 'abandoned'
                    END,
                    message = CASE
                        WHEN status = 'running' AND job_type = 'ha_outbox_gc'
                            THEN COALESCE(message, 'deferred=process_restart')
                        ELSE COALESCE(message, 'abandoned after process restart')
                    END,
                    started_at = CASE
                        WHEN status = 'running' AND job_type = 'ha_outbox_gc' THEN NULL
                        ELSE started_at
                    END,
                    finished_at = CASE
                        WHEN status = 'running' AND job_type = 'ha_outbox_gc' THEN NULL
                        ELSE ?
                    END,
                    available_at = CASE
                        WHEN status = 'running' AND job_type = 'ha_outbox_gc' THEN MAX(available_at, ?)
                        ELSE available_at
                    END
                WHERE (
                        status = 'running'
                        OR (
                            status = 'queued'
                            -- These are durable GC catch-up contracts. Their available_at
                            -- guards still prevent an early claim after restart.
                            AND NOT (
                                trigger_source = 'auto'
                                AND (
                                    job_type = 'request_logs_gc'
                                    OR job_type = 'ha_outbox_gc'
                                )
                            )
                        )
                    )
                  AND finished_at IS NULL
                "#,
        )
        .bind(now)
        .bind(now.saturating_add(30))
        .execute(&mut *conn)
        .await
        .map(|result| result.rows_affected())
        .map_err(ProxyError::Database);
        match result {
            Ok(rows) => {
                conn.finish(Ok(())).await?;
                Ok(rows)
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed control transaction cannot commit"),
            },
        }
    }

    pub(crate) async fn abandon_running_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        self.abandon_active_scheduled_jobs().await
    }

    pub(crate) async fn recover_stale_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = sqlx::query(
            r#"UPDATE scheduled_jobs
               SET status = 'queued',
                   started_at = NULL,
                   finished_at = NULL,
                   available_at = CASE
                       WHEN job_type = 'request_logs_gc' THEN ?
                       ELSE ?
                   END,
                   claim_generation = claim_generation + 1,
                   message = CASE
                       WHEN job_type = 'ha_outbox_gc' THEN 'deferred=stale_recovery'
                       WHEN job_type IN ('upstream_reconciliation', 'upstream_reconciliation_research_drain') THEN 'deferred=stale_reconciliation_recovery'
                       WHEN job_type = 'request_logs_gc' THEN 'deferred=stale_request_logs_gc_recovery'
                       ELSE 'deferred=stale_control_recovery'
                   END
               WHERE status = 'running'
                 AND started_at IS NOT NULL
                 AND (
                     (job_type = 'ha_outbox_gc' AND started_at <= ?)
                     OR (job_type IN ('upstream_reconciliation', 'upstream_reconciliation_research_drain') AND started_at <= ?)
                     OR (job_type = 'request_logs_gc' AND started_at <= ?)
                     OR (
                         job_type NOT IN ('ha_outbox_gc', 'upstream_reconciliation', 'upstream_reconciliation_research_drain', 'request_logs_gc', 'db_compaction')
                         AND started_at <= ?
                     )
                 )"#,
        )
        .bind(now.saturating_add(300))
        .bind(now.saturating_add(30))
        .bind(now.saturating_sub(120))
        .bind(now.saturating_sub(60))
        .bind(now.saturating_sub(120))
        .bind(now.saturating_sub(300))
        .execute(&mut *conn)
        .await
        .map(|result| result.rows_affected())
        .map_err(ProxyError::Database);
        match result {
            Ok(rows) => {
                conn.finish(Ok(())).await?;
                Ok(rows)
            }
            Err(err) => match conn.finish(Err(err)).await {
                Err(err) => Err(err),
                Ok(()) => unreachable!("failed control transaction cannot commit"),
            },
        }
    }

    pub(crate) async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = sqlx::query(
            r#"UPDATE scheduled_jobs SET status = ?, message = ?, finished_at = ? WHERE id = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(job_id)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(ProxyError::Database);
        conn.finish(result).await
    }

    pub(crate) async fn scheduled_job_finish_claimed(
        &self,
        job_id: i64,
        claim_generation: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let write_result = match sqlx::query(
            r#"UPDATE scheduled_jobs
               SET status = ?, message = ?, finished_at = ?
               WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(job_id)
        .bind(claim_generation)
        .execute(&mut *conn)
        .await
        {
            Ok(updated) if updated.rows_affected() == 0 => Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            }),
            Ok(_) => Ok(()),
            Err(err) => Err(ProxyError::Database(err)),
        };
        conn.finish(write_result).await
    }

    pub(crate) async fn scheduled_job_update_message(
        &self,
        job_id: i64,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = sqlx::query(r#"UPDATE scheduled_jobs SET message = ? WHERE id = ?"#)
            .bind(message)
            .bind(job_id)
            .execute(&mut *conn)
            .await
            .map(|_| ())
            .map_err(ProxyError::Database);
        conn.finish(result).await
    }
}
