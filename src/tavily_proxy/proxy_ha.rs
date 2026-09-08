impl TavilyProxy {
    fn spawn_request_stats_coalescer(&self) {
        // The worker must not extend the lifetime of a short-lived proxy. In
        // particular, test proxies and restarted runtimes must release their
        // SQLite pool after their final external owner is gone.
        let store = Arc::downgrade(&self.key_store);
        let owner = Arc::downgrade(&self.background_task_owner);
        let coalescer = self.key_store.request_stats_coalescer.clone();
        tokio::spawn(async move {
            {
                let mut state = coalescer.state.lock().await;
                state.worker_stopped = false;
            }
            loop {
                if owner.upgrade().is_none() {
                    let mut state = coalescer.state.lock().await;
                    state.worker_stopped = true;
                    coalescer.flushed.notify_waiters();
                    break;
                }
                let Some(store) = store.upgrade() else {
                    let mut state = coalescer.state.lock().await;
                    state.worker_stopped = true;
                    coalescer.flushed.notify_waiters();
                    break;
                };
                let (should_flush_now, wait_duration) = {
                    let state = coalescer.state.lock().await;
                    let should_flush_now = (state.shutdown && state.dashboard_rollup_repairs.is_empty())
                        || state
                            .flush_deadline
                            .map(|deadline| Instant::now() >= deadline)
                            .unwrap_or(false);
                    let wait_duration = state
                        .flush_deadline
                        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                        .unwrap_or(RequestStatsCoalescer::FLUSH_INTERVAL);
                    (should_flush_now, wait_duration)
                };
                if !should_flush_now {
                    tokio::select! {
                        _ = coalescer.wake.notified() => {}
                        _ = tokio::time::sleep(wait_duration) => {}
                    }
                    drop(store);
                    continue;
                }

                let shutdown_after_flush = {
                    let state = coalescer.state.lock().await;
                    if state.pending_dashboard_rollups.is_empty()
                        && state.pending_api_key_usage.is_empty()
                        && state.pending_auth_token_activity.is_empty()
                        && state.pending_account_request_rollups.is_empty()
                        && state.pending_request_log_catalog.is_empty()
                        && !state.shutdown
                    {
                        drop(store);
                        continue;
                    }
                    state.shutdown && state.dashboard_rollup_repairs.is_empty()
                };

                let flush_started = Instant::now();
                match store.flush_request_stats_writes_in_background().await {
                    Ok(crate::store::RequestStatsBackgroundFlushOutcome::Flushed) => {
                        log_slow_db_operation(
                            "request stats persist",
                            flush_started.elapsed(),
                            Some("component=request-stats-coalescer"),
                        );
                    }
                    Ok(crate::store::RequestStatsBackgroundFlushOutcome::Deferred(reason)) => {
                        tracing::debug!(
                            component = "request_stats",
                            event = "persist_deferred",
                            defer_reason = reason.as_str(),
                            elapsed_ms = flush_started.elapsed().as_millis() as u64,
                            "request stats flush deferred before SQLite connection acquisition"
                        );
                        tokio::time::sleep(RequestStatsCoalescer::FLUSH_INTERVAL).await;
                    }
                    Err(err) => {
                        if !crate::store::is_transient_sqlite_write_error(&err) {
                            log_db_operation_error(
                                "request stats persist",
                                flush_started.elapsed(),
                                Some("component=request-stats-coalescer"),
                                &err,
                            );
                        }
                        tracing::debug!(
                            component = "request_stats",
                            event = "persist_retry",
                            elapsed_ms = flush_started.elapsed().as_millis() as u64,
                            err = %err,
                            "request stats persist deferred after structured database error"
                        );
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }

                {
                    let mut state = coalescer.state.lock().await;
                    if shutdown_after_flush
                        && state.pending_dashboard_rollups.is_empty()
                        && state.pending_api_key_usage.is_empty()
                        && state.pending_auth_token_activity.is_empty()
                        && state.pending_account_request_rollups.is_empty()
                        && state.pending_request_log_catalog.is_empty()
                        && state.dashboard_rollup_repairs.is_empty()
                    {
                        state.worker_stopped = true;
                        coalescer.flushed.notify_waiters();
                        break;
                    }
                }
                drop(store);
            }
        });
    }

    pub async fn nudge_request_stats_flush(&self) {
        self.key_store.request_stats_coalescer.nudge_flush().await;
    }

    pub async fn shutdown_request_stats_coalescer(
        &self,
        timeout: Duration,
    ) -> Result<(), ProxyError> {
        let coalescer = self.key_store.request_stats_coalescer.clone();
        coalescer.begin_shutdown().await;
        tokio::time::timeout(timeout, coalescer.wait_until_worker_stopped())
            .await
            .map_err(|_| {
                ProxyError::Other(format!(
                    "request stats coalescer did not drain within {}ms",
                    timeout.as_millis()
                ))
            })
    }

    pub async fn shutdown_sqlite_maintenance_bulk(&self, timeout: Duration) -> bool {
        self.key_store
            .sqlite_runtime
            .shutdown_maintenance_bulk(timeout)
            .await
    }

    pub fn begin_sqlite_maintenance_run_shutdown(&self) {
        self.key_store
            .sqlite_runtime
            .begin_maintenance_run_shutdown();
    }

    pub(crate) fn sqlite_maintenance_runs_shutting_down(&self) -> bool {
        self.key_store
            .sqlite_runtime
            .maintenance_runs_shutting_down()
    }

    pub async fn run_dashboard_rollup_integrity_slice(
        &self,
    ) -> Result<DashboardRollupIntegrityRun, ProxyError> {
        // The integrity slice owns short SQLite writes. Keep the complete
        // slice in a runtime-owned task so a scheduler timeout or caller
        // cancellation cannot drop an in-flight write before its controlled
        // commit/rollback boundary.
        let key_store = Arc::clone(&self.key_store);
        let result = tokio::spawn(async move { key_store.run_dashboard_rollup_integrity_slice().await })
            .await
            .map_err(|error| {
                ProxyError::Other(format!(
                    "dashboard rollup integrity task failed before completion: {error}"
                ))
            })??;
        let (state, next_delay_secs) = match result {
            crate::store::DashboardRollupIntegritySlice::Verified { next_delay_secs } => {
                ("verified", next_delay_secs)
            }
            crate::store::DashboardRollupIntegritySlice::Deferred { next_delay_secs } => {
                ("deferred", next_delay_secs)
            }
            crate::store::DashboardRollupIntegritySlice::Repaired { next_delay_secs } => {
                ("repaired", next_delay_secs)
            }
        };
        Ok(DashboardRollupIntegrityRun {
            state: state.to_string(),
            next_delay_secs,
        })
    }

    pub fn admit_dashboard_rollup_integrity(&self) -> SqliteAdmissionOutcome {
        match self.key_store.try_admit_dashboard_rollup_integrity() {
            Ok(permit) => SqliteAdmissionOutcome::Admitted(SqliteMaintenanceAdmission {
                _kind: SqliteMaintenanceAdmissionKind::Bulk { _permit: permit },
            }),
            Err(reason) => SqliteAdmissionOutcome::Deferred {
                reason: reason.as_str(),
            },
        }
    }

    pub fn dashboard_overview_refresh_defer_reason(&self) -> Option<&'static str> {
        self.key_store
            .dashboard_overview_refresh_defer_reason()
            .map(SqliteAdmissionDeferReason::as_str)
    }

    pub async fn dashboard_rollup_integrity_status(
        &self,
    ) -> Result<DashboardRollupIntegrityStatus, ProxyError> {
        self.key_store.dashboard_rollup_integrity_status().await
    }

    pub async fn mark_dashboard_rollup_integrity_failure(
        &self,
        err: &ProxyError,
        next_attempt_at: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .mark_dashboard_rollup_integrity_failure(err, next_attempt_at)
            .await
    }

    fn spawn_ha_state_coalescer(&self) {
        let store = Arc::downgrade(&self.key_store);
        let owner = Arc::downgrade(&self.background_task_owner);
        let coalescer = self.ha_state_coalescer.clone();
        tokio::spawn(async move {
            loop {
                if owner.upgrade().is_none() {
                    break;
                }
                let Some(store) = store.upgrade() else {
                    break;
                };
                let (should_flush_now, wait_duration) = {
                    let state = coalescer.state.lock().await;
                    let pending_key_count = HaStateCoalescer::pending_key_count(&state);
                    let should_flush_now = state.shutdown
                        || pending_key_count >= HaStateCoalescer::MAX_PENDING_KEYS
                        || state
                            .flush_deadline
                            .map(|deadline| Instant::now() >= deadline)
                            .unwrap_or(false);
                    let wait_duration = state
                        .flush_deadline
                        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                        .unwrap_or(HaStateCoalescer::FLUSH_INTERVAL);
                    (should_flush_now, wait_duration)
                };
                if !should_flush_now {
                    tokio::select! {
                        _ = coalescer.wake.notified() => {}
                        _ = tokio::time::sleep(wait_duration) => {}
                    }
                    drop(store);
                    continue;
                }

                let (pending_node_state, pending_sync_watermarks, shutdown_after_flush) = {
                    let mut state = coalescer.state.lock().await;
                    if state.pending_node_state.is_none()
                        && state.pending_sync_watermarks.is_empty()
                        && !state.shutdown
                    {
                        drop(store);
                        continue;
                    }
                    state.flushing = true;
                    (
                        state.pending_node_state.take(),
                        state.pending_sync_watermarks.drain().collect::<Vec<_>>(),
                        state.shutdown,
                    )
                };

                for pending in pending_sync_watermarks {
                    let (name, watermark) = pending;
                    if let Err(err) = store
                        .persist_ha_sync_watermark(
                            &name,
                            watermark.source_node_id.as_deref(),
                            watermark.target_node_id.as_deref(),
                            watermark.watermark,
                            watermark.detail.as_deref(),
                        )
                        .await
                    {
                        tracing::warn!(
                            component = "ha",
                            event = "sync_watermark_persist_failed",
                            channel = %name,
                            err = %err,
                        );
                    }
                }

                let mut flushed_node_state = None;
                if let Some(pending) = pending_node_state {
                    match store
                        .persist_ha_node_state(
                            &pending.node_id,
                            pending.role,
                            pending.edgeone_origin.as_deref(),
                            pending.source_settings.as_ref(),
                            pending.message.as_deref(),
                        )
                        .await
                    {
                        Ok(()) => {
                            flushed_node_state = Some(pending);
                        }
                        Err(err) => {
                            tracing::warn!(
                                component = "ha",
                                event = "node_state_persist_failed",
                                node_id = %pending.node_id,
                                err = %err,
                            );
                        }
                    }
                }

                {
                    let mut state = coalescer.state.lock().await;
                    if let Some(flushed_node_state) = flushed_node_state {
                        state.last_flushed_node_state = Some(flushed_node_state);
                    }
                    state.flushing = false;
                    state.flush_deadline = None;
                    coalescer.flushed.notify_waiters();
                    if shutdown_after_flush
                        && state.pending_node_state.is_none()
                        && state.pending_sync_watermarks.is_empty()
                    {
                        break;
                    }
                }
                drop(store);
            }
        });
    }

    pub async fn persist_ha_node_state(
        &self,
        node_id: &str,
        role: HaNodeRole,
        edgeone_origin: Option<&str>,
        source_settings: Option<&HaSourceSettingsView>,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.ha_state_coalescer
            .enqueue_node_state(node_id, role, edgeone_origin, source_settings, message)
            .await;
        Ok(())
    }

    pub async fn get_ha_source_settings(&self) -> Result<Option<HaSourceSettings>, ProxyError> {
        self.key_store.get_ha_source_settings().await
    }

    pub async fn get_persisted_ha_node_role(&self) -> Result<Option<HaNodeRole>, ProxyError> {
        self.key_store.get_persisted_ha_node_role().await
    }

    pub async fn persist_ha_sync_watermark(
        &self,
        name: &str,
        source_node_id: Option<&str>,
        target_node_id: Option<&str>,
        watermark: i64,
        detail: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.ha_state_coalescer
            .enqueue_sync_watermark(name, source_node_id, target_node_id, watermark, detail)
            .await;
        Ok(())
    }

    pub async fn get_ha_sync_watermark(&self, name: &str) -> Result<Option<i64>, ProxyError> {
        if let Some(pending) = self.ha_state_coalescer.pending_sync_watermark(name).await {
            return Ok(Some(pending.watermark));
        }
        self.key_store.get_ha_sync_watermark(name).await
    }

    pub async fn ha_channel_high_watermark(
        &self,
        channel: HaSyncChannel,
    ) -> Result<i64, ProxyError> {
        self.key_store.ha_channel_high_watermark(channel).await
    }

    pub async fn ha_channel_outbox_stats(
        &self,
        channel: HaSyncChannel,
        peer_node_id: Option<&str>,
    ) -> Result<HaOutboxStats, ProxyError> {
        self.key_store
            .ha_channel_outbox_stats(channel, peer_node_id)
            .await
    }

    pub async fn gc_ha_outbox_with_options(
        &self,
        options: HaOutboxGcOptions,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        self.key_store.gc_ha_outbox_with_options(options).await
    }

    pub async fn gc_ha_outbox_online(&self) -> Result<HaOutboxGcReport, ProxyError> {
        self.key_store.gc_ha_outbox_online().await
    }

    pub fn admit_ha_outbox_gc(&self) -> SqliteAdmissionOutcome {
        match self.key_store.try_admit_ha_outbox_gc() {
            Ok(permit) => SqliteAdmissionOutcome::Admitted(SqliteMaintenanceAdmission {
                _kind: SqliteMaintenanceAdmissionKind::Bulk { _permit: permit },
            }),
            Err(reason) => SqliteAdmissionOutcome::Deferred {
                reason: reason.as_str(),
            },
        }
    }

    pub fn admit_request_logs_gc(&self) -> SqliteAdmissionOutcome {
        match self.key_store.try_admit_request_logs_gc() {
            Ok(permit) => SqliteAdmissionOutcome::Admitted(SqliteMaintenanceAdmission {
                _kind: SqliteMaintenanceAdmissionKind::Bulk { _permit: permit },
            }),
            Err(reason) => SqliteAdmissionOutcome::Deferred {
                reason: reason.as_str(),
            },
        }
    }

    pub fn request_logs_gc_continue_defer_reason(&self) -> Option<&'static str> {
        self.key_store
            .request_logs_gc_continue_defer_reason()
            .map(|reason| reason.as_str())
    }

    pub fn admit_server_pressure_rebuild(&self) -> SqliteAdmissionOutcome {
        match self.key_store.try_admit_server_pressure_rebuild() {
            Ok(permit) => SqliteAdmissionOutcome::Admitted(SqliteMaintenanceAdmission {
                _kind: SqliteMaintenanceAdmissionKind::Bulk { _permit: permit },
            }),
            Err(reason) => SqliteAdmissionOutcome::Deferred {
                reason: reason.as_str(),
            },
        }
    }

    pub fn admit_upstream_reconciliation_projection(&self) -> SqliteAdmissionOutcome {
        match self.key_store.try_admit_upstream_reconciliation_projection() {
            Ok(permit) => SqliteAdmissionOutcome::Admitted(SqliteMaintenanceAdmission {
                _kind: SqliteMaintenanceAdmissionKind::Bulk { _permit: permit },
            }),
            Err(reason) => SqliteAdmissionOutcome::Deferred {
                reason: reason.as_str(),
            },
        }
    }

    pub async fn prewarm_upstream_reconciliation_projection_capacity(
        &self,
    ) -> Result<(), ProxyError> {
        self.key_store
            .prewarm_upstream_reconciliation_projection_capacity()
            .await
    }

    pub fn record_foreground_activity(&self) {
        self.key_store.record_foreground_activity();
    }

    pub fn foreground_activity_rps(&self) -> i64 {
        self.key_store.foreground_activity_rps()
    }

    pub fn foreground_activity_low_pressure_since_floor(&self) -> i64 {
        self.key_store.foreground_activity_low_pressure_since_floor()
    }

    pub fn subscribe_dashboard_sse(&self) -> impl Drop {
        self.key_store.subscribe_dashboard_sse()
    }

    pub fn dashboard_read_generation(&self) -> Option<u64> {
        self.key_store
            .request_stats_coalescer
            .try_request_stats_version()
    }

    pub async fn gc_ha_outbox_online_with_foreground_rps(
        &self,
        foreground_rps: i64,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        self.key_store
            .gc_ha_outbox_online_with_foreground_rps(foreground_rps)
            .await
    }

    pub async fn gc_ha_outbox_online_with_foreground_pressure(
        &self,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        self.key_store
            .gc_ha_outbox_online_with_foreground_pressure(foreground_rps, low_pressure_since_floor)
            .await
    }

    pub async fn gc_ha_outbox_online_with_foreground_activity<F>(
        &self,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
        foreground_rps_now: F,
    ) -> Result<HaOutboxGcReport, ProxyError>
    where
        F: Fn() -> i64,
    {
        self.key_store
            .gc_ha_outbox_online_with_foreground_activity(
                foreground_rps,
                low_pressure_since_floor,
                foreground_rps_now,
            )
            .await
    }

    pub async fn ha_outbox_gc_watchdog_needed(&self) -> Result<bool, ProxyError> {
        self.key_store.ha_outbox_gc_watchdog_needed().await
    }

    pub async fn ha_peer_channel_health(
        &self,
        channel: HaSyncChannel,
        peer_node_id: &str,
    ) -> Result<HaChannelHealthView, ProxyError> {
        self.key_store
            .ha_peer_channel_health(channel, peer_node_id)
            .await
    }

    pub async fn flush_ha_state_writes(&self) -> Result<(), ProxyError> {
        self.ha_state_coalescer.wake.notify_one();
        self.ha_state_coalescer.wait_until_flushed().await;
        Ok(())
    }

    pub async fn export_ha_baseline_ndjson(
        &self,
        channel: HaSyncChannel,
        node_id: &str,
    ) -> Result<HaBaselineExport, ProxyError> {
        self.key_store.export_ha_baseline_ndjson(channel, node_id).await
    }

    pub async fn write_ha_baseline_ndjson<W>(
        &self,
        channel: HaSyncChannel,
        node_id: &str,
        writer: &mut W,
    ) -> Result<HaApplyResult, ProxyError>
    where
        W: tokio::io::AsyncWrite + Unpin + Send,
    {
        self.key_store
            .write_ha_baseline_ndjson(channel, node_id, writer)
            .await
    }

    pub async fn count_ha_baseline_rows(
        &self,
        channel: HaSyncChannel,
    ) -> Result<usize, ProxyError> {
        self.key_store.count_ha_baseline_rows(channel).await
    }

    pub async fn begin_ha_baseline_read(
        &self,
        channel: HaSyncChannel,
    ) -> Result<crate::store::HaBaselineReadSession, ProxyError> {
        self.key_store.begin_ha_baseline_read(channel).await
    }

    pub async fn begin_ha_events_read(
        &self,
        channel: HaSyncChannel,
    ) -> Result<crate::store::HaEventsReadSession, ProxyError> {
        self.key_store.begin_ha_events_read(channel).await
    }

    pub async fn begin_ha_baseline_apply(
        &self,
        channel: HaSyncChannel,
    ) -> Result<crate::store::HaBaselineApplySession, ProxyError> {
        self.key_store.begin_ha_baseline_apply(channel).await
    }

    pub async fn begin_ha_baseline_apply_with_mode(
        &self,
        channel: HaSyncChannel,
        mode: crate::store::HaBaselineApplyMode,
    ) -> Result<crate::store::HaBaselineApplySession, ProxyError> {
        self.key_store
            .begin_ha_baseline_apply_with_mode(channel, mode)
            .await
    }

    pub async fn begin_ha_events_apply(
        &self,
        channel: HaSyncChannel,
    ) -> Result<crate::store::HaEventsApplySession, ProxyError> {
        self.key_store.begin_ha_events_apply(channel).await
    }

    pub async fn apply_ha_baseline_ndjson(
        &self,
        channel: HaSyncChannel,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        self.key_store.apply_ha_baseline_ndjson(channel, ndjson).await
    }

    pub async fn apply_ha_events_ndjson(
        &self,
        channel: HaSyncChannel,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        self.key_store.apply_ha_events_ndjson(channel, ndjson).await
    }

    pub async fn list_ha_events_after(
        &self,
        channel: HaSyncChannel,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<HaEventRecord>, ProxyError> {
        self.key_store
            .list_ha_events_after(channel, after_seq, limit)
            .await
    }

    pub async fn ack_ha_peer_watermark(
        &self,
        channel: HaSyncChannel,
        peer_node_id: &str,
        acked_seq: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .ack_ha_peer_watermark(channel, peer_node_id, acked_seq)
            .await
    }

    pub async fn insert_ha_failover_operation(
        &self,
        record: &HaFailoverOperationRecord,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_ha_failover_operation(record)
            .await
    }

    pub async fn insert_ha_edgeone_audit_log(
        &self,
        id: &str,
        action: &str,
        request_json: Option<&str>,
        response_json: Option<&str>,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_ha_edgeone_audit_log(
                id,
                action,
                request_json,
                response_json,
                status,
                message,
            )
            .await
    }

    pub async fn insert_ha_control_plane_event(
        &self,
        event: &HaControlPlaneEventInsert,
    ) -> Result<i64, ProxyError> {
        self.key_store.insert_ha_control_plane_event(event).await
    }

    pub async fn list_ha_control_plane_events(
        &self,
        cursor: Option<i64>,
        limit: i64,
        node_id: Option<&str>,
        category: Option<HaControlPlaneEventCategory>,
    ) -> Result<Vec<HaControlPlaneEventView>, ProxyError> {
        self.key_store
            .list_ha_control_plane_events(cursor, limit, node_id, category)
            .await
    }

    pub async fn list_ha_control_plane_events_for_node_interactions(
        &self,
        cursor: Option<i64>,
        limit: i64,
        node_id: &str,
    ) -> Result<Vec<HaControlPlaneEventView>, ProxyError> {
        self.key_store
            .list_ha_control_plane_events_for_node_interactions(cursor, limit, node_id)
            .await
    }

    pub async fn gc_ha_control_plane_events(&self) -> Result<i64, ProxyError> {
        self.key_store.gc_ha_control_plane_events().await
    }

    pub async fn claim_ha_recovery_batch(
        &self,
        batch_id: &str,
        source_node_id: &str,
        event_count: i64,
        checksum: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .claim_ha_recovery_batch(batch_id, source_node_id, event_count, checksum)
            .await
    }

    pub async fn complete_ha_recovery_batch(
        &self,
        batch_id: &str,
        status: &str,
        event_count: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .complete_ha_recovery_batch(batch_id, status, event_count)
            .await
    }

    pub async fn import_ha_recovery_events(&self) -> Result<i64, ProxyError> {
        self.key_store.import_ha_recovery_events().await
    }

    pub async fn rebuild_ha_recovery_rollups(&self) -> Result<(), ProxyError> {
        self.key_store.rebuild_request_log_catalog_rollups().await?;
        self.key_store.rebuild_api_key_usage_buckets().await?;
        self.key_store
            .rebuild_dashboard_request_rollup_buckets()
            .await?;
        self.key_store
            .rebuild_account_usage_rollup_buckets_v1()
            .await?;
        Ok(())
    }
}
