use futures_util::stream;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaPromoteRequest {
    force: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaPlannedCutoverRequest {
    target_node_id: String,
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
    channel: Option<String>,
    after: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaEventsAckRequest {
    channel: String,
    peer_node_id: String,
    acked_seq: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaTimelineQuery {
    cursor: Option<i64>,
    limit: Option<i64>,
    node_id: Option<String>,
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaNodeDetailQuery {
    cursor: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InternalHaStatusQuery {
    refresh_authority: Option<bool>,
    peer_node_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InternalHaFinalizeRequest {
    operation_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InternalHaLeaderUpdateRequest {
    full_master_node_id: String,
    operation_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannedCutoverResponse {
    operation_id: String,
    status: String,
    detail: Option<String>,
}

const HA_EVENTS_MAX_COMPRESSED_BYTES: usize = 4 * 1024 * 1024;
const HA_BASELINE_MAX_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const HA_PLANNED_CUTOVER_MAX_LAG_SECS: i64 = 30;
const HA_PEER_STALE_SECS: i64 = 30;
const HA_PLANNED_CUTOVER_POLL_TIMEOUT_SECS: u64 = 30;

struct HaPerfEvent {
    event: &'static str,
    elapsed: Duration,
    channel: tavily_hikari::HaSyncChannel,
    row_count: usize,
    payload_bytes: usize,
    compressed_bytes: u64,
}

fn emit_ha_perf_event(
    perf: HaPerfEvent,
    sampling: tavily_hikari::HaPerfSampling,
    outbox: Option<&tavily_hikari::HaOutboxStats>,
) {
    if sampling.emit_info || sampling.capture_heavy_stats {
        let memory = sampling
            .capture_heavy_stats
            .then(tavily_hikari::capture_runtime_memory_snapshot);
        tracing::info!(
            component = "ha",
            event = perf.event,
            elapsed_ms = perf.elapsed.as_millis() as u64,
            channel = perf.channel.as_str(),
            row_count = perf.row_count as u64,
            payload_bytes = perf.payload_bytes as u64,
            compressed_bytes = perf.compressed_bytes,
            outbox_sampled = outbox.is_some(),
            outbox_sequence_span_estimate = outbox.map(|value| value.sequence_span_estimate).unwrap_or_default(),
            outbox_high_watermark = outbox.map(|value| value.high_watermark).unwrap_or_default(),
            outbox_oldest_age_secs = outbox.map(|value| value.oldest_age_secs),
            outbox_ack_lag = outbox.and_then(|value| value.ack_lag),
            memory_sampled = memory.is_some(),
            memory_current_bytes = memory.and_then(|value| value.memory_current_bytes),
            memory_limit_bytes = memory.and_then(|value| value.memory_limit_bytes),
            headroom_bytes = memory.and_then(|value| value.headroom_bytes),
            process_rss_bytes = memory.and_then(|value| value.process_rss_bytes),
            child_process_rss_bytes = memory.and_then(|value| value.child_process_rss_bytes),
            process_group_rss_bytes = memory.and_then(|value| value.process_group_rss_bytes),
            process_hwm_bytes = memory.and_then(|value| value.process_hwm_bytes),
            process_swap_bytes = memory.and_then(|value| value.process_swap_bytes),
            "ha perf"
        );
    } else {
        tracing::debug!(
            component = "ha",
            event = perf.event,
            elapsed_ms = perf.elapsed.as_millis() as u64,
            channel = perf.channel.as_str(),
            row_count = perf.row_count as u64,
            payload_bytes = perf.payload_bytes as u64,
            compressed_bytes = perf.compressed_bytes,
            "ha perf"
        );
    }
}

fn parse_ha_channel(raw: Option<&str>) -> Result<tavily_hikari::HaSyncChannel, (StatusCode, String)> {
    let Some(value) = raw else {
        return Err((
            StatusCode::BAD_REQUEST,
            "missing required HA channel".to_string(),
        ));
    };
    tavily_hikari::HaSyncChannel::parse(value).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid HA channel: {value}"),
        )
    })
}

async fn is_ha_admin_or_internal(state: &AppState, headers: &HeaderMap) -> bool {
    if is_admin_request(state, headers).await {
        return true;
    }
    let token = headers
        .get("x-ha-internal-token")
        .and_then(|value| value.to_str().ok());
    state.ha.internal_token_matches(token)
}

fn is_ha_internal_request(state: &AppState, headers: &HeaderMap) -> bool {
    let token = headers
        .get("x-ha-internal-token")
        .and_then(|value| value.to_str().ok());
    state.ha.internal_token_matches(token)
}

fn parse_timeline_category(
    raw: Option<&str>,
) -> Result<Option<tavily_hikari::HaControlPlaneEventCategory>, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    tavily_hikari::HaControlPlaneEventCategory::parse(raw).map(Some).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid HA timeline category: {raw}"),
        )
    })
}

fn source_settings_from_view(
    view: &tavily_hikari::HaSourceSettingsView,
) -> Result<tavily_hikari::HaSourceSettings, String> {
    tavily_hikari::HaSourceSettings {
        source_kind: view.source_kind,
        direct_origin_scheme: view.direct_origin_scheme,
        direct_origin_host: view.direct_origin_host.clone(),
        direct_origin_port: view.direct_origin_port,
        origin_group_id: view.origin_group_id.clone(),
    }
    .validate()
}

fn latest_probe_timestamp(status: &tavily_hikari::HaStatusView) -> Option<i64> {
    match (status.last_edgeone_check_at, status.last_sync_at) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn local_planned_cutover_eligible(status: &tavily_hikari::HaStatusView, now_ts: i64) -> bool {
    status.role == tavily_hikari::HaNodeRole::Standby
        && status.recovery_status.is_none()
        && status
            .sync_lag_seconds
            .is_some_and(|value| value <= HA_PLANNED_CUTOVER_MAX_LAG_SECS)
        && latest_probe_timestamp(status)
            .is_some_and(|value| now_ts.saturating_sub(value) <= HA_PEER_STALE_SECS)
}

fn ha_can_export_channel(status: &tavily_hikari::HaStatusView, channel: tavily_hikari::HaSyncChannel) -> bool {
    match channel {
        tavily_hikari::HaSyncChannel::Control => status.allows_full_writes,
        tavily_hikari::HaSyncChannel::Billing | tavily_hikari::HaSyncChannel::Runtime => {
            status.allows_basic_business
        }
    }
}

fn peer_view_from_status(
    config: &tavily_hikari::HaPeerNodeConfig,
    status: &tavily_hikari::HaStatusView,
    last_seen_at: i64,
    now_ts: i64,
) -> tavily_hikari::HaPeerNodeView {
    let stale = latest_probe_timestamp(status)
        .map(|value| now_ts.saturating_sub(value) > HA_PEER_STALE_SECS)
        .unwrap_or(true);
    let planned_cutover_eligible = config.role_hint == tavily_hikari::HaPeerRoleHint::StandbyCandidate
        && !stale
        && local_planned_cutover_eligible(status, now_ts);
    tavily_hikari::HaPeerNodeView {
        node_id: config.node_id.clone(),
        public_origin: Some(config.public_origin.clone()),
        source_config_target: status
            .ha_source_effective
            .as_ref()
            .and_then(|settings| settings.target.clone()),
        role: Some(status.role),
        allows_basic_business: status.allows_basic_business,
        allows_full_writes: status.allows_full_writes,
        last_sync_at: status.last_sync_at,
        sync_lag_seconds: status.sync_lag_seconds,
        recovery_status: status.recovery_status.clone(),
        message: status.message.clone(),
        last_seen_at: Some(last_seen_at),
        stale,
        role_hint: config.role_hint,
        planned_cutover_eligible,
        channel_health: status
            .peer_nodes
            .iter()
            .find(|node| node.node_id == status.node_id)
            .map(|node| node.channel_health.clone())
            .unwrap_or_default(),
    }
}

fn peer_view_from_error(
    config: &tavily_hikari::HaPeerNodeConfig,
    message: String,
) -> tavily_hikari::HaPeerNodeView {
    tavily_hikari::HaPeerNodeView {
        node_id: config.node_id.clone(),
        public_origin: Some(config.public_origin.clone()),
        source_config_target: None,
        role: None,
        allows_basic_business: false,
        allows_full_writes: false,
        last_sync_at: None,
        sync_lag_seconds: None,
        recovery_status: None,
        message: Some(message),
        last_seen_at: None,
        stale: true,
        role_hint: config.role_hint,
        planned_cutover_eligible: false,
        channel_health: Vec::new(),
    }
}

async fn fetch_internal_ha_status(
    client: &Client,
    peer: &tavily_hikari::HaPeerNodeConfig,
    internal_token: &str,
    local_node_id: &str,
) -> Result<tavily_hikari::HaStatusView, String> {
    let response = client
        .get(format!("{}/api/internal/ha/status", peer.admin_base_url))
        .query(&[
            ("refreshAuthority", "true"),
            ("peerNodeId", local_node_id),
        ])
        .header("x-ha-internal-token", internal_token)
        .send()
        .await
        .map_err(|err| format!("peer {} unreachable: {err}", peer.node_id))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(format!(
            "peer {} returned {} for internal HA status: {}",
            peer.node_id, status, detail
        ));
    }
    response
        .json::<tavily_hikari::HaStatusView>()
        .await
        .map_err(|err| format!("peer {} returned invalid HA status: {err}", peer.node_id))
}

async fn post_internal_ha_leader_update(
    client: &Client,
    peer: &tavily_hikari::HaPeerNodeConfig,
    internal_token: &str,
    full_master_node_id: &str,
    operation_id: Option<&str>,
) -> Result<tavily_hikari::HaStatusView, String> {
    let response = client
        .post(format!("{}/api/internal/ha/leader", peer.admin_base_url))
        .header("x-ha-internal-token", internal_token)
        .json(&InternalHaLeaderUpdateRequest {
            full_master_node_id: full_master_node_id.to_string(),
            operation_id: operation_id.map(str::to_string),
        })
        .send()
        .await
        .map_err(|err| format!("dual-active peer leader update failed: {err}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(format!(
            "dual-active peer leader update failed with {status}: {detail}"
        ));
    }
    response
        .json::<tavily_hikari::HaStatusView>()
        .await
        .map_err(|err| format!("dual-active peer leader update response invalid: {err}"))
}

