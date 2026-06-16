impl KeyStore {
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
                | "mcp_sessions_gc"
                | "mcp_session_init_backoffs_gc"
                | "token_usage_rollup"
                | "usage_aggregation",
            ) => 3,
            (
                _,
                "linuxdo_user_tag_binding_refresh"
                | "forward_proxy_geo_refresh"
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
                WHEN {job_type_column} = 'auth_token_logs_gc' OR {job_type_column} = 'mcp_sessions_gc' OR {job_type_column} = 'mcp_session_init_backoffs_gc' OR {job_type_column} = 'token_usage_rollup' OR {job_type_column} = 'usage_aggregation' THEN 3 \
                WHEN {job_type_column} = 'linuxdo_user_tag_binding_refresh' OR {job_type_column} = 'forward_proxy_geo_refresh' OR {job_type_column} = 'linuxdo_user_status_sync' THEN 4 \
                WHEN {job_type_column} = 'quota_sync' OR {job_type_column} = 'quota_sync/manual' OR {job_type_column} = 'quota_sync/hot' THEN 5 \
                ELSE 6 \
            END"
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
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_queue
            ON scheduled_jobs(status, queued_at ASC, id ASC)
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
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_recent
            ON scheduled_jobs(COALESCE(started_at, queued_at) DESC, id DESC)
            "#,
        )
        .execute(&mut **conn)
        .await?;
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_queue
            ON scheduled_jobs(status, queued_at ASC, id ASC)
            "#,
        )
        .execute(&mut **conn)
        .await?;
        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_jobs_active_identity
            ON scheduled_jobs(job_type, IFNULL(key_id, ''))
            WHERE status = 'queued' OR status = 'running'
            "#,
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }

    pub(crate) async fn ensure_scheduled_jobs_queue_schema(&self) -> Result<(), ProxyError> {
        if !self.table_column_exists("scheduled_jobs", "queued_at").await? {
            self.rebuild_scheduled_jobs_table().await?;
        }
        self.create_scheduled_jobs_indexes().await
    }

    async fn rebuild_scheduled_jobs_table(&self) -> Result<(), ProxyError> {
        let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;

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

        let reenable_result = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
        if rebuild_result.is_ok() {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
        } else {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        if let Err(err) = reenable_result {
            return Err(ProxyError::Database(err));
        }
        rebuild_result
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
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
                if let Some((job_id, status, _current_trigger_source)) =
                    Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id).await?
                    && status == "running"
                {
                    sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                        started_at
                    )
                    VALUES (?, ?, ?, 'running', ?, ?, ?)
                    "#,
                )
                .bind(job_type)
                .bind(trigger_source)
                .bind(key_id)
                .bind(attempt)
                .bind(started_at)
                .bind(started_at)
                .execute(&mut *conn)
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(res.last_insert_rowid())
            }
            .await;

            match result {
                Ok(job_id) => return Ok(job_id),
                Err(err) => {
                    if Self::is_scheduled_job_active_identity_conflict(&err)
                        && let Some((job_id, _status, _current_trigger_source)) =
                            self.scheduled_job_lookup_active(job_type, key_id).await?
                    {
                        return Ok(job_id);
                    }
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job start",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        };
    }

    async fn abandon_stale_quota_sync_job_locked(
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
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
        .execute(&mut **conn)
        .await?;
        Ok(())
    }

    async fn scheduled_job_lookup_active_locked(
        conn: &mut sqlx::pool::PoolConnection<Sqlite>,
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
        .fetch_optional(&mut **conn)
        .await
        .map_err(ProxyError::from)
    }

    async fn scheduled_job_lookup_active(
        &self,
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
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn scheduled_job_enqueue(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        // Fast-path repeated coalesce reads so owner-facing manual triggers do not
        // fail behind an unrelated long-lived write window.
        if Self::scheduled_job_stale_group(job_type).is_none()
            && let Some((job_id, status, current_trigger_source)) =
                self.scheduled_job_lookup_active(job_type, key_id).await?
        {
            let promoted = Self::should_promote_scheduled_job_trigger_source(
                job_type,
                &current_trigger_source,
                trigger_source,
            );
            if !promoted {
                return Ok(ScheduledJobEnqueueResult {
                    job_id,
                    created: false,
                    promoted: false,
                    status,
                    trigger_source: current_trigger_source,
                });
            }
        }

        let queued_at = self.backend_time.now_ts();
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
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
                    if promoted {
                        sqlx::query(
                            r#"UPDATE scheduled_jobs
                               SET trigger_source = ?
                               WHERE id = ?"#,
                        )
                        .bind(trigger_source)
                        .bind(job_id)
                        .execute(&mut *conn)
                        .await?;
                    }
                    sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                        started_at,
                        finished_at
                    )
                    VALUES (?, ?, ?, 'queued', ?, ?, NULL, NULL)
                    "#,
                )
                .bind(job_type)
                .bind(trigger_source)
                .bind(key_id)
                .bind(attempt)
                .bind(queued_at)
                .execute(&mut *conn)
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                Ok(outcome) => return Ok(outcome),
                Err(err) => {
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job enqueue",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    pub(crate) async fn fetch_queued_scheduled_jobs(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedScheduledJob>, ProxyError> {
        let limit = limit.clamp(1, 128) as i64;
        let priority_sql = Self::scheduled_job_priority_sql("job_type", "trigger_source");
        let query = format!(
            r#"
            SELECT id, job_type, trigger_source, key_id, attempt, queued_at
            FROM scheduled_jobs
            WHERE status = 'queued'
            ORDER BY {priority_sql}, queued_at ASC, id ASC
            LIMIT ?
            "#
        );
        sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64)>(&query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(id, job_type, trigger_source, key_id, attempt, queued_at)| {
                            QueuedScheduledJob {
                                id,
                                job_type,
                                trigger_source,
                                key_id,
                                attempt,
                                queued_at,
                            }
                        },
                    )
                    .collect()
            })
            .map_err(ProxyError::from)
    }

    pub(crate) async fn scheduled_job_mark_running(
        &self,
        job_id: i64,
    ) -> Result<Option<JobLog>, ProxyError> {
        let started_at = self.backend_time.now_ts();
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
                let updated = sqlx::query(
                    r#"
                    UPDATE scheduled_jobs
                    SET status = 'running',
                        started_at = ?
                    WHERE id = ?
                      AND status = 'queued'
                    "#,
                )
                .bind(started_at)
                .bind(job_id)
                .execute(&mut *conn)
                .await?;
                if updated.rows_affected() == 0 {
                    sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                        j.finished_at
                    FROM scheduled_jobs j
                    LEFT JOIN api_keys k ON k.id = j.key_id
                    WHERE j.id = ?
                    LIMIT 1
                    "#,
                )
                .bind(job_id)
                .fetch_optional(&mut *conn)
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                    },
                ))
            }
            .await;

            match result {
                Ok(job) => return Ok(job),
                Err(err) => {
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job mark running",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
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
                j.finished_at
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
                },
            )
        })
        .map_err(ProxyError::from)
    }

    pub(crate) async fn scheduled_job_claim(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<Option<i64>, ProxyError> {
        let now = self.backend_time.now_ts();
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
                Self::abandon_stale_quota_sync_job_locked(&mut conn, job_type, key_id, now).await?;
                if Self::scheduled_job_lookup_active_locked(&mut conn, job_type, key_id)
                    .await?
                    .is_some()
                {
                    sqlx::query("COMMIT").execute(&mut *conn).await?;
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
                        started_at,
                        finished_at
                    )
                    VALUES (?, ?, ?, 'running', ?, ?, ?, NULL)
                    "#,
                )
                .bind(job_type)
                .bind(trigger_source)
                .bind(key_id)
                .bind(attempt)
                .bind(now)
                .bind(now)
                .execute(&mut *conn)
                .await?;
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(Some(res.last_insert_rowid()))
            }
            .await;

            match result {
                Ok(job_id) => return Ok(job_id),
                Err(err) => {
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job claim",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    pub(crate) async fn abandon_active_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        let now = self.backend_time.now_ts();
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            match sqlx::query(
                r#"
                UPDATE scheduled_jobs
                SET status = 'abandoned',
                    message = COALESCE(message, 'abandoned after process restart'),
                    finished_at = ?
                WHERE (status = 'queued' OR status = 'running')
                  AND finished_at IS NULL
                "#,
            )
            .bind(now)
            .execute(&self.pool)
            .await
            {
                Ok(result) => return Ok(result.rows_affected()),
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job abandon",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    pub(crate) async fn abandon_running_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        self.abandon_active_scheduled_jobs().await
    }

    pub(crate) async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = self.backend_time.now_ts();
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            match sqlx::query(
                r#"UPDATE scheduled_jobs SET status = ?, message = ?, finished_at = ? WHERE id = ?"#,
            )
            .bind(status)
            .bind(message)
            .bind(finished_at)
            .bind(job_id)
            .execute(&self.pool)
            .await
            {
                Ok(_) => break,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job finish",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn scheduled_job_update_message(
        &self,
        job_id: i64,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            match sqlx::query(r#"UPDATE scheduled_jobs SET message = ? WHERE id = ?"#)
                .bind(message)
                .bind(job_id)
                .execute(&self.pool)
                .await
            {
                Ok(_) => break,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "scheduled job update message",
                        retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}
