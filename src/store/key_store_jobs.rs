impl KeyStore {
    pub(crate) async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        let started_at = Utc::now().timestamp();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut retry_attempt = 0usize;
        let res = loop {
            match sqlx::query(
                r#"INSERT INTO scheduled_jobs (job_type, key_id, status, attempt, started_at)
                   VALUES (?, ?, 'running', ?, ?)"#,
            )
            .bind(job_type)
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
}
