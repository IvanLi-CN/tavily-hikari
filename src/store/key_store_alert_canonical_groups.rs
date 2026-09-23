const ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS: i64 = 250;
// Reduction output is committed atomically with its cursor CAS. Keep the
// write-side batches bounded by both row count and encoded payload bytes so
// a source page yields before turning one owned transaction into an unbounded
// writer hold.
const ADMIN_ALERT_CANONICAL_GROUPS_CAPTURE_SLICE_ROWS: i64 = 25;
const ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_ROWS: usize =
    ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS as usize;
const ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES: usize = 512 * 1024;
// Keep the read batch bounded while reducing per-batch index/JSON setup for the
// large production-shaped canonical snapshot. Write transactions remain capped
// by the existing 250-row/512KiB ranges below.
const ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCH_ROWS: i64 = 1_000;
// Consume a few independently bounded prefixes within one Groups stage.
const ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCHES_PER_STAGE: usize = 4;
// Batch across partitions without exceeding the former per-partition fast-path
// read budget.
const ADMIN_ALERT_CANONICAL_GROUPS_FAST_SEMANTIC_MAX_BYTES: usize = 2 * 1024 * 1024;
const ADMIN_ALERT_CANONICAL_GROUPS_FAST_SEMANTIC_MAX_FRAGMENTS: i64 = 64;
// Semantic classification uses a bounded page near the existing write budget;
// the native read deadline and 512KiB transaction cap remain independent of it.
const ADMIN_ALERT_CANONICAL_SEMANTIC_CLASSIFY_READ_ROWS: i64 = 18;
const ADMIN_ALERT_CANONICAL_SEMANTIC_CLASSIFY_READ_BYTES: usize = 512 * 1024;
// Keep source reads on the conservative 250ms path while committing their
// bounded rows in short transactions. Historical rowid allocation is
// intentionally not used as a proxy for retained snapshot size.
const ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES: usize = 64 * 1024;
const ADMIN_ALERT_CANONICAL_GROUP_CLEAR_TABLES: &[&str] = &[
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
];

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

fn canonical_alert_fragment_storage_bytes(
    partition_key: &str,
    fragment: &CanonicalAlertFragmentPayload,
) -> usize {
    partition_key.len()
        + match fragment {
            CanonicalAlertFragmentPayload::Events(events_json) => events_json.len(),
            CanonicalAlertFragmentPayload::OversizedEventChunks(chunks) => chunks
                .iter()
                .map(String::len)
                .sum::<usize>()
                .saturating_add(32),
        }
}

