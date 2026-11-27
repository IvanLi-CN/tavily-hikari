impl KeyStore {
    pub(crate) async fn process_dashboard_rollup_integrity_work_item_pg(
        &self,
        item: DashboardRollupIntegrityWorkItem,
        now: i64,
        deadline: Option<tokio::time::Instant>,
        recovery_target: bool,
        claimed_probe_day_start: Option<i64>,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        let Some(deadline) = deadline else {
            return self
                .process_dashboard_rollup_integrity_work_item_inner_pg(
                    item,
                    now,
                    None,
                    recovery_target,
                    claimed_probe_day_start,
                )
                .await;
        };
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs: 1,
                reason: "recovery_budget",
            });
        }
        let range_start = item.range_start;
        match tokio::time::timeout(
            remaining,
            self.process_dashboard_rollup_integrity_work_item_inner_pg(
                item,
                now,
                Some(deadline),
                recovery_target,
                claimed_probe_day_start,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                // The inner path may have installed a coalescer repair barrier
                // before its final database observation. Cancellation is safe
                // for the immediate transaction, but the in-memory barrier
                // still needs an explicit rollback marker before the next turn.
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(range_start, false)
                    .await;
                Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs: 1,
                    reason: "recovery_budget",
                })
            }
        }
    }

    pub(crate) async fn begin_dashboard_integrity_read_pg(
        &self,
        read_budget: Duration,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, ProxyError> {
        let deadline = tokio::time::Instant::now() + read_budget;
        let mut tx = tokio::time::timeout_at(deadline, self.pg()?.begin()).await
            .map_err(|_| Self::dashboard_integrity_read_deferred())??;
        let setup = async {
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
                .execute(&mut *tx).await?;
            let millis = read_budget.as_millis().max(1).to_string();
            sqlx::query("SELECT set_config('statement_timeout', $1, true)")
                .bind(millis).execute(&mut *tx).await?;
            Ok::<(), sqlx::Error>(())
        };
        tokio::time::timeout_at(deadline, setup).await
            .map_err(|_| Self::dashboard_integrity_read_deferred())??;
        Ok(tx)
    }

    pub(crate) async fn process_dashboard_rollup_integrity_work_item_inner_pg(
        &self,
        mut item: DashboardRollupIntegrityWorkItem,
        now: i64,
        deadline: Option<tokio::time::Instant>,
        recovery_target: bool,
        claimed_probe_day_start: Option<i64>,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        if deadline
            .map(|deadline| tokio::time::Instant::now() >= deadline)
            .unwrap_or(false)
        {
            return Ok(dashboard_integrity_deferred(1, "recovery_budget"));
        }
        let day_start = local_day_bucket_start_utc_ts(item.range_start);
        if let Some(next_delay_secs) = self
            .dashboard_rollup_integrity_blocked_range_probe_delay(
                day_start,
                now,
                claimed_probe_day_start,
            )
            .await?
        {
            return Ok(dashboard_integrity_deferred(
                next_delay_secs,
                "source_baseline_probe_not_due",
            ));
        }
        let work_delay_budget = dashboard_integrity_read_budget(deadline);
        if work_delay_budget.is_zero() {
            return Ok(dashboard_integrity_deferred(1, "recovery_budget"));
        }
        let next_delay_secs = match tokio::time::timeout(
            work_delay_budget,
            self.dashboard_rollup_integrity_work_delay_pg(item.range_start),
        )
        .await
        {
            Ok(value) => value?,
            Err(_) => {
                return Ok(dashboard_integrity_deferred(
                    1,
                    dashboard_integrity_budget_reason(deadline),
                ));
            }
        };
        let day_end = next_local_day_start_utc_ts(day_start);
        let baseline_read_budget = dashboard_integrity_read_budget(deadline);
        if baseline_read_budget.is_zero() {
            return Ok(dashboard_integrity_deferred(next_delay_secs, "recovery_budget"));
        }
        match self
            .dashboard_rollup_gc_deleted_source_baseline_missing_pg(
                day_start,
                day_end,
                baseline_read_budget,
            )
            .await
        {
            Ok(true) => {
                self.persist_dashboard_rollup_integrity_blocked_range(
                    day_start,
                    "deleted_source_baseline_unavailable",
                    false,
                )
                .await?;
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs,
                    reason: "deleted_source_baseline_unavailable",
                });
            }
            Ok(false) => {}
            Err(error) if error.is_deferred() => {
                return Ok(dashboard_integrity_deferred(
                    next_delay_secs,
                    dashboard_integrity_budget_reason(deadline),
                ));
            }
            Err(error) => return Err(error),
        }
        tracing::debug!(
            component = "dashboard_rollup_integrity",
            event = "slice_started",
            lane = "dashboard_integrity",
            target_range_start = item.range_start,
            target_range_end = item.range_end,
            source_fence = item.source_fence_id,
            checkpoint_created_at = ?item.cursor_created_at,
            checkpoint_id = ?item.cursor_id,
            "started a source-backed bounded integrity slice"
        );
        let read_started = StdInstant::now();
        let mut read_budget = dashboard_integrity_read_budget(deadline);
        if read_budget.is_zero() {
            return Ok(dashboard_integrity_deferred(next_delay_secs, "recovery_budget"));
        }
        if item.cursor_created_at.is_none()
            && item.cursor_id.is_none()
            && item.counts.is_empty()
        {
            let counts_read_budget = dashboard_integrity_read_budget(deadline);
            if counts_read_budget.is_zero() {
                return Ok(dashboard_integrity_deferred(next_delay_secs, "recovery_budget"));
            }
            let deleted_counts = match self
                .load_dashboard_rollup_gc_deleted_source_counts_pg(
                    item.range_start,
                    item.range_end,
                    counts_read_budget,
                )
                .await
            {
                Ok(counts) => counts,
                Err(error) if error.is_deferred() => {
                    return Ok(dashboard_integrity_deferred(
                        next_delay_secs,
                        dashboard_integrity_budget_reason(deadline),
                    ));
                }
                Err(error) => return Err(error),
            };
            for (minute_start, counts) in deleted_counts {
                item.counts.entry(minute_start).or_default().add(counts);
            }
        }
        read_budget = dashboard_integrity_read_budget(deadline);
        if read_budget.is_zero() {
            return Ok(dashboard_integrity_deferred(next_delay_secs, "recovery_budget"));
        }
        let read_deadline = tokio::time::Instant::now() + read_budget;
        let mut snapshot = self.begin_dashboard_integrity_read_pg(read_budget).await?;
        let result = tokio::time::timeout_at(read_deadline, async { sqlx::query(
            r#"
            SELECT id, created_at, result_status, failure_kind, request_kind_key, request_body,
                   path, business_credits, counts_business_quota
            FROM observability.request_logs
            WHERE visibility = $1
              AND created_at >= $2 AND created_at < $3 AND id <= $4
              AND (
                  $5::bigint IS NULL OR created_at > $6 OR (created_at = $7 AND id > $8)
              )
            ORDER BY created_at ASC, id ASC
            LIMIT $9
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
        .fetch_all(&mut *snapshot)
        .await }).await;
        let rows = match Self::complete_dashboard_integrity_read_pg(snapshot, result).await {
            Ok(rows) => rows,
            Err(error) if error.is_deferred() => {
                return Ok(dashboard_integrity_deferred(next_delay_secs, dashboard_integrity_budget_reason(deadline)));
            }
            Err(error) => return Err(error),
        };
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
            if deadline
                .map(|deadline| tokio::time::Instant::now() >= deadline)
                .unwrap_or(false)
            {
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs,
                    reason: "recovery_budget",
                });
            }
            self.persist_dashboard_rollup_integrity_work_item_pg(&item, now).await?;
            let defer_reason = if rows.len() as i64 >= DASHBOARD_ROLLUP_INTEGRITY_SOURCE_PAGE_ROWS {
                "source_page_limit"
            } else {
                "read_budget"
            };
            tracing::debug!(
                component = "dashboard_rollup_integrity",
                event = "checkpoint_deferred",
                lane = "dashboard_integrity",
                target_range_start = item.range_start,
                target_range_end = item.range_end,
                source_fence = item.source_fence_id,
                checkpoint_created_at = ?item.cursor_created_at,
                checkpoint_id = ?item.cursor_id,
                defer_reason,
                "persisted a bounded source-scan checkpoint"
            );
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: defer_reason,
            });
        }

        if self
            .request_stats_coalescer
            .dashboard_rollup_range_has_uncommitted_changes(item.range_start, item.range_end)
            .await
        {
            if deadline
                .map(|deadline| tokio::time::Instant::now() >= deadline)
                .unwrap_or(false)
            {
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs,
                    reason: "recovery_budget",
                });
            }
            self.persist_dashboard_rollup_integrity_work_item_pg(&item, now).await?;
            tracing::debug!(
                component = "dashboard_rollup_integrity",
                event = "checkpoint_deferred",
                lane = "dashboard_integrity",
                target_range_start = item.range_start,
                target_range_end = item.range_end,
                source_fence = item.source_fence_id,
                checkpoint_created_at = ?item.cursor_created_at,
                checkpoint_id = ?item.cursor_id,
                defer_reason = "relevant_unflushed_statistics",
                "relevant dashboard statistics remain unflushed"
            );
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "relevant_unflushed_statistics",
            });
        }
        if self
            .dashboard_rollup_integrity_source_changed_since_fence_pg(&item)
            .await?
        {
            tracing::debug!(
                component = "dashboard_rollup_integrity",
                event = "checkpoint_deferred",
                lane = "dashboard_integrity",
                target_range_start = item.range_start,
                target_range_end = item.range_end,
                source_fence = item.source_fence_id,
                defer_reason = "source_fence_changed",
                "source data changed before repair began"
            );
            self.restart_dashboard_rollup_integrity_work_item_pg(
                &item,
                now,
                recovery_target,
            )
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "source_fence_changed",
            });
        }

        if deadline
            .map(|deadline| tokio::time::Instant::now() >= deadline)
            .unwrap_or(false)
        {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "recovery_budget",
            });
        }
        self.request_stats_coalescer
            .begin_dashboard_rollup_repair(item.range_start, item.range_end, item.source_fence_id)
            .await;
        let source_changed_after_barrier = match self
            .dashboard_rollup_integrity_source_changed_since_fence_pg(&item)
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
            tracing::debug!(
                component = "dashboard_rollup_integrity",
                event = "checkpoint_deferred",
                lane = "dashboard_integrity",
                target_range_start = item.range_start,
                target_range_end = item.range_end,
                source_fence = item.source_fence_id,
                defer_reason = "source_fence_changed_after_barrier",
                "source data changed after the repair barrier"
            );
            self.request_stats_coalescer
                .finish_dashboard_rollup_repair(item.range_start, false)
                .await;
            self.restart_dashboard_rollup_integrity_work_item_pg(
                &item,
                now,
                recovery_target,
            )
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "source_fence_changed_after_barrier",
            });
        }

        let actual = match self
            .load_dashboard_rollup_counts_pg(item.range_start, item.range_end)
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
            if deadline
                .map(|deadline| tokio::time::Instant::now() >= deadline)
                .unwrap_or(false)
            {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Ok(DashboardRollupIntegritySlice::Deferred {
                    next_delay_secs,
                    reason: "recovery_budget",
                });
            }
            if let Err(err) = self.record_dashboard_rollup_integrity_gap_pg(&item, now).await {
                self.request_stats_coalescer
                    .finish_dashboard_rollup_repair(item.range_start, false)
                    .await;
                return Err(err);
            }
            slow_repair_write = match self.replace_dashboard_rollup_minutes_pg(&item, now).await {
                Ok(slow) => slow,
                Err(err) => {
                    self.request_stats_coalescer
                        .finish_dashboard_rollup_repair(item.range_start, false)
                        .await;
                    return Err(err);
                }
            };
        }
        if deadline
            .map(|deadline| tokio::time::Instant::now() >= deadline)
            .unwrap_or(false)
        {
            self.request_stats_coalescer
                .finish_dashboard_rollup_repair(item.range_start, false)
                .await;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "recovery_budget",
            });
        }
        let source_changed = match self
            .dashboard_rollup_integrity_source_changed_since_fence_pg(&item)
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
            self.record_dashboard_rollup_integrity_gap_pg(&item, now).await?;
            self.restart_dashboard_rollup_integrity_work_item_pg(
                &item,
                now,
                recovery_target,
            )
                .await?;
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "source_fence_changed_after_repair",
            });
        }
        if deadline
            .map(|deadline| tokio::time::Instant::now() >= deadline)
            .unwrap_or(false)
        {
            return Ok(DashboardRollupIntegritySlice::Deferred {
                next_delay_secs,
                reason: "recovery_budget",
            });
        }
        self.clear_dashboard_rollup_integrity_gap_pg(item.range_start).await?;
        self.finish_dashboard_rollup_integrity_work_item_pg(&item, now).await?;
        let day_start = local_day_bucket_start_utc_ts(item.range_start);
        let day_recovery_sealed = if self
            .complete_dashboard_rollup_integrity_day_reaudit_if_ready_pg(day_start, now)
            .await?
        {
            // The final source-backed minute slice recreated the day rollup and seal.
            true
        } else if mismatch {
            self.refresh_dashboard_rollup_daily_seal_after_repair_pg(day_start, now)
                .await?;
            false
        } else {
            self.maybe_seal_dashboard_rollup_day_pg(day_start, now).await?;
            false
        };
        let next_delay_secs = if slow_repair_write {
            next_delay_secs.max(60)
        } else {
            next_delay_secs
        };
        self.mark_dashboard_rollup_integrity_success_pg(now, next_delay_secs)
            .await?;
        tracing::info!(
            component = "dashboard_rollup_integrity",
            event = "checkpoint_accepted",
            lane = "dashboard_integrity",
            target_range_start = item.range_start,
            target_range_end = item.range_end,
            source_fence = item.source_fence_id,
            checkpoint_created_at = ?item.cursor_created_at,
            checkpoint_id = ?item.cursor_id,
            mismatch,
            day_start,
            sealed_day = day_recovery_sealed,
            "accepted a source-backed integrity checkpoint"
        );
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

    pub(crate) async fn dashboard_rollup_integrity_source_changed_since_fence_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
    ) -> Result<bool, ProxyError> {
        let latest_source_id: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(id), 0) FROM observability.request_logs
            WHERE visibility = $1 AND created_at >= $2 AND created_at < $3
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(item.range_start)
        .bind(item.range_end)
        .fetch_one(self.pg()?)
        .await?;
        let (durable_source_version, source_version) = self
            .dashboard_rollup_integrity_source_versions_pg(item.range_start, item.range_end)
            .await?;
        Ok(latest_source_id > item.source_fence_id
            || source_version != item.source_version
            || durable_source_version != item.durable_source_version
            || !self
                .request_stats_coalescer
                .dashboard_rollup_source_mutations_are_stable(item.range_start, item.range_end))
    }

    pub(crate) async fn restart_dashboard_rollup_integrity_work_item_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
        recovery_target: bool,
    ) -> Result<(), ProxyError> {
        let source_fence_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM observability.request_logs")
            .fetch_one(self.pg()?)
            .await?;
        let (durable_source_version, source_version) = self
            .dashboard_rollup_integrity_source_versions_pg(item.range_start, item.range_end)
            .await?;
        let counts_json = serde_json::to_string(&BTreeMap::<i64, DashboardRequestRollupCounts>::new())
            .map_err(|err| {
                ProxyError::Other(format!("serialize restarted integrity work item: {err}"))
            })?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            sqlx::query(
                r#"
                UPDATE observability.dashboard_rollup_integrity_work_items
                SET source_fence = $1, source_version = $2, durable_source_version = $3, cursor_created_at = NULL, cursor_id = NULL, counts_json = $4,
                    status = 'pending', updated_at = $5
                WHERE range_start = $6
                "#,
            )
            .bind(source_fence_id)
            .bind(source_version)
            .bind(durable_source_version)
            .bind(counts_json)
            .bind(now)
            .bind(item.range_start)
            .execute(&mut *conn)
            .await?;
            if recovery_target {
                let day_start = local_day_bucket_start_utc_ts(item.range_start);
                sqlx::query(
                    "UPDATE observability.dashboard_rollup_integrity_day_reaudits SET source_fence = $1, cursor = bucket_start, status = 'pending', updated_at = $2 WHERE bucket_start = $3",
                )
                .bind(source_fence_id)
                .bind(now)
                .bind(day_start)
                .execute(&mut *conn)
                .await?;
            }
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await
    }

    pub(crate) async fn dashboard_rollup_integrity_work_delay_pg(
        &self,
        range_start: i64,
    ) -> Result<i64, ProxyError> {
        let gc_blocking: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1::bigint FROM observability.dashboard_rollup_integrity_day_reaudits WHERE gc_blocking = 1 AND status = 'pending' AND bucket_start <= $1 AND bucket_end > $2)",
        ).bind(range_start).bind(range_start).fetch_one(self.pg()?).await?;
        if gc_blocking {
            return Ok(1);
        }
        let hot_fence: i64 = sqlx::query_scalar(
            "SELECT hot_fence FROM observability.dashboard_rollup_integrity_state WHERE id = 1",
        )
        .fetch_one(self.pg()?)
        .await?;
        Ok(if range_start >= hot_fence.saturating_sub(DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS) {
            15
        } else {
            60
        })
    }

    pub(crate) async fn persist_dashboard_rollup_integrity_work_item_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let counts_json = serde_json::to_string(&item.counts)
            .map_err(|err| ProxyError::Other(format!("serialize integrity work item: {err}")))?;
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            UPDATE observability.dashboard_rollup_integrity_work_items
            SET cursor_created_at = $1, cursor_id = $2, counts_json = $3,
                priority = CASE WHEN priority = 4 THEN $4 ELSE priority END,
                updated_at = $5
            WHERE range_start = $6 AND status = 'pending'
            "#,
        )
        .bind(item.cursor_created_at)
        .bind(item.cursor_id)
        .bind(counts_json)
        .bind(DASHBOARD_ROLLUP_INTEGRITY_CONTINUED_HOT_PRIORITY)
        .bind(now)
        .bind(item.range_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn load_dashboard_rollup_counts_pg(
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
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_secs = $1 AND bucket_start >= $2 AND bucket_start < $3
            "#,
        )
        .bind(SECS_PER_MINUTE)
        .bind(range_start)
        .bind(range_end)
        .fetch_all(self.pg()?)
        .await?;
        rows.into_iter()
            .map(|row| Ok((row.try_get("bucket_start")?, Self::dashboard_rollup_counts_from_row_pg(&row)?)))
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }

    pub(crate) async fn load_dashboard_rollup_gc_deleted_source_counts_pg(
        &self,
        range_start: i64,
        range_end: i64,
        read_budget: Duration,
    ) -> Result<BTreeMap<i64, DashboardRequestRollupCounts>, ProxyError> {
        let read_deadline = tokio::time::Instant::now() + read_budget;
        let mut snapshot = self.begin_dashboard_integrity_read_pg(read_budget).await?;
        let result = tokio::time::timeout_at(read_deadline, async { sqlx::query(
            r#"
            SELECT bucket_start, counts_json
            FROM observability.dashboard_rollup_gc_deleted_source_contributions
            WHERE bucket_start >= $1 AND bucket_start < $2
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut *snapshot)
        .await }).await;
        let rows = Self::complete_dashboard_integrity_read_pg(snapshot, result).await?;
        rows.into_iter()
            .map(|row| {
                let bucket_start: i64 = row.try_get("bucket_start")?;
                let counts_json: String = row.try_get("counts_json")?;
                let counts = serde_json::from_str(&counts_json).map_err(|err| {
                    ProxyError::Other(format!(
                        "invalid deleted dashboard source contribution: {err}"
                    ))
                })?;
                Ok((bucket_start, counts))
            })
            .collect()
    }

    pub(crate) async fn dashboard_rollup_gc_deleted_source_baseline_missing_pg(
        &self,
        day_start: i64,
        day_end: i64,
        read_budget: Duration,
    ) -> Result<bool, ProxyError> {
        let read_deadline = tokio::time::Instant::now() + read_budget;
        let mut snapshot = self.begin_dashboard_integrity_read_pg(read_budget).await?;
        let result = tokio::time::timeout_at(read_deadline, async {
            let counts_json = sqlx::query_scalar::<_, String>(
                "SELECT counts_json FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
            )
            .bind(day_start)
            .fetch_optional(&mut *snapshot)
            .await?;
            let Some(counts_json) = counts_json else {
                return Ok((None, 0, 0));
            };
            let retained_source_rows = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *snapshot)
            .await?;
            let deleted_source_rows = sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(SUM((counts_json::jsonb ->> 'total_requests')::bigint), 0)::bigint FROM observability.dashboard_rollup_gc_deleted_source_contributions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *snapshot)
            .await?;
            Ok::<_, sqlx::Error>((Some(counts_json), retained_source_rows, deleted_source_rows))
        })
        .await;
        let (counts_json, retained_source_rows, deleted_source_rows) =
            Self::complete_dashboard_integrity_read_pg(snapshot, result).await?;
        let Some(counts_json) = counts_json else {
            return Ok(false);
        };
        let sealed_counts: DashboardRequestRollupCounts = serde_json::from_str(&counts_json)
            .map_err(|err| ProxyError::Other(format!("invalid dashboard day seal: {err}")))?;
        if sealed_counts.total_requests <= 0 {
            return Ok(false);
        }
        Ok(retained_source_rows.saturating_add(deleted_source_rows) < sealed_counts.total_requests)
    }

    pub(crate) async fn record_dashboard_rollup_integrity_gap_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            r#"
            INSERT INTO observability.dashboard_rollup_integrity_gaps (
                range_start, range_end, detected_at, updated_at, reason
            ) VALUES ($1, $2, $3, $4, 'source_rollup_mismatch')
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
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn clear_dashboard_rollup_integrity_gap_pg(&self, range_start: i64) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query("DELETE FROM observability.dashboard_rollup_integrity_gaps WHERE range_start = $1")
            .bind(range_start)
            .execute(&mut *conn)
            .await
            .map(|_| ())
            .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) async fn replace_dashboard_rollup_minutes_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let started = StdInstant::now();
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = async {
            sqlx::query(
                "DELETE FROM observability.dashboard_request_rollup_buckets WHERE bucket_secs = $1 AND bucket_start >= $2 AND bucket_start < $3",
            )
            .bind(SECS_PER_MINUTE)
            .bind(item.range_start)
            .bind(item.range_end)
            .execute(&mut *conn)
            .await?;
            for (bucket_start, counts) in &item.counts {
                Self::insert_dashboard_rollup_bucket_exact_pg(
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
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
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

    pub(crate) async fn insert_dashboard_rollup_bucket_exact_pg(
        conn: &mut sqlx::PgConnection,
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
            INSERT INTO observability.dashboard_request_rollup_buckets (
                bucket_start, bucket_secs, total_requests, success_count, error_count,
                quota_exhausted_count, valuable_success_count, valuable_failure_count,
                valuable_failure_429_count, other_success_count, other_failure_count,
                unknown_count, mcp_non_billable, mcp_billable, api_non_billable, api_billable,
                local_estimated_credits, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
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

    pub(crate) async fn begin_dashboard_rollup_integrity_short_write_pg(
        &self,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, ProxyError> {
        Ok(self.pg()?.begin().await?)
    }

    pub(crate) async fn finish_dashboard_rollup_integrity_short_write_pg(
        &self,
        conn: sqlx::Transaction<'_, sqlx::Postgres>,
        write_result: Result<(), ProxyError>,
    ) -> Result<(), ProxyError> {
        match write_result {
            Ok(()) => { conn.commit().await?; Ok(()) }
            Err(error) => { let _ = conn.rollback().await; Err(error) }
        }
    }

    pub(crate) async fn finish_dashboard_rollup_integrity_work_item_pg(
        &self,
        item: &DashboardRollupIntegrityWorkItem,
        now: i64,
    ) -> Result<(), ProxyError> {
        let mut conn = self.begin_dashboard_rollup_integrity_short_write_pg().await?;
        let write_result = sqlx::query(
            "UPDATE observability.dashboard_rollup_integrity_work_items SET status = 'done', updated_at = $1 WHERE range_start = $2",
        )
        .bind(now)
        .bind(item.range_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write_pg(conn, write_result)
            .await?;
        Ok(())
    }

    pub(crate) fn dashboard_rollup_counts_from_row_pg(
        row: &sqlx::postgres::PgRow,
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

    async fn complete_dashboard_integrity_read_pg<T>(
        snapshot: sqlx::Transaction<'_, sqlx::Postgres>,
        result: Result<Result<T, sqlx::Error>, tokio::time::error::Elapsed>,
    ) -> Result<T, ProxyError> {
        let result = match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(sqlx::Error::Database(error))) if error.code().as_deref() == Some("57014") => {
                Err(Self::dashboard_integrity_read_deferred())
            }
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(Self::dashboard_integrity_read_deferred()),
        };
        let close = snapshot.rollback().await;
        match (result, close) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }
}