async fn build_internal_ha_status(state: &Arc<AppState>) -> tavily_hikari::HaStatusView {
    let now_ts = state.proxy.backend_time().now_ts();
    let mut status = state.ha.status().await;
    status.planned_cutover_eligible = local_planned_cutover_eligible(&status, now_ts);
    status.peer_nodes = Vec::new();
    status
}

async fn attach_internal_source_channel_health(
    state: &Arc<AppState>,
    status: &mut tavily_hikari::HaStatusView,
    peer_node_id: Option<&str>,
) {
    let mut channel_health = Vec::with_capacity(3);
    for channel in [
        tavily_hikari::HaSyncChannel::Control,
        tavily_hikari::HaSyncChannel::Billing,
        tavily_hikari::HaSyncChannel::Runtime,
    ] {
        let high_watermark_result = state.proxy.ha_channel_high_watermark(channel).await;
        let source_unavailable = high_watermark_result.is_err();
        let high_watermark = high_watermark_result.unwrap_or(0);
        let peer_health = match peer_node_id {
            Some(peer_node_id) => state.proxy.ha_peer_channel_health(channel, peer_node_id).await.ok(),
            None => None,
        };
        channel_health.push(tavily_hikari::HaChannelHealthView {
            channel,
            acked_seq: None,
            high_watermark,
            ack_lag: None,
            cursor_state: if source_unavailable {
                "unavailable"
            } else if peer_health.as_ref().is_some_and(|health| health.expired_backlog) {
                "expired_backlog"
            } else {
                "source"
            }
            .to_string(),
            retention_secs: match channel {
                tavily_hikari::HaSyncChannel::Control => 72 * 60 * 60,
                tavily_hikari::HaSyncChannel::Billing
                | tavily_hikari::HaSyncChannel::Runtime => 14 * 24 * 60 * 60,
            },
            expired_backlog: peer_health.as_ref().is_some_and(|health| health.expired_backlog),
            gc_state: peer_health.as_ref().map(|health| health.gc_state.clone()).unwrap_or_else(|| "unknown".to_string()),
            oldest_age_secs: peer_health.as_ref().and_then(|health| health.oldest_age_secs),
            last_progress_at: peer_health.as_ref().and_then(|health| health.last_progress_at),
            last_defer_reason: peer_health.as_ref().and_then(|health| health.last_defer_reason.clone()),
            next_retry_at: peer_health.as_ref().and_then(|health| health.next_retry_at),
            batch_size: peer_health.as_ref().map(|health| health.batch_size).unwrap_or(250),
            gc_debt_mode: peer_health.as_ref().map(|health| health.gc_debt_mode.clone()).unwrap_or_else(|| "unknown".to_string()),
            gc_observed_at: peer_health.as_ref().and_then(|health| health.gc_observed_at),
            gc_deleted_rows_per_minute: peer_health.as_ref().map(|health| health.gc_deleted_rows_per_minute).unwrap_or(0.0),
            gc_recovery_deadline_at: peer_health.as_ref().and_then(|health| health.gc_recovery_deadline_at),
            gc_slo_state: peer_health.as_ref().map(|health| health.gc_slo_state.clone()).unwrap_or_else(|| "unknown".to_string()),
            gc_foreground_rps: peer_health.as_ref().map(|health| health.gc_foreground_rps).unwrap_or(0),
        });
    }
    status.peer_nodes = vec![tavily_hikari::HaPeerNodeView {
        node_id: status.node_id.clone(),
        public_origin: status.node_public_origin.clone(),
        source_config_target: status
            .ha_source_effective
            .as_ref()
            .and_then(|settings| settings.target.clone()),
        role: Some(status.role),
        allows_basic_business: status.allows_basic_business,
        allows_full_writes: status.allows_full_writes,
        last_sync_at: status.last_sync_at,
        sync_lag_seconds: status.sync_lag_seconds,
        recovery_status: status.recovery_status.clone(),
        message: status.message.clone(),
        last_seen_at: status.last_sync_at,
        stale: false,
        role_hint: tavily_hikari::HaPeerRoleHint::Observer,
        planned_cutover_eligible: status.planned_cutover_eligible,
        channel_health,
    }];
}

