const ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS: i64 = 250;
const ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS: usize = 25;

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
                       build_next_position
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
                                  build_next_position = 1
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
                                  build_next_position = 1
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
                        "admin_alert_canonical_catalog_facets",
                        "admin_alert_canonical_catalog_payloads",
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
                        "admin_alert_canonical_catalog_facets",
                        "admin_alert_canonical_catalog_payloads",
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
                                      build_next_position = 1
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
                 ORDER BY current.rowid ASC
                 LIMIT ?"#,
        )
        .bind(snapshot.build_generation)
        .bind(state.build_cursor_source_rowid)
        .bind(state.build_source_rowid_upper_bound)
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
                let payload_json = row.try_get::<String, _>("payload_json")?;
                let projection = Self::decode_default_alert_event_projection_row(row)?;
                let event = Self::build_alert_event_from_projection(projection);
                let partition_key = event.as_ref().map_or_else(String::new, canonical_alert_group_partition_key);
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
        let fragment = self
            .read_admin_alert_canonical_groups_partition_fragment(
                snapshot,
                &state.build_partition_key,
                state.build_partition_finalize_fragment_position,
            )
            .await?;
        let Some((fragment_position, fragment_events)) = fragment else {
            return self
                .finalize_admin_alert_canonical_groups_partition(snapshot, state)
                .await;
        };
        let mut events = serde_json::from_str::<Vec<AlertEventRecord>>(
            &state.build_partition_events_json,
        )
        .map_err(|_| ProxyError::Other("invalid canonical alert group reduction".to_string()))?;
        events.extend(fragment_events);
        let events_json = serde_json::to_string(&events)
            .map_err(|error| ProxyError::Other(format!("serialize alert group reduction: {error}")))?;
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
                    .bind(events_json)
                    .bind(fragment_position + 1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(partition)
                    .bind(fragment_position)
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
            .record_admin_alerts_canonical_group_build_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }

    async fn finalize_admin_alert_canonical_groups_partition(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let events = serde_json::from_str::<Vec<AlertEventRecord>>(
            &state.build_partition_events_json,
        )
        .map_err(|_| ProxyError::Other("invalid canonical alert group reduction".to_string()))?;
        let groups = build_group_records_from_events(events).top_level_items;
        let mut staged = Vec::with_capacity(groups.len());
        for (index, group) in groups.into_iter().enumerate() {
            let payload_json = serde_json::to_string(&group).map_err(|error| {
                ProxyError::Other(format!("serialize canonical alert group: {error}"))
            })?;
            staged.push((
                state.build_next_position + index as i64,
                group.last_seen,
                group.count,
                group.alert_type,
                group.id,
                payload_json,
            ));
        }
        for chunk in staged.chunks(ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS) {
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let chunk = chunk.to_vec();
            self.sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        for (position, last_seen, total_count, alert_type, group_id, payload_json) in chunk {
                            sqlx::query(
                                r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups
                                       (build_generation, position, last_seen, total_count,
                                        alert_type, group_id, payload_json)
                                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                            )
                            .bind(snapshot.build_generation)
                            .bind(position)
                            .bind(last_seen)
                            .bind(total_count)
                            .bind(alert_type)
                            .bind(group_id)
                            .bind(payload_json)
                            .execute(&mut **tx)
                            .await?;
                        }
                        Ok::<_, ProxyError>(())
                    })
                })
                .await?;
        }
        let next_position = state.build_next_position + staged.len() as i64;
        let partition = state.build_partition_key.clone();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        self.sqlite_runtime
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
                              AND build_projection_revision = ? AND build_phase = 'aggregating'"#,
                    )
                    .bind(partition)
                    .bind(next_position)
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
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
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
        let events_json = serde_json::to_string(&events)
            .map_err(|error| ProxyError::Other(format!("serialize alert group fragment: {error}")))?;
        let fragment_position = state.build_partition_fragment_next_position;
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
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
                    .bind(fragment_position + (!events.is_empty()) as i64)
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
                    if !events.is_empty() {
                        sqlx::query(
                            r#"INSERT INTO observability.admin_alert_canonical_group_fragments
                                   (build_generation, partition_key, position, events_json)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(snapshot.build_generation)
                        .bind(&partition)
                        .bind(fragment_position)
                        .bind(events_json)
                        .execute(&mut **tx)
                        .await?;
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
        let finish = session.finish().await;
        finish?;
        row.map(|(position, events_json)| {
            serde_json::from_str::<Vec<AlertEventRecord>>(&events_json)
                .map(|events| (position, events))
                .map_err(|_| ProxyError::Other("invalid canonical alert group fragment".to_string()))
        })
        .transpose()
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
                                  build_next_position = 1
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
                    let cleanup_remaining = cleanup_events > 0
                        || cleanup_overrides > 0
                        || cleanup_fragments > 0
                        || cleanup_catalog > 0
                        || cleanup_catalog_payloads > 0
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
                            retired_before_remaining || retired_after_remaining || cleanup_remaining,
                        )
                    }
                })
            })
            .await
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
                 WHERE build_generation = ? AND position <= ?",
            )
            .bind(generation)
            .bind(row_count)
            .fetch_one(&mut *session)
            .await;
            let total = session.query(total_result).await?;
            let rows_result = sqlx::query_scalar::<_, String>(
                "SELECT payload_json FROM observability.admin_alert_canonical_groups \
                 WHERE build_generation = ? AND position <= ? \
                 ORDER BY last_seen DESC, total_count DESC, alert_type DESC, group_id DESC \
                 LIMIT 20 OFFSET 0",
            )
            .bind(generation)
            .bind(row_count)
            .fetch_all(&mut *session)
            .await;
            let items = session
                .query(rows_result)
                .await?
                .into_iter()
                .map(|payload_json| {
                    serde_json::from_str(&payload_json).map_err(|_| {
                        ProxyError::Other("invalid canonical alert group payload".to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, ProxyError>(PaginatedAlertGroups {
                items,
                total,
                page: 1,
                per_page: 20,
            })
        }
        .await;
        let finish = session.finish().await;
        finish?;
        result
    }
}
