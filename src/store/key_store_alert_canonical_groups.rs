const ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS: i64 = 250;
const ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS: usize = 25;
const ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct CompatGroupReductionState {
    event_count: i64,
    first_seen: i64,
    latest_event: AlertEventRecord,
}

enum CanonicalAlertFragmentPayload {
    Events(String),
    OversizedEventChunks(Vec<String>),
}

#[derive(Debug, serde::Deserialize)]
struct CanonicalAlertOversizedEventMarker {
    #[serde(rename = "__canonical_event_chunks")]
    chunk_count: i64,
}

const PAYLOAD_ASSEMBLY_LEVEL_SHIFT: u32 = 32;

fn encode_payload_assembly_cursor(level: i64, position: i64) -> i64 {
    -2 - ((level << PAYLOAD_ASSEMBLY_LEVEL_SHIFT) + position)
}

fn decode_payload_assembly_cursor(cursor: i64) -> (i64, i64) {
    if cursor == -1 {
        return (0, 0);
    }
    let encoded = (-cursor - 2).max(0);
    (
        encoded >> PAYLOAD_ASSEMBLY_LEVEL_SHIFT,
        encoded & ((1_i64 << PAYLOAD_ASSEMBLY_LEVEL_SHIFT) - 1),
    )
}

fn encode_payload_assembly_segment(level: i64, position: i64) -> i64 {
    (level << PAYLOAD_ASSEMBLY_LEVEL_SHIFT) + position
}

fn canonical_alert_event_fragment_payloads(
    events: Vec<AlertEventRecord>,
) -> Result<Vec<CanonicalAlertFragmentPayload>, ProxyError> {
    let mut fragments = Vec::new();
    let mut fragment = String::from("[");
    let mut has_event = false;
    for event in events {
        let (_, event_json) = serialize_alert_event_record_for_projection(event)?;
        if event_json.len() + 2 > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES {
            if has_event {
                fragment.push(']');
                fragments.push(CanonicalAlertFragmentPayload::Events(fragment));
                fragment = String::from("[");
                has_event = false;
            }
            fragments.push(CanonicalAlertFragmentPayload::OversizedEventChunks(
                canonical_alert_payload_chunks(&event_json),
            ));
            continue;
        }
        let separator_bytes = usize::from(has_event);
        if has_event
            && fragment.len() + separator_bytes + event_json.len() + 1
                > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES
        {
            fragment.push(']');
            fragments.push(CanonicalAlertFragmentPayload::Events(fragment));
            fragment = String::from("[");
            has_event = false;
        }
        if has_event {
            fragment.push(',');
        }
        fragment.push_str(&event_json);
        has_event = true;
    }
    if has_event {
        fragment.push(']');
        fragments.push(CanonicalAlertFragmentPayload::Events(fragment));
    }
    Ok(fragments)
}

fn canonical_alert_payload_chunks(payload: &str) -> Vec<String> {
    canonical_alert_payload_chunks_iter(payload).collect()
}

struct CanonicalAlertPayloadChunkIter<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    current: String,
}

impl<'a> Iterator for CanonicalAlertPayloadChunkIter<'a> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(character) = self.chars.peek().copied() {
            if !self.current.is_empty()
                && self.current.len() + character.len_utf8()
                    > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES
            {
                return Some(std::mem::take(&mut self.current));
            }
            self.chars.next();
            self.current.push(character);
        }
        (!self.current.is_empty()).then(|| std::mem::take(&mut self.current))
    }
}

