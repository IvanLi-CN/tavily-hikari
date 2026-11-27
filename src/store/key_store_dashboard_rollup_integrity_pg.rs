// PostgreSQL twins preserve the source fences, checkpoints and coalescer barriers.
impl KeyStore {
    pub(crate) async fn reset_dashboard_rollup_integrity_pending_work_on_startup_pg(
        &self,
    ) -> Result<(), ProxyError> {
        let source_fence_id: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
                .fetch_one(self.pg()?)
                .await?;
        let now = self.backend_time.now_ts();
        let empty_counts = serde_json::to_string(&BTreeMap::<i64, DashboardRequestRollupCounts>::new())
            .map_err(|err| ProxyError::Other(format!("serialize reset integrity work item: {err}")))?;
        sqlx::query(
            r#"
            UPDATE observability.dashboard_rollup_integrity_work_items
            SET source_fence = $1,
                durable_source_version = (
                    SELECT COALESCE(SUM(revision), 0)::bigint
                    FROM observability.dashboard_rollup_source_revisions
                    WHERE bucket_start >= range_start AND bucket_start < range_end
                ),
                source_version = (
                    SELECT COALESCE(SUM(revision), 0)::bigint
                    FROM observability.dashboard_rollup_source_revisions
                    WHERE bucket_start >= range_start AND bucket_start < range_end
                ),
                cursor_created_at = NULL, cursor_id = NULL,
                counts_json = $2, status = 'pending', updated_at = $3
            WHERE status = 'pending' AND recovery = 0
            "#,
        )
        .bind(source_fence_id)
        .bind(empty_counts)
        .bind(now)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn dashboard_rollup_integrity_status_pg(
        &self,
    ) -> Result<DashboardRollupIntegrityStatus, ProxyError> {
        let mut conn = self.pg()?.acquire().await?;
        let result = async {
            let now = self.backend_time.now_ts();
            let state = sqlx::query(
            r#"
            SELECT last_verified_at, stalled_since, next_attempt_at, hot_cursor, hot_fence, history_cursor
            FROM observability.dashboard_rollup_integrity_state
            WHERE id = 1
            "#,
            )
            .fetch_optional(&mut *conn)
            .await?;
            let unverified_bucket_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM (
                SELECT range_start FROM observability.dashboard_rollup_integrity_gaps
                UNION
                SELECT range_start FROM observability.dashboard_rollup_integrity_work_items WHERE status = 'pending'
                UNION
                SELECT bucket_start FROM observability.dashboard_rollup_integrity_day_reaudits WHERE status = 'pending'
            )
            AS pending_ranges"#,
            )
            .fetch_one(&mut *conn)
            .await?;
            let recovery_backlog_bucket_count: i64 = sqlx::query_scalar(
            r#"
            SELECT CASE
                WHEN status = 'complete' OR range_end IS NULL OR cursor IS NULL THEN 0
                ELSE GREATEST(1, (range_end - cursor + $1 - 1) / $2)
            END
            FROM observability.dashboard_rollup_rebalance_recovery
            WHERE id = 1
            "#,
            )
            .bind(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
            .bind(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or_default();
            let blocked_range_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_blocked_ranges",
            ).fetch_one(&mut *conn).await?;
            let blocked_range: Option<(String, i64)> = sqlx::query_as(
                "SELECT reason, next_probe_at FROM observability.dashboard_rollup_integrity_blocked_ranges ORDER BY next_probe_at ASC, day_start ASC LIMIT 1",
            ).fetch_optional(&mut *conn).await?;
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
                "SELECT MIN(created_at) FROM observability.request_logs WHERE visibility = $1",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .fetch_one(&mut *conn)
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
            } else if blocked_range_count > 0
                || unverified_bucket_count > 0
                || audit_incomplete
                || last_verified_at.is_none()
            {
                "repairing"
            } else {
                "healthy"
            };
            Ok::<_, ProxyError>(DashboardRollupIntegrityStatus {
                state: state.to_string(),
                last_verified_at,
                next_attempt_at,
                unverified_bucket_count: unverified_bucket_count
                    + hot_backlog_bucket_count
                    + history_backlog_bucket_count
                    + recovery_backlog_bucket_count,
                blocked_reason: blocked_range.as_ref().map(|(reason, _)| reason.clone()),
                next_probe_at: blocked_range.map(|(_, next_probe_at)| next_probe_at),
                blocked_range_count,
            })
        }
        .await;
        drop(conn);
        result
    }

