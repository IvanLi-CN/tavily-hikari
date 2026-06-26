impl KeyStore {
    async fn request_logs_support_server_pressure_rebuild(&self) -> Result<bool, ProxyError> {
        for column in [
            "request_user_id",
            "counts_business_quota",
            "upstream_operation",
            "result_status",
            "visibility",
            "created_at",
        ] {
            if !Self::table_column_exists_in_pool(&self.pool, "request_logs", column).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) async fn ensure_server_pressure_bucket_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS observability.server_pressure_buckets (
                bucket_kind TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                failure_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (bucket_kind, bucket_start)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        for sql in [
            r#"CREATE INDEX IF NOT EXISTS observability.idx_server_pressure_buckets_kind_time
               ON server_pressure_buckets(bucket_kind, bucket_start DESC)"#,
            r#"CREATE INDEX IF NOT EXISTS observability.idx_server_pressure_buckets_time
               ON server_pressure_buckets(bucket_start DESC)"#,
        ] {
            sqlx::query(sql).execute(&self.pool).await?;
        }

        Ok(())
    }

    pub(crate) async fn rebuild_server_pressure_buckets(&self) -> Result<(), ProxyError> {
        self.ensure_server_pressure_bucket_schema().await?;
        if self.uses_legacy_single_db_observability_compatibility() {
            return Ok(());
        }
        if !self.request_logs_support_server_pressure_rebuild().await? {
            return Ok(());
        }

        let updated_at = self.backend_time.now_ts();
        let five_minute_since = updated_at.saturating_sub(48 * SECS_PER_HOUR);
        let hour_since = updated_at.saturating_sub(8 * SECS_PER_DAY);

        let five_minute_sql = r#"
            INSERT INTO observability.server_pressure_buckets (
                bucket_kind,
                bucket_start,
                bucket_secs,
                success_count,
                failure_count,
                updated_at
            )
            SELECT
                'five_minute' AS bucket_kind,
                (created_at / ?) * ? AS bucket_start,
                ? AS bucket_secs,
                SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN result_status != ? THEN 1 ELSE 0 END) AS failure_count,
                ? AS updated_at
            FROM observability.request_logs
            WHERE visibility = ?
              AND created_at >= ?
              AND request_user_id IS NOT NULL
              AND counts_business_quota = 1
              AND upstream_operation IS NOT NULL
              AND result_status != ?
            GROUP BY bucket_start
        "#;
        let hour_sql = r#"
            INSERT INTO observability.server_pressure_buckets (
                bucket_kind,
                bucket_start,
                bucket_secs,
                success_count,
                failure_count,
                updated_at
            )
            SELECT
                'hour' AS bucket_kind,
                CAST(
                    strftime(
                        '%s',
                        strftime('%Y-%m-%d %H:00:00', created_at, 'unixepoch', 'localtime'),
                        'utc'
                    ) AS INTEGER
                ) AS bucket_start,
                ? AS bucket_secs,
                SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN result_status != ? THEN 1 ELSE 0 END) AS failure_count,
                ? AS updated_at
            FROM observability.request_logs
            WHERE visibility = ?
              AND created_at >= ?
              AND request_user_id IS NOT NULL
              AND counts_business_quota = 1
              AND upstream_operation IS NOT NULL
              AND result_status != ?
            GROUP BY bucket_start
        "#;

        let mut conn = begin_immediate_sqlite_connection_with_retry(
            &self.pool,
            &self.backend_time,
            "rebuild_server_pressure_buckets",
            Duration::from_secs(5),
        )
        .await?;
        let result = async {
            sqlx::query("DELETE FROM observability.server_pressure_buckets")
                .execute(&mut *conn)
                .await?;
            sqlx::query(five_minute_sql)
                .bind(SECS_PER_FIVE_MINUTES)
                .bind(SECS_PER_FIVE_MINUTES)
                .bind(SECS_PER_FIVE_MINUTES)
                .bind(OUTCOME_SUCCESS)
                .bind(OUTCOME_SUCCESS)
                .bind(updated_at)
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .bind(five_minute_since)
                .bind(OUTCOME_QUOTA_EXHAUSTED)
                .execute(&mut *conn)
                .await?;
            sqlx::query(hour_sql)
                .bind(SECS_PER_HOUR)
                .bind(OUTCOME_SUCCESS)
                .bind(OUTCOME_SUCCESS)
                .bind(updated_at)
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .bind(hour_since)
                .bind(OUTCOME_QUOTA_EXHAUSTED)
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        if result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        result
    }

    pub(crate) async fn upsert_server_pressure_event(
        &self,
        created_at: i64,
        result_status: &str,
    ) -> Result<(), ProxyError> {
        self.ensure_server_pressure_bucket_schema().await?;
        let success = if result_status == OUTCOME_SUCCESS { 1_i64 } else { 0_i64 };
        let failure = if result_status == OUTCOME_SUCCESS { 0_i64 } else { 1_i64 };
        let updated_at = self.backend_time.now_ts();
        let Some(utc_dt) = chrono::Utc.timestamp_opt(created_at, 0).single() else {
            return Ok(());
        };
        let local_dt = utc_dt.with_timezone(&chrono::Local);
        let five_minute_bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
        let hour_bucket_start = start_of_local_hour_utc_ts(local_dt);

        let sql = r#"
            INSERT INTO observability.server_pressure_buckets (
                bucket_kind,
                bucket_start,
                bucket_secs,
                success_count,
                failure_count,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_kind, bucket_start) DO UPDATE SET
                success_count = server_pressure_buckets.success_count + excluded.success_count,
                failure_count = server_pressure_buckets.failure_count + excluded.failure_count,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind("five_minute")
            .bind(five_minute_bucket_start)
            .bind(SECS_PER_FIVE_MINUTES)
            .bind(success)
            .bind(failure)
            .bind(updated_at)
            .execute(&self.pool)
            .await?;
        sqlx::query(sql)
            .bind("hour")
            .bind(hour_bucket_start)
            .bind(SECS_PER_HOUR)
            .bind(success)
            .bind(failure)
            .bind(updated_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn fetch_server_pressure_points(
        &self,
        bucket_kind: &str,
        since: i64,
        until: i64,
    ) -> Result<Vec<AnalysisPressurePoint>, ProxyError> {
        self.ensure_server_pressure_bucket_schema().await?;
        let rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            r#"
            SELECT bucket_start, success_count, failure_count, bucket_secs
            FROM observability.server_pressure_buckets
            WHERE bucket_kind = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(bucket_kind)
        .bind(since)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(bucket_start, success_count, failure_count, _bucket_secs)| AnalysisPressurePoint {
                bucket_start,
                display_bucket_start: bucket_start,
                pressure: success_count + failure_count,
                success_count,
                failure_count,
            })
            .collect())
    }
}