async fn build_admin_ha_status(state: &Arc<AppState>) -> tavily_hikari::HaStatusView {
    let now_ts = state.proxy.backend_time().now_ts();
    let mut status = build_internal_ha_status(state).await;
    if let Some(internal_token) = state.ha.internal_token() {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| Client::new());
        let peers: Vec<_> = state
            .ha
            .peer_nodes()
            .into_iter()
            .filter(|peer| peer.node_id != status.node_id)
            .collect();
        let local_node_id = status.node_id.clone();
        status.peer_nodes = stream::iter(peers)
            .map(|peer| {
                let client = client.clone();
                let internal_token = internal_token.to_string();
                let local_node_id = local_node_id.clone();
                async move {
                    let last_seen_at = now_ts;
                    match fetch_internal_ha_status(&client, &peer, &internal_token, &local_node_id).await {
                        Ok(peer_status) => peer_view_from_status(&peer, &peer_status, last_seen_at, now_ts),
                        Err(err) => peer_view_from_error(&peer, err),
                    }
                }
            })
            .buffer_unordered(8)
            .collect()
            .await;
    } else {
        status.peer_nodes = state
            .ha
            .peer_nodes()
            .into_iter()
            .filter(|peer| peer.node_id != status.node_id)
            .map(|peer| peer_view_from_error(&peer, "HA_INTERNAL_TOKEN is required for peer probing".to_string()))
            .collect();
    }
    for peer in &mut status.peer_nodes {
        let mut health = Vec::with_capacity(3);
        let peer_probe_succeeded = peer.role.is_some() && peer.last_seen_at.is_some();
        for channel in [
            tavily_hikari::HaSyncChannel::Control,
            tavily_hikari::HaSyncChannel::Billing,
            tavily_hikari::HaSyncChannel::Runtime,
        ] {
            let value = if !peer_probe_succeeded {
                Some(tavily_hikari::HaChannelHealthView {
                    channel,
                    acked_seq: None,
                    high_watermark: 0,
                    ack_lag: None,
                    cursor_state: "unavailable".to_string(),
                    retention_secs: match channel {
                        tavily_hikari::HaSyncChannel::Control => 72 * 60 * 60,
                        tavily_hikari::HaSyncChannel::Billing
                        | tavily_hikari::HaSyncChannel::Runtime => 14 * 24 * 60 * 60,
                    },
                    expired_backlog: false,
                    gc_state: "unknown".to_string(),
                    oldest_age_secs: None,
                    last_progress_at: None,
                    last_defer_reason: None,
                    next_retry_at: None,
                    batch_size: 250,
                    gc_debt_mode: "unknown".to_string(),
                    gc_observed_at: None,
                    gc_deleted_rows_per_minute: 0.0,
                    gc_recovery_deadline_at: None,
                    gc_slo_state: "unknown".to_string(),
                    gc_foreground_rps: 0,
                })
            } else if status.role == tavily_hikari::HaNodeRole::Standby {
                let source_health = peer
                    .channel_health
                    .iter()
                    .find(|health| health.channel == channel)
                    .cloned();
                let source_high_watermark = source_health.as_ref().map(|health| health.high_watermark);
                let source_expired_backlog = source_health
                    .as_ref()
                    .is_some_and(|health| health.expired_backlog);
                let source_unavailable = source_health
                    .as_ref()
                    .is_some_and(|health| health.cursor_state == "unavailable");
                let watermark_name = if state.ha.dual_active_enabled() {
                    format!("peer_{}_{}_applied_seq", peer.node_id, channel.as_str())
                } else {
                    format!("standby_{}_applied_seq", channel.as_str())
                };
                let baseline_name = if state.ha.dual_active_enabled() {
                    format!("peer_{}_{}_baseline_applied", peer.node_id, channel.as_str())
                } else {
                    format!("standby_{}_baseline_applied", channel.as_str())
                };
                let applied_seq = state
                    .proxy
                    .get_ha_sync_watermark(&watermark_name)
                    .await
                    .ok()
                    .flatten();
                let baseline_applied = state
                    .proxy
                    .get_ha_sync_watermark(&baseline_name)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|value| value > 0);
                let acked_seq = match applied_seq {
                    Some(value) if value > 0 => Some(value),
                    Some(0) if baseline_applied => Some(0),
                    _ => None,
                };
                let high_watermark = source_high_watermark.unwrap_or(0);
                Some(tavily_hikari::HaChannelHealthView {
                    channel,
                    acked_seq,
                    high_watermark,
                    ack_lag: source_high_watermark
                        .zip(acked_seq)
                        .map(|(high, acked)| high.saturating_sub(acked).max(0)),
                    cursor_state: if source_unavailable {
                        "unavailable"
                    } else if source_expired_backlog {
                        "expired_backlog"
                    } else if source_health.is_none() || source_high_watermark.is_none() {
                        // Older peers can answer successfully without the optional channel
                        // telemetry. Keep the probe result neutral during rolling upgrades.
                        "unknown"
                    } else if acked_seq.is_none() {
                        "baseline_required"
                    } else if acked_seq.is_some_and(|acked| acked >= high_watermark) {
                        "healthy"
                    } else {
                        "catching_up"
                    }
                    .to_string(),
                    retention_secs: match channel {
                        tavily_hikari::HaSyncChannel::Control => 72 * 60 * 60,
                        tavily_hikari::HaSyncChannel::Billing
                        | tavily_hikari::HaSyncChannel::Runtime => 14 * 24 * 60 * 60,
                    },
                    expired_backlog: source_expired_backlog,
                    gc_state: source_health.as_ref().map(|health| health.gc_state.clone()).unwrap_or_else(|| "unknown".to_string()),
                    oldest_age_secs: source_health.as_ref().and_then(|health| health.oldest_age_secs),
                    last_progress_at: source_health.as_ref().and_then(|health| health.last_progress_at),
                    last_defer_reason: source_health.as_ref().and_then(|health| health.last_defer_reason.clone()),
                    next_retry_at: source_health.as_ref().and_then(|health| health.next_retry_at),
                    batch_size: source_health.as_ref().map(|health| health.batch_size).unwrap_or(250),
                    gc_debt_mode: source_health.as_ref().map(|health| health.gc_debt_mode.clone()).unwrap_or_else(|| "unknown".to_string()),
                    gc_observed_at: source_health.as_ref().and_then(|health| health.gc_observed_at),
                    gc_deleted_rows_per_minute: source_health.as_ref().map(|health| health.gc_deleted_rows_per_minute).unwrap_or(0.0),
                    gc_recovery_deadline_at: source_health.as_ref().and_then(|health| health.gc_recovery_deadline_at),
                    gc_slo_state: source_health.as_ref().map(|health| health.gc_slo_state.clone()).unwrap_or_else(|| "unknown".to_string()),
                    gc_foreground_rps: source_health.as_ref().map(|health| health.gc_foreground_rps).unwrap_or(0),
                })
            } else {
                state.proxy.ha_peer_channel_health(channel, &peer.node_id).await.ok()
            };
            if let Some(value) = value {
                health.push(value);
            }
        }
        peer.channel_health = health;
    }
    status
}

async fn persist_ha_status_snapshot(
    state: &Arc<AppState>,
    status: &tavily_hikari::HaStatusView,
) -> Result<(), (StatusCode, String)> {
    if !status.allows_full_writes {
        state
            .proxy
            .reset_post_ready_serving_tasks_for_writable_tenure();
    }
    sync_forward_proxy_runtime_for_status(state.proxy.clone(), status)
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
    state
        .proxy
        .flush_ha_state_writes()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let _ = spawn_post_ready_serving_tasks_for_status(state.clone(), status);
    Ok(())
}

async fn record_ha_control_plane_event(
    state: &Arc<AppState>,
    event: tavily_hikari::HaControlPlaneEventInsert,
) -> Result<(), (StatusCode, String)> {
    state
        .proxy
        .insert_ha_control_plane_event(&event)
        .await
        .map(|_| ())
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn record_edgeone_audit_entries(
    state: &Arc<AppState>,
    operation_id: &str,
    audit_entries: &[tavily_hikari::EdgeOneAuditEntry],
) -> Result<(), (StatusCode, String)> {
    for (idx, entry) in audit_entries.iter().enumerate() {
        state
            .proxy
            .insert_ha_edgeone_audit_log(
                &format!("{operation_id}-edgeone-{}-{idx}", nanoid::nanoid!(8)),
                &entry.action,
                entry.request_json.as_deref(),
                entry.response_json.as_deref(),
                &entry.status,
                entry.message.as_deref(),
            )
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        record_ha_control_plane_event(
            state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: format!("edgeone_{}", entry.action.to_ascii_lowercase()),
                category: tavily_hikari::HaControlPlaneEventCategory::Edgeone,
                status: if entry.status.eq_ignore_ascii_case("success") {
                    tavily_hikari::HaControlPlaneEventStatus::Success
                } else {
                    tavily_hikari::HaControlPlaneEventStatus::Error
                },
                node_id: None,
                operation_id: Some(operation_id.to_string()),
                summary: format!("EdgeOne {} {}", entry.action, entry.status),
                detail: entry.message.clone(),
                technical_details: Some(json!({
                    "action": entry.action,
                    "requestJson": entry.request_json,
                    "responseJson": entry.response_json,
                    "status": entry.status,
                })),
            },
        )
        .await?;
    }
    Ok(())
}

