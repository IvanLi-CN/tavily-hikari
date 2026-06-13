impl TavilyProxy {
    fn spawn_request_stats_coalescer(&self) {
        let store = self.key_store.clone();
        let coalescer = self.key_store.request_stats_coalescer.clone();
        tokio::spawn(async move {
            loop {
                let should_flush_now = {
                    let state = coalescer.state.lock().await;
                    state.shutdown
                        || !state.pending_dashboard_rollups.is_empty()
                        || !state.pending_api_key_usage.is_empty()
                        || !state.pending_auth_token_activity.is_empty()
                        || !state.pending_account_request_rollups.is_empty()
                        || !state.pending_request_log_catalog.is_empty()
                        || state.pending_dashboard_rollups.len()
                            + state.pending_api_key_usage.len()
                            + state.pending_auth_token_activity.len()
                            + state.pending_account_request_rollups.len()
                            + state.pending_request_log_catalog.len()
                            >= RequestStatsCoalescer::MAX_PENDING_KEYS
                };
                if !should_flush_now {
                    tokio::select! {
                        _ = coalescer.wake.notified() => {}
                        _ = tokio::time::sleep(RequestStatsCoalescer::FLUSH_INTERVAL) => {}
                    }
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
                        continue;
                    }
                    state.shutdown
                };

                if let Err(err) = store.flush_request_stats_writes().await {
                    eprintln!("request stats persist warning: {err}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                {
                    let state = coalescer.state.lock().await;
                    if shutdown_after_flush
                        && state.pending_dashboard_rollups.is_empty()
                        && state.pending_api_key_usage.is_empty()
                        && state.pending_auth_token_activity.is_empty()
                        && state.pending_account_request_rollups.is_empty()
                        && state.pending_request_log_catalog.is_empty()
                    {
                        break;
                    }
                }
            }
        });
    }

    fn spawn_ha_state_coalescer(&self) {
        let store = self.key_store.clone();
        let coalescer = self.ha_state_coalescer.clone();
        tokio::spawn(async move {
            loop {
                let should_flush_now = {
                    let state = coalescer.state.lock().await;
                    state.shutdown
                        || state.pending_node_state.is_some()
                        || state.pending_sync_watermarks.len() >= HaStateCoalescer::MAX_PENDING_KEYS
                };
                if !should_flush_now {
                    tokio::select! {
                        _ = coalescer.wake.notified() => {}
                        _ = tokio::time::sleep(HaStateCoalescer::FLUSH_INTERVAL) => {}
                    }
                }

                let (pending_node_state, pending_sync_watermarks, shutdown_after_flush) = {
                    let mut state = coalescer.state.lock().await;
                    if state.pending_node_state.is_none()
                        && state.pending_sync_watermarks.is_empty()
                        && !state.shutdown
                    {
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
                        eprintln!("HA sync watermark persist warning: {err}");
                    }
                }

                if let Some(pending) = pending_node_state
                    && let Err(err) = store
                        .persist_ha_node_state(
                            &pending.node_id,
                            pending.role,
                            pending.edgeone_origin.as_deref(),
                            pending.message.as_deref(),
                        )
                        .await
                {
                    eprintln!("HA node state persist warning: {err}");
                }

                {
                    let mut state = coalescer.state.lock().await;
                    state.flushing = false;
                    coalescer.flushed.notify_waiters();
                    if shutdown_after_flush
                        && state.pending_node_state.is_none()
                        && state.pending_sync_watermarks.is_empty()
                    {
                        break;
                    }
                }
            }
        });
    }

    pub async fn persist_ha_node_state(
        &self,
        node_id: &str,
        role: HaNodeRole,
        edgeone_origin: Option<&str>,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.ha_state_coalescer
            .enqueue_node_state(node_id, role, edgeone_origin, message)
            .await;
        Ok(())
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

    pub async fn flush_ha_state_writes(&self) -> Result<(), ProxyError> {
        self.ha_state_coalescer.wake.notify_one();
        self.ha_state_coalescer.wait_until_flushed().await;
        Ok(())
    }

    pub async fn export_ha_baseline_ndjson(
        &self,
        node_id: &str,
    ) -> Result<HaBaselineExport, ProxyError> {
        self.key_store.export_ha_baseline_ndjson(node_id).await
    }

    pub async fn apply_ha_baseline_ndjson(
        &self,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        self.key_store.apply_ha_baseline_ndjson(ndjson).await
    }

    pub async fn apply_ha_events_ndjson(
        &self,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        self.key_store.apply_ha_events_ndjson(ndjson).await
    }

    pub async fn list_ha_outbox_events_after(
        &self,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<HaOutboxEventRecord>, ProxyError> {
        self.key_store
            .list_ha_outbox_events_after(after_seq, limit)
            .await
    }

    pub async fn ack_ha_peer_watermark(
        &self,
        peer_node_id: &str,
        acked_seq: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .ack_ha_peer_watermark(peer_node_id, acked_seq)
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
