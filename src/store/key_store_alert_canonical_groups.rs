const ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS: i64 = 250;
const ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS: usize = 25;

impl KeyStore {
    async fn fetch_admin_alert_canonical_groups_page(
        &self,
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        let source_fence = self.admin_alerts_canonical_warm_projection_fence().await?;
        if self
            .admin_alert_canonical_groups_model_is_current(source_fence)
            .await?
        {
            return self.read_admin_alert_canonical_groups_model(source_fence).await;
        }

        self.build_admin_alert_canonical_groups_model(source_fence)
            .await?;
        self.read_admin_alert_canonical_groups_model(source_fence).await
    }

    async fn admin_alert_canonical_groups_model_is_current(
        &self,
        source_fence: (i64, i64),
    ) -> Result<bool, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT active_generation, source_recent_generation, source_history_generation \
             FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
        )
        .fetch_optional(&mut *session)
        .await;
        let state = session.query(result).await;
        let finish = session.finish().await;
        finish?;
        let state = state?;
        Ok(state.is_some_and(|(active, recent, history)| {
            active > 0 && (recent, history) == source_fence
        }))
    }

    async fn build_admin_alert_canonical_groups_model(
        &self,
        source_fence: (i64, i64),
    ) -> Result<(), ProxyError> {
        let mut cursor = (i64::MIN, String::new());
        let mut events = Vec::new();
        loop {
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let query_result = sqlx::query(
                r#"SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json
                     FROM observability.dashboard_alert_projection_events
                    WHERE occurred_at > ?
                       OR (occurred_at = ? AND row_sort_id > ?)
                    ORDER BY occurred_at ASC, row_sort_id ASC
                    LIMIT ?"#,
            )
            .bind(cursor.0)
            .bind(cursor.0)
            .bind(&cursor.1)
            .bind(ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(query_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = rows?;
            if rows.is_empty() {
                break;
            }
            cursor = rows
                .last()
                .map(|row| {
                    Ok::<_, sqlx::Error>((row.try_get("occurred_at")?, row.try_get("row_sort_id")?))
                })
                .transpose()?
                .expect("nonempty alert projection page has a cursor");
            let projection_rows = rows
                .into_iter()
                .map(Self::decode_default_alert_event_projection_row)
                .collect::<Result<Vec<_>, _>>()?;
            events.extend(
                projection_rows
                    .into_iter()
                    .filter_map(Self::build_alert_event_from_projection),
            );
            self.record_admin_alerts_warm_slice();
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_build_slice();
        }

        if self.admin_alerts_canonical_warm_projection_fence().await? != source_fence {
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_defer();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_source_fence_changed".to_string(),
            });
        }

        let groups = build_group_records_from_events(events).top_level_items;
        let build_generation = self.next_admin_alert_canonical_groups_generation().await?;
        let staged_rows = groups
            .into_iter()
            .enumerate()
            .map(|(position, group)| {
                let payload_json = serde_json::to_string(&group).map_err(|error| {
                    ProxyError::Other(format!("serialize canonical alert group: {error}"))
                })?;
                Ok::<_, ProxyError>(
                    (
                        (position + 1) as i64,
                        group.last_seen,
                        group.count,
                        group.alert_type,
                        group.id,
                        payload_json,
                    ),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        for chunk in staged_rows.chunks(ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS) {
            let rows = chunk.to_vec();
            self.sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        for (position, last_seen, total_count, alert_type, group_id, payload_json) in rows {
                            sqlx::query(
                        r#"INSERT OR REPLACE INTO observability.admin_alert_canonical_groups (
                                       build_generation, position, last_seen, total_count,
                                       alert_type, group_id, payload_json
                                   ) VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                            )
                            .bind(build_generation)
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

        let active_row_count = staged_rows.len() as i64;
        let published = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_groups_state
                              SET active_generation = ?, active_row_count = ?, source_recent_generation = ?,
                                  source_history_generation = ?
                            WHERE singleton = 1
                              AND ? = (SELECT COALESCE(SUM(generation), 0)
                                         FROM observability.dashboard_alert_projection_state)
                              AND ? = (SELECT COALESCE(SUM(generation), 0)
                                         FROM observability.dashboard_alert_projection_history_state)"#,
                    )
                    .bind(build_generation)
                    .bind(active_row_count)
                    .bind(source_fence.0)
                    .bind(source_fence.1)
                    .bind(source_fence.0)
                    .bind(source_fence.1)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !published {
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_defer();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_source_fence_changed".to_string(),
            });
        }
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_publish();
        Ok(())
    }

    async fn next_admin_alert_canonical_groups_generation(&self) -> Result<i64, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let query_result = sqlx::query_scalar::<_, i64>(
            "SELECT CASE active_generation WHEN 1 THEN 2 ELSE 1 END \
             FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
        )
        .fetch_one(&mut *session)
        .await;
        let generation = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        let generation = generation?;
        Ok(generation)
    }

    pub(crate) async fn reclaim_admin_alert_canonical_groups_generations(
        &self,
    ) -> Result<bool, ProxyError> {
        self.sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, |tx| {
                Box::pin(async move {
                    let (active_generation, active_row_count) = sqlx::query_as::<_, (i64, i64)>(
                        "SELECT active_generation, active_row_count \
                         FROM observability.admin_alert_canonical_groups_state \
                         WHERE singleton = 1",
                    )
                    .fetch_optional(&mut **tx)
                    .await?
                    .unwrap_or_default();
                    let active_uses_reusable_slot = if matches!(active_generation, 1 | 2) {
                        1
                    } else {
                        0
                    };
                    sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_groups
                             WHERE rowid IN (
                                 SELECT rowid
                                   FROM observability.admin_alert_canonical_groups
                                  WHERE (? = 1 AND (
                                             (build_generation = ? AND position > ?)
                                             OR build_generation NOT IN (1, 2)
                                         ))
                                     OR (? = 0 AND build_generation <> ?)
                                  ORDER BY CASE
                                               WHEN ? = 1 AND build_generation NOT IN (1, 2) THEN 0
                                               ELSE 1
                                           END,
                                           build_generation ASC,
                                           position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_uses_reusable_slot)
                    .bind(active_generation)
                    .bind(active_row_count)
                    .bind(active_uses_reusable_slot)
                    .bind(active_generation)
                    .bind(active_uses_reusable_slot)
                    .execute(&mut **tx)
                    .await?;
                    let remaining = sqlx::query_scalar::<_, bool>(
                        r#"SELECT EXISTS(
                             SELECT 1
                               FROM observability.admin_alert_canonical_groups
                              WHERE (? = 1 AND (
                                         (build_generation = ? AND position > ?)
                                         OR build_generation NOT IN (1, 2)
                                     ))
                                 OR (? = 0 AND build_generation <> ?)
                         )"#,
                    )
                    .bind(active_uses_reusable_slot)
                    .bind(active_generation)
                    .bind(active_row_count)
                    .bind(active_uses_reusable_slot)
                    .bind(active_generation)
                    .fetch_one(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(remaining)
                })
            })
            .await
    }

    async fn read_admin_alert_canonical_groups_model(
        &self,
        source_fence: (i64, i64),
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = async {
            let state_result = sqlx::query_as::<_, (i64, i64, i64, i64)>(
                "SELECT active_generation, active_row_count, source_recent_generation, \
                        source_history_generation \
                 FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
            )
            .fetch_optional(&mut *session)
            .await;
            let Some((generation, row_count, recent, history)) = session.query(state_result).await? else {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_model_unavailable".to_string(),
                });
            };
            if generation <= 0 || (recent, history) != source_fence {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_source_fence_changed".to_string(),
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