async fn get_admin_ha_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    Ok(Json(build_admin_ha_status(&state).await))
}

async fn put_admin_ha_source_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaSourceSettingsRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    gone_ha_snapshot_response()
}

async fn put_admin_ha_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    _body: Bytes,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
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
    Query(query): Query<HaEventsQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let channel = parse_ha_channel(query.channel.as_deref())?;
    let status = state.ha.status().await;
    if !ha_can_export_channel(&status, channel) {
        return Err((
            StatusCode::CONFLICT,
            format!("HA baseline export requires serving role for {}", channel.as_str()),
        ));
    }
    let baseline = build_ha_baseline_reader(&state.proxy, channel, &status.node_id).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-ndjson")
        .header("content-encoding", "zstd")
        .header("x-ha-schema-version", "2")
        .header("x-ha-channel", channel.as_str())
        .header(
            "x-ha-high-watermark",
            baseline.export.high_watermark.to_string(),
        )
        .header("x-ha-row-count", baseline.export.row_count.to_string())
        .body(Body::from_stream(ReaderStream::new(baseline.reader)))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

struct HaBaselineReader {
    reader: tokio::fs::File,
    export: tavily_hikari::HaApplyResult,
}

struct HaEventsReader {
    reader: tokio::fs::File,
    export: tavily_hikari::HaApplyResult,
}

fn create_ha_temp_output_file() -> Result<tokio::fs::File, (StatusCode, String)> {
    tempfile::tempfile()
        .map(tokio::fs::File::from_std)
        .map_err(internal_error)
}

async fn rewind_ha_temp_output_file(
    file: &mut tokio::fs::File,
) -> Result<(), (StatusCode, String)> {
    tokio::io::AsyncSeekExt::seek(file, std::io::SeekFrom::Start(0))
        .await
        .map(|_| ())
        .map_err(internal_error)
}

async fn build_ha_baseline_reader(
    proxy: &TavilyProxy,
    channel: tavily_hikari::HaSyncChannel,
    node_id: &str,
) -> Result<HaBaselineReader, (StatusCode, String)> {
    let started = Instant::now();
    #[cfg(test)]
    if std::env::var("TAVILY_TEST_FAIL_HA_BASELINE_EXPORT")
        .ok()
        .as_deref()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case(channel.as_str()))
    {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "forced HA baseline export failure".to_string(),
        ));
    }
    let mut preflight = proxy
        .begin_ha_baseline_read(channel)
        .await
        .map_err(internal_error)?;
    let mut export = preflight.export_info().await.map_err(internal_error)?;
    let mut reader = create_ha_temp_output_file()?;
    let encode_result = {
        let mut encoder =
            ZstdEncoder::with_quality(&mut reader, async_compression::Level::Precise(3));
        let export_result = preflight
            .write_ndjson(
                node_id,
                export.high_watermark,
                export.row_count,
                &mut encoder,
            )
            .await
            .map_err(internal_error);
        match export_result {
            Ok(payload_bytes) => match encoder.shutdown().await.map_err(internal_error) {
                Ok(()) => Ok(payload_bytes),
                Err(err) => Err(err),
            },
            Err(err) => Err(err),
        }
    };
    let close_result = preflight.close().await.map_err(internal_error);
    export.payload_bytes = encode_result?;
    close_result?;
    let compressed_bytes = reader.metadata().await.map_err(internal_error)?.len();
    if compressed_bytes > ha_baseline_max_compressed_bytes() {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "HA payload exceeds compressed limit: {compressed_bytes} > {}",
                ha_baseline_max_compressed_bytes()
            ),
        ));
    }
    rewind_ha_temp_output_file(&mut reader).await?;
    let elapsed = started.elapsed();
    let sampling = tavily_hikari::sample_ha_perf_event("baseline_export_completed", channel, elapsed);
    let outbox = if sampling.capture_heavy_stats {
        Some(
            proxy
                .ha_channel_outbox_stats(channel, None)
                .await
                .map_err(map_ha_export_error)?,
        )
    } else {
        None
    };
    emit_ha_perf_event(
        HaPerfEvent {
            event: "baseline_export_completed",
            elapsed,
            channel,
            row_count: export.row_count,
            payload_bytes: export.payload_bytes,
            compressed_bytes,
        },
        sampling,
        outbox.as_ref(),
    );
    Ok(HaBaselineReader { reader, export })
}

async fn build_ha_events_reader(
    proxy: &TavilyProxy,
    channel: tavily_hikari::HaSyncChannel,
    after: i64,
    limit: i64,
) -> Result<HaEventsReader, (StatusCode, String)> {
    let started = Instant::now();
    let max_compressed_bytes = ha_events_max_compressed_bytes();
    let mut preflight = proxy.begin_ha_events_read(channel).await.map_err(map_ha_export_error)?;
    let available = preflight
        .available_event_count(after, limit)
        .await
        .map_err(map_ha_export_error)?;
    let mut event_count = available;
    loop {
        let mut reader = create_ha_temp_output_file()?;
        let export_result = {
            let mut encoder =
                ZstdEncoder::with_quality(&mut reader, async_compression::Level::Precise(3));
            match preflight.write_ndjson(after, limit, event_count, &mut encoder).await {
                Ok(export) => match encoder.shutdown().await.map_err(internal_error) {
                    Ok(()) => Ok(export),
                    Err(err) => Err(err),
                },
                Err(err) => Err(map_ha_export_error(err)),
            }
        };
        let export = match export_result {
            Ok(export) => export,
            Err(err) => {
                let close_result = preflight.close().await.map_err(internal_error);
                close_result?;
                return Err(err);
            }
        };
        let compressed_bytes = reader.metadata().await.map_err(internal_error)?.len();
        if compressed_bytes <= max_compressed_bytes {
            preflight.close().await.map_err(internal_error)?;
            rewind_ha_temp_output_file(&mut reader).await?;
            let elapsed = started.elapsed();
            let sampling =
                tavily_hikari::sample_ha_perf_event("events_export_completed", channel, elapsed);
            let outbox = if sampling.capture_heavy_stats {
                Some(
                    proxy
                        .ha_channel_outbox_stats(channel, None)
                        .await
                        .map_err(map_ha_export_error)?,
                )
            } else {
                None
            };
            emit_ha_perf_event(
                HaPerfEvent {
                    event: "events_export_completed",
                    elapsed,
                    channel,
                    row_count: export.row_count,
                    payload_bytes: export.payload_bytes,
                    compressed_bytes,
                },
                sampling,
                outbox.as_ref(),
            );
            return Ok(HaEventsReader { reader, export });
        }
        if event_count <= 1 {
            preflight.close().await.map_err(internal_error)?;
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "HA events payload exceeds compressed limit: {compressed_bytes} > {max_compressed_bytes}"
                ),
            ));
        }
        event_count = event_count.div_ceil(2);
    }
}

