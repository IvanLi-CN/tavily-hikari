impl KeyStore {
    fn scheduled_job_stale_group(job_type: &str) -> Option<&'static str> {
        match job_type {
            "quota_sync" | "quota_sync/manual" => Some("quota_sync"),
            "quota_sync/hot" => Some("quota_sync/hot"),
            _ => None,
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
        let started_at = Utc::now().timestamp();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut retry_attempt = 0usize;
        let res = loop {
            match sqlx::query(
                r#"INSERT INTO scheduled_jobs (job_type, trigger_source, key_id, status, attempt, started_at)
                   VALUES (?, ?, ?, 'running', ?, ?)"#,
            )
            .bind(job_type)
            .bind(trigger_source)
            .bind(key_id)
            .bind(attempt)
            .bind(started_at)
            .execute(&self.pool)
            .await
            {
                Ok(res) => break res,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
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
        Ok(res.last_insert_rowid())
    }

    pub(crate) async fn scheduled_job_claim(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<Option<i64>, ProxyError> {
        let started_at = Utc::now().timestamp();
        let stale_before = started_at.saturating_sub(QUOTA_SYNC_STALE_RUNNING_SECS);
        let stale_group = Self::scheduled_job_stale_group(job_type);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
                if let Some(stale_group) = stale_group {
                    sqlx::query(
                        r#"
                        UPDATE scheduled_jobs
                        SET status = 'abandoned',
                            message = COALESCE(message, 'abandoned after quota_sync timeout window'),
                            finished_at = ?
                        WHERE status = 'running'
                          AND finished_at IS NULL
                          AND started_at <= ?
                          AND (
                                (job_type = 'quota_sync' OR job_type = 'quota_sync/manual')
                                AND ? = 'quota_sync'
                              OR job_type = ? AND ? = 'quota_sync/hot'
                          )
                          AND ((key_id IS NULL AND ? IS NULL) OR key_id = ?)
                        "#,
                    )
                    .bind(started_at)
                    .bind(stale_before)
                    .bind(stale_group)
                    .bind(stale_group)
                    .bind(stale_group)
                    .bind(key_id)
                    .bind(key_id)
                    .execute(&mut *conn)
                    .await?;
                }
                let running: Option<i64> = sqlx::query_scalar(
                    r#"
                    SELECT id
                    FROM scheduled_jobs
                    WHERE job_type = ?
                      AND status = 'running'
                      AND ((key_id IS NULL AND ? IS NULL) OR key_id = ?)
                    ORDER BY started_at DESC, id DESC
                    LIMIT 1
                    "#,
                )
                .bind(job_type)
                .bind(key_id)
                .bind(key_id)
                .fetch_optional(&mut *conn)
                .await?;
                if running.is_some() {
                    sqlx::query("COMMIT").execute(&mut *conn).await?;
                    return Ok::<Option<i64>, ProxyError>(None);
                }
                let res = sqlx::query(
                    r#"INSERT INTO scheduled_jobs (job_type, trigger_source, key_id, status, attempt, started_at)
                       VALUES (?, ?, ?, 'running', ?, ?)"#,
                )
                .bind(job_type)
                .bind(trigger_source)
                .bind(key_id)
                .bind(attempt)
                .bind(started_at)
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

    pub(crate) async fn abandon_running_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        let now = Utc::now().timestamp();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut retry_attempt = 0usize;
        loop {
            match sqlx::query(
                r#"
                UPDATE scheduled_jobs
                SET status = 'abandoned',
                    message = COALESCE(message, 'abandoned after process restart'),
                    finished_at = ?
                WHERE status = 'running' AND finished_at IS NULL
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

    pub(crate) async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let finished_at = Utc::now().timestamp();
        let deadline = Instant::now() + Duration::from_secs(10);
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
        let deadline = Instant::now() + Duration::from_secs(10);
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
