impl KeyStore {
    pub(crate) async fn run_dashboard_rollup_integrity_slice(
        &self,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        let now = self.backend_time.now_ts();
        self.ensure_dashboard_rollup_integrity_state(now).await?;
        self.ensure_dashboard_rollup_rebalance_recovery(now).await?;
        let claimed_probe_day_start = if let Some((day_start, day_end, probe_generation)) =
            self.claim_dashboard_rollup_integrity_probe(now).await?
        {
            self.enqueue_dashboard_rollup_integrity_day_reaudit(day_start, now, true)
                .await?;
            tracing::info!(
                component = "dashboard_rollup_integrity",
                event = "blocked_range_probe",
                day_start,
                day_end,
                probe_generation,
                "claimed a due blocked range for a bounded source probe"
            );
            Some(day_start)
        } else {
            None
        };
        if self.dashboard_rollup_integrity_hot_work_due(now).await?
            || self.dashboard_rollup_integrity_gc_work_due().await?
        {
            self.create_next_dashboard_rollup_integrity_work_item(now, claimed_probe_day_start)
                .await?;
        }
        if let Some(item) = self
            .load_dashboard_rollup_integrity_work_item(now, claimed_probe_day_start)
            .await?
        {
            return self
                .process_dashboard_rollup_integrity_work_item(
                    item,
                    now,
                    None,
                    false,
                    claimed_probe_day_start,
                )
                .await;
        }
        if self.dashboard_rollup_integrity_seal_verification_due(now).await? {
            self.verify_next_dashboard_rollup_daily_seal(now).await?;
            self.mark_dashboard_rollup_integrity_seal_attempt(now).await?;
            if let Some(item) = self
                .load_dashboard_rollup_integrity_work_item(now, claimed_probe_day_start)
                .await?
            {
                return self
                    .process_dashboard_rollup_integrity_work_item(
                        item,
                        now,
                        None,
                        false,
                        claimed_probe_day_start,
                    )
                    .await;
            }
        }
        if let Some(item) = self
            .create_next_dashboard_rollup_integrity_work_item(now, claimed_probe_day_start)
            .await?
        {
            return self
                .process_dashboard_rollup_integrity_work_item(
                    item,
                    now,
                    None,
                    false,
                    claimed_probe_day_start,
                )
                .await;
        }
        if let Some(item) = self
            .create_next_dashboard_rollup_rebalance_recovery_work_item(now)
            .await?
        {
            return self
                .process_dashboard_rollup_integrity_work_item(
                    item,
                    now,
                    None,
                    false,
                    claimed_probe_day_start,
                )
                .await;
        }
        self.complete_dashboard_rollup_rebalance_recovery_if_ready(now)
            .await?;
        let next_delay_secs = self
            .dashboard_rollup_integrity_next_probe_delay(now)
            .await?
            .map(|next_probe_at| next_probe_at.saturating_sub(now).max(1))
            .unwrap_or(60);
        self.mark_dashboard_rollup_integrity_success(now, next_delay_secs)
            .await?;
        Ok(DashboardRollupIntegritySlice::Verified { next_delay_secs })
    }

