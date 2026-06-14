#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaPromoteRequest {
    force: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaSourceSettingsRequest {
    source_kind: tavily_hikari::HaSourceKind,
    direct_origin_scheme: Option<tavily_hikari::OriginScheme>,
    direct_origin_host: Option<String>,
    direct_origin_port: Option<u16>,
    origin_group_id: Option<String>,
    apply_to_edgeone: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaRecoveryImportRequest {
    batch_id: Option<String>,
    source_node_id: Option<String>,
    message: Option<String>,
    request_logs: Option<Vec<Value>>,
    auth_token_logs: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaEventsQuery {
    after: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaEventsAckRequest {
    peer_node_id: String,
    acked_seq: i64,
}

#[derive(Debug)]
struct HaEventResponseItem {
    seq: i64,
    value: Value,
}

const HA_BASELINE_MAX_COMPRESSED_BYTES: usize = 64 * 1024 * 1024;
const HA_EVENTS_MAX_COMPRESSED_BYTES: usize = 4 * 1024 * 1024;

fn is_ha_admin_or_internal(state: &AppState, headers: &HeaderMap) -> bool {
    if is_admin_request(state, headers) {
        return true;
    }
    let token = headers
        .get("x-ha-internal-token")
        .and_then(|value| value.to_str().ok());
    state.ha.internal_token_matches(token)
}

async fn get_admin_ha_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    Ok(Json(state.ha.status().await))
}

async fn put_admin_ha_source_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaSourceSettingsRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }

    let settings = tavily_hikari::HaSourceSettings {
        source_kind: payload.source_kind,
        direct_origin_scheme: payload.direct_origin_scheme,
        direct_origin_host: payload.direct_origin_host,
        direct_origin_port: payload.direct_origin_port,
        origin_group_id: payload.origin_group_id,
    }
    .validate()
    .map_err(|err| (StatusCode::BAD_REQUEST, err))?;

    let status = if payload.apply_to_edgeone.unwrap_or(false) {
        state
            .ha
            .set_local_source_settings(Some(settings.clone()))
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
        let (status, audit_entries) = state
            .ha
            .apply_local_source_settings_to_edgeone(settings.clone())
            .await
            .map_err(|err| (StatusCode::CONFLICT, err))?;
        state
            .proxy
            .persist_ha_node_state(
                &status.node_id,
                status.role,
                status.edgeone_origin.as_deref(),
                status.ha_source_effective.as_ref(),
                status.message.as_deref(),
            )
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        for (idx, entry) in audit_entries.iter().enumerate() {
            state
                .proxy
                .insert_ha_edgeone_audit_log(
                    &format!("ha-source-settings-{}-{idx}", nanoid::nanoid!(8)),
                    &entry.action,
                    entry.request_json.as_deref(),
                    entry.response_json.as_deref(),
                    &entry.status,
                    entry.message.as_deref(),
                )
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        }
        status
    } else {
        let status = state
            .ha
            .set_local_source_settings_view(Some(tavily_hikari::HaSourceSettingsView {
                source_kind: settings.source_kind,
                direct_origin_scheme: settings.direct_origin_scheme,
                direct_origin_host: settings.direct_origin_host.clone(),
                direct_origin_port: settings.direct_origin_port,
                origin_group_id: settings.origin_group_id.clone(),
                target: settings.effective_target(),
            }))
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
        state
            .proxy
            .persist_ha_node_state(
                &status.node_id,
                status.role,
                status.edgeone_origin.as_deref(),
                status.ha_source_effective.as_ref(),
                status.message.as_deref(),
            )
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        status
    };

    Ok(Json(status))
}

async fn get_admin_ha_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    gone_ha_snapshot_response()
}

async fn put_admin_ha_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    _body: Bytes,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    gone_ha_snapshot_response()
}

