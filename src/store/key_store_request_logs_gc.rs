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

struct RequestLogBodyGcCandidatePage {
    candidates: Vec<RequestLogBodyGcCandidate>,
    scanned: i64,
    last_scanned: Option<(i64, i64)>,
    has_more: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct RequestLogBodyGcBatch {
    cleaned: i64,
    has_more: bool,
    diagnostics: RequestLogBodyGcDiagnostics,
}

#[derive(Debug, Clone, Copy, Default)]
struct RequestLogBodyGcDiagnostics {
    scanned_body_candidates: i64,
    unique_retention_users: i64,
    retention_context_cache_hits: i64,
    body_candidate_query_elapsed_ms: u128,
    body_retention_decision_elapsed_ms: u128,
    body_write_elapsed_ms: u128,
}

impl RequestLogBodyGcDiagnostics {
    fn merge(&mut self, other: Self) {
        self.scanned_body_candidates += other.scanned_body_candidates;
        self.unique_retention_users += other.unique_retention_users;
        self.retention_context_cache_hits += other.retention_context_cache_hits;
        self.body_candidate_query_elapsed_ms += other.body_candidate_query_elapsed_ms;
        self.body_retention_decision_elapsed_ms += other.body_retention_decision_elapsed_ms;
        self.body_write_elapsed_ms += other.body_write_elapsed_ms;
    }
}

#[derive(Debug, Clone, Copy)]
struct RequestLogBodyGcUserContext {
    debug_shared: bool,
    heavy_usage: bool,
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
    pub(crate) fn try_admit_request_logs_gc(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::RequestLogsGc)
    }

