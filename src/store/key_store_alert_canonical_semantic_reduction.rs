const ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_EVENT_ROWS: i64 = 8;
const ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_CHUNKS_PER_TX: usize = 16;
// A canonical group is a summary. Keep inline history only while it fits in
// one existing read fragment; the drawer already loads request records from
// its paginated source when an administrator asks to inspect a child window.
const ADMIN_ALERT_CANONICAL_INLINE_CHILD_EVENTS_MAX_BYTES: i64 =
    ALERT_EVENT_PROJECTION_MAX_BYTES as i64;

fn append_bounded_semantic_payload_chunks(
    payload: &str,
    byte_offset: &mut usize,
    chunks: &mut Vec<String>,
) -> bool {
    let Some(remaining) = payload.get(*byte_offset..) else {
        return false;
    };
    let mut iterator = canonical_alert_payload_chunks_iter(remaining);
    while chunks.len() < ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_CHUNKS_PER_TX {
        let Some(chunk) = iterator.next() else {
            *byte_offset = payload.len();
            return true;
        };
        *byte_offset += chunk.len();
        chunks.push(chunk);
    }
    iterator.next().is_none()
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SemanticChildReduction {
    ordinal: i64,
    mother_ordinal: i64,
    first_event_position: i64,
    last_event_position: i64,
    first_seen: i64,
    last_seen: i64,
    event_count: i64,
    latest_event: AlertEventRecord,
    semantic_kind: String,
    semantic_window_minutes: Option<i64>,
    semantic_window_start: Option<i64>,
    semantic_window_end: Option<i64>,
    semantic_window_key: Option<String>,
    window_group_key: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SemanticMotherReduction {
    ordinal: i64,
    first_child_ordinal: i64,
    last_child_ordinal: i64,
    first_seen: i64,
    last_seen: i64,
    event_count: i64,
    child_count: i64,
    latest_event: AlertEventRecord,
    semantic_kind: String,
    semantic_window_minutes: Option<i64>,
    semantic_window_start: Option<i64>,
    semantic_window_end: Option<i64>,
    semantic_window_key: Option<String>,
    last_child_boundary_end: i64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SemanticClassifyingProgress {
    next_event_position: i64,
    next_child_ordinal: i64,
    next_mother_ordinal: i64,
    active_child: Option<SemanticChildReduction>,
    active_mother: Option<SemanticMotherReduction>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SemanticOutputProgress {
    mother_ordinal: i64,
    child_ordinal: i64,
    next_event_position: i64,
    next_chunk_position: i64,
    #[serde(default)]
    stage_chunk_position: i64,
    #[serde(default)]
    stage_event_position: i64,
    #[serde(default)]
    stage_event_byte_offset: i64,
    #[serde(default)]
    inline_child_event_bytes: i64,
    #[serde(default)]
    current_child_event_bytes: i64,
    #[serde(default)]
    omit_current_child_events: bool,
    output_position: i64,
    stage: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
enum SemanticReductionProgress {
    Classifying(SemanticClassifyingProgress),
    Outputting(SemanticOutputProgress),
}

impl Default for SemanticClassifyingProgress {
    fn default() -> Self {
        Self {
            next_event_position: 1,
            next_child_ordinal: 1,
            next_mother_ordinal: 1,
            active_child: None,
            active_mother: None,
        }
    }
}

impl KeyStore {
    async fn admin_alert_canonical_semantic_child_event_bytes(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        child_ordinal: i64,
    ) -> Result<i64, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_scalar::<_, i64>(
            r#"SELECT COALESCE(SUM(length(event_json)), 0)
                  FROM observability.admin_alert_canonical_group_reduction_events
                 WHERE build_generation = ? AND partition_key = ? AND child_ordinal = ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(child_ordinal)
        .fetch_one(&mut *session)
        .await;
        let value = session.query(result).await;
        session.finish().await?;
        value
    }

    async fn advance_admin_alert_canonical_semantic_reduction(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let progress = serde_json::from_str::<SemanticReductionProgress>(
            &state.build_partition_events_json,
        )
        .unwrap_or_else(|_| SemanticReductionProgress::Classifying(Default::default()));
        match progress {
            SemanticReductionProgress::Classifying(progress) => {
                self.classify_admin_alert_canonical_semantic_fragment(snapshot, state, progress)
                    .await
            }
            SemanticReductionProgress::Outputting(progress) => {
                self.stream_admin_alert_canonical_semantic_output(snapshot, state, progress)
                    .await
            }
        }
    }

    fn semantic_child_from_event(
        ordinal: i64,
        event_position: i64,
        event: AlertEventRecord,
    ) -> Result<SemanticChildReduction, ProxyError> {
        let semantic = event.semantic_window.clone().ok_or_else(|| {
            ProxyError::Other("semantic canonical partition has no semantic window".to_string())
        })?;
        let semantic_kind = semantic.kind.as_str().to_string();
        let window_group_key = semantic.window_key.clone().unwrap_or_else(|| {
            format!("{}:{}", semantic_kind, event.occurred_at)
        });
        Ok(SemanticChildReduction {
            ordinal,
            mother_ordinal: 0,
            first_event_position: event_position,
            last_event_position: event_position,
            first_seen: event.occurred_at,
            last_seen: event.occurred_at,
            event_count: 1,
            latest_event: event,
            semantic_kind,
            semantic_window_minutes: semantic.window_minutes,
            semantic_window_start: semantic.window_start,
            semantic_window_end: semantic.window_end,
            semantic_window_key: semantic.window_key.clone(),
            window_group_key,
        })
    }

    fn semantic_child_starts_new_window(
        child: &SemanticChildReduction,
        event: &AlertEventRecord,
    ) -> Result<bool, ProxyError> {
        let semantic = event.semantic_window.as_ref().ok_or_else(|| {
            ProxyError::Other("semantic canonical partition has no semantic window".to_string())
        })?;
        if child.semantic_kind == "request_rate" {
            let threshold = child.semantic_window_minutes.unwrap_or(5) * 60;
            return Ok(event.occurred_at.saturating_sub(child.last_seen) > threshold);
        }
        Ok(child.window_group_key
            != semantic
                .window_key
                .clone()
                .unwrap_or_else(|| format!("{}:{}", semantic.kind.as_str(), event.occurred_at)))
    }

    fn extend_semantic_child(child: &mut SemanticChildReduction, event_position: i64, event: AlertEventRecord) {
        child.last_event_position = event_position;
        child.last_seen = event.occurred_at;
        child.event_count += 1;
        if child.semantic_kind == "request_rate"
            && let Some(semantic) = event.semantic_window.as_ref()
        {
            child.semantic_window_start = match (
                child.semantic_window_start,
                semantic.window_start,
            ) {
                (Some(current), Some(next)) => Some(current.min(next)),
                (current, next) => current.or(next),
            };
            child.semantic_window_end = match (child.semantic_window_end, semantic.window_end) {
                (Some(current), Some(next)) => Some(current.max(next)),
                (current, next) => current.or(next),
            };
        }
        child.latest_event = event;
    }

    fn semantic_mother_starts_new_chain(
        mother: &SemanticMotherReduction,
        child: &SemanticChildReduction,
    ) -> bool {
        if mother.semantic_kind != child.semantic_kind {
            return true;
        }
        match mother.semantic_kind.as_str() {
            "request_rate" => {
                let threshold = mother.semantic_window_minutes.unwrap_or(5) * 60;
                child
                    .semantic_window_start
                    .unwrap_or(child.first_seen)
                    .saturating_sub(mother.last_child_boundary_end)
                    > threshold
            }
            "rolling_hour" => child
                .semantic_window_start
                .unwrap_or(child.first_seen)
                .saturating_sub(mother.last_child_boundary_end)
                > 60 * 60,
            "day" | "month" => child
                .semantic_window_start
                .unwrap_or(child.first_seen)
                .saturating_sub(mother.last_child_boundary_end)
                > 1,
            _ => true,
        }
    }

    fn semantic_mother_from_child(ordinal: i64, child: &SemanticChildReduction) -> SemanticMotherReduction {
        SemanticMotherReduction {
            ordinal,
            first_child_ordinal: child.ordinal,
            last_child_ordinal: child.ordinal,
            first_seen: child.first_seen,
            last_seen: child.last_seen,
            event_count: child.event_count,
            child_count: 1,
            latest_event: child.latest_event.clone(),
            semantic_kind: child.semantic_kind.clone(),
            semantic_window_minutes: child.semantic_window_minutes,
            semantic_window_start: child.semantic_window_start,
            semantic_window_end: child.semantic_window_end,
            semantic_window_key: child.semantic_window_key.clone().or_else(|| {
                (child.semantic_kind == "request_rate").then(|| child.window_group_key.clone())
            }),
            last_child_boundary_end: child.semantic_window_end.unwrap_or(child.last_seen),
        }
    }

    fn extend_semantic_mother(mother: &mut SemanticMotherReduction, child: &SemanticChildReduction) {
        mother.last_child_ordinal = child.ordinal;
        mother.last_seen = child.last_seen;
        mother.event_count += child.event_count;
        mother.child_count += 1;
        mother.latest_event = child.latest_event.clone();
        mother.semantic_window_start = match (mother.semantic_window_start, child.semantic_window_start) {
            (Some(current), Some(next)) => Some(current.min(next)),
            (current, next) => current.or(next),
        };
        mother.semantic_window_end = match (mother.semantic_window_end, child.semantic_window_end) {
            (Some(current), Some(next)) => Some(current.max(next)),
            (current, next) => current.or(next),
        };
        mother.semantic_window_key = child.semantic_window_key.clone().or_else(|| {
            (child.semantic_kind == "request_rate").then(|| child.window_group_key.clone())
        });
        mother.last_child_boundary_end = child.semantic_window_end.unwrap_or(child.last_seen);
    }

    fn accept_semantic_child(
        progress: &mut SemanticClassifyingProgress,
        mut child: SemanticChildReduction,
        children: &mut Vec<SemanticChildReduction>,
        mothers: &mut Vec<SemanticMotherReduction>,
    ) {
        let starts_new_mother = progress
            .active_mother
            .as_ref()
            .is_none_or(|mother| Self::semantic_mother_starts_new_chain(mother, &child));
        if starts_new_mother {
            if let Some(mother) = progress.active_mother.take() {
                mothers.push(mother);
            }
            let ordinal = progress.next_mother_ordinal;
            progress.next_mother_ordinal += 1;
            child.mother_ordinal = ordinal;
            progress.active_mother = Some(Self::semantic_mother_from_child(ordinal, &child));
        } else if let Some(mother) = progress.active_mother.as_mut() {
            child.mother_ordinal = mother.ordinal;
            Self::extend_semantic_mother(mother, &child);
        }
        children.push(child);
    }

    async fn classify_admin_alert_canonical_semantic_fragment(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
        mut progress: SemanticClassifyingProgress,
    ) -> Result<(), ProxyError> {
        let fragment = self
            .read_admin_alert_canonical_groups_partition_fragment(
                snapshot,
                &state.build_partition_key,
                state.build_partition_finalize_fragment_position,
            )
            .await?;
        let prior_fragment_position = state.build_partition_finalize_fragment_position;
        let mut children = Vec::new();
        let mut mothers = Vec::new();
        let mut staged_events = Vec::new();
        let next_fragment_position = if let Some((position, events)) = fragment {
            for event in events {
                let event_position = progress.next_event_position;
                progress.next_event_position += 1;
                if progress
                    .active_child
                    .as_ref()
                    .map(|child| Self::semantic_child_starts_new_window(child, &event))
                    .transpose()?
                    .unwrap_or(false)
                    && let Some(child) = progress.active_child.take()
                {
                        Self::accept_semantic_child(&mut progress, child, &mut children, &mut mothers);
                }
                if progress.active_child.is_none() {
                    let ordinal = progress.next_child_ordinal;
                    progress.next_child_ordinal += 1;
                    progress.active_child = Some(Self::semantic_child_from_event(
                        ordinal,
                        event_position,
                        event.clone(),
                    )?);
                } else if let Some(child) = progress.active_child.as_mut() {
                    Self::extend_semantic_child(child, event_position, event.clone());
                }
                let child_ordinal = progress
                    .active_child
                    .as_ref()
                    .map(|child| child.ordinal)
                    .ok_or_else(|| ProxyError::Other("semantic reducer lost active child".to_string()))?;
                let (_, event_json) = serialize_alert_event_record_for_projection(event)?;
                staged_events.push((
                    event_position,
                    child_ordinal,
                    event_json,
                ));
            }
            position + 1
        } else {
            if let Some(child) = progress.active_child.take() {
                Self::accept_semantic_child(&mut progress, child, &mut children, &mut mothers);
            }
            if let Some(mother) = progress.active_mother.take() {
                mothers.push(mother);
            }
            let output = SemanticOutputProgress {
                mother_ordinal: 1,
                child_ordinal: 0,
                next_event_position: 0,
                next_chunk_position: 0,
                stage_chunk_position: 0,
                stage_event_position: 0,
                stage_event_byte_offset: 0,
                inline_child_event_bytes: 0,
                current_child_event_bytes: 0,
                omit_current_child_events: false,
                output_position: state.build_next_position,
                stage: "mother_header".to_string(),
            };
            let progress_json = serde_json::to_string(&SemanticReductionProgress::Outputting(output))
                .map_err(|error| ProxyError::Other(format!("serialize semantic output progress: {error}")))?;
            return self
                .commit_admin_alert_canonical_semantic_classification(
                    snapshot,
                    state,
                    progress_json,
                    prior_fragment_position,
                    staged_events,
                    children,
                    mothers,
                )
                .await;
        };
        let progress_json = serde_json::to_string(&SemanticReductionProgress::Classifying(progress))
            .map_err(|error| ProxyError::Other(format!("serialize semantic reduction progress: {error}")))?;
        self.commit_admin_alert_canonical_semantic_classification(
            snapshot,
            state,
            progress_json,
            next_fragment_position,
            staged_events,
            children,
            mothers,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_admin_alert_canonical_semantic_classification(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
        progress_json: String,
        next_fragment_position: i64,
        staged_events: Vec<(i64, i64, String)>,
        children: Vec<SemanticChildReduction>,
        mothers: Vec<SemanticMotherReduction>,
    ) -> Result<(), ProxyError> {
        let partition = state.build_partition_key.clone();
        let prior_progress_json = state.build_partition_events_json.clone();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    for (event_position, child_ordinal, event_json) in staged_events {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_reduction_events
                                   (build_generation, partition_key, event_position, child_ordinal, event_json)
                               VALUES (?, ?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(&partition)
                        .bind(event_position)
                        .bind(child_ordinal)
                        .bind(event_json)
                        .execute(&mut **tx)
                        .await?;
                    }
                    for child in children {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_reduction_children
                                   (build_generation, partition_key, child_ordinal, mother_ordinal,
                                    first_event_position, last_event_position, first_seen, last_seen,
                                    event_count, latest_event_json, semantic_kind,
                                    semantic_window_minutes, semantic_window_start, semantic_window_end,
                                    semantic_window_key)
                               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(&partition)
                        .bind(child.ordinal)
                        .bind(child.mother_ordinal)
                        .bind(child.first_event_position)
                        .bind(child.last_event_position)
                        .bind(child.first_seen)
                        .bind(child.last_seen)
                        .bind(child.event_count)
                        .bind(serialize_alert_event_record_for_projection(child.latest_event.clone())?.1)
                        .bind(child.semantic_kind)
                        .bind(child.semantic_window_minutes)
                        .bind(child.semantic_window_start)
                        .bind(child.semantic_window_end)
                        .bind(child.semantic_window_key)
                        .execute(&mut **tx)
                        .await?;
                    }
                    for mother in mothers {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_reduction_mothers
                                   (build_generation, partition_key, mother_ordinal,
                                    first_child_ordinal, last_child_ordinal, first_seen, last_seen,
                                    event_count, child_count, latest_event_json, semantic_kind,
                                    semantic_window_minutes, semantic_window_start, semantic_window_end,
                                    semantic_window_key)
                               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(&partition)
                        .bind(mother.ordinal)
                        .bind(mother.first_child_ordinal)
                        .bind(mother.last_child_ordinal)
                        .bind(mother.first_seen)
                        .bind(mother.last_seen)
                        .bind(mother.event_count)
                        .bind(mother.child_count)
                        .bind(serialize_alert_event_record_for_projection(mother.latest_event.clone())?.1)
                        .bind(mother.semantic_kind.clone())
                        .bind(mother.semantic_window_minutes)
                        .bind(mother.semantic_window_start)
                        .bind(mother.semantic_window_end)
                        .bind(mother.semantic_window_key.clone())
                        .execute(&mut **tx)
                        .await?;

                        // Keep the in-flight generation observable for reclaim and restart
                        // recovery. Its payload remains unpublished until output chunks exist.
                        let group = Self::semantic_mother_group_record(&mother);
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups
                                   (build_generation, position, last_seen, total_count,
                                    alert_type, group_id, payload_json)
                               VALUES (?, ?, ?, ?, ?, ?, '')"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(state.build_next_position + mother.ordinal - 1)
                        .bind(group.last_seen)
                        .bind(group.count)
                        .bind(group.alert_type)
                        .bind(group.id)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_events_json = ?,
                                  build_partition_finalize_fragment_position = ?
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ? AND build_phase = 'aggregating'
                              AND build_partition_key = ?
                              AND build_partition_finalize_fragment_position = ?
                              AND build_partition_events_json = ?
                              AND build_source_recent_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_state
                              )
                              AND build_source_history_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_history_state
                              )"#,
                    )
                    .bind(progress_json)
                    .bind(next_fragment_position)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(&partition)
                    .bind(state.build_partition_finalize_fragment_position)
                    .bind(prior_progress_json)
                    .execute(&mut **tx)
                    .await?;
                    if changed.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_build_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(())
                })
            })
            .await?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }

    async fn read_admin_alert_canonical_semantic_child(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        ordinal: i64,
    ) -> Result<Option<SemanticChildReduction>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query(
            r#"SELECT child_ordinal, mother_ordinal, first_event_position, last_event_position,
                       first_seen, last_seen, event_count, latest_event_json, semantic_kind,
                       semantic_window_minutes, semantic_window_start, semantic_window_end,
                       semantic_window_key
                  FROM observability.admin_alert_canonical_group_reduction_children
                 WHERE build_generation = ? AND partition_key = ? AND child_ordinal = ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(ordinal)
        .fetch_optional(&mut *session)
        .await;
        let row = session.query(result).await?;
        session.finish().await?;
        row.map(|row| {
            let latest_event_json: String = row.try_get("latest_event_json")?;
            let latest_event = serde_json::from_str(&latest_event_json).map_err(|_| {
                ProxyError::Other("invalid semantic child latest event".to_string())
            })?;
            let semantic_kind: String = row.try_get("semantic_kind")?;
            let first_seen: i64 = row.try_get("first_seen")?;
            let last_seen: i64 = row.try_get("last_seen")?;
            Ok(SemanticChildReduction {
                ordinal: row.try_get("child_ordinal")?,
                mother_ordinal: row.try_get("mother_ordinal")?,
                first_event_position: row.try_get("first_event_position")?,
                last_event_position: row.try_get("last_event_position")?,
                first_seen,
                last_seen,
                event_count: row.try_get("event_count")?,
                latest_event,
                semantic_kind: semantic_kind.clone(),
                semantic_window_minutes: row.try_get("semantic_window_minutes")?,
                semantic_window_start: row.try_get("semantic_window_start")?,
                semantic_window_end: row.try_get("semantic_window_end")?,
                semantic_window_key: row.try_get("semantic_window_key")?,
                window_group_key: row
                    .try_get::<Option<String>, _>("semantic_window_key")?
                    .unwrap_or_else(|| format!("{semantic_kind}:{first_seen}")),
            })
        })
        .transpose()
    }

    async fn read_admin_alert_canonical_semantic_mother(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        ordinal: i64,
    ) -> Result<Option<SemanticMotherReduction>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query(
            r#"SELECT mother_ordinal, first_child_ordinal, last_child_ordinal,
                       first_seen, last_seen, event_count, child_count, latest_event_json,
                       semantic_kind, semantic_window_minutes, semantic_window_start,
                       semantic_window_end, semantic_window_key
                  FROM observability.admin_alert_canonical_group_reduction_mothers
                 WHERE build_generation = ? AND partition_key = ? AND mother_ordinal = ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(ordinal)
        .fetch_optional(&mut *session)
        .await;
        let row = session.query(result).await?;
        session.finish().await?;
        row.map(|row| {
            let latest_event_json: String = row.try_get("latest_event_json")?;
            let latest_event = serde_json::from_str(&latest_event_json).map_err(|_| {
                ProxyError::Other("invalid semantic mother latest event".to_string())
            })?;
            let last_seen: i64 = row.try_get("last_seen")?;
            Ok(SemanticMotherReduction {
                ordinal: row.try_get("mother_ordinal")?,
                first_child_ordinal: row.try_get("first_child_ordinal")?,
                last_child_ordinal: row.try_get("last_child_ordinal")?,
                first_seen: row.try_get("first_seen")?,
                last_seen,
                event_count: row.try_get("event_count")?,
                child_count: row.try_get("child_count")?,
                latest_event,
                semantic_kind: row.try_get("semantic_kind")?,
                semantic_window_minutes: row.try_get("semantic_window_minutes")?,
                semantic_window_start: row.try_get("semantic_window_start")?,
                semantic_window_end: row.try_get("semantic_window_end")?,
                semantic_window_key: row.try_get("semantic_window_key")?,
                last_child_boundary_end: last_seen,
            })
        })
        .transpose()
    }

    fn normalized_semantic_latest_event(
        mut event: AlertEventRecord,
        semantic_kind: &str,
        semantic_window_start: Option<i64>,
        semantic_window_end: Option<i64>,
        semantic_window_key: Option<&str>,
    ) -> AlertEventRecord {
        if semantic_kind == "request_rate"
            && let Some(semantic) = event.semantic_window.as_mut()
        {
            semantic.window_start = semantic_window_start;
            semantic.window_end = semantic_window_end;
            if let Some(semantic_window_key) = semantic_window_key {
                semantic.window_key = Some(semantic_window_key.to_string());
            }
        }
        bound_alert_event_record_for_projection(event)
    }

    fn semantic_child_group_record(child: &SemanticChildReduction) -> AlertGroupRecord {
        let latest_event = Self::normalized_semantic_latest_event(
            child.latest_event.clone(),
            &child.semantic_kind,
            child.semantic_window_start,
            child.semantic_window_end,
            child
                .semantic_window_key
                .as_deref()
                .or(Some(child.window_group_key.as_str())),
        );
        let parent_id = semantic_group_base_id(&latest_event);
        AlertGroupRecord {
            id: child_group_id(&parent_id, (child.ordinal - 1) as usize),
            alert_type: latest_event.alert_type.clone(),
            subject_kind: latest_event.subject_kind.clone(),
            subject_id: latest_event.subject_id.clone(),
            subject_label: latest_event.subject_label.clone(),
            user: latest_event.user.clone(),
            token: latest_event.token.clone(),
            key: latest_event.key.clone(),
            job: latest_event.job.clone(),
            request_kind: None,
            count: child.event_count,
            first_seen: child.first_seen,
            last_seen: child.last_seen,
            latest_event,
            grouping_kind: "child".to_string(),
            semantic_window_kind: Some(child.semantic_kind.clone()),
            semantic_window_minutes: child.semantic_window_minutes,
            semantic_window_start: child.semantic_window_start,
            semantic_window_end: child.semantic_window_end,
            semantic_window_key: child.semantic_window_key.clone().or_else(|| {
                (child.semantic_kind == "request_rate").then(|| child.window_group_key.clone())
            }),
            child_count: 0,
            event_count: child.event_count,
            children: Vec::new(),
            child_events: Vec::new(),
        }
    }

    fn semantic_mother_group_record(mother: &SemanticMotherReduction) -> AlertGroupRecord {
        let latest_event = Self::normalized_semantic_latest_event(
            mother.latest_event.clone(),
            &mother.semantic_kind,
            mother.semantic_window_start,
            mother.semantic_window_end,
            mother.semantic_window_key.as_deref(),
        );
        AlertGroupRecord {
            id: semantic_mother_id_from_child(&latest_event, (mother.ordinal - 1) as usize),
            alert_type: latest_event.alert_type.clone(),
            subject_kind: latest_event.subject_kind.clone(),
            subject_id: latest_event.subject_id.clone(),
            subject_label: latest_event.subject_label.clone(),
            user: latest_event.user.clone(),
            token: latest_event.token.clone(),
            key: latest_event.key.clone(),
            job: latest_event.job.clone(),
            request_kind: None,
            count: mother.event_count,
            first_seen: mother.first_seen,
            last_seen: mother.last_seen,
            latest_event,
            grouping_kind: "mother".to_string(),
            semantic_window_kind: Some(mother.semantic_kind.clone()),
            semantic_window_minutes: mother.semantic_window_minutes,
            semantic_window_start: mother.semantic_window_start,
                semantic_window_end: mother.semantic_window_end,
                semantic_window_key: None,
            child_count: mother.child_count,
            event_count: mother.event_count,
            children: Vec::new(),
            child_events: Vec::new(),
        }
    }

    fn canonical_group_json_prefix(group: &AlertGroupRecord, open_field: &str) -> Result<String, ProxyError> {
        let payload = serde_json::to_string(group)
            .map_err(|error| ProxyError::Other(format!("serialize semantic group prefix: {error}")))?;
        let suffix = ",\"children\":[],\"child_events\":[]}";
        let prefix = payload.strip_suffix(suffix).ok_or_else(|| {
            ProxyError::Other("canonical group JSON schema is not streamable".to_string())
        })?;
        Ok(match open_field {
            "children" => format!("{prefix},\"children\":["),
            "child_events" => format!("{prefix},\"children\":[],\"child_events\":["),
            _ => return Err(ProxyError::Other("unknown canonical group output field".to_string())),
        })
    }

    async fn read_admin_alert_canonical_semantic_events(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        child_ordinal: i64,
        next_event_position: i64,
    ) -> Result<Vec<(i64, AlertEventRecord, String)>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_as::<_, (i64, String)>(
            r#"SELECT event_position, event_json
                  FROM observability.admin_alert_canonical_group_reduction_events
                 WHERE build_generation = ? AND partition_key = ? AND child_ordinal = ?
                   AND event_position <= ?
                 ORDER BY event_position DESC LIMIT ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(child_ordinal)
        .bind(next_event_position)
        .bind(ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_EVENT_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(result).await?;
        session.finish().await?;
        rows.into_iter()
            .map(|(position, event_json)| {
            serde_json::from_str(&event_json)
                .map(|event| (position, event, event_json))
                .map_err(|_| ProxyError::Other("invalid semantic reduction event".to_string()))
            })
            .collect()
    }

    async fn stream_admin_alert_canonical_semantic_output(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
        mut progress: SemanticOutputProgress,
    ) -> Result<(), ProxyError> {
        let partition = &state.build_partition_key;
        let write_position = progress.output_position;
        let Some(mother) = self
            .read_admin_alert_canonical_semantic_mother(snapshot, partition, progress.mother_ordinal)
            .await?
        else {
            return self
                .commit_admin_alert_canonical_semantic_output(
                    snapshot,
                    state,
                    progress,
                    Vec::new(),
                    None,
                    write_position,
                    true,
                    true,
                )
                .await;
        };
        let mut chunks = Vec::new();
        let mut stage_complete = true;
        let mut stage_byte_offset = progress.stage_event_byte_offset.max(0) as usize;
        macro_rules! append_chunks {
            ($payload:expr) => {
                if stage_complete {
                    stage_complete = append_bounded_semantic_payload_chunks(
                        $payload,
                        &mut stage_byte_offset,
                        &mut chunks,
                    );
                }
            };
        }
        let mut accepted_group = None;
        match progress.stage.as_str() {
            "mother_header" => {
                let prefix = Self::canonical_group_json_prefix(
                    &Self::semantic_mother_group_record(&mother),
                    "children",
                )?;
                append_chunks!(&prefix);
                progress.stage_event_byte_offset = stage_byte_offset as i64;
                if stage_complete {
                    progress.child_ordinal = mother.last_child_ordinal;
                    progress.stage = "child_header".to_string();
                    progress.stage_event_position = 0;
                    progress.stage_event_byte_offset = 0;
                }
            }
            "child_header" => {
                let child = self
                    .read_admin_alert_canonical_semantic_child(
                        snapshot,
                        partition,
                        progress.child_ordinal,
                    )
                    .await?
                    .ok_or_else(|| {
                        ProxyError::Other("semantic canonical child is unavailable".to_string())
                    })?;
                let prefix = Self::canonical_group_json_prefix(
                    &Self::semantic_child_group_record(&child),
                    "child_events",
                )?;
                let header = if progress.child_ordinal != mother.last_child_ordinal {
                    format!(",{prefix}")
                } else {
                    prefix
                };
                progress.current_child_event_bytes = self
                    .admin_alert_canonical_semantic_child_event_bytes(
                        snapshot,
                        partition,
                        child.ordinal,
                    )
                    .await?;
                progress.omit_current_child_events = progress
                    .inline_child_event_bytes
                    .saturating_add(progress.current_child_event_bytes)
                    > ADMIN_ALERT_CANONICAL_INLINE_CHILD_EVENTS_MAX_BYTES;
                append_chunks!(&header);
                progress.stage_event_byte_offset = stage_byte_offset as i64;
                if stage_complete {
                    progress.next_event_position = child.last_event_position;
                    progress.stage_event_position = child.last_event_position;
                    progress.stage_event_byte_offset = 0;
                    progress.stage = if progress.omit_current_child_events {
                        "child_suffix".to_string()
                    } else {
                        "child_events".to_string()
                    };
                }
            }
            "child_events" => {
                let child = self
                    .read_admin_alert_canonical_semantic_child(
                        snapshot,
                        partition,
                        progress.child_ordinal,
                    )
                    .await?
                    .ok_or_else(|| {
                        ProxyError::Other("semantic canonical child is unavailable".to_string())
                    })?;
                let events = self
                    .read_admin_alert_canonical_semantic_events(
                        snapshot,
                        partition,
                        child.ordinal,
                        progress.stage_event_position,
                    )
                    .await?;
                let events_empty = events.is_empty();
                for (position, event, _event_json) in events {
                    let event_prefix = if position != child.last_event_position { "," } else { "" };
                    let event_payload = if position == child.last_event_position {
                        let event = Self::normalized_semantic_latest_event(
                            event,
                            &child.semantic_kind,
                            child.semantic_window_start,
                            child.semantic_window_end,
                            child
                                .semantic_window_key
                                .as_deref()
                                .or(Some(child.window_group_key.as_str())),
                        );
                        serialize_alert_event_record_for_projection(event)?.1
                    } else {
                        serialize_alert_event_record_for_projection(event)?.1
                    };
                    let event_payload = format!("{event_prefix}{event_payload}");
                    append_chunks!(&event_payload);
                    progress.stage_event_byte_offset = stage_byte_offset as i64;
                    if stage_complete {
                        progress.next_event_position = position - 1;
                        progress.stage_event_position = position - 1;
                        progress.stage_event_byte_offset = 0;
                        stage_byte_offset = 0;
                        if position <= child.first_event_position {
                            progress.stage = "child_suffix".to_string();
                            break;
                        }
                    }
                    if !stage_complete {
                        break;
                    }
                }
                if events_empty {
                    progress.stage = "child_suffix".to_string();
                    progress.stage_event_position = 0;
                    progress.stage_event_byte_offset = 0;
                }
            }
            "child_suffix" => {
                append_chunks!("]}");
                progress.stage_event_byte_offset = stage_byte_offset as i64;
                if stage_complete {
                    if !progress.omit_current_child_events {
                        progress.inline_child_event_bytes = progress
                            .inline_child_event_bytes
                            .saturating_add(progress.current_child_event_bytes);
                    }
                    progress.current_child_event_bytes = 0;
                    progress.omit_current_child_events = false;
                    if progress.child_ordinal > mother.first_child_ordinal {
                        progress.child_ordinal -= 1;
                        progress.stage = "child_header".to_string();
                    } else {
                        progress.stage = "mother_suffix".to_string();
                    }
                    progress.stage_event_byte_offset = 0;
                }
            }
            "mother_suffix" => {
                append_chunks!("],\"child_events\":[]}");
                progress.stage_event_byte_offset = stage_byte_offset as i64;
                if stage_complete {
                    accepted_group = Some(Self::semantic_mother_group_record(&mother));
                    progress.mother_ordinal += 1;
                    progress.child_ordinal = 0;
                    progress.next_event_position = 0;
                    progress.inline_child_event_bytes = 0;
                    progress.current_child_event_bytes = 0;
                    progress.omit_current_child_events = false;
                    progress.stage = "mother_header".to_string();
                    progress.stage_event_byte_offset = 0;
                    progress.output_position += 1;
                }
            }
            _ => {
                return Err(ProxyError::Other(
                    "unknown semantic canonical output stage".to_string(),
                ));
            }
        }
        self.commit_admin_alert_canonical_semantic_output(
            snapshot,
            state,
            progress,
            chunks,
            accepted_group,
            write_position,
            false,
            stage_complete,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_admin_alert_canonical_semantic_output(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
        progress: SemanticOutputProgress,
        chunks: Vec<String>,
        accepted_group: Option<AlertGroupRecord>,
        write_position: i64,
        finish_partition: bool,
        stage_complete: bool,
    ) -> Result<(), ProxyError> {
        let partition = state.build_partition_key.clone();
        let prior_progress_json = state.build_partition_events_json.clone();
        let mut progress = progress;
        let chunk_start_position = progress.next_chunk_position;
        let stage_chunk_start_position = progress.stage_chunk_position;
        let emitted_chunks = chunks
            .len()
            .min(ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_CHUNKS_PER_TX);
        let chunk_end_position = chunk_start_position
            + emitted_chunks as i64;
        let partial = accepted_group.is_none()
            && !stage_complete;
        if partial {
            let stage_event_position = progress.stage_event_position;
            let stage_event_byte_offset = progress.stage_event_byte_offset;
            let prior_progress = serde_json::from_str::<SemanticReductionProgress>(&prior_progress_json)
                .map_err(|error| ProxyError::Other(format!("invalid semantic output progress: {error}")))?;
            if let SemanticReductionProgress::Outputting(prior_output) = prior_progress {
                // Re-run the exact same logical stage on the next slice. Only
                // the durable payload cursor advances; stage ordinals/event
                // cursors must not advance until the complete stage payload
                // is written.
                progress = prior_output;
            }
            progress.stage_event_position = stage_event_position;
            progress.stage_event_byte_offset = stage_event_byte_offset;
            progress.next_chunk_position = chunk_end_position;
            progress.stage_chunk_position = stage_chunk_start_position
                + (chunk_end_position - chunk_start_position);
        } else {
            progress.next_chunk_position = if accepted_group.is_some() {
                0
            } else {
                chunk_end_position
            };
            progress.stage_chunk_position = 0;
        }
        let next_progress_json = serde_json::to_string(&SemanticReductionProgress::Outputting(
            progress.clone(),
        ))
        .map_err(|error| ProxyError::Other(format!("serialize semantic output progress: {error}")))?;
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let mut chunk_position = chunk_start_position;
                    for payload_chunk in chunks
                        .into_iter()
                        .take(ADMIN_ALERT_CANONICAL_SEMANTIC_OUTPUT_CHUNKS_PER_TX)
                    {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks
                                   (build_generation, position, chunk_position, payload_chunk)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(write_position)
                        .bind(chunk_position)
                        .bind(payload_chunk)
                        .execute(&mut **tx)
                        .await?;
                        chunk_position += 1;
                    }
                    if let Some(group) = accepted_group {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups
                                   (build_generation, position, last_seen, total_count,
                                    alert_type, group_id, payload_json)
                               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(write_position)
                        .bind(group.last_seen)
                        .bind(group.count)
                        .bind(group.alert_type)
                        .bind(group.id)
                        .bind("")
                        .execute(&mut **tx)
                        .await?;
                    }
                    let changed = if partial {
                        sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_groups_state
                                  SET build_partition_events_json = ?
                                WHERE singleton = 1 AND build_generation = ?
                                  AND build_projection_revision = ? AND build_phase = 'aggregating'
                                  AND build_partition_key = ?
                                  AND build_partition_events_json = ?
                                  AND build_source_recent_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_state
                                  )
                                  AND build_source_history_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_history_state
                                  )"#,
                        )
                        .bind(next_progress_json)
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(&partition)
                        .bind(&prior_progress_json)
                        .execute(&mut **tx)
                        .await?
                    } else if finish_partition {
                        sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_groups_state
                                  SET build_partition_key = '', build_partition_after_key = ?,
                                      build_partition_cursor_occurred_at = -9223372036854775808,
                                      build_partition_cursor_row_sort_id = '',
                                      build_partition_events_json = '[]',
                                      build_partition_source_complete = 0,
                                      build_partition_fragment_next_position = 1,
                                      build_partition_finalize_fragment_position = 1,
                                      build_next_position = ?
                                WHERE singleton = 1 AND build_generation = ?
                                  AND build_projection_revision = ? AND build_phase = 'aggregating'
                                  AND build_partition_key = ?
                                  AND build_partition_events_json = ?
                                  AND build_source_recent_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_state
                                  )
                                  AND build_source_history_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_history_state
                                  )"#,
                        )
                        .bind(&partition)
                        .bind(progress.output_position)
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(&partition)
                        .bind(&prior_progress_json)
                        .execute(&mut **tx)
                        .await?
                    } else {
                        sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_groups_state
                                  SET build_partition_events_json = ?
                                WHERE singleton = 1 AND build_generation = ?
                                  AND build_projection_revision = ? AND build_phase = 'aggregating'
                                  AND build_partition_key = ?
                                  AND build_partition_events_json = ?
                                  AND build_source_recent_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_state
                                  )
                                  AND build_source_history_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_history_state
                                  )"#,
                        )
                        .bind(next_progress_json)
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(&partition)
                        .bind(&prior_progress_json)
                        .execute(&mut **tx)
                        .await?
                    };
                    if changed.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_build_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(())
                })
            })
            .await?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }
}
