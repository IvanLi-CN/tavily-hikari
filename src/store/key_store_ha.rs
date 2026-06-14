#[derive(Clone, Debug)]
pub struct HaBaselineExport {
    pub ndjson: String,
    pub high_watermark: i64,
    pub row_count: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaOutboxEventRecord {
    pub seq: i64,
    pub kind: String,
    pub resource: String,
    pub resource_id: String,
    pub op: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub checksum: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HaApplyResult {
    pub high_watermark: i64,
    pub row_count: usize,
}

const HA_BASELINE_SCHEMA_VERSION: i64 = 1;
const HA_OUTBOX_RETENTION_SECS: i64 = 72 * 60 * 60;

const HA_BASELINE_TABLES: &[&str] = &[
    "account_monthly_quota",
    "account_quota_limit_snapshots",
    "account_quota_limits",
    "account_usage_buckets",
    "account_usage_rollup_buckets",
    "announcements",
    "api_key_low_quota_depletions",
    "api_key_maintenance_records",
    "api_key_quarantines",
    "api_key_quota_sync_samples",
    "api_key_transient_backoffs",
    "api_key_usage_buckets",
    "api_key_user_usage_buckets",
    "api_keys",
    "auth_token_quota",
    "auth_tokens",
    "forward_proxy_key_affinity",
    "forward_proxy_node_overrides",
    "forward_proxy_settings",
    "http_project_api_key_affinity",
    "linuxdo_credit_recharge_entitlements",
    "linuxdo_credit_recharge_orders",
    "meta",
    "mcp_sessions",
    "oauth_accounts",
    "quota_subject_locks",
    "request_rate_limit_snapshots",
    "scheduled_jobs",
    "subject_key_breakages",
    "token_api_key_bindings",
    "token_primary_api_key_affinity",
    "token_usage_buckets",
    "token_usage_stats",
    "user_api_key_bindings",
    "user_primary_api_key_affinity",
    "user_tag_bindings",
    "user_tags",
    "user_token_bindings",
    "users",
];

const HA_META_KEYS: &[&str] = &[
    "allow_registration_v1",
    "api_rebalance_enabled_v1",
    "api_rebalance_percent_v1",
    "global_ip_limit_v1",
    "mcp_session_affinity_key_count_v1",
    "rebalance_mcp_enabled_v1",
    "rebalance_mcp_session_percent_v1",
    "recharge_feature_enabled_v1",
    "recharge_user_enabled_v1",
    "request_rate_limit_v1",
    "trusted_client_ip_headers_v1",
    "trusted_proxy_cidrs_v1",
    "user_blocked_key_base_limit_v1",
];

impl KeyStore {
    pub(crate) async fn persist_ha_node_state(
        &self,
        node_id: &str,
        role: HaNodeRole,
        edgeone_origin: Option<&str>,
        source_settings: Option<&HaSourceSettingsView>,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        let (source_kind, direct_origin_scheme, direct_origin_host, direct_origin_port, origin_group_id) =
            match source_settings {
                Some(settings) => match settings.source_kind {
                    HaSourceKind::Direct => (
                        Some(settings.source_kind.as_str().to_string()),
                        settings
                            .direct_origin_scheme
                            .map(|scheme| format!("{scheme:?}").to_ascii_lowercase()),
                        settings.direct_origin_host.clone(),
                        settings.direct_origin_port.map(i64::from),
                        None,
                    ),
                    HaSourceKind::OriginGroup => (
                        Some(settings.source_kind.as_str().to_string()),
                        None,
                        None,
                        None,
                        settings.origin_group_id.clone(),
                    ),
                },
                None => (None, None, None, None, None),
            };
        sqlx::query(
            r#"
            INSERT INTO ha_node_state (
                id, node_id, role, edgeone_origin, ha_source_kind,
                ha_direct_origin_scheme, ha_direct_origin_host, ha_direct_origin_port,
                ha_origin_group_id, message, updated_at
            )
            VALUES ('local', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                node_id = excluded.node_id,
                role = excluded.role,
                edgeone_origin = excluded.edgeone_origin,
                ha_source_kind = excluded.ha_source_kind,
                ha_direct_origin_scheme = excluded.ha_direct_origin_scheme,
                ha_direct_origin_host = excluded.ha_direct_origin_host,
                ha_direct_origin_port = excluded.ha_direct_origin_port,
                ha_origin_group_id = excluded.ha_origin_group_id,
                message = excluded.message,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(node_id)
        .bind(role.as_str())
        .bind(edgeone_origin)
        .bind(source_kind)
        .bind(direct_origin_scheme)
        .bind(direct_origin_host)
        .bind(direct_origin_port)
        .bind(origin_group_id)
        .bind(message)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_ha_source_settings(
        &self,
    ) -> Result<Option<HaSourceSettings>, ProxyError> {
        let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>)>(
            r#"
            SELECT ha_source_kind, ha_direct_origin_scheme, ha_direct_origin_host,
                   ha_direct_origin_port, ha_origin_group_id
              FROM ha_node_state
             WHERE id = 'local'
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((source_kind, direct_origin_scheme, direct_origin_host, direct_origin_port, origin_group_id)) = row else {
            return Ok(None);
        };
        let Some(source_kind) = source_kind.as_deref().and_then(parse_ha_source_kind) else {
            return Ok(None);
        };
        let settings = match source_kind {
            HaSourceKind::Direct => Some(HaSourceSettings {
                source_kind,
                direct_origin_scheme: direct_origin_scheme
                    .as_deref()
                    .and_then(parse_origin_scheme),
                direct_origin_host,
                direct_origin_port: direct_origin_port.and_then(|port| u16::try_from(port).ok()),
                origin_group_id: None,
            }),
            HaSourceKind::OriginGroup => Some(HaSourceSettings {
                source_kind,
                direct_origin_scheme: None,
                direct_origin_host: None,
                direct_origin_port: None,
                origin_group_id,
            }),
        };
        Ok(settings)
    }

    pub(crate) async fn get_persisted_ha_node_role(&self) -> Result<Option<HaNodeRole>, ProxyError> {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT role FROM ha_node_state WHERE id = 'local'")
                .fetch_optional(&self.pool)
                .await?;
        Ok(raw.as_deref().and_then(parse_ha_node_role))
    }

    pub(crate) async fn persist_ha_sync_watermark(
        &self,
        name: &str,
        source_node_id: Option<&str>,
        target_node_id: Option<&str>,
        watermark: i64,
        detail: Option<&str>,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO ha_sync_watermarks (
                name, source_node_id, target_node_id, watermark, updated_at, detail
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(name) DO UPDATE SET
                source_node_id = excluded.source_node_id,
                target_node_id = excluded.target_node_id,
                watermark = excluded.watermark,
                updated_at = excluded.updated_at,
                detail = excluded.detail
            "#,
        )
        .bind(name)
        .bind(source_node_id)
        .bind(target_node_id)
        .bind(watermark)
        .bind(Utc::now().timestamp())
        .bind(detail)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn export_ha_baseline_ndjson(
        &self,
        node_id: &str,
    ) -> Result<HaBaselineExport, ProxyError> {
        let high_watermark = self.ha_outbox_high_watermark().await?;
        let mut ndjson = String::new();
        let mut row_count = 0_usize;
        ndjson.push_str(
            &serde_json::to_string(&serde_json::json!({
                "schemaVersion": HA_BASELINE_SCHEMA_VERSION,
                "kind": "baseline_start",
                "nodeId": node_id,
                "generatedAt": Utc::now().timestamp(),
                "highWatermark": high_watermark,
                "encoding": "zstd-ndjson"
            }))
            .map_err(|err| ProxyError::Other(err.to_string()))?,
        );
        ndjson.push('\n');

        for table in HA_BASELINE_TABLES {
            if !self.table_exists(table).await? {
                continue;
            }
            let rows = self.fetch_ha_table_json_rows(table).await?;
            for row in rows {
                row_count += 1;
                let row = sanitize_ha_resource_payload(table, row);
                ndjson.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "schemaVersion": HA_BASELINE_SCHEMA_VERSION,
                        "kind": "resource",
                        "resource": table,
                        "op": "upsert",
                        "data": row
                    }))
                    .map_err(|err| ProxyError::Other(err.to_string()))?,
                );
                ndjson.push('\n');
            }
        }

        ndjson.push_str(
            &serde_json::to_string(&serde_json::json!({
                "schemaVersion": HA_BASELINE_SCHEMA_VERSION,
                "kind": "baseline_end",
                "nodeId": node_id,
                "highWatermark": high_watermark,
                "rowCount": row_count
            }))
            .map_err(|err| ProxyError::Other(err.to_string()))?,
        );
        ndjson.push('\n');
        Ok(HaBaselineExport {
            ndjson,
            high_watermark,
            row_count,
        })
    }

    pub(crate) async fn ha_outbox_high_watermark(&self) -> Result<i64, ProxyError> {
        Ok(
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(seq) FROM ha_outbox")
                .fetch_one(&self.pool)
                .await?
                .unwrap_or(0),
        )
    }

    pub(crate) async fn get_ha_sync_watermark(
        &self,
        name: &str,
    ) -> Result<Option<i64>, ProxyError> {
        Ok(
            sqlx::query_scalar("SELECT watermark FROM ha_sync_watermarks WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn apply_ha_baseline_ndjson(
        &self,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        let mut high_watermark = 0_i64;
        let mut row_count = 0_usize;
        let mut saw_start = false;
        let mut saw_end = false;
        let mut resources = Vec::new();

        for line in ndjson.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line)
                .map_err(|err| ProxyError::Other(format!("invalid HA baseline NDJSON: {err}")))?;
            let kind = value.get("kind").and_then(serde_json::Value::as_str);
            match kind {
                Some("baseline_start") => {
                    saw_start = true;
                    high_watermark = value
                        .get("highWatermark")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0)
                        .max(0);
                }
                Some("resource") => {
                    let resource = value
                        .get("resource")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            ProxyError::Other("HA baseline resource is missing".to_string())
                        })?;
                    ensure_ha_resource_whitelisted(resource)?;
                    let data = value.get("data").cloned().ok_or_else(|| {
                        ProxyError::Other("HA baseline resource data is missing".to_string())
                    })?;
                    let data = sanitize_ha_resource_payload(resource, data);
                    resources.push((resource.to_string(), data));
                }
                Some("baseline_end") => {
                    saw_end = true;
                    high_watermark = value
                        .get("highWatermark")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(high_watermark)
                        .max(high_watermark);
                }
                other => {
                    return Err(ProxyError::Other(format!(
                        "unsupported HA baseline record kind: {other:?}"
                    )));
                }
            }
        }
        if !saw_start || !saw_end {
            return Err(ProxyError::Other(
                "HA baseline must include baseline_start and baseline_end".to_string(),
            ));
        }

        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        let result = async {
            insert_ha_outbox_suppression_on_conn(&mut conn).await?;
            for table in HA_BASELINE_TABLES {
                if self.table_exists(table).await? {
                    let sql = if *table == "meta" {
                        format!(
                            "DELETE FROM {} WHERE key IN ({})",
                            quote_sqlite_identifier(table),
                            ha_meta_key_list_sql()
                        )
                    } else {
                        format!("DELETE FROM {}", quote_sqlite_identifier(table))
                    };
                    sqlx::query(&sql).execute(&mut *conn).await?;
                }
            }
            for (resource, data) in &resources {
                insert_json_row_on_conn(&mut conn, resource, data).await?;
                row_count += 1;
            }
            clear_ha_outbox_suppression_on_conn(&mut conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        match result {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(&mut *conn)
                    .await?;
                Ok(HaApplyResult {
                    high_watermark,
                    row_count,
                })
            }
            Err(err) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                let _ = sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(&mut *conn)
                    .await;
                Err(err)
            }
        }
    }

    pub(crate) async fn apply_ha_events_ndjson(
        &self,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        let mut last_seq = 0_i64;
        let mut row_count = 0_usize;
        let mut events = Vec::new();
        for line in ndjson.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line)
                .map_err(|err| ProxyError::Other(format!("invalid HA events NDJSON: {err}")))?;
            match value.get("kind").and_then(serde_json::Value::as_str) {
                Some("events_start") => {}
                Some("event") => {
                    let event = value.get("event").ok_or_else(|| {
                        ProxyError::Other("HA event wrapper is missing event".to_string())
                    })?;
                    let resource = event
                        .get("resource")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| ProxyError::Other("HA event resource is missing".to_string()))?;
                    ensure_ha_resource_whitelisted(resource)?;
                    let op = event.get("op").and_then(serde_json::Value::as_str).unwrap_or("upsert");
                    let resource_id = event
                        .get("resourceId")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let payload = event.get("payload").cloned().unwrap_or(serde_json::Value::Null);
                    let payload = sanitize_ha_resource_payload(resource, payload);
                    last_seq = event
                        .get("seq")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(last_seq)
                        .max(last_seq);
                    events.push((resource.to_string(), resource_id, op.to_string(), payload));
                }
                Some("events_end") => {
                    last_seq = value
                        .get("lastSeq")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(last_seq)
                        .max(last_seq);
                }
                other => {
                    return Err(ProxyError::Other(format!(
                        "unsupported HA events record kind: {other:?}"
                    )));
                }
            }
        }

        let mut conn = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        let result = async {
            insert_ha_outbox_suppression_on_conn(&mut conn).await?;
            for (resource, resource_id, op, payload) in &events {
                match op.as_str() {
                    "delete" => {
                        delete_json_row_on_conn(&mut conn, resource, resource_id, payload).await?
                    }
                    "upsert" => insert_json_row_on_conn(&mut conn, resource, payload).await?,
                    other => {
                        return Err(ProxyError::Other(format!(
                            "unsupported HA event operation: {other}"
                        )));
                    }
                }
                row_count += 1;
            }
            clear_ha_outbox_suppression_on_conn(&mut conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        match result {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                Ok(HaApplyResult {
                    high_watermark: last_seq,
                    row_count,
                })
            }
            Err(err) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(err)
            }
        }
    }

    pub(crate) async fn list_ha_outbox_events_after(
        &self,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<HaOutboxEventRecord>, ProxyError> {
        self.prune_ha_outbox_retention().await?;
        let min_seq: Option<i64> = sqlx::query_scalar("SELECT MIN(seq) FROM ha_outbox")
            .fetch_one(&self.pool)
            .await?;
        let last_seq: Option<i64> =
            sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'ha_outbox'")
                .fetch_optional(&self.pool)
                .await?;
        if min_seq.is_none() && after_seq > 0 && last_seq.unwrap_or(0) > after_seq {
            return Err(ProxyError::Other(
                "HA outbox cursor is older than retention window".to_string(),
            ));
        }
        if let Some(min_seq) = min_seq
            && after_seq > 0
            && after_seq < min_seq.saturating_sub(1)
        {
            return Err(ProxyError::Other(
                "HA outbox cursor is older than retention window".to_string(),
            ));
        }
        let whitelist_sql = HA_BASELINE_TABLES
            .iter()
            .map(|table| quote_sqlite_string(table))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT seq, kind, resource, resource_id, op, payload_json, created_at, checksum
              FROM ha_outbox
             WHERE seq > ?
               AND resource IN ({whitelist_sql})
             ORDER BY seq ASC
             LIMIT ?
            "#
        );
        let rows = sqlx::query(&sql)
        .bind(after_seq.max(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let payload_raw: String = row.try_get("payload_json")?;
                let payload = serde_json::from_str(&payload_raw)
                    .map_err(|err| ProxyError::Other(format!("invalid HA outbox payload: {err}")))?;
                let resource: String = row.try_get("resource")?;
                let payload = sanitize_ha_resource_payload(&resource, payload);
                Ok(HaOutboxEventRecord {
                    seq: row.try_get("seq")?,
                    kind: row.try_get("kind")?,
                    resource,
                    resource_id: row.try_get("resource_id")?,
                    op: row.try_get("op")?,
                    payload,
                    created_at: row.try_get("created_at")?,
                    checksum: row.try_get("checksum")?,
                })
            })
            .collect()
    }

    pub(crate) async fn ack_ha_peer_watermark(
        &self,
        peer_node_id: &str,
        acked_seq: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO ha_peer_watermarks (peer_node_id, acked_seq, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(peer_node_id) DO UPDATE SET
                acked_seq = MAX(ha_peer_watermarks.acked_seq, excluded.acked_seq),
                updated_at = excluded.updated_at
            "#,
        )
        .bind(peer_node_id)
        .bind(acked_seq.max(0))
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn insert_ha_outbox_event(
        &self,
        kind: &str,
        resource: &str,
        resource_id: &str,
        op: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, ProxyError> {
        if !HA_BASELINE_TABLES.contains(&resource) {
            return Err(ProxyError::Other(format!(
                "HA outbox resource is not whitelisted: {resource}"
            )));
        }
        let payload_json =
            serde_json::to_string(payload).map_err(|err| ProxyError::Other(err.to_string()))?;
        let checksum = sha256_hex_bytes(payload_json.as_bytes());
        let result = sqlx::query(
            r#"
            INSERT INTO ha_outbox (
                kind, resource, resource_id, op, payload_json, created_at, checksum
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(kind)
        .bind(resource)
        .bind(resource_id)
        .bind(op)
        .bind(payload_json)
        .bind(Utc::now().timestamp())
        .bind(checksum)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub(crate) async fn insert_ha_failover_operation(
        &self,
        record: &HaFailoverOperationRecord,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO ha_failover_operations (
                id, operation_kind, target_node_id, from_origin, to_origin, status,
                message, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                message = excluded.message,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&record.operation_id)
        .bind(&record.operation_kind)
        .bind(record.target_node_id.as_deref())
        .bind(record.from_origin.as_deref())
        .bind(record.to_origin.as_deref())
        .bind(&record.status)
        .bind(record.message.as_deref())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn insert_ha_edgeone_audit_log(
        &self,
        id: &str,
        action: &str,
        request_json: Option<&str>,
        response_json: Option<&str>,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO ha_edgeone_audit_logs (
                id, action, request_json, response_json, status, message, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(action)
        .bind(request_json)
        .bind(response_json)
        .bind(status)
        .bind(message)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn claim_ha_recovery_batch(
        &self,
        batch_id: &str,
        source_node_id: &str,
        event_count: i64,
        checksum: &str,
    ) -> Result<bool, ProxyError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO ha_recovery_batches (
                id, source_node_id, status, event_count, created_at, checksum
            )
            VALUES (?, ?, 'importing', ?, ?, ?)
            "#,
        )
        .bind(batch_id)
        .bind(source_node_id)
        .bind(event_count)
        .bind(now)
        .bind(checksum)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub(crate) async fn complete_ha_recovery_batch(
        &self,
        batch_id: &str,
        status: &str,
        event_count: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE ha_recovery_batches
               SET status = ?, event_count = ?, imported_at = ?
             WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(event_count)
        .bind(Utc::now().timestamp())
        .bind(batch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn import_ha_recovery_events(&self) -> Result<i64, ProxyError> {
        Ok(0)
    }

    pub(crate) async fn table_exists(&self, table: &str) -> Result<bool, ProxyError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        )
        .bind(table)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    async fn fetch_ha_table_json_rows(
        &self,
        table: &str,
    ) -> Result<Vec<serde_json::Value>, ProxyError> {
        let columns = self.table_columns(table).await?;
        if columns.is_empty() {
            return Ok(Vec::new());
        }
        let json_args = columns
            .iter()
            .map(|column| {
                format!(
                    "{}, {}",
                    quote_sqlite_string(column),
                    quote_sqlite_identifier(column)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = if table == "meta" {
            format!(
                "SELECT json_object({json_args}) AS row_json FROM {} WHERE key IN ({}) ORDER BY key ASC",
                quote_sqlite_identifier(table),
                ha_meta_key_list_sql()
            )
        } else {
            format!(
                "SELECT json_object({json_args}) AS row_json FROM {} ORDER BY rowid ASC",
                quote_sqlite_identifier(table)
            )
        };
        let rows = sqlx::query_scalar::<_, String>(&sql)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|err| ProxyError::Other(format!("invalid HA baseline row: {err}")))
            })
            .collect()
    }

    async fn table_columns(&self, table: &str) -> Result<Vec<String>, ProxyError> {
        let sql = format!("PRAGMA table_info({})", quote_sqlite_identifier(table));
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| row.try_get::<String, _>("name").map_err(ProxyError::from))
            .collect()
    }

    pub(crate) async fn ensure_ha_outbox_triggers(&self) -> Result<(), ProxyError> {
        for table in HA_BASELINE_TABLES {
            if !self.table_exists(table).await? {
                continue;
            }
            let columns = self.table_columns(table).await?;
            if columns.is_empty() {
                continue;
            }
            let new_json = ha_trigger_json_object("NEW", &columns);
            let old_json = ha_trigger_json_object("OLD", &columns);
            let new_resource_id = ha_trigger_resource_id("NEW", &columns);
            let old_resource_id = ha_trigger_resource_id("OLD", &columns);
            let table_ident = quote_sqlite_identifier(table);
            let table_lit = quote_sqlite_string(table);
            let meta_filter = if *table == "meta" {
                Some(format!("key IN ({})", ha_meta_key_list_sql()))
            } else {
                None
            };
            for (suffix, timing, row_json, resource_id, op) in [
                ("insert", "AFTER INSERT", new_json.as_str(), new_resource_id.as_str(), "upsert"),
                ("update", "AFTER UPDATE", new_json.as_str(), new_resource_id.as_str(), "upsert"),
                ("delete", "AFTER DELETE", old_json.as_str(), old_resource_id.as_str(), "delete"),
            ] {
                let trigger = quote_sqlite_identifier(&format!("trg_ha_outbox_{table}_{suffix}"));
                let row_alias = if timing == "AFTER DELETE" { "OLD" } else { "NEW" };
                let row_filter = meta_filter
                    .as_ref()
                    .map(|filter| format!(" AND {row_alias}.{filter}"))
                    .unwrap_or_default();
                let sql = format!(
                    r#"
                    CREATE TRIGGER IF NOT EXISTS {trigger}
                    {timing} ON {table_ident}
                    WHEN NOT EXISTS (SELECT 1 FROM ha_outbox_suppression WHERE id = 'local'){row_filter}
                    BEGIN
                        INSERT INTO ha_outbox (
                            kind, resource, resource_id, op, payload_json, created_at, checksum
                        )
                        VALUES (
                            'state',
                            {table_lit},
                            {resource_id},
                            '{op}',
                            {row_json},
                            CAST(strftime('%s','now') AS INTEGER),
                            NULL
                        );
                    END
                    "#
                );
                sqlx::query(&sql).execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    async fn prune_ha_outbox_retention(&self) -> Result<(), ProxyError> {
        let threshold = Utc::now().timestamp() - HA_OUTBOX_RETENTION_SECS;
        sqlx::query("DELETE FROM ha_outbox WHERE created_at < ?")
            .bind(threshold)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

}

async fn insert_ha_outbox_suppression_on_conn(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
) -> Result<(), ProxyError> {
    sqlx::query("INSERT OR IGNORE INTO ha_outbox_suppression (id) VALUES ('local')")
        .execute(&mut **conn)
        .await?;
    Ok(())
}

async fn clear_ha_outbox_suppression_on_conn(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
) -> Result<(), ProxyError> {
    sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
        .execute(&mut **conn)
        .await?;
    Ok(())
}

fn quote_sqlite_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_sqlite_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ha_meta_key_list_sql() -> String {
    HA_META_KEYS
        .iter()
        .map(|key| quote_sqlite_string(key))
        .collect::<Vec<_>>()
        .join(", ")
}

fn ensure_ha_resource_whitelisted(resource: &str) -> Result<(), ProxyError> {
    if HA_BASELINE_TABLES.contains(&resource) {
        Ok(())
    } else {
        Err(ProxyError::Other(format!(
            "HA resource is not whitelisted: {resource}"
        )))
    }
}

fn sanitize_ha_resource_payload(
    resource: &str,
    mut payload: serde_json::Value,
) -> serde_json::Value {
    let columns: &[&str] = match resource {
        "api_key_maintenance_records" => &["request_log_id", "auth_token_log_id"],
        "api_key_transient_backoffs" => &["source_request_log_id"],
        _ => &[],
    };
    if columns.is_empty() {
        return payload;
    }
    if let Some(object) = payload.as_object_mut() {
        for column in columns {
            if object.contains_key(*column) {
                object.insert((*column).to_string(), serde_json::Value::Null);
            }
        }
    }
    payload
}

fn ha_trigger_json_object(alias: &str, columns: &[String]) -> String {
    let args = columns
        .iter()
        .map(|column| {
            format!(
                "{}, {alias}.{}",
                quote_sqlite_string(column),
                quote_sqlite_identifier(column)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("json_object({args})")
}

fn ha_trigger_resource_id(alias: &str, columns: &[String]) -> String {
    if columns.iter().any(|column| column == "id") {
        format!(
            "COALESCE(CAST({alias}.{} AS TEXT), CAST({alias}.rowid AS TEXT))",
            quote_sqlite_identifier("id")
        )
    } else {
        format!("CAST({alias}.rowid AS TEXT)")
    }
}

async fn insert_json_row_on_conn(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    table: &str,
    row: &serde_json::Value,
) -> Result<(), ProxyError> {
    ensure_ha_resource_whitelisted(table)?;
    let Some(object) = row.as_object() else {
        return Err(ProxyError::Other(
            "HA row payload must be a JSON object".to_string(),
        ));
    };
    let columns = table_column_info_on_conn(conn, table).await?;
    let selected = columns
        .iter()
        .filter_map(|column| {
            object
                .get(&column.name)
                .map(|value| (column.name.as_str(), value))
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(ProxyError::Other(format!(
            "HA row payload has no columns for {table}"
        )));
    }
    let mut primary_key_columns = columns
        .iter()
        .filter(|column| column.pk > 0)
        .collect::<Vec<_>>();
    primary_key_columns.sort_by_key(|column| column.pk);
    if !primary_key_columns
        .iter()
        .all(|column| object.contains_key(&column.name))
    {
        return Err(ProxyError::Other(format!(
            "HA row payload is missing primary key columns for {table}"
        )));
    }
    let column_sql = selected
        .iter()
        .map(|(column, _)| quote_sqlite_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = std::iter::repeat_n("?", selected.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = if primary_key_columns.is_empty() {
        format!(
            "INSERT OR REPLACE INTO {} ({column_sql}) VALUES ({placeholders})",
            quote_sqlite_identifier(table)
        )
    } else {
        let conflict_sql = primary_key_columns
            .iter()
            .map(|column| quote_sqlite_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let update_sql = selected
            .iter()
            .filter(|(column, _)| !primary_key_columns.iter().any(|pk| pk.name == *column))
            .map(|(column, _)| {
                let ident = quote_sqlite_identifier(column);
                format!("{ident} = excluded.{ident}")
            })
            .collect::<Vec<_>>();
        if update_sql.is_empty() {
            format!(
                "INSERT INTO {} ({column_sql}) VALUES ({placeholders}) ON CONFLICT({conflict_sql}) DO NOTHING",
                quote_sqlite_identifier(table)
            )
        } else {
            format!(
                "INSERT INTO {} ({column_sql}) VALUES ({placeholders}) ON CONFLICT({conflict_sql}) DO UPDATE SET {}",
                quote_sqlite_identifier(table),
                update_sql.join(", ")
            )
        }
    };
    let mut query = sqlx::query(&sql);
    for (_, value) in selected {
        query = bind_json_value(query, value);
    }
    query.execute(&mut **conn).await?;
    Ok(())
}

async fn delete_json_row_on_conn(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    table: &str,
    resource_id: &str,
    row: &serde_json::Value,
) -> Result<(), ProxyError> {
    ensure_ha_resource_whitelisted(table)?;
    let columns = table_column_info_on_conn(conn, table).await?;
    let mut primary_key_columns = columns
        .iter()
        .filter(|column| column.pk > 0)
        .collect::<Vec<_>>();
    primary_key_columns.sort_by_key(|column| column.pk);
    if let Some(object) = row.as_object()
        && !primary_key_columns.is_empty()
        && primary_key_columns
            .iter()
            .all(|column| object.contains_key(&column.name))
    {
        let where_sql = primary_key_columns
            .iter()
            .map(|column| format!("{} = ?", quote_sqlite_identifier(&column.name)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "DELETE FROM {} WHERE {where_sql}",
            quote_sqlite_identifier(table)
        );
        let mut query = sqlx::query(&sql);
        for column in primary_key_columns {
            let value = object.get(&column.name).ok_or_else(|| {
                ProxyError::Other(format!("HA delete payload missing primary key for {table}"))
            })?;
            query = bind_json_value(query, value);
        }
        query.execute(&mut **conn).await?;
        return Ok(());
    }
    let key_column = if columns.iter().any(|column| column.name == "id") {
        quote_sqlite_identifier("id")
    } else {
        "rowid".to_string()
    };
    let sql = format!(
        "DELETE FROM {} WHERE {key_column} = ?",
        quote_sqlite_identifier(table)
    );
    sqlx::query(&sql)
        .bind(resource_id)
        .execute(&mut **conn)
        .await?;
    Ok(())
}

#[derive(Debug)]
struct SqliteColumnInfo {
    name: String,
    pk: i64,
}

async fn table_column_info_on_conn(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    table: &str,
) -> Result<Vec<SqliteColumnInfo>, ProxyError> {
    let rows = sqlx::query(&format!("PRAGMA table_info({})", quote_sqlite_identifier(table)))
        .fetch_all(&mut **conn)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(SqliteColumnInfo {
                name: row.try_get("name")?,
                pk: row.try_get("pk")?,
            })
        })
        .collect()
}

fn bind_json_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: &'q serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match value {
        serde_json::Value::Null => query.bind(Option::<String>::None),
        serde_json::Value::Bool(value) => query.bind(i64::from(*value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                query.bind(value)
            } else if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
                query.bind(value)
            } else if let Some(value) = value.as_f64() {
                query.bind(value)
            } else {
                query.bind(value.to_string())
            }
        }
        serde_json::Value::String(value) => query.bind(value.as_str()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => query.bind(value.to_string()),
    }
}

fn parse_ha_node_role(value: &str) -> Option<HaNodeRole> {
    match value {
        "full_master" => Some(HaNodeRole::FullMaster),
        "provisional_master" => Some(HaNodeRole::ProvisionalMaster),
        "standby" => Some(HaNodeRole::Standby),
        "recovery" => Some(HaNodeRole::Recovery),
        _ => None,
    }
}
