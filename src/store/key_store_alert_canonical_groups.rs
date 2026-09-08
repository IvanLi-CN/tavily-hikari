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
    build_cursor: (i64, String),
    build_phase: String,
    build_partition_key: String,
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
                       build_cursor_occurred_at, build_cursor_row_sort_id, build_phase,
                       build_partition_key, build_next_position
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
            build_cursor: (
                row.try_get("build_cursor_occurred_at")?,
                row.try_get("build_cursor_row_sort_id")?,
            ),
            build_phase: row.try_get("build_phase")?,
            build_partition_key: row.try_get("build_partition_key")?,
            build_next_position: row.try_get("build_next_position")?,
        })
    }

    async fn start_admin_alert_canonical_groups_build(
        &self,
    ) -> Result<AdminAlertsCanonicalSnapshot, ProxyError> {
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, |tx| {
                Box::pin(async move {
                    let (revision, recent, history, active, building) = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
                        r#"SELECT
                               (SELECT revision FROM observability.dashboard_alert_projection_revision_state WHERE singleton = 1),
                               (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state),
                               (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_history_state),
                               active_generation,
                               build_generation
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
                    let build_generation = active.max(building).saturating_add(1).max(1);
                    sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_generation = ?, build_projection_revision = ?,
                                  build_source_recent_generation = ?, build_source_history_generation = ?,
                                  build_cursor_occurred_at = -9223372036854775808,
                                  build_cursor_row_sort_id = '', build_phase = 'copying',
                                  build_partition_key = '', build_next_position = 1
                            WHERE singleton = 1"#,
                    )
                    .bind(build_generation)
                    .bind(revision)
                    .bind(recent)
                    .bind(history)
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
            "copying" => self.copy_admin_alert_canonical_groups_snapshot_slice(snapshot, state).await,
            "aggregating" => self.aggregate_admin_alert_canonical_groups_partition(snapshot, state).await,
            _ => Err(ProxyError::Other("unknown canonical alert Groups build phase".to_string())),
        }
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
            r#"WITH snapshot_events AS (
                    SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json
                      FROM observability.dashboard_alert_projection_events
                     WHERE projection_revision <= ?
                    UNION ALL
                    SELECT override.source_kind, override.source_id, override.occurred_at,
                           override.row_sort_id, override.payload_json
                      FROM observability.admin_alert_canonical_group_overrides AS override
                      JOIN observability.dashboard_alert_projection_events AS current
                        ON current.source_kind = override.source_kind
                       AND current.source_id = override.source_id
                     WHERE override.build_generation = ?
                       AND current.projection_revision > ?
                )
                SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json
                  FROM snapshot_events
                 WHERE occurred_at > ? OR (occurred_at = ? AND row_sort_id > ?)
                 ORDER BY occurred_at ASC, row_sort_id ASC
                 LIMIT ?"#,
        )
        .bind(snapshot.projection_revision)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(state.build_cursor.0)
        .bind(state.build_cursor.0)
        .bind(&state.build_cursor.1)
        .bind(ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        let rows = rows?;
        let next_cursor = rows
            .last()
            .map(|row| {
                Ok::<_, sqlx::Error>((row.try_get("occurred_at")?, row.try_get("row_sort_id")?))
            })
            .transpose()?
            .unwrap_or(state.build_cursor);
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
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_cursor_occurred_at = ?, build_cursor_row_sort_id = ?,
                                  build_phase = CASE WHEN ? THEN 'aggregating' ELSE 'copying' END
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ?"#,
                    )
                    .bind(next_cursor.0)
                    .bind(next_cursor.1)
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
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let partition_result = sqlx::query_scalar::<_, String>(
            "SELECT partition_key FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = ? AND partition_key > '' AND partition_key > ? \
             ORDER BY partition_key ASC LIMIT 1",
        )
        .bind(snapshot.build_generation)
        .bind(&state.build_partition_key)
        .fetch_optional(&mut *session)
        .await;
        let partition = session.query(partition_result).await?;
        let Some(partition) = partition else {
            let finish = session.finish().await;
            finish?;
            return self.publish_admin_alert_canonical_groups_snapshot(snapshot, state.build_next_position - 1).await;
        };
        let rows_result = sqlx::query(
            "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
             FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = ? AND partition_key = ? \
             ORDER BY occurred_at DESC, row_sort_id DESC",
        )
        .bind(snapshot.build_generation)
        .bind(&partition)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await;
        let finish = session.finish().await;
        finish?;
        let events = rows?
            .into_iter()
            .map(Self::decode_default_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect();
        let groups = build_group_records_from_events(events).top_level_items;
        let staged = groups
            .into_iter()
            .enumerate()
            .map(|(index, group)| {
                let payload_json = serde_json::to_string(&group).map_err(|error| {
                    ProxyError::Other(format!("serialize canonical alert group: {error}"))
                })?;
                Ok::<_, ProxyError>((
                    state.build_next_position + index as i64,
                    group.last_seen,
                    group.count,
                    group.alert_type,
                    group.id,
                    payload_json,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for chunk in staged.chunks(ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS) {
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
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET build_partition_key = ?, build_next_position = ?
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
        Ok(())
    }

    async fn publish_admin_alert_canonical_groups_snapshot(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        row_count: i64,
    ) -> Result<(), ProxyError> {
        let published = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET active_generation = ?, active_row_count = ?,
                                  active_projection_revision = ?, source_recent_generation = ?,
                                  source_history_generation = ?, build_generation = 0,
                                  build_projection_revision = -1, build_phase = 'idle',
                                  build_partition_key = '', build_next_position = 1
                            WHERE singleton = 1 AND build_generation = ?
                              AND build_projection_revision = ?"#,
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
                    let cleanup_remaining = cleanup_events > 0
                        || cleanup_overrides > 0
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
                        .await?;
                    let reusable_slot_active = matches!(active_generation, 1 | 2);
                    if reusable_slot_active {
                        let legacy_deleted = sqlx::query(
                            r#"DELETE FROM observability.admin_alert_canonical_groups
                                 WHERE rowid IN (
                                     SELECT rowid
                                       FROM observability.admin_alert_canonical_groups
                                      WHERE build_generation > 2
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
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
                                          WHERE build_generation = ?
                                          ORDER BY position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(inactive_generation)
                            .execute(&mut **tx)
                            .await?
                            .rows_affected();
                            if inactive_deleted == 0 {
                                sqlx::query(
                                    r#"DELETE FROM observability.admin_alert_canonical_groups
                                         WHERE rowid IN (
                                             SELECT rowid
                                               FROM observability.admin_alert_canonical_groups
                                              WHERE build_generation = ? AND position > ?
                                              ORDER BY position ASC
                                              LIMIT 25
                                         )"#,
                                )
                                .bind(active_generation)
                                .bind(active_row_count)
                                .execute(&mut **tx)
                                .await?;
                            }
                        }
                        let inactive_generation = if active_generation == 1 { 2 } else { 1 };
                        let legacy_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > 2)",
                        )
                        .fetch_one(&mut **tx)
                        .await?;
                        let inactive_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = ?)",
                        )
                        .bind(inactive_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        let active_tail_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = ? AND position > ?)",
                        )
                        .bind(active_generation)
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
                                      WHERE build_generation < ?
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
                        .bind(active_generation)
                        .execute(&mut **tx)
                        .await?
                        .rows_affected();
                        if retired_before_active == 0 {
                            sqlx::query(
                                r#"DELETE FROM observability.admin_alert_canonical_groups
                                     WHERE rowid IN (
                                         SELECT rowid
                                           FROM observability.admin_alert_canonical_groups
                                          WHERE build_generation > ?
                                          ORDER BY build_generation ASC, position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(active_generation)
                            .execute(&mut **tx)
                            .await?;
                        }
                        let retired_before_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation < ?)",
                        )
                        .bind(active_generation)
                        .fetch_one(&mut **tx)
                        .await?;
                        let retired_after_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > ?)",
                        )
                        .bind(active_generation)
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
