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

enum FencedRequestLogDelete {
    Deleted(i64),
    SourceChanged,
}

async fn request_logs_gc_before_deadline<T, F>(
    deadline: tokio::time::Instant,
    future: F,
) -> Result<Option<T>, ProxyError>
where
    F: std::future::Future<Output = Result<T, ProxyError>>,
{
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
        crate::store::wait_for_owned_finishes(&[crate::store::SqliteOperation::RequestLogsGc])
            .await;
        return Ok(None);
    }
    match tokio::time::timeout(remaining, future).await {
        Ok(result) => result.map(Some),
        Err(_) => {
            crate::store::wait_for_owned_finishes(&[crate::store::SqliteOperation::RequestLogsGc])
                .await;
            Ok(None)
        }
    }
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

    pub(crate) fn try_admit_request_logs_gc_recovery(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_bounded_recovery_bulk(SqliteOperation::RequestLogsGc)
    }

    pub(crate) fn try_admit_request_logs_gc_exclusive_recovery(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_exclusive_recovery_bulk(SqliteOperation::RequestLogsGc)
    }

    pub(crate) fn request_logs_gc_continue_defer_reason(
        &self,
    ) -> Option<SqliteAdmissionDeferReason> {
        self.sqlite_runtime.maintenance_bulk_continue_reason()
    }

    /// Serializes request-log GC batches for the whole transaction.
    ///
    /// PostgreSQL has no database-wide write lock, so this advisory lock replaces the
    /// mutual exclusion that the SQLite immediate-write lock used to give the fence read,
    /// the reference unlink, and the bounded delete. The key is derived from a fixed lane
    /// name inside PostgreSQL (`hashtext`), so every process computes the same value.
    async fn lock_request_log_gc_batch(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ProxyError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tavily_hikari.request_logs_gc'))")
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Turns the batch deadline into the transaction's lock wait budget.
    async fn set_gc_statement_budget(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        deadline: tokio::time::Instant,
    ) -> Result<(), ProxyError> {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let millis = (remaining.as_millis() as i64).max(1);
        sqlx::query("SELECT set_config('lock_timeout', $1, true)")
            .bind(format!("{millis}ms"))
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Asserts the indexes the bounded GC pass depends on.
    ///
    /// PostgreSQL owns this schema: `src/store/pg/migrations/0001_baseline.sql` creates all
    /// four indexes, so the startup path only checks their presence instead of issuing DDL.
    pub(crate) async fn ensure_request_logs_gc_support_indexes(&self) -> Result<(), ProxyError> {
        let missing: Option<String> = sqlx::query_scalar(
            r#"
            SELECT required.rel
            FROM unnest($1::text[]) AS required(rel)
            WHERE to_regclass(required.rel) IS NULL
            ORDER BY required.rel
            LIMIT 1
            "#,
        )
        .bind(vec![
            "core.idx_token_logs_request_log_id",
            "core.idx_api_key_maintenance_records_request_log",
            "core.idx_api_key_transient_backoffs_source_request_log",
            "observability.idx_request_logs_time",
        ])
        .fetch_optional(self.pg()?)
        .await?;
        match missing {
            None => Ok(()),
            Some(relation) => Err(ProxyError::Other(format!(
                "postgresql baseline is missing {relation}; apply src/store/pg/migrations first"
            ))),
        }
    }

    async fn delete_old_request_logs_batch_fenced(
        &self,
        guard: DashboardRollupRequestLogGcGuard,
        batch_size: i64,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<FencedRequestLogDelete, ProxyError> {
        // SQLite relied on its immediate-write lock (a database-wide lock) to keep the
        // fence check and the delete in one serialized critical section. In PostgreSQL
        // this batch takes the request-log GC advisory lock for the whole transaction,
        // so concurrent GC passes serialize, and the fence/version equality checks below
        // still prove the day was sealed before any row is removed.
        let mut tx = self.pg()?.begin().await?;
        if let Some(deadline) = deadline {
            Self::set_gc_statement_budget(&mut tx, deadline).await?;
        }
        Self::lock_request_log_gc_batch(&mut tx).await?;
        let result = async {
            sqlx::query(
                "UPDATE observability.dashboard_rollup_gc_delete_state SET active = 1 WHERE id = 1",
            )
            .execute(&mut *tx)
            .await?;
            let current_fence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(guard.day_start)
            .bind(guard.day_end)
            .fetch_one(&mut *tx)
            .await?;
            let current_durable_source_version: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(guard.day_start)
            .bind(guard.day_end)
                .fetch_one(&mut *tx)
                .await?;
            let current_source_version = current_durable_source_version.saturating_add(
                self.request_stats_coalescer
                    .dashboard_rollup_source_version(guard.day_start, guard.day_end)
                    .await,
            );
            if current_fence != guard.source_fence
                || current_source_version != guard.source_version
                || current_durable_source_version != guard.durable_source_version
                || !self
                    .request_stats_coalescer
                    .dashboard_rollup_source_mutations_are_stable(guard.day_start, guard.day_end)
            {
                sqlx::query(
                    "UPDATE observability.dashboard_rollup_gc_delete_state SET active = 0 WHERE id = 1",
                )
                .execute(&mut *tx)
                .await?;
                return Ok(FencedRequestLogDelete::SourceChanged);
            }

            let selected_rows = sqlx::query(
                r#"
                SELECT created_at, visibility, result_status, failure_kind, request_kind_key,
                       request_body, path, business_credits, counts_business_quota
                FROM observability.request_logs
                WHERE created_at >= $1 AND created_at < $2
                ORDER BY CASE WHEN visibility = 'visible' THEN 1 ELSE 0 END ASC, id ASC
                LIMIT $3
                "#,
            )
            .bind(guard.day_start)
            .bind(guard.day_end)
            .bind(batch_size)
            .fetch_all(&mut *tx)
            .await?;
            let mut deleted_source_counts:
                std::collections::BTreeMap<i64, DashboardRequestRollupCounts> =
                std::collections::BTreeMap::new();
            for row in selected_rows {
                let visibility: String = row.try_get("visibility")?;
                if visibility != REQUEST_LOG_VISIBILITY_VISIBLE {
                    continue;
                }
                let created_at: i64 = row.try_get("created_at")?;
                let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                let path: String = row.try_get("path")?;
                let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
                let request_kind_key = canonicalize_request_log_request_kind(
                    &path,
                    request_body.as_deref(),
                    stored_request_kind_key,
                    None,
                    None,
                )
                .key;
                let stored_counts_business_quota: Option<i64> =
                    row.try_get("counts_business_quota")?;
                let counts_business_quota = stored_counts_business_quota
                    .map(|value| value != 0)
                    .unwrap_or_else(|| {
                        request_log_counts_business_quota(&request_kind_key, request_body.as_deref())
                    });
                let result_status: String = row.try_get("result_status")?;
                let failure_kind: Option<String> = row.try_get("failure_kind")?;
                let business_credits: Option<i64> = row.try_get("business_credits")?;
                let minute_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
                deleted_source_counts
                    .entry(minute_start)
                    .or_default()
                    .add(Self::dashboard_rollup_counts_for_request(
                        &request_kind_key,
                        request_body.as_deref(),
                        &result_status,
                        failure_kind.as_deref(),
                        business_credits.unwrap_or_default(),
                        counts_business_quota,
                    ));
            }
            for (bucket_start, counts) in deleted_source_counts {
                let existing_json: Option<String> = sqlx::query_scalar(
                    "SELECT counts_json FROM observability.dashboard_rollup_gc_deleted_source_contributions WHERE bucket_start = $1",
                )
                .bind(bucket_start)
                .fetch_optional(&mut *tx)
                .await?;
                let mut total = match existing_json {
                    Some(existing_json) => serde_json::from_str(&existing_json).map_err(|err| {
                        ProxyError::Other(format!(
                            "invalid deleted dashboard source contribution: {err}"
                        ))
                    })?,
                    None => DashboardRequestRollupCounts::default(),
                };
                total.add(counts);
                let counts_json = serde_json::to_string(&total).map_err(|err| {
                    ProxyError::Other(format!("serialize deleted dashboard source contribution: {err}"))
                })?;
                sqlx::query(
                    r#"
                    INSERT INTO observability.dashboard_rollup_gc_deleted_source_contributions (
                        bucket_start, counts_json, updated_at
                    ) VALUES ($1, $2, $3)
                    ON CONFLICT(bucket_start) DO UPDATE SET
                        counts_json = excluded.counts_json,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(bucket_start)
                .bind(counts_json)
                .bind(self.backend_time.now_ts())
                .execute(&mut *tx)
                .await?;
            }
            for sql in [
                r#"
                    UPDATE core.auth_token_logs
                    SET request_log_id = NULL
                    WHERE request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE created_at >= $1 AND created_at < $2
                        ORDER BY CASE WHEN visibility = 'visible' THEN 1 ELSE 0 END ASC, id ASC LIMIT $3
                    )
                    "#,
                r#"
                    UPDATE core.api_key_maintenance_records
                    SET request_log_id = NULL
                    WHERE request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE created_at >= $1 AND created_at < $2
                        ORDER BY CASE WHEN visibility = 'visible' THEN 1 ELSE 0 END ASC, id ASC LIMIT $3
                    )
                    "#,
                r#"
                    UPDATE core.api_key_transient_backoffs
                    SET source_request_log_id = NULL
                    WHERE source_request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE created_at >= $1 AND created_at < $2
                        ORDER BY CASE WHEN visibility = 'visible' THEN 1 ELSE 0 END ASC, id ASC LIMIT $3
                    )
                    "#,
            ] {
                sqlx::query(sql)
                    .bind(guard.day_start)
                    .bind(guard.day_end)
                    .bind(batch_size)
                    .execute(&mut *tx)
                    .await?;
            }
            let deleted = sqlx::query(
                r#"
                DELETE FROM observability.request_logs
                WHERE id IN (
                    SELECT id
                    FROM observability.request_logs
                    WHERE created_at >= $1 AND created_at < $2
                    ORDER BY CASE WHEN visibility = 'visible' THEN 1 ELSE 0 END ASC, id ASC
                    LIMIT $3
                )
                "#,
            )
            .bind(guard.day_start)
            .bind(guard.day_end)
            .bind(batch_size)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE observability.dashboard_rollup_gc_delete_state SET active = 0 WHERE id = 1",
            )
            .execute(&mut *tx)
            .await?;
            Ok::<_, ProxyError>(FencedRequestLogDelete::Deleted(
                deleted.rows_affected() as i64,
            ))
        }
        .await;
        match result {
            Ok(result) => {
                tx.commit().await?;
                Ok(result)
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

    async fn delete_old_request_logs_batch_hidden_only(
        &self,
        threshold: i64,
        batch_size: i64,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<i64, ProxyError> {
        // Same serialization contract as the sealed-day batch: the GC advisory lock holds
        // the unlink + delete critical section that the SQLite immediate-write lock used to
        // provide.
        let mut tx = self.pg()?.begin().await?;
        if let Some(deadline) = deadline {
            Self::set_gc_statement_budget(&mut tx, deadline).await?;
        }
        Self::lock_request_log_gc_batch(&mut tx).await?;
        let result = async {
            sqlx::query(
                "UPDATE observability.dashboard_rollup_gc_delete_state SET active = 1 WHERE id = 1",
            )
            .execute(&mut *tx)
            .await?;
            for sql in [
                r#"
                    UPDATE core.auth_token_logs
                    SET request_log_id = NULL
                    WHERE request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE visibility <> 'visible' AND created_at < $1
                        ORDER BY id ASC LIMIT $2
                    )
                    "#,
                r#"
                    UPDATE core.api_key_maintenance_records
                    SET request_log_id = NULL
                    WHERE request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE visibility <> 'visible' AND created_at < $1
                        ORDER BY id ASC LIMIT $2
                    )
                    "#,
                r#"
                    UPDATE core.api_key_transient_backoffs
                    SET source_request_log_id = NULL
                    WHERE source_request_log_id IN (
                        SELECT id FROM observability.request_logs
                        WHERE visibility <> 'visible' AND created_at < $1
                        ORDER BY id ASC LIMIT $2
                    )
                    "#,
            ] {
                sqlx::query(sql)
                    .bind(threshold)
                    .bind(batch_size)
                    .execute(&mut *tx)
                    .await?;
            }
            let deleted = sqlx::query(
                r#"
                DELETE FROM observability.request_logs
                WHERE id IN (
                    SELECT id
                    FROM observability.request_logs
                    WHERE visibility <> 'visible' AND created_at < $1
                    ORDER BY id ASC
                    LIMIT $2
                )
                "#,
            )
            .bind(threshold)
            .bind(batch_size)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE observability.dashboard_rollup_gc_delete_state SET active = 0 WHERE id = 1",
            )
            .execute(&mut *tx)
            .await?;
            Ok::<_, ProxyError>(deleted.rows_affected() as i64)
        }
        .await;
        match result {
            Ok(deleted) => {
                tx.commit().await?;
                Ok(deleted)
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

    async fn delete_old_request_log_rollups_batch(
        &self,
        threshold: i64,
        batch_size: i64,
        target_day: Option<(i64, i64)>,
    ) -> Result<i64, ProxyError> {
        let rollup_table_exists: bool = sqlx::query_scalar(
            "SELECT to_regclass('observability.request_log_catalog_rollups') IS NOT NULL",
        )
        .fetch_one(self.pg()?)
        .await?;
        if !rollup_table_exists {
            return Ok(0);
        }
        let deadline = self.backend_time.deadline_after(Duration::from_secs(10));
        let mut retry_attempt = 0usize;
        loop {
            let mut conn = self
                .sqlite_runtime
                .acquire_operation_connection(SqliteOperation::RequestLogsGc)
                .await?;
            let result = match target_day {
                Some((day_start, day_end)) => {
                    sqlx::query(
                        r#"
                        DELETE FROM observability.request_log_catalog_rollups
                        WHERE (
                            bucket_start,
                            request_kind_key,
                            request_kind_label,
                            result_bucket,
                            key_effect_code,
                            binding_effect_code,
                            selection_effect_code,
                            auth_token_id,
                            api_key_id,
                            operational_class
                        ) IN (
                            SELECT
                                bucket_start,
                                request_kind_key,
                                request_kind_label,
                                result_bucket,
                                key_effect_code,
                                binding_effect_code,
                                selection_effect_code,
                                auth_token_id,
                                api_key_id,
                                operational_class
                            FROM observability.request_log_catalog_rollups
                            WHERE bucket_start >= $1 AND bucket_start < $2 AND bucket_start < $3
                            ORDER BY bucket_start ASC
                            LIMIT $4
                        )
                        "#,
                    )
                    .bind(day_start)
                    .bind(day_end)
                    .bind(threshold)
                    .bind(batch_size)
                    .execute(&mut *conn)
                    .await
                }
                None => {
                    sqlx::query(
                        r#"
                        DELETE FROM observability.request_log_catalog_rollups
                        WHERE (
                            bucket_start,
                            request_kind_key,
                            request_kind_label,
                            result_bucket,
                            key_effect_code,
                            binding_effect_code,
                            selection_effect_code,
                            auth_token_id,
                            api_key_id,
                            operational_class
                        ) IN (
                            SELECT
                                bucket_start,
                                request_kind_key,
                                request_kind_label,
                                result_bucket,
                                key_effect_code,
                                binding_effect_code,
                                selection_effect_code,
                                auth_token_id,
                                api_key_id,
                                operational_class
                            FROM observability.request_log_catalog_rollups
                            WHERE bucket_start < $1
                            ORDER BY bucket_start ASC
                            LIMIT $2
                        )
                        "#,
                    )
                    .bind(threshold)
                    .bind(batch_size)
                    .execute(&mut *conn)
                    .await
                }
            };
            match result
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

    async fn has_old_request_log_rows(
        &self,
        threshold: i64,
        target_day: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let exists = match target_day {
            Some((day_start, day_end)) => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM observability.request_logs WHERE created_at >= $1 AND created_at < $2 AND created_at < $3 LIMIT 1",
                )
                .bind(day_start)
                .bind(day_end)
                .bind(threshold)
                .fetch_optional(&mut *conn)
                .await?
            }
            None => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM observability.request_logs WHERE created_at < $1 LIMIT 1",
                )
                .bind(threshold)
                .fetch_optional(&mut *conn)
                .await?
            }
        };
        conn.close().await?;
        Ok(exists.is_some())
    }

    async fn has_old_request_log_rollup_rows(
        &self,
        threshold: i64,
        target_day: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::RequestLogsGc)
            .await?;
        let rollup_table_exists: bool = sqlx::query_scalar(
            "SELECT to_regclass('observability.request_log_catalog_rollups') IS NOT NULL",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !rollup_table_exists {
            conn.close().await?;
            return Ok(false);
        }
        let exists = match target_day {
            Some((day_start, day_end)) => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM observability.request_log_catalog_rollups WHERE bucket_start >= $1 AND bucket_start < $2 AND bucket_start < $3 LIMIT 1",
                )
                .bind(day_start)
                .bind(day_end)
                .bind(threshold)
                .fetch_optional(&mut *conn)
                .await?
            }
            None => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM observability.request_log_catalog_rollups WHERE bucket_start < $1 LIMIT 1",
                )
                .bind(threshold)
                .fetch_optional(&mut *conn)
                .await?
            }
        };
        conn.close().await?;
        Ok(exists.is_some())
    }

    fn map_request_log_body_gc_candidate<R>(
        row: R,
    ) -> Result<RequestLogBodyGcCandidate, sqlx::Error>
    where
        R: sqlx::Row,
        for<'r> &'r str: sqlx::ColumnIndex<R>,
        for<'r> i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        for<'r> Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        for<'r> Vec<u8>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        for<'r> Option<Vec<u8>>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        for<'r> Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
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
                FROM observability.request_logs
                WHERE created_at >= $1
                  AND (created_at > $2 OR (created_at = $3 AND id > $4))
                ORDER BY created_at ASC, id ASC
                LIMIT $5
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
                FROM observability.request_logs
                WHERE created_at >= $1
                ORDER BY created_at ASC, id ASC
                LIMIT $2
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
                FROM observability.request_logs
                WHERE created_at >= $1
                  AND (created_at > $2 OR (created_at = $3 AND id > $4))
                  AND (created_at < $5 OR (created_at = $6 AND id <= $7))
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
                FROM observability.request_logs
                WHERE created_at >= $1
                  AND (created_at < $2 OR (created_at = $3 AND id <= $4))
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
            "SELECT value FROM core.meta WHERE key = $1 LIMIT 1",
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
        // The cursor row is the only durable fence for the body scan; the GC advisory
        // lock serializes concurrent GC passes now that there is no database-wide writer.
        let mut tx = self.pg()?.begin().await?;
        Self::lock_request_log_gc_batch(&mut tx).await?;
        let result = if let Some(cursor) = cursor {
            let value = if let Some(restart_at) = cursor.restart_at {
                format!("{}:{}:{}", cursor.created_at, cursor.id, restart_at)
            } else {
                format!("{}:{}", cursor.created_at, cursor.id)
            };
            sqlx::query(
                r#"
                INSERT INTO core.meta (key, value)
                VALUES ($1, $2)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
            .bind(value)
            .execute(&mut *tx)
            .await
        } else {
            sqlx::query("DELETE FROM core.meta WHERE key = $1")
                .bind(META_KEY_REQUEST_LOG_BODY_GC_CURSOR_V1)
                .execute(&mut *tx)
                .await
        };
        match result {
            Ok(_) => {
                tx.commit().await?;
                Ok(())
            }
            Err(err) => {
                tx.rollback().await?;
                Err(ProxyError::Database(err))
            }
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
                            request_kind_key = $1,
                            request_kind_label = $2,
                            request_kind_detail = $3,
                            counts_business_quota = COALESCE(counts_business_quota, $4),
                            request_body_bytes = COALESCE(request_body_bytes, $5),
                            response_body_bytes = COALESCE(response_body_bytes, $6),
                            request_body_sha256 = COALESCE(request_body_sha256, $7),
                            response_body_sha256 = COALESCE(response_body_sha256, $8),
                            body_retention_days = $9,
                            body_retention_profile = $10,
                            body_cleaned_reason = $11,
                            body_cleaned_at = $12
                        WHERE id = $13 AND (request_body IS NOT NULL OR response_body IS NOT NULL)
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
        if self.backend_time.instant_now() >= deadline && after.is_some() {
            has_more = true;
        }
        let persisted_cursor = after
            .filter(|_| has_more || restart_at.is_some())
            .map(|(created_at, id)| RequestLogBodyGcCursor {
                created_at,
                id,
                restart_at,
            });
        if after.is_some() && self.backend_time.instant_now() < deadline {
            self.set_request_log_body_gc_cursor(persisted_cursor).await?;
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
        self.delete_old_request_logs_bounded_with_deadline_scoped(
            threshold,
            options,
            retention_days,
            settings,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_old_request_logs_bounded_for_recovery_target_with_deadline(
        &self,
        threshold: i64,
        options: RequestLogsGcOptions,
        retention_days: i64,
        settings: &RequestLogRetentionSettings,
        target_day_start: i64,
        target_day_end: i64,
        outer_deadline: Option<tokio::time::Instant>,
    ) -> Result<RequestLogsGcReport, ProxyError> {
        self.delete_old_request_logs_bounded_with_deadline_scoped(
            threshold,
            options,
            retention_days,
            settings,
            outer_deadline,
            Some((target_day_start, target_day_end)),
        )
        .await
    }

    async fn delete_old_request_logs_bounded_with_deadline_scoped(
        &self,
        threshold: i64,
        options: RequestLogsGcOptions,
        retention_days: i64,
        settings: &RequestLogRetentionSettings,
        outer_deadline: Option<tokio::time::Instant>,
        target_day: Option<(i64, i64)>,
    ) -> Result<RequestLogsGcReport, ProxyError> {
        let batch_size = options.batch_size.max(1);
        let max_batches = options.max_batches.max(1);
        let local_deadline = self
            .backend_time
            .deadline_after(Duration::from_secs(options.max_runtime_secs));
        let deadline = outer_deadline.map_or(local_deadline, |outer| outer.min(local_deadline));
        let started = self.backend_time.instant_now();
        let mut cleaned_request_log_bodies = 0_i64;
        let mut deleted_request_logs = 0_i64;
        let mut deleted_rollups = 0_i64;
        let mut body_batch_has_more = false;
        let initial_body_cursor = request_logs_gc_before_deadline(
            deadline,
            self.get_request_log_body_gc_cursor(),
        )
        .await?
        .flatten()
        .map(|cursor| (cursor.created_at, cursor.id));
        let mut blocked_by_integrity = false;
        let mut blocked_day_start = None;
        let mut blocked_reason = None;
        let mut batches = 0_i64;
        let mut retention_contexts = std::collections::HashMap::new();
        let mut body_gc_diagnostics = RequestLogBodyGcDiagnostics::default();
        let mut deadline_exhausted = false;
        while batches < max_batches && self.backend_time.instant_now() < deadline {
            let Some(body_batch) = request_logs_gc_before_deadline(
                deadline,
                self.clear_request_log_body_batch(
                    settings,
                    batch_size,
                    deadline,
                    &mut retention_contexts,
                ),
            )
            .await?
            else {
                deadline_exhausted = true;
                body_batch_has_more = true;
                break;
            };
            body_batch_has_more = body_batch.has_more;
            cleaned_request_log_bodies += body_batch.cleaned;
            body_gc_diagnostics.merge(body_batch.diagnostics);
            if self.backend_time.instant_now() >= deadline {
                deadline_exhausted = true;
                break;
            }
            let raw_delete_cutoff = match target_day {
                Some((target_day_start, target_day_end)) => self
                    .dashboard_rollup_integrity_request_log_gc_decision_for_day_before_deadline(
                        threshold,
                        target_day_start,
                        target_day_end,
                        deadline,
                    )
                    .await?,
                None => self
                    .dashboard_rollup_integrity_request_log_gc_decision_before_deadline(
                        threshold,
                        deadline,
                    )
                    .await?,
            };
            if self.backend_time.instant_now() >= deadline {
                deadline_exhausted = true;
                break;
            }
            let request_deleted = match raw_delete_cutoff {
                DashboardRollupRequestLogGcDecision::Allowed {
                    cutoff,
                    guard: Some(guard),
                } => {
                    debug_assert_eq!(cutoff, guard.day_end);
                    match request_logs_gc_before_deadline(
                        deadline,
                        self.delete_old_request_logs_batch_fenced(
                            guard,
                            batch_size,
                            Some(deadline),
                        ),
                    )
                    .await?
                    {
                        Some(FencedRequestLogDelete::Deleted(deleted)) => deleted,
                        Some(FencedRequestLogDelete::SourceChanged) => {
                            blocked_day_start = Some(guard.day_start);
                            blocked_reason = Some("source_changed_during_delete".to_string());
                            blocked_by_integrity = true;
                            0
                        }
                        None => {
                            deadline_exhausted = true;
                            0
                        }
                    }
                }
                DashboardRollupRequestLogGcDecision::Allowed { cutoff, guard: None } => {
                    match request_logs_gc_before_deadline(
                        deadline,
                    self.delete_old_request_logs_batch_hidden_only(
                            cutoff.min(threshold),
                            batch_size,
                            Some(deadline),
                        ),
                    )
                    .await?
                    {
                        Some(deleted) => deleted,
                        None => {
                            deadline_exhausted = true;
                            0
                        }
                    }
                }
                DashboardRollupRequestLogGcDecision::Blocked { day_start, reason } => {
                    tracing::debug!(
                        component = "dashboard_rollup_integrity",
                        event = "request_logs_gc_blocked_unsealed_day",
                        threshold,
                        "request log deletion and reference unlinking delayed until its local-day recovery seal exists"
                    );
                    blocked_day_start = Some(day_start);
                    blocked_reason = Some(reason.to_string());
                    blocked_by_integrity = true;
                    0
                }
            };
            if self.backend_time.instant_now() >= deadline {
                deadline_exhausted = true;
                break;
            }
            let rollup_deleted = if blocked_by_integrity {
                // Raw request rows and their derived rollups must advance as one
                // retention unit. A missing day seal is not productive work, so
                // do not turn one delayed continuation into five write batches.
                0
            } else {
                match request_logs_gc_before_deadline(
                        deadline,
                        self.delete_old_request_log_rollups_batch(
                            threshold,
                            batch_size,
                            target_day,
                        ),
                    )
                .await?
                {
                    Some(deleted) => deleted,
                    None => {
                        deadline_exhausted = true;
                        0
                    }
                }
            };
            deleted_request_logs += request_deleted;
            deleted_rollups += rollup_deleted;
            batches += 1;

            if deadline_exhausted {
                break;
            }

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
                let sleep_for = Duration::from_millis(options.inter_batch_sleep_ms);
                let remaining = deadline.saturating_duration_since(self.backend_time.instant_now());
                if remaining <= sleep_for {
                    break;
                }
                self.backend_time.sleep(sleep_for).await;
            }
        }

        let has_more = if deadline_exhausted {
            true
        } else {
            let raw_has_more = request_logs_gc_before_deadline(
                deadline,
                self.has_old_request_log_rows(threshold, target_day),
            )
            .await?
            .unwrap_or(true);
            let rollup_has_more = request_logs_gc_before_deadline(
                deadline,
                self.has_old_request_log_rollup_rows(threshold, target_day),
            )
            .await?
            .unwrap_or(true);
            blocked_by_integrity || raw_has_more || rollup_has_more || body_batch_has_more
        };
        self.invalidate_request_logs_catalog_cache().await;
        let body_scan_cursor_advanced = request_logs_gc_before_deadline(
            deadline,
            self.get_request_log_body_gc_cursor(),
        )
        .await?
        .flatten()
            .is_some_and(|cursor| initial_body_cursor
                .is_none_or(|initial| (cursor.created_at, cursor.id) > initial));
        let progress_status = if !has_more {
            "completed"
        } else if blocked_by_integrity {
            "incomplete_blocked_integrity"
        } else if cleaned_request_log_bodies + deleted_request_logs + deleted_rollups > 0 {
            "incomplete_progress"
        } else {
            "incomplete_zero_progress"
        };
        let committed_range_rows = cleaned_request_log_bodies
            .saturating_add(deleted_request_logs)
            .saturating_add(deleted_rollups)
            .max(0) as u64;
        self.sqlite_runtime.record_committed_maintenance_progress(
            body_scan_cursor_advanced,
            committed_range_rows,
            !has_more,
        );
        tracing::debug!(
            component = "request_logs_gc",
            event = "bounded_pass_completed",
            lane = "request_log_gc",
            target_threshold = threshold,
            batches,
            cleaned_request_log_bodies,
            deleted_request_logs,
            deleted_rollups,
            blocked_day_start = ?blocked_day_start,
            blocked_reason = ?blocked_reason,
            body_scan_cursor_advanced,
            progress_status,
            "completed a bounded request-log GC pass"
        );
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
            body_scan_cursor_advanced,
            unique_retention_users: body_gc_diagnostics.unique_retention_users,
            retention_context_cache_hits: body_gc_diagnostics.retention_context_cache_hits,
            body_candidate_query_elapsed_ms: body_gc_diagnostics.body_candidate_query_elapsed_ms,
            body_retention_decision_elapsed_ms: body_gc_diagnostics
                .body_retention_decision_elapsed_ms,
            body_write_elapsed_ms: body_gc_diagnostics.body_write_elapsed_ms,
            blocked_day_start,
            blocked_reason,
            progress_status: progress_status.to_string(),
        })
    }

}