    pub(crate) async fn prepare_request_statistics_recovery_target_pg(
        &self,
        day_start: i64,
    ) -> Result<i64, ProxyError> {
        let now = self.backend_time.now_ts();
        self.ensure_dashboard_rollup_integrity_state_pg(now).await?;
        self.ensure_dashboard_rollup_rebalance_recovery_pg(now).await?;
        let day_end = next_local_day_start_utc_ts(day_start);
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if day_end > latest_closed {
            return Err(ProxyError::Other(
                "request statistics recovery target day is not closed".to_string(),
            ));
        }
        if self
            .dashboard_rollup_integrity_blocked_range_probe_delay(day_start, now, None)
            .await?
            .is_some()
            && !self
                .promote_dashboard_rollup_integrity_probe_on_source_change(day_start, now)
                .await?
        {
            return self
                .request_statistics_recovery_source_fence_pg(day_start, 0)
                .await;
        }
        let mut fixed_source_fence = 0_i64;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            let persisted_target: Option<(Option<i64>, i64)> = sqlx::query_as(
                "SELECT source_fence, cursor FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1",
            )
            .bind(day_start)
            .fetch_optional(&mut *conn)
            .await?;
            let current_day_source_fence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let candidate_source_fence = if let Some((Some(source_fence), cursor)) = persisted_target
                && cursor < day_end
            {
                source_fence
            } else if let Some(source_fence) = sqlx::query_scalar::<_, Option<i64>>(
                r#"
                SELECT MIN(source_fence)
                FROM observability.dashboard_rollup_integrity_work_items
                WHERE recovery = 1 AND range_start >= $1 AND range_end <= $2
                "#,
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?
            {
                source_fence
            } else {
                sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
                    .fetch_one(&mut *conn)
                    .await?
            };
            fixed_source_fence = if current_day_source_fence > candidate_source_fence {
                sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
                    .fetch_one(&mut *conn)
                    .await?
            } else {
                candidate_source_fence
            };
            sqlx::query(
                r#"
                INSERT INTO observability.dashboard_rollup_integrity_day_reaudits (
                    bucket_start, bucket_end, cursor, status, updated_at, source_fence, gc_blocking
                ) VALUES ($1, $2, $3, 'pending', $4, $5, 1)
                ON CONFLICT(bucket_start) DO UPDATE SET
                    bucket_end = excluded.bucket_end,
                    cursor = CASE
                        WHEN observability.dashboard_rollup_integrity_day_reaudits.cursor >= excluded.bucket_end
                            OR excluded.source_fence > COALESCE(
                                observability.dashboard_rollup_integrity_day_reaudits.source_fence,
                                -1
                            )
                        THEN excluded.cursor
                        ELSE observability.dashboard_rollup_integrity_day_reaudits.cursor
                    END,
                    status = 'pending',
                    updated_at = excluded.updated_at,
                    source_fence = CASE
                        WHEN excluded.source_fence > COALESCE(
                            observability.dashboard_rollup_integrity_day_reaudits.source_fence,
                            -1
                        )
                        THEN excluded.source_fence
                        ELSE observability.dashboard_rollup_integrity_day_reaudits.source_fence
                    END,
                    gc_blocking = GREATEST(
                        observability.dashboard_rollup_integrity_day_reaudits.gc_blocking,
                        excluded.gc_blocking
                    )
                "#,
            )
            .bind(day_start)
            .bind(day_end)
            .bind(day_start)
            .bind(now)
            .bind(fixed_source_fence)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        tracing::info!(
            component = "request_statistics_recovery",
            event = "target_prepared",
            target_day_start = day_start,
            target_day_end = day_end,
            source_fence = fixed_source_fence,
            "prepared a fixed source-backed recovery target"
        );
        Ok(fixed_source_fence)
    }

    pub(crate) async fn dashboard_rollup_integrity_durable_source_version_pg(
        &self,
        range_start: i64,
        range_end: i64,
    ) -> Result<i64, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT COALESCE(SUM(revision), 0)::bigint
            FROM observability.dashboard_rollup_source_revisions
            WHERE bucket_start >= $1 AND bucket_start < $2
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_one(self.pg()?)
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn dashboard_rollup_integrity_source_versions_pg(
        &self,
        range_start: i64,
        range_end: i64,
    ) -> Result<(i64, i64), ProxyError> {
        let durable_source_version = self
            .dashboard_rollup_integrity_durable_source_version_pg(range_start, range_end)
            .await?;
        let coalescer_source_version = self
            .request_stats_coalescer
            .dashboard_rollup_source_version(range_start, range_end)
            .await;
        Ok((
            durable_source_version,
            durable_source_version.saturating_add(coalescer_source_version),
        ))
    }

    pub(crate) async fn run_request_statistics_recovery_integrity_slice_pg(
        &self,
        day_start: i64,
        source_fence_id: i64,
        deadline: tokio::time::Instant,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        if tokio::time::Instant::now() >= deadline {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs: 1,
                reason: "recovery_budget",
            });
        }
        let setup_budget = deadline.saturating_duration_since(tokio::time::Instant::now());
        let setup = tokio::time::timeout(setup_budget, async {
            let now = self.backend_time.now_ts();
            self.ensure_dashboard_rollup_integrity_state_pg(now).await?;
            self.ensure_dashboard_rollup_rebalance_recovery_pg(now).await?;
            if let Some(next_delay_secs) = self
                .dashboard_rollup_integrity_blocked_range_probe_delay(day_start, now, None)
                .await?
            {
                return Ok::<_, ProxyError>((now, None, Some(next_delay_secs)));
            }
            let source_fence_id = self
                .request_statistics_recovery_source_fence_pg(day_start, source_fence_id)
                .await?;
            let mut item = if let Some(item) = self
                .load_request_statistics_recovery_work_item_pg(day_start, now)
                .await?
            {
                Some(item)
            } else {
                self.create_request_statistics_recovery_work_item_pg(
                    day_start,
                    source_fence_id,
                    now,
                )
                .await?
            };
            if item.is_none()
                && !self
                    .request_statistics_recovery_target_complete_pg(day_start)
                    .await?
            {
                let refreshed_source_fence = self
                    .prepare_request_statistics_recovery_target_pg(day_start)
                    .await?;
                item = self
                    .create_request_statistics_recovery_work_item_pg(
                        day_start,
                        refreshed_source_fence,
                        now,
                    )
                    .await?;
            }
            Ok::<_, ProxyError>((now, item, None))
        })
        .await;
        let (now, item, blocked_probe_delay) = match setup {
            Ok(result) => result?,
            Err(_) => {
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs: 1,
                    reason: "recovery_budget",
                });
            }
        };
        if let Some(next_delay_secs) = blocked_probe_delay {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "source_baseline_probe_not_due",
            });
        }
        if let Some(item) = item {
            return self
                .process_dashboard_rollup_integrity_work_item_pg(
                    item,
                    now,
                    Some(deadline),
                    true,
                    None,
                )
                .await;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs: 1,
                reason: "recovery_budget",
            });
        }
        match tokio::time::timeout(remaining, async {
            self.complete_dashboard_rollup_integrity_day_reaudit_if_ready_pg(day_start, now)
                .await?;
            self.mark_dashboard_rollup_integrity_success_pg(now, 60).await?;
            Ok::<_, ProxyError>(())
        })
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs: 1,
                    reason: "recovery_budget",
                });
            }
        }
        Ok(DashboardRollupIntegritySlice::Verified { next_delay_secs: 60 })
    }

    pub(crate) async fn request_statistics_recovery_source_fence_pg(
        &self,
        day_start: i64,
        fallback: i64,
    ) -> Result<i64, ProxyError> {
        let persisted: Option<Option<i64>> = sqlx::query(
            "SELECT source_fence FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending'",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?
        .map(|row| row.try_get("source_fence"))
        .transpose()?;
        match persisted {
            Some(Some(source_fence)) => Ok(source_fence),
            Some(None) => sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
                .fetch_one(self.pg()?)
                .await
                .map_err(Into::into),
            None => Ok(fallback),
        }
    }

    pub(crate) async fn request_statistics_recovery_target_checkpoint_pg(
        &self,
        day_start: i64,
    ) -> Result<i64, ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let cursor: Option<i64> = sqlx::query_scalar(
            "SELECT cursor FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending'",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        if let Some(cursor) = cursor {
            return Ok(cursor);
        }
        let sealed: Option<i64> = sqlx::query_scalar(
            "SELECT 1::bigint FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        Ok(sealed.map(|_| day_end).unwrap_or(day_start))
    }

    pub(crate) async fn request_statistics_recovery_target_complete_pg(
        &self,
        day_start: i64,
    ) -> Result<bool, ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let now = self.backend_time.now_ts();
        if self
            .dashboard_rollup_integrity_blocked_range_probe_delay(day_start, now, None)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let sealed: Option<(Option<i64>, i64, i64)> = sqlx::query_as(
            "SELECT source_fence, source_version, durable_source_version FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        let Some((sealed_fence, sealed_version, sealed_durable_source_version)) = sealed else {
            return Ok(false);
        };
        let current_fence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
        .await?;
        let (current_durable_source_version, current_version) = self
            .dashboard_rollup_integrity_source_versions_pg(day_start, day_end)
            .await?;
        if !self
            .request_stats_coalescer
            .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
        {
            return Ok(false);
        }
        // A successful fenced GC pass may remove the final raw rows from the
        // A fenced GC pass may lower MAX(id) without changing the durable
        // source revision. Treat that as the valid post-GC representation of
        // the seal; a fence that moves forward still indicates a late insert.
        let source_fence_advanced = match sealed_fence {
            Some(sealed_fence) => current_fence > sealed_fence,
            None => current_fence != 0,
        };
        if source_fence_advanced
            || sealed_version != current_version
            || sealed_durable_source_version != current_durable_source_version
        {
            return Ok(false);
        }
        if self
            .dashboard_rollup_gc_deleted_source_baseline_missing_pg(
                day_start,
                day_end,
                DASHBOARD_ROLLUP_INTEGRITY_READ_BUDGET,
            )
            .await?
        {
            return Ok(false);
        }
        let pending_work: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM observability.dashboard_rollup_integrity_work_items
            WHERE status = 'pending' AND recovery = 1
              AND range_start < $1 AND range_end > $2
            "#,
        )
        .bind(day_end)
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        let pending_reaudit: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_day_reaudits WHERE status = 'pending' AND bucket_start = $1",
        )
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        Ok(pending_work == 0 && pending_reaudit == 0)
    }

    pub(crate) async fn request_statistics_recovery_target_expired_rows_pg(
        &self,
        day_start: i64,
        threshold: i64,
    ) -> Result<i64, ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let source_fence: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT source_fence FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1), (SELECT MAX(id) FROM observability.request_logs), 0)",
        )
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.request_logs WHERE created_at >= $1 AND created_at < $2 AND created_at < $3 AND id <= $4",
        )
        .bind(day_start)
        .bind(day_end)
        .bind(threshold)
        .bind(source_fence)
        .fetch_one(self.pg()?)
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn load_request_statistics_recovery_work_item_pg(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let row = sqlx::query(
            r#"
            SELECT range_start, range_end, source_fence, source_version, durable_source_version,
                   cursor_created_at, cursor_id, counts_json
            FROM observability.dashboard_rollup_integrity_work_items
            WHERE status = 'pending' AND recovery = 1
              AND range_start >= $1 AND range_end <= $2
              AND NOT EXISTS (
                  SELECT 1::bigint FROM observability.dashboard_rollup_integrity_blocked_ranges blocked
                  WHERE blocked.day_start = $3
                    AND blocked.reason IN ('source_baseline', 'deleted_source_baseline_unavailable')
                    AND blocked.next_probe_at > $4
              )
            ORDER BY range_start ASC
            LIMIT 1
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .bind(day_start)
        .bind(now)
        .fetch_optional(self.pg()?)
        .await?;
        row.map(|row| {
            let counts_json: String = row.try_get("counts_json")?;
            let counts = serde_json::from_str(&counts_json).map_err(|err| {
                sqlx::Error::Protocol(format!(
                    "invalid request statistics recovery work item: {err}"
                ))
            })?;
            Ok::<DashboardRollupIntegrityWorkItem, sqlx::Error>(DashboardRollupIntegrityWorkItem {
                range_start: row.try_get("range_start")?,
                range_end: row.try_get("range_end")?,
                source_fence_id: row.try_get("source_fence")?,
                source_version: row.try_get("source_version")?,
                durable_source_version: row.try_get("durable_source_version")?,
                cursor_created_at: row.try_get("cursor_created_at")?,
                cursor_id: row.try_get("cursor_id")?,
                counts,
            })
        })
        .transpose()
        .map_err(Into::into)
    }

    pub(crate) async fn create_request_statistics_recovery_work_item_pg(
        &self,
        day_start: i64,
        source_fence_id: i64,
        now: i64,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        if self
            .dashboard_rollup_integrity_blocked_range_probe_delay(day_start, now, None)
            .await?
            .is_some()
        {
            return Ok(None);
        }
        let Some(row) = sqlx::query(
            "SELECT bucket_end, cursor FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending'",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?
        else {
            return Ok(None);
        };
        let day_end: i64 = row.try_get("bucket_end")?;
        let cursor: i64 = row.try_get("cursor")?;
        if cursor >= day_end {
            return Ok(None);
        }
        let range_end = (cursor + DASHBOARD_ROLLUP_INTEGRITY_GC_BLOCKING_WORK_SECS).min(day_end);
        let (durable_source_version, source_version) = self
            .dashboard_rollup_integrity_source_versions_pg(cursor, range_end)
            .await?;
        if let Some(existing) = sqlx::query(
            "SELECT status, recovery, range_end, source_fence, source_version, durable_source_version FROM observability.dashboard_rollup_integrity_work_items WHERE range_start = $1",
        )
        .bind(cursor)
        .fetch_optional(self.pg()?)
        .await?
        {
            let status: String = existing.try_get("status")?;
            let existing_recovery: i64 = existing.try_get("recovery")?;
            let existing_range_end: i64 = existing.try_get("range_end")?;
            let existing_fence: i64 = existing.try_get("source_fence")?;
            let existing_source_version: i64 = existing.try_get("source_version")?;
            let existing_durable_source_version: i64 =
                existing.try_get("durable_source_version")?;
            if existing_recovery == 1
                && status == "pending"
                && existing_range_end == range_end
                && existing_fence == source_fence_id
                && existing_source_version == source_version
                && existing_durable_source_version == durable_source_version
            {
                return Ok(None);
            }
            if existing_recovery == 1
                && status == "done"
                && existing_range_end == range_end
                && existing_fence == source_fence_id
                && existing_source_version == source_version
                && existing_durable_source_version == durable_source_version
            {
                let next_cursor = (cursor + DASHBOARD_ROLLUP_INTEGRITY_GC_BLOCKING_WORK_SECS)
                    .min(day_end);
                sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_day_reaudits SET cursor = $1, updated_at = $2 WHERE bucket_start = $3 AND status = 'pending'",
                )
                .bind(next_cursor)
                .bind(now)
                .bind(day_start)
                .execute(self.pg()?)
                .await?;
                return Ok(None);
            }
        }
        let item = DashboardRollupIntegrityWorkItem::empty(
            cursor,
            range_end,
            source_fence_id,
            durable_source_version,
            source_version,
        );
        let counts_json = serde_json::to_string(&item.counts).map_err(|err| {
            ProxyError::Other(format!("serialize request statistics recovery work item: {err}"))
        })?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            sqlx::query(
                r#"
                INSERT INTO observability.dashboard_rollup_integrity_work_items (
                    range_start, range_end, source_fence, source_version, durable_source_version, cursor_created_at,
                    cursor_id, counts_json, status, priority, recovery, updated_at
                ) VALUES ($1, $2, $3, $4, $5, NULL, NULL, $6, 'pending', 3, 1, $7)
                ON CONFLICT(range_start) DO UPDATE SET
                    range_end = excluded.range_end,
                    source_fence = excluded.source_fence,
                    source_version = excluded.source_version,
                    durable_source_version = excluded.durable_source_version,
                    cursor_created_at = NULL,
                    cursor_id = NULL,
                    counts_json = excluded.counts_json,
                    status = 'pending',
                    priority = 3,
                    recovery = 1,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(item.range_start)
            .bind(item.range_end)
            .bind(item.source_fence_id)
            .bind(item.source_version)
            .bind(item.durable_source_version)
            .bind(counts_json)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                "UPDATE observability.dashboard_rollup_integrity_day_reaudits SET cursor = $1, updated_at = $2 WHERE bucket_start = $3 AND status = 'pending'",
            )
            .bind(range_end)
            .bind(now)
            .bind(day_start)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        tracing::info!(
            component = "request_statistics_recovery",
            event = "target_checkpoint_accepted",
            target_day_start = day_start,
            range_start = item.range_start,
            range_end = item.range_end,
            source_fence = source_fence_id,
            "materialized a bounded target checkpoint"
        );
        Ok(Some(item))
    }

    pub(crate) async fn dashboard_rollup_integrity_hot_work_due_pg(&self, now: i64) -> Result<bool, ProxyError> {
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        let hot_start = latest_closed.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS);
        sqlx::query_scalar(
            r#"SELECT (hot_cursor < hot_fence OR hot_fence < $1) AND NOT EXISTS (
                SELECT 1::bigint FROM observability.dashboard_rollup_integrity_work_items
                WHERE status = 'pending' AND recovery = 0 AND range_start >= $2 AND range_end <= $3
                  AND priority IN (0, 3, 4, 5)
            ) FROM observability.dashboard_rollup_integrity_state WHERE id = 1"#,
        )
        .bind(latest_closed)
        .bind(hot_start)
        .bind(latest_closed)
        .fetch_one(self.pg()?)
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn dashboard_rollup_integrity_gc_work_due_pg(&self) -> Result<bool, ProxyError> {
        sqlx::query_scalar(
            r#"SELECT EXISTS (
                SELECT 1::bigint FROM (
                    SELECT bucket_start, bucket_end
                    FROM observability.dashboard_rollup_integrity_day_reaudits
                    WHERE gc_blocking = 1 AND status = 'pending'
                    ORDER BY updated_at ASC, bucket_start ASC LIMIT 1
                ) d WHERE NOT EXISTS (
                    SELECT 1::bigint FROM observability.dashboard_rollup_integrity_work_items w
                    WHERE w.status = 'pending'
                      AND w.range_start < d.bucket_end AND w.range_end > d.bucket_start
                )
            )"#,
        )
        .fetch_one(self.pg()?)
        .await
        .map_err(Into::into)
    }

    pub(crate) async fn ensure_dashboard_rollup_integrity_state_pg(&self, now: i64) -> Result<(), ProxyError> {
        let fence = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        let hot_start = fence.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS);
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO observability.dashboard_rollup_integrity_state (
                id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
                last_history_attempt_at, last_day_reaudit_attempt_at, last_seal_attempt_at,
                seal_cursor, updated_at
            ) VALUES (1, $1, $2, $3, $4, NULL, NULL, NULL, NULL, $5)
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
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn ensure_dashboard_rollup_rebalance_recovery_pg(&self, now: i64) -> Result<(), ProxyError> {
        let current: Option<(i64, String)> = sqlx::query_as(
            "SELECT version, status FROM observability.dashboard_rollup_rebalance_recovery WHERE id = 1",
        )
        .fetch_optional(self.pg()?)
        .await?;
        if current
            .as_ref()
            .is_some_and(|(version, _)| *version >= DASHBOARD_ROLLUP_REBALANCE_RECOVERY_VERSION)
        {
            return Ok(());
        }

        let (minimum, maximum): (Option<i64>, Option<i64>) = sqlx::query_as(
            r#"
            SELECT MIN(created_at), MAX(created_at)
            FROM observability.request_logs
            WHERE visibility = $1
              AND gateway_mode = 'rebalance'
              AND experiment_variant = 'rebalance'
              AND upstream_operation = 'mcp'
              AND request_kind_key IS NULL
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_one(self.pg()?)
        .await?;
        let source_fence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
            .fetch_one(self.pg()?)
            .await?;
        let (status, range_start, range_end, cursor) = if let (Some(minimum), Some(maximum)) = (minimum, maximum) {
            let range_start = minimum - minimum.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
            let range_end = (maximum + 1 + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS - 1)
                .div_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
                * DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS;
            ("pending", Some(range_start), Some(range_end), Some(range_start))
        } else {
            ("complete", None, None, None)
        };
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO observability.dashboard_rollup_rebalance_recovery (
                id, version, status, range_start, range_end, source_fence, cursor,
                last_error, completed_at, updated_at
            ) VALUES (1, $1, $2, $3, $4, $5, $6, NULL, $7, $8)
            ON CONFLICT(id) DO UPDATE SET
                version = excluded.version,
                status = excluded.status,
                range_start = excluded.range_start,
                range_end = excluded.range_end,
                source_fence = excluded.source_fence,
                cursor = excluded.cursor,
                last_error = NULL,
                completed_at = excluded.completed_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(DASHBOARD_ROLLUP_REBALANCE_RECOVERY_VERSION)
        .bind(status)
        .bind(range_start)
        .bind(range_end)
        .bind(source_fence)
        .bind(cursor)
        .bind((status == "complete").then_some(now))
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        tracing::info!(
            component = "dashboard_rollup_integrity",
            event = "rebalance_rollup_recovery_initialized",
            status,
            range_start = ?range_start,
            range_end = ?range_end,
            source_fence,
        );
        Ok(())
    }

    pub(crate) async fn create_next_dashboard_rollup_rebalance_recovery_work_item_pg(
        &self,
        now: i64,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        let Some(row) = sqlx::query(
            "SELECT status, range_start, range_end, source_fence, cursor FROM observability.dashboard_rollup_rebalance_recovery WHERE id = 1",
        )
        .fetch_optional(self.pg()?)
        .await?
        else {
            return Ok(None);
        };
        let status: String = row.try_get("status")?;
        let Some(_range_start): Option<i64> = row.try_get("range_start")? else {
            return Ok(None);
        };
        let range_end: i64 = row.try_get("range_end")?;
        let source_fence_id: i64 = row.try_get("source_fence")?;
        let cursor: i64 = row.try_get("cursor")?;
        if status == "complete" || cursor >= range_end {
            return Ok(None);
        }

        if let Some(existing) = sqlx::query(
            "SELECT status, source_fence FROM observability.dashboard_rollup_integrity_work_items WHERE range_start = $1",
        )
        .bind(cursor)
        .fetch_optional(self.pg()?)
        .await?
        {
            let existing_status: String = existing.try_get("status")?;
            let existing_fence: i64 = existing.try_get("source_fence")?;
            if existing_status == "pending" && existing_fence == source_fence_id {
                return Ok(None);
            }
            if existing_status == "done" && existing_fence == source_fence_id {
                let next_cursor = (cursor + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(range_end);
                sqlx::query(
                    "UPDATE observability.dashboard_rollup_rebalance_recovery SET cursor = $1, updated_at = $2 WHERE id = 1 AND status != 'complete'",
                )
                .bind(next_cursor)
                .bind(now)
                .execute(self.pg()?)
                .await?;
                return Ok(None);
            }
        }

        let range_end_for_item = (cursor + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS).min(range_end);
        let (durable_source_version, source_version) = self
            .dashboard_rollup_integrity_source_versions_pg(cursor, range_end_for_item)
            .await?;
        let item = DashboardRollupIntegrityWorkItem::empty(
            cursor,
            range_end_for_item,
            source_fence_id,
            durable_source_version,
            source_version,
        );
        let counts_json = serde_json::to_string(&item.counts)
            .map_err(|err| ProxyError::Other(format!("serialize rebalance recovery work item: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO observability.dashboard_rollup_integrity_work_items (
                range_start, range_end, source_fence, source_version, durable_source_version, cursor_created_at,
                cursor_id, counts_json, status, priority, recovery, updated_at
            ) VALUES ($1, $2, $3, $4, $5, NULL, NULL, $6, 'pending', 0, 1, $7)
            ON CONFLICT(range_start) DO UPDATE SET
                range_end = excluded.range_end,
                source_fence = excluded.source_fence,
                source_version = excluded.source_version,
                durable_source_version = excluded.durable_source_version,
                cursor_created_at = NULL,
                cursor_id = NULL,
                counts_json = excluded.counts_json,
                status = 'pending',
                priority = 0,
                recovery = 1,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(item.range_start)
        .bind(item.range_end)
        .bind(item.source_fence_id)
        .bind(item.source_version)
        .bind(item.durable_source_version)
        .bind(counts_json)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        tracing::info!(
            component = "dashboard_rollup_integrity",
            event = "rebalance_rollup_recovery_slice_started",
            range_start = item.range_start,
            range_end = item.range_end,
            source_fence = item.source_fence_id,
        );
        Ok(Some(item))
    }

    pub(crate) async fn create_next_dashboard_rollup_integrity_work_item_pg(
        &self,
        now: i64,
        claimed_probe_day_start: Option<i64>,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
                   last_history_attempt_at, last_day_reaudit_attempt_at
            FROM observability.dashboard_rollup_integrity_state WHERE id = 1
            "#,
        )
        .fetch_one(self.pg()?)
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
            SELECT bucket_start, bucket_end, cursor, gc_blocking
            FROM observability.dashboard_rollup_integrity_day_reaudits
            WHERE status = 'pending'
              AND NOT EXISTS (
                  SELECT 1::bigint FROM observability.dashboard_rollup_integrity_blocked_ranges blocked
                  WHERE blocked.day_start = observability.dashboard_rollup_integrity_day_reaudits.bucket_start
                    AND blocked.reason IN ('source_baseline', 'deleted_source_baseline_unavailable')
                    AND blocked.next_probe_at > $1
                    AND ($2::bigint IS NULL OR blocked.day_start != $3)
              )
            ORDER BY gc_blocking DESC, updated_at ASC, bucket_start ASC
            LIMIT 1
            "#,
        )
        .bind(now)
        .bind(claimed_probe_day_start)
        .bind(claimed_probe_day_start)
        .fetch_optional(self.pg()?)
        .await?;
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        let oldest: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(created_at) FROM observability.request_logs WHERE visibility = $1",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch_one(self.pg()?)
        .await?;
        let history_due = last_history_attempt_at
            .map(|attempted| now.saturating_sub(attempted) >= 60)
            .unwrap_or(true);
        let history_floor = oldest.map(local_day_bucket_start_utc_ts);
        let can_scan_history = history_floor
            .map(|floor| history_cursor > floor && history_due)
            .unwrap_or(false);
        let should_scan_history =
            can_scan_history && hot_cursor >= hot_fence && hot_fence >= latest_closed;

        let hot_start = latest_closed.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS);
        // Old state must not turn historical debt into an unbounded initial hot pass.
        // Historical source days remain protected by their separate recovery seals.
        let hot_cursor = hot_cursor.max(hot_start);
        let hot_fence = hot_fence.max(hot_start);
        let hot_is_behind = hot_cursor < hot_fence || hot_fence < latest_closed;
        let day_reaudit_due = last_day_reaudit_attempt_at
            .map(|attempted| now.saturating_sub(attempted) >= 60)
            .unwrap_or(true);
        let gc_reaudit_due = sealed_day_reaudit
            .as_ref()
            .map(|row| row.get::<i64, _>("gc_blocking") != 0)
            .unwrap_or(false);
        // Materialize an unqueued hot page before a GC-blocking day. Once a page
        // has a durable checkpoint, its continuation priority lets GC take a turn.
        let new_hot_segment_due = hot_fence < latest_closed;
        let pending_hot_page: bool = sqlx::query_scalar(
            r#"SELECT EXISTS (
                SELECT 1::bigint FROM observability.dashboard_rollup_integrity_work_items
                WHERE status = 'pending' AND recovery = 0
                  AND range_start >= $1 AND range_end <= $2
                  AND priority IN (0, 3, 4, 5)
            )"#,
        )
        .bind(hot_start)
        .bind(latest_closed)
        .fetch_one(self.pg()?)
        .await?;
        let initial_hot_page_due = hot_cursor < hot_fence && !pending_hot_page;
        let hot_page_due = new_hot_segment_due || initial_hot_page_due;
        let day_reaudit = if (gc_reaudit_due && !hot_page_due)
            || (day_reaudit_due && !hot_is_behind)
        {
            sealed_day_reaudit
        } else {
            None
        };
        let (range_start, range_end, kind) = if let Some(day_reaudit) = day_reaudit {
            let day_start: i64 = day_reaudit.try_get("bucket_start")?;
            let day_end: i64 = day_reaudit.try_get("bucket_end")?;
            let cursor: i64 = day_reaudit.try_get("cursor")?;
            if self
                .dashboard_rollup_integrity_blocked_range_probe_delay(
                    day_start,
                    now,
                    claimed_probe_day_start,
                )
                .await?
                .is_some()
            {
                return Ok(None);
            }
            if cursor >= day_end {
                self.complete_dashboard_rollup_integrity_day_reaudit_if_ready_pg(day_start, now)
                    .await?;
                return Ok(None);
            }
            (
                cursor,
                (cursor
                    + if gc_reaudit_due {
                        DASHBOARD_ROLLUP_INTEGRITY_GC_BLOCKING_WORK_SECS
                    } else {
                        DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS
                    })
                    .min(day_end),
                DashboardRollupIntegrityWorkKind::SealedDayReaudit {
                    day_start,
                    gc_blocking: gc_reaudit_due,
                },
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
                (hot_fence.max(hot_start) + DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
                    .min(latest_closed),
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
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
                .fetch_one(self.pg()?)
                .await?;
        let (durable_source_version, source_version) = self
            .dashboard_rollup_integrity_source_versions_pg(range_start, range_end)
            .await?;
        let item = DashboardRollupIntegrityWorkItem::empty(
            range_start,
            range_end,
            source_fence_id,
            durable_source_version,
            source_version,
        );
        let counts_json = serde_json::to_string(&item.counts)
            .map_err(|err| ProxyError::Other(format!("serialize integrity work item: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            sqlx::query(
                r#"
                INSERT INTO observability.dashboard_rollup_integrity_work_items (
                    range_start, range_end, source_fence, source_version, durable_source_version, cursor_created_at, cursor_id, counts_json, status, priority, recovery, updated_at
                ) VALUES ($1, $2, $3, $4, $5, NULL, NULL, $6, 'pending', $7, 0, $8)
                ON CONFLICT(range_start) DO UPDATE SET
                    range_end = excluded.range_end,
                    source_fence = excluded.source_fence,
                    source_version = excluded.source_version,
                    durable_source_version = excluded.durable_source_version,
                    cursor_created_at = NULL,
                    cursor_id = NULL,
                    counts_json = excluded.counts_json,
                    status = 'pending',
                    priority = excluded.priority,
                    recovery = 0,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(item.range_start)
            .bind(item.range_end)
            .bind(item.source_fence_id)
            .bind(item.source_version)
            .bind(item.durable_source_version)
            .bind(counts_json)
            .bind(kind.priority())
            .bind(now)
            .execute(&mut *conn)
            .await?;
            match kind {
                DashboardRollupIntegrityWorkKind::InitialHot => sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_state SET hot_cursor = $1, hot_fence = $2, updated_at = $3 WHERE id = 1",
                )
                .bind(range_end)
                .bind(latest_closed)
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::History => sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_state SET history_cursor = $1, last_history_attempt_at = $2, updated_at = $3 WHERE id = 1",
                )
                .bind(range_start)
                .bind(now)
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::HotReaudit => sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_state SET hot_reaudit_cursor = $1, updated_at = $2 WHERE id = 1",
                )
                .bind(if range_end >= latest_closed { hot_start } else { range_end })
                .bind(now)
                .execute(&mut *conn)
                .await?,
                DashboardRollupIntegrityWorkKind::SealedDayReaudit { day_start, .. } => sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_day_reaudits SET cursor = $1, updated_at = $2 WHERE bucket_start = $3 AND status = 'pending'",
                )
                .bind(range_end)
                .bind(now)
                .bind(day_start)
                .execute(&mut *conn)
                .await?,
            };
            if matches!(kind, DashboardRollupIntegrityWorkKind::SealedDayReaudit { .. }) {
                sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_state SET last_day_reaudit_attempt_at = $1, updated_at = $2 WHERE id = 1",
                )
                .bind(now)
                .bind(now)
                .execute(&mut *conn)
                .await?;
            }
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(Some(item))
    }
}

include!("key_store_dashboard_rollup_integrity_repair_pg.rs");
include!("key_store_dashboard_rollup_integrity_seals_pg.rs");