fn canonical_alert_payload_chunks_iter(payload: &str) -> CanonicalAlertPayloadChunkIter<'_> {
    CanonicalAlertPayloadChunkIter {
        chars: payload.chars().peekable(),
        current: String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdminAlertsCanonicalSnapshot {
    pub(crate) build_generation: i64,
    pub(crate) projection_revision: i64,
    pub(crate) source_fence: (i64, i64),
}

#[derive(Debug, Clone)]
struct AdminAlertCanonicalGroupsState {
    active_generation: i64,
    active_projection_revision: i64,
    active_source_fence: (i64, i64),
    build_generation: i64,
    build_projection_revision: i64,
    build_source_fence: (i64, i64),
    build_source_rowid_upper_bound: i64,
    build_cursor_source_rowid: i64,
    build_phase: String,
    build_partition_key: String,
    build_partition_after_key: String,
    build_partition_cursor: (i64, String),
    build_partition_events_json: String,
    build_partition_source_complete: bool,
    build_partition_fragment_next_position: i64,
    build_partition_finalize_fragment_position: i64,
    build_next_position: i64,
    payload_read_generation: i64,
    payload_read_position: i64,
    payload_read_chunk_position: i64,
    payload_read_json: String,
}

impl KeyStore {
    async fn fetch_admin_alert_canonical_groups_page(
        &self,
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        // This compatibility path retains the controller's one-slice contract.
        // Callers receive a typed defer while a new immutable snapshot is
        // staged; only AppState owns retrying subsequent slices.
        self.admin_alert_canonical_groups_page_for_warm()
            .await
            .map(|(groups, _)| groups)
    }

    pub(crate) async fn admin_alert_canonical_groups_page_for_warm(
        &self,
    ) -> Result<(PaginatedAlertGroups, AdminAlertsCanonicalSnapshot), ProxyError> {
        let current_fence = self.admin_alerts_canonical_warm_projection_fence().await?;
        let state = self
            .load_admin_alert_canonical_groups_state()
            .await?;
        if state.build_generation == 0
            && state.active_generation > 0
            && state.active_source_fence == current_fence
        {
            let snapshot = AdminAlertsCanonicalSnapshot {
                build_generation: state.active_generation,
                projection_revision: state.active_projection_revision,
                source_fence: state.active_source_fence,
            };
            return self
                .read_admin_alert_canonical_groups_model(snapshot)
                .await
                .map(|groups| (groups, snapshot));
        }

        // A staged snapshot is valid only for the projection fence captured
        // before its first source slice. Publishing it after either lane moves
        // would mix an old Groups payload with newer Catalog/Events data.
        if state.build_generation > 0 && state.build_source_fence != current_fence {
            self.discard_admin_alert_canonical_groups_build(&state)
                .await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_source_fence_changed".to_string(),
            });
        }

        let snapshot = if state.build_generation > 0 {
            AdminAlertsCanonicalSnapshot {
                build_generation: state.build_generation,
                projection_revision: state.build_projection_revision,
                source_fence: state.build_source_fence,
            }
        } else {
            self.start_admin_alert_canonical_groups_build().await?
        };
        self.advance_admin_alert_canonical_groups_build(snapshot).await?;
        let state = self.load_admin_alert_canonical_groups_state().await?;
        if state.build_generation > 0 {
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_defer();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_in_progress".to_string(),
            });
        }
        let published = AdminAlertsCanonicalSnapshot {
            build_generation: state.active_generation,
            projection_revision: state.active_projection_revision,
            source_fence: state.active_source_fence,
        };
        self.read_admin_alert_canonical_groups_model(published)
            .await
            .map(|groups| (groups, published))
    }

    async fn load_admin_alert_canonical_groups_state(
        &self,
    ) -> Result<AdminAlertCanonicalGroupsState, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query(
            r#"SELECT active_generation, active_row_count, active_projection_revision,
                       source_recent_generation, source_history_generation,
                       build_generation, build_projection_revision,
                       build_source_recent_generation, build_source_history_generation,
                       build_phase,
                       build_source_rowid_upper_bound, build_cursor_source_rowid,
                       build_partition_key, build_partition_after_key,
                       build_partition_cursor_occurred_at, build_partition_cursor_row_sort_id,
                       build_partition_events_json,
                       build_partition_source_complete, build_partition_fragment_next_position,
                       build_partition_finalize_fragment_position,
                       build_next_position,
                       payload_read_generation, payload_read_position,
                       payload_read_chunk_position, payload_read_json
                  FROM observability.admin_alert_canonical_groups_state
                 WHERE singleton = 1"#,
        )
        .fetch_optional(&mut *session)
        .await;
        let row = session.query(result).await?;
        let finish = session.finish().await;
        finish?;
        let row = row.ok_or_else(|| {
            ProxyError::Other("canonical alert Groups state is unavailable".to_string())
        })?;
        Ok(AdminAlertCanonicalGroupsState {
            active_generation: row.try_get("active_generation")?,
            active_projection_revision: row.try_get("active_projection_revision")?,
            active_source_fence: (
                row.try_get("source_recent_generation")?,
                row.try_get("source_history_generation")?,
            ),
            build_generation: row.try_get("build_generation")?,
            build_projection_revision: row.try_get("build_projection_revision")?,
            build_source_fence: (
                row.try_get("build_source_recent_generation")?,
                row.try_get("build_source_history_generation")?,
            ),
            build_source_rowid_upper_bound: row.try_get("build_source_rowid_upper_bound")?,
            build_cursor_source_rowid: row.try_get("build_cursor_source_rowid")?,
            build_phase: row.try_get("build_phase")?,
            build_partition_key: row.try_get("build_partition_key")?,
            build_partition_after_key: row.try_get("build_partition_after_key")?,
            build_partition_cursor: (
                row.try_get("build_partition_cursor_occurred_at")?,
                row.try_get("build_partition_cursor_row_sort_id")?,
            ),
            build_partition_events_json: row.try_get("build_partition_events_json")?,
            build_partition_source_complete: row.try_get("build_partition_source_complete")?,
            build_partition_fragment_next_position: row
                .try_get("build_partition_fragment_next_position")?,
            build_partition_finalize_fragment_position: row
                .try_get("build_partition_finalize_fragment_position")?,
            build_next_position: row.try_get("build_next_position")?,
            payload_read_generation: row.try_get("payload_read_generation")?,
            payload_read_position: row.try_get("payload_read_position")?,
            payload_read_chunk_position: row.try_get("payload_read_chunk_position")?,
            payload_read_json: row.try_get("payload_read_json")?,
        })
    }

    async fn start_admin_alert_canonical_groups_build(
        &self,
    ) -> Result<AdminAlertsCanonicalSnapshot, ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, |tx| {
                Box::pin(async move {
                    let (revision, recent, history, active, building, source_rowid_upper_bound) = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
                        r#"SELECT
                               (SELECT revision FROM observability.dashboard_alert_projection_revision_state WHERE singleton = 1),
                               (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state),
                               (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_history_state),
                               active_generation,
                               build_generation,
                               (SELECT COALESCE(MAX(rowid), 0)
                                  FROM observability.dashboard_alert_projection_events)
                            FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1"#,
                    )
                    .fetch_one(&mut **tx)
                    .await?;
                    if building > 0 {
                        return Ok::<_, ProxyError>(AdminAlertsCanonicalSnapshot {
                            build_generation: building,
                            projection_revision: sqlx::query_scalar::<_, i64>(
                                "SELECT build_projection_revision FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
                            )
                            .fetch_one(&mut **tx)
                            .await?,
                            source_fence: sqlx::query_as::<_, (i64, i64)>(
                                "SELECT build_source_recent_generation, build_source_history_generation \
                                 FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
                            )
                            .fetch_one(&mut **tx)
                            .await?,
                        });
                    }
                    let build_generation = match active {
                        1 => 2,
                        2 => 1,
                        _ => 1,
                    };
                    sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_generation = ?, build_projection_revision = ?,
                                  build_source_recent_generation = ?, build_source_history_generation = ?,
                                  build_cursor_occurred_at = -9223372036854775808,
                                  build_cursor_row_sort_id = '', build_source_rowid_upper_bound = ?,
                                  build_cursor_source_rowid = 0, build_phase = 'clearing',
                                  build_partition_key = '', build_partition_after_key = '',
                                  build_partition_cursor_occurred_at = -9223372036854775808,
                                  build_partition_cursor_row_sort_id = '',
                                  build_partition_events_json = '[]',
                                  build_partition_source_complete = 0,
                                  build_partition_fragment_next_position = 1,
                                  build_partition_finalize_fragment_position = 1,
                                  build_next_position = 1,
                                  payload_read_generation = 0,
                                  payload_read_position = 0,
                                  payload_read_chunk_position = 0,
                                  payload_read_json = ''
                            WHERE singleton = 1"#,
                    )
                    .bind(build_generation)
                    .bind(revision)
                    .bind(recent)
                    .bind(history)
                    .bind(source_rowid_upper_bound)
                    .execute(&mut **tx)
                    .await?;
                    Ok(AdminAlertsCanonicalSnapshot {
                        build_generation,
                        projection_revision: revision,
                        source_fence: (recent, history),
                    })
                })
            })
            .await
    }

    async fn discard_admin_alert_canonical_groups_build(
        &self,
        state: &AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let build_generation = state.build_generation;
        let projection_revision = state.build_projection_revision;
        let source_fence = state.build_source_fence;
        let discarded = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_generation = 0, build_projection_revision = -1,
                                  build_source_recent_generation = -1,
                                  build_source_history_generation = -1,
                                  build_source_rowid_upper_bound = 0,
                                  build_cursor_source_rowid = 0, build_phase = 'idle',
                                  build_partition_key = '', build_partition_after_key = '',
                                  build_partition_cursor_occurred_at = -9223372036854775808,
                                  build_partition_cursor_row_sort_id = '',
                                  build_partition_events_json = '[]',
                                  build_partition_source_complete = 0,
                                  build_partition_fragment_next_position = 1,
                                  build_partition_finalize_fragment_position = 1,
                                  build_next_position = 1,
                                  payload_read_generation = 0,
                                  payload_read_position = 0,
                                  payload_read_chunk_position = 0,
                                  payload_read_json = ''
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ?
                              AND build_source_recent_generation = ?
                              AND build_source_history_generation = ?"#,
                    )
                    .bind(build_generation)
                    .bind(projection_revision)
                    .bind(source_fence.0)
                    .bind(source_fence.1)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !discarded {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_defer();
        Ok(())
    }

    async fn advance_admin_alert_canonical_groups_build(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<(), ProxyError> {
        if let Some(reason) = self.admin_alerts_cache_warm_defer_reason() {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: reason.to_string(),
            });
        }
        let state = self.load_admin_alert_canonical_groups_state().await?;
        if state.build_generation != snapshot.build_generation {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        match state.build_phase.as_str() {
            "clearing" => self.clear_admin_alert_canonical_groups_build_slot(snapshot).await,
            "copying" => self.copy_admin_alert_canonical_groups_snapshot_slice(snapshot, state).await,
            "aggregating" => self.aggregate_admin_alert_canonical_groups_partition(snapshot, state).await,
            _ => Err(ProxyError::Other("unknown canonical alert Groups build phase".to_string())),
        }
    }

    async fn clear_admin_alert_canonical_groups_build_slot(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let advanced = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let still_current = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                         WHERE singleton = 1 AND build_generation = ? \
                           AND build_projection_revision = ? AND build_phase = 'clearing')",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !still_current {
                        return Ok::<_, ProxyError>(false);
                    }
                    for table in [
                        "admin_alert_canonical_groups",
                        "admin_alert_canonical_group_events",
                        "admin_alert_canonical_group_overrides",
                        "admin_alert_canonical_group_fragments",
                        "admin_alert_canonical_group_reduction_events",
                        "admin_alert_canonical_group_reduction_children",
                        "admin_alert_canonical_group_reduction_mothers",
                        "admin_alert_canonical_group_payload_chunks",
                        "admin_alert_canonical_group_payload_read_chunks_v2",
                        "admin_alert_canonical_catalog_facets",
                        "admin_alert_canonical_catalog_payloads",
                        "admin_alert_canonical_catalog_payload_items",
                        "admin_alert_canonical_catalog_payload_items_v2",
                    ] {
                        let delete = format!(
                            "DELETE FROM observability.{table} WHERE rowid IN ( \
                             SELECT rowid FROM observability.{table} \
                              WHERE build_generation = ? LIMIT 25)"
                        );
                        sqlx::query(&delete)
                            .bind(snapshot.build_generation)
                            .execute(&mut **tx)
                            .await?;
                    }
                    let mut remaining = false;
                    for table in [
                        "admin_alert_canonical_groups",
                        "admin_alert_canonical_group_events",
                        "admin_alert_canonical_group_overrides",
                        "admin_alert_canonical_group_fragments",
                        "admin_alert_canonical_group_reduction_events",
                        "admin_alert_canonical_group_reduction_children",
                        "admin_alert_canonical_group_reduction_mothers",
                        "admin_alert_canonical_group_payload_chunks",
                        "admin_alert_canonical_group_payload_read_chunks_v2",
                        "admin_alert_canonical_catalog_facets",
                        "admin_alert_canonical_catalog_payloads",
                        "admin_alert_canonical_catalog_payload_items",
                        "admin_alert_canonical_catalog_payload_items_v2",
                    ] {
                        let exists = format!(
                            "SELECT EXISTS(SELECT 1 FROM observability.{table} \
                             WHERE build_generation = ?)"
                        );
                        if sqlx::query_scalar::<_, bool>(&exists)
                            .bind(snapshot.build_generation)
                            .fetch_one(&mut **tx)
                            .await?
                        {
                            remaining = true;
                            break;
                        }
                    }
                    let changed = if remaining {
                        true
                    } else {
                        sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_groups_state
                                  SET build_phase = 'copying',
                                      build_cursor_source_rowid = 0,
                                      build_partition_key = '', build_partition_after_key = '',
                                      build_partition_cursor_occurred_at = -9223372036854775808,
                                      build_partition_cursor_row_sort_id = '',
                                      build_partition_source_complete = 0,
                                      build_partition_fragment_next_position = 1,
                                      build_partition_finalize_fragment_position = 1,
                                      build_next_position = 1,
                                      payload_read_generation = 0,
                                      payload_read_position = 0,
                                      payload_read_chunk_position = 0,
                                      payload_read_json = ''
                                WHERE singleton = 1 AND build_generation = ?
                                  AND build_projection_revision = ? AND build_phase = 'clearing'"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected()
                            == 1
                    };
                    if !remaining && changed {
                        sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_catalog_state
                                  SET build_generation = ?,
                                      cursor_occurred_at = -9223372036854775808,
                                      cursor_row_sort_id = '', source_complete = 0
                                WHERE singleton = 1"#,
                        )
                        .bind(snapshot.build_generation)
                        .execute(&mut **tx)
                        .await?;
                    }
                    Ok::<_, ProxyError>(changed)
                })
            })
            .await?;
        if !advanced {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    async fn copy_admin_alert_canonical_groups_snapshot_slice(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let query_result = sqlx::query(
            r#"SELECT current.rowid AS source_rowid,
                       COALESCE(override.source_kind, current.source_kind) AS source_kind,
                       COALESCE(override.source_id, current.source_id) AS source_id,
                       COALESCE(override.occurred_at, current.occurred_at) AS occurred_at,
                       COALESCE(override.row_sort_id, current.row_sort_id) AS row_sort_id,
                       COALESCE(override.payload_json, current.payload_json) AS payload_json
                  FROM observability.dashboard_alert_projection_events AS current
                 LEFT JOIN observability.admin_alert_canonical_group_overrides AS override
                    ON override.build_generation = ?
                   AND override.source_kind = current.source_kind
                   AND override.source_id = current.source_id
                 WHERE current.rowid > ? AND current.rowid <= ?
                   AND current.occurred_at >= ?
                 ORDER BY current.rowid ASC
                 LIMIT ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(state.build_cursor_source_rowid)
        .bind(state.build_source_rowid_upper_bound)
        .bind(self.alert_projection_retention_since())
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        let rows = rows?;
        let next_cursor_source_rowid = rows
            .last()
            .map(|row| row.try_get::<i64, _>("source_rowid"))
            .transpose()?
            .unwrap_or(state.build_cursor_source_rowid);
        let row_count = rows.len();
        let staged = rows
            .into_iter()
            .map(|row| {
                let source_kind = row.try_get::<String, _>("source_kind")?;
                let source_id = row.try_get::<String, _>("source_id")?;
                let occurred_at = row.try_get::<i64, _>("occurred_at")?;
                let row_sort_id = row.try_get::<String, _>("row_sort_id")?;
                let projection = Self::decode_default_alert_event_projection_row(row)?;
                let payload_json = serialize_alert_event_projection_payload(projection.clone())?;
                let event = Self::build_alert_event_from_projection(projection);
                let partition_key = event.as_ref().map_or_else(String::new, canonical_alert_group_partition_key);
                // The legacy in-memory grouping contract breaks ties by the
                // canonical AlertEventRecord identity. Keep that identity in
                // the sidecar seek key so a resumable reducer cannot select a
                // different latest event when projection row_sort_id uses a
                // different encoding (for example, numeric source IDs).
                let row_sort_id = event
                    .as_ref()
                    .map(|event| event.id.clone())
                    .unwrap_or(row_sort_id);
                Ok::<_, ProxyError>((
                    source_kind,
                    source_id,
                    occurred_at,
                    row_sort_id,
                    partition_key,
                    payload_json,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for chunk in staged.chunks(ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS) {
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let chunk = chunk.to_vec();
            self.sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        for (source_kind, source_id, occurred_at, row_sort_id, partition_key, payload_json) in chunk {
                            sqlx::query(
                                r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_events
                                       (build_generation, source_kind, source_id, occurred_at, row_sort_id,
                                        partition_key, payload_json)
                                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                            )
                            .bind(snapshot.build_generation)
                            .bind(source_kind)
                            .bind(source_id)
                            .bind(occurred_at)
                            .bind(row_sort_id)
                            .bind(partition_key)
                            .bind(payload_json)
                            .execute(&mut **tx)
                            .await?;
                        }
                        Ok::<_, ProxyError>(())
                    })
                })
                .await?;
        }
        let complete = row_count < ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS as usize;
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_cursor_source_rowid = ?,
                                  build_phase = CASE WHEN ? THEN 'aggregating' ELSE 'copying' END
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ?
                              AND build_source_recent_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_state
                              )
                              AND build_source_history_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_history_state
                              )"#,
                    )
                    .bind(next_cursor_source_rowid)
                    .bind(complete)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?
            .then_some(())
            .ok_or_else(|| ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            })?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    async fn aggregate_admin_alert_canonical_groups_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        if state.build_partition_key.is_empty() {
            return self
                .select_admin_alert_canonical_groups_partition(snapshot, state)
                .await;
        }
        if !state.build_partition_source_complete {
            return self
                .capture_admin_alert_canonical_groups_partition_slice(snapshot, state)
                .await;
        }
        self.finalize_admin_alert_canonical_groups_partition(snapshot, state)
            .await
    }

    async fn finalize_admin_alert_canonical_groups_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        if serde_json::from_str::<SemanticReductionProgress>(&state.build_partition_events_json)
            .is_ok()
        {
            return self
                .finalize_admin_alert_canonical_semantic_partition(snapshot, state)
                .await;
        }
        let Some((fragment_position, events)) = self
            .read_admin_alert_canonical_groups_partition_fragment(
                snapshot,
                &state.build_partition_key,
                state.build_partition_finalize_fragment_position,
            )
            .await?
        else {
            return self
                .publish_admin_alert_canonical_compat_partition(snapshot, state)
                .await;
        };
        if events.iter().any(|event| {
            matches!(
                event.alert_type.as_str(),
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED | ALERT_TYPE_USER_QUOTA_EXHAUSTED
            ) && event.semantic_window.is_some()
        }) {
            return self
                .finalize_admin_alert_canonical_semantic_partition(snapshot, state)
                .await;
        }

        let current = serde_json::from_str::<CompatGroupReductionState>(
            &state.build_partition_events_json,
        )
        .ok();
        let Some(latest_event) = events.last().cloned() else {
            return Err(ProxyError::Other(
                "canonical alert group fragment cannot be empty".to_string(),
            ));
        };
        let next = CompatGroupReductionState {
            event_count: current
                .as_ref()
                .map_or(events.len() as i64, |reduction| {
                    reduction.event_count + events.len() as i64
                }),
            first_seen: current
                .as_ref()
                .map_or_else(|| events[0].occurred_at, |reduction| reduction.first_seen),
            latest_event,
        };
        let reduction_json = serde_json::to_string(&next).map_err(|error| {
            ProxyError::Other(format!("serialize canonical compat reduction state: {error}"))
        })?;
        let partition = state.build_partition_key.clone();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let advanced = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_events_json = ?,
                                  build_partition_finalize_fragment_position = ?
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ? AND build_phase = 'aggregating'
                              AND build_partition_key = ?
                              AND build_partition_finalize_fragment_position = ?
                              AND build_source_recent_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_state
                              )
                              AND build_source_history_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_history_state
                              )"#,
                    )
                    .bind(reduction_json)
                    .bind(fragment_position + 1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(&partition)
                    .bind(state.build_partition_finalize_fragment_position)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !advanced {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }

    async fn publish_admin_alert_canonical_compat_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let reduction = serde_json::from_str::<CompatGroupReductionState>(
            &state.build_partition_events_json,
        )
        .map_err(|_| {
            ProxyError::Other("canonical compat reduction state is unavailable".to_string())
        })?;
        let mut group = build_compat_group_record(std::slice::from_ref(&reduction.latest_event)).ok_or_else(|| {
            ProxyError::Other("canonical compat reduction has no latest event".to_string())
        })?;
        group.count = reduction.event_count;
        group.event_count = reduction.event_count;
        group.first_seen = reduction.first_seen;
        let payload_json = serde_json::to_string(&group)
            .map_err(|error| ProxyError::Other(format!("serialize canonical alert group: {error}")))?;
        let position = state.build_next_position;
        for (chunk_position, payload_chunk) in
            canonical_alert_payload_chunks(&payload_json).into_iter().enumerate()
        {
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            self.sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks
                                   (build_generation, position, chunk_position, payload_chunk)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(position)
                        .bind(chunk_position as i64)
                        .bind(payload_chunk)
                        .execute(&mut **tx)
                        .await?;
                        Ok::<_, ProxyError>(())
                    })
                })
                .await?;
        }
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                let group = group.clone();
                Box::pin(async move {
                    sqlx::query(
                        r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups
                               (build_generation, position, last_seen, total_count,
                                alert_type, group_id, payload_json)
                           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .bind(group.last_seen)
                    .bind(group.count)
                    .bind(group.alert_type)
                    .bind(group.id)
                    .bind("")
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(())
                })
            })
            .await?;
        let partition = state.build_partition_key.clone();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let finalized = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
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
                    .bind(position + 1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(&partition)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !finalized {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }

    async fn finalize_admin_alert_canonical_semantic_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        self.advance_admin_alert_canonical_semantic_reduction(snapshot, state)
            .await
    }

    async fn capture_admin_alert_canonical_groups_partition_slice(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let partition = state.build_partition_key.clone();
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let rows_result = sqlx::query(
            "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
             FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = ? AND partition_key = ? \
               AND (occurred_at > ? OR (occurred_at = ? AND row_sort_id > ?)) \
             ORDER BY occurred_at ASC, row_sort_id ASC LIMIT ?",
        )
        .bind(snapshot.build_generation)
        .bind(&partition)
        .bind(state.build_partition_cursor.0)
        .bind(state.build_partition_cursor.0)
        .bind(&state.build_partition_cursor.1)
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await;
        let finish = session.finish().await;
        finish?;
        let rows = rows?;
        let complete = rows.len() < ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS as usize;
        let next_cursor = rows
            .last()
            .map(|row| {
                Ok::<_, sqlx::Error>((
                    row.try_get::<i64, _>("occurred_at")?,
                    row.try_get::<String, _>("row_sort_id")?,
                ))
            })
            .transpose()?
            .unwrap_or_else(|| state.build_partition_cursor.clone());
        let events = rows
            .into_iter()
            .map(Self::decode_default_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect::<Vec<_>>();
        let fragments = canonical_alert_event_fragment_payloads(events)?;
        let fragment_position = state.build_partition_fragment_next_position;
        let fragment_count = fragments.len();
        for (offset, fragment_payload) in fragments.into_iter().enumerate() {
            let position = fragment_position + offset as i64;
            let (events_json, oversized_chunks) = match fragment_payload {
                CanonicalAlertFragmentPayload::Events(events_json) => (events_json, None),
                CanonicalAlertFragmentPayload::OversizedEventChunks(chunks) => {
                    let marker = format!(
                        r#"{{"__canonical_event_chunks":{}}}"#,
                        chunks.len()
                    );
                    (marker, Some(chunks))
                }
            };
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let partition = partition.clone();
            self.sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_fragments
                                   (build_generation, partition_key, position, events_json)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(partition)
                        .bind(position)
                        .bind(events_json)
                        .execute(&mut **tx)
                        .await?;
                        Ok::<_, ProxyError>(())
                    })
                })
                .await?;
            if let Some(chunks) = oversized_chunks {
                for (chunk_position, payload_chunk) in chunks.into_iter().enumerate() {
                    let chunk_position = chunk_position as i64;
                    let chunk_key = -position;
                    self.ensure_admin_alerts_cache_warm_write_admitted()?;
                    self.sqlite_runtime
                        .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                            Box::pin(async move {
                                sqlx::query(
                                    r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks
                                           (build_generation, position, chunk_position, payload_chunk)
                                       VALUES (?, ?, ?, ?)"#,
                                )
                                .bind(snapshot.build_generation)
                                .bind(chunk_key)
                                .bind(chunk_position)
                                .bind(payload_chunk)
                                .execute(&mut **tx)
                                .await?;
                                Ok::<_, ProxyError>(())
                            })
                        })
                        .await?;
                }
            }
        }
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let next_fragment_position = fragment_position + fragment_count as i64;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_cursor_occurred_at = ?,
                                  build_partition_cursor_row_sort_id = ?,
                                  build_partition_source_complete = ?,
                                  build_partition_fragment_next_position = ?
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ? AND build_phase = 'aggregating'
                              AND build_partition_key = ?
                              AND build_partition_cursor_occurred_at = ?
                              AND build_partition_cursor_row_sort_id = ?
                              AND build_source_recent_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_state
                              )
                              AND build_source_history_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_history_state
                              )"#,
                    )
                    .bind(next_cursor.0)
                    .bind(next_cursor.1)
                    .bind(complete)
                    .bind(next_fragment_position)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(&partition)
                    .bind(state.build_partition_cursor.0)
                    .bind(state.build_partition_cursor.1)
                        .execute(&mut **tx)
                        .await?;
                    if changed.rows_affected() != 1 {
                        return Ok::<_, ProxyError>(false);
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    async fn read_admin_alert_canonical_groups_partition_fragment(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        position: i64,
    ) -> Result<Option<(i64, Vec<AlertEventRecord>)>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_as::<_, (i64, String)>(
            "SELECT position, events_json \
             FROM observability.admin_alert_canonical_group_fragments \
             WHERE build_generation = ? AND partition_key = ? AND position >= ? \
             ORDER BY position ASC LIMIT 1",
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(position)
        .fetch_optional(&mut *session)
        .await;
        let row = session.query(result).await?;
        let Some((position, events_json)) = row else {
            session.finish().await?;
            return Ok(None);
        };
        session.finish().await?;
        if let Ok(events) = serde_json::from_str::<Vec<AlertEventRecord>>(&events_json) {
            return Ok(Some((position, events)));
        }
        let marker = serde_json::from_str::<CanonicalAlertOversizedEventMarker>(&events_json)
            .map_err(|_| ProxyError::Other("invalid canonical alert group fragment".to_string()))?;
        if marker.chunk_count <= 0 {
            return Err(ProxyError::Other(
                "canonical oversized alert event has no chunks".to_string(),
            ));
        }
        let event_json = self
            .read_admin_alert_canonical_group_payload_chunks(snapshot, -position, Some(marker.chunk_count))
            .await?;
        let event = serde_json::from_str::<AlertEventRecord>(&event_json)
            .map_err(|_| ProxyError::Other("invalid canonical oversized alert event".to_string()))?;
        self.clear_admin_alert_canonical_group_payload_read(
            snapshot,
            -position,
        )
        .await?;
        Ok(Some((position, vec![event])))
    }

    async fn select_admin_alert_canonical_groups_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_scalar::<_, String>(
            "SELECT partition_key FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = ? AND partition_key > '' AND partition_key > ? \
             ORDER BY partition_key ASC LIMIT 1",
        )
        .bind(snapshot.build_generation)
        .bind(&state.build_partition_after_key)
        .fetch_optional(&mut *session)
        .await;
        let partition = session.query(result).await?;
        let finish = session.finish().await;
        finish?;
        let Some(partition) = partition else {
            return self
                .publish_admin_alert_canonical_groups_snapshot(snapshot, state.build_next_position - 1)
                .await;
        };
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_key = ?,
                                  build_partition_cursor_occurred_at = -9223372036854775808,
                                  build_partition_cursor_row_sort_id = '',
                                  build_partition_events_json = '[]',
                                  build_partition_source_complete = 0,
                                  build_partition_fragment_next_position = 1,
                                  build_partition_finalize_fragment_position = 1
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ? AND build_phase = 'aggregating'
                              AND build_partition_key = ''"#,
                    )
                    .bind(partition)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?
            .then_some(())
            .ok_or_else(|| ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            })?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    async fn publish_admin_alert_canonical_groups_snapshot(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        row_count: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let published = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET active_generation = ?, active_row_count = ?,
                                  active_projection_revision = ?, source_recent_generation = ?,
                                  source_history_generation = ?, build_generation = 0,
                                  build_projection_revision = -1,
                                  build_source_rowid_upper_bound = 0,
                                  build_cursor_source_rowid = 0, build_phase = 'idle',
                                  build_partition_key = '', build_partition_after_key = '',
                                  build_partition_cursor_occurred_at = -9223372036854775808,
                                  build_partition_cursor_row_sort_id = '',
                                  build_partition_events_json = '[]',
                                  build_partition_source_complete = 0,
                                  build_partition_fragment_next_position = 1,
                                  build_partition_finalize_fragment_position = 1,
                                  build_next_position = 1,
                                  payload_read_generation = 0,
                                  payload_read_position = 0,
                                  payload_read_chunk_position = 0,
                                  payload_read_json = ''
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ?
                              AND build_source_recent_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_state
                              )
                              AND build_source_history_generation = (
                                  SELECT COALESCE(SUM(generation), 0)
                                    FROM observability.dashboard_alert_projection_history_state
                              )"#,
                    )
                    .bind(snapshot.build_generation)
                    .bind(row_count)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !published {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_publish();
        Ok(())
    }

    pub(crate) async fn reclaim_admin_alert_canonical_groups_generations(
        &self,
    ) -> Result<bool, ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, |tx| {
                Box::pin(async move {
                    let (active_generation, active_row_count, build_generation) = sqlx::query_as::<_, (i64, i64, i64)>(
                        "SELECT active_generation, active_row_count, build_generation \
                         FROM observability.admin_alert_canonical_groups_state \
                         WHERE singleton = 1",
                    )
                    .fetch_optional(&mut **tx)
                    .await?
                    .unwrap_or_default();
                    let cleanup_events = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_events
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_events
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, occurred_at ASC, row_sort_id ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_overrides = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_overrides
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_overrides
                                  WHERE build_generation <> ?
                                  ORDER BY build_generation ASC, source_kind ASC, source_id ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_fragments = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_fragments
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_fragments
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, partition_key ASC, position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_events = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_events
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_reduction_events
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, partition_key ASC, event_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_children = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_children
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_reduction_children
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, partition_key ASC, child_ordinal ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_mothers = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_mothers
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_reduction_mothers
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, partition_key ASC, mother_ordinal ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_group_payload_chunks = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_payload_chunks
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_payload_chunks
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, position ASC, chunk_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_group_payload_read_chunks = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_group_payload_read_chunks_v2
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, position ASC, chunk_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_facets
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_catalog_facets
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payloads = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payloads
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_catalog_payloads
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, facet_kind ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payload_items = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payload_items
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_catalog_payload_items
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payload_items_v2 = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payload_items_v2
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_catalog_payload_items_v2
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC,
                                           facet_label ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
                    let cleanup_remaining = cleanup_events > 0
                        || cleanup_overrides > 0
                        || cleanup_fragments > 0
                        || cleanup_reduction_events > 0
                        || cleanup_reduction_children > 0
                        || cleanup_reduction_mothers > 0
                        || cleanup_group_payload_chunks > 0
                        || cleanup_group_payload_read_chunks > 0
                        || cleanup_catalog > 0
                        || cleanup_catalog_payloads > 0
                        || cleanup_catalog_payload_items > 0
                        || cleanup_catalog_payload_items_v2 > 0
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_events \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_overrides \
                             WHERE build_generation <> ?)",
                        )
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_fragments \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_payload_chunks \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_events \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_children \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_mothers \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_facets \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payloads \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payload_items \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payload_items_v2 \
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                    let reusable_slot_active = matches!(active_generation, 1 | 2);
                    if reusable_slot_active {
                        let legacy_deleted = sqlx::query(
                            r#"DELETE FROM observability.admin_alert_canonical_groups
                                 WHERE rowid IN (
                                     SELECT rowid
                                      FROM observability.admin_alert_canonical_groups
                                      WHERE build_generation > 2 AND build_generation <> ?
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
                        .bind(build_generation)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected();
                        if legacy_deleted == 0 {
                            let inactive_generation = if active_generation == 1 { 2 } else { 1 };
                            let inactive_deleted = sqlx::query(
                                r#"DELETE FROM observability.admin_alert_canonical_groups
                                     WHERE rowid IN (
                                         SELECT rowid
                                          FROM observability.admin_alert_canonical_groups
                                          WHERE build_generation = ? AND build_generation <> ?
                                          ORDER BY position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(inactive_generation)
                            .bind(build_generation)
                            .execute(&mut **tx)
                            .await?
                            .rows_affected();
                            if inactive_deleted == 0 {
                                sqlx::query(
                                    r#"DELETE FROM observability.admin_alert_canonical_groups
                                         WHERE rowid IN (
                                             SELECT rowid
                                               FROM observability.admin_alert_canonical_groups
                                              WHERE build_generation = ? AND build_generation <> ?
                                                AND position > ?
                                              ORDER BY position ASC
                                              LIMIT 25
                                         )"#,
                                )
                                .bind(active_generation)
                                .bind(build_generation)
                                .bind(active_row_count)
                                .execute(&mut **tx)
                                .await?;
                            }
                        }
                        let inactive_generation = if active_generation == 1 { 2 } else { 1 };
                        let legacy_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > 2 AND build_generation <> ?)",
                        )
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        let inactive_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = ? AND build_generation <> ?)",
                        )
                        .bind(inactive_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        let active_tail_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = ? AND build_generation <> ? AND position > ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .bind(active_row_count)
                        .fetch_one(&mut **tx)
                        .await?;
                        Ok::<_, ProxyError>(
                            legacy_remaining
                                || inactive_remaining
                                || active_tail_remaining
                                || cleanup_remaining,
                        )
                    } else {
                        let retired_before_active = sqlx::query(
                            r#"DELETE FROM observability.admin_alert_canonical_groups
                                 WHERE rowid IN (
                                     SELECT rowid
                                      FROM observability.admin_alert_canonical_groups
                                      WHERE build_generation < ? AND build_generation <> ?
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected();
                        if retired_before_active == 0 {
                            sqlx::query(
                                r#"DELETE FROM observability.admin_alert_canonical_groups
                                     WHERE rowid IN (
                                         SELECT rowid
                                           FROM observability.admin_alert_canonical_groups
                                          WHERE build_generation > ? AND build_generation <> ?
                                          ORDER BY build_generation ASC, position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(active_generation)
                            .bind(build_generation)
                            .execute(&mut **tx)
                            .await?;
                        }
                        let retired_before_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation < ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        let retired_after_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        Ok::<_, ProxyError>(
                            retired_before_remaining
                                || retired_after_remaining
                                || cleanup_remaining,
                        )
                    }
                })
            })
            .await
    }

    async fn reset_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = ?, payload_read_position = ?, \
                                payload_read_chunk_position = 0, \
                                payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) ",
                    )
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected() == 1)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        chunks: Vec<(i64, String)>,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating'))) ",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    for (chunk_position, payload_chunk) in chunks {
                        sqlx::query(
                            "INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                             (build_generation, build_projection_revision, source_recent_generation, \
                              source_history_generation, position, chunk_position, payload_chunk) \
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(snapshot.source_fence.0)
                        .bind(snapshot.source_fence.1)
                        .bind(position)
                        .bind(chunk_position)
                        .bind(payload_chunk)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ? \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
                    )
                    .bind(next_chunk_position)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .bind(prior_chunk_position)
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn mark_admin_alert_canonical_group_payload_read_ready(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')))",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Ok::<_, ProxyError>(false);
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ?, payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
                    )
                    .bind(encode_payload_assembly_cursor(0, 0))
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .bind(prior_chunk_position)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected() == 1)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_assembly(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        segment_position: i64,
        segment_payload: String,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating'))) ",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    if !segment_payload.is_empty() {
                        sqlx::query(
                            "INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                             (build_generation, build_projection_revision, source_recent_generation, \
                              source_history_generation, position, chunk_position, payload_chunk) \
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(snapshot.source_fence.0)
                        .bind(snapshot.source_fence.1)
                        .bind(position)
                        .bind(segment_position)
                        .bind(segment_payload)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ? \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
                    )
                    .bind(next_chunk_position)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .bind(prior_chunk_position)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected())
                })
            })
            .await?;
        if changed != 1 {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn complete_admin_alert_canonical_group_payload(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        payload: String,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')))",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups \
                            SET payload_json = ? \
                          WHERE build_generation = ? AND position = ?",
                    )
                    .bind(payload)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Other(
                            "canonical alert group payload row is unavailable".to_string(),
                        ));
                    }
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    let state = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = 0, payload_read_position = 0, \
                                payload_read_chunk_position = 0, payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    if state.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn clear_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = 0, payload_read_position = 0, \
                                payload_read_chunk_position = 0, \
                                payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn read_admin_alert_canonical_group_payload_chunks(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        expected_chunks: Option<i64>,
    ) -> Result<String, ProxyError> {
        const PAYLOAD_CHUNK_READ_ROWS: i64 = 16;
        let state = self.load_admin_alert_canonical_groups_state().await?;
        let state_matches = state.payload_read_generation
            == snapshot.build_generation
            && state.payload_read_position == position
            && ((state.build_generation == 0
                && state.active_generation == snapshot.build_generation
                && state.active_projection_revision == snapshot.projection_revision
                && state.active_source_fence == snapshot.source_fence)
                || (state.build_generation == snapshot.build_generation
                    && state.build_projection_revision == snapshot.projection_revision
                    && state.build_source_fence == snapshot.source_fence
                    && state.build_phase == "aggregating"))
        ;
        if !state_matches {
            self.reset_admin_alert_canonical_group_payload_read(snapshot, position)
                .await?;
        }

        let state = if state_matches {
            state
        } else {
            self.load_admin_alert_canonical_groups_state().await?
        };
        let mut next_chunk_position = state.payload_read_chunk_position;
        let legacy_assembly_position = (!state.payload_read_json.is_empty())
            .then_some(next_chunk_position);
        let mut assembling = state_matches
            && (next_chunk_position < 0 || legacy_assembly_position.is_some());

        if !assembling {
            let limit = expected_chunks
                .map(|expected| (expected - next_chunk_position).min(PAYLOAD_CHUNK_READ_ROWS))
                .unwrap_or(PAYLOAD_CHUNK_READ_ROWS);
            if limit <= 0 {
                return Err(ProxyError::Other(
                    "canonical alert group payload chunks are incomplete".to_string(),
                ));
            }
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let rows_result = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_chunks \
                  WHERE build_generation = ? AND position = ? AND chunk_position >= ? \
                  ORDER BY chunk_position ASC LIMIT ?",
            )
            .bind(snapshot.build_generation)
            .bind(position)
            .bind(next_chunk_position)
            .bind(limit)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(rows_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = rows?;
            if rows.is_empty() {
                if expected_chunks.is_none() && next_chunk_position > 0 {
                    self.mark_admin_alert_canonical_group_payload_read_ready(
                        snapshot,
                        position,
                        next_chunk_position,
                    )
                    .await?;
                    next_chunk_position = encode_payload_assembly_cursor(0, 0);
                    assembling = true;
                } else {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks are incomplete".to_string(),
                    ));
                }
            }
            if !rows.is_empty() {
                let prior_chunk_position = next_chunk_position;
                for (chunk_position, _chunk) in &rows {
                    if *chunk_position != next_chunk_position {
                        return Err(ProxyError::Other(
                            "canonical alert group payload chunks are non-contiguous".to_string(),
                        ));
                    }
                    next_chunk_position += 1;
                }
                if expected_chunks.is_some_and(|expected| next_chunk_position > expected) {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks exceed marker".to_string(),
                    ));
                }
                let batch_len = rows.len();
                self.checkpoint_admin_alert_canonical_group_payload_read(
                    snapshot,
                    position,
                    prior_chunk_position,
                    next_chunk_position,
                    rows.into_iter().collect(),
                )
                .await?;
                let source_complete = expected_chunks
                    .is_some_and(|expected| next_chunk_position >= expected)
                    || batch_len < PAYLOAD_CHUNK_READ_ROWS as usize;
                if !source_complete {
                    return Err(ProxyError::Deferred {
                        operation: "admin_alerts_cache_warm",
                        reason: "groups_build_in_progress".to_string(),
                    });
                }
                self.mark_admin_alert_canonical_group_payload_read_ready(
                    snapshot,
                    position,
                    next_chunk_position,
                )
                .await?;
                next_chunk_position = encode_payload_assembly_cursor(0, 0);
                assembling = true;
            }
        }

        if assembling {
            let (assembly_level, assembly_position) = if legacy_assembly_position.is_some() {
                (0, 0)
            } else {
                decode_payload_assembly_cursor(next_chunk_position)
            };
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let source_level = assembly_level == 0;
            let source_position = if source_level {
                assembly_position
            } else {
                encode_payload_assembly_segment(assembly_level, assembly_position)
            };
            let source_upper_bound = if source_level {
                encode_payload_assembly_segment(1, 0)
            } else {
                encode_payload_assembly_segment(assembly_level, assembly_position + 16)
            };
            let rows_result = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                  WHERE build_generation = ? AND build_projection_revision = ? \
                    AND source_recent_generation = ? AND source_history_generation = ? \
                    AND position = ? AND chunk_position >= ? AND chunk_position < ? \
                  ORDER BY chunk_position ASC LIMIT ?",
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(position)
            .bind(source_position)
            .bind(source_upper_bound)
            .bind(PAYLOAD_CHUNK_READ_ROWS)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(rows_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = rows?;
            if rows.is_empty() {
                if assembly_level == 0 && assembly_position == 0 {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks are incomplete".to_string(),
                    ));
                }
                let next_cursor = encode_payload_assembly_cursor(assembly_level + 1, 0);
                self.checkpoint_admin_alert_canonical_group_payload_assembly(
                    snapshot,
                    position,
                    next_chunk_position,
                    next_cursor,
                    encode_payload_assembly_segment(assembly_level + 1, 0),
                    String::new(),
                )
                .await?;
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_in_progress".to_string(),
                });
            }
            let mut after_position = assembly_position;
            let mut segment_payload = String::new();
            for (chunk_position, chunk) in &rows {
                let expected_position = if source_level {
                    after_position
                } else {
                    encode_payload_assembly_segment(assembly_level, after_position)
                };
                if *chunk_position != expected_position {
                    return Err(ProxyError::Other(format!(
                        "canonical alert group staged chunks are non-contiguous (expected {expected_position}, got {chunk_position})",
                    )));
                }
                segment_payload.push_str(chunk);
                after_position += 1;
            }
            if rows.len() == PAYLOAD_CHUNK_READ_ROWS as usize {
                let next_output_index = assembly_position / 16;
                let next_cursor = encode_payload_assembly_cursor(assembly_level, after_position);
                self.checkpoint_admin_alert_canonical_group_payload_assembly(
                    snapshot,
                    position,
                    next_chunk_position,
                    next_cursor,
                    encode_payload_assembly_segment(assembly_level + 1, next_output_index),
                    segment_payload,
                )
                .await?;
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_in_progress".to_string(),
                });
            }
            if assembly_level > 0 && assembly_position == 0 && rows.len() == 1 {
                return Ok(segment_payload);
            }
            let next_output_index = assembly_position / 16;
            let next_cursor = encode_payload_assembly_cursor(assembly_level + 1, 0);
            self.checkpoint_admin_alert_canonical_group_payload_assembly(
                snapshot,
                position,
                next_chunk_position,
                next_cursor,
                encode_payload_assembly_segment(assembly_level + 1, next_output_index),
                segment_payload,
            )
            .await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_in_progress".to_string(),
            });
        }
        Err(ProxyError::Other(
            "canonical alert group payload chunks are incomplete".to_string(),
        ))
    }

    async fn read_admin_alert_canonical_group_payload(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<AlertGroupRecord, ProxyError> {
        let payload = self
            .read_admin_alert_canonical_group_payload_chunks(snapshot, position, None)
            .await?;
        let group = serde_json::from_str(&payload)
            .map_err(|_| ProxyError::Other("invalid canonical alert group payload".to_string()))?;
        self.complete_admin_alert_canonical_group_payload(snapshot, position, payload)
            .await?;
        Ok(group)
    }

    async fn read_admin_alert_canonical_groups_model(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = async {
            let state_result = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
                "SELECT active_generation, active_row_count, active_projection_revision, \
                        source_recent_generation, source_history_generation \
                 FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
            )
            .fetch_optional(&mut *session)
            .await;
            let Some((generation, row_count, revision, recent, history)) = session.query(state_result).await? else {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_model_unavailable".to_string(),
                });
            };
            if generation != snapshot.build_generation
                || revision != snapshot.projection_revision
                || (recent, history) != snapshot.source_fence
            {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_snapshot_replaced".to_string(),
                });
            }
            let total_result = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM observability.admin_alert_canonical_groups \
                 WHERE build_generation = ? AND position <= ? AND last_seen >= ?",
            )
            .bind(generation)
            .bind(row_count)
            .bind(self.alert_projection_retention_since())
            .fetch_one(&mut *session)
            .await;
            let total = session.query(total_result).await?;
            let rows_result = sqlx::query_as::<_, (i64, String)>(
                "SELECT position, payload_json FROM observability.admin_alert_canonical_groups \
                 WHERE build_generation = ? AND position <= ? AND last_seen >= ? \
                 ORDER BY last_seen DESC, total_count DESC, alert_type DESC, group_id DESC \
                 LIMIT 20 OFFSET 0",
            )
            .bind(generation)
            .bind(row_count)
            .bind(self.alert_projection_retention_since())
            .fetch_all(&mut *session)
            .await;
            Ok::<_, ProxyError>((total, session.query(rows_result).await?))
        }
        .await;
        let finish = session.finish().await;
        finish?;
        let (total, positions) = result?;
        let mut items = Vec::with_capacity(positions.len());
        for (position, payload_json) in positions {
            if payload_json.is_empty() {
                items.push(
                    self.read_admin_alert_canonical_group_payload(snapshot, position)
                        .await?,
                );
            } else {
                items.push(serde_json::from_str(&payload_json).map_err(|_| {
                    ProxyError::Other("invalid canonical alert group payload".to_string())
                })?);
            }
        }
        Ok(PaginatedAlertGroups {
            items,
            total,
            page: 1,
            per_page: 20,
        })
    }
}
