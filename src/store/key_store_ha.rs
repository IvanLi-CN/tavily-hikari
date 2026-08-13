use std::collections::{HashMap, HashSet};

type HaGcChannelStateRow = (
    Option<i64>,
    i64,
    Option<String>,
    Option<i64>,
    i64,
    i64,
    String,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    f64,
    Option<i64>,
    String,
    i64,
);

fn record_ha_outbox_gc_batch_timing(
    active_elapsed_ms: &mut u128,
    max_batch_elapsed_ms: &mut u128,
    batch_started: Instant,
) {
    let batch_elapsed_ms = batch_started.elapsed().as_millis();
    *active_elapsed_ms = active_elapsed_ms.saturating_add(batch_elapsed_ms);
    *max_batch_elapsed_ms = (*max_batch_elapsed_ms).max(batch_elapsed_ms);
}

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
        .bind(self.backend_time.now_ts())
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
        .bind(self.backend_time.now_ts())
        .bind(detail)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn export_ha_baseline_ndjson(
        &self,
        channel: HaSyncChannel,
        node_id: &str,
    ) -> Result<HaBaselineExport, ProxyError> {
        let high_watermark = self.ha_channel_high_watermark(channel).await?;
        let mut ndjson = String::new();
        let mut row_count = 0_usize;
        ndjson.push_str(
            &serde_json::to_string(&serde_json::json!({
                "schemaVersion": HA_SCHEMA_VERSION,
                "kind": "baseline_start",
                "channel": channel,
                "nodeId": node_id,
                "generatedAt": self.backend_time.now_ts(),
                "highWatermark": high_watermark,
                "encoding": "zstd-ndjson"
            }))
            .map_err(|err| ProxyError::Other(err.to_string()))?,
        );
        ndjson.push('\n');

        for table in ha_baseline_tables(channel) {
            if !self.table_exists(table).await? {
                continue;
            }
            let rows = self.fetch_ha_table_json_rows(channel, table).await?;
            for row in rows {
                row_count += 1;
                let row = sanitize_ha_resource_payload(table, row);
                ndjson.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "schemaVersion": HA_SCHEMA_VERSION,
                        "kind": "resource",
                        "channel": channel,
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
                "schemaVersion": HA_SCHEMA_VERSION,
                "kind": "baseline_end",
                "channel": channel,
                "nodeId": node_id,
                "highWatermark": high_watermark,
                "rowCount": row_count
            }))
            .map_err(|err| ProxyError::Other(err.to_string()))?,
        );
        ndjson.push('\n');
        Ok(HaBaselineExport {
            channel,
            ndjson,
            high_watermark,
            row_count,
        })
    }

    pub(crate) async fn write_ha_baseline_ndjson<W>(
        &self,
        channel: HaSyncChannel,
        node_id: &str,
        writer: &mut W,
    ) -> Result<HaApplyResult, ProxyError>
    where
        W: AsyncWrite + Unpin + Send,
    {
        let mut session = self.begin_ha_baseline_read(channel).await?;
        let mut export = session.export_info().await?;
        let write_result = session
            .write_ndjson(node_id, export.high_watermark, export.row_count, writer)
            .await;
        let close_result = session.close().await;
        if let Ok(payload_bytes) = write_result.as_ref() {
            export.payload_bytes = *payload_bytes;
        }
        write_result?;
        close_result?;
        Ok(export)
    }

    pub(crate) async fn count_ha_baseline_rows(
        &self,
        channel: HaSyncChannel,
    ) -> Result<usize, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        Self::count_ha_baseline_rows_on_conn(&mut conn, channel).await
    }

    pub(crate) async fn ha_channel_high_watermark(
        &self,
        channel: HaSyncChannel,
    ) -> Result<i64, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        Self::ha_channel_high_watermark_on_conn(&mut conn, channel).await
    }

    pub(crate) async fn ha_channel_outbox_stats(
        &self,
        channel: HaSyncChannel,
        peer_node_id: Option<&str>,
    ) -> Result<HaOutboxStats, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let oldest_created_at: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT created_at FROM {table} WHERE resource IN ({allowed_resources}) ORDER BY created_at ASC, seq ASC LIMIT 1"
        ))
        .fetch_optional(&mut *conn)
        .await?;
        let minimum_retained_seq: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT MIN(seq) FROM {table} WHERE resource IN ({allowed_resources})"
        ))
        .fetch_one(&mut *conn)
        .await?;
        let acked_seq = match peer_node_id {
            Some(peer_node_id) => {
                sqlx::query_scalar::<_, Option<i64>>(
                    r#"
                    SELECT acked_seq
                      FROM ha_peer_watermarks
                     WHERE peer_node_id = ?
                       AND channel = ?
                    "#,
                )
                .bind(peer_node_id)
                .bind(channel.as_str())
                .fetch_optional(&mut *conn)
                .await?
                .flatten()
            }
            None => None,
        };
        let high_watermark = Self::ha_channel_high_watermark_on_conn(&mut conn, channel).await?;
        let now = self.backend_time.now_ts();
        let sequence_span_estimate = minimum_retained_seq
            .map(|minimum_seq| high_watermark.saturating_sub(minimum_seq).saturating_add(1))
            .unwrap_or(0);
        Ok(HaOutboxStats {
            sequence_span_estimate,
            high_watermark,
            oldest_age_secs: oldest_created_at
                .map(|created_at| now.saturating_sub(created_at).max(0))
                .unwrap_or(0),
            ack_lag: acked_seq.map(|acked| high_watermark.saturating_sub(acked).max(0)),
        })
    }

    pub(crate) async fn ha_peer_channel_health(
        &self,
        channel: HaSyncChannel,
        peer_node_id: &str,
    ) -> Result<crate::HaChannelHealthView, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let high_watermark = Self::ha_channel_high_watermark_on_conn(&mut conn, channel).await?;
        let acked_seq = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT acked_seq FROM ha_peer_watermarks WHERE peer_node_id = ? AND channel = ?"#,
        )
        .bind(peer_node_id)
        .bind(channel.as_str())
        .fetch_optional(&mut *conn)
        .await?
        .flatten();
        let threshold = self.backend_time.now_ts() - ha_channel_retention_secs(channel);
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let (has_retained_after_ack, has_retained_gap_after_ack) =
            if let Some(acked) = acked_seq {
                let has_retained_after_ack: bool = sqlx::query_scalar(&format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE created_at >= ? AND resource IN ({allowed_resources}) AND seq > ?)"
                ))
                .bind(threshold)
                .bind(acked)
                .fetch_one(&mut *conn)
                .await?;
                let first_retained_seq: Option<i64> = sqlx::query_scalar(&format!(
                    "SELECT MIN(seq) FROM {table} WHERE created_at >= ? AND resource IN ({allowed_resources}) AND seq > ?"
                ))
                .bind(threshold)
                .bind(acked)
                    .fetch_one(&mut *conn)
                    .await?;
                let expired_valid_seq =
                    Self::ha_channel_expired_valid_watermark_on_conn(&mut conn, channel).await?;
                let expired_legacy_seq =
                    Self::ha_channel_expired_legacy_watermark_on_conn(&mut conn, channel).await?;
                let first_retained_gap = if let Some(first) = first_retained_seq {
                    let gap = first > acked.saturating_add(1);
                    let expired_valid_gap = expired_valid_seq.is_some_and(|deleted| acked < deleted);
                    let expired_legacy_gap = expired_legacy_seq
                        .is_some_and(|deleted| acked < deleted && deleted < first);
                    let has_any_bridge: bool = if expired_valid_gap || expired_legacy_gap {
                        false
                    } else {
                        sqlx::query_scalar(&format!(
                            "SELECT EXISTS(SELECT 1 FROM {table} WHERE created_at >= ? AND seq > ? AND seq < ?)"
                        ))
                        .bind(threshold)
                        .bind(acked)
                        .bind(first)
                        .fetch_one(&mut *conn)
                        .await?
                    };
                    gap && (expired_valid_gap || (!expired_legacy_gap && !has_any_bridge))
                } else {
                    false
                };
                (has_retained_after_ack, first_retained_gap)
            } else {
                (false, false)
            };
        let expired_backlog = acked_seq.is_some_and(|acked| {
            acked > 0
                && high_watermark > acked
                && (!has_retained_after_ack || has_retained_gap_after_ack)
        });
        let oldest_created_at: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT created_at FROM {table} ORDER BY created_at ASC, seq ASC LIMIT 1"
        ))
        .fetch_optional(&mut *conn)
        .await?;
        let gc_state_row: Option<HaGcChannelStateRow> =
            sqlx::query_as(
                r#"SELECT last_progress_at, last_deleted_rows, last_defer_reason,
                          next_retry_at, batch_size, consecutive_no_progress,
                          debt_mode, last_observed_at, last_ingress_seq_delta,
                          last_net_rows_delta_estimate, deleted_rows_per_minute,
                          recovery_deadline_at, slo_state, foreground_rps
                   FROM ha_outbox_gc_channel_state WHERE channel = ?"#,
            )
            .bind(channel.as_str())
            .fetch_optional(&mut *conn)
            .await?;
        let gc_state_unknown = gc_state_row.is_none();
        let (
            last_progress_at,
            last_deleted_rows,
            last_defer_reason,
            next_retry_at,
            batch_size,
            consecutive_no_progress,
            gc_debt_mode,
            gc_observed_at,
            last_ingress_seq_delta,
            last_net_rows_delta_estimate,
            gc_deleted_rows_per_minute,
            gc_recovery_deadline_at,
            gc_slo_state,
            gc_foreground_rps,
        ) = gc_state_row.unwrap_or((
            None,
            0,
            None,
            None,
            250,
            0,
            "unknown".to_string(),
            None,
            None,
            None,
            0.0,
            None,
            "unknown".to_string(),
            0,
        ));
        let now = self.backend_time.now_ts();
        let gc_state = if gc_state_unknown || gc_observed_at.is_none() {
            "unknown"
        } else if consecutive_no_progress >= 3 {
            "stalled"
        } else if gc_debt_mode == "recovering" {
            "recovering"
        } else if last_deleted_rows > 0 {
            "draining"
        } else if last_defer_reason.is_some() {
            "deferred"
        } else {
            "eligible"
        };
        let cursor_state = if expired_backlog {
            "expired_backlog"
        } else if acked_seq == Some(0) && high_watermark > 0 {
            "baseline_required"
        } else if high_watermark == 0 || acked_seq.is_some_and(|acked| acked >= high_watermark) {
            "healthy"
        } else if acked_seq.is_some() {
            "catching_up"
        } else {
            "baseline_required"
        };
        Ok(crate::HaChannelHealthView {
            channel,
            acked_seq,
            high_watermark,
            ack_lag: acked_seq.map(|acked| high_watermark.saturating_sub(acked).max(0)),
            cursor_state: cursor_state.to_string(),
            retention_secs: ha_channel_retention_secs(channel),
            expired_backlog,
            gc_state: gc_state.to_string(),
            oldest_age_secs: oldest_created_at.map(|created_at| now.saturating_sub(created_at).max(0)),
            last_progress_at,
            last_defer_reason,
            next_retry_at,
            batch_size,
            gc_debt_mode,
            gc_observed_at,
            last_ingress_seq_delta,
            last_net_rows_delta_estimate,
            gc_deleted_rows_per_minute,
            gc_recovery_deadline_at,
            gc_slo_state,
            gc_foreground_rps,
        })
    }

    pub(crate) async fn begin_ha_baseline_read(
        &self,
        channel: HaSyncChannel,
    ) -> Result<HaBaselineReadSession, ProxyError> {
        let conn = self
            .sqlite_runtime
            .begin_read_snapshot(SqliteOperation::HaBaselineRead)
            .await?;
        Ok(HaBaselineReadSession {
            channel,
            conn,
            generated_at: self.backend_time.now_ts(),
        })
    }

    pub(crate) async fn begin_ha_events_read(
        &self,
        channel: HaSyncChannel,
    ) -> Result<HaEventsReadSession, ProxyError> {
        let conn = self
            .sqlite_runtime
            .begin_read_snapshot(SqliteOperation::HaEventsRead)
            .await?;
        Ok(HaEventsReadSession {
            channel,
            conn,
            generated_at: self.backend_time.now_ts(),
        })
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
        channel: HaSyncChannel,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        let mut session = self
            .begin_ha_baseline_apply_with_mode(channel, HaBaselineApplyMode::Replace)
            .await?;
        for line in ndjson.lines().filter(|line| !line.trim().is_empty()) {
            if let Err(err) = session.apply_line(line).await {
                let _ = session.abort().await;
                return Err(err);
            }
        }
        session.finish().await
    }

    pub(crate) async fn apply_ha_events_ndjson(
        &self,
        channel: HaSyncChannel,
        ndjson: &str,
    ) -> Result<HaApplyResult, ProxyError> {
        let mut session = self.begin_ha_events_apply(channel).await?;
        for line in ndjson.lines().filter(|line| !line.trim().is_empty()) {
            if let Err(err) = session.apply_line(line).await {
                let _ = session.abort().await;
                return Err(err);
            }
        }
        session.finish().await
    }

    pub(crate) async fn list_ha_events_after(
        &self,
        channel: HaSyncChannel,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<HaEventRecord>, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let threshold = self.backend_time.now_ts() - ha_channel_retention_secs(channel);
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        Self::validate_ha_events_cursor_on_conn(&mut conn, channel, after_seq, threshold).await?;
        let sql = format!(
            r#"
            SELECT seq, kind, resource, resource_id, op, payload_json, created_at, checksum
              FROM {table}
             WHERE seq > ?
               AND created_at >= ?
               AND resource IN ({allowed_resources})
             ORDER BY seq ASC
             LIMIT ?
            "#
        );
        let mut rows = sqlx::query(&sql)
            .bind(after_seq.max(0))
            .bind(threshold)
            .bind(limit.clamp(1, 1000))
            .fetch(&mut *conn);

        let mut events = Vec::new();
        while let Some(row) = rows.try_next().await? {
            let resource: String = row.try_get("resource")?;
            let payload_raw: String = row.try_get("payload_json")?;
            let payload = serde_json::from_str(&payload_raw)
                .map_err(|err| ProxyError::Other(format!("invalid HA outbox payload: {err}")))?;
            let payload = sanitize_ha_resource_payload(&resource, payload);
            events.push(HaEventRecord {
                channel,
                seq: row.try_get("seq")?,
                kind: row.try_get("kind")?,
                resource,
                resource_id: row.try_get("resource_id")?,
                op: row.try_get("op")?,
                payload,
                created_at: row.try_get("created_at")?,
                checksum: row.try_get("checksum")?,
            });
        }
        Ok(events)
    }

    pub(crate) async fn ack_ha_peer_watermark(
        &self,
        channel: HaSyncChannel,
        peer_node_id: &str,
        acked_seq: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO ha_peer_watermarks (peer_node_id, channel, acked_seq, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(peer_node_id, channel) DO UPDATE SET
                acked_seq = MAX(ha_peer_watermarks.acked_seq, excluded.acked_seq),
                updated_at = excluded.updated_at
            "#,
        )
        .bind(peer_node_id)
        .bind(channel.as_str())
        .bind(acked_seq.max(0))
        .bind(self.backend_time.now_ts())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn insert_ha_outbox_event(
        &self,
        channel: HaSyncChannel,
        kind: &str,
        resource: &str,
        resource_id: &str,
        op: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, ProxyError> {
        if ensure_ha_resource_whitelisted(channel, resource).is_err() {
            return Err(ProxyError::Other(format!(
                "HA outbox resource is not whitelisted: {resource}"
            )));
        }
        let payload_json =
            serde_json::to_string(payload).map_err(|err| ProxyError::Other(err.to_string()))?;
        let checksum = sha256_hex_bytes(payload_json.as_bytes());
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let result = sqlx::query(
            &format!(
                r#"
            INSERT INTO {table} (
                kind, resource, resource_id, op, payload_json, created_at, checksum
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
            ),
        )
        .bind(kind)
        .bind(resource)
        .bind(resource_id)
        .bind(op)
        .bind(payload_json)
        .bind(self.backend_time.now_ts())
        .bind(checksum)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub(crate) async fn insert_ha_failover_operation(
        &self,
        record: &HaFailoverOperationRecord,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
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
        .bind(self.backend_time.now_ts())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn insert_ha_control_plane_event(
        &self,
        event: &HaControlPlaneEventInsert,
    ) -> Result<i64, ProxyError> {
        let technical_details_json = event
            .technical_details
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        let result = sqlx::query(
            r#"
            INSERT INTO ha_control_plane_events (
                event_kind, category, status, node_id, operation_id, summary,
                detail, technical_details_json, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.event_kind)
        .bind(event.category.as_str())
        .bind(event.status.as_str())
        .bind(event.node_id.as_deref())
        .bind(event.operation_id.as_deref())
        .bind(&event.summary)
        .bind(event.detail.as_deref())
        .bind(technical_details_json)
        .bind(self.backend_time.now_ts())
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub(crate) async fn list_ha_control_plane_events(
        &self,
        cursor: Option<i64>,
        limit: i64,
        node_id: Option<&str>,
        category: Option<HaControlPlaneEventCategory>,
    ) -> Result<Vec<HaControlPlaneEventView>, ProxyError> {
        let threshold = self.backend_time.now_ts() - HA_CONTROL_PLANE_EVENT_RETENTION_SECS;
        let sql = r#"
            SELECT id, event_kind, category, status, node_id, operation_id, summary, detail,
                   technical_details_json, created_at
              FROM ha_control_plane_events
             WHERE created_at >= ?
               AND (? IS NULL OR id < ?)
               AND (? IS NULL OR node_id = ?)
               AND (? IS NULL OR category = ?)
             ORDER BY id DESC
             LIMIT ?
        "#;
        let rows = sqlx::query(sql)
            .bind(threshold)
            .bind(cursor)
            .bind(cursor)
            .bind(node_id)
            .bind(node_id)
            .bind(category.map(HaControlPlaneEventCategory::as_str))
            .bind(category.map(HaControlPlaneEventCategory::as_str))
            .bind(limit.clamp(1, 200))
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let category_raw: String = row.try_get("category")?;
                let status_raw: String = row.try_get("status")?;
                let technical_details = row
                    .try_get::<Option<String>, _>("technical_details_json")?
                    .map(|raw| {
                        serde_json::from_str(&raw)
                            .map_err(|err| ProxyError::Other(format!("invalid HA timeline details: {err}")))
                    })
                    .transpose()?;
                Ok(HaControlPlaneEventView {
                    id: row.try_get("id")?,
                    event_kind: row.try_get("event_kind")?,
                    category: HaControlPlaneEventCategory::parse(&category_raw).ok_or_else(|| {
                        ProxyError::Other(format!("invalid HA timeline category: {category_raw}"))
                    })?,
                    status: HaControlPlaneEventStatus::parse(&status_raw).ok_or_else(|| {
                        ProxyError::Other(format!("invalid HA timeline status: {status_raw}"))
                    })?,
                    node_id: row.try_get("node_id")?,
                    operation_id: row.try_get("operation_id")?,
                    summary: row.try_get("summary")?,
                    detail: row.try_get("detail")?,
                    technical_details,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    pub(crate) async fn list_ha_control_plane_events_for_node_interactions(
        &self,
        cursor: Option<i64>,
        limit: i64,
        node_id: &str,
    ) -> Result<Vec<HaControlPlaneEventView>, ProxyError> {
        let threshold = self.backend_time.now_ts() - HA_CONTROL_PLANE_EVENT_RETENTION_SECS;
        let direct_limit = limit.clamp(1, 200).saturating_mul(3);
        let direct_rows = sqlx::query(
            r#"
            SELECT id, event_kind, category, status, node_id, operation_id, summary, detail,
                   technical_details_json, created_at
              FROM ha_control_plane_events
             WHERE created_at >= ?
               AND (? IS NULL OR id < ?)
               AND node_id = ?
             ORDER BY id DESC
             LIMIT ?
            "#,
        )
        .bind(threshold)
        .bind(cursor)
        .bind(cursor)
        .bind(node_id)
        .bind(direct_limit)
        .fetch_all(&self.pool)
        .await?;
        let mut events = direct_rows
            .into_iter()
            .map(Self::decode_ha_control_plane_event_row)
            .collect::<Result<Vec<_>, _>>()?;

        let operation_ids = events
            .iter()
            .filter_map(|event| event.operation_id.as_deref())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .collect::<HashSet<_>>();
        if !operation_ids.is_empty() {
            let placeholders = std::iter::repeat_n("?", operation_ids.len()).collect::<Vec<_>>().join(", ");
            let sql = format!(
                r#"
                SELECT id, event_kind, category, status, node_id, operation_id, summary, detail,
                       technical_details_json, created_at
                  FROM ha_control_plane_events
                 WHERE created_at >= ?
                   AND (? IS NULL OR id < ?)
                   AND category = ?
                   AND operation_id IN ({placeholders})
                 ORDER BY id DESC
                 LIMIT ?
                "#
            );
            let mut query = sqlx::query(&sql)
                .bind(threshold)
                .bind(cursor)
                .bind(cursor)
                .bind(HaControlPlaneEventCategory::Edgeone.as_str());
            for operation_id in &operation_ids {
                query = query.bind(operation_id);
            }
            let edgeone_rows = query.bind(direct_limit).fetch_all(&self.pool).await?;
            events.extend(
                edgeone_rows
                    .into_iter()
                    .map(Self::decode_ha_control_plane_event_row)
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }

        let mut deduped = HashMap::new();
        for event in events {
            deduped.entry(event.id).or_insert(event);
        }
        let mut merged = deduped.into_values().collect::<Vec<_>>();
        merged.sort_by(|left, right| {
            right
                .id
                .cmp(&left.id)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        merged.truncate(limit.clamp(1, 200).saturating_add(1) as usize);
        Ok(merged)
    }

    pub(crate) async fn gc_ha_control_plane_events(&self) -> Result<i64, ProxyError> {
        let threshold = self.backend_time.now_ts() - HA_CONTROL_PLANE_EVENT_RETENTION_SECS;
        let result = sqlx::query("DELETE FROM ha_control_plane_events WHERE created_at < ?")
            .bind(threshold)
            .execute(&self.pool)
            .await?;
        Ok(i64::try_from(result.rows_affected()).unwrap_or(i64::MAX))
    }

    fn decode_ha_control_plane_event_row(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<HaControlPlaneEventView, ProxyError> {
        let category_raw: String = row.try_get("category")?;
        let status_raw: String = row.try_get("status")?;
        let technical_details = row
            .try_get::<Option<String>, _>("technical_details_json")?
            .map(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|err| ProxyError::Other(format!("invalid HA timeline details: {err}")))
            })
            .transpose()?;
        Ok(HaControlPlaneEventView {
            id: row.try_get("id")?,
            event_kind: row.try_get("event_kind")?,
            category: HaControlPlaneEventCategory::parse(&category_raw)
                .ok_or_else(|| ProxyError::Other(format!("invalid HA timeline category: {category_raw}")))?,
            status: HaControlPlaneEventStatus::parse(&status_raw)
                .ok_or_else(|| ProxyError::Other(format!("invalid HA timeline status: {status_raw}")))?,
            node_id: row.try_get("node_id")?,
            operation_id: row.try_get("operation_id")?,
            summary: row.try_get("summary")?,
            detail: row.try_get("detail")?,
            technical_details,
            created_at: row.try_get("created_at")?,
        })
    }

    pub(crate) async fn claim_ha_recovery_batch(
        &self,
        batch_id: &str,
        source_node_id: &str,
        event_count: i64,
        checksum: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
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
        .bind(self.backend_time.now_ts())
        .bind(batch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn import_ha_recovery_events(&self) -> Result<i64, ProxyError> {
        Ok(0)
    }

    pub(crate) async fn table_exists(&self, table: &str) -> Result<bool, ProxyError> {
        let sql = if is_observability_table(table) {
            "SELECT EXISTS(SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = ?)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)"
        };
        Ok(sqlx::query_scalar::<_, i64>(sql)
            .bind(table)
            .fetch_one(&self.pool)
            .await?
            != 0)
    }

    async fn fetch_ha_table_json_rows(
        &self,
        channel: HaSyncChannel,
        table: &str,
    ) -> Result<Vec<serde_json::Value>, ProxyError> {
        let columns = self.table_columns(table).await?;
        if columns.is_empty() {
            return Ok(Vec::new());
        }
        let json_args = columns
            .iter()
                .map(|column| ha_export_json_arg(channel, table, None, column))
                .collect::<Vec<_>>()
                .join(", ");
        let sql = ha_baseline_select_sql(table, &json_args);
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

    async fn table_columns_on_conn(
        conn: &mut SqliteConnection,
        table: &str,
    ) -> Result<Vec<String>, ProxyError> {
        let sql = format!("PRAGMA table_info({})", quote_sqlite_identifier(table));
        let rows = sqlx::query(&sql).fetch_all(&mut *conn).await?;
        rows.into_iter()
            .map(|row| row.try_get::<String, _>("name").map_err(ProxyError::from))
            .collect()
    }

    async fn table_columns(&self, table: &str) -> Result<Vec<String>, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        Self::table_columns_on_conn(&mut conn, table).await
    }

    async fn table_exists_on_conn(
        conn: &mut SqliteConnection,
        table: &str,
    ) -> Result<bool, ProxyError> {
        let sql = if is_observability_table(table) {
            "SELECT EXISTS(SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = ?)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)"
        };
        Ok(sqlx::query_scalar::<_, i64>(sql)
            .bind(table)
            .fetch_one(&mut *conn)
            .await?
            != 0)
    }

    async fn count_ha_baseline_rows_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
    ) -> Result<usize, ProxyError> {
        let mut total = 0_i64;
        for table in ha_baseline_tables(channel) {
            if !Self::table_exists_on_conn(conn, table).await? {
                continue;
            }
            let sql = ha_baseline_count_sql(table);
            total += sqlx::query_scalar::<_, i64>(&sql)
                .fetch_one(&mut *conn)
                .await?;
        }
        Ok(total.max(0) as usize)
    }

    async fn validate_ha_events_cursor_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
        after_seq: i64,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let min_seq: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT MIN(seq) FROM {table} WHERE created_at >= ? AND resource IN ({allowed_resources}) AND seq > ?"
        ))
        .bind(threshold)
        .bind(after_seq)
        .fetch_one(&mut *conn)
        .await?;
        // Cursor validity must use the same effective watermark as exports and
        // baselines. sqlite_sequence also includes discarded legacy rows and
        // would make a peer repeatedly reset after the last valid event is GC'd.
        let valid_high_watermark = Self::ha_channel_high_watermark_on_conn(conn, channel).await?;
        if min_seq.is_none() && after_seq > 0 && valid_high_watermark > after_seq {
            return Err(ProxyError::Other(format!(
                "HA {} cursor is older than retention window",
                channel.as_str()
            )));
        }
        if let Some(min_seq) = min_seq
            && after_seq > 0
            && after_seq < min_seq.saturating_sub(1)
        {
            let expired_valid_seq =
                Self::ha_channel_expired_valid_watermark_on_conn(conn, channel).await?;
            let has_expired_valid_gap = expired_valid_seq.is_some_and(|deleted| after_seq < deleted);
            let expired_legacy_seq =
                Self::ha_channel_expired_legacy_watermark_on_conn(conn, channel).await?;
            let has_expired_legacy_gap = expired_legacy_seq
                .is_some_and(|deleted| after_seq < deleted && deleted < min_seq);
            let has_any_bridge: bool = if has_expired_valid_gap || has_expired_legacy_gap {
                false
            } else {
                sqlx::query_scalar(&format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE created_at >= ? AND seq > ? AND seq < ?)"
                ))
                .bind(threshold)
                .bind(after_seq)
                .bind(min_seq)
                .fetch_one(&mut *conn)
                .await?
            };
            if has_expired_valid_gap || (!has_expired_legacy_gap && !has_any_bridge) {
                return Err(ProxyError::Other(format!(
                    "HA {} cursor is older than retention window",
                    channel.as_str()
                )));
            }
        }
        Ok(())
    }

    async fn count_ha_events_after_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
        after_seq: i64,
        limit: i64,
        threshold: i64,
    ) -> Result<usize, ProxyError> {
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let sql = format!(
            r#"
            SELECT seq
              FROM {table}
             WHERE seq > ?
               AND created_at >= ?
               AND resource IN ({allowed_resources})
             ORDER BY seq ASC
             LIMIT ?
            "#
        );
        let rows = sqlx::query(&sql)
            .bind(after_seq.max(0))
            .bind(threshold)
            .bind(limit.clamp(1, 1000))
            .fetch_all(&mut *conn)
            .await?;
        Ok(rows.len())
    }


    pub(crate) async fn configure_ha_event_writes(&self, mode: HaMode) -> Result<(), ProxyError> {
        self.repair_ha_triggers(mode).await.map(|_| ())
    }

    pub(crate) async fn begin_ha_baseline_apply(
        &self,
        channel: HaSyncChannel,
    ) -> Result<HaBaselineApplySession, ProxyError> {
        self.begin_ha_baseline_apply_with_mode(channel, HaBaselineApplyMode::Replace)
            .await
    }

    pub(crate) async fn begin_ha_baseline_apply_with_mode(
        &self,
        channel: HaSyncChannel,
        mode: HaBaselineApplyMode,
    ) -> Result<HaBaselineApplySession, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let mut conn = ImmediateSqliteTransaction::begin(conn).await?;
        let init_result = async {
            insert_ha_outbox_suppression_on_conn(&mut conn).await?;
            if mode == HaBaselineApplyMode::Replace {
                for table in ha_baseline_tables(channel) {
                    if Self::table_exists_on_conn(&mut conn, table).await? {
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
            }
            Ok::<(), ProxyError>(())
        }
        .await;
        if let Err(err) = init_result {
            let _ = conn.rollback_and_reenable_foreign_keys().await;
            return Err(err);
        }
        Ok(HaBaselineApplySession {
            channel,
            conn,
            high_watermark: 0,
            row_count: 0,
            payload_bytes: 0,
            saw_start: false,
            saw_end: false,
            quota_cache_dirty: mode == HaBaselineApplyMode::Replace
                && channel != HaSyncChannel::Billing,
            quota_cache: Arc::clone(&self.account_quota_resolution_cache),
            quota_cache_generation: Arc::clone(&self.account_quota_resolution_generation),
            quota_cache_transitions: Arc::clone(&self.account_quota_resolution_transitions),
        })
    }

    pub(crate) async fn begin_ha_events_apply(
        &self,
        channel: HaSyncChannel,
    ) -> Result<HaEventsApplySession, ProxyError> {
        let conn = self.pool.acquire().await?;
        let mut conn = ImmediateSqliteTransaction::begin(conn).await?;
        if let Err(err) = insert_ha_outbox_suppression_on_conn(&mut conn).await {
            let _ = conn.rollback().await;
            return Err(err);
        }
        Ok(HaEventsApplySession {
            channel,
            conn,
            high_watermark: 0,
            row_count: 0,
            payload_bytes: 0,
            saw_start: false,
            saw_end: false,
            quota_cache_dirty: false,
            quota_cache: Arc::clone(&self.account_quota_resolution_cache),
            quota_cache_generation: Arc::clone(&self.account_quota_resolution_generation),
            quota_cache_transitions: Arc::clone(&self.account_quota_resolution_transitions),
        })
    }

    pub(crate) async fn repair_ha_triggers(
        &self,
        mode: HaMode,
    ) -> Result<HaTriggerRepairReport, ProxyError> {
        let started = Instant::now();
        let mut channels = Vec::new();

        for channel in [
            HaSyncChannel::Control,
            HaSyncChannel::Billing,
            HaSyncChannel::Runtime,
        ] {
            let (legacy_triggers_dropped, current_triggers_dropped) =
                self.drop_ha_channel_triggers(channel).await?;
            let triggers_created = if mode == HaMode::Single {
                0
            } else {
                self.ensure_ha_channel_outbox_triggers(channel).await?
            };
            channels.push(HaTriggerRepairChannelReport {
                channel,
                legacy_triggers_dropped,
                current_triggers_dropped,
                triggers_created,
            });
        }

        Ok(HaTriggerRepairReport {
            mode,
            legacy_triggers_dropped: channels
                .iter()
                .map(|channel| channel.legacy_triggers_dropped)
                .sum(),
            current_triggers_dropped: channels
                .iter()
                .map(|channel| channel.current_triggers_dropped)
                .sum(),
            triggers_created: channels.iter().map(|channel| channel.triggers_created).sum(),
            channels,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub(crate) async fn ensure_ha_channel_outbox_triggers(
        &self,
        channel: HaSyncChannel,
    ) -> Result<i64, ProxyError> {
        let mut conn = self.pool.acquire().await?;
        let mut created = 0_i64;
        for table in ha_channel_event_tables(channel) {
            let table_exists = sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
            )
            .bind(table)
            .fetch_one(&mut *conn)
            .await?
                != 0;
            if !table_exists {
                continue;
            }
            let columns = Self::table_columns_on_conn(&mut conn, table).await?;
            if columns.is_empty() {
                continue;
            }
            let new_json = ha_trigger_json_object(channel, table, "NEW", &columns);
            let old_json = ha_trigger_json_object(channel, table, "OLD", &columns);
            let new_resource_id = ha_trigger_resource_id("NEW", &columns);
            let old_resource_id = ha_trigger_resource_id("OLD", &columns);
            let table_ident = sqlite_qualified_table_name(table);
            let table_lit = quote_sqlite_string(table);
            for (suffix, timing, row_json, resource_id, op) in [
                ("insert", "AFTER INSERT", new_json.as_str(), new_resource_id.as_str(), "upsert"),
                ("update", "AFTER UPDATE", new_json.as_str(), new_resource_id.as_str(), "upsert"),
                ("delete", "AFTER DELETE", old_json.as_str(), old_resource_id.as_str(), "delete"),
            ] {
                let trigger = quote_sqlite_identifier(&ha_trigger_name(
                    channel,
                    table,
                    suffix,
                ));
                let row_alias = if timing == "AFTER DELETE" { "OLD" } else { "NEW" };
                let row_filter = match *table {
                    "meta" => format!(" AND {row_alias}.key IN ({})", ha_meta_key_list_sql()),
                    "billing_ledger" => {
                        format!(" AND {row_alias}.auth_token_log_id > 0")
                    }
                    _ => String::new(),
                };
                let sql = format!(
                    r#"
                    CREATE TRIGGER IF NOT EXISTS {trigger}
                    {timing} ON {table_ident}
                    WHEN NOT EXISTS (SELECT 1 FROM ha_outbox_suppression WHERE id = 'local'){row_filter}
                    BEGIN
                        INSERT INTO {} (
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
                    "#,
                    quote_sqlite_identifier(ha_channel_event_table(channel))
                );
                sqlx::query(&sql).execute(&mut *conn).await?;
                created += 1;
            }
        }
        Ok(created)
    }

    async fn drop_ha_channel_triggers(&self, channel: HaSyncChannel) -> Result<(i64, i64), ProxyError> {
        let mut conn = self.pool.acquire().await?;
        let mut current_trigger_names = std::collections::HashSet::new();
        for table in ha_channel_event_tables(channel) {
            for suffix in ["insert", "update", "delete"] {
                if channel == HaSyncChannel::Control {
                    current_trigger_names.insert(format!("trg_ha_outbox_{}_{}", table, suffix));
                }
                current_trigger_names.insert(ha_trigger_name(channel, table, suffix));
            }
        }
        let mut current_triggers_dropped = 0_i64;
        let mut legacy_triggers_dropped = 0_i64;
        let prefixes = ha_channel_outbox_trigger_prefixes(channel);
        let rows = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' ORDER BY name ASC",
        )
        .fetch_all(&mut *conn)
        .await?;
        for row in rows {
            let name: String = row.try_get("name")?;
            if !prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                continue;
            }
            sqlx::query(&format!(
                "DROP TRIGGER IF EXISTS {}",
                quote_sqlite_identifier(&name)
            ))
            .execute(&mut *conn)
            .await?;
            if current_trigger_names.contains(&name) {
                current_triggers_dropped += 1;
            } else {
                legacy_triggers_dropped += 1;
            }
        }
        Ok((legacy_triggers_dropped, current_triggers_dropped))
    }

}

async fn insert_ha_outbox_suppression_on_conn(
    conn: &mut SqliteConnection,
) -> Result<(), ProxyError> {
    sqlx::query("INSERT OR IGNORE INTO ha_outbox_suppression (id) VALUES ('local')")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn clear_ha_outbox_suppression_on_conn(
    conn: &mut SqliteConnection,
) -> Result<(), ProxyError> {
    sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
        .execute(&mut *conn)
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

fn ha_baseline_select_sql(table: &str, json_args: &str) -> String {
    if table == "meta" {
        return format!(
            "SELECT json_object({json_args}) AS row_json FROM {} WHERE key IN ({}) ORDER BY key ASC",
            quote_sqlite_identifier(table),
            ha_meta_key_list_sql()
        );
    }
    if table == "billing_ledger" {
        return format!(
            "SELECT json_object({json_args}) AS row_json FROM {} WHERE auth_token_log_id > 0 ORDER BY rowid ASC",
            quote_sqlite_identifier(table)
        );
    }
    format!(
        "SELECT json_object({json_args}) AS row_json FROM {} ORDER BY rowid ASC",
        quote_sqlite_identifier(table)
    )
}

fn ha_baseline_count_sql(table: &str) -> String {
    if table == "meta" {
        return format!(
            "SELECT COUNT(*) FROM {} WHERE key IN ({})",
            quote_sqlite_identifier(table),
            ha_meta_key_list_sql()
        );
    }
    if table == "billing_ledger" {
        return format!(
            "SELECT COUNT(*) FROM {} WHERE auth_token_log_id > 0",
            quote_sqlite_identifier(table)
        );
    }
    format!("SELECT COUNT(*) FROM {}", quote_sqlite_identifier(table))
}

fn ensure_ha_resource_whitelisted(
    channel: HaSyncChannel,
    resource: &str,
) -> Result<(), ProxyError> {
    if ha_resource_allowed_for_channel(channel, resource) {
        Ok(())
    } else {
        Err(ProxyError::Other(format!(
            "HA {} resource is not whitelisted: {resource}",
            channel.as_str()
        )))
    }
}

impl HaBaselineReadSession {
    pub async fn export_info(&mut self) -> Result<HaApplyResult, ProxyError> {
        Ok(HaApplyResult {
            channel: self.channel,
            high_watermark: KeyStore::ha_channel_high_watermark_on_conn(&mut self.conn, self.channel)
                .await?,
            row_count: KeyStore::count_ha_baseline_rows_on_conn(&mut self.conn, self.channel).await?,
            payload_bytes: 0,
        })
    }

    // Read/export sessions only hold a snapshot transaction; closing them just releases that snapshot.
    pub async fn close(self) -> Result<(), ProxyError> {
        self.conn.close().await
    }

    pub async fn rollback(self) -> Result<(), ProxyError> {
        self.close().await
    }

    pub async fn write_ndjson<W>(
        &mut self,
        node_id: &str,
        high_watermark: i64,
        row_count: usize,
        writer: &mut W,
    ) -> Result<usize, ProxyError>
    where
        W: AsyncWrite + Unpin + Send,
    {
        let mut payload_bytes = 0usize;
        let start_line = serde_json::to_string(&serde_json::json!({
            "schemaVersion": HA_SCHEMA_VERSION,
            "kind": "baseline_start",
            "channel": self.channel,
            "nodeId": node_id,
            "generatedAt": self.generated_at,
            "highWatermark": high_watermark,
            "encoding": "zstd-ndjson"
        }))
        .map_err(|err| ProxyError::Other(err.to_string()))?;
        payload_bytes += start_line.len();
        writer
            .write_all(start_line.as_bytes())
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;

        for table in ha_baseline_tables(self.channel) {
            if !KeyStore::table_exists_on_conn(&mut self.conn, table).await? {
                continue;
            }
            let columns = KeyStore::table_columns_on_conn(&mut self.conn, table).await?;
            if columns.is_empty() {
                continue;
            }
            let json_args = columns
                .iter()
                .map(|column| {
                    ha_export_json_arg(self.channel, table, None, column)
                })
                .collect::<Vec<_>>()
                .join(", ");
            let sql = ha_baseline_select_sql(table, &json_args);
            let mut rows = sqlx::query_scalar::<_, String>(&sql).fetch(&mut *self.conn);
            while let Some(raw_row) = rows.try_next().await? {
                let row: serde_json::Value = serde_json::from_str(&raw_row)
                    .map_err(|err| ProxyError::Other(format!("invalid HA baseline row: {err}")))?;
                let row = sanitize_ha_resource_payload(table, row);
                let line = serde_json::to_string(&serde_json::json!({
                    "schemaVersion": HA_SCHEMA_VERSION,
                    "kind": "resource",
                    "channel": self.channel,
                    "resource": table,
                    "op": "upsert",
                    "data": row
                }))
                .map_err(|err| ProxyError::Other(err.to_string()))?;
                payload_bytes += line.len();
                writer
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|err| ProxyError::Other(err.to_string()))?;
                writer
                    .write_all(b"\n")
                    .await
                    .map_err(|err| ProxyError::Other(err.to_string()))?;
            }
        }

        let end_line = serde_json::to_string(&serde_json::json!({
            "schemaVersion": HA_SCHEMA_VERSION,
            "kind": "baseline_end",
            "channel": self.channel,
            "nodeId": node_id,
            "highWatermark": high_watermark,
            "rowCount": row_count
        }))
        .map_err(|err| ProxyError::Other(err.to_string()))?;
        payload_bytes += end_line.len();
        writer
            .write_all(end_line.as_bytes())
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        Ok(payload_bytes)
    }
}

impl HaEventsReadSession {
    pub async fn available_event_count(
        &mut self,
        after_seq: i64,
        limit: i64,
    ) -> Result<usize, ProxyError> {
        let threshold = self.generated_at - ha_channel_retention_secs(self.channel);
        KeyStore::validate_ha_events_cursor_on_conn(&mut self.conn, self.channel, after_seq, threshold)
            .await?;
        KeyStore::count_ha_events_after_on_conn(
            &mut self.conn,
            self.channel,
            after_seq,
            limit,
            threshold,
        )
        .await
    }

    pub async fn close(self) -> Result<(), ProxyError> {
        self.conn.close().await
    }

    pub async fn rollback(self) -> Result<(), ProxyError> {
        self.close().await
    }

    pub async fn write_ndjson<W>(
        &mut self,
        after_seq: i64,
        limit: i64,
        event_count: usize,
        writer: &mut W,
    ) -> Result<HaApplyResult, ProxyError>
    where
        W: AsyncWrite + Unpin + Send,
    {
        let mut payload_bytes = 0usize;
        let threshold = self.generated_at - ha_channel_retention_secs(self.channel);
        KeyStore::validate_ha_events_cursor_on_conn(&mut self.conn, self.channel, after_seq, threshold)
            .await?;

        let start_line = serde_json::to_string(&serde_json::json!({
            "schemaVersion": HA_SCHEMA_VERSION,
            "kind": "events_start",
            "channel": self.channel,
            "after": after_seq,
            "limit": limit
        }))
        .map_err(|err| ProxyError::Other(err.to_string()))?;
        payload_bytes += start_line.len();
        writer
            .write_all(start_line.as_bytes())
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;

        let allowed_resources = ha_channel_allowed_resources_sql(self.channel);
        let table = quote_sqlite_identifier(ha_channel_event_table(self.channel));
        let sql = format!(
            r#"
            SELECT seq, kind, resource, resource_id, op, payload_json, created_at, checksum
              FROM {table}
             WHERE seq > ?
               AND created_at >= ?
               AND resource IN ({allowed_resources})
             ORDER BY seq ASC
             LIMIT ?
            "#
        );
        let mut rows = sqlx::query(&sql)
            .bind(after_seq.max(0))
            .bind(threshold)
            .bind(event_count as i64)
            .fetch(&mut *self.conn);
        let mut last_seq = after_seq.max(0);
        let mut row_count = 0usize;
        while let Some(row) = rows.try_next().await? {
            let resource: String = row.try_get("resource")?;
            let payload_raw: String = row.try_get("payload_json")?;
            let payload = serde_json::from_str(&payload_raw)
                .map_err(|err| ProxyError::Other(format!("invalid HA outbox payload: {err}")))?;
            let payload = sanitize_ha_resource_payload(&resource, payload);
            let seq: i64 = row.try_get("seq")?;
            let line = serde_json::to_string(&serde_json::json!({
                "schemaVersion": HA_SCHEMA_VERSION,
                "kind": "event",
                "channel": self.channel,
                "event": {
                    "channel": self.channel,
                    "seq": seq,
                    "kind": row.try_get::<String, _>("kind")?,
                    "resource": resource,
                    "resourceId": row.try_get::<String, _>("resource_id")?,
                    "op": row.try_get::<String, _>("op")?,
                    "payload": payload,
                    "createdAt": row.try_get::<i64, _>("created_at")?,
                    "checksum": row.try_get::<Option<String>, _>("checksum")?
                }
            }))
            .map_err(|err| ProxyError::Other(err.to_string()))?;
            payload_bytes += line.len();
            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|err| ProxyError::Other(err.to_string()))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|err| ProxyError::Other(err.to_string()))?;
            last_seq = seq;
            row_count += 1;
        }

        let end_line = serde_json::to_string(&serde_json::json!({
            "schemaVersion": HA_SCHEMA_VERSION,
            "kind": "events_end",
            "channel": self.channel,
            "lastSeq": last_seq,
            "eventCount": row_count
        }))
        .map_err(|err| ProxyError::Other(err.to_string()))?;
        payload_bytes += end_line.len();
        writer
            .write_all(end_line.as_bytes())
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|err| ProxyError::Other(err.to_string()))?;
        Ok(HaApplyResult {
            channel: self.channel,
            high_watermark: last_seq,
            row_count,
            payload_bytes,
        })
    }
}

fn ha_trigger_name(channel: HaSyncChannel, table: &str, suffix: &str) -> String {
    format!("trg_ha_{}_{}_{}", channel.as_str(), table, suffix)
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

fn ha_column_ref(alias: Option<&str>, column: &str) -> String {
    match alias {
        Some(alias) => format!("{alias}.{}", quote_sqlite_identifier(column)),
        None => quote_sqlite_identifier(column),
    }
}

fn ha_runtime_counter_scope_expr(table: &str, alias: Option<&str>) -> Option<(String, String)> {
    match table {
        "auth_token_quota" => Some((
            ha_column_ref(alias, "token_id"),
            ha_column_ref(alias, "month_start"),
        )),
        "account_monthly_quota" => Some((
            ha_column_ref(alias, "user_id"),
            ha_column_ref(alias, "month_start"),
        )),
        "token_usage_buckets" => {
            let bucket_start = ha_column_ref(alias, "bucket_start");
            let granularity = ha_column_ref(alias, "granularity");
            Some((
                ha_column_ref(alias, "token_id"),
                format!("CAST({granularity} AS TEXT) || ':' || CAST({bucket_start} AS TEXT)"),
            ))
        }
        "account_usage_buckets" => {
            let bucket_start = ha_column_ref(alias, "bucket_start");
            let granularity = ha_column_ref(alias, "granularity");
            Some((
                ha_column_ref(alias, "user_id"),
                format!("CAST({granularity} AS TEXT) || ':' || CAST({bucket_start} AS TEXT)"),
            ))
        }
        _ => None,
    }
}

fn ha_runtime_counter_value_column(table: &str) -> Option<&'static str> {
    match table {
        "auth_token_quota" | "account_monthly_quota" => Some("month_count"),
        "token_usage_buckets" | "account_usage_buckets" => Some("count"),
        _ => None,
    }
}

fn ha_runtime_counter_local_value_expr(
    channel: HaSyncChannel,
    table: &str,
    alias: Option<&str>,
    column: &str,
) -> Option<String> {
    if channel != HaSyncChannel::Runtime || ha_runtime_counter_value_column(table) != Some(column) {
        return None;
    }
    let (resource_id_expr, counter_scope_expr) = ha_runtime_counter_scope_expr(table, alias)?;
    let value_expr = ha_column_ref(alias, column);
    let imported_total_expr = format!(
        "COALESCE((SELECT SUM(counter_value) FROM ha_runtime_counter_imports \
         WHERE resource = {} \
           AND resource_id = CAST({resource_id_expr} AS TEXT) \
           AND counter_scope = CAST({counter_scope_expr} AS TEXT)), 0)",
        quote_sqlite_string(table)
    );
    Some(format!(
        "MAX(0, COALESCE({value_expr}, 0) - {imported_total_expr})"
    ))
}

fn ha_export_json_arg(
    channel: HaSyncChannel,
    table: &str,
    alias: Option<&str>,
    column: &str,
) -> String {
    let value_expr = ha_runtime_counter_local_value_expr(channel, table, alias, column)
        .unwrap_or_else(|| ha_column_ref(alias, column));
    format!("{}, {value_expr}", quote_sqlite_string(column))
}

fn ha_trigger_json_object(
    channel: HaSyncChannel,
    table: &str,
    alias: &str,
    columns: &[String],
) -> String {
    let args = columns
        .iter()
        .map(|column| ha_export_json_arg(channel, table, Some(alias), column))
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
    } else if columns.iter().any(|column| column == "auth_token_log_id") {
        format!(
            "COALESCE(CAST({alias}.{} AS TEXT), CAST({alias}.rowid AS TEXT))",
            quote_sqlite_identifier("auth_token_log_id")
        )
    } else {
        format!("CAST({alias}.rowid AS TEXT)")
    }
}

async fn lookup_peer_billing_ledger_local_id_on_conn(
    conn: &mut SqliteConnection,
    peer_node_id: &str,
    peer_auth_token_log_id: i64,
) -> Result<Option<i64>, ProxyError> {
    sqlx::query_scalar(
        r#"
        SELECT local_auth_token_log_id
        FROM ha_billing_ledger_imports
        WHERE peer_node_id = ? AND peer_auth_token_log_id = ?
        LIMIT 1
        "#,
    )
    .bind(peer_node_id)
    .bind(peer_auth_token_log_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(ProxyError::from)
}

async fn get_or_create_peer_billing_ledger_local_id_on_conn(
    conn: &mut SqliteConnection,
    peer_node_id: &str,
    peer_auth_token_log_id: i64,
) -> Result<i64, ProxyError> {
    if let Some(local_id) =
        lookup_peer_billing_ledger_local_id_on_conn(conn, peer_node_id, peer_auth_token_log_id)
            .await?
    {
        sqlx::query(
            r#"
            UPDATE ha_billing_ledger_imports
            SET updated_at = CAST(strftime('%s','now') AS INTEGER)
            WHERE peer_node_id = ? AND peer_auth_token_log_id = ?
            "#,
        )
        .bind(peer_node_id)
        .bind(peer_auth_token_log_id)
        .execute(&mut *conn)
        .await?;
        return Ok(local_id);
    }

    let min_ledger_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MIN(auth_token_log_id) FROM billing_ledger WHERE auth_token_log_id < 0",
    )
    .fetch_one(&mut *conn)
    .await?
    .unwrap_or(0);
    let min_mapping_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MIN(local_auth_token_log_id) FROM ha_billing_ledger_imports",
    )
    .fetch_one(&mut *conn)
    .await?
    .unwrap_or(0);
    let min_existing_id = min_ledger_id.min(min_mapping_id).min(0);
    if min_existing_id == i64::MIN {
        return Err(ProxyError::Other(
            "HA billing ledger import id namespace exhausted".to_string(),
        ));
    }
    let local_id = min_existing_id - 1;
    sqlx::query(
        r#"
        INSERT INTO ha_billing_ledger_imports (
            peer_node_id, peer_auth_token_log_id, local_auth_token_log_id, updated_at
        )
        VALUES (?, ?, ?, CAST(strftime('%s','now') AS INTEGER))
        "#,
    )
    .bind(peer_node_id)
    .bind(peer_auth_token_log_id)
    .bind(local_id)
    .execute(&mut *conn)
    .await?;
    Ok(local_id)
}

async fn prepare_peer_billing_ledger_row_on_conn(
    conn: &mut SqliteConnection,
    peer_node_id: &str,
    op: &str,
    row: serde_json::Value,
) -> Result<Option<serde_json::Value>, ProxyError> {
    let Some(object) = row.as_object() else {
        return Err(ProxyError::Other(
            "HA billing ledger payload must be a JSON object".to_string(),
        ));
    };
    let peer_auth_token_log_id =
        ha_json_i64_field(object, "billing_ledger", "auth_token_log_id")?;
    if peer_auth_token_log_id <= 0 {
        return Ok(None);
    }
    let local_auth_token_log_id = match op {
        "upsert" => {
            get_or_create_peer_billing_ledger_local_id_on_conn(
                conn,
                peer_node_id,
                peer_auth_token_log_id,
            )
            .await?
        }
        "delete" => {
            let Some(local_id) = lookup_peer_billing_ledger_local_id_on_conn(
                conn,
                peer_node_id,
                peer_auth_token_log_id,
            )
            .await?
            else {
                return Ok(None);
            };
            local_id
        }
        _ => return Ok(Some(row)),
    };

    let mut object = object.clone();
    object.insert(
        "auth_token_log_id".to_string(),
        serde_json::json!(local_auth_token_log_id),
    );
    if object.contains_key("request_log_id") {
        object.insert("request_log_id".to_string(), serde_json::Value::Null);
    }
    Ok(Some(serde_json::Value::Object(object)))
}

async fn prepare_peer_import_row_on_conn(
    conn: &mut SqliteConnection,
    peer_node_id: Option<&str>,
    channel: HaSyncChannel,
    table: &str,
    op: &str,
    row: serde_json::Value,
) -> Result<Option<serde_json::Value>, ProxyError> {
    if channel == HaSyncChannel::Billing
        && table == "billing_ledger"
        && let Some(peer_node_id) = peer_node_id
    {
        return prepare_peer_billing_ledger_row_on_conn(conn, peer_node_id, op, row).await;
    }
    Ok(Some(row))
}

async fn insert_json_row_on_conn(
    conn: &mut SqliteConnection,
    channel: HaSyncChannel,
    table: &str,
    row: &serde_json::Value,
    peer_import_node_id: Option<&str>,
) -> Result<(), ProxyError> {
    ensure_ha_resource_whitelisted(channel, table)?;
    let Some(row) = prepare_peer_import_row_on_conn(
        conn,
        peer_import_node_id,
        channel,
        table,
        "upsert",
        row.clone(),
    )
    .await?
    else {
        return Ok(());
    };
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
    query.execute(&mut *conn).await?;
    Ok(())
}

async fn delete_json_row_on_conn(
    conn: &mut SqliteConnection,
    channel: HaSyncChannel,
    table: &str,
    resource_id: &str,
    row: &serde_json::Value,
    peer_import_node_id: Option<&str>,
) -> Result<(), ProxyError> {
    ensure_ha_resource_whitelisted(channel, table)?;
    let Some(row) = prepare_peer_import_row_on_conn(
        conn,
        peer_import_node_id,
        channel,
        table,
        "delete",
        row.clone(),
    )
    .await?
    else {
        return Ok(());
    };
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
        query.execute(&mut *conn).await?;
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
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[derive(Debug)]
struct SqliteColumnInfo {
    name: String,
    pk: i64,
}

async fn table_column_info_on_conn(
    conn: &mut SqliteConnection,
    table: &str,
) -> Result<Vec<SqliteColumnInfo>, ProxyError> {
    let rows = sqlx::query(&format!("PRAGMA table_info({})", quote_sqlite_identifier(table)))
        .fetch_all(&mut *conn)
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

#[derive(Debug)]
enum HaRuntimeCounterFields {
    AuthTokenQuota {
        token_id: String,
        month_start: i64,
    },
    AccountMonthlyQuota {
        user_id: String,
        month_start: i64,
    },
    TokenUsageBucket {
        token_id: String,
        bucket_start: i64,
        granularity: String,
    },
    AccountUsageBucket {
        user_id: String,
        bucket_start: i64,
        granularity: String,
    },
}

#[derive(Debug)]
struct HaRuntimeCounterImport {
    resource_id: String,
    counter_scope: String,
    counter_value: i64,
    fields: HaRuntimeCounterFields,
}

fn ha_json_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    table: &str,
    field: &str,
) -> Result<String, ProxyError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ProxyError::Other(format!("HA counter payload missing {field} for {table}")))
}

fn ha_json_i64_field(
    object: &serde_json::Map<String, serde_json::Value>,
    table: &str,
    field: &str,
) -> Result<i64, ProxyError> {
    let Some(value) = object.get(field) else {
        return Err(ProxyError::Other(format!(
            "HA counter payload missing {field} for {table}"
        )));
    };
    let parsed = value.as_i64().or_else(|| {
        value
            .as_u64()
            .and_then(|value| i64::try_from(value).ok())
    }).or_else(|| value.as_str().and_then(|value| value.trim().parse::<i64>().ok()));
    parsed.ok_or_else(|| {
        ProxyError::Other(format!(
            "HA counter payload has invalid {field} for {table}"
        ))
    })
}

fn ha_runtime_counter_import_for_row(
    table: &str,
    row: &serde_json::Value,
) -> Result<Option<HaRuntimeCounterImport>, ProxyError> {
    let Some(object) = row.as_object() else {
        return Ok(None);
    };
    let counter = match table {
        "auth_token_quota" => {
            let token_id = ha_json_string_field(object, table, "token_id")?;
            let month_start = ha_json_i64_field(object, table, "month_start")?;
            let counter_value = ha_json_i64_field(object, table, "month_count")?;
            HaRuntimeCounterImport {
                resource_id: token_id.clone(),
                counter_scope: month_start.to_string(),
                counter_value,
                fields: HaRuntimeCounterFields::AuthTokenQuota {
                    token_id,
                    month_start,
                },
            }
        }
        "account_monthly_quota" => {
            let user_id = ha_json_string_field(object, table, "user_id")?;
            let month_start = ha_json_i64_field(object, table, "month_start")?;
            let counter_value = ha_json_i64_field(object, table, "month_count")?;
            HaRuntimeCounterImport {
                resource_id: user_id.clone(),
                counter_scope: month_start.to_string(),
                counter_value,
                fields: HaRuntimeCounterFields::AccountMonthlyQuota {
                    user_id,
                    month_start,
                },
            }
        }
        "token_usage_buckets" => {
            let token_id = ha_json_string_field(object, table, "token_id")?;
            let bucket_start = ha_json_i64_field(object, table, "bucket_start")?;
            let granularity = ha_json_string_field(object, table, "granularity")?;
            let counter_value = ha_json_i64_field(object, table, "count")?;
            HaRuntimeCounterImport {
                resource_id: token_id.clone(),
                counter_scope: format!("{granularity}:{bucket_start}"),
                counter_value,
                fields: HaRuntimeCounterFields::TokenUsageBucket {
                    token_id,
                    bucket_start,
                    granularity,
                },
            }
        }
        "account_usage_buckets" => {
            let user_id = ha_json_string_field(object, table, "user_id")?;
            let bucket_start = ha_json_i64_field(object, table, "bucket_start")?;
            let granularity = ha_json_string_field(object, table, "granularity")?;
            let counter_value = ha_json_i64_field(object, table, "count")?;
            HaRuntimeCounterImport {
                resource_id: user_id.clone(),
                counter_scope: format!("{granularity}:{bucket_start}"),
                counter_value,
                fields: HaRuntimeCounterFields::AccountUsageBucket {
                    user_id,
                    bucket_start,
                    granularity,
                },
            }
        }
        _ => return Ok(None),
    };
    if counter.counter_value < 0 {
        return Err(ProxyError::Other(format!(
            "HA counter payload has negative count for {table}"
        )));
    }
    Ok(Some(counter))
}

async fn merge_peer_runtime_counter_on_conn(
    conn: &mut SqliteConnection,
    peer_node_id: &str,
    channel: HaSyncChannel,
    table: &str,
    op: &str,
    row: &serde_json::Value,
) -> Result<bool, ProxyError> {
    if channel != HaSyncChannel::Runtime {
        return Ok(false);
    }
    let Some(counter) = ha_runtime_counter_import_for_row(table, row)? else {
        return Ok(false);
    };
    if op == "delete" {
        return Ok(true);
    }
    if op != "upsert" {
        return Ok(false);
    }
    let previous = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT counter_value
        FROM ha_runtime_counter_imports
        WHERE peer_node_id = ? AND resource = ? AND resource_id = ? AND counter_scope = ?
        LIMIT 1
        "#,
    )
    .bind(peer_node_id)
    .bind(table)
    .bind(&counter.resource_id)
    .bind(&counter.counter_scope)
    .fetch_optional(&mut *conn)
    .await?
    .unwrap_or(0);
    let delta = counter.counter_value.saturating_sub(previous).max(0);
    let shadow_value = previous.max(counter.counter_value);
    sqlx::query(
        r#"
        INSERT INTO ha_runtime_counter_imports (
            peer_node_id, resource, resource_id, counter_scope, counter_value, updated_at
        )
        VALUES (?, ?, ?, ?, ?, CAST(strftime('%s','now') AS INTEGER))
        ON CONFLICT(peer_node_id, resource, resource_id, counter_scope) DO UPDATE SET
            counter_value = excluded.counter_value,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(peer_node_id)
    .bind(table)
    .bind(&counter.resource_id)
    .bind(&counter.counter_scope)
    .bind(shadow_value)
    .execute(&mut *conn)
    .await?;
    if delta <= 0 {
        return Ok(true);
    }
    match counter.fields {
        HaRuntimeCounterFields::AuthTokenQuota {
            token_id,
            month_start,
        } => {
            sqlx::query(
                r#"
                INSERT INTO auth_token_quota (token_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(token_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                        ELSE auth_token_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                        ELSE auth_token_quota.month_count + excluded.month_count
                    END
                "#,
            )
            .bind(token_id)
            .bind(month_start)
            .bind(delta)
            .execute(&mut *conn)
            .await?;
        }
        HaRuntimeCounterFields::AccountMonthlyQuota {
            user_id,
            month_start,
        } => {
            sqlx::query(
                r#"
                INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                        ELSE account_monthly_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                        ELSE account_monthly_quota.month_count + excluded.month_count
                    END
                "#,
            )
            .bind(user_id)
            .bind(month_start)
            .bind(delta)
            .execute(&mut *conn)
            .await?;
        }
        HaRuntimeCounterFields::TokenUsageBucket {
            token_id,
            bucket_start,
            granularity,
        } => {
            sqlx::query(
                r#"
                INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(token_id, bucket_start, granularity) DO UPDATE SET
                    count = token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(bucket_start)
            .bind(granularity)
            .bind(delta)
            .execute(&mut *conn)
            .await?;
        }
        HaRuntimeCounterFields::AccountUsageBucket {
            user_id,
            bucket_start,
            granularity,
        } => {
            sqlx::query(
                r#"
                INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id, bucket_start, granularity) DO UPDATE SET
                    count = account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(bucket_start)
            .bind(granularity)
            .bind(delta)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(true)
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

impl HaBaselineApplySession {
    pub async fn apply_line(&mut self, line: &str) -> Result<(), ProxyError> {
        self.apply_line_inner(line, None).await
    }

    pub async fn apply_line_with_peer_import(
        &mut self,
        line: &str,
        peer_node_id: &str,
    ) -> Result<(), ProxyError> {
        self.apply_line_inner(line, Some(peer_node_id)).await
    }

    async fn apply_line_inner(
        &mut self,
        line: &str,
        peer_import_node_id: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.payload_bytes += line.len();
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| ProxyError::Other(format!("invalid HA baseline NDJSON: {err}")))?;
        match value.get("kind").and_then(serde_json::Value::as_str) {
            Some("baseline_start") => {
                self.saw_start = true;
                self.high_watermark = value
                    .get("highWatermark")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0)
                    .max(0);
            }
            Some("resource") => {
                let resource = value
                    .get("resource")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| ProxyError::Other("HA baseline resource is missing".to_string()))?;
                if ha_resource_retired_for_channel(self.channel, resource) {
                    return Ok(());
                }
                ensure_ha_resource_whitelisted(self.channel, resource)?;
                self.quota_cache_dirty |= ha_resource_affects_account_quota(resource);
                let data = value
                    .get("data")
                    .cloned()
                    .ok_or_else(|| ProxyError::Other("HA baseline resource data is missing".to_string()))?;
                let data = sanitize_ha_resource_payload(resource, data);
                self.payload_bytes += serde_json::to_vec(&data)
                    .map(|bytes| bytes.len())
                    .unwrap_or_default();
                let merged_counter =
                    if let Some(peer_node_id) = peer_import_node_id {
                        merge_peer_runtime_counter_on_conn(
                            &mut self.conn,
                            peer_node_id,
                            self.channel,
                            resource,
                            "upsert",
                            &data,
                        )
                        .await?
                    } else {
                        false
                    };
                if !merged_counter {
                    insert_json_row_on_conn(
                        &mut self.conn,
                        self.channel,
                        resource,
                        &data,
                        peer_import_node_id,
                    )
                    .await?;
                }
                self.row_count += 1;
            }
            Some("baseline_end") => {
                self.saw_end = true;
                self.high_watermark = value
                    .get("highWatermark")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(self.high_watermark)
                    .max(self.high_watermark);
            }
            other => {
                return Err(ProxyError::Other(format!(
                    "unsupported HA baseline record kind: {other:?}"
                )));
            }
        }
        Ok(())
    }

    pub async fn finish(mut self) -> Result<HaApplyResult, ProxyError> {
        if !self.saw_start || !self.saw_end {
            let _ = self.conn.rollback_and_reenable_foreign_keys().await;
            return Err(ProxyError::Other(
                "HA baseline must include baseline_start and baseline_end".to_string(),
            ));
        }
        if let Err(err) = clear_ha_outbox_suppression_on_conn(&mut self.conn).await {
            let _ = self.conn.rollback_and_reenable_foreign_keys().await;
            return Err(err);
        }
        if self.quota_cache_dirty {
            self.quota_cache_transitions
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.quota_cache_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        let mut conn = self.conn.commit_connection().await?;
        if self.quota_cache_dirty {
            self.quota_cache_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.quota_cache.write().await.clear();
            self.quota_cache_transitions
                .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
        if let Err(err) = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
        {
            conn.detach().close().await.ok();
            return Err(ProxyError::Database(err));
        }
        Ok(HaApplyResult {
            channel: self.channel,
            high_watermark: self.high_watermark,
            row_count: self.row_count,
            payload_bytes: self.payload_bytes,
        })
    }

    pub async fn abort(self) -> Result<(), ProxyError> {
        self.conn.rollback_and_reenable_foreign_keys().await
    }
}

impl HaEventsApplySession {
    pub async fn apply_line(&mut self, line: &str) -> Result<(), ProxyError> {
        self.apply_line_inner(line, None).await
    }

    pub async fn apply_line_with_peer_import(
        &mut self,
        line: &str,
        peer_node_id: &str,
    ) -> Result<(), ProxyError> {
        self.apply_line_inner(line, Some(peer_node_id)).await
    }

    async fn apply_line_inner(
        &mut self,
        line: &str,
        peer_import_node_id: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.payload_bytes += line.len();
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| ProxyError::Other(format!("invalid HA events NDJSON: {err}")))?;
        match value.get("kind").and_then(serde_json::Value::as_str) {
            Some("events_start") => {
                self.saw_start = true;
            }
            Some("event") => {
                let event = value
                    .get("event")
                    .ok_or_else(|| ProxyError::Other("HA event wrapper is missing event".to_string()))?;
                let resource = event
                    .get("resource")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| ProxyError::Other("HA event resource is missing".to_string()))?;
                self.high_watermark = event
                    .get("seq")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(self.high_watermark)
                    .max(self.high_watermark);
                if ha_resource_retired_for_channel(self.channel, resource) {
                    return Ok(());
                }
                ensure_ha_resource_whitelisted(self.channel, resource)?;
                self.quota_cache_dirty |= ha_resource_affects_account_quota(resource);
                let op = event
                    .get("op")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("upsert");
                let resource_id = event
                    .get("resourceId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let payload = sanitize_ha_resource_payload(
                    resource,
                    event
                        .get("payload")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                );
                self.payload_bytes += serde_json::to_vec(&payload)
                    .map(|bytes| bytes.len())
                    .unwrap_or_default();
                let merged_counter =
                    if let Some(peer_node_id) = peer_import_node_id {
                        merge_peer_runtime_counter_on_conn(
                            &mut self.conn,
                            peer_node_id,
                            self.channel,
                            resource,
                            op,
                            &payload,
                        )
                        .await?
                    } else {
                        false
                    };
                if !merged_counter {
                    match op {
                        "delete" => {
                            delete_json_row_on_conn(
                                &mut self.conn,
                                self.channel,
                                resource,
                                resource_id,
                                &payload,
                                peer_import_node_id,
                            )
                            .await?
                        }
                        "upsert" => {
                            insert_json_row_on_conn(
                                &mut self.conn,
                                self.channel,
                                resource,
                                &payload,
                                peer_import_node_id,
                            )
                            .await?
                        }
                        other => {
                            return Err(ProxyError::Other(format!(
                                "unsupported HA event operation: {other}"
                            )));
                        }
                    }
                }
                self.row_count += 1;
            }
            Some("events_end") => {
                self.saw_end = true;
                self.high_watermark = value
                    .get("lastSeq")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(self.high_watermark)
                    .max(self.high_watermark);
            }
            other => {
                return Err(ProxyError::Other(format!(
                    "unsupported HA events record kind: {other:?}"
                )));
            }
        }
        Ok(())
    }

    pub async fn finish(mut self) -> Result<HaApplyResult, ProxyError> {
        if !self.saw_start || !self.saw_end {
            let _ = self.conn.rollback().await;
            return Err(ProxyError::Other(
                "HA events must include events_start and events_end".to_string(),
            ));
        }
        if let Err(err) = clear_ha_outbox_suppression_on_conn(&mut self.conn).await {
            let _ = self.conn.rollback().await;
            return Err(err);
        }
        if self.quota_cache_dirty {
            self.quota_cache_transitions
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.quota_cache_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        self.conn.commit().await?;
        if self.quota_cache_dirty {
            self.quota_cache_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.quota_cache.write().await.clear();
            self.quota_cache_transitions
                .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
        Ok(HaApplyResult {
            channel: self.channel,
            high_watermark: self.high_watermark,
            row_count: self.row_count,
            payload_bytes: self.payload_bytes,
        })
    }

    pub async fn abort(self) -> Result<(), ProxyError> {
        self.conn.rollback().await
    }
}

impl KeyStore {
    async fn max_ha_expired_valid_seq(
        &self,
        channel: HaSyncChannel,
        threshold: i64,
        batch_size: i64,
    ) -> Result<Option<i64>, ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        Ok(sqlx::query_scalar(&format!(
            "SELECT MAX(seq) FROM (SELECT seq FROM {table} WHERE created_at < ? AND resource IN ({allowed_resources}) ORDER BY seq ASC LIMIT ?)"
        ))
        .bind(threshold)
        .bind(batch_size.max(1))
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn delete_ha_channel_events_bounded(
        &self,
        channel: HaSyncChannel,
        threshold: i64,
        batch_size: i64,
    ) -> Result<i64, ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let deleted = sqlx::query(&format!(
            r#"
            DELETE FROM {table}
            WHERE seq IN (
                SELECT seq
                FROM {table}
                WHERE created_at < ?
                  AND resource IN ({allowed_resources})
                ORDER BY seq ASC
                LIMIT ?
            )
            "#
        ))
        .bind(threshold)
        .bind(batch_size.max(1))
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected() as i64)
    }

    pub(crate) async fn delete_ha_invalid_legacy_events_bounded(
        &self,
        channel: HaSyncChannel,
        batch_size: i64,
    ) -> Result<i64, ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_outbox_gc_allowed_resources(channel)
            .iter()
            .map(|resource| quote_sqlite_string(resource))
            .collect::<Vec<_>>()
            .join(", ");
        let where_sql = if allowed_resources.is_empty() {
            "1=1".to_string()
        } else {
            format!("resource NOT IN ({allowed_resources})")
        };
        let max_deleted_seq: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT MAX(seq) FROM (SELECT seq FROM {table} WHERE {where_sql} ORDER BY seq ASC LIMIT ?)"
        ))
        .bind(batch_size.max(1))
        .fetch_one(&self.pool)
        .await?;
        let deleted = sqlx::query(&format!(
            r#"
            DELETE FROM {table}
            WHERE seq IN (
                SELECT seq
                FROM {table}
                WHERE {where_sql}
                ORDER BY seq ASC
                LIMIT ?
            )
            "#
        ))
        .bind(batch_size.max(1))
        .execute(&self.pool)
        .await?;
        if deleted.rows_affected() > 0
            && let Some(max_deleted_seq) = max_deleted_seq
        {
            let mut conn = self.pool.acquire().await?;
            Self::remember_ha_channel_expired_legacy_watermark_on_conn(
                &mut conn,
                channel,
                max_deleted_seq,
                self.backend_time.now_ts(),
            )
            .await?;
        }
        Ok(deleted.rows_affected() as i64)
    }

    pub(crate) async fn ha_invalid_legacy_events_exist(
        &self,
        channel: HaSyncChannel,
    ) -> Result<bool, ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_outbox_gc_allowed_resources(channel)
            .iter()
            .map(|resource| quote_sqlite_string(resource))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = if allowed_resources.is_empty() {
            format!("SELECT EXISTS(SELECT 1 FROM {table} LIMIT 1)")
        } else {
            format!(
                "SELECT EXISTS(SELECT 1 FROM {table} WHERE resource NOT IN ({allowed_resources}) LIMIT 1)"
            )
        };
        Ok(sqlx::query_scalar::<_, bool>(&sql)
            .fetch_one(&self.pool)
            .await?)
    }

    pub(crate) async fn gc_ha_outbox_with_options(
        &self,
        options: HaOutboxGcOptions,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        let started = Instant::now();
        let batch_size = options.batch_size.max(1);
        let max_batches = options.max_batches.max(1);
        let deadline = started + Duration::from_secs(options.max_runtime_secs.max(1));
        let mut deleted_rows = 0_i64;
        let mut batches = 0_i64;
        let mut channels = Vec::new();
        let mut active_elapsed_ms = 0_u128;
        let mut max_batch_elapsed_ms = 0_u128;

        for channel in [
            HaSyncChannel::Control,
            HaSyncChannel::Billing,
            HaSyncChannel::Runtime,
        ] {
            let mut watermark_conn = self.pool.acquire().await?;
            Self::remember_ha_channel_valid_watermark_on_conn(
                &mut watermark_conn,
                channel,
                self.backend_time.now_ts(),
            )
            .await?;
            let retention_secs = ha_channel_retention_secs(channel);
            let threshold = self.backend_time.now_ts() - retention_secs;
            let mut channel_deleted_rows = 0_i64;
            let mut invalid_legacy_deleted_rows = 0_i64;
            let mut retention_deleted_rows = 0_i64;
            let mut channel_batches = 0_i64;

            while channel_batches < max_batches && Instant::now() < deadline {
                let batch_started = Instant::now();
                let deleted_invalid = self
                    .delete_ha_invalid_legacy_events_bounded(channel, batch_size)
                    .await?;
                invalid_legacy_deleted_rows += deleted_invalid;
                channel_deleted_rows += deleted_invalid;
                channel_batches += 1;
                if deleted_invalid > 0 {
                    if deleted_invalid < batch_size {
                        record_ha_outbox_gc_batch_timing(
                            &mut active_elapsed_ms,
                            &mut max_batch_elapsed_ms,
                            batch_started,
                        );
                        continue;
                    }
                } else {
                    let max_deleted_valid_seq = self
                        .max_ha_expired_valid_seq(channel, threshold, batch_size)
                        .await?;
                    let deleted_retention = self
                        .delete_ha_channel_events_bounded(channel, threshold, batch_size)
                        .await?;
                    if deleted_retention > 0
                        && let Some(max_deleted_valid_seq) = max_deleted_valid_seq
                    {
                        let mut marker_conn = self.pool.acquire().await?;
                        Self::remember_ha_channel_expired_valid_watermark_on_conn(
                            &mut marker_conn,
                            channel,
                            max_deleted_valid_seq,
                            self.backend_time.now_ts(),
                        )
                        .await?;
                    }
                    retention_deleted_rows += deleted_retention;
                    channel_deleted_rows += deleted_retention;
                    if deleted_retention < batch_size {
                        record_ha_outbox_gc_batch_timing(
                            &mut active_elapsed_ms,
                            &mut max_batch_elapsed_ms,
                            batch_started,
                        );
                        break;
                    }
                }
                record_ha_outbox_gc_batch_timing(
                    &mut active_elapsed_ms,
                    &mut max_batch_elapsed_ms,
                    batch_started,
                );
                if options.inter_batch_sleep_ms > 0 {
                    self.backend_time
                        .sleep(Duration::from_millis(options.inter_batch_sleep_ms))
                        .await;
                }
            }

            let has_more_retention: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE created_at < ? LIMIT 1)",
                quote_sqlite_identifier(ha_channel_event_table(channel))
            ))
            .bind(threshold)
            .fetch_one(&self.pool)
            .await?;
            let has_more_invalid = self.ha_invalid_legacy_events_exist(channel).await?;
            let has_more = has_more_invalid || has_more_retention;
            let oldest_created_at: Option<i64> = sqlx::query_scalar(&format!(
                "SELECT MIN(created_at) FROM {} WHERE created_at < ?",
                quote_sqlite_identifier(ha_channel_event_table(channel))
            ))
            .bind(threshold)
            .fetch_one(&self.pool)
            .await?;
            let observed_at = self.backend_time.now_ts();
            deleted_rows += channel_deleted_rows;
            batches += channel_batches;
            channels.push(HaOutboxGcChannelReport {
                channel,
                retention_secs,
                threshold,
                invalid_legacy_deleted_rows,
                retention_deleted_rows,
                deleted_rows: channel_deleted_rows,
                batches: channel_batches,
                has_more,
                debt_mode: "offline".to_string(),
                oldest_deletable_age_secs: oldest_created_at
                    .map(|created_at| observed_at.saturating_sub(created_at).max(0)),
                deleted_rows_per_minute: if started.elapsed().as_secs() > 0 {
                    channel_deleted_rows as f64 * 60.0 / started.elapsed().as_secs() as f64
                } else {
                    0.0
                },
            recovery_deadline_at: None,
            slo_state: "not_applicable".to_string(),
            slo_state_transition: None,
            foreground_rps: 0,
                observed_at,
            });
        }

        let (busy, log_frames, checkpointed_frames) = if deleted_rows > 0 {
            let (busy, log_frames, checkpointed_frames) =
                self.checkpoint_sqlite_wal_passive().await?;
            (busy != 0, log_frames, checkpointed_frames)
        } else {
            (false, 0, 0)
        };

        let completed = !channels.iter().any(|channel| channel.has_more);
        let elapsed_ms = started.elapsed().as_millis();
        Ok(HaOutboxGcReport {
            batch_size,
            max_batches,
            deleted_rows,
            batches,
            completed,
            has_more: channels.iter().any(|channel| channel.has_more),
            channels,
            wal_checkpoint_busy: busy,
            wal_checkpoint_log_frames: log_frames,
            wal_checkpoint_checkpointed_frames: checkpointed_frames,
            active_elapsed_ms,
            max_batch_elapsed_ms,
            elapsed_ms,
            continuation_delay_secs: None,
        })
    }

    async fn delete_ha_channel_events_returning_max_seq_on_conn(
        conn: &mut sqlx::SqliteConnection,
        channel: HaSyncChannel,
        threshold: i64,
        batch_size: i64,
    ) -> Result<(i64, Option<i64>), ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let deleted_seqs: Vec<i64> = sqlx::query_scalar(&format!(
            "DELETE FROM {table} WHERE seq IN (SELECT seq FROM {table} WHERE created_at < ? AND resource IN ({allowed_resources}) ORDER BY created_at ASC, seq ASC LIMIT ?) RETURNING seq"
        ))
        .bind(threshold)
        .bind(batch_size.max(1))
        .fetch_all(&mut *conn)
        .await?;
        let max_deleted_seq = deleted_seqs.iter().copied().max();
        Ok((deleted_seqs.len() as i64, max_deleted_seq))
    }

    async fn delete_ha_invalid_legacy_events_bounded_on_conn(
        conn: &mut sqlx::SqliteConnection,
        channel: HaSyncChannel,
        batch_size: i64,
        updated_at: i64,
    ) -> Result<(i64, bool, bool), ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let last_seq: i64 = sqlx::query_scalar(
            "SELECT legacy_cursor_seq FROM ha_outbox_gc_channel_state WHERE channel = ?",
        )
        .bind(channel.as_str())
        .fetch_one(&mut *conn)
        .await?;
        let scanned: Vec<(i64, String)> = sqlx::query_as(&format!(
            "SELECT seq, resource FROM {table} WHERE seq > ? ORDER BY seq ASC LIMIT ?"
        ))
        .bind(last_seq)
        .bind(batch_size.max(1))
        .fetch_all(&mut *conn)
        .await?;
        let scanned_len = scanned.len() as i64;
        let next_seq = scanned.last().map(|(seq, _)| *seq).unwrap_or(last_seq);
        let allowed_resources = ha_outbox_gc_allowed_resources(channel);
        let invalid_seqs = scanned
            .iter()
            .filter_map(|(seq, resource)| (!allowed_resources.contains(&resource.as_str())).then_some(*seq))
            .collect::<Vec<_>>();
        if !invalid_seqs.is_empty() {
            let placeholders = std::iter::repeat_n("?", invalid_seqs.len())
                .collect::<Vec<_>>()
                .join(", ");
            let delete_sql = format!("DELETE FROM {table} WHERE seq IN ({placeholders})");
            let mut query = sqlx::query(&delete_sql);
            for seq in &invalid_seqs {
                query = query.bind(seq);
            }
            query.execute(&mut *conn).await?;
        }
        let reached_end = if scanned_len < batch_size.max(1) {
            true
        } else {
            !sqlx::query_scalar::<_, bool>(&format!(
                "SELECT EXISTS(SELECT 1 FROM {table} WHERE seq > ? LIMIT 1)"
            ))
            .bind(next_seq)
            .fetch_one(&mut *conn)
            .await?
        };
        // Keep the range cursor at the last scanned primary key even at the
        // end of the table. Resetting it to zero makes every online slice
        // rescan the entire outbox and defeats the bounded legacy pass.
        sqlx::query(
            "UPDATE ha_outbox_gc_channel_state SET legacy_cursor_seq = ? WHERE channel = ?",
        )
        .bind(next_seq)
        .bind(channel.as_str())
        .execute(&mut *conn)
        .await?;
        if let Some(max_deleted_seq) = invalid_seqs.iter().copied().max() {
            Self::remember_ha_channel_expired_legacy_watermark_on_conn(
                conn,
                channel,
                max_deleted_seq,
                updated_at,
            )
            .await?;
        }
        Ok((invalid_seqs.len() as i64, !reached_end, scanned_len > 0))
    }

    fn ha_outbox_gc_channel_mask(channel: HaSyncChannel) -> i64 {
        match channel {
            HaSyncChannel::Control => 1,
            HaSyncChannel::Billing => 2,
            HaSyncChannel::Runtime => 4,
        }
    }

    fn ha_outbox_gc_channel_from_name(name: &str) -> HaSyncChannel {
        match name {
            "billing" => HaSyncChannel::Billing,
            "runtime" => HaSyncChannel::Runtime,
            _ => HaSyncChannel::Control,
        }
    }

    fn next_ha_outbox_gc_channel(channel: HaSyncChannel) -> HaSyncChannel {
        match channel {
            HaSyncChannel::Control => HaSyncChannel::Billing,
            HaSyncChannel::Billing => HaSyncChannel::Runtime,
            HaSyncChannel::Runtime => HaSyncChannel::Control,
        }
    }

}

include!("key_store_ha_gc_online.rs");
