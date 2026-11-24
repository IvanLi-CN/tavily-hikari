impl KeyStore {
    pub(crate) async fn maybe_seal_dashboard_rollup_day_pg(
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
            "SELECT 1::bigint FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        if already_sealed.is_some() {
            return Ok(());
        }
        let completed_buckets: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM observability.dashboard_rollup_integrity_work_items
            WHERE range_start >= $1 AND range_start < $2 AND status = 'done'
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
        .await?;
        let expected_buckets = day_end
            .saturating_sub(day_start)
            .div_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if completed_buckets != expected_buckets {
            return Ok(());
        }
        self.seal_dashboard_rollup_day_pg(day_start, now).await
    }

    pub(crate) async fn refresh_dashboard_rollup_daily_seal_after_repair_pg(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        let sealed: Option<i64> = sqlx::query_scalar(
            "SELECT 1::bigint FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        if sealed.is_some() {
            // Preserve the last fully verified recovery baseline until every
            // slice in the retained source day has been reaudited.
            self.enqueue_dashboard_rollup_integrity_day_reaudit_pg(day_start, now, false)
                .await
        } else {
            self.maybe_seal_dashboard_rollup_day_pg(day_start, now).await
        }
    }

    pub(crate) async fn enqueue_dashboard_rollup_integrity_day_reaudit_pg(
        &self,
        day_start: i64,
        now: i64,
        gc_blocking: bool,
    ) -> Result<(), ProxyError> {
        self.enqueue_dashboard_rollup_integrity_day_reaudit_with_source_fence_pg(
            day_start,
            now,
            gc_blocking,
            None,
        )
        .await
    }

    pub(crate) async fn enqueue_dashboard_rollup_integrity_day_reaudit_with_source_fence_pg(
        &self,
        day_start: i64,
        now: i64,
        gc_blocking: bool,
        source_fence: Option<i64>,
    ) -> Result<(), ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let registration = sqlx::query(
            r#"
            INSERT INTO observability.dashboard_rollup_integrity_day_reaudits (
                bucket_start, bucket_end, cursor, status, updated_at, source_fence, gc_blocking
            ) VALUES ($1, $2, $3, 'pending', $4, $5, $6)
            ON CONFLICT(bucket_start) DO UPDATE SET
                bucket_end = excluded.bucket_end,
                status = 'pending',
                updated_at = excluded.updated_at,
                source_fence = COALESCE(
                    observability.dashboard_rollup_integrity_day_reaudits.source_fence,
                    excluded.source_fence
                ),
                gc_blocking = GREATEST(observability.dashboard_rollup_integrity_day_reaudits.gc_blocking, excluded.gc_blocking)
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .bind(day_start)
        .bind(now)
        .bind(source_fence)
        .bind(i64::from(gc_blocking));
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = registration
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await
    }

    pub(crate) async fn complete_dashboard_rollup_integrity_day_reaudit_if_ready_pg(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let reauditing = sqlx::query(
            "SELECT bucket_end, cursor FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending' LIMIT 1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        let Some(reauditing) = reauditing else {
            return Ok(false);
        };
        let day_end: i64 = reauditing.try_get("bucket_end")?;
        let cursor: i64 = reauditing.try_get("cursor")?;
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if day_end > latest_closed {
            return Ok(false);
        }
        if cursor < day_end {
            return Ok(false);
        }
        let pending_slices: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_work_items
            WHERE status = 'pending' AND range_start >= $1 AND range_start < $2
            "#,
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
        .await?;
        if pending_slices > 0 {
            return Ok(false);
        }
        self.seal_dashboard_rollup_day_pg(day_start, now).await?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            let seal: (Option<i64>, i64, i64) = sqlx::query_as(
                "SELECT source_fence, source_version, durable_source_version FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
            )
            .bind(day_start)
            .fetch_one(&mut *conn)
            .await?;
            let current_fence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let current_durable_source_version: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
                .bind(day_end)
                .fetch_one(&mut *conn)
                .await?;
            let current_source_version = current_durable_source_version.saturating_add(
                self.request_stats_coalescer
                    .dashboard_rollup_source_version(day_start, day_end)
                    .await,
            );
            if !self
                .request_stats_coalescer
                .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
            {
                return Err(ProxyError::Other(
                    "dashboard rollup source mutation is still in flight".to_string(),
                ));
            }
            if seal != (
                Some(current_fence),
                current_source_version,
                current_durable_source_version,
            ) {
                return Err(ProxyError::Other(
                    "dashboard rollup source changed while finalizing the day seal".to_string(),
                ));
            }
            sqlx::query(
                "DELETE FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1",
            )
            .bind(day_start)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(true)
    }

    pub(crate) async fn seal_dashboard_rollup_day_pg(&self, day_start: i64, now: i64) -> Result<(), ProxyError> {
        let day_end = next_local_day_start_utc_ts(day_start);
        let latest_closed = now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS);
        if day_end > latest_closed {
            return Err(ProxyError::Other(
                "dashboard rollup day seal requires a closed local day".to_string(),
            ));
        }
        let gc_blocking_recovery: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1::bigint FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending' AND gc_blocking = 1)",
        )
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        let source_fence_before: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
        .await?;
        if !self
            .request_stats_coalescer
            .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
        {
            return Err(ProxyError::Other(
                "dashboard rollup source mutation is still in flight".to_string(),
            ));
        }
        let (durable_source_version_before, source_version_before) = self
            .dashboard_rollup_integrity_source_versions_pg(day_start, day_end)
            .await?;
        let source_counts = self
            .load_dashboard_rollup_counts_pg(day_start, day_end)
            .await?
            .into_values()
            .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                total.add(value);
                total
            });
        let source_fence_after: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
        .await?;
        let (durable_source_version_after, source_version_after) = self
            .dashboard_rollup_integrity_source_versions_pg(day_start, day_end)
            .await?;
        if source_fence_before != source_fence_after
            || durable_source_version_before != durable_source_version_after
            || source_version_before != source_version_after
            || !self
                .request_stats_coalescer
                .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
        {
            return Err(ProxyError::Other(
                "dashboard rollup source changed while sealing the day".to_string(),
            ));
        }
        let counts_json = serde_json::to_string(&source_counts)
            .map_err(|err| ProxyError::Other(format!("serialize dashboard day seal: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            let current_fence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let current_durable_source_version: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
            .bind(day_end)
                .fetch_one(&mut *conn)
                .await?;
            let current_source_version = current_durable_source_version.saturating_add(
                self.request_stats_coalescer
                    .dashboard_rollup_source_version(day_start, day_end)
                    .await,
            );
            if !self
                .request_stats_coalescer
                .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
            {
                return Err(ProxyError::Other(
                    "dashboard rollup source mutation is still in flight".to_string(),
                ));
            }
            let revision_rows = sqlx::query(
                "SELECT bucket_start, revision FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_all(&mut *conn)
            .await?;
            let revisions = revision_rows.into_iter().try_fold(
                BTreeMap::<i64, i64>::new(),
                |mut revisions, row| {
                    revisions.insert(row.try_get("bucket_start")?, row.try_get("revision")?);
                    Ok::<_, sqlx::Error>(revisions)
                },
            )?;
            let done_work_items = sqlx::query(
                r#"
                SELECT item.range_start, item.range_end, item.source_fence, item.source_version,
                       item.durable_source_version,
                       COALESCE(
                           (
                               SELECT MAX(id)
                               FROM observability.request_logs
                               WHERE visibility = $1
                                 AND created_at >= item.range_start
                                 AND created_at < item.range_end
                           ),
                           0
                       ) AS current_source_fence
                FROM observability.dashboard_rollup_integrity_work_items
                AS item
                WHERE status = 'done'
                  AND range_start >= $2 AND range_end <= $3
                  AND ($4 = 1 OR item.priority = 3 OR item.recovery = 1)
                "#,
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .bind(i64::from(!gc_blocking_recovery))
            .fetch_all(&mut *conn)
            .await?;
            let mut stale_work_item = false;
            for item in done_work_items {
                let item_range_start: i64 = item.try_get("range_start")?;
                let item_range_end: i64 = item.try_get("range_end")?;
                let item_durable_source_version = revisions
                    .range(item_range_start..item_range_end)
                    .map(|(_, revision)| *revision)
                    .fold(0_i64, i64::saturating_add);
                let item_source_version = item_durable_source_version.saturating_add(
                    self.request_stats_coalescer
                        .dashboard_rollup_source_version(item_range_start, item_range_end)
                        .await,
                );
                if item.try_get::<i64, _>("source_fence")?
                    < item.try_get::<i64, _>("current_source_fence")?
                    || item.try_get::<i64, _>("source_version")? != item_source_version
                    || item.try_get::<i64, _>("durable_source_version")?
                        != item_durable_source_version
                {
                    stale_work_item = true;
                    break;
                }
            }
            if current_fence != source_fence_before
                || current_durable_source_version != durable_source_version_before
                || current_source_version != source_version_before
                || stale_work_item
            {
                return Err(ProxyError::Other(
                    "dashboard rollup source changed while sealing the day".to_string(),
                ));
            }
            sqlx::query(
                "DELETE FROM observability.dashboard_request_rollup_buckets WHERE bucket_secs = $1 AND bucket_start = $2",
            )
            .bind(SECS_PER_DAY)
            .bind(day_start)
            .execute(&mut *conn)
            .await?;
            Self::insert_dashboard_rollup_bucket_exact_pg(
                &mut conn,
                day_start,
                SECS_PER_DAY,
                source_counts,
                now,
            )
            .await?;
            sqlx::query(
                r#"
                INSERT INTO observability.dashboard_rollup_daily_seals (
                    bucket_start, counts_json, verified_at, source_fence, source_version, durable_source_version
                ) VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT(bucket_start) DO UPDATE SET
                    counts_json = excluded.counts_json,
                    verified_at = excluded.verified_at,
                    source_fence = excluded.source_fence,
                    source_version = excluded.source_version,
                    durable_source_version = excluded.durable_source_version
                "#,
            )
            .bind(day_start)
            .bind(counts_json)
            .bind(now)
            .bind(source_fence_before)
            .bind(source_version_before)
            .bind(durable_source_version_before)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn verify_next_dashboard_rollup_daily_seal_pg(&self, now: i64) -> Result<(), ProxyError> {
        let cursor: Option<i64> = sqlx::query_scalar(
            "SELECT seal_cursor FROM observability.dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(self.pg()?)
        .await?;
        let row = sqlx::query(
            r#"
            SELECT bucket_start, counts_json FROM observability.dashboard_rollup_daily_seals
            WHERE bucket_start > COALESCE($1, -9223372036854775808)
            ORDER BY bucket_start ASC LIMIT 1
            "#,
        )
        .bind(cursor)
        .fetch_optional(self.pg()?)
        .await?;
        let Some(row) = row else {
            let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
            let write_result = sqlx::query("UPDATE observability.dashboard_rollup_integrity_state SET seal_cursor = NULL, updated_at = $1 WHERE id = 1")
                .bind(now)
                .execute(&mut *conn)
                .await
                .map(|_| ())
                .map_err(Into::into);
            self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
                .await?;
            return Ok(());
        };
        let day_start: i64 = row.try_get("bucket_start")?;
        let expected: DashboardRequestRollupCounts = serde_json::from_str(&row.try_get::<String, _>("counts_json")?)
            .map_err(|err| ProxyError::Other(format!("invalid dashboard day seal: {err}")))?;
        let minute_actual = self
            .load_dashboard_rollup_counts_pg(day_start, next_local_day_start_utc_ts(day_start))
            .await?
            .into_values()
            .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                total.add(value);
                total
            });
        let daily_actual = self
            .load_dashboard_rollup_bucket_pg(day_start, SECS_PER_DAY)
            .await?;
        let retained_source_exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1::bigint FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3 LIMIT 1",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(next_local_day_start_utc_ts(day_start))
        .fetch_optional(self.pg()?)
        .await?;
        let should_restore_daily = minute_actual == expected && daily_actual != expected;
        let should_restore_expired_day = retained_source_exists.is_none() && minute_actual != expected;
        if retained_source_exists.is_some() && minute_actual != expected {
            self.enqueue_dashboard_rollup_integrity_day_reaudit_pg(day_start, now, false)
                .await?;
        } else if should_restore_daily || should_restore_expired_day {
            let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
            let write_result = async {
                sqlx::query("DELETE FROM observability.dashboard_request_rollup_buckets WHERE bucket_secs = $1 AND bucket_start = $2")
                    .bind(SECS_PER_DAY)
                    .bind(day_start)
                    .execute(&mut *conn)
                    .await?;
                Self::insert_dashboard_rollup_bucket_exact_pg(
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
            self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
                .await?;
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query("UPDATE observability.dashboard_rollup_integrity_state SET seal_cursor = $1, updated_at = $2 WHERE id = 1")
            .bind(day_start)
            .bind(now)
            .execute(&mut *conn)
            .await
            .map(|_| ())
            .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn dashboard_rollup_integrity_seal_verification_due_pg(
        &self,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let row = sqlx::query(
            "SELECT hot_cursor, hot_fence, last_seal_attempt_at FROM observability.dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(self.pg()?)
        .await?;
        let hot_cursor: i64 = row.try_get("hot_cursor")?;
        let hot_fence: i64 = row.try_get("hot_fence")?;
        let last_attempt: Option<i64> = row.try_get("last_seal_attempt_at")?;
        Ok(hot_cursor >= hot_fence
            && last_attempt
                .map(|attempted| now.saturating_sub(attempted) >= 60)
                .unwrap_or(true))
    }

    pub(crate) async fn mark_dashboard_rollup_integrity_seal_attempt_pg(
        &self,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            "UPDATE observability.dashboard_rollup_integrity_state SET last_seal_attempt_at = $1, updated_at = $2 WHERE id = 1",
        )
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await
    }

    pub(crate) async fn load_dashboard_rollup_bucket_pg(
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
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_start = $1 AND bucket_secs = $2
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_secs)
        .fetch_optional(self.pg()?)
        .await?;
        row.map(|row| Self::dashboard_rollup_counts_from_row_pg(&row))
            .transpose()
            .map(|counts| counts.unwrap_or_default())
            .map_err(Into::into)
    }

    pub(crate) async fn mark_dashboard_rollup_integrity_success_pg(
        &self,
        now: i64,
        next_delay_secs: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE observability.dashboard_rollup_integrity_state
            SET last_verified_at = $1, stalled_since = NULL, last_error = NULL,
                next_attempt_at = $2, updated_at = $3
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
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn mark_dashboard_rollup_integrity_failure_pg(
        &self,
        err: &ProxyError,
        next_attempt_at: i64,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE observability.dashboard_rollup_integrity_state
            SET stalled_since = COALESCE(stalled_since, $1), last_error = $2,
                next_attempt_at = $3, updated_at = $4
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
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_decision_for_day_inner_pg(
        &self,
        threshold: i64,
        target_day: Option<(i64, i64)>,
        outer_deadline: Option<tokio::time::Instant>,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self.pg()?.acquire().await?;
        let result: Result<DashboardRollupRequestLogGcDecision, ProxyError> = async {
            let day = if let Some((day_start, day_end)) = target_day {
                Some((day_start, day_end))
            } else {
                sqlx::query_scalar::<_, Option<i64>>(
                    "SELECT MIN(created_at) FROM observability.request_logs WHERE visibility = $1 AND created_at < $2",
                )
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .bind(threshold)
                .fetch_one(&mut *conn)
                .await?
                .map(|oldest| {
                    let day_start = local_day_bucket_start_utc_ts(oldest);
                    (day_start, next_local_day_start_utc_ts(day_start))
                })
            };
            let Some((day_start, day_end)) = day else {
                return Ok(DashboardRollupRequestLogGcDecision::Allowed {
                    cutoff: threshold,
                    guard: None,
                });
            };
            if day_end > threshold {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "retention_boundary",
                });
            }
            let blocked_reason: Option<String> = sqlx::query_scalar(
                "SELECT reason FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE day_start = $1 AND next_probe_at > $2",
            )
            .bind(day_start)
            .bind(now)
            .fetch_optional(&mut *conn)
            .await?;
            if let Some(reason) = blocked_reason.filter(|reason| {
                matches!(
                    reason.as_str(),
                    "source_baseline" | "deleted_source_baseline_unavailable"
                )
            }) {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: if reason == "source_baseline" {
                        "source_baseline"
                    } else {
                        "deleted_source_baseline_unavailable"
                    },
                });
            }
            let reaudit_pending: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1::bigint FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending')",
            )
            .bind(day_start)
            .fetch_one(&mut *conn)
            .await?;
            if reaudit_pending {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "reaudit_pending",
                });
            }
            let sealed: Option<(String, Option<i64>, i64, i64)> = sqlx::query_as(
                "SELECT counts_json, source_fence, source_version, durable_source_version FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
            )
            .bind(day_start)
            .fetch_optional(&mut *conn)
            .await?;
            let Some((counts_json, sealed_fence, sealed_version, sealed_durable_source_version)) = sealed else {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "missing_seal",
                });
            };
            let current_fence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(id), 0) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let current_durable_source_version: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
            .bind(day_end)
                .fetch_one(&mut *conn)
                .await?;
            let current_version = current_durable_source_version.saturating_add(
                self.request_stats_coalescer
                    .dashboard_rollup_source_version(day_start, day_end)
                    .await,
            );
            if !self
                .request_stats_coalescer
                .dashboard_rollup_source_mutations_are_stable(day_start, day_end)
            {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "source_changed",
                });
            }
            // A fenced GC pass may lower MAX(id) without changing the durable
            // source revision. A fence that moves forward still indicates a
            // late insert; the deleted-source baseline below covers missing
            // rows from legacy or untracked deletion paths.
            let source_fence_advanced = match sealed_fence {
                Some(sealed_fence) => current_fence > sealed_fence,
                None => current_fence != 0,
            };
            if source_fence_advanced
                || sealed_version != current_version
                || sealed_durable_source_version != current_durable_source_version
            {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "source_changed",
                });
            }
            let expected: DashboardRequestRollupCounts = serde_json::from_str(&counts_json)
                .map_err(|err| ProxyError::Other(format!("invalid dashboard day seal: {err}")))?;
            let baseline_read_budget = dashboard_integrity_read_budget(outer_deadline);
            if baseline_read_budget.is_zero() {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "recovery_budget",
                });
            }
            let (retained_source_rows, deleted_source_rows) = match self
                .dashboard_rollup_gc_deleted_source_row_counts(
                    day_start,
                    day_end,
                    baseline_read_budget,
                    outer_deadline,
                )
                .await
            {
                Ok(counts) => counts,
                Err(error) if error.is_deferred() => {
                    let reason = match &error {
                        ProxyError::Deferred { reason, .. } if reason == "recovery_budget" => {
                            "recovery_budget"
                        }
                        _ => "read_budget",
                    };
                    return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                        day_start,
                        reason,
                    });
                }
                Err(error)
                    if matches!(&error, ProxyError::Database(sqlx::Error::PoolTimedOut)) =>
                {
                    return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                        day_start,
                        reason: "read_budget",
                    });
                }
                Err(error) => return Err(error),
            };
            if retained_source_rows.saturating_add(deleted_source_rows)
                < expected.total_requests
            {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "deleted_source_baseline_unavailable",
                });
            }
            let minute_rows = sqlx::query(
                r#"
            SELECT bucket_start, total_requests, success_count, error_count, quota_exhausted_count,
                   valuable_success_count, valuable_failure_count, valuable_failure_429_count,
                   other_success_count, other_failure_count, unknown_count, mcp_non_billable,
                   mcp_billable, api_non_billable, api_billable, local_estimated_credits
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_secs = $1 AND bucket_start >= $2 AND bucket_start < $3
            "#,
            )
            .bind(SECS_PER_MINUTE)
            .bind(day_start)
            .bind(day_end)
            .fetch_all(&mut *conn)
            .await?;
            let minute_actual = minute_rows
                .into_iter()
                .map(|row| Self::dashboard_rollup_counts_from_row_pg(&row))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .fold(DashboardRequestRollupCounts::default(), |mut total, value| {
                    total.add(value);
                    total
                });
            let daily_row = sqlx::query(
                r#"
            SELECT total_requests, success_count, error_count, quota_exhausted_count,
                   valuable_success_count, valuable_failure_count, valuable_failure_429_count,
                   other_success_count, other_failure_count, unknown_count, mcp_non_billable,
                   mcp_billable, api_non_billable, api_billable, local_estimated_credits
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_start = $1 AND bucket_secs = $2
            "#,
            )
            .bind(day_start)
            .bind(SECS_PER_DAY)
            .fetch_optional(&mut *conn)
            .await?;
            let daily_actual = daily_row
                .map(|row| Self::dashboard_rollup_counts_from_row_pg(&row))
                .transpose()?
                .unwrap_or_default();
            if minute_actual != expected || daily_actual != expected {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "rollup_mismatch",
                });
            }
            // Existing source recovery must finish before its raw truth is deleted.
            for pending_query in [
                "SELECT EXISTS(SELECT 1::bigint FROM observability.dashboard_rollup_integrity_work_items WHERE status = 'pending' AND range_start < $1 AND range_end > $2)",
                "SELECT EXISTS(SELECT 1::bigint FROM observability.dashboard_rollup_rebalance_recovery WHERE status = 'pending' AND cursor < $1 AND range_end > $2)",
            ] {
                    let pending: bool = sqlx::query_scalar(pending_query)
                        .bind(day_end)
                        .bind(day_start)
                        .fetch_one(&mut *conn)
                        .await?;
                    if pending {
                        return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                            day_start,
                            reason: "source_recovery_pending",
                        });
                    }
            }
            Ok(DashboardRollupRequestLogGcDecision::Allowed {
                cutoff: day_end,
                guard: Some(DashboardRollupRequestLogGcGuard {
                    day_start,
                    day_end,
                    source_fence: current_fence,
                    source_version: current_version,
                    durable_source_version: current_durable_source_version,
                }),
            })
        }
        .await;
        drop(conn);
        let decision = result?;
        match decision {
            DashboardRollupRequestLogGcDecision::Blocked {
                day_start,
                reason: reason @ ("reaudit_pending"
                | "missing_seal"
                | "rollup_mismatch"
                | "source_changed"
                | "deleted_source_baseline_unavailable"),
            } => self.block_request_log_gc_for_day(day_start, reason).await,
            DashboardRollupRequestLogGcDecision::Allowed {
                cutoff,
                guard: Some(guard),
            } => {
                self.clear_dashboard_rollup_integrity_blocked_range(guard.day_start)
                    .await?;
                Ok(DashboardRollupRequestLogGcDecision::Allowed {
                    cutoff,
                    guard: Some(guard),
                })
            }
            decision => Ok(decision),
        }
    }
}