fn gone_ha_snapshot_response() -> Result<Response<Body>, (StatusCode, String)> {
    Response::builder()
        .status(StatusCode::GONE)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "error": "ha_snapshot_removed",
                "message": "Full SQLite HA snapshots are disabled; use /api/admin/ha/baseline and /api/admin/ha/events."
            })
            .to_string(),
        ))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn get_admin_ha_baseline(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let status = state.ha.status().await;
    if !status.allows_basic_business {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "HA baseline export requires active/provisional role, current role is {:?}",
                status.role
            ),
        ));
    }
    let export = state
        .proxy
        .export_ha_baseline_ndjson(&status.node_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let compressed = encode_zstd_limited(&export.ndjson, HA_BASELINE_MAX_COMPRESSED_BYTES)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-ndjson")
        .header("content-encoding", "zstd")
        .header("x-ha-schema-version", "1")
        .header("x-ha-high-watermark", export.high_watermark.to_string())
        .header("x-ha-row-count", export.row_count.to_string())
        .body(Body::from(compressed))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn get_admin_ha_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HaEventsQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let status = state.ha.status().await;
    if !status.allows_basic_business {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "HA events export requires active/provisional role, current role is {:?}",
                status.role
            ),
        ));
    }
    let after = query.after.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let events = state
        .proxy
        .list_ha_outbox_events_after(after, limit)
        .await
        .map_err(|err| {
            let message = err.to_string();
            if message.contains("retention window") {
                (StatusCode::GONE, message)
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        })?;
    let event_items = events
        .iter()
        .map(|event| HaEventResponseItem {
            seq: event.seq,
            value: json!({
                "schemaVersion": 1,
                "kind": "event",
                "event": event
            }),
        })
        .collect::<Vec<_>>();
    let encoded = encode_ha_events_limited(after, limit, &event_items, HA_EVENTS_MAX_COMPRESSED_BYTES)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-ndjson")
        .header("content-encoding", "zstd")
        .header("x-ha-schema-version", "1")
        .header("x-ha-last-seq", encoded.last_seq.to_string())
        .header("x-ha-event-count", encoded.event_count.to_string())
        .body(Body::from(encoded.compressed))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

struct EncodedHaEvents {
    compressed: Vec<u8>,
    last_seq: i64,
    event_count: usize,
}

fn encode_ha_events_limited(
    after: i64,
    limit: i64,
    events: &[HaEventResponseItem],
    max_compressed_bytes: usize,
) -> Result<EncodedHaEvents, (StatusCode, String)> {
    let mut event_count = events.len();
    loop {
        let selected = &events[..event_count];
        let mut ndjson = String::new();
        let mut last_seq = after;
        append_ha_events_ndjson(&mut ndjson, after, limit, selected, &mut last_seq)?;
        let compressed = zstd::stream::encode_all(ndjson.as_bytes(), 3)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        if compressed.len() <= max_compressed_bytes {
            return Ok(EncodedHaEvents {
                compressed,
                last_seq,
                event_count,
            });
        }
        if event_count <= 1 {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "HA events payload exceeds compressed limit: {} > {max_compressed_bytes}",
                    compressed.len()
                ),
            ));
        }
        event_count = event_count.div_ceil(2);
    }
}

fn append_ha_events_ndjson(
    ndjson: &mut String,
    after: i64,
    limit: i64,
    events: &[HaEventResponseItem],
    last_seq: &mut i64,
) -> Result<(), (StatusCode, String)> {
    ndjson.push_str(
        &serde_json::to_string(&json!({
            "schemaVersion": 1,
            "kind": "events_start",
            "after": after,
            "limit": limit
        }))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    ndjson.push('\n');
    for event in events {
        *last_seq = event.seq;
        ndjson.push_str(
            &serde_json::to_string(&event.value)
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
        );
        ndjson.push('\n');
    }
    ndjson.push_str(
        &serde_json::to_string(&json!({
            "schemaVersion": 1,
            "kind": "events_end",
            "lastSeq": *last_seq,
            "eventCount": events.len()
        }))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    ndjson.push('\n');
    Ok(())
}

async fn post_admin_ha_events_ack(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaEventsAckRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    state
        .proxy
        .ack_ha_peer_watermark(&payload.peer_node_id, payload.acked_seq)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "peerNodeId": payload.peer_node_id,
        "ackedSeq": payload.acked_seq.max(0)
    })))
}

fn encode_zstd_limited(value: &str, limit: usize) -> Result<Vec<u8>, (StatusCode, String)> {
    let compressed = zstd::stream::encode_all(value.as_bytes(), 3)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if compressed.len() > limit {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "HA payload exceeds compressed limit: {} > {limit}",
                compressed.len()
            ),
        ));
    }
    Ok(compressed)
}

