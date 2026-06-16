struct RequestLogBodyGcCandidate {
    id: i64,
    created_at: i64,
    request_user_id: Option<String>,
    result_status: String,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    path: String,
    request_body: Option<Vec<u8>>,
    response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct RequestLogBodyGcBatch {
    cleaned: i64,
    has_more: bool,
}

#[derive(Debug, Clone, Copy)]
struct RequestLogBodyGcCursor {
    created_at: i64,
    id: i64,
    restart_at: Option<i64>,
}

const META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1: &str = "request_log_body_gc_cursor_v1";
const REQUEST_LOG_BODY_GC_SCAN_MULTIPLIER: i64 = 64;

fn request_value_bucket_for_stored_request_log(
    request_kind_key: &str,
    body: Option<&[u8]>,
    counts_business_quota: bool,
) -> RequestValueBucket {
    let normalized = request_kind_key.trim();
    if normalized == "mcp:batch" && body.is_none() {
        if counts_business_quota {
            RequestValueBucket::Valuable
        } else {
            RequestValueBucket::Other
        }
    } else if normalized == "mcp:batch" && !counts_business_quota {
        RequestValueBucket::Other
    } else {
        request_value_bucket_for_request_log(normalized, body)
    }
}

impl KeyStore {
    pub(crate) async fn ensure_request_logs_gc_support_indexes(&self) -> Result<(), ProxyError> {
        for (table, sql) in [
            (
                "auth_token_logs",
                r#"CREATE INDEX IF NOT EXISTS idx_token_logs_request_log_id
                   ON auth_token_logs(request_log_id)"#,
            ),
            (
                "api_key_maintenance_records",
                r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_request_log
                   ON api_key_maintenance_records(request_log_id)"#,
            ),
            (
                "api_key_transient_backoffs",
                r#"CREATE INDEX IF NOT EXISTS idx_api_key_transient_backoffs_source_request_log
                   ON api_key_transient_backoffs(source_request_log_id)"#,
            ),
            (
                "request_logs",
                r#"CREATE INDEX IF NOT EXISTS observability.idx_request_logs_time
                   ON request_logs(created_at DESC, id DESC)"#,
            ),
        ] {
            if !self.table_exists(table).await? {
                continue;
            }
            sqlx::query(sql).execute(&self.pool).await?;
        }

        Ok(())
    }

    async fn delete_old_request_logs_batch(
        &self,
        threshold: i64,
        batch_size: i64,
    ) -> Result<i64, ProxyError> {
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let result = async {
                let mut tx = self.pool.begin().await?;
                sqlx::query("PRAGMA secure_delete = OFF")
                    .execute(&mut *tx)
                    .await?;
                let result = sqlx::query(
                    r#"
                    DELETE FROM observability.request_logs
                    WHERE id IN (
                        SELECT id
                        FROM observability.request_logs
                        WHERE created_at < ?
                        ORDER BY created_at ASC, id ASC
                        LIMIT ?
                    )
                    "#,
                )
                .bind(threshold)
                .bind(batch_size)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok::<_, sqlx::Error>(result)
            }
            .await;
            match result {
                Ok(result) => return Ok(result.rows_affected() as i64),
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "request logs gc batch delete",
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

    async fn unlink_old_request_log_references_batch(
        &self,
        threshold: i64,
        batch_size: i64,
    ) -> Result<(), ProxyError> {
        for (table, operation, sql) in [
            (
                "auth_token_logs",
                "auth token request log unlink",
                r#"
                UPDATE auth_token_logs
                SET request_log_id = NULL
                WHERE request_log_id IN (
                    SELECT id
                    FROM observability.request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            ),
            (
                "api_key_maintenance_records",
                "maintenance request log unlink",
                r#"
                UPDATE api_key_maintenance_records
                SET request_log_id = NULL
                WHERE request_log_id IN (
                    SELECT id
                    FROM observability.request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            ),
            (
                "api_key_transient_backoffs",
                "transient backoff request log unlink",
                r#"
                UPDATE api_key_transient_backoffs
                SET source_request_log_id = NULL
                WHERE source_request_log_id IN (
                    SELECT id
                    FROM observability.request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            ),
        ] {
            if !self.table_exists(table).await? {
                continue;
            }
            let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
            let mut retry_attempt = 0usize;
            loop {
                match sqlx::query(sql)
                    .bind(threshold)
                    .bind(batch_size)
                    .execute(&self.pool)
                    .await
                {
                    Ok(_) => break,
                    Err(err) => {
                        let err = ProxyError::Database(err);
                        if sleep_before_sqlite_transient_write_retry(
                            &self.backend_time,
                            operation,
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

        Ok(())
    }

    async fn delete_old_request_log_rollups_batch(
        &self,
        threshold: i64,
        batch_size: i64,
    ) -> Result<i64, ProxyError> {
        if !self.table_exists("request_log_catalog_rollups").await? {
            return Ok(0);
        }
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            match sqlx::query(
                r#"
                DELETE FROM observability.request_log_catalog_rollups
                WHERE rowid IN (
                    SELECT rowid
                    FROM observability.request_log_catalog_rollups
                    WHERE bucket_start < ?
                    ORDER BY bucket_start ASC
                    LIMIT ?
                )
                "#,
            )
            .bind(threshold)
            .bind(batch_size)
            .execute(&self.pool)
            .await
            {
                Ok(result) => return Ok(result.rows_affected() as i64),
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "request log rollups gc batch delete",
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

    async fn has_old_request_log_rows(&self, threshold: i64) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.request_logs WHERE created_at < ? LIMIT 1",
        )
        .bind(threshold)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn has_old_request_log_rollup_rows(&self, threshold: i64) -> Result<bool, ProxyError> {
        if !self.table_exists("request_log_catalog_rollups").await? {
            return Ok(false);
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.request_log_catalog_rollups WHERE bucket_start < ? LIMIT 1",
        )
        .bind(threshold)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    fn map_request_log_body_gc_candidate(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<RequestLogBodyGcCandidate, sqlx::Error> {
        Ok(RequestLogBodyGcCandidate {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            request_user_id: row.try_get("request_user_id")?,
            result_status: row.try_get("result_status")?,
            request_kind_key: row.try_get("request_kind_key")?,
            request_kind_label: row.try_get("request_kind_label")?,
            request_kind_detail: row.try_get("request_kind_detail")?,
            path: row.try_get("path")?,
            request_body: row.try_get("request_body")?,
            response_body: row.try_get("response_body")?,
        })
    }

    async fn fetch_request_log_body_gc_candidates(
        &self,
        batch_size: i64,
        after: Option<(i64, i64)>,
        row_retention_threshold: i64,
    ) -> Result<Vec<RequestLogBodyGcCandidate>, ProxyError> {
        let rows = if let Some((created_at, id)) = after {
            sqlx::query(
                r#"
                SELECT id, created_at, request_user_id, result_status, request_kind_key,
                       request_kind_label, request_kind_detail, path, request_body, response_body
                FROM observability.request_logs
                WHERE (request_body IS NOT NULL OR response_body IS NOT NULL)
                  AND created_at >= ?
                  AND (created_at > ? OR (created_at = ? AND id > ?))
                ORDER BY created_at ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(row_retention_threshold)
            .bind(created_at)
            .bind(created_at)
            .bind(id)
            .bind(batch_size)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, created_at, request_user_id, result_status, request_kind_key,
                       request_kind_label, request_kind_detail, path, request_body, response_body
                FROM observability.request_logs
                WHERE (request_body IS NOT NULL OR response_body IS NOT NULL)
                  AND created_at >= ?
                ORDER BY created_at ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(row_retention_threshold)
            .bind(batch_size)
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter()
            .map(Self::map_request_log_body_gc_candidate)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    fn request_log_body_is_expired(
        created_at: i64,
        retention_days: i64,
        now: chrono::DateTime<Local>,
    ) -> bool {
        retention_days <= 0
            || created_at < configured_request_logs_retention_threshold_utc_ts_at(retention_days, now)
    }

    fn request_log_body_cursor_restart_at(
        created_at: i64,
        retention_days: i64,
        now: i64,
    ) -> i64 {
        if retention_days <= 0 {
            return now;
        }
        let day_start = local_day_bucket_start_utc_ts(created_at);
        let days = retention_days.min(i64::from(i32::MAX)).max(1) as i32;
        shift_local_day_start_utc_ts(day_start, days).max(now)
    }

    async fn get_request_log_body_gc_cursor(
        &self,
    ) -> Result<Option<RequestLogBodyGcCursor>, ProxyError> {
        let Some(value) = self
            .get_meta_string(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
            .await?
        else {
            return Ok(None);
        };
        let mut parts = value.split(':');
        let Some(created_at) = parts.next() else {
            return Ok(None);
        };
        let Some(id) = parts.next() else {
            return Ok(None);
        };
        let restart_at = parts.next().and_then(|part| part.parse::<i64>().ok());
        let Ok(created_at) = created_at.parse::<i64>() else {
            return Ok(None);
        };
        let Ok(id) = id.parse::<i64>() else {
            return Ok(None);
        };
        Ok(Some(RequestLogBodyGcCursor {
            created_at,
            id,
            restart_at,
        }))
    }

    async fn set_request_log_body_gc_cursor(
        &self,
        cursor: Option<RequestLogBodyGcCursor>,
    ) -> Result<(), ProxyError> {
        if let Some(cursor) = cursor {
            let value = if let Some(restart_at) = cursor.restart_at {
                format!("{}:{}:{}", cursor.created_at, cursor.id, restart_at)
            } else {
                format!("{}:{}", cursor.created_at, cursor.id)
            };
            self.set_meta_string(
                META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1,
                &value,
            )
            .await?;
        } else {
            sqlx::query("DELETE FROM meta WHERE key = ?")
                .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn clear_request_log_body_gc_cursor(&self) -> Result<(), ProxyError> {
        self.set_request_log_body_gc_cursor(None).await
    }

    async fn clear_request_log_body_batch(
        &self,
        settings: &RequestLogRetentionSettings,
        batch_size: i64,
        deadline: Instant,
    ) -> Result<RequestLogBodyGcBatch, ProxyError> {
        let mut cleaned = 0_i64;
        let mut has_more = false;
        let now = self.backend_time.now_ts();
        let mut cursor = self.get_request_log_body_gc_cursor().await?;
        let row_retention_threshold = configured_request_logs_retention_threshold_utc_ts_at(
            settings.max_log_retention_days,
            self.backend_time.local_now(),
        );
        if cursor
            .and_then(|cursor| cursor.restart_at)
            .is_some_and(|restart_at| restart_at <= now)
        {
            self.set_request_log_body_gc_cursor(None).await?;
            cursor = None;
        }
        let mut after = cursor.map(|cursor| (cursor.created_at, cursor.id));
        let mut restart_at = cursor.and_then(|cursor| cursor.restart_at);
        let mut scanned = 0_i64;
        let scan_limit = batch_size.saturating_mul(REQUEST_LOG_BODY_GC_SCAN_MULTIPLIER);
        'scan: while cleaned < batch_size
            && scanned < scan_limit
            && self.backend_time.instant_now() < deadline
        {
            let candidates = self
                .fetch_request_log_body_gc_candidates(batch_size, after, row_retention_threshold)
                .await?;
            if candidates.is_empty() {
                break;
            }
            let fetched = candidates.len() as i64;
            for candidate in candidates {
                after = Some((candidate.created_at, candidate.id));
                scanned += 1;
                let request_body_slice = candidate.request_body.as_deref().unwrap_or(&[]);
                let request_kind = canonicalize_request_log_request_kind(
                    &candidate.path,
                    Some(request_body_slice),
                    candidate.request_kind_key.clone(),
                    candidate.request_kind_label.clone(),
                    candidate.request_kind_detail.clone(),
                );
                let counts_business_quota =
                    request_log_counts_business_quota(&request_kind.key, Some(request_body_slice));
                let request_value_bucket =
                    request_value_bucket_for_request_log(&request_kind.key, Some(request_body_slice));
                let retention_decision = self
                    .request_log_body_retention_decision(
                        settings,
                        candidate.request_user_id.as_deref(),
                        &candidate.result_status,
                        request_value_bucket,
                        0,
                        RequestLogBodyRetentionDecisionMode {
                            include_debug_shared: true,
                            include_heavy_usage: true,
                        },
                    )
                    .await?;
                let retention_days = retention_decision.days;
                if !Self::request_log_body_is_expired(
                    candidate.created_at,
                    retention_days,
                    self.backend_time.local_now(),
                ) {
                    let cursor_retention_days = Self::request_log_body_cursor_retention_days(
                        settings,
                        &retention_decision,
                        &candidate.result_status,
                        request_value_bucket,
                        candidate.request_user_id.is_some(),
                    );
                    let candidate_restart_at = Self::request_log_body_cursor_restart_at(
                        candidate.created_at,
                        cursor_retention_days,
                        now,
                    );
                    restart_at = Some(
                        restart_at
                            .map(|current| current.min(candidate_restart_at))
                            .unwrap_or(candidate_restart_at),
                    );
                    if scanned >= scan_limit || self.backend_time.instant_now() >= deadline {
                        has_more = true;
                        break 'scan;
                    }
                    continue;
                }

                let response_body_slice = candidate.response_body.as_deref().unwrap_or(&[]);
                let reason = if retention_days <= 0 {
                    REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO
                } else {
                    REQUEST_LOG_BODY_CLEANED_REASON_RETENTION_EXPIRED
                };
                let request_kind_key = request_kind.key;
                let request_kind_label = request_kind.label;
                let request_kind_detail = request_kind.detail;
                let request_body_bytes = request_body_slice.len() as i64;
                let response_body_bytes = response_body_slice.len() as i64;
                let request_body_sha256 = sha256_hex_bytes(request_body_slice);
                let response_body_sha256 = sha256_hex_bytes(response_body_slice);
                let mut retry_attempt = 0usize;
                let result = loop {
                    match sqlx::query(
                        r#"
                        UPDATE observability.request_logs
                        SET request_body = NULL,
                            response_body = NULL,
                            request_kind_key = ?,
                            request_kind_label = ?,
                            request_kind_detail = ?,
                            counts_business_quota = COALESCE(counts_business_quota, ?),
                            request_body_bytes = COALESCE(request_body_bytes, ?),
                            response_body_bytes = COALESCE(response_body_bytes, ?),
                            request_body_sha256 = COALESCE(request_body_sha256, ?),
                            response_body_sha256 = COALESCE(response_body_sha256, ?),
                            body_retention_days = ?,
                            body_retention_profile = ?,
                            body_cleaned_reason = ?,
                            body_cleaned_at = ?
                        WHERE id = ? AND (request_body IS NOT NULL OR response_body IS NOT NULL)
                        "#,
                    )
                    .bind(&request_kind_key)
                    .bind(&request_kind_label)
                    .bind(request_kind_detail.as_deref())
                    .bind(i64::from(counts_business_quota))
                    .bind(request_body_bytes)
                    .bind(response_body_bytes)
                    .bind(&request_body_sha256)
                    .bind(&response_body_sha256)
                    .bind(retention_days)
                    .bind(retention_decision.profile)
                    .bind(reason)
                    .bind(now)
                    .bind(candidate.id)
                    .execute(&self.pool)
                    .await
                    {
                        Ok(result) => break result,
                        Err(err) => {
                            let err = ProxyError::Database(err);
                            if sleep_before_sqlite_transient_write_retry(
                                &self.backend_time,
                                "request log body cleanup",
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
                cleaned += result.rows_affected() as i64;
                if cleaned >= batch_size
                    || scanned >= scan_limit
                    || self.backend_time.instant_now() >= deadline
                {
                    has_more = true;
                    break 'scan;
                }
            }
            if fetched < batch_size {
                break;
            }
        }
        if has_more {
            self.set_request_log_body_gc_cursor(after.map(|(created_at, id)| {
                RequestLogBodyGcCursor {
                    created_at,
                    id,
                    restart_at,
                }
            }))
            .await?;
        } else if self.backend_time.instant_now() >= deadline && after.is_some() {
            has_more = true;
            self.set_request_log_body_gc_cursor(after.map(|(created_at, id)| {
                RequestLogBodyGcCursor {
                    created_at,
                    id,
                    restart_at,
                }
            }))
            .await?;
        } else if let Some((created_at, id)) = after {
            if let Some(restart_at) = restart_at {
                self.set_request_log_body_gc_cursor(Some(RequestLogBodyGcCursor {
                    created_at,
                    id,
                    restart_at: Some(restart_at),
                }))
                .await?;
            } else {
                self.set_request_log_body_gc_cursor(None).await?;
            }
        }

        Ok(RequestLogBodyGcBatch { cleaned, has_more })
    }

    pub(crate) async fn delete_old_request_logs_bounded(
        &self,
        threshold: i64,
        options: RequestLogsGcOptions,
        retention_days: i64,
        settings: &RequestLogRetentionSettings,
    ) -> Result<RequestLogsGcReport, ProxyError> {
        let batch_size = options.batch_size.max(1);
        let max_batches = options.max_batches.max(1);
        let deadline = self
            .backend_time
            .deadline_after(Duration::from_secs(options.max_runtime_secs));
        let started = self.backend_time.instant_now();
        let mut cleaned_request_log_bodies = 0_i64;
        let mut deleted_request_logs = 0_i64;
        let mut deleted_rollups = 0_i64;
        let mut body_batch_has_more = false;
        let mut batches = 0_i64;

        while batches < max_batches && self.backend_time.instant_now() < deadline {
            let body_batch = self
                .clear_request_log_body_batch(settings, batch_size, deadline)
                .await?;
            self.unlink_old_request_log_references_batch(threshold, batch_size)
                .await?;
            let request_deleted = self
                .delete_old_request_logs_batch(threshold, batch_size)
                .await?;
            let rollup_deleted = self
                .delete_old_request_log_rollups_batch(threshold, batch_size)
                .await?;
            body_batch_has_more = body_batch.has_more;
            cleaned_request_log_bodies += body_batch.cleaned;
            deleted_request_logs += request_deleted;
            deleted_rollups += rollup_deleted;
            batches += 1;

            if !body_batch.has_more
                && body_batch.cleaned == 0
                && request_deleted == 0
                && rollup_deleted == 0
            {
                break;
            }

            if batches < max_batches && options.inter_batch_sleep_ms > 0 {
                self.backend_time
                    .sleep(Duration::from_millis(options.inter_batch_sleep_ms))
                    .await;
            }
        }

        let has_more = self.has_old_request_log_rows(threshold).await?
            || self.has_old_request_log_rollup_rows(threshold).await?
            || body_batch_has_more;
        self.invalidate_request_logs_catalog_cache().await;
        Ok(RequestLogsGcReport {
            retention_days,
            threshold,
            batch_size,
            max_batches,
            cleaned_request_log_bodies,
            deleted_request_logs,
            deleted_rollups,
            batches,
            completed: !has_more,
            has_more,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

}
