use std::collections::BTreeMap;
use std::time::Instant as StdInstant;

const DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS: i64 = SECS_PER_FIVE_MINUTES;
const DASHBOARD_ROLLUP_INTEGRITY_SOURCE_PAGE_ROWS: i64 = 500;
const DASHBOARD_ROLLUP_INTEGRITY_READ_BUDGET: Duration = Duration::from_millis(150);
const DASHBOARD_ROLLUP_INTEGRITY_WRITE_TARGET: Duration = Duration::from_millis(100);
const DASHBOARD_ROLLUP_INTEGRITY_WRITE_WARN: Duration = Duration::from_millis(250);
const DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS: i64 = SECS_PER_DAY;
const DASHBOARD_ROLLUP_INTEGRITY_STALLED_SECS: i64 = 2 * SECS_PER_HOUR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DashboardRollupIntegritySlice {
    Verified { next_delay_secs: i64 },
    Deferred { next_delay_secs: i64 },
    Repaired { next_delay_secs: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardRollupIntegrityWorkKind {
    InitialHot,
    HotReaudit,
    History,
    SealedDayReaudit { day_start: i64 },
}

#[derive(Debug)]
struct DashboardRollupIntegrityWorkItem {
    range_start: i64,
    range_end: i64,
    source_fence_id: i64,
    source_version: i64,
    cursor_created_at: Option<i64>,
    cursor_id: Option<i64>,
    counts: BTreeMap<i64, DashboardRequestRollupCounts>,
}

#[derive(Default)]
struct DashboardRollupIntegrityStateRow {
    last_verified_at: Option<i64>,
    stalled_since: Option<i64>,
    next_attempt_at: Option<i64>,
    hot_cursor: Option<i64>,
    hot_fence: Option<i64>,
    history_cursor: Option<i64>,
}

impl DashboardRollupIntegrityWorkItem {
    fn empty(
        range_start: i64,
        range_end: i64,
        source_fence_id: i64,
        source_version: i64,
    ) -> Self {
        Self {
            range_start,
            range_end,
            source_fence_id,
            source_version,
            cursor_created_at: None,
            cursor_id: None,
            counts: BTreeMap::new(),
        }
    }
}

impl KeyStore {
    pub(crate) async fn reset_dashboard_rollup_integrity_pending_work_on_startup(
        &self,
    ) -> Result<(), ProxyError> {
        let source_fence_id: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM request_logs")
                .fetch_one(&self.pool)
                .await?;
        let now = self.backend_time.now_ts();
        let empty_counts = serde_json::to_string(&BTreeMap::<i64, DashboardRequestRollupCounts>::new())
            .map_err(|err| ProxyError::Other(format!("serialize reset integrity work item: {err}")))?;
        sqlx::query(
            r#"
            UPDATE dashboard_rollup_integrity_work_items
            SET source_fence = ?, source_version = 0, cursor_created_at = NULL, cursor_id = NULL,
                counts_json = ?, status = 'pending', updated_at = ?
            WHERE status = 'pending'
            "#,
        )
        .bind(source_fence_id)
        .bind(empty_counts)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn dashboard_rollup_integrity_status(
        &self,
    ) -> Result<DashboardRollupIntegrityStatus, ProxyError> {
        let now = self.backend_time.now_ts();
        let state = sqlx::query(
            r#"
            SELECT last_verified_at, stalled_since, next_attempt_at, hot_cursor, hot_fence, history_cursor
            FROM dashboard_rollup_integrity_state
            WHERE id = 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;
        let unverified_bucket_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM (
                SELECT range_start FROM dashboard_rollup_integrity_gaps
                UNION
                SELECT range_start FROM dashboard_rollup_integrity_work_items WHERE status = 'pending'
                UNION
                SELECT bucket_start FROM dashboard_rollup_integrity_day_reaudits WHERE status = 'pending'
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        let DashboardRollupIntegrityStateRow {
            last_verified_at,
            stalled_since,
            next_attempt_at,
            hot_cursor,
            hot_fence,
            history_cursor,
        } = state
            .map(|row| {
                Ok::<_, sqlx::Error>(DashboardRollupIntegrityStateRow {
                    last_verified_at: row.try_get("last_verified_at")?,
                    stalled_since: row.try_get("stalled_since")?,
                    next_attempt_at: row.try_get("next_attempt_at")?,
                    hot_cursor: row.try_get("hot_cursor")?,
                    hot_fence: row.try_get("hot_fence")?,
                    history_cursor: row.try_get("history_cursor")?,
                })
            })
            .transpose()?
            .unwrap_or_default();
        let oldest_visible_log: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(created_at) FROM request_logs WHERE visibility = ?",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_one(&self.pool)
        .await?;
        let hot_backlog_bucket_count = hot_cursor
            .zip(hot_fence)
            .map(|(cursor, fence)| {
                let pending_seconds = fence.saturating_sub(cursor).max(0);
                pending_seconds
                    .saturating_add(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS - 1)
                    / DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS
            })
            .unwrap_or_default();
        let history_backlog_bucket_count = history_cursor
            .zip(oldest_visible_log)
            .map(|(cursor, oldest)| {
                let pending_seconds = cursor.saturating_sub(oldest).max(0);
                pending_seconds
                    .saturating_add(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS - 1)
                    / DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS
            })
            .unwrap_or_default();
        let history_incomplete = history_backlog_bucket_count > 0;
        let audit_incomplete = hot_backlog_bucket_count > 0 || history_incomplete;
        let state = if stalled_since
            .map(|started| now.saturating_sub(started) >= DASHBOARD_ROLLUP_INTEGRITY_STALLED_SECS)
            .unwrap_or(false)
        {
            "degraded"
        } else if unverified_bucket_count > 0 || audit_incomplete || last_verified_at.is_none() {
            "repairing"
        } else {
            "healthy"
        };
        Ok(DashboardRollupIntegrityStatus {
            state: state.to_string(),
            last_verified_at,
            next_attempt_at,
            unverified_bucket_count: unverified_bucket_count
                + hot_backlog_bucket_count
                + history_backlog_bucket_count,
        })
    }

    pub(crate) async fn run_dashboard_rollup_integrity_slice(
        &self,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        let now = self.backend_time.now_ts();
        self.ensure_dashboard_rollup_integrity_state(now).await?;
        if let Some(item) = self.load_dashboard_rollup_integrity_work_item().await? {
            return self.process_dashboard_rollup_integrity_work_item(item, now).await;
        }
        if self.dashboard_rollup_integrity_seal_verification_due(now).await? {
            self.verify_next_dashboard_rollup_daily_seal(now).await?;
            self.mark_dashboard_rollup_integrity_seal_attempt(now).await?;
            if let Some(item) = self.load_dashboard_rollup_integrity_work_item().await? {
                return self.process_dashboard_rollup_integrity_work_item(item, now).await;
            }
        }
        if let Some(item) = self
            .create_next_dashboard_rollup_integrity_work_item(now)
            .await?
        {
            return self.process_dashboard_rollup_integrity_work_item(item, now).await;
        }
        self.mark_dashboard_rollup_integrity_success(now, 60).await?;
        Ok(DashboardRollupIntegritySlice::Verified {
            next_delay_secs: 60,
        })
    }

    async fn ensure_dashboard_rollup_integrity_state(&self, now: i64) -> Result<(), ProxyError> {
        let fence = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        let hot_start = fence.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS);
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO dashboard_rollup_integrity_state (
                id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
                last_history_attempt_at, last_day_reaudit_attempt_at, last_seal_attempt_at,
                seal_cursor, updated_at
            ) VALUES (1, ?, ?, ?, ?, NULL, NULL, NULL, NULL, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(hot_start)
        .bind(fence)
        .bind(hot_start)
        .bind(hot_start)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn load_dashboard_rollup_integrity_work_item(
        &self,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT range_start, range_end, source_fence, source_version, cursor_created_at, cursor_id, counts_json
            FROM dashboard_rollup_integrity_work_items
            WHERE status = 'pending'
            ORDER BY updated_at ASC, range_start ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let counts_json: String = row.try_get("counts_json")?;
            let counts = serde_json::from_str(&counts_json).map_err(|err| {
                sqlx::Error::Protocol(format!("invalid dashboard integrity work item: {err}"))
            })?;
            Ok::<DashboardRollupIntegrityWorkItem, sqlx::Error>(DashboardRollupIntegrityWorkItem {
                range_start: row.try_get("range_start")?,
                range_end: row.try_get("range_end")?,
                source_fence_id: row.try_get("source_fence")?,
                source_version: row.try_get("source_version")?,
                cursor_created_at: row.try_get("cursor_created_at")?,
                cursor_id: row.try_get("cursor_id")?,
                counts,
            })
        })
        .transpose()
        .map_err(Into::into)
    }

    async fn create_next_dashboard_rollup_integrity_work_item(
        &self,
        now: i64,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
                   last_history_attempt_at, last_day_reaudit_attempt_at
            FROM dashboard_rollup_integrity_state WHERE id = 1
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        let hot_cursor: i64 = row.try_get("hot_cursor")?;
        let hot_fence: i64 = row.try_get("hot_fence")?;
        let hot_reaudit_cursor: Option<i64> = row.try_get("hot_reaudit_cursor")?;
        let history_cursor: i64 = row.try_get("history_cursor")?;
        let last_history_attempt_at: Option<i64> = row.try_get("last_history_attempt_at")?;
        let last_day_reaudit_attempt_at: Option<i64> =
            row.try_get("last_day_reaudit_attempt_at")?;
        let sealed_day_reaudit = sqlx::query(
            r#"
            SELECT bucket_start, bucket_end, cursor
            FROM dashboard_rollup_integrity_day_reaudits
            WHERE status = 'pending'
            ORDER BY updated_at ASC, bucket_start ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        let oldest: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(created_at) FROM request_logs WHERE visibility = ?",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_one(&self.pool)
        .await?;
        let history_due = last_history_attempt_at
            .map(|attempted| now.saturating_sub(attempted) >= 60)
            .unwrap_or(true);
        let history_floor = oldest.map(local_day_bucket_start_utc_ts);
        let can_scan_history = history_floor
            .map(|floor| history_cursor > floor && history_due)
            .unwrap_or(false);
        let should_scan_history = can_scan_history && hot_cursor >= hot_fence;

        let hot_start = latest_closed.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS);
        let hot_is_behind = hot_cursor < hot_fence || hot_fence < latest_closed;
        let day_reaudit_due = last_day_reaudit_attempt_at
            .map(|attempted| now.saturating_sub(attempted) >= 60)
            .unwrap_or(true);
        let day_reaudit = if !hot_is_behind && day_reaudit_due {
            sealed_day_reaudit
        } else {
            None
        };
        let (range_start, range_end, kind) = if let Some(day_reaudit) = day_reaudit {
            let day_start: i64 = day_reaudit.try_get("bucket_start")?;
            let day_end: i64 = day_reaudit.try_get("bucket_end")?;
            let cursor: i64 = day_reaudit.try_get("cursor")?;
            if cursor >= day_end {
                return Ok(None);
            }
            (
                cursor,
                (cursor + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(day_end),
                DashboardRollupIntegrityWorkKind::SealedDayReaudit { day_start },
            )
        } else if hot_cursor < hot_fence && !should_scan_history {
            (
                hot_cursor,
                (hot_cursor + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(hot_fence),
                DashboardRollupIntegrityWorkKind::InitialHot,
            )
        } else if should_scan_history {
            (
                history_cursor.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS),
                history_cursor,
                DashboardRollupIntegrityWorkKind::History,
            )
        } else if hot_cursor < hot_fence {
            (
                hot_cursor,
                (hot_cursor + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(hot_fence),
                DashboardRollupIntegrityWorkKind::InitialHot,
            )
        } else if hot_fence < latest_closed {
            (
                hot_fence.max(hot_start),
                (hot_fence + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(latest_closed),
                DashboardRollupIntegrityWorkKind::InitialHot,
            )
        } else {
            let reauditing = hot_reaudit_cursor.unwrap_or(hot_start).max(hot_start);
            if reauditing >= latest_closed {
                return Ok(None);
            }
            (
                reauditing,
                (reauditing + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(latest_closed),
                DashboardRollupIntegrityWorkKind::HotReaudit,
            )
        };
        if range_end <= range_start {
            return Ok(None);
        }
        // A task only aggregates rows that existed before it was created. Late writes
        // with an old created_at are checked by the next rolling hot pass.
        let source_fence_id: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM request_logs")
                .fetch_one(&self.pool)
                .await?;
        let source_version = self
            .request_stats_coalescer
            .dashboard_rollup_source_version(range_start)
            .await;
        let item = DashboardRollupIntegrityWorkItem::empty(
            range_start,
            range_end,
            source_fence_id,
            source_version,
        );
        let counts_json = serde_json::to_string(&item.counts)
            .map_err(|err| ProxyError::Other(format!("serialize integrity work item: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = async {
            sqlx::query(
                r#"
                INSERT INTO dashboard_rollup_integrity_work_items (
                    range_start, range_end, source_fence, source_version, cursor_created_at, cursor_id, counts_json, status, updated_at
                ) VALUES (?, ?, ?, ?, NULL, NULL, ?, 'pending', ?)
                ON CONFLICT(range_start) DO UPDATE SET
                    range_end = excluded.range_end,
                    source_fence = excluded.source_fence,
                    source_version = excluded.source_version,
                    cursor_created_at = NULL,
                    cursor_id = NULL,
                    counts_json = excluded.counts_json,
                    status = 'pending',
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(item.range_start)
            .bind(item.range_end)
            .bind(item.source_fence_id)
            .bind(item.source_version)
            .bind(counts_json)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            match kind {
                DashboardRollupIntegrityWorkKind::InitialHot => sqlx::query(
                    "UPDATE dashboard_rollup_integrity_state SET hot_cursor = ?, hot_fence = ?, updated_at = ? WHERE id = 1",
                )
                .bind(range_end)
                .bind(latest_closed)
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::History => sqlx::query(
                    "UPDATE dashboard_rollup_integrity_state SET history_cursor = ?, last_history_attempt_at = ?, updated_at = ? WHERE id = 1",
                )
                .bind(range_start)
                .bind(now)
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::HotReaudit => sqlx::query(
                    "UPDATE dashboard_rollup_integrity_state SET hot_reaudit_cursor = ?, updated_at = ? WHERE id = 1",
                )
                .bind(if range_end >= latest_closed { hot_start } else { range_end })
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::SealedDayReaudit { day_start } => sqlx::query(
                    "UPDATE dashboard_rollup_integrity_day_reaudits SET cursor = ?, updated_at = ? WHERE bucket_start = ? AND status = 'pending'",
                )
                .bind(range_end)
                .bind(now)
                .bind(day_start)
                .execute(&mut *conn)
                .await?,
            };
            if matches!(kind, DashboardRollupIntegrityWorkKind::SealedDayReaudit { .. }) {
                sqlx::query(
                    "UPDATE dashboard_rollup_integrity_state SET last_day_reaudit_attempt_at = ?, updated_at = ? WHERE id = 1",
                )
                .bind(now)
                .bind(now)
                .execute(&mut *conn)
                .await?;
            }
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(Some(item))
    }

    async fn process_dashboard_rollup_integrity_work_item(
        &self,
        mut item: DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        let next_delay_secs = self
            .dashboard_rollup_integrity_work_delay(item.range_start)
            .await?;
        let read_started = StdInstant::now();
        let rows = sqlx::query(
            r#"
            SELECT id, created_at, result_status, failure_kind, request_kind_key, request_body,
                   path, business_credits, counts_business_quota
            FROM request_logs
            WHERE visibility = ?
              AND created_at >= ? AND created_at < ? AND id <= ?
              AND (
                  ? IS NULL OR created_at > ? OR (created_at = ? AND id > ?)
              )
            ORDER BY created_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(item.range_start)
        .bind(item.range_end)
        .bind(item.source_fence_id)
        .bind(item.cursor_created_at)
        .bind(item.cursor_created_at)
        .bind(item.cursor_created_at)
        .bind(item.cursor_id)
        .bind(DASHBOARD_ROLLUP_INTEGRITY_SOURCE_PAGE_ROWS)
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
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
            let stored_counts_business_quota: Option<i64> = row.try_get("counts_business_quota")?;
            let counts_business_quota = stored_counts_business_quota
                .map(|value| value != 0)
                .unwrap_or_else(|| {
                    request_log_counts_business_quota(&request_kind_key, request_body.as_deref())
                });
            let result_status: String = row.try_get("result_status")?;
            let failure_kind: Option<String> = row.try_get("failure_kind")?;
            let business_credits: Option<i64> = row.try_get("business_credits")?;
            let minute_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
            item.counts.entry(minute_start).or_default().add(
                Self::dashboard_rollup_counts_for_request(
                    &request_kind_key,
                    request_body.as_deref(),
                    &result_status,
                    failure_kind.as_deref(),
                    business_credits.unwrap_or_default(),
                    counts_business_quota,
                ),
            );
        }
        let last_row = rows.last();
        if let Some(last_row) = last_row {
            item.cursor_created_at = Some(last_row.try_get("created_at")?);
            item.cursor_id = Some(last_row.try_get("id")?);
        }
        if rows.len() as i64 >= DASHBOARD_ROLLUP_INTEGRITY_SOURCE_PAGE_ROWS
            || read_started.elapsed() >= DASHBOARD_ROLLUP_INTEGRITY_READ_BUDGET
        {
            self.persist_dashboard_rollup_integrity_work_item(&item, now).await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
            });
        }

        if self
            .best_effort_flush_request_stats_writes_for_maintenance(
                "dashboard_rollup_integrity_before_replace",
            )
            .await?
            != RequestStatsReadFreshness::Fresh
        {
            self.persist_dashboard_rollup_integrity_work_item(&item, now).await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
            });
        }
        if self
            .dashboard_rollup_integrity_source_changed_since_fence(&item)
            .await?
        {
            self.restart_dashboard_rollup_integrity_work_item(&item, now)
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
            });
        }

        self.request_stats_coalescer
            .begin_dashboard_rollup_repair(item.range_start, item.range_end, item.source_fence_id)
            .await;
        let source_changed_after_barrier = match self
            .dashboard_rollup_integrity_source_changed_since_fence(&item)
            .await
        {
            Ok(changed) => changed,
            Err(err) => {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Err(err);
            }
        };
        if source_changed_after_barrier {
            self.request_stats_coalescer
                .finish_dashboard_rollup_repair(item.range_start, false)
                .await;
            self.restart_dashboard_rollup_integrity_work_item(&item, now)
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
            });
        }

        let actual = match self
            .load_dashboard_rollup_counts(item.range_start, item.range_end)
            .await
        {
            Ok(actual) => actual,
            Err(err) => {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Err(err);
            }
        };
        let mut mismatch = false;
        let mut slow_repair_write = false;
        for minute_start in (item.range_start..item.range_end).step_by(SECS_PER_MINUTE as usize) {
            if item.counts.get(&minute_start).copied().unwrap_or_default()
                != actual.get(&minute_start).copied().unwrap_or_default()
            {
                mismatch = true;
                break;
            }
        }
        if mismatch {
            if let Err(err) = self.record_dashboard_rollup_integrity_gap(&item, now).await {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Err(err);
            }
            slow_repair_write = match self.replace_dashboard_rollup_minutes(&item, now).await {
                Ok(slow) => slow,
                Err(err) => {
                    self.request_stats_coalescer
                        .finish_dashboard_rollup_repair(item.range_start, false)
                        .await;
                    return Err(err);
                }
            };
        }
        let source_changed = match self
            .dashboard_rollup_integrity_source_changed_since_fence(&item)
            .await
        {
            Ok(changed) => changed,
            Err(err) => {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Err(err);
            }
        };
        let has_post_fence_changes = self
            .request_stats_coalescer
            .finish_dashboard_rollup_repair(item.range_start, true)
            .await;
        if source_changed || has_post_fence_changes {
            self.record_dashboard_rollup_integrity_gap(&item, now).await?;
            self.restart_dashboard_rollup_integrity_work_item(&item, now)
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
            });
        }
        self.clear_dashboard_rollup_integrity_gap(item.range_start).await?;
        self.finish_dashboard_rollup_integrity_work_item(&item, now).await?;
        let day_start = local_day_bucket_start_utc_ts(item.range_start);
        if self
            .complete_dashboard_rollup_integrity_day_reaudit_if_ready(day_start, now)
            .await?
        {
            // The final source-backed minute slice recreated the day rollup and seal.
        } else if mismatch {
            self.refresh_dashboard_rollup_daily_seal_after_repair(day_start, now)
                .await?;
        } else {
            self.maybe_seal_dashboard_rollup_day(day_start, now).await?;
        }
        let next_delay_secs = if slow_repair_write {
            next_delay_secs.max(60)
        } else {
            next_delay_secs
        };
        self.mark_dashboard_rollup_integrity_success(now, next_delay_secs)
            .await?;
        Ok(if mismatch {
            DashboardRollupIntegritySlice::Repaired {
                next_delay_secs,
            }
        } else {
            DashboardRollupIntegritySlice::Verified {
                next_delay_secs,
            }
        })
    }

    async fn dashboard_rollup_integrity_source_changed_since_fence(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
    ) -> Result<bool, ProxyError> {
        let latest_source_id: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(id), 0) FROM request_logs
            WHERE visibility = ? AND created_at >= ? AND created_at < ?
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(item.range_start)
        .bind(item.range_end)
        .fetch_one(&self.pool)
        .await?;
        Ok(latest_source_id > item.source_fence_id
            || !self
                .request_stats_coalescer
                .dashboard_rollup_source_version_is_stable(item.range_start, item.source_version)
                .await)
    }

    async fn restart_dashboard_rollup_integrity_work_item(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let source_fence_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM request_logs")
            .fetch_one(&self.pool)
            .await?;
        let source_version = self
            .request_stats_coalescer
            .dashboard_rollup_source_version(item.range_start)
            .await;
        let counts_json = serde_json::to_string(&BTreeMap::<i64, DashboardRequestRollupCounts>::new())
            .map_err(|err| {
                ProxyError::Other(format!("serialize restarted integrity work item: {err}"))
            })?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE dashboard_rollup_integrity_work_items
            SET source_fence = ?, source_version = ?, cursor_created_at = NULL, cursor_id = NULL, counts_json = ?,
                status = 'pending', updated_at = ?
            WHERE range_start = ?
            "#,
        )
        .bind(source_fence_id)
        .bind(source_version)
        .bind(counts_json)
        .bind(now)
        .bind(item.range_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await
    }

    async fn dashboard_rollup_integrity_work_delay(
        &self,
        range_start: i64,
    ) -> Result<i64, ProxyError> {
        let hot_fence: i64 = sqlx::query_scalar(
            "SELECT hot_fence FROM dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(if range_start >= hot_fence.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS) {
            15
        } else {
            60
        })
    }

    async fn persist_dashboard_rollup_integrity_work_item(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let counts_json = serde_json::to_string(&item.counts)
            .map_err(|err| ProxyError::Other(format!("serialize integrity work item: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE dashboard_rollup_integrity_work_items
            SET cursor_created_at = ?, cursor_id = ?, counts_json = ?, updated_at = ?
            WHERE range_start = ? AND status = 'pending'
            "#,
        )
        .bind(item.cursor_created_at)
        .bind(item.cursor_id)
        .bind(counts_json)
        .bind(now)
        .bind(item.range_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn load_dashboard_rollup_counts(
        &self,
        range_start: i64,
        range_end: i64,
    ) -> Result<BTreeMap<i64, DashboardRequestRollupCounts>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT bucket_start, total_requests, success_count, error_count, quota_exhausted_count,
                   valuable_success_count, valuable_failure_count, valuable_failure_429_count,
                   other_success_count, other_failure_count, unknown_count, mcp_non_billable,
                   mcp_billable, api_non_billable, api_billable, local_estimated_credits
            FROM dashboard_request_rollup_buckets
            WHERE bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?
            "#,
        )
        .bind(SECS_PER_MINUTE)
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| Ok((row.try_get("bucket_start")?, Self::dashboard_rollup_counts_from_row(&row)?)))
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }

    fn dashboard_rollup_counts_from_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<DashboardRequestRollupCounts, sqlx::Error> {
        Ok(DashboardRequestRollupCounts {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
            valuable_success_count: row.try_get("valuable_success_count")?,
            valuable_failure_count: row.try_get("valuable_failure_count")?,
            valuable_failure_429_count: row.try_get("valuable_failure_429_count")?,
            other_success_count: row.try_get("other_success_count")?,
            other_failure_count: row.try_get("other_failure_count")?,
            unknown_count: row.try_get("unknown_count")?,
            mcp_non_billable: row.try_get("mcp_non_billable")?,
            mcp_billable: row.try_get("mcp_billable")?,
            api_non_billable: row.try_get("api_non_billable")?,
            api_billable: row.try_get("api_billable")?,
            local_estimated_credits: row.try_get("local_estimated_credits")?,
        })
    }

    async fn record_dashboard_rollup_integrity_gap(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO dashboard_rollup_integrity_gaps (
                range_start, range_end, detected_at, updated_at, reason
            ) VALUES (?, ?, ?, ?, 'source_rollup_mismatch')
            ON CONFLICT(range_start) DO UPDATE SET
                range_end = excluded.range_end, updated_at = excluded.updated_at, reason = excluded.reason
            "#,
        )
        .bind(item.range_start)
        .bind(item.range_end)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn clear_dashboard_rollup_integrity_gap(&self, range_start: i64) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query("DELETE FROM dashboard_rollup_integrity_gaps WHERE range_start = ?")
            .bind(range_start)
            .execute(&mut *conn)
            .await
            .map(|_| ())
            .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn replace_dashboard_rollup_minutes(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let started = StdInstant::now();
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = async {
            sqlx::query(
                "DELETE FROM dashboard_request_rollup_buckets WHERE bucket_secs = ? AND bucket_start >= ? AND bucket_start < ?",
            )
            .bind(SECS_PER_MINUTE)
            .bind(item.range_start)
            .bind(item.range_end)
            .execute(&mut *conn)
            .await?;
            for (bucket_start, counts) in &item.counts {
                Self::insert_dashboard_rollup_bucket_exact(
                    &mut conn,
                    *bucket_start,
                    SECS_PER_MINUTE,
                    *counts,
                    now,
                )
                .await?;
            }
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        let elapsed = started.elapsed();
        if elapsed >= DASHBOARD_ROLLUP_INTEGRITY_WRITE_TARGET {
            tracing::info!(
                component = "dashboard_rollup_integrity",
                elapsed_ms = elapsed.as_millis(),
                "dashboard rollup integrity write exceeded the 100ms target"
            );
        }
        if elapsed >= DASHBOARD_ROLLUP_INTEGRITY_WRITE_WARN {
            tracing::warn!(
                component = "dashboard_rollup_integrity",
                elapsed_ms = elapsed.as_millis(),
                "dashboard rollup integrity write exceeded the 250ms budget"
            );
        }
        Ok(elapsed >= DASHBOARD_ROLLUP_INTEGRITY_WRITE_WARN)
    }

    async fn insert_dashboard_rollup_bucket_exact(
        conn: &mut sqlx::SqliteConnection,
        bucket_start: i64,
        bucket_secs: i64,
        counts: DashboardRequestRollupCounts,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        if counts == DashboardRequestRollupCounts::default() {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO dashboard_request_rollup_buckets (
                bucket_start, bucket_secs, total_requests, success_count, error_count,
                quota_exhausted_count, valuable_success_count, valuable_failure_count,
                valuable_failure_429_count, other_success_count, other_failure_count,
                unknown_count, mcp_non_billable, mcp_billable, api_non_billable, api_billable,
                local_estimated_credits, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_secs)
        .bind(counts.total_requests)
        .bind(counts.success_count)
        .bind(counts.error_count)
        .bind(counts.quota_exhausted_count)
        .bind(counts.valuable_success_count)
        .bind(counts.valuable_failure_count)
        .bind(counts.valuable_failure_429_count)
        .bind(counts.other_success_count)
        .bind(counts.other_failure_count)
        .bind(counts.unknown_count)
        .bind(counts.mcp_non_billable)
        .bind(counts.mcp_billable)
        .bind(counts.api_non_billable)
        .bind(counts.api_billable)
        .bind(counts.local_estimated_credits)
        .bind(updated_at)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    async fn begin_dashboard_rollup_integrity_short_write(
        &self,
    ) -> Result<SqliteImmediateTransaction, ProxyError> {
        self.sqlite_runtime
            .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
            .await
    }

    async fn finish_dashboard_rollup_integrity_short_write(
        &self,
        conn: &mut SqliteImmediateTransaction,
        write_result: Result<(), ProxyError>,
    ) -> Result<(), ProxyError> {
        conn.finish(write_result).await
    }

    async fn finish_dashboard_rollup_integrity_work_item(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            "UPDATE dashboard_rollup_integrity_work_items SET status = 'done', updated_at = ? WHERE range_start = ?",
        )
        .bind(now)
        .bind(item.range_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn maybe_seal_dashboard_rollup_day(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if day_end > latest_closed {
            return Ok(());
        }
        let already_sealed: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
        )
        .bind(day_start)
        .fetch_optional(&self.pool)
        .await?;
        if already_sealed.is_some() {
            return Ok(());
        }
        let completed_buckets: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM dashboard_rollup_integrity_work_items
            WHERE range_start >= ? AND range_start < ? AND status = 'done'
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(&self.pool)
        .await?;
        let expected_buckets = day_end
            .saturating_sub(day_start)
            .div_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if completed_buckets != expected_buckets {
            return Ok(());
        }
        self.seal_dashboard_rollup_day(day_start, now).await
    }

    async fn refresh_dashboard_rollup_daily_seal_after_repair(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        let sealed: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
        )
        .bind(day_start)
        .fetch_optional(&self.pool)
        .await?;
        if sealed.is_some() {
            // Preserve the last fully verified recovery baseline until every
            // slice in the retained source day has been reaudited.
            self.enqueue_dashboard_rollup_integrity_day_reaudit(day_start, now)
                .await
        } else {
            self.maybe_seal_dashboard_rollup_day(day_start, now).await
        }
    }

    async fn enqueue_dashboard_rollup_integrity_day_reaudit(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO dashboard_rollup_integrity_day_reaudits (
                bucket_start, bucket_end, cursor, status, updated_at
            ) VALUES (?, ?, ?, 'pending', ?)
            ON CONFLICT(bucket_start) DO UPDATE SET
                bucket_end = excluded.bucket_end,
                status = 'pending',
                updated_at = excluded.updated_at
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .bind(day_start)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await
    }

    async fn complete_dashboard_rollup_integrity_day_reaudit_if_ready(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let reauditing = sqlx::query(
            "SELECT bucket_end, cursor FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ? AND status = 'pending' LIMIT 1",
        )
        .bind(day_start)
        .fetch_optional(&self.pool)
        .await?;
        let Some(reauditing) = reauditing else {
            return Ok(false);
        };
        let day_end: i64 = reauditing.try_get("bucket_end")?;
        let cursor: i64 = reauditing.try_get("cursor")?;
        if cursor < day_end {
            return Ok(false);
        }
        let pending_slices: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM dashboard_rollup_integrity_work_items
            WHERE status = 'pending' AND range_start >= ? AND range_start < ?
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(&self.pool)
        .await?;
        if pending_slices > 0 {
            return Ok(false);
        }
        self.seal_dashboard_rollup_day(day_start, now).await?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            "DELETE FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?",
        )
        .bind(day_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(true)
    }

    async fn seal_dashboard_rollup_day(&self, day_start: i64, now: i64) -> Result<(), ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let source_counts = self
            .load_dashboard_rollup_counts(day_start, day_end)
            .await?
            .into_values()
            .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                total.add(value);
                total
            });
        let counts_json = serde_json::to_string(&source_counts)
            .map_err(|err| ProxyError::Other(format!("serialize dashboard day seal: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = async {
            sqlx::query(
                "DELETE FROM dashboard_request_rollup_buckets WHERE bucket_secs = ? AND bucket_start = ?",
            )
            .bind(SECS_PER_DAY)
            .bind(day_start)
            .execute(&mut *conn)
            .await?;
            Self::insert_dashboard_rollup_bucket_exact(
                &mut conn,
                day_start,
                SECS_PER_DAY,
                source_counts,
                now,
            )
            .await?;
            sqlx::query(
                r#"
                INSERT INTO dashboard_rollup_daily_seals (bucket_start, counts_json, verified_at)
                VALUES (?, ?, ?)
                ON CONFLICT(bucket_start) DO UPDATE SET counts_json = excluded.counts_json, verified_at = excluded.verified_at
                "#,
            )
            .bind(day_start)
            .bind(counts_json)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn verify_next_dashboard_rollup_daily_seal(&self, now: i64) -> Result<(), ProxyError> {
        let cursor: Option<i64> = sqlx::query_scalar(
            "SELECT seal_cursor FROM dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        let row = sqlx::query(
            r#"
            SELECT bucket_start, counts_json FROM dashboard_rollup_daily_seals
            WHERE bucket_start > COALESCE(?, -9223372036854775808)
            ORDER BY bucket_start ASC LIMIT 1
            "#,
        )
        .bind(cursor)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
            let write_result = sqlx::query("UPDATE dashboard_rollup_integrity_state SET seal_cursor = NULL, updated_at = ? WHERE id = 1")
                .bind(now)
                .execute(&mut *conn)
                .await
                .map(|_| ())
                .map_err(Into::into);
            self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
                .await?;
            return Ok(());
        };
        let day_start: i64 = row.try_get("bucket_start")?;
        let expected: DashboardRequestRollupCounts = serde_json::from_str(&row.try_get::<String, _>("counts_json")?)
            .map_err(|err| ProxyError::Other(format!("invalid dashboard day seal: {err}")))?;
        let minute_actual = self
            .load_dashboard_rollup_counts(day_start, next_local_day_start_utc_ts(day_start))
            .await?
            .into_values()
            .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                total.add(value);
                total
            });
        let daily_actual = self
            .load_dashboard_rollup_bucket(day_start, SECS_PER_DAY)
            .await?;
        let retained_source_exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM request_logs WHERE visibility = ? AND created_at >= ? AND created_at < ? LIMIT 1",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(next_local_day_start_utc_ts(day_start))
        .fetch_optional(&self.pool)
        .await?;
        let should_restore_daily = minute_actual == expected && daily_actual != expected;
        let should_restore_expired_day = retained_source_exists.is_none() && minute_actual != expected;
        if retained_source_exists.is_some() && minute_actual != expected {
            self.enqueue_dashboard_rollup_integrity_day_reaudit(day_start, now)
                .await?;
        } else if should_restore_daily || should_restore_expired_day {
            let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
            let write_result = async {
                sqlx::query("DELETE FROM dashboard_request_rollup_buckets WHERE bucket_secs = ? AND bucket_start = ?")
                    .bind(SECS_PER_DAY)
                    .bind(day_start)
                    .execute(&mut *conn)
                    .await?;
                Self::insert_dashboard_rollup_bucket_exact(
                    &mut conn,
                    day_start,
                    SECS_PER_DAY,
                    expected,
                    now,
                )
                .await?;
                Ok::<_, ProxyError>(())
            }
            .await;
            self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
                .await?;
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query("UPDATE dashboard_rollup_integrity_state SET seal_cursor = ?, updated_at = ? WHERE id = 1")
            .bind(day_start)
            .bind(now)
            .execute(&mut *conn)
            .await
            .map(|_| ())
            .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    async fn dashboard_rollup_integrity_seal_verification_due(
        &self,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let row = sqlx::query(
            "SELECT hot_cursor, hot_fence, last_seal_attempt_at FROM dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        let hot_cursor: i64 = row.try_get("hot_cursor")?;
        let hot_fence: i64 = row.try_get("hot_fence")?;
        let last_attempt: Option<i64> = row.try_get("last_seal_attempt_at")?;
        Ok(hot_cursor >= hot_fence
            && last_attempt
                .map(|attempted| now.saturating_sub(attempted) >= 60)
                .unwrap_or(true))
    }

    async fn mark_dashboard_rollup_integrity_seal_attempt(
        &self,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            "UPDATE dashboard_rollup_integrity_state SET last_seal_attempt_at = ?, updated_at = ? WHERE id = 1",
        )
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await
    }

    async fn load_dashboard_rollup_bucket(
        &self,
        bucket_start: i64,
        bucket_secs: i64,
    ) -> Result<DashboardRequestRollupCounts, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT total_requests, success_count, error_count, quota_exhausted_count,
                   valuable_success_count, valuable_failure_count, valuable_failure_429_count,
                   other_success_count, other_failure_count, unknown_count, mcp_non_billable,
                   mcp_billable, api_non_billable, api_billable, local_estimated_credits
            FROM dashboard_request_rollup_buckets
            WHERE bucket_start = ? AND bucket_secs = ?
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_secs)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| Self::dashboard_rollup_counts_from_row(&row))
            .transpose()
            .map(|counts| counts.unwrap_or_default())
            .map_err(Into::into)
    }

    async fn mark_dashboard_rollup_integrity_success(
        &self,
        now: i64,
        next_delay_secs: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE dashboard_rollup_integrity_state
            SET last_verified_at = ?, stalled_since = NULL, last_error = NULL,
                next_attempt_at = ?, updated_at = ?
            WHERE id = 1
            "#,
        )
        .bind(now)
        .bind(now.saturating_add(next_delay_secs))
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn mark_dashboard_rollup_integrity_failure(
        &self,
        err: &ProxyError,
        next_attempt_at: i64,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE dashboard_rollup_integrity_state
            SET stalled_since = COALESCE(stalled_since, ?), last_error = ?,
                next_attempt_at = ?, updated_at = ?
            WHERE id = 1
            "#,
        )
        .bind(now)
        .bind(err.to_string())
        .bind(next_attempt_at)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_cutoff(
        &self,
        threshold: i64,
    ) -> Result<Option<i64>, ProxyError> {
        let oldest: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(created_at) FROM request_logs WHERE visibility = ? AND created_at < ?",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(threshold)
        .fetch_one(&self.pool)
        .await?;
        let Some(oldest) = oldest else {
            return Ok(Some(threshold));
        };
        let day_start = local_day_bucket_start_utc_ts(oldest);
        let day_end = next_local_day_start_utc_ts(day_start);
        if day_end > threshold {
            return Ok(None);
        }
        let sealed: Option<String> = sqlx::query_scalar(
            "SELECT counts_json FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
        )
        .bind(day_start)
        .fetch_optional(&self.pool)
        .await?;
        let Some(counts_json) = sealed else {
            return Ok(None);
        };
        let expected: DashboardRequestRollupCounts = serde_json::from_str(&counts_json)
            .map_err(|err| ProxyError::Other(format!("invalid dashboard day seal: {err}")))?;
        let minute_actual = self
            .load_dashboard_rollup_counts(day_start, day_end)
            .await?
            .into_values()
            .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                total.add(value);
                total
            });
        let daily_actual = self
            .load_dashboard_rollup_bucket(day_start, SECS_PER_DAY)
            .await?;
        if minute_actual != expected || daily_actual != expected {
            return Ok(None);
        }
        Ok(Some(day_end))
    }
}