async fn get_public_ha_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    let mut status = state.ha.status().await;
    status.edgeone_expected_origin = None;
    Ok(Json(status))
}

async fn post_admin_ha_promote(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaPromoteRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let before = state.ha.status().await;
    let result = state
        .ha
        .promote_self_to_provisional_with_audit(payload.force.unwrap_or(false))
        .await;
    let (status, audit_entries) = result.map_err(|err| (StatusCode::CONFLICT, err))?;
    let operation = tavily_hikari::HaFailoverOperationRecord {
        operation_id: format!("promote-{}-{}", status.node_id, Utc::now().timestamp()),
        operation_kind: "promote".to_string(),
        target_node_id: Some(status.node_id.clone()),
        from_origin: before.edgeone_origin,
        to_origin: status.edgeone_current_target.clone(),
        status: "provisional_master".to_string(),
        message: status.message.clone(),
    };
    state
        .proxy
        .insert_ha_failover_operation(&operation)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    state
        .proxy
        .persist_ha_node_state(
            &status.node_id,
            status.role,
            status.edgeone_origin.as_deref(),
            status.ha_source_effective.as_ref(),
            status.message.as_deref(),
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    for (idx, entry) in audit_entries.iter().enumerate() {
        state
            .proxy
            .insert_ha_edgeone_audit_log(
                &format!(
                    "{}-edgeone-{}-{idx}",
                    operation.operation_id,
                    nanoid::nanoid!(8)
                ),
                &entry.action,
                entry.request_json.as_deref(),
                entry.response_json.as_deref(),
                &entry.status,
                entry.message.as_deref(),
            )
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    }
    Ok(Json(status))
}

async fn post_admin_ha_finalize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let status = state
        .ha
        .finalize_failover()
        .await
        .map_err(|err| (StatusCode::CONFLICT, err))?;
    state
        .proxy
        .persist_ha_node_state(
            &status.node_id,
            status.role,
            status.edgeone_origin.as_deref(),
            status.ha_source_effective.as_ref(),
            status.message.as_deref(),
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(status))
}

async fn post_admin_ha_recovery_import(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaRecoveryImportRequest>,
) -> Result<Json<tavily_hikari::HaRecoveryImportResult>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let batch = payload.batch_id.unwrap_or_else(|| "manual".to_string());
    let source = payload.source_node_id.unwrap_or_else(|| "unknown".to_string());
    let message = payload
        .message
        .unwrap_or_else(|| format!("recovery batch {batch} imported from {source}"));
    let request_logs = payload.request_logs.unwrap_or_default();
    let auth_token_logs = payload.auth_token_logs.unwrap_or_default();
    if !request_logs.is_empty() || !auth_token_logs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "HA recovery no longer accepts request_logs or auth_token_logs".to_string(),
        ));
    }
    let requested_event_count = 0_usize;
    let checksum_payload = serde_json::json!({
        "message": &message,
        "ledgerEvents": [],
    });
    let checksum = tavily_hikari::sha256_hex_bytes(checksum_payload.to_string().as_bytes());
    let imported = state
        .proxy
        .claim_ha_recovery_batch(
            &batch,
            &source,
            i64::try_from(requested_event_count).unwrap_or(i64::MAX),
            &checksum,
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut imported_event_count = 0_i64;
    if imported {
        imported_event_count = state
            .proxy
            .import_ha_recovery_events()
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        state
            .proxy
            .rebuild_ha_recovery_rollups()
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        state
            .proxy
            .complete_ha_recovery_batch(&batch, "imported", imported_event_count)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    }
    let status = state.ha.status().await;
    state
        .proxy
        .persist_ha_node_state(
            &status.node_id,
            status.role,
            status.edgeone_origin.as_deref(),
            status.ha_source_effective.as_ref(),
            status.message.as_deref(),
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(tavily_hikari::HaRecoveryImportResult {
        batch_id: batch,
        source_node_id: source,
        imported,
        event_count: imported_event_count.max(i64::try_from(requested_event_count).unwrap_or(0)),
        checksum,
        message,
        status,
    }))
}