fn internal_error(err: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn map_ha_export_error(err: impl std::fmt::Display) -> (StatusCode, String) {
    let message = err.to_string();
    if message.contains("retention window") {
        (StatusCode::GONE, message)
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

fn ha_baseline_max_compressed_bytes() -> u64 {
    #[cfg(test)]
    if let Ok(value) = std::env::var("TAVILY_TEST_HA_BASELINE_MAX_COMPRESSED_BYTES")
        && let Ok(parsed) = value.parse::<u64>()
    {
        return parsed;
    }
    HA_BASELINE_MAX_COMPRESSED_BYTES
}

fn ha_events_max_compressed_bytes() -> u64 {
    #[cfg(test)]
    if let Ok(value) = std::env::var("TAVILY_TEST_HA_EVENTS_MAX_COMPRESSED_BYTES")
        && let Ok(parsed) = value.parse::<u64>()
    {
        return parsed;
    }
    HA_EVENTS_MAX_COMPRESSED_BYTES as u64
}

async fn get_admin_ha_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HaEventsQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let channel = parse_ha_channel(query.channel.as_deref())?;
    let status = state.ha.status().await;
    if !ha_can_export_channel(&status, channel) {
        return Err((
            StatusCode::CONFLICT,
            format!("HA events export requires serving role for {}", channel.as_str()),
        ));
    }
    let after = query.after.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let events = build_ha_events_reader(&state.proxy, channel, after, limit).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-ndjson")
        .header("content-encoding", "zstd")
        .header("x-ha-schema-version", "2")
        .header("x-ha-channel", channel.as_str())
        .header("x-ha-last-seq", events.export.high_watermark.to_string())
        .header("x-ha-event-count", events.export.row_count.to_string())
        .body(Body::from_stream(ReaderStream::new(events.reader)))
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn post_admin_ha_events_ack(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaEventsAckRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let channel = parse_ha_channel(Some(payload.channel.as_str()))?;
    state
        .proxy
        .ack_ha_peer_watermark(channel, &payload.peer_node_id, payload.acked_seq)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "channel": channel,
        "peerNodeId": payload.peer_node_id,
        "ackedSeq": payload.acked_seq.max(0)
    })))
}

async fn get_public_ha_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    let mut status = build_internal_ha_status(&state).await;
    status.edgeone_expected_origin = None;
    status.peer_nodes.clear();
    status.planned_cutover_eligible = false;
    Ok(Json(status))
}

async fn get_internal_ha_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<InternalHaStatusQuery>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_ha_internal_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if query.refresh_authority.unwrap_or(false) {
        let mut status = state
            .ha
            .refresh_authoritative_role()
            .await
            .map_err(|err| (StatusCode::BAD_GATEWAY, err))?;
        attach_internal_source_channel_health(&state, &mut status, query.peer_node_id.as_deref()).await;
        return Ok(Json(status));
    }
    let mut status = build_internal_ha_status(&state).await;
    attach_internal_source_channel_health(&state, &mut status, query.peer_node_id.as_deref()).await;
    Ok(Json(status))
}

async fn get_internal_ha_mcp_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(proxy_session_id): Path<String>,
) -> Result<Json<tavily_hikari::McpSessionBinding>, (StatusCode, String)> {
    if !is_ha_internal_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let session = state
        .proxy
        .get_active_mcp_session(&proxy_session_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let Some(session) = session else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown MCP session: {proxy_session_id}"),
        ));
    };
    Ok(Json(session))
}

async fn get_internal_ha_research_request(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !is_ha_internal_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let affinity = state
        .proxy
        .get_research_request_affinity(&request_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let Some((key_id, token_id, expires_at)) = affinity else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown research request: {request_id}"),
        ));
    };
    Ok(Json(json!({
        "keyId": key_id,
        "tokenId": token_id,
        "expiresAt": expires_at,
    })))
}

async fn post_admin_ha_promote(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaPromoteRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if state.ha.dual_active_enabled() {
        if !payload.force.unwrap_or(false) {
            return Err((
                StatusCode::CONFLICT,
                "dual-active promote requires force=true; use planned cutover while the current full_master is reachable".to_string(),
            ));
        }
        let node_id = state.ha.status().await.node_id;
        let before = state.ha.status().await;
        let peers = state
            .ha
            .peer_nodes()
            .into_iter()
            .filter(|peer| peer.node_id != node_id)
            .collect::<Vec<_>>();
        if !peers.is_empty() {
            let internal_token = state.ha.internal_token().ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    "dual-active force promote requires HA_INTERNAL_TOKEN to verify peers"
                        .to_string(),
                )
            })?;
            let client = Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| Client::new());
            let mut peers_to_update = Vec::new();
            for peer in peers {
                match fetch_internal_ha_status(&client, &peer, &internal_token, &node_id).await {
                    Ok(peer_status) => {
                        if peer_status.allows_full_writes
                            || peer_status.full_master_node_id.as_deref()
                                == Some(peer.node_id.as_str())
                        {
                            return Err((
                                StatusCode::CONFLICT,
                                format!(
                                    "dual-active force promote refused because peer {} still allows full writes; use planned cutover",
                                    peer.node_id
                                ),
                            ));
                        }
                        peers_to_update.push(peer);
                    }
                    Err(err) => {
                        tracing::warn!(
                            component = "ha",
                            event = "dual_active_force_promote_peer_probe_failed",
                            peer_node_id = peer.node_id,
                            err = %err,
                            "HA dual-active force promote could not probe peer"
                        );
                    }
                }
            }
            for peer in peers_to_update {
                post_internal_ha_leader_update(
                    &client,
                    &peer,
                    &internal_token,
                    &node_id,
                    None,
                )
                .await
                .map_err(|err| (StatusCode::BAD_GATEWAY, err))?;
            }
        }
        state
            .proxy
            .set_ha_full_master_node_id(&node_id)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        state
            .ha
            .apply_dual_active_leader(Some(node_id.clone()))
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
        let status = state.ha.status().await;
        persist_ha_status_snapshot(&state, &status).await?;
        record_ha_control_plane_event(
            &state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: "manual_promote".to_string(),
                category: tavily_hikari::HaControlPlaneEventCategory::ManualFailover,
                status: tavily_hikari::HaControlPlaneEventStatus::Success,
                node_id: Some(node_id.clone()),
                operation_id: None,
                summary: "Force promote updated dual-active leader key".to_string(),
                detail: status.message.clone(),
                technical_details: Some(json!({
                    "fromRole": before.role,
                    "toRole": status.role,
                    "dualActive": true,
                    "force": true,
                })),
            },
        )
        .await?;
        return Ok(Json(status));
    }
    let before = state.ha.status().await;
    let result = state
        .ha
        .promote_self_to_provisional_with_audit(payload.force.unwrap_or(false))
        .await;
    let (status, audit_entries) = result.map_err(|err| (StatusCode::CONFLICT, err))?;
    let node_id = status.node_id.clone();
    let edgeone_origin = status.edgeone_origin.clone();
    let source_effective = status.ha_source_effective.clone();
    let message = status.message.clone();
    let operation = tavily_hikari::HaFailoverOperationRecord {
        operation_id: format!(
            "promote-{}-{}",
            node_id,
            state.proxy.backend_time().now_ts()
        ),
        operation_kind: "promote".to_string(),
        target_node_id: Some(node_id.clone()),
        from_origin: before.edgeone_origin,
        to_origin: status.edgeone_current_target.clone(),
        status: "provisional_master".to_string(),
        message: message.clone(),
    };
    state
        .proxy
        .insert_ha_failover_operation(&operation)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let _ = (edgeone_origin, source_effective, message);
    persist_ha_status_snapshot(&state, &status).await?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "manual_promote".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::ManualFailover,
            status: tavily_hikari::HaControlPlaneEventStatus::Success,
            node_id: Some(node_id),
            operation_id: Some(operation.operation_id.clone()),
            summary: "Manual promote switched this node into provisional_master".to_string(),
            detail: status.message.clone(),
            technical_details: Some(json!({
                "fromOrigin": operation.from_origin,
                "toOrigin": operation.to_origin,
                "role": status.role,
            })),
        },
    )
    .await?;
    record_edgeone_audit_entries(&state, &operation.operation_id, &audit_entries).await?;
    Ok(Json(status))
}

