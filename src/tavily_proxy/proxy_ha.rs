impl TavilyProxy {
    pub async fn persist_ha_node_state(
        &self,
        node_id: &str,
        role: HaNodeRole,
        edgeone_origin: Option<&str>,
        source_settings: Option<&HaSourceSettingsView>,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .persist_ha_node_state(node_id, role, edgeone_origin, source_settings, message)
            .await
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
        self.key_store
            .persist_ha_sync_watermark(name, source_node_id, target_node_id, watermark, detail)
            .await
    }

    pub async fn get_ha_sync_watermark(&self, name: &str) -> Result<Option<i64>, ProxyError> {
        self.key_store.get_ha_sync_watermark(name).await
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
