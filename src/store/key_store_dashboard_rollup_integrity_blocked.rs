impl KeyStore {
    pub(crate) async fn run_dashboard_rollup_integrity_slice(
        &self,
    ) -> Result<DashboardRollupIntegritySlice, ProxyError> {
        let now = self.backend_time.now_ts();
        self.ensure_dashboard_rollup_integrity_state(now).await?;
        self.ensure_dashboard_rollup_rebalance_recovery(now).await?;
        if let Some((day_start, day_end, probe_generation)) =
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
        }
        if self.dashboard_rollup_integrity_hot_work_due(now).await?
            || self.dashboard_rollup_integrity_gc_work_due().await?
        {
            self.create_next_dashboard_rollup_integrity_work_item(now)
                .await?;
        }
        if let Some(item) = self.load_dashboard_rollup_integrity_work_item(now).await? {
            return self
                .process_dashboard_rollup_integrity_work_item(item, now, None, false)
                .await;
        }
        if self.dashboard_rollup_integrity_seal_verification_due(now).await? {
            self.verify_next_dashboard_rollup_daily_seal(now).await?;
            self.mark_dashboard_rollup_integrity_seal_attempt(now).await?;
            if let Some(item) = self.load_dashboard_rollup_integrity_work_item(now).await? {
                return self
                    .process_dashboard_rollup_integrity_work_item(item, now, None, false)
                    .await;
            }
        }
        if let Some(item) = self
            .create_next_dashboard_rollup_integrity_work_item(now)
            .await?
        {
            return self
                .process_dashboard_rollup_integrity_work_item(item, now, None, false)
                .await;
        }
        if let Some(item) = self
            .create_next_dashboard_rollup_rebalance_recovery_work_item(now)
            .await?
        {
            return self
                .process_dashboard_rollup_integrity_work_item(item, now, None, false)
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

    async fn dashboard_rollup_integrity_blocked_status(
        &self,
        conn: &mut sqlite_runtime::SqliteOperationConnection,
    ) -> Result<(i64, Option<(String, i64)>), ProxyError> {
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
        Ok(sqlx::query_scalar(
            "SELECT MIN(next_probe_at) FROM dashboard_rollup_integrity_blocked_ranges WHERE next_probe_at > ?",
        )
        .bind(now)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn block_request_log_gc_for_day(
        &self,
        day_start: i64,
        reason: &'static str,
    ) -> Result<DashboardRollupRequestLogGcDecision, ProxyError> {
        let now = self.backend_time.now_ts();
        let day_end = next_local_day_start_utc_ts(day_start);
        let mut conn = self.begin_dashboard_rollup_integrity_short_write().await?;
        let mut enqueue_probe = false;
        let result = async {
            let current: Option<i64> = sqlx::query_scalar(
                "SELECT next_probe_at FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
            )
            .bind(day_start)
            .fetch_optional(&mut *conn)
            .await?;
            enqueue_probe = current.map(|next_probe_at| next_probe_at <= now).unwrap_or(true);
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
            let next_probe_at = current
                .filter(|_| !enqueue_probe)
                .unwrap_or_else(|| now.saturating_add(DASHBOARD_ROLLUP_INTEGRITY_BLOCKED_PROBE_SECS));
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
        Ok(DashboardRollupRequestLogGcDecision::Blocked { day_start, reason })
    }

    async fn clear_dashboard_rollup_integrity_blocked_range(
        &self,
        day_start: i64,
    ) -> Result<(), ProxyError> {
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
}