async fn post_admin_ha_finalize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if state.ha.dual_active_enabled() {
        return Err((
            StatusCode::CONFLICT,
            "finalize is disabled in dual-active mode; update the leader key instead".to_string(),
        ));
    }
    let status = state
        .ha
        .finalize_failover()
        .await
        .map_err(|err| (StatusCode::CONFLICT, err))?;
    let node_id = status.node_id.clone();
    persist_ha_status_snapshot(&state, &status).await?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "manual_finalize".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::ManualFailover,
            status: tavily_hikari::HaControlPlaneEventStatus::Success,
            node_id: Some(node_id),
            operation_id: None,
            summary: "Manual finalize completed failover on this node".to_string(),
            detail: status.message.clone(),
            technical_details: Some(json!({
                "role": status.role,
            })),
        },
    )
    .await?;
    Ok(Json(status))
}

async fn post_internal_ha_finalize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<InternalHaFinalizeRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_ha_internal_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if state.ha.dual_active_enabled() {
        return Err((
            StatusCode::CONFLICT,
            "finalize is disabled in dual-active mode; update the leader key instead".to_string(),
        ));
    }
    let status = state
        .ha
        .finalize_failover()
        .await
        .map_err(|err| (StatusCode::CONFLICT, err))?;
    persist_ha_status_snapshot(&state, &status).await?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "planned_cutover_finalize".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
            status: tavily_hikari::HaControlPlaneEventStatus::Success,
            node_id: Some(status.node_id.clone()),
            operation_id: payload.operation_id.clone(),
            summary: "Internal planned cutover finalize promoted this node to full_master"
                .to_string(),
            detail: status.message.clone(),
            technical_details: Some(json!({
                "role": status.role,
            })),
        },
    )
    .await?;
    Ok(Json(status))
}

async fn post_internal_ha_leader(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<InternalHaLeaderUpdateRequest>,
) -> Result<Json<tavily_hikari::HaStatusView>, (StatusCode, String)> {
    if !is_ha_internal_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if !state.ha.dual_active_enabled() {
        return Err((
            StatusCode::CONFLICT,
            "internal leader update requires dual-active mode".to_string(),
        ));
    }
    state
        .proxy
        .set_ha_full_master_node_id(&payload.full_master_node_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    state
        .ha
        .apply_dual_active_leader(Some(payload.full_master_node_id.clone()))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let status = state.ha.status().await;
    if let Err((status_code, err)) = persist_ha_status_snapshot(&state, &status).await {
        tracing::warn!(
            component = "ha",
            event = "internal_dual_active_leader_snapshot_persist_failed",
            http_status = status_code.as_u16(),
            err = %err,
            "HA internal dual-active leader snapshot persist warning"
        );
    }
    if let Err((status_code, err)) = record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "internal_dual_active_leader_update".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
            status: tavily_hikari::HaControlPlaneEventStatus::Success,
            node_id: Some(status.node_id.clone()),
            operation_id: payload.operation_id,
            summary: format!(
                "Internal dual-active leader update applied {}",
                payload.full_master_node_id
            ),
            detail: status.message.clone(),
            technical_details: Some(json!({
                "fullMasterNodeId": payload.full_master_node_id,
                "role": status.role,
            })),
        },
    )
    .await
    {
        tracing::warn!(
            component = "ha",
            event = "internal_dual_active_leader_event_persist_failed",
            http_status = status_code.as_u16(),
            err = %err,
            "HA internal dual-active leader event persist warning"
        );
    }
    Ok(Json(status))
}

async fn post_admin_ha_recovery_import(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaRecoveryImportRequest>,
) -> Result<Json<tavily_hikari::HaRecoveryImportResult>, (StatusCode, String)> {
    if !is_ha_admin_or_internal(state.as_ref(), &headers).await {
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
    async {
        state
            .proxy
            .persist_ha_node_state(
                &status.node_id,
                status.role,
                status.edgeone_origin.as_deref(),
                status.ha_source_effective.as_ref(),
                status.message.as_deref(),
            )
            .await?;
        state.proxy.flush_ha_state_writes().await
    }
    .await
    .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "recovery_import".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::Recovery,
            status: if imported {
                tavily_hikari::HaControlPlaneEventStatus::Success
            } else {
                tavily_hikari::HaControlPlaneEventStatus::Info
            },
            node_id: Some(status.node_id.clone()),
            operation_id: None,
            summary: if imported {
                format!("Recovery import applied batch {batch}")
            } else {
                format!("Recovery import reused existing batch {batch}")
            },
            detail: Some(message.clone()),
            technical_details: Some(json!({
                "batchId": batch,
                "sourceNodeId": source,
                "imported": imported,
                "eventCount": imported_event_count,
            })),
        },
    )
    .await?;
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

async fn get_admin_ha_timeline(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HaTimelineQuery>,
) -> Result<Json<tavily_hikari::HaTimelinePage>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let category = parse_timeline_category(query.category.as_deref())?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let events = state
        .proxy
        .list_ha_control_plane_events(
            query.cursor,
            limit + 1,
            query.node_id.as_deref(),
            category,
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut events = events;
    let next_cursor = if events.len() as i64 > limit {
        events.pop().expect("timeline extra event exists");
        events.last().map(|event| event.id)
    } else {
        None
    };
    Ok(Json(tavily_hikari::HaTimelinePage { events, next_cursor }))
}