fn canonical_alert_event_fragment_payloads_bounded(
    rows: impl IntoIterator<Item = Option<AlertEventRecord>>,
    partition_key: &str,
    max_bytes: usize,
) -> Result<(usize, Vec<CanonicalAlertFragmentPayload>), ProxyError> {
    let mut fragments = Vec::new();
    let mut current_fragment = String::from("[");
    let mut has_event = false;
    let mut accepted_rows = 0_usize;
    let mut committed_bytes = 0_usize;

    for event in rows {
        let Some(event) = event else {
            accepted_rows += 1;
            continue;
        };
        let (_, event_json) = serialize_alert_event_record_for_projection(event)?;
        if event_json.len() + 2 > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES {
            let oversized = CanonicalAlertFragmentPayload::OversizedEventChunks(
                canonical_alert_payload_chunks(&event_json),
            );
            let current = has_event.then(|| {
                CanonicalAlertFragmentPayload::Events(format!("{current_fragment}]"))
            });
            let additional_bytes = current
                .as_ref()
                .map(|fragment| canonical_alert_fragment_storage_bytes(partition_key, fragment))
                .unwrap_or_default()
                .saturating_add(canonical_alert_fragment_storage_bytes(
                    partition_key,
                    &oversized,
                ));
            if accepted_rows > 0 && committed_bytes.saturating_add(additional_bytes) > max_bytes {
                break;
            }
            if let Some(current) = current {
                committed_bytes = committed_bytes
                    .saturating_add(canonical_alert_fragment_storage_bytes(partition_key, &current));
                fragments.push(current);
                current_fragment = String::from("[");
                has_event = false;
            }
            committed_bytes = committed_bytes
                .saturating_add(canonical_alert_fragment_storage_bytes(partition_key, &oversized));
            fragments.push(oversized);
            accepted_rows += 1;
            continue;
        }

        let separator_bytes = usize::from(has_event);
        let candidate_fragment = if has_event
            && current_fragment.len() + separator_bytes + event_json.len() + 1
                > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES
        {
            format!("[{event_json}]")
        } else if has_event {
            format!("{current_fragment},{event_json}]")
        } else {
            format!("[{event_json}]")
        };
        let current_fragment_will_close = has_event
            && current_fragment.len() + separator_bytes + event_json.len() + 1
                > ADMIN_ALERT_CANONICAL_FRAGMENT_MAX_BYTES;
        let additional_bytes = if current_fragment_will_close {
            canonical_alert_fragment_storage_bytes(
                partition_key,
                &CanonicalAlertFragmentPayload::Events(format!("{current_fragment}]")),
            )
            .saturating_add(canonical_alert_fragment_storage_bytes(
                partition_key,
                &CanonicalAlertFragmentPayload::Events(candidate_fragment.clone()),
            ))
        } else {
            canonical_alert_fragment_storage_bytes(
                partition_key,
                &CanonicalAlertFragmentPayload::Events(candidate_fragment.clone()),
            )
        };
        if accepted_rows > 0 && committed_bytes.saturating_add(additional_bytes) > max_bytes {
            break;
        }
        if current_fragment_will_close {
            let current = CanonicalAlertFragmentPayload::Events(format!("{current_fragment}]"));
            committed_bytes = committed_bytes
                .saturating_add(canonical_alert_fragment_storage_bytes(partition_key, &current));
            fragments.push(current);
        }
        current_fragment = if current_fragment_will_close {
            format!("[{event_json}")
        } else if has_event {
            format!("{current_fragment},{event_json}")
        } else {
            format!("[{event_json}")
        };
        has_event = true;
        accepted_rows += 1;
    }

    if has_event {
        fragments.push(CanonicalAlertFragmentPayload::Events(format!(
            "{current_fragment}]"
        )));
    }
    Ok((accepted_rows, fragments))
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

fn canonical_group_write_ranges(
    staged: &[(String, String, i64, String, String, String, String)],
) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut range_start = 0;
    let mut range_bytes = 0_usize;
    for (index, row) in staged.iter().enumerate() {
        let row_bytes =
            row.0.len() + row.1.len() + row.3.len() + row.4.len() + row.5.len() + row.6.len();
        let row_count = index.saturating_sub(range_start);
        if index > range_start
            && (row_count >= ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_ROWS
                || range_bytes.saturating_add(row_bytes)
                    > ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES)
        {
            ranges.push(range_start..index);
            range_start = index;
            range_bytes = 0;
        }
        range_bytes = range_bytes.saturating_add(row_bytes);
    }
    if range_start < staged.len() || staged.is_empty() {
        ranges.push(range_start..staged.len());
    }
    ranges
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
    build_cursor_occurred_at: i64,
    build_cursor_row_sort_id: String,
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

struct FastCanonicalGroupOutput {
    partition_key: String,
    last_seen: i64,
    total_count: i64,
    alert_type: String,
    group_id: String,
    payload_chunks: Vec<String>,
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
        let state = self.load_admin_alert_canonical_groups_state().await?;
        // Builds created before the time-keyset cursor existed have a rowid-only
        // cursor and cannot be resumed safely with the new ordering. Discard
        // only that inactive staged generation; the next call starts a fenced
        // snapshot from the retention boundary without touching business data.
        if state.build_generation > 0
            && state.build_phase == "copying"
            && state.build_cursor_occurred_at == i64::MIN
            && state.build_cursor_row_sort_id.is_empty()
        {
            self.discard_admin_alert_canonical_groups_build(&state)
                .await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_legacy_cursor_reset".to_string(),
            });
        }
        // A staged build is tied to the source fence captured when it started.
        // If either projection lane advances before the next slice, discard
        // the staged generation so it can be rebuilt from one coherent fence.
        // Keep the active generation intact for last-good HTTP responses.
        if state.build_generation > 0 && state.build_source_fence != current_fence {
            self.discard_admin_alert_canonical_groups_build(&state)
                .await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_source_fence_changed".to_string(),
            });
        }
        if state.build_generation == 0
            && state.active_generation > 0
            && state.active_source_fence.0 == current_fence.0
            && state.active_source_fence.1 == current_fence.1
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

        let snapshot = if state.build_generation > 0 {
            AdminAlertsCanonicalSnapshot {
                build_generation: state.build_generation,
                projection_revision: state.build_projection_revision,
                source_fence: state.build_source_fence,
            }
        } else {
            self.start_admin_alert_canonical_groups_build().await?
        };
        if state.build_generation > 0
            && state.build_phase == "aggregating"
            && state.build_cursor_source_rowid == 1
            && self.alert_projection_is_complete().await?
        {
            let request_kinds = Vec::new();
            let filters = AlertEventFilters {
                alert_type: None,
                since: None,
                until: None,
                user_id: None,
                token_id: None,
                key_id: None,
                request_kinds: &request_kinds,
            };
            let (page_items, total) = self
                .fetch_projected_alert_group_page_for_operation(
                    filters,
                    1,
                    20,
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let items = self
                .populate_selected_mother_groups_for_operation(
                    filters,
                    page_items,
                    AlertReadSource::Projected,
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            self.publish_admin_alert_canonical_groups_snapshot(snapshot, total)
                .await?;
            return Ok((
                PaginatedAlertGroups {
                    items,
                    total,
                    page: 1,
                    per_page: 20,
                },
                snapshot,
            ));
        }
        self.advance_admin_alert_canonical_groups_build(snapshot)
            .await?;
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

    pub(crate) async fn admin_alerts_canonical_last_good_for_rehydrate(
        &self,
    ) -> Result<
        Option<(
            AlertCatalog,
            PaginatedAlertEvents,
            PaginatedAlertGroups,
            (i64, i64),
        )>,
        ProxyError,
    > {
        let state = self.load_admin_alert_canonical_groups_state().await?;
        if state.active_generation <= 0 {
            return Ok(None);
        }

        let snapshot = AdminAlertsCanonicalSnapshot {
            build_generation: state.active_generation,
            projection_revision: state.active_projection_revision,
            source_fence: state.active_source_fence,
        };
        let groups = self.read_admin_alert_canonical_groups_model(snapshot).await?;
        let catalog = self
            .admin_alert_canonical_catalog_for_rehydrate(snapshot.build_generation)
            .await?;
        let events = self
            .fetch_admin_alert_events_page_for_canonical_snapshot(
                snapshot.build_generation,
                1,
                20,
            )
            .await?;
        let final_state = self.load_admin_alert_canonical_groups_state().await?;
        if final_state.active_generation != snapshot.build_generation
            || final_state.active_projection_revision != snapshot.projection_revision
            || final_state.active_source_fence != snapshot.source_fence
        {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(Some((catalog, events, groups, snapshot.source_fence)))
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
                       build_cursor_occurred_at,
                       build_cursor_row_sort_id,
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
            build_cursor_occurred_at: row.try_get("build_cursor_occurred_at")?,
            build_cursor_row_sort_id: row.try_get("build_cursor_row_sort_id")?,
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
                    // Build generations are durable identities for staged rows. Never reuse an
                    // old generation while sidecar rows from a discarded build remain, otherwise
                    // the bounded reclaimer must clear a large slot before the replacement can
                    // make progress.
                    let max_staged_generation = sqlx::query_scalar::<_, Option<i64>>(
                        r#"SELECT MAX(build_generation)
                             FROM (
                               SELECT build_generation FROM observability.admin_alert_canonical_groups
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_events
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_overrides
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_fragments
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_reduction_events
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_reduction_children
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_reduction_mothers
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_payload_chunks
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_group_payload_read_chunks_v2
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_catalog_facets
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_catalog_payloads
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_catalog_payload_items
                               UNION ALL
                               SELECT build_generation FROM observability.admin_alert_canonical_catalog_payload_items_v2
                             )"#,
                    )
                    .fetch_one(&mut **tx)
                    .await?
                    .unwrap_or_default();
                    let build_generation = active
                        .max(building)
                        .max(max_staged_generation)
                        .saturating_add(1);
                    sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_generation = ?, build_projection_revision = ?,
                                  build_source_recent_generation = ?, build_source_history_generation = ?,
                                  build_cursor_occurred_at = 9223372036854775807,
                                  build_cursor_row_sort_id = char(0x10ffff), build_source_rowid_upper_bound = ?,
                                  build_cursor_source_rowid = -1, build_phase = 'clearing',
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
            "clearing" => {
                self.clear_admin_alert_canonical_groups_build_slot(snapshot)
                    .await
            }
            "copying" => {
                self.copy_admin_alert_canonical_groups_snapshot_slice(snapshot, state)
                    .await
            }
            "aggregating" => {
                self.aggregate_admin_alert_canonical_groups_partition(snapshot, state)
                    .await
            }
            _ => Err(ProxyError::Other(
                "unknown canonical alert Groups build phase".to_string(),
            )),
        }
    }

    async fn clear_admin_alert_canonical_groups_build_slot(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<(), ProxyError> {
        // Clearing used to delete from every sidecar table and then scan every
        // table for remaining rows inside one 250ms transaction. On the live
        // observability database that transaction could hit the native
        // deadline, leaving the build permanently in `clearing`. Advance one
        // bounded batch from every table so no sidecar table is starved by a
        // large first table.
        let mut made_progress = false;
        for table in ADMIN_ALERT_CANONICAL_GROUP_CLEAR_TABLES {
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let table = *table;
            let (still_current, deleted) = self
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
                            return Ok::<_, ProxyError>((false, false));
                        }
                        let delete = format!(
                            "DELETE FROM observability.{table} WHERE rowid IN ( \
                             SELECT rowid FROM observability.{table} \
                              WHERE build_generation = ? LIMIT 25)"
                        );
                        let deleted = sqlx::query(&delete)
                            .bind(snapshot.build_generation)
                            .execute(&mut **tx)
                            .await?
                            .rows_affected()
                            > 0;
                        Ok::<_, ProxyError>((true, deleted))
                    })
                })
                .await?;
            if !still_current {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_replaced".to_string(),
                });
            }
            if deleted {
                made_progress = true;
            }
        }

        if made_progress {
            self.record_admin_alerts_warm_slice();
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_build_slice();
            return Ok(());
        }

        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let advanced = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_projection_revision = (
                                      SELECT revision
                                        FROM observability.dashboard_alert_projection_revision_state
                                       WHERE singleton = 1
                                  ),
                                  build_source_recent_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_state
                                  ),
                                  build_source_history_generation = (
                                      SELECT COALESCE(SUM(generation), 0)
                                        FROM observability.dashboard_alert_projection_history_state
                                  ),
                                  build_source_rowid_upper_bound = (
                                      SELECT COALESCE(MAX(rowid), 0)
                                        FROM observability.dashboard_alert_projection_events
                                  ),
                                  build_phase = 'copying',
                                  build_cursor_source_rowid = -1,
                                  build_cursor_occurred_at = 9223372036854775807,
                                  build_cursor_row_sort_id = char(0x10ffff),
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
                        == 1;
                    if changed {
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
        // Projection writers encode the source kind into row_sort_id (atl:,
        // maint:, or job:) and use the source's primary key suffix, making
        // (occurred_at, row_sort_id) a stable total order for this snapshot.
        // Keep the source scan on the projection's time index. A writer
        // preserves the pre-build row in the override table before changing a
        // live payload; those immutable values are merged below for this
        // bounded batch instead of putting COALESCE expressions in ORDER BY.
        let rows = {
            // Keep the source scan's 250ms native deadline independent from
            // the optional override lookup below. Both reads are fenced by
            // the projection generation; sharing one session would make the
            // second bounded statement inherit the first statement's elapsed
            // budget and reject otherwise-valid slices on large snapshots.
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let query_result = sqlx::query(
                r#"SELECT rowid AS source_rowid,
                           source_kind, source_id, occurred_at, row_sort_id, payload_json
                      FROM observability.dashboard_alert_projection_events INDEXED BY idx_dashboard_alert_projection_events_time
                     WHERE rowid <= ?
                       AND occurred_at >= ?
                       AND (occurred_at, row_sort_id) < (?, ?)
                     ORDER BY occurred_at DESC, row_sort_id DESC
                     LIMIT ?"#,
            )
            .bind(state.build_source_rowid_upper_bound)
            .bind(self.alert_projection_retention_since())
            .bind(state.build_cursor_occurred_at)
            .bind(&state.build_cursor_row_sort_id)
            .bind(ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(query_result).await?;
            session.finish().await?;
            rows
        };
        let source_ids = rows
            .iter()
            .map(|row| {
                Ok::<_, ProxyError>((
                    row.try_get::<String, _>("source_kind")?,
                    row.try_get::<String, _>("source_id")?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut overrides = StdHashMap::new();
        // New builds begin with no overrides. Projection writers mark the
        // durable build state only when they actually preserve a pre-build
        // row, so the normal liveness path avoids one read session per page.
        // Zero and legacy sentinel values remain conservative and keep the
        // compatibility lookup until the first page establishes the marker.
        if !source_ids.is_empty() && state.build_cursor_source_rowid != -1 {
            // This is a separate bounded read session by design. The source
            // page and override lookup are validated against the same durable
            // fence before the staged rows are committed; if a projection
            // writer advances that fence, the commit CAS rejects the slice.
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let mut overrides_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
                   FROM observability.admin_alert_canonical_group_overrides \
                  WHERE build_generation = ",
            );
            overrides_query.push_bind(snapshot.build_generation);
            overrides_query.push(" AND (source_kind, source_id) IN (");
            for (index, (source_kind, source_id)) in source_ids.iter().enumerate() {
                if index > 0 {
                    overrides_query.push(", ");
                }
                overrides_query
                    .push("(")
                    .push_bind(source_kind)
                    .push(", ")
                    .push_bind(source_id)
                    .push(")");
            }
            overrides_query.push(")");
            let override_query_result = overrides_query.build().fetch_all(&mut *session).await;
            let override_rows = session.query(override_query_result).await?;
            session.finish().await?;
            for row in override_rows {
                overrides.insert(
                    (
                        row.try_get::<String, _>("source_kind")?,
                        row.try_get::<String, _>("source_id")?,
                    ),
                    (
                        row.try_get::<i64, _>("occurred_at")?,
                        row.try_get::<String, _>("row_sort_id")?,
                        row.try_get::<String, _>("payload_json")?,
                    ),
                );
            }
        }
        let overrides_found = !overrides.is_empty();
        let row_count = rows.len();
        let staged: Vec<(String, String, i64, String, String, String, String)> = rows
            .into_iter()
            .map(|row| {
                let source_kind = row.try_get::<String, _>("source_kind")?;
                let source_id = row.try_get::<String, _>("source_id")?;
                let fallback_occurred_at = row.try_get::<i64, _>("occurred_at")?;
                let fallback_cursor_row_sort_id = row.try_get::<String, _>("row_sort_id")?;
                let fallback_payload_json = row.try_get::<String, _>("payload_json")?;
                let (occurred_at, cursor_row_sort_id, payload_json) = overrides
                    .get(&(source_kind.clone(), source_id.clone()))
                    .cloned()
                    .unwrap_or((
                        fallback_occurred_at,
                        fallback_cursor_row_sort_id,
                        fallback_payload_json,
                    ));
                let projection = Self::decode_default_alert_event_projection_payload(
                    &source_kind,
                    &source_id,
                    occurred_at,
                    &cursor_row_sort_id,
                    &payload_json,
                )?;
                // Re-encode the decoded projection before persisting the sidecar row. The live
                // projection may predate the display-size boundary, so copying its raw JSON
                // would reintroduce an oversized diagnostic into the derived model.
                let payload_json = serialize_alert_event_projection_payload(projection.clone())?;
                let event = Self::build_alert_event_from_projection(projection);
                let partition_key = event
                    .as_ref()
                    .map_or_else(String::new, canonical_alert_group_partition_key);
                // The legacy in-memory grouping contract breaks ties by the
                // canonical AlertEventRecord identity. Keep that identity in
                // the sidecar seek key so a resumable reducer cannot select a
                // different latest event when projection row_sort_id uses a
                // different encoding (for example, numeric source IDs).
                let row_sort_id = event
                    .as_ref()
                    .map(|event| event.id.clone())
                    .unwrap_or_else(|| cursor_row_sort_id.clone());
                Ok::<_, ProxyError>((
                    source_kind,
                    source_id,
                    occurred_at,
                    cursor_row_sort_id,
                    row_sort_id,
                    partition_key,
                    payload_json,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let complete = row_count < ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS as usize;
        let write_ranges = canonical_group_write_ranges(&staged);
        let chunk_count = write_ranges.len();
        let mut expected_cursor = (
            state.build_cursor_occurred_at,
            state.build_cursor_row_sort_id.clone(),
        );
        for (chunk_index, range) in write_ranges.iter().enumerate() {
            let chunk = &staged[range.clone()];
            let chunk_cursor = chunk
                .last()
                .map(|row| (row.2, row.3.clone()))
                .unwrap_or_else(|| expected_cursor.clone());
            let chunk_complete = chunk_index + 1 == chunk_count && complete;
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let chunk = chunk.to_vec();
            let prior_cursor = expected_cursor.clone();
            let committed_cursor = chunk_cursor.clone();
            let advanced = self
                .sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        let valid = sqlx::query_scalar::<_, bool>(
                            r#"SELECT EXISTS(
                                 SELECT 1
                                   FROM observability.admin_alert_canonical_groups_state
                                  WHERE singleton = 1 AND build_generation = ?
                                    AND build_projection_revision = ?
                                    AND build_source_recent_generation = ?
                                    AND build_source_history_generation = ?
                                    AND build_source_rowid_upper_bound = ?
                                    AND build_cursor_occurred_at = ?
                                    AND build_cursor_row_sort_id = ?
                                    AND build_phase = 'copying'
                               )"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(snapshot.source_fence.0)
                        .bind(snapshot.source_fence.1)
                        .bind(state.build_source_rowid_upper_bound)
                        .bind(prior_cursor.0)
                        .bind(&prior_cursor.1)
                        .fetch_one(&mut **tx)
                        .await?;
                        if !valid {
                            return Ok::<_, ProxyError>(false);
                        }
                        if !chunk.is_empty() {
                            let mut insert = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                                r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_events
                                       (build_generation, source_kind, source_id, occurred_at, row_sort_id,
                                        partition_key, payload_json)
                                   "#,
                            );
                            insert.push_values(
                                chunk,
                                |mut values,
                                 (
                                     source_kind,
                                     source_id,
                                     occurred_at,
                                     _cursor_row_sort_id,
                                     row_sort_id,
                                     partition_key,
                                     payload_json,
                                 )| {
                                    values
                                        .push_bind(snapshot.build_generation)
                                        .push_bind(source_kind)
                                        .push_bind(source_id)
                                        .push_bind(occurred_at)
                                        .push_bind(row_sort_id)
                                        .push_bind(partition_key)
                                        .push_bind(payload_json);
                                },
                            );
                            insert.build().execute(&mut **tx).await?;
                        }
                        let changed = sqlx::query(
                            r#"UPDATE observability.admin_alert_canonical_groups_state
                                  SET build_cursor_source_rowid = CASE
                                          WHEN build_cursor_source_rowid = 1 OR ? THEN 1
                                          ELSE -1
                                      END,
                                      build_cursor_occurred_at = ?,
                                      build_cursor_row_sort_id = ?,
                                      build_phase = CASE WHEN ? THEN 'aggregating' ELSE 'copying' END
                                WHERE singleton = 1 AND build_generation = ?
                                  AND build_projection_revision = ?
                                  AND build_cursor_occurred_at = ?
                                  AND build_cursor_row_sort_id = ?
                                  "#,
                        )
                        .bind(overrides_found)
                        .bind(committed_cursor.0)
                        .bind(&committed_cursor.1)
                        .bind(chunk_complete)
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(prior_cursor.0)
                        .bind(&prior_cursor.1)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected();
                        Ok::<_, ProxyError>(changed == 1)
                    })
                })
                .await?;
            if !advanced {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_replaced".to_string(),
                });
            }
            expected_cursor = chunk_cursor;
            self.record_admin_alerts_warm_slice();
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_build_slice();
        }
        Ok(())
    }

    async fn aggregate_admin_alert_canonical_groups_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        if state.build_partition_key.is_empty() {
            let mut state = state;
            let mut made_progress = false;
            for batch_index in 0..ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCHES_PER_STAGE {
                if !self
                    .batch_admin_alert_canonical_compat_groups(snapshot, &state)
                    .await?
                {
                    return if made_progress {
                        Ok(())
                    } else {
                        self.select_admin_alert_canonical_groups_partition(snapshot, state)
                            .await
                    };
                }
                made_progress = true;
                if batch_index + 1 == ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCHES_PER_STAGE {
                    break;
                }

                tokio::task::yield_now().await;
                state = self.load_admin_alert_canonical_groups_state().await?;
                if state.build_generation == 0 {
                    return Ok(());
                }
                if state.build_generation != snapshot.build_generation {
                    return Err(ProxyError::Deferred {
                        operation: "admin_alerts_cache_warm",
                        reason: "groups_build_replaced".to_string(),
                    });
                }
                if state.build_phase != "aggregating" || !state.build_partition_key.is_empty() {
                    return Ok(());
                }
            }
            return Ok(());
        }
        if !state.build_partition_source_complete {
            return self
                .capture_admin_alert_canonical_groups_partition_slice(snapshot, state)
                .await;
        }
        self.finalize_admin_alert_canonical_groups_partition(snapshot, state)
            .await
    }

    async fn batch_admin_alert_canonical_compat_groups(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: &AdminAlertCanonicalGroupsState,
    ) -> Result<bool, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let partition_result = sqlx::query_as::<_, (String, i64, i64, i64)>(
            "SELECT partition_key, COUNT(*) AS event_count, MIN(occurred_at), MAX(occurred_at) \
               FROM observability.admin_alert_canonical_group_events \
              WHERE build_generation = ? AND partition_key > ? \
              GROUP BY partition_key \
              ORDER BY partition_key ASC LIMIT ?",
        )
        .bind(snapshot.build_generation)
        .bind(&state.build_partition_after_key)
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCH_ROWS)
        .fetch_all(&mut *session)
        .await;
        let partitions = session.query(partition_result).await?;
        session.finish().await?;
        if partitions.is_empty() {
            return Ok(false);
        }

        let mut simple_partition_count = 0_usize;
        let mut outputs = Vec::new();
        let mut fragments = StdHashMap::<String, String>::new();
        let first_partition = partitions[0].0.clone();
        let last_partition = partitions
            .last()
            .map(|(key, _, _, _)| key.clone())
            .unwrap_or_default();
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let fragment_result = sqlx::query_as::<_, (String, String)>(
            "SELECT fragments.partition_key, fragments.events_json \
               FROM observability.admin_alert_canonical_group_fragments AS fragments \
               JOIN ( \
                    SELECT partition_key, MAX(position) AS position \
                      FROM observability.admin_alert_canonical_group_fragments \
                     WHERE build_generation = ? AND partition_key >= ? AND partition_key <= ? \
                     GROUP BY partition_key \
                     ORDER BY partition_key ASC LIMIT ? \
               ) AS latest \
                 ON latest.partition_key = fragments.partition_key \
                AND latest.position = fragments.position \
              WHERE fragments.build_generation = ? \
              ORDER BY fragments.partition_key ASC",
        )
        .bind(snapshot.build_generation)
        .bind(&first_partition)
        .bind(&last_partition)
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_FAST_COMPAT_BATCH_ROWS)
        .bind(snapshot.build_generation)
        .fetch_all(&mut *session)
        .await;
        for row in session.query(fragment_result).await? {
            fragments.insert(row.0, row.1);
        }
        session.finish().await?;

        // Unfragmented singleton compatibility rows need only their aggregate and event.
        // Resolve them from this immutable generation instead of capturing one fragment
        // per high-cardinality partition; multi-event groups still require fragments.
        let missing_partitions = partitions
            .iter()
            .filter(|(partition_key, event_count, _, _)| {
                *event_count == 1 && !fragments.contains_key(partition_key)
            })
            .collect::<Vec<_>>();
        let mut latest_events = StdHashMap::<String, AlertEventRecord>::new();
        if !missing_partitions.is_empty() {
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let mut latest_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "WITH requested(partition_key) AS (VALUES ",
            );
            for (index, (partition_key, _, _, _)) in missing_partitions.iter().enumerate() {
                if index > 0 {
                    latest_query.push(", ");
                }
                latest_query.push("(").push_bind(partition_key).push(")");
            }
            latest_query.push(
                ") SELECT requested.partition_key AS batch_partition_key, \
                         event.source_kind, event.source_id, event.occurred_at, \
                         event.row_sort_id, event.payload_json \
                    FROM requested \
                    JOIN observability.admin_alert_canonical_group_events AS event \
                      ON event.build_generation = ",
            );
            latest_query.push_bind(snapshot.build_generation);
            latest_query.push(
                " AND event.rowid = ( \
                        SELECT latest.rowid \
                          FROM observability.admin_alert_canonical_group_events AS latest \
                         WHERE latest.build_generation = ",
            );
            latest_query.push_bind(snapshot.build_generation);
            latest_query.push(
                " AND latest.partition_key = requested.partition_key \
                         ORDER BY latest.occurred_at DESC, latest.row_sort_id DESC LIMIT 1 \
                    ) \
                    ORDER BY requested.partition_key",
            );
            let latest_result = latest_query.build().fetch_all(&mut *session).await;
            let latest_rows = session.query(latest_result).await?;
            session.finish().await?;
            for row in latest_rows {
                let partition_key = row.try_get::<String, _>("batch_partition_key")?;
                let event = Self::decode_default_alert_event_projection_row(row)
                    .ok()
                    .and_then(Self::build_alert_event_from_projection);
                if let Some(event) = event {
                    latest_events.insert(partition_key, event);
                }
            }
        }

        let semantic_partition_event_counts = partitions
            .iter()
            .filter_map(|(partition_key, event_count, _, _)| {
                if *event_count <= 1 {
                    return None;
                }
                let fragment_json = fragments.get(partition_key)?;
                serde_json::from_str::<Vec<AlertEventRecord>>(fragment_json)
                    .ok()?
                    .iter()
                    .any(|event| event.semantic_window.is_some())
                    .then(|| (partition_key.clone(), *event_count))
            })
            .take(ADMIN_ALERT_CANONICAL_GROUPS_FAST_SEMANTIC_MAX_FRAGMENTS as usize)
            .collect::<Vec<_>>();
        let semantic_events_by_partition = self
            .read_admin_alert_canonical_semantic_partition_batch_for_fast_path(
                snapshot,
                &semantic_partition_event_counts,
            )
            .await?;

        for (partition_key, event_count, first_seen, last_seen) in partitions {
            let partition_events = if let Some(fragment_json) = fragments.get(&partition_key) {
                let Ok(events) = serde_json::from_str::<Vec<AlertEventRecord>>(fragment_json)
                else {
                    break;
                };
                events
            } else if event_count == 1 {
                let Some(event) = latest_events.get(&partition_key) else {
                    break;
                };
                vec![event.clone()]
            } else {
                break;
            };
            let Some(event) = partition_events.last() else {
                break;
            };
            let groups = if partition_events
                .iter()
                .any(|event| event.semantic_window.is_some())
            {
                let semantic_events = if event_count == 1 {
                    partition_events
                } else {
                    let Some(semantic_events) = semantic_events_by_partition.get(&partition_key)
                    else {
                        break;
                    };
                    semantic_events.clone()
                };
                if semantic_events.iter().any(|event| {
                    event.semantic_window.is_none()
                        || canonical_alert_group_partition_key(event) != partition_key
                }) {
                    break;
                }
                let mothers = build_semantic_mother_groups(build_semantic_child_windows(
                    semantic_events,
                ));
                if mothers.is_empty() {
                    break;
                }
                mothers
            } else {
                if canonical_alert_group_partition_key(event) != partition_key {
                    break;
                }
                let Some(mut group) = build_compat_group_record(std::slice::from_ref(event)) else {
                    break;
                };
                group.count = event_count;
                group.event_count = event_count;
                group.first_seen = first_seen;
                group.last_seen = last_seen;
                vec![group]
            };
            for group in groups {
                let payload_json = serde_json::to_string(&group).map_err(|error| {
                    ProxyError::Other(format!("serialize fast canonical alert group: {error}"))
                })?;
                outputs.push(FastCanonicalGroupOutput {
                    partition_key: partition_key.clone(),
                    last_seen: group.last_seen,
                    total_count: group.count,
                    alert_type: group.alert_type,
                    group_id: group.id,
                    payload_chunks: canonical_alert_payload_chunks(&payload_json),
                });
                simple_partition_count += 1;
            }
        }
        if simple_partition_count == 0 {
            return Ok(false);
        }

        let mut range_start = 0_usize;
        while range_start < outputs.len() {
            let mut range_end = range_start;
            let mut range_bytes = 0_usize;
            while range_end < outputs.len() {
                let output = &outputs[range_end];
                let output_bytes = output
                    .partition_key
                    .len()
                    .saturating_add(output.alert_type.len())
                    .saturating_add(output.group_id.len())
                    .saturating_add(output.payload_chunks.iter().map(String::len).sum::<usize>());
                let output_count = range_end.saturating_sub(range_start);
                if range_end > range_start
                    && (output_count >= ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_ROWS
                        || range_bytes.saturating_add(output_bytes)
                            > ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES)
                {
                    break;
                }
                range_bytes = range_bytes.saturating_add(output_bytes);
                range_end += 1;
            }
            let range = &outputs[range_start..range_end];
            let prior_after_key = if range_start == 0 {
                state.build_partition_after_key.clone()
            } else {
                outputs[range_start - 1].partition_key.clone()
            };
            let next_after_key = range.last().map(|output| output.partition_key.clone()).ok_or_else(|| {
                ProxyError::Other("fast canonical Groups batch produced no output".to_string())
            })?;
            let prior_position = state.build_next_position
                + i64::try_from(range_start).map_err(|_| {
                    ProxyError::Other("fast canonical Groups position overflow".to_string())
                })?;
            let next_position = prior_position
                + i64::try_from(range.len()).map_err(|_| {
                    ProxyError::Other("fast canonical Groups position overflow".to_string())
                })?;
            let build_generation = snapshot.build_generation;
            let projection_revision = snapshot.projection_revision;
            let source_recent_generation = snapshot.source_fence.0;
            let source_history_generation = snapshot.source_fence.1;
            let range = range
                .iter()
                .map(|output| {
                    (
                        output.partition_key.clone(),
                        output.last_seen,
                        output.total_count,
                        output.alert_type.clone(),
                        output.group_id.clone(),
                        output.payload_chunks.clone(),
                    )
                })
                .collect::<Vec<_>>();
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let changed = self
                .sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        let owner = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                              WHERE singleton = 1 AND build_generation = ? \
                                AND build_projection_revision = ? \
                                AND build_source_recent_generation = ? \
                                AND build_source_history_generation = ? \
                                AND build_phase = 'aggregating' AND build_partition_key = '' \
                                AND build_partition_after_key = ? AND build_next_position = ?)",
                        )
                        .bind(build_generation)
                        .bind(projection_revision)
                        .bind(source_recent_generation)
                        .bind(source_history_generation)
                        .bind(&prior_after_key)
                        .bind(prior_position)
                        .fetch_one(&mut **tx)
                        .await?;
                        if !owner {
                            return Ok::<_, ProxyError>(false);
                        }
                        for (index, (_, last_seen, total_count, alert_type, group_id, chunks)) in
                            range.into_iter().enumerate()
                        {
                            let position = prior_position + index as i64;
                            for (chunk_position, payload_chunk) in chunks.into_iter().enumerate() {
                                sqlx::query(
                                    "INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks \
                                       (build_generation, position, chunk_position, payload_chunk) \
                                     VALUES (?, ?, ?, ?)",
                                )
                                .bind(build_generation)
                                .bind(position)
                                .bind(chunk_position as i64)
                                .bind(payload_chunk)
                                .execute(&mut **tx)
                                .await?;
                            }
                            sqlx::query(
                                "INSERT OR REPLACE INTO observability.admin_alert_canonical_groups \
                                   (build_generation, position, last_seen, total_count, alert_type, group_id, payload_json) \
                                 VALUES (?, ?, ?, ?, ?, ?, '')",
                            )
                            .bind(build_generation)
                            .bind(position)
                            .bind(last_seen)
                            .bind(total_count)
                            .bind(alert_type)
                            .bind(group_id)
                            .execute(&mut **tx)
                            .await?;
                        }
                        let updated = sqlx::query(
                            "UPDATE observability.admin_alert_canonical_groups_state \
                                SET build_partition_after_key = ?, build_next_position = ? \
                              WHERE singleton = 1 AND build_generation = ? \
                                AND build_projection_revision = ? \
                                AND build_source_recent_generation = ? \
                                AND build_source_history_generation = ? \
                                AND build_phase = 'aggregating' AND build_partition_key = '' \
                                AND build_partition_after_key = ? AND build_next_position = ?",
                        )
                        .bind(&next_after_key)
                        .bind(next_position)
                        .bind(build_generation)
                        .bind(projection_revision)
                        .bind(source_recent_generation)
                        .bind(source_history_generation)
                        .bind(&prior_after_key)
                        .bind(prior_position)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected();
                        Ok(updated == 1)
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
                .record_admin_alerts_canonical_group_reduction_slice();
            range_start = range_end;
        }
        Ok(true)
    }

    async fn read_admin_alert_canonical_semantic_partition_batch_for_fast_path(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partitions: &[(String, i64)],
    ) -> Result<StdHashMap<String, Vec<AlertEventRecord>>, ProxyError> {
        if partitions.is_empty() {
            return Ok(StdHashMap::new());
        }
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT partition_key, position, events_json \
               FROM observability.admin_alert_canonical_group_fragments \
              WHERE build_generation = ",
        );
        query.push_bind(snapshot.build_generation);
        query.push(" AND partition_key IN (");
        for (index, (partition_key, _)) in partitions.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push_bind(partition_key);
        }
        query.push(") ORDER BY partition_key ASC, position ASC LIMIT ");
        query.push_bind(ADMIN_ALERT_CANONICAL_GROUPS_FAST_SEMANTIC_MAX_FRAGMENTS);
        let rows_result = query
            .build_query_as::<(String, i64, String)>()
            .fetch_all(&mut *session)
            .await;
        let rows = session.query(rows_result).await?;
        session.finish().await?;

        let expected_event_counts = partitions.iter().cloned().collect::<StdHashMap<_, _>>();
        let mut complete = StdHashMap::new();
        let mut current_partition: Option<String> = None;
        let mut current_fragment_count = 0_i64;
        let mut current_events = Vec::new();
        let mut current_event_count = 0_i64;
        let mut total_bytes = 0_usize;
        for (partition_key, position, events_json) in rows {
            if current_partition.as_deref() != Some(partition_key.as_str()) {
                if let Some(prior_partition) = current_partition.take()
                    && current_event_count
                        != expected_event_counts
                            .get(&prior_partition)
                            .copied()
                            .unwrap_or_default()
                {
                    break;
                }
                current_partition = Some(partition_key.clone());
                current_fragment_count = 0;
                current_event_count = 0;
            }
            total_bytes = total_bytes.saturating_add(events_json.len());
            if total_bytes > ADMIN_ALERT_CANONICAL_GROUPS_FAST_SEMANTIC_MAX_BYTES {
                break;
            }
            if position != current_fragment_count + 1 {
                break;
            }
            let Ok(events) = serde_json::from_str::<Vec<AlertEventRecord>>(&events_json) else {
                break;
            };
            current_event_count += events.len() as i64;
            current_events.extend(events);
            current_fragment_count += 1;
            let expected_event_count = expected_event_counts
                .get(&partition_key)
                .copied()
                .unwrap_or_default();
            if current_event_count > expected_event_count {
                break;
            }
            if current_event_count == expected_event_count {
                if expected_event_count <= 0 {
                    break;
                }
                complete.insert(partition_key, std::mem::take(&mut current_events));
                current_partition = None;
                current_fragment_count = 0;
                current_event_count = 0;
            }
        }
        Ok(complete)
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

        let current =
            serde_json::from_str::<CompatGroupReductionState>(&state.build_partition_events_json)
                .ok();
        let Some(latest_event) = events.last().cloned() else {
            return Err(ProxyError::Other(
                "canonical alert group fragment cannot be empty".to_string(),
            ));
        };
        let next = CompatGroupReductionState {
            event_count: current.as_ref().map_or(events.len() as i64, |reduction| {
                reduction.event_count + events.len() as i64
            }),
            first_seen: current
                .as_ref()
                .map_or_else(|| events[0].occurred_at, |reduction| reduction.first_seen),
            latest_event,
        };
        let reduction_json = serde_json::to_string(&next).map_err(|error| {
            ProxyError::Other(format!(
                "serialize canonical compat reduction state: {error}"
            ))
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
                              AND build_source_recent_generation = ?
                              AND build_source_history_generation = ?
                              AND build_partition_key = ?
                              AND build_partition_finalize_fragment_position = ?
                              "#,
                    )
                    .bind(reduction_json)
                    .bind(fragment_position + 1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
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
        let reduction =
            serde_json::from_str::<CompatGroupReductionState>(&state.build_partition_events_json)
                .map_err(|_| {
                ProxyError::Other("canonical compat reduction state is unavailable".to_string())
            })?;
        let mut group = build_compat_group_record(std::slice::from_ref(&reduction.latest_event))
            .ok_or_else(|| {
                ProxyError::Other("canonical compat reduction has no latest event".to_string())
            })?;
        group.count = reduction.event_count;
        group.event_count = reduction.event_count;
        group.first_seen = reduction.first_seen;
        let payload_json = serde_json::to_string(&group).map_err(|error| {
            ProxyError::Other(format!("serialize canonical alert group: {error}"))
        })?;
        let position = state.build_next_position;
        let payload_chunks = canonical_alert_payload_chunks(&payload_json);
        let partition = state.build_partition_key.clone();
        let expected_finalize_position = state.build_partition_finalize_fragment_position;
        let build_generation = snapshot.build_generation;
        let projection_revision = snapshot.projection_revision;
        let source_recent_generation = snapshot.source_fence.0;
        let source_history_generation = snapshot.source_fence.1;
        let last_seen = group.last_seen;
        let count = group.count;
        let alert_type = group.alert_type.clone();
        let group_id = group.id.clone();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let finalized = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    for (chunk_position, payload_chunk) in payload_chunks.into_iter().enumerate() {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks
                                   (build_generation, position, chunk_position, payload_chunk)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(build_generation)
                        .bind(position)
                        .bind(chunk_position as i64)
                        .bind(payload_chunk)
                        .execute(&mut **tx)
                        .await?;
                    }
                    sqlx::query(
                        r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups
                               (build_generation, position, last_seen, total_count,
                                alert_type, group_id, payload_json)
                           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(build_generation)
                    .bind(position)
                    .bind(last_seen)
                    .bind(count)
                    .bind(alert_type)
                    .bind(group_id)
                    .bind("")
                    .execute(&mut **tx)
                    .await?;
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
                              AND build_source_recent_generation = ?
                              AND build_source_history_generation = ?
                              AND build_partition_key = ?
                              AND build_partition_finalize_fragment_position = ?
                              "#,
                    )
                    .bind(&partition)
                    .bind(position + 1)
                    .bind(build_generation)
                    .bind(projection_revision)
                    .bind(source_recent_generation)
                    .bind(source_history_generation)
                    .bind(&partition)
                    .bind(expected_finalize_position)
                    .execute(&mut **tx)
                    .await?;
                    if changed.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_build_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        debug_assert!(finalized);
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
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_CAPTURE_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await;
        let finish = session.finish().await;
        finish?;
        let decoded_rows = rows?
            .into_iter()
            .map(|row| {
                let cursor = (
                    row.try_get::<i64, _>("occurred_at")?,
                    row.try_get::<String, _>("row_sort_id")?,
                );
                let event = Self::build_alert_event_from_projection(
                    Self::decode_default_alert_event_projection_row(row)?,
                );
                Ok::<_, ProxyError>((cursor, event))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (accepted_rows, fragments) = canonical_alert_event_fragment_payloads_bounded(
            decoded_rows.iter().map(|(_, event)| event.clone()),
            &partition,
            ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES,
        )?;
        let complete = accepted_rows == decoded_rows.len()
            && decoded_rows.len() < ADMIN_ALERT_CANONICAL_GROUPS_CAPTURE_SLICE_ROWS as usize;
        let next_cursor = decoded_rows
            .get(accepted_rows.saturating_sub(1))
            .map(|row| row.0.clone())
            .unwrap_or_else(|| state.build_partition_cursor.clone());
        let fragment_position = state.build_partition_fragment_next_position;
        let fragment_count = fragments.len();
        let mut fragment_writes = Vec::with_capacity(fragment_count);
        for (offset, fragment_payload) in fragments.into_iter().enumerate() {
            let position = fragment_position + offset as i64;
            let (events_json, oversized_chunks) = match fragment_payload {
                CanonicalAlertFragmentPayload::Events(events_json) => (events_json, None),
                CanonicalAlertFragmentPayload::OversizedEventChunks(chunks) => {
                    let marker = format!(r#"{{"__canonical_event_chunks":{}}}"#, chunks.len());
                    (marker, Some(chunks))
                }
            };
            fragment_writes.push((position, events_json, oversized_chunks.unwrap_or_default()));
        }
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let next_fragment_position = fragment_position + fragment_count as i64;
        let build_generation = snapshot.build_generation;
        let projection_revision = snapshot.projection_revision;
        let expected_cursor = state.build_partition_cursor.clone();
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    for (position, events_json, chunks) in fragment_writes {
                        sqlx::query(
                            r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_fragments
                                   (build_generation, partition_key, position, events_json)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(build_generation)
                        .bind(&partition)
                        .bind(position)
                        .bind(events_json)
                        .execute(&mut **tx)
                        .await?;
                        for (chunk_position, payload_chunk) in chunks.into_iter().enumerate() {
                            sqlx::query(
                                r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_chunks
                                       (build_generation, position, chunk_position, payload_chunk)
                                   VALUES (?, ?, ?, ?)"#,
                            )
                            .bind(build_generation)
                            .bind(-position)
                            .bind(chunk_position as i64)
                            .bind(payload_chunk)
                            .execute(&mut **tx)
                            .await?;
                        }
                    }
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_cursor_occurred_at = ?,
                                  build_partition_cursor_row_sort_id = ?,
                                  build_partition_source_complete = ?,
                                  build_partition_fragment_next_position = ?
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ? AND build_phase = 'aggregating'
                              AND build_source_recent_generation = ?
                              AND build_source_history_generation = ?
                              AND build_partition_key = ?
                              AND build_partition_cursor_occurred_at = ?
                              AND build_partition_cursor_row_sort_id = ?
                              "#,
                    )
                    .bind(next_cursor.0)
                    .bind(next_cursor.1)
                    .bind(complete)
                    .bind(next_fragment_position)
                    .bind(build_generation)
                    .bind(projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(&partition)
                    .bind(expected_cursor.0)
                    .bind(expected_cursor.1)
                        .execute(&mut **tx)
                        .await?;
                    if changed.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_build_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        debug_assert!(changed);
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    async fn read_admin_alert_canonical_groups_partition_fragments(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        position: i64,
    ) -> Result<Vec<(i64, Vec<AlertEventRecord>)>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let rows_result = sqlx::query_as::<_, (i64, String)>(
            "SELECT position, events_json \
               FROM observability.admin_alert_canonical_group_fragments \
              WHERE build_generation = ? AND partition_key = ? AND position >= ? \
              ORDER BY position ASC LIMIT ?",
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(position)
        .bind(ADMIN_ALERT_CANONICAL_SEMANTIC_CLASSIFY_READ_ROWS + 1)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await?;
        session.finish().await?;

        let mut selected = Vec::new();
        let mut selected_bytes = 0_usize;
        for (position, events_json) in rows {
            if !selected.is_empty()
                && (selected.len() >= ADMIN_ALERT_CANONICAL_SEMANTIC_CLASSIFY_READ_ROWS as usize
                    || selected_bytes.saturating_add(events_json.len())
                        > ADMIN_ALERT_CANONICAL_SEMANTIC_CLASSIFY_READ_BYTES)
            {
                break;
            }
            selected_bytes = selected_bytes.saturating_add(events_json.len());
            selected.push((position, events_json));
        }

        let mut fragments = Vec::with_capacity(selected.len());
        for (position, events_json) in selected {
            if let Ok(events) = serde_json::from_str::<Vec<AlertEventRecord>>(&events_json) {
                fragments.push((position, events));
                continue;
            }
            let marker = serde_json::from_str::<CanonicalAlertOversizedEventMarker>(&events_json)
                .map_err(|_| ProxyError::Other("invalid canonical alert group fragment".to_string()))?;
            if marker.chunk_count <= 0 {
                return Err(ProxyError::Other(
                    "canonical oversized alert event has no chunks".to_string(),
                ));
            }
            let event_json = self
                .read_admin_alert_canonical_group_payload_chunks(
                    snapshot,
                    -position,
                    Some(marker.chunk_count),
                )
                .await?;
            let event = serde_json::from_str::<AlertEventRecord>(&event_json).map_err(|_| {
                ProxyError::Other("invalid canonical oversized alert event".to_string())
            })?;
            self.clear_admin_alert_canonical_group_payload_read(snapshot, -position)
                .await?;
            fragments.push((position, vec![event]));
        }
        Ok(fragments)
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
            .read_admin_alert_canonical_group_payload_chunks(
                snapshot,
                -position,
                Some(marker.chunk_count),
            )
            .await?;
        let event = serde_json::from_str::<AlertEventRecord>(&event_json).map_err(|_| {
            ProxyError::Other("invalid canonical oversized alert event".to_string())
        })?;
        self.clear_admin_alert_canonical_group_payload_read(snapshot, -position)
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
                .publish_admin_alert_canonical_groups_snapshot(
                    snapshot,
                    state.build_next_position - 1,
                )
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
        let Some(_canonical_publish_gate) = self
            .sqlite_runtime
            .acquire_admin_alerts_canonical_publish_gate()
            .await
        else {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "projection_publish_busy".to_string(),
            });
        };
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
                              AND build_source_recent_generation = ?
                              AND build_source_history_generation = ?
                              "#,
                    )
                    .bind(snapshot.build_generation)
                    .bind(row_count)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
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
        self.ensure_admin_alerts_cache_warm_reclaimer_write_admitted()?;
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
                                  WHERE build_generation <> ? AND build_generation <> ?
                                  ORDER BY build_generation ASC, source_kind ASC, source_id ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
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
                             WHERE build_generation <> ? AND build_generation <> ?)",
                        )
                        .bind(active_generation)
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

}

impl KeyStore {
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
            let Some((generation, row_count, revision, recent, history)) =
                session.query(state_result).await?
            else {
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

#[cfg(test)]
mod canonical_group_tests {
    use super::{
        ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES,
        ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_ROWS, canonical_group_write_ranges,
    };

    fn staged_row(payload: &str) -> (String, String, i64, String, String, String, String) {
        (
            "alert".to_string(),
            "source".to_string(),
            1,
            "cursor".to_string(),
            "id".to_string(),
            "group".to_string(),
            payload.to_string(),
        )
    }

    #[test]
    fn canonical_group_write_ranges_keep_normal_source_page_in_one_batch() {
        let staged = (0..250)
            .map(|index| staged_row(&index.to_string()))
            .collect::<Vec<_>>();
        let ranges = canonical_group_write_ranges(&staged);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..staged.len());
        assert!(ranges.iter().all(|range| {
            range.end.saturating_sub(range.start)
                <= ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_ROWS
        }));
        assert_eq!(ranges.last().map(|range| range.end), Some(staged.len()));
    }

    #[test]
    fn canonical_group_write_ranges_shrink_for_large_payloads() {
        let payload = "x".repeat(300 * 1024);
        let staged = vec![staged_row(&payload); 3];
        let ranges = canonical_group_write_ranges(&staged);

        assert_eq!(ranges.len(), 3);
        assert!(ranges.iter().all(|range| {
            let bytes = range
                .clone()
                .map(|index| staged[index].6.len())
                .sum::<usize>();
            bytes <= ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_MAX_BYTES
        }));
    }
}