    async fn complete_dashboard_rollup_rebalance_recovery_if_ready(
        &self,
        now: i64,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            let Some(row) = sqlx::query(
                "SELECT status, range_start, range_end, cursor FROM observability.dashboard_rollup_rebalance_recovery WHERE id = 1",
            )
            .fetch_optional(self.pg()?)
            .await?
            else {
                return Ok(());
            };
            let status: String = row.try_get("status")?;
            let Some(range_start): Option<i64> = row.try_get("range_start")? else {
                return Ok(());
            };
            let range_end: i64 = row.try_get("range_end")?;
            let cursor: i64 = row.try_get("cursor")?;
            if status == "complete" || cursor < range_end {
                return Ok(());
            }
            let pending_work: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_work_items WHERE status = 'pending' AND range_start >= $1 AND range_start < $2",
            )
            .bind(range_start)
            .bind(range_end)
            .fetch_one(self.pg()?)
            .await?;
            let pending_days: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_day_reaudits WHERE status = 'pending' AND bucket_end > $1 AND bucket_start < $2",
            )
            .bind(range_start)
            .bind(range_end)
            .fetch_one(self.pg()?)
            .await?;
            if pending_work > 0 || pending_days > 0 {
                return Ok(());
            }
            sqlx::query(
                "UPDATE observability.dashboard_rollup_rebalance_recovery SET status = 'complete', completed_at = $1, updated_at = $2, last_error = NULL WHERE id = 1",
            )
            .bind(now)
            .bind(now)
            .execute(self.pg()?)
            .await?;
            tracing::info!(
                component = "dashboard_rollup_integrity",
                event = "rebalance_rollup_recovery_completed",
                range_start,
                range_end,
            );
            return Ok(());
        }
        let Some(row) = sqlx::query(
            "SELECT status, range_start, range_end, cursor FROM dashboard_rollup_rebalance_recovery WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(());
        };
        let status: String = row.try_get("status")?;
        let Some(range_start): Option<i64> = row.try_get("range_start")? else {
            return Ok(());
        };
        let range_end: i64 = row.try_get("range_end")?;
        let cursor: i64 = row.try_get("cursor")?;
        if status == "complete" || cursor < range_end {
            return Ok(());
        }
        let pending_work: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_rollup_integrity_work_items WHERE status = 'pending' AND range_start >= ? AND range_start < ?",
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_one(&self.pool)
        .await?;
        let pending_days: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_rollup_integrity_day_reaudits WHERE status = 'pending' AND bucket_end > ? AND bucket_start < ?",
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_one(&self.pool)
        .await?;
        if pending_work > 0 || pending_days > 0 {
            return Ok(());
        }
        sqlx::query(
            "UPDATE dashboard_rollup_rebalance_recovery SET status = 'complete', completed_at = ?, updated_at = ?, last_error = NULL WHERE id = 1",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        tracing::info!(
            component = "dashboard_rollup_integrity",
            event = "rebalance_rollup_recovery_completed",
            range_start,
            range_end,
        );
        Ok(())
    }

    async fn load_dashboard_rollup_integrity_work_item(
        &self,
        now: i64,
        claimed_probe_day_start: Option<i64>,
    ) -> Result<Option<DashboardRollupIntegrityWorkItem>, ProxyError> {
        if self.pg_pool.is_some() {
            // range_start is the work-item primary key and breaks updated_at ties without rowid.
            let row = sqlx::query(
                r#"
                SELECT range_start, range_end, source_fence, source_version, durable_source_version, cursor_created_at, cursor_id, counts_json
                FROM observability.dashboard_rollup_integrity_work_items
                WHERE status = 'pending'
                  AND (
                      recovery = 0 OR NOT EXISTS (
                          SELECT 1 FROM observability.dashboard_rollup_integrity_state
                          WHERE id = 1
                            AND (hot_cursor < hot_fence OR hot_fence < $1)
                      )
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM observability.dashboard_rollup_integrity_blocked_ranges blocked
                      WHERE blocked.day_start <= range_start
                        AND blocked.day_end > range_start
                        AND blocked.reason IN ('source_baseline', 'deleted_source_baseline_unavailable')
                        AND blocked.next_probe_at > $2
                        AND ($3::bigint IS NULL OR blocked.day_start != $4)
                  )
                ORDER BY CASE
                    WHEN range_start >= $5 AND recovery = 0 AND priority IN (0, 3, 4) THEN 4
                    WHEN EXISTS (SELECT 1 FROM observability.dashboard_rollup_integrity_day_reaudits d
                        WHERE d.gc_blocking = 1 AND d.status = 'pending'
                          AND range_start >= d.bucket_start AND range_end <= d.bucket_end) THEN 3
                    WHEN recovery = 0 AND priority = $6 THEN 2
                    ELSE priority END DESC, updated_at ASC, range_start ASC
                LIMIT 1
                "#,
            )
            .bind(now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS))
            .bind(now)
            .bind(claimed_probe_day_start)
            .bind(claimed_probe_day_start)
            .bind(
                now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
                    - DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS,
            )
            .bind(DASHBOARD_ROLLUP_INTEGRITY_CONTINUED_HOT_PRIORITY)
            .fetch_optional(self.pg()?)
            .await?;
            return row
                .map(|row| {
                    let counts_json: String = row.try_get("counts_json")?;
                    let counts = serde_json::from_str(&counts_json).map_err(|err| {
                        sqlx::Error::Protocol(format!(
                            "invalid dashboard integrity work item: {err}"
                        ))
                    })?;
                    Ok::<DashboardRollupIntegrityWorkItem, sqlx::Error>(
                        DashboardRollupIntegrityWorkItem {
                            range_start: row.try_get("range_start")?,
                            range_end: row.try_get("range_end")?,
                            source_fence_id: row.try_get("source_fence")?,
                            source_version: row.try_get("source_version")?,
                            durable_source_version: row.try_get("durable_source_version")?,
                            cursor_created_at: row.try_get("cursor_created_at")?,
                            cursor_id: row.try_get("cursor_id")?,
                            counts,
                        },
                    )
                })
                .transpose()
                .map_err(Into::into);
        }
        let row = sqlx::query(
            r#"
            SELECT range_start, range_end, source_fence, source_version, durable_source_version, cursor_created_at, cursor_id, counts_json
            FROM dashboard_rollup_integrity_work_items
            WHERE status = 'pending'
              AND (
                  recovery = 0 OR NOT EXISTS (
                      SELECT 1 FROM dashboard_rollup_integrity_state
                      WHERE id = 1
                        AND (hot_cursor < hot_fence OR hot_fence < ?)
                  )
              )
              AND NOT EXISTS (
                  SELECT 1 FROM dashboard_rollup_integrity_blocked_ranges blocked
                  WHERE blocked.day_start <= range_start
                    AND blocked.day_end > range_start
                    AND blocked.reason IN ('source_baseline', 'deleted_source_baseline_unavailable')
                    AND blocked.next_probe_at > ?
                    AND (? IS NULL OR blocked.day_start != ?)
              )
            ORDER BY CASE
                WHEN range_start >= ? AND recovery = 0 AND priority IN (0, 3, 4) THEN 4
                WHEN EXISTS (SELECT 1 FROM dashboard_rollup_integrity_day_reaudits d
                    WHERE d.gc_blocking = 1 AND d.status = 'pending'
                      AND range_start >= d.bucket_start AND range_end <= d.bucket_end) THEN 3
                WHEN recovery = 0 AND priority = ? THEN 2
                ELSE priority END DESC, updated_at ASC, range_start ASC
            LIMIT 1
            "#,
        )
        .bind(now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS))
        .bind(now)
        .bind(claimed_probe_day_start)
        .bind(claimed_probe_day_start)
        .bind(
            now - now.rem_euclid(DASHBOARD_ROLLUP_INTEGRITY_WORK_SECS)
                - DASHBOARD_ROLLUP_INTEGRITY_HOT_WINDOW_SECS,
        )
        .bind(DASHBOARD_ROLLUP_INTEGRITY_CONTINUED_HOT_PRIORITY)
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
                durable_source_version: row.try_get("durable_source_version")?,
                cursor_created_at: row.try_get("cursor_created_at")?,
                cursor_id: row.try_get("cursor_id")?,
                counts,
            })
        })
        .transpose()
        .map_err(Into::into)
    }

    async fn dashboard_rollup_integrity_blocked_status(
        &self,
        conn: &mut sqlite_runtime::SqliteOperationConnection,
    ) -> Result<(i64, Option<(String, i64)>), ProxyError> {
        if self.pg_pool.is_some() {
            let count = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.dashboard_rollup_integrity_blocked_ranges",
            )
            .fetch_one(self.pg()?)
            .await?;
            let range = sqlx::query_as(
                r#"SELECT reason, next_probe_at
                   FROM observability.dashboard_rollup_integrity_blocked_ranges
                   ORDER BY next_probe_at ASC, day_start ASC
                   LIMIT 1"#,
            )
            .fetch_optional(self.pg()?)
            .await?;
            return Ok((count, range));
        }
        let count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_rollup_integrity_blocked_ranges",
        )
        .fetch_one(&mut **conn)
        .await?;
        let range = sqlx::query_as(
            r#"SELECT reason, next_probe_at
               FROM dashboard_rollup_integrity_blocked_ranges
               ORDER BY next_probe_at ASC, day_start ASC
               LIMIT 1"#,
        )
        .fetch_optional(&mut **conn)
        .await?;
        Ok((count, range))
    }

    async fn claim_dashboard_rollup_integrity_probe(
        &self,
        now: i64,
    ) -> Result<Option<(i64, i64, i64)>, ProxyError> {
        if self.pg_pool.is_some() {
            let mut tx = self.pg()?.begin().await?;
            let result = async {
                // day_start is the primary-key tie-break, and SKIP LOCKED lets
                // concurrent workers claim distinct due ranges safely.
                let row: Option<(i64, i64, i64)> = sqlx::query_as(
                    r#"SELECT day_start, day_end, probe_generation
                       FROM observability.dashboard_rollup_integrity_blocked_ranges
                       WHERE next_probe_at <= $1
                       ORDER BY next_probe_at ASC, day_start ASC
                       LIMIT 1
                       FOR UPDATE SKIP LOCKED"#,
                )
                .bind(now)
                .fetch_optional(&mut *tx)
                .await?;
                let Some((day_start, day_end, probe_generation)) = row else {
                    return Ok::<_, ProxyError>(None);
                };
                let result = sqlx::query(
                    r#"UPDATE observability.dashboard_rollup_integrity_blocked_ranges
                       SET next_probe_at = $1,
                           probe_generation = probe_generation + 1,
                           updated_at = $2
                       WHERE day_start = $3 AND next_probe_at <= $4"#,
                )
                .bind(now.saturating_add(DASHBOARD_ROLLUP_INTEGRITY_BLOCKED_PROBE_SECS))
                .bind(now)
                .bind(day_start)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                Ok((result.rows_affected() == 1).then_some((
                    day_start,
                    day_end,
                    probe_generation + 1,
                )))
            }
            .await;
            match result {
                Ok(claimed) => {
                    tx.commit().await?;
                    return Ok(claimed);
                }
                Err(error) => {
                    tx.rollback().await?;
                    return Err(error);
                }
            }
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let mut claimed = None;
        let result = async {
            let row: Option<(i64, i64, i64)> = sqlx::query_as(
                r#"SELECT day_start, day_end, probe_generation
                   FROM dashboard_rollup_integrity_blocked_ranges
                   WHERE next_probe_at <= ?
                   ORDER BY next_probe_at ASC, day_start ASC
                   LIMIT 1"#,
            )
            .bind(now)
            .fetch_optional(&mut *conn)
            .await?;
            if let Some((day_start, day_end, probe_generation)) = row {
                let result = sqlx::query(
                    r#"UPDATE dashboard_rollup_integrity_blocked_ranges
                       SET next_probe_at = ?,
                           probe_generation = probe_generation + 1,
                           updated_at = ?
                       WHERE day_start = ? AND next_probe_at <= ?"#,
                )
                .bind(now.saturating_add(DASHBOARD_ROLLUP_INTEGRITY_BLOCKED_PROBE_SECS))
                .bind(now)
                .bind(day_start)
                .bind(now)
                .execute(&mut *conn)
                .await?;
                if result.rows_affected() == 1 {
                    claimed = Some((day_start, day_end, probe_generation + 1));
                }
            }
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, result)
            .await?;
        Ok(claimed)
    }

    async fn dashboard_rollup_integrity_next_probe_delay(
        &self,
        now: i64,
    ) -> Result<Option<i64>, ProxyError> {
        if self.pg_pool.is_some() {
            return sqlx::query_scalar(
                "SELECT MIN(next_probe_at) FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE reason IN ('source_baseline', 'deleted_source_baseline_unavailable') AND next_probe_at > $1",
            )
            .bind(now)
            .fetch_one(self.pg()?)
            .await
            .map_err(Into::into);
        }
        Ok(sqlx::query_scalar(
            "SELECT MIN(next_probe_at) FROM dashboard_rollup_integrity_blocked_ranges WHERE reason IN ('source_baseline', 'deleted_source_baseline_unavailable') AND next_probe_at > ?",
        )
        .bind(now)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn dashboard_rollup_integrity_blocked_range_probe_delay(
        &self,
        day_start: i64,
        now: i64,
        claimed_probe_day_start: Option<i64>,
    ) -> Result<Option<i64>, ProxyError> {
        if claimed_probe_day_start == Some(day_start) {
            return Ok(None);
        }
        if self.pg_pool.is_some() {
            let blocked: Option<(String, i64)> = sqlx::query_as(
                "SELECT reason, next_probe_at FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE day_start = $1",
            )
            .bind(day_start)
            .fetch_optional(self.pg()?)
            .await?;
            let Some((reason, next_probe_at)) = blocked else {
                return Ok(None);
            };
            if !matches!(
                reason.as_str(),
                "source_baseline" | "deleted_source_baseline_unavailable"
            ) {
                return Ok(None);
            }
            return Ok((next_probe_at > now).then(|| next_probe_at.saturating_sub(now).max(1)));
        }
        let blocked: Option<(String, i64)> = sqlx::query_as(
            "SELECT reason, next_probe_at FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
        )
        .bind(day_start)
        .fetch_optional(&self.pool)
        .await?;
        let Some((reason, next_probe_at)) = blocked else {
            return Ok(None);
        };
        if !matches!(
            reason.as_str(),
            "source_baseline" | "deleted_source_baseline_unavailable"
        ) {
            return Ok(None);
        }
        Ok((next_probe_at > now).then(|| next_probe_at.saturating_sub(now).max(1)))
    }

    async fn promote_dashboard_rollup_integrity_probe_on_source_change(
        &self,
        day_start: i64,
        now: i64,
    ) -> Result<bool, ProxyError> {
        if self.pg_pool.is_some() {
            let Some((reason, next_probe_at, stored_source_fence, stored_source_revision)) =
                sqlx::query_as::<_, (String, i64, Option<i64>, i64)>(
                    "SELECT reason, next_probe_at, source_fence, source_revision FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE day_start = $1",
                )
                .bind(day_start)
                .fetch_optional(self.pg()?)
                .await?
            else {
                return Ok(false);
            };
            if next_probe_at <= now
                || !matches!(
                    reason.as_str(),
                    "source_baseline" | "deleted_source_baseline_unavailable"
                )
            {
                return Ok(false);
            }
            let day_end = next_local_day_start_utc_ts(day_start);
            let current_source_fence: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(id) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(self.pg()?)
            .await?;
            let current_source_revision: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_one(self.pg()?)
            .await?;
            if current_source_fence == stored_source_fence
                && current_source_revision == stored_source_revision
            {
                return Ok(false);
            }
            let result = sqlx::query(
                r#"UPDATE observability.dashboard_rollup_integrity_blocked_ranges
                   SET next_probe_at = $1, source_fence = $2, source_revision = $3, updated_at = $4
                   WHERE day_start = $5
                     AND next_probe_at = $6
                     AND source_fence IS NOT DISTINCT FROM $7
                     AND source_revision = $8"#,
            )
            .bind(now)
            .bind(current_source_fence)
            .bind(current_source_revision)
            .bind(now)
            .bind(day_start)
            .bind(next_probe_at)
            .bind(stored_source_fence)
            .bind(stored_source_revision)
            .execute(self.pg()?)
            .await?;
            return Ok(result.rows_affected() == 1);
        }
        let Some((reason, next_probe_at, stored_source_fence, stored_source_revision)) =
            sqlx::query_as::<_, (String, i64, Option<i64>, i64)>(
                "SELECT reason, next_probe_at, source_fence, source_revision FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
            )
            .bind(day_start)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(false);
        };
        if next_probe_at <= now
            || !matches!(
                reason.as_str(),
                "source_baseline" | "deleted_source_baseline_unavailable"
            )
        {
            return Ok(false);
        }
        let day_end = next_local_day_start_utc_ts(day_start);
        let current_source_fence: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(id) FROM request_logs WHERE visibility = ? AND created_at >= ? AND created_at < ?",
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(day_start)
        .bind(day_end)
        .fetch_one(&self.pool)
        .await?;
        let current_source_revision: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(revision), 0) FROM dashboard_rollup_source_revisions WHERE bucket_start >= ? AND bucket_start < ?",
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(&self.pool)
        .await?;
        if current_source_fence == stored_source_fence
            && current_source_revision == stored_source_revision
        {
            return Ok(false);
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let (write_result, promoted) = match sqlx::query(
            r#"UPDATE dashboard_rollup_integrity_blocked_ranges
               SET next_probe_at = ?, source_fence = ?, source_revision = ?, updated_at = ?
               WHERE day_start = ?
                 AND next_probe_at = ?
                 AND source_fence IS ?
                 AND source_revision = ?"#,
        )
        .bind(now)
        .bind(current_source_fence)
        .bind(current_source_revision)
        .bind(now)
        .bind(day_start)
        .bind(next_probe_at)
        .bind(stored_source_fence)
        .bind(stored_source_revision)
        .execute(&mut *conn)
        .await
        {
            Ok(result) => (Ok(()), result.rows_affected() == 1),
            Err(error) => (Err(error.into()), false),
        };
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, write_result)
            .await?;
        Ok(promoted)
    }

    async fn persist_dashboard_rollup_integrity_blocked_range(
        &self,
        day_start: i64,
        reason: &'static str,
        queue_probe: bool,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let day_end = next_local_day_start_utc_ts(day_start);
        if self.pg_pool.is_some() {
            let mut tx = self.pg()?.begin().await?;
            let result = async {
                let current: Option<(String, i64)> = sqlx::query_as(
                    "SELECT reason, next_probe_at FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE day_start = $1 FOR UPDATE",
                )
                .bind(day_start)
                .fetch_optional(&mut *tx)
                .await?;
                let probe_due = current
                    .as_ref()
                    .map(|(_, next_probe_at)| *next_probe_at <= now)
                    .unwrap_or(true);
                let current_is_baseline = current.as_ref().is_some_and(|(current_reason, _)| {
                    matches!(
                        current_reason.as_str(),
                        "source_baseline" | "deleted_source_baseline_unavailable"
                    )
                });
                let reason_is_baseline = matches!(
                    reason,
                    "source_baseline" | "deleted_source_baseline_unavailable"
                );
                let reset_probe = if reason_is_baseline {
                    !current_is_baseline || probe_due
                } else {
                    probe_due
                };
                let enqueue_probe = queue_probe && reset_probe;
                let source_fence: Option<i64> = sqlx::query_scalar(
                    "SELECT MAX(id) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
                )
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .bind(day_start)
                .bind(day_end)
                .fetch_one(&mut *tx)
                .await?;
                let source_revision: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
                )
                .bind(day_start)
                .bind(day_end)
                .fetch_one(&mut *tx)
                .await?;
                let next_probe_at = if reset_probe {
                    now.saturating_add(DASHBOARD_ROLLUP_INTEGRITY_BLOCKED_PROBE_SECS)
                } else {
                    current
                        .as_ref()
                        .map(|(_, next_probe_at)| *next_probe_at)
                        .expect("a non-due blocked range has a next probe")
                };
                sqlx::query(
                    r#"INSERT INTO observability.dashboard_rollup_integrity_blocked_ranges AS blocked_range (
                           day_start, day_end, reason, next_probe_at, source_fence,
                           source_revision, probe_generation, updated_at
                       ) VALUES ($1, $2, $3, $4, $5, $6, 0, $7)
                       ON CONFLICT (day_start) DO UPDATE SET
                           day_end = EXCLUDED.day_end,
                           reason = EXCLUDED.reason,
                           next_probe_at = CASE
                               WHEN blocked_range.next_probe_at <= $8
                                    OR blocked_range.reason NOT IN (
                                        'source_baseline', 'deleted_source_baseline_unavailable'
                                    )
                               THEN EXCLUDED.next_probe_at
                               ELSE blocked_range.next_probe_at
                           END,
                           source_fence = EXCLUDED.source_fence,
                           source_revision = EXCLUDED.source_revision,
                           updated_at = EXCLUDED.updated_at"#,
                )
                .bind(day_start)
                .bind(day_end)
                .bind(reason)
                .bind(next_probe_at)
                .bind(source_fence)
                .bind(source_revision)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                Ok::<_, ProxyError>(enqueue_probe)
            }
            .await;
            let enqueue_probe = match result {
                Ok(enqueue_probe) => {
                    tx.commit().await?;
                    enqueue_probe
                }
                Err(error) => {
                    tx.rollback().await?;
                    return Err(error);
                }
            };
            if enqueue_probe {
                self.enqueue_dashboard_rollup_integrity_day_reaudit(day_start, now, true)
                    .await?;
                tracing::info!(
                    component = "dashboard_rollup_integrity",
                    event = "blocked_range_probe_queued",
                    day_start,
                    reason,
                    "queued the first or due probe for a blocked range"
                );
            } else {
                tracing::debug!(
                    component = "dashboard_rollup_integrity",
                    event = "blocked_range_probe_deferred",
                    day_start,
                    reason,
                    "retained the blocked range until its next probe time"
                );
            }
            return Ok(());
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let mut enqueue_probe = false;
        let result = async {
            let current: Option<(String, i64)> = sqlx::query_as(
                "SELECT reason, next_probe_at FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
            )
            .bind(day_start)
            .fetch_optional(&mut *conn)
            .await?;
            let probe_due = current
                .as_ref()
                .map(|(_, next_probe_at)| *next_probe_at <= now)
                .unwrap_or(true);
            let current_is_baseline = current.as_ref().is_some_and(|(current_reason, _)| {
                matches!(
                    current_reason.as_str(),
                    "source_baseline" | "deleted_source_baseline_unavailable"
                )
            });
            let reason_is_baseline = matches!(
                reason,
                "source_baseline" | "deleted_source_baseline_unavailable"
            );
            let reset_probe = if reason_is_baseline {
                !current_is_baseline || probe_due
            } else {
                probe_due
            };
            enqueue_probe = queue_probe && reset_probe;
            let source_fence: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(id) FROM request_logs WHERE visibility = ? AND created_at >= ? AND created_at < ?",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let source_revision: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(revision), 0) FROM dashboard_rollup_source_revisions WHERE bucket_start >= ? AND bucket_start < ?",
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *conn)
            .await?;
            let next_probe_at = if reset_probe {
                now.saturating_add(DASHBOARD_ROLLUP_INTEGRITY_BLOCKED_PROBE_SECS)
            } else {
                current
                    .as_ref()
                    .map(|(_, next_probe_at)| *next_probe_at)
                    .expect("a non-due blocked range has a next probe")
            };
            sqlx::query(
                r#"INSERT INTO dashboard_rollup_integrity_blocked_ranges (
                       day_start, day_end, reason, next_probe_at, source_fence,
                       source_revision, probe_generation, updated_at
                   ) VALUES (?, ?, ?, ?, ?, ?, 0, ?)
                   ON CONFLICT(day_start) DO UPDATE SET
                       day_end = excluded.day_end,
                       reason = excluded.reason,
                       next_probe_at = CASE
                           WHEN dashboard_rollup_integrity_blocked_ranges.next_probe_at <= ?
                                OR dashboard_rollup_integrity_blocked_ranges.reason NOT IN (
                                    'source_baseline', 'deleted_source_baseline_unavailable'
                                )
                           THEN excluded.next_probe_at
                           ELSE dashboard_rollup_integrity_blocked_ranges.next_probe_at
                       END,
                       source_fence = excluded.source_fence,
                       source_revision = excluded.source_revision,
                       updated_at = excluded.updated_at"#,
            )
            .bind(day_start)
            .bind(day_end)
            .bind(reason)
            .bind(next_probe_at)
            .bind(source_fence)
            .bind(source_revision)
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            Ok::<_, ProxyError>(())
        }
        .await;
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, result)
            .await?;
        if enqueue_probe {
            self.enqueue_dashboard_rollup_integrity_day_reaudit(day_start, now, true)
                .await?;
            tracing::info!(
                component = "dashboard_rollup_integrity",
                event = "blocked_range_probe_queued",
                day_start,
                reason,
                "queued the first or due probe for a blocked range"
            );
        } else {
            tracing::debug!(
                component = "dashboard_rollup_integrity",
                event = "blocked_range_probe_deferred",
                day_start,
                reason,
                "retained the blocked range until its next probe time"
            );
        }
        Ok(())
    }

    async fn block_request_log_gc_for_day(
        &self,
        day_start: i64,
        reason: &'static str,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        self.persist_dashboard_rollup_integrity_blocked_range(day_start, reason, true)
            .await?;
        Ok(DashboardRollupRequestLogGcDecision::Blocked { day_start, reason })
    }

    async fn clear_dashboard_rollup_integrity_blocked_range(
        &self,
        day_start: i64,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            sqlx::query(
                "DELETE FROM observability.dashboard_rollup_integrity_blocked_ranges WHERE day_start = $1",
            )
            .bind(day_start)
            .execute(self.pg()?)
            .await?;
            return Ok(());
        }
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?)",
        )
        .bind(day_start)
        .fetch_one(&self.pool)
        .await?;
        if exists == 0 {
            return Ok(());
        }
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let result = sqlx::query(
            "DELETE FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
        )
        .bind(day_start)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(Into::into);
        self.finish_dashboard_rollup_integrity_short_write(&mut conn, result)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_cutoff(
        &self,
        threshold: i64,
    ) -> Result<Option<i64>, ProxyError> {
        Ok(
            match self
                .dashboard_rollup_integrity_request_log_gc_decision(threshold)
                .await?
            {
                DashboardRollupRequestLogGcDecision::Allowed { cutoff, .. } => Some(cutoff),
                DashboardRollupRequestLogGcDecision::Blocked { .. } => None,
            },
        )
    }

    #[cfg(test)]
    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_decision(
        &self,
        threshold: i64,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        if self.pg_pool.is_some() {
            return self
                .dashboard_rollup_integrity_request_log_gc_decision_for_day_pg(
                    threshold, None, None,
                )
                .await;
        }
        self.dashboard_rollup_integrity_request_log_gc_decision_for_day_inner(
            threshold, None, None,
        )
        .await
    }

    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_decision_before_deadline(
        &self,
        threshold: i64,
        deadline: tokio::time::Instant,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        if self.pg_pool.is_some() {
            return self
                .dashboard_rollup_integrity_request_log_gc_decision_for_day_pg(
                    threshold,
                    None,
                    Some(deadline),
                )
                .await;
        }
        self.dashboard_rollup_integrity_request_log_gc_decision_for_day_inner(
            threshold,
            None,
            Some(deadline),
        )
        .await
    }

    pub(crate) async fn dashboard_rollup_integrity_request_log_gc_decision_for_day_before_deadline(
        &self,
        threshold: i64,
        day_start: i64,
        day_end: i64,
        deadline: tokio::time::Instant,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        if self.pg_pool.is_some() {
            return self
                .dashboard_rollup_integrity_request_log_gc_decision_for_day_pg(
                    threshold,
                    Some((day_start, day_end)),
                    Some(deadline),
                )
                .await;
        }
        self.dashboard_rollup_integrity_request_log_gc_decision_for_day_inner(
            threshold,
            Some((day_start, day_end)),
            Some(deadline),
        )
        .await
    }

    async fn begin_dashboard_integrity_read_before_deadline(
        &self,
        deadline: tokio::time::Instant,
        read_budget: Duration,
    ) -> Result<SqliteReadSnapshot, ProxyError> {
        self.sqlite_runtime
            .begin_read_snapshot_before_deadline(
                SqliteOperation::DashboardIntegrityWrite,
                deadline.into_std(),
                read_budget,
            )
            .await
    }

    async fn dashboard_rollup_integrity_request_log_gc_decision_for_day_pg(
        &self,
        threshold: i64,
        target_day: Option<(i64, i64)>,
        outer_deadline: Option<tokio::time::Instant>,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        let now = self.backend_time.now_ts();
        let read = self.dashboard_rollup_integrity_request_log_gc_decision_for_day_pg_read(
            threshold,
            target_day,
            now,
            outer_deadline,
        );
        let decision = if let Some(deadline) = outer_deadline {
            tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                read,
            )
            .await
            .map_err(|_| Self::dashboard_integrity_read_deferred())??
        } else {
            read.await?
        };
        match decision {
            DashboardRollupRequestLogGcDecision::Blocked {
                day_start,
                reason:
                    reason @ ("reaudit_pending"
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

    async fn dashboard_rollup_integrity_request_log_gc_decision_for_day_pg_read(
        &self,
        threshold: i64,
        target_day: Option<(i64, i64)>,
        now: i64,
        outer_deadline: Option<tokio::time::Instant>,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        let day = if let Some((day_start, day_end)) = target_day {
            Some((day_start, day_end))
        } else {
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MIN(created_at) FROM observability.request_logs WHERE visibility = $1 AND created_at < $2",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(threshold)
            .fetch_one(self.pg()?)
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
        .fetch_optional(self.pg()?)
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
        let reaudit_pending: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.dashboard_rollup_integrity_day_reaudits WHERE bucket_start = $1 AND status = 'pending')::bigint",
        )
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        if reaudit_pending != 0 {
            return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                day_start,
                reason: "reaudit_pending",
            });
        }
        let sealed: Option<(String, Option<i64>, i64, i64)> = sqlx::query_as(
            "SELECT counts_json, source_fence, source_version, durable_source_version FROM observability.dashboard_rollup_daily_seals WHERE bucket_start = $1",
        )
        .bind(day_start)
        .fetch_optional(self.pg()?)
        .await?;
        let Some((counts_json, sealed_fence, sealed_version, sealed_durable_source_version)) =
            sealed
        else {
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
        .fetch_one(self.pg()?)
        .await?;
        let current_durable_source_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(revision), 0)::bigint FROM observability.dashboard_rollup_source_revisions WHERE bucket_start >= $1 AND bucket_start < $2",
        )
        .bind(day_start)
        .bind(day_end)
        .fetch_one(self.pg()?)
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
        // source revision. A fence that moves forward still indicates a late
        // insert; the deleted-source baseline covers untracked deletions.
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
                return Ok(DashboardRollupRequestLogGcDecision::Blocked { day_start, reason });
            }
            Err(error) if matches!(&error, ProxyError::Database(sqlx::Error::PoolTimedOut)) => {
                return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                    day_start,
                    reason: "read_budget",
                });
            }
            Err(error) => return Err(error),
        };
        if retained_source_rows.saturating_add(deleted_source_rows) < expected.total_requests {
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
        .fetch_all(self.pg()?)
        .await?;
        let mut minute_actual = DashboardRequestRollupCounts::default();
        for row in minute_rows {
            minute_actual.add(DashboardRequestRollupCounts {
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
            });
        }
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
        .fetch_optional(self.pg()?)
        .await?;
        let daily_actual = daily_row
            .map(|row| {
                Ok::<_, sqlx::Error>(DashboardRequestRollupCounts {
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
            })
            .transpose()?
            .unwrap_or_default();
        if minute_actual != expected || daily_actual != expected {
            return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                day_start,
                reason: "rollup_mismatch",
            });
        }
        let source_recovery_pending: i64 = sqlx::query_scalar(
            r#"SELECT (
                   EXISTS(SELECT 1 FROM observability.dashboard_rollup_integrity_work_items
                          WHERE status = 'pending' AND range_start < $1 AND range_end > $2)
                   OR EXISTS(SELECT 1 FROM observability.dashboard_rollup_rebalance_recovery
                             WHERE status = 'pending' AND cursor < $1 AND range_end > $2)
               )::bigint"#,
        )
        .bind(day_end)
        .bind(day_start)
        .fetch_one(self.pg()?)
        .await?;
        if source_recovery_pending != 0 {
            return Ok(DashboardRollupRequestLogGcDecision::Blocked {
                day_start,
                reason: "source_recovery_pending",
            });
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

    async fn dashboard_rollup_gc_deleted_source_row_counts(
        &self,
        day_start: i64,
        day_end: i64,
        read_budget: Duration,
        outer_deadline: Option<tokio::time::Instant>,
    ) -> Result<(i64, i64), ProxyError> {
        if self.pg_pool.is_some() {
            if read_budget.is_zero() {
                return Err(Self::dashboard_integrity_read_deferred());
            }
            let result = tokio::time::timeout(read_budget, async {
                let mut tx = self.pg()?.begin().await?;
                sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
                    .execute(&mut *tx)
                    .await?;
                let counts = async {
                    let retained_source_rows: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM observability.request_logs WHERE visibility = $1 AND created_at >= $2 AND created_at < $3",
                    )
                    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                    .bind(day_start)
                    .bind(day_end)
                    .fetch_one(&mut *tx)
                    .await?;
                    let deleted_source_rows: i64 = sqlx::query_scalar(
                        "SELECT COALESCE(SUM((counts_json::jsonb ->> 'total_requests')::bigint), 0)::bigint FROM observability.dashboard_rollup_gc_deleted_source_contributions WHERE bucket_start >= $1 AND bucket_start < $2",
                    )
                    .bind(day_start)
                    .bind(day_end)
                    .fetch_one(&mut *tx)
                    .await?;
                    Ok::<_, ProxyError>((retained_source_rows, deleted_source_rows))
                }
                .await;
                match counts {
                    Ok(counts) => {
                        tx.commit().await?;
                        Ok(counts)
                    }
                    Err(error) => {
                        tx.rollback().await?;
                        Err(error)
                    }
                }
            })
            .await;
            return result.map_err(|_| Self::dashboard_integrity_read_deferred())?;
        }
        let mut snapshot = match outer_deadline {
            Some(deadline) => self
                .begin_dashboard_integrity_read_before_deadline(deadline, read_budget)
                .await?,
            None => self.begin_dashboard_integrity_read(read_budget).await?,
        };
        let result: Result<(i64, i64), sqlx::Error> = async {
            let retained_source_rows = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM request_logs WHERE visibility = ? AND created_at >= ? AND created_at < ?",
            )
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *snapshot)
            .await?;
            let deleted_source_rows = sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(SUM(CAST(json_extract(counts_json, '$.total_requests') AS INTEGER)), 0) FROM dashboard_rollup_gc_deleted_source_contributions WHERE bucket_start >= ? AND bucket_start < ?",
            )
            .bind(day_start)
            .bind(day_end)
            .fetch_one(&mut *snapshot)
            .await?;
            Ok((retained_source_rows, deleted_source_rows))
        }
        .await;
        match snapshot.complete_native_read(result).await? {
            crate::store::sqlite_runtime::SqliteCooperativeQueryOutcome::Completed(counts) => {
                Ok(counts)
            }
            crate::store::sqlite_runtime::SqliteCooperativeQueryOutcome::DeadlineExceeded => {
                Err(Self::dashboard_integrity_read_deferred())
            }
        }
    }
}