    pub(crate) fn request_logs_gc_continue_defer_reason(
        &self,
    ) -> Option<SqliteAdmissionDeferReason> {
        self.sqlite_runtime.maintenance_bulk_continue_reason()
    }

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
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::RequestLogsGc)
            .await?;
        let result = async {
            sqlx::query(
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
            .await
        }
        .await
        .map_err(ProxyError::Database);
        match result {
            Ok(result) => {
                tx.finish(Ok(())).await?;
                Ok(result.rows_affected() as i64)
            }
            Err(err) => tx.finish(Err(err)).await.map(|_| unreachable!()),
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
                let mut conn = self
                    .sqlite_runtime
                    .acquire_operation_connection(SqliteOperation::RequestLogsGc)
                    .await?;
                match sqlx::query(sql)
                    .bind(threshold)
                    .bind(batch_size)
                    .execute(&mut *conn)
                    .await
                {
                    Ok(_) => {
                        conn.close().await?;
                        break;
                    }
                    Err(err) => {
                        drop(conn);
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
            let mut conn = self
                .sqlite_runtime
                .acquire_operation_connection(SqliteOperation::RequestLogsGc)
                .await?;
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
            .execute(&mut *conn)
            .await
            {
                Ok(result) => {
                    conn.close().await?;
                    return Ok(result.rows_affected() as i64);
                }
                Err(err) => {
                    drop(conn);
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
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.request_logs WHERE created_at < ? LIMIT 1",
        )
        .bind(threshold)
        .fetch_optional(&mut *conn)
        .await?;
        conn.close().await?;
        Ok(exists.is_some())
    }

    async fn has_old_request_log_rollup_rows(&self, threshold: i64) -> Result<bool, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let rollup_table_exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = ?)",
        )
        .bind("request_log_catalog_rollups")
        .fetch_one(&mut *conn)
        .await?;
        if rollup_table_exists == 0 {
            conn.close().await?;
            return Ok(false);
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM observability.request_log_catalog_rollups WHERE bucket_start < ? LIMIT 1",
        )
        .bind(threshold)
        .fetch_optional(&mut *conn)
        .await?;
        conn.close().await?;
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
        scan_limit: i64,
        after: Option<(i64, i64)>,
        row_retention_threshold: i64,
    ) -> Result<RequestLogBodyGcCandidatePage, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let scan_window: Vec<(i64, i64)> = if let Some((created_at, id)) = after {
            sqlx::query_as(
                r#"
                SELECT id, created_at
                FROM observability.request_logs INDEXED BY idx_request_logs_time
                WHERE created_at >= ?
                  AND (created_at > ? OR (created_at = ? AND id > ?))
                ORDER BY created_at ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(row_retention_threshold)
            .bind(created_at)
            .bind(created_at)
            .bind(id)
            .bind(scan_limit)
            .fetch_all(&mut *conn)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT id, created_at
                FROM observability.request_logs INDEXED BY idx_request_logs_time
                WHERE created_at >= ?
                ORDER BY created_at ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(row_retention_threshold)
            .bind(scan_limit)
            .fetch_all(&mut *conn)
            .await?
        };
        let Some(&(last_id, last_created_at)) = scan_window.last() else {
            conn.close().await?;
            return Ok(RequestLogBodyGcCandidatePage {
                candidates: Vec::new(),
                scanned: 0,
                last_scanned: None,
                has_more: false,
            });
        };
        let rows = if let Some((created_at, id)) = after {
            sqlx::query(
                r#"
                SELECT id, created_at, request_user_id, result_status, request_kind_key,
                       request_kind_label, request_kind_detail, path, request_body, response_body
                FROM observability.request_logs INDEXED BY idx_request_logs_time
                WHERE created_at >= ?
                  AND (created_at > ? OR (created_at = ? AND id > ?))
                  AND (created_at < ? OR (created_at = ? AND id <= ?))
                  AND (request_body IS NOT NULL OR response_body IS NOT NULL)
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(row_retention_threshold)
            .bind(created_at)
            .bind(created_at)
            .bind(id)
            .bind(last_created_at)
            .bind(last_created_at)
            .bind(last_id)
            .fetch_all(&mut *conn)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, created_at, request_user_id, result_status, request_kind_key,
                       request_kind_label, request_kind_detail, path, request_body, response_body
                FROM observability.request_logs INDEXED BY idx_request_logs_time
                WHERE created_at >= ?
                  AND (created_at < ? OR (created_at = ? AND id <= ?))
                  AND (request_body IS NOT NULL OR response_body IS NOT NULL)
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(row_retention_threshold)
            .bind(last_created_at)
            .bind(last_created_at)
            .bind(last_id)
            .fetch_all(&mut *conn)
            .await?
        };
        conn.close().await?;
        let candidates = rows
            .into_iter()
            .map(Self::map_request_log_body_gc_candidate)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)?;
        Ok(RequestLogBodyGcCandidatePage {
            candidates,
            scanned: scan_window.len() as i64,
            last_scanned: Some((last_created_at, last_id)),
            has_more: scan_window.len() as i64 >= scan_limit,
        })
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
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value FROM meta WHERE key = ? LIMIT 1",
        )
        .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
        .fetch_optional(&mut *conn)
        .await?;
        conn.close().await?;
        let Some(value) = value else {
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
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::RequestLogsGc)
            .await?;
        let result = if let Some(cursor) = cursor {
            let value = if let Some(restart_at) = cursor.restart_at {
                format!("{}:{}:{}", cursor.created_at, cursor.id, restart_at)
            } else {
                format!("{}:{}", cursor.created_at, cursor.id)
            };
            sqlx::query(
                r#"
                INSERT INTO meta (key, value)
                VALUES (?, ?)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
            .bind(value)
            .execute(&mut *tx)
            .await
        } else {
            sqlx::query("DELETE FROM meta WHERE key = ?")
                .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
                .execute(&mut *tx)
                .await
        };
        match result {
            Ok(_) => tx.finish(Ok(())).await,
            Err(err) => tx.finish(Err(ProxyError::Database(err))).await,
        }
    }

    pub(crate) async fn clear_request_log_body_gc_cursor(&self) -> Result<(), ProxyError> {
        self.set_request_log_body_gc_cursor(None).await
    }

    async fn clear_request_log_body_batch(
        &self,
        settings: &RequestLogRetentionSettings,
        batch_size: i64,
        deadline: Instant,
        retention_contexts: &mut std::collections::HashMap<String, RequestLogBodyGcUserContext>,
    ) -> Result<RequestLogBodyGcBatch, ProxyError> {
        let mut cleaned = 0_i64;
        let mut has_more = false;
        let mut diagnostics = RequestLogBodyGcDiagnostics::default();
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
            let query_started = self.backend_time.instant_now();
            let page = self
                .fetch_request_log_body_gc_candidates(
                    scan_limit.saturating_sub(scanned),
                    after,
                    row_retention_threshold,
                )
                .await?;
            diagnostics.body_candidate_query_elapsed_ms += query_started.elapsed().as_millis();
            if page.scanned == 0 {
                break;
            }
            scanned += page.scanned;
            diagnostics.scanned_body_candidates += page.scanned;
            for candidate in page.candidates {
                after = Some((candidate.created_at, candidate.id));
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
                let decision_started = self.backend_time.instant_now();
                let retention_decision = self
                    .request_log_body_retention_decision_for_gc(
                        settings,
                        candidate.request_user_id.as_deref(),
                        &candidate.result_status,
                        request_value_bucket,
                        retention_contexts,
                        &mut diagnostics,
                    )
                    .await?;
                diagnostics.body_retention_decision_elapsed_ms +=
                    decision_started.elapsed().as_millis();
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
                    if self.backend_time.instant_now() >= deadline {
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
                let write_started = self.backend_time.instant_now();
                let result = loop {
                    let mut conn = self
                        .sqlite_runtime
                        .acquire_operation_connection(SqliteOperation::RequestLogsGc)
                        .await?;
                    let result = sqlx::query(
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
                    .execute(&mut *conn)
                    .await;
                    match result {
                        Ok(result) => {
                            conn.close().await?;
                            break result;
                        }
                        Err(err) => {
                            drop(conn);
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
                diagnostics.body_write_elapsed_ms += write_started.elapsed().as_millis();
                cleaned += result.rows_affected() as i64;
                if cleaned >= batch_size
                    || scanned >= scan_limit
                    || self.backend_time.instant_now() >= deadline
                {
                    has_more = true;
                    break 'scan;
                }
            }
            after = page.last_scanned;
            if page.has_more {
                has_more = true;
            } else {
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

        Ok(RequestLogBodyGcBatch {
            cleaned,
            has_more,
            diagnostics,
        })
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
        let mut blocked_by_integrity = false;
        let mut batches = 0_i64;
        let mut retention_contexts = std::collections::HashMap::new();
        let mut body_gc_diagnostics = RequestLogBodyGcDiagnostics::default();
        while batches < max_batches && self.backend_time.instant_now() < deadline {
            let body_batch = self
                .clear_request_log_body_batch(
                    settings,
                    batch_size,
                    deadline,
                    &mut retention_contexts,
                )
                .await?;
            let raw_delete_cutoff = self
                .dashboard_rollup_integrity_request_log_gc_cutoff(threshold)
                .await?;
            let request_deleted = if let Some(raw_delete_cutoff) = raw_delete_cutoff {
                // Delete only the earliest sealed local day. This prevents one large
                // batch from crossing into a later day that has not been sealed yet.
                self.unlink_old_request_log_references_batch(raw_delete_cutoff, batch_size)
                    .await?;
                self.delete_old_request_logs_batch(raw_delete_cutoff, batch_size)
                    .await?
            } else {
                tracing::debug!(
                    component = "dashboard_rollup_integrity",
                    event = "request_logs_gc_blocked_unsealed_day",
                    threshold,
                    "request log deletion and reference unlinking delayed until its local-day recovery seal exists"
                );
                blocked_by_integrity = true;
                0
            };
            let rollup_deleted = if blocked_by_integrity {
                // Raw request rows and their derived rollups must advance as one
                // retention unit. A missing day seal is not productive work, so
                // do not turn one delayed continuation into five write batches.
                0
            } else {
                self.delete_old_request_log_rollups_batch(threshold, batch_size)
                    .await?
            };
            body_batch_has_more = body_batch.has_more;
            cleaned_request_log_bodies += body_batch.cleaned;
            body_gc_diagnostics.merge(body_batch.diagnostics);
            deleted_request_logs += request_deleted;
            deleted_rollups += rollup_deleted;
            batches += 1;

            if blocked_by_integrity {
                break;
            }

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

        let has_more = blocked_by_integrity
            || self.has_old_request_log_rows(threshold).await?
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
            scanned_body_candidates: body_gc_diagnostics.scanned_body_candidates,
            unique_retention_users: body_gc_diagnostics.unique_retention_users,
            retention_context_cache_hits: body_gc_diagnostics.retention_context_cache_hits,
            body_candidate_query_elapsed_ms: body_gc_diagnostics.body_candidate_query_elapsed_ms,
            body_retention_decision_elapsed_ms: body_gc_diagnostics
                .body_retention_decision_elapsed_ms,
            body_write_elapsed_ms: body_gc_diagnostics.body_write_elapsed_ms,
            progress_status: if !has_more {
                "completed"
            } else if blocked_by_integrity {
                "incomplete_blocked_integrity"
            } else if cleaned_request_log_bodies + deleted_request_logs + deleted_rollups > 0 {
                "incomplete_progress"
            } else {
                "incomplete_zero_progress"
            }
            .to_string(),
        })
    }

}