async fn get_admin_ha_node_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Query(query): Query<HaNodeDetailQuery>,
) -> Result<Json<tavily_hikari::HaNodeDetailView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let status = build_admin_ha_status(&state).await;
    let node = status
        .peer_nodes
        .iter()
        .find(|peer| peer.node_id == node_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown HA peer node: {node_id}")))?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let mut events = state
        .proxy
        .list_ha_control_plane_events_for_node_interactions(query.cursor, limit, &node_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let next_cursor = if events.len() > limit as usize {
        events.pop().expect("node detail extra event exists");
        events.last().map(|event| event.id)
    } else {
        None
    };
    Ok(Json(tavily_hikari::HaNodeDetailView {
        current_node_id: status.node_id,
        node,
        timeline: tavily_hikari::HaTimelinePage { events, next_cursor },
    }))
}

async fn post_admin_ha_planned_cutover(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<HaPlannedCutoverRequest>,
) -> Result<Json<PlannedCutoverResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if state.ha.dual_active_enabled() {
        let operation_id = format!("dual-active-{}", state.proxy.backend_time().now_ts());
        let local_before = build_internal_ha_status(&state).await;
        let local_node_id = local_before.node_id.clone();
        if local_before.role != tavily_hikari::HaNodeRole::FullMaster {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "planned cutover requires full_master, current role is {}",
                    local_before.role.as_str()
                ),
            ));
        }
        let peer = state
            .ha
            .peer_nodes()
            .into_iter()
            .find(|peer| peer.node_id == payload.target_node_id)
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    format!("unknown planned cutover target: {}", payload.target_node_id),
                )
            })?;
        if peer.role_hint != tavily_hikari::HaPeerRoleHint::StandbyCandidate {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "planned cutover target {} is not a standby_candidate",
                    peer.node_id
                ),
            ));
        }
        let internal_token = state
            .ha
            .internal_token()
            .ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    "planned cutover requires HA_INTERNAL_TOKEN".to_string(),
                )
            })?;
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| Client::new());
        let peer_before = fetch_internal_ha_status(&client, &peer, &internal_token, &local_node_id)
            .await
            .map_err(|err| (StatusCode::BAD_GATEWAY, err))?;
        let peer_last_probe = latest_probe_timestamp(&peer_before).ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!("peer {} has never reported a status probe", peer.node_id),
            )
        })?;
        if state.proxy.backend_time().now_ts().saturating_sub(peer_last_probe) > HA_PEER_STALE_SECS {
            return Err((
                StatusCode::CONFLICT,
                format!("peer {} is stale for planned cutover", peer.node_id),
            ));
        }
        if peer_before.role != tavily_hikari::HaNodeRole::Standby
            || peer_before.recovery_status.is_some()
            || peer_before
                .sync_lag_seconds
                .is_none_or(|value| value > HA_PLANNED_CUTOVER_MAX_LAG_SECS)
        {
            return Err((
                StatusCode::CONFLICT,
                format!("peer {} failed planned cutover precheck", peer.node_id),
            ));
        }
        let local_fence = async {
            state
                .proxy
                .set_ha_full_master_node_id(&peer.node_id)
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            state
                .ha
                .apply_dual_active_leader(Some(peer.node_id.clone()))
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
            let local_after = state.ha.status().await;
            persist_ha_status_snapshot(&state, &local_after).await?;
            Ok::<tavily_hikari::HaStatusView, (StatusCode, String)>(local_after)
        }
        .await;
        let local_after = match local_fence {
            Ok(status) => status,
            Err((status, err)) => {
                let rollback_detail = match async {
                    state
                        .proxy
                        .set_ha_full_master_node_id(&local_node_id)
                        .await
                        .map_err(|rollback_err| {
                            (StatusCode::INTERNAL_SERVER_ERROR, rollback_err.to_string())
                        })?;
                    state
                        .ha
                        .apply_dual_active_leader(Some(local_node_id.clone()))
                        .await
                        .map_err(|rollback_err| {
                            (StatusCode::INTERNAL_SERVER_ERROR, rollback_err)
                        })?;
                    let rollback_status = state.ha.status().await;
                    persist_ha_status_snapshot(&state, &rollback_status).await?;
                    Ok::<(), (StatusCode, String)>(())
                }
                .await
                {
                    Ok(()) => "local rollback applied".to_string(),
                    Err((rollback_status, rollback_err)) => format!(
                        "local rollback failed after fencing failure ({rollback_status}): {rollback_err}"
                    ),
                };
                return Err((
                    status,
                    format!("dual-active local fencing failed: {err}; {rollback_detail}"),
                ));
            }
        };
        let peer_after = match post_internal_ha_leader_update(
            &client,
            &peer,
            &internal_token,
            &peer.node_id,
            Some(&operation_id),
        )
        .await
        {
            Ok(status) => status,
            Err(err) => {
                match fetch_internal_ha_status(&client, &peer, &internal_token, &local_node_id).await {
                    Ok(peer_status) => {
                        let peer_promoted = peer_status.full_master_node_id.as_deref()
                            == Some(peer.node_id.as_str())
                            || peer_status.role == tavily_hikari::HaNodeRole::FullMaster
                            || peer_status.allows_full_writes;
                        if peer_promoted {
                            return Err((
                                StatusCode::BAD_GATEWAY,
                                format!(
                                    "{err}; peer {} already reports full_master after ambiguous response, local node remains fenced",
                                    peer.node_id
                                ),
                            ));
                        }
                    }
                    Err(reconcile_err) => {
                        return Err((
                            StatusCode::BAD_GATEWAY,
                            format!(
                                "{err}; peer reconciliation failed ({reconcile_err}); local node remains fenced"
                            ),
                        ));
                    }
                }
                let rollback_detail = match async {
                    state
                        .proxy
                        .set_ha_full_master_node_id(&local_node_id)
                        .await
                        .map_err(|rollback_err| {
                            (StatusCode::INTERNAL_SERVER_ERROR, rollback_err.to_string())
                        })?;
                    state
                        .ha
                        .apply_dual_active_leader(Some(local_node_id.clone()))
                        .await
                        .map_err(|rollback_err| {
                            (StatusCode::INTERNAL_SERVER_ERROR, rollback_err)
                        })?;
                    let rollback_status = state.ha.status().await;
                    persist_ha_status_snapshot(&state, &rollback_status).await?;
                    Ok::<(), (StatusCode, String)>(())
                }
                .await
                {
                    Ok(()) => "local rollback applied".to_string(),
                    Err((rollback_status, rollback_err)) => format!(
                        "local rollback failed after peer update failure ({rollback_status}): {rollback_err}"
                    ),
                };
                return Err((
                    StatusCode::BAD_GATEWAY,
                    format!(
                        "{err}; peer status after error confirmed no promotion; {rollback_detail}"
                    ),
                ));
            }
        };
        record_ha_control_plane_event(
            &state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: "planned_cutover_succeeded".to_string(),
                category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                status: tavily_hikari::HaControlPlaneEventStatus::Success,
                node_id: Some(peer.node_id.clone()),
                operation_id: Some(operation_id.clone()),
                summary: format!("Planned cutover updated leader to {}", peer.node_id),
                detail: local_after.message.clone(),
                technical_details: Some(json!({
                    "targetNodeId": peer.node_id,
                    "dualActive": true,
                    "localRole": local_after.role,
                    "peerRole": peer_after.role,
                })),
            },
        )
        .await?;
        return Ok(Json(PlannedCutoverResponse {
            operation_id,
            status: "success".to_string(),
            detail: None,
        }));
    }
    let local_before = build_internal_ha_status(&state).await;
    if local_before.role != tavily_hikari::HaNodeRole::FullMaster {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "planned cutover requires full_master, current role is {}",
                local_before.role.as_str()
            ),
        ));
    }
    let internal_token = state
        .ha
        .internal_token()
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                "planned cutover requires HA_INTERNAL_TOKEN".to_string(),
            )
        })?;
    let peer = state
        .ha
        .peer_nodes()
        .into_iter()
        .find(|peer| peer.node_id == payload.target_node_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("unknown planned cutover target: {}", payload.target_node_id),
            )
        })?;
    if peer.role_hint != tavily_hikari::HaPeerRoleHint::StandbyCandidate {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "planned cutover target {} is not a standby_candidate",
                peer.node_id
            ),
        ));
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| Client::new());
    let operation_id = format!(
        "planned-cutover-{}-{}",
        peer.node_id,
        state.proxy.backend_time().now_ts()
    );
    let peer_before = fetch_internal_ha_status(&client, &peer, &internal_token, &local_before.node_id)
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err))?;
    let peer_last_probe = latest_probe_timestamp(&peer_before).ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            format!("peer {} has never reported a status probe", peer.node_id),
        )
    })?;
    if state.proxy.backend_time().now_ts().saturating_sub(peer_last_probe) > HA_PEER_STALE_SECS {
        record_ha_control_plane_event(
            &state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: "planned_cutover_rejected_stale".to_string(),
                category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                status: tavily_hikari::HaControlPlaneEventStatus::Warning,
                node_id: Some(peer.node_id.clone()),
                operation_id: Some(operation_id.clone()),
                summary: format!(
                    "Planned cutover rejected because peer {} is stale",
                    peer.node_id
                ),
                detail: peer_before.message.clone(),
                technical_details: Some(json!({
                    "lastProbeAt": peer_last_probe,
                    "syncLagSeconds": peer_before.sync_lag_seconds,
                })),
            },
        )
        .await?;
        return Err((
            StatusCode::CONFLICT,
            format!("peer {} is stale for planned cutover", peer.node_id),
        ));
    }
    if peer_before.role != tavily_hikari::HaNodeRole::Standby
        || peer_before.recovery_status.is_some()
        || peer_before
            .sync_lag_seconds
            .is_none_or(|value| value > HA_PLANNED_CUTOVER_MAX_LAG_SECS)
    {
        record_ha_control_plane_event(
            &state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: "planned_cutover_rejected_precheck".to_string(),
                category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                status: tavily_hikari::HaControlPlaneEventStatus::Warning,
                node_id: Some(peer.node_id.clone()),
                operation_id: Some(operation_id.clone()),
                summary: format!(
                    "Planned cutover precheck rejected target {}",
                    peer.node_id
                ),
                detail: peer_before.message.clone(),
                technical_details: Some(json!({
                    "role": peer_before.role,
                    "recoveryStatus": peer_before.recovery_status,
                    "syncLagSeconds": peer_before.sync_lag_seconds,
                })),
            },
        )
        .await?;
        return Err((
            StatusCode::CONFLICT,
            format!("peer {} failed planned cutover precheck", peer.node_id),
        ));
    }
    let target_settings = source_settings_from_view(
        peer_before
            .ha_source_effective
            .as_ref()
            .ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    format!("peer {} is missing ha_source_effective", peer.node_id),
                )
            })?,
    )
    .map_err(|err| (StatusCode::CONFLICT, err))?;
    let operation = tavily_hikari::HaFailoverOperationRecord {
        operation_id: operation_id.clone(),
        operation_kind: "planned_cutover".to_string(),
        target_node_id: Some(peer.node_id.clone()),
        from_origin: local_before.edgeone_origin.clone(),
        to_origin: target_settings.effective_target(),
        status: "running".to_string(),
        message: Some(format!("planned cutover to {}", peer.node_id)),
    };
    state
        .proxy
        .insert_ha_failover_operation(&operation)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "planned_cutover_started".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
            status: tavily_hikari::HaControlPlaneEventStatus::Running,
            node_id: Some(peer.node_id.clone()),
            operation_id: Some(operation_id.clone()),
            summary: format!(
                "Planned cutover started from {} to {}",
                local_before.node_id, peer.node_id
            ),
            detail: Some(format!(
                "EdgeOne target will move from {:?} to {:?}",
                local_before.edgeone_origin,
                target_settings.effective_target()
            )),
            technical_details: Some(json!({
                "fromNodeId": local_before.node_id,
                "targetNodeId": peer.node_id,
                "fromOrigin": local_before.edgeone_origin,
                "toTarget": target_settings.effective_target(),
            })),
        },
    )
    .await?;
    let (_status_after_switch, audit_entries) = state
        .ha
        .switch_edgeone_target_with_audit(target_settings.clone())
        .await
        .map_err(|err| (StatusCode::CONFLICT, err))?;
    record_edgeone_audit_entries(&state, &operation_id, &audit_entries).await?;
    let ingress_already_switched_detail =
        format!("EdgeOne ingress already switched to {}; complete recovery reconciliation on both nodes before retrying.", peer.node_id);
    let deadline = Instant::now() + Duration::from_secs(HA_PLANNED_CUTOVER_POLL_TIMEOUT_SECS);
    let peer_after = loop {
        let current = fetch_internal_ha_status(&client, &peer, &internal_token, &local_before.node_id)
            .await
            .map_err(|err| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("{err} Ingress already moved to {}; do not retry blindly.", peer.node_id),
                )
            })?;
        if current.role == tavily_hikari::HaNodeRole::ProvisionalMaster {
            break current;
        }
        if Instant::now() >= deadline {
            let _ = record_ha_control_plane_event(
                &state,
                tavily_hikari::HaControlPlaneEventInsert {
                    event_kind: "planned_cutover_reconcile_required".to_string(),
                    category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                    status: tavily_hikari::HaControlPlaneEventStatus::Error,
                    node_id: Some(peer.node_id.clone()),
                    operation_id: Some(operation_id.clone()),
                    summary: format!(
                        "Planned cutover moved ingress to {} but peer did not acknowledge provisional_master in time",
                        peer.node_id
                    ),
                    detail: Some(ingress_already_switched_detail.clone()),
                    technical_details: Some(json!({
                        "targetNodeId": peer.node_id,
                        "toTarget": target_settings.effective_target(),
                    })),
                },
            )
            .await;
            return Err((
                StatusCode::GATEWAY_TIMEOUT,
                format!(
                    "planned cutover timed out waiting for peer {} to enter provisional_master. {}",
                    peer.node_id, ingress_already_switched_detail
                ),
            ));
        }
        state.proxy.backend_time().sleep(Duration::from_secs(1)).await;
    };
    let finalize_response = client
        .post(format!("{}/api/internal/ha/finalize", peer.admin_base_url))
        .header("x-ha-internal-token", &internal_token)
        .json(&InternalHaFinalizeRequest {
            operation_id: Some(operation_id.clone()),
        })
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("peer finalize failed: {err}")))?;
    if !finalize_response.status().is_success() {
        let status = finalize_response.status();
        let detail = finalize_response.text().await.unwrap_or_default();
        let _ = record_ha_control_plane_event(
            &state,
            tavily_hikari::HaControlPlaneEventInsert {
                event_kind: "planned_cutover_finalize_failed".to_string(),
                category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                status: tavily_hikari::HaControlPlaneEventStatus::Error,
                node_id: Some(peer.node_id.clone()),
                operation_id: Some(operation_id.clone()),
                summary: format!(
                    "Planned cutover moved ingress to {} but internal finalize failed",
                    peer.node_id
                ),
                detail: Some(format!("{ingress_already_switched_detail} Finalize returned {status}: {detail}")),
                technical_details: Some(json!({
                    "targetNodeId": peer.node_id,
                    "httpStatus": status.as_u16(),
                    "response": detail,
                })),
            },
        )
        .await;
        return Err((
            StatusCode::BAD_GATEWAY,
            format!(
                "peer finalize failed with {status}: {detail}. {}",
                ingress_already_switched_detail
            ),
        ));
    }
    let _peer_finalized = finalize_response
        .json::<tavily_hikari::HaStatusView>()
        .await
        .map_err(|err| {
            (
                StatusCode::BAD_GATEWAY,
                format!(
                    "peer finalize response invalid: {err}. {}",
                    ingress_already_switched_detail
                ),
            )
        })?;
    let local_after = loop {
        let current = state
            .ha
            .refresh_authoritative_role()
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
        if current.role == tavily_hikari::HaNodeRole::Recovery {
            break current;
        }
        if Instant::now() >= deadline {
            let _ = record_ha_control_plane_event(
                &state,
                tavily_hikari::HaControlPlaneEventInsert {
                    event_kind: "planned_cutover_local_recovery_timeout".to_string(),
                    category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
                    status: tavily_hikari::HaControlPlaneEventStatus::Error,
                    node_id: Some(local_before.node_id.clone()),
                    operation_id: Some(operation_id.clone()),
                    summary: "Planned cutover switched ingress but local node did not enter recovery in time"
                        .to_string(),
                    detail: Some(ingress_already_switched_detail.clone()),
                    technical_details: Some(json!({
                        "currentNodeId": local_before.node_id,
                        "targetNodeId": peer.node_id,
                    })),
                },
            )
            .await;
            return Err((
                StatusCode::GATEWAY_TIMEOUT,
                format!(
                    "planned cutover timed out waiting for local node to enter recovery. {}",
                    ingress_already_switched_detail
                ),
            ));
        }
        state.proxy.backend_time().sleep(Duration::from_secs(1)).await;
    };
    persist_ha_status_snapshot(&state, &local_after).await?;
    state
        .proxy
        .insert_ha_failover_operation(&tavily_hikari::HaFailoverOperationRecord {
            operation_id: operation_id.clone(),
            operation_kind: "planned_cutover".to_string(),
            target_node_id: Some(peer.node_id.clone()),
            from_origin: operation.from_origin.clone(),
            to_origin: operation.to_origin.clone(),
            status: "success".to_string(),
            message: Some(format!("planned cutover completed to {}", peer.node_id)),
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    record_ha_control_plane_event(
        &state,
        tavily_hikari::HaControlPlaneEventInsert {
            event_kind: "planned_cutover_succeeded".to_string(),
            category: tavily_hikari::HaControlPlaneEventCategory::PlannedCutover,
            status: tavily_hikari::HaControlPlaneEventStatus::Success,
            node_id: Some(peer.node_id.clone()),
            operation_id: Some(operation_id.clone()),
            summary: format!("Planned cutover completed to {}", peer.node_id),
            detail: Some(format!(
                "{} became full_master and {} entered recovery",
                peer.node_id, local_after.node_id
            )),
            technical_details: Some(json!({
                "targetNodeId": peer.node_id,
                "peerRoleBeforeFinalize": peer_after.role,
                "localRoleAfter": local_after.role,
            })),
        },
    )
    .await?;
    Ok(Json(PlannedCutoverResponse {
        operation_id,
        status: "success".to_string(),
        detail: None,
    }))
}
