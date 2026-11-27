/// PostgreSQL branches for the canonical semantic reduction readers.
///
/// The SQLite implementation in `key_store_alert_canonical_semantic_reduction.rs` owns the
/// classification and output state machines; these twins change only how the reduction rows are
/// read, because every other step is dialect independent.
impl KeyStore {
    async fn admin_alert_canonical_semantic_child_event_bytes_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        child_ordinal: i64,
    ) -> Result<i64, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let value = sqlx::query_scalar::<_, i64>(
            r#"SELECT CAST(COALESCE(SUM(length(event_json)), 0) AS BIGINT)
                 FROM observability.admin_alert_canonical_group_reduction_events
                WHERE build_generation = $1 AND partition_key = $2 AND child_ordinal = $3"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(child_ordinal)
        .fetch_one(&mut *session.connection())
        .await?;
        session.finish().await?;
        Ok(value)
    }

    async fn read_admin_alert_canonical_semantic_child_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        ordinal: i64,
    ) -> Result<Option<SemanticChildReduction>, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let row = sqlx::query(
            r#"SELECT child_ordinal, mother_ordinal, first_event_position, last_event_position,
                      first_seen, last_seen, event_count, latest_event_json, semantic_kind,
                      semantic_window_minutes, semantic_window_start, semantic_window_end,
                      semantic_window_key
                 FROM observability.admin_alert_canonical_group_reduction_children
                WHERE build_generation = $1 AND partition_key = $2 AND child_ordinal = $3"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(ordinal)
        .fetch_optional(&mut *session.connection())
        .await?;
        session.finish().await?;
        row.map(|row| Self::decode_semantic_child_row_pg(&row)).transpose()
    }

    async fn read_admin_alert_canonical_semantic_mother_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        ordinal: i64,
    ) -> Result<Option<SemanticMotherReduction>, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let row = sqlx::query(
            r#"SELECT mother_ordinal, first_child_ordinal, last_child_ordinal,
                      first_seen, last_seen, event_count, child_count, latest_event_json,
                      semantic_kind, semantic_window_minutes, semantic_window_start,
                      semantic_window_end, semantic_window_key
                 FROM observability.admin_alert_canonical_group_reduction_mothers
                WHERE build_generation = $1 AND partition_key = $2 AND mother_ordinal = $3"#,
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(ordinal)
        .fetch_optional(&mut *session.connection())
        .await?;
        session.finish().await?;
        row.map(|row| Self::decode_semantic_mother_row_pg(&row)).transpose()
    }

    fn decode_semantic_child_row_pg(
        row: &sqlx::postgres::PgRow,
    ) -> Result<SemanticChildReduction, ProxyError> {
        let latest_event_json: String = row.try_get("latest_event_json")?;
        let latest_event = serde_json::from_str(&latest_event_json)
            .map_err(|_| ProxyError::Other("invalid semantic child latest event".to_string()))?;
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
    }

    fn decode_semantic_mother_row_pg(
        row: &sqlx::postgres::PgRow,
    ) -> Result<SemanticMotherReduction, ProxyError> {
        let latest_event_json: String = row.try_get("latest_event_json")?;
        let latest_event = serde_json::from_str(&latest_event_json)
            .map_err(|_| ProxyError::Other("invalid semantic mother latest event".to_string()))?;
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
    }
}

impl KeyStore {
    /// PostgreSQL twin of `commit_admin_alert_canonical_semantic_classification`: one transaction
    /// covering the staged events, the reduced children and mothers, the in-flight group rows, and
    /// the optimistic state advance. `INSERT OR REPLACE` becomes an `ON CONFLICT DO UPDATE` that
    /// rewrites every mutable column.
    #[allow(clippy::too_many_arguments)]
    async fn commit_admin_alert_canonical_semantic_classification_pg(
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
        let mut tx = self.begin_pg_write().await?;

        for (event_position, child_ordinal, event_json) in staged_events {
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_group_reduction_events
                       (build_generation, partition_key, event_position, child_ordinal, event_json)
                   VALUES ($1, $2, $3, $4, $5)
                   ON CONFLICT (build_generation, partition_key, event_position)
                   DO UPDATE SET child_ordinal = excluded.child_ordinal,
                                 event_json = excluded.event_json"#,
            )
            .bind(snapshot.build_generation)
            .bind(&partition)
            .bind(event_position)
            .bind(child_ordinal)
            .bind(event_json)
            .execute(&mut *tx)
            .await?;
        }

        for child in children {
            let latest_event_json =
                serialize_alert_event_record_for_projection(child.latest_event.clone())?.1;
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_group_reduction_children
                       (build_generation, partition_key, child_ordinal, mother_ordinal,
                        first_event_position, last_event_position, first_seen, last_seen,
                        event_count, latest_event_json, semantic_kind, semantic_window_minutes,
                        semantic_window_start, semantic_window_end, semantic_window_key)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                   ON CONFLICT (build_generation, partition_key, child_ordinal)
                   DO UPDATE SET mother_ordinal = excluded.mother_ordinal,
                                 first_event_position = excluded.first_event_position,
                                 last_event_position = excluded.last_event_position,
                                 first_seen = excluded.first_seen,
                                 last_seen = excluded.last_seen,
                                 event_count = excluded.event_count,
                                 latest_event_json = excluded.latest_event_json,
                                 semantic_kind = excluded.semantic_kind,
                                 semantic_window_minutes = excluded.semantic_window_minutes,
                                 semantic_window_start = excluded.semantic_window_start,
                                 semantic_window_end = excluded.semantic_window_end,
                                 semantic_window_key = excluded.semantic_window_key"#,
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
            .bind(latest_event_json)
            .bind(child.semantic_kind)
            .bind(child.semantic_window_minutes)
            .bind(child.semantic_window_start)
            .bind(child.semantic_window_end)
            .bind(child.semantic_window_key)
            .execute(&mut *tx)
            .await?;
        }

        for mother in mothers {
            let latest_event_json =
                serialize_alert_event_record_for_projection(mother.latest_event.clone())?.1;
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_group_reduction_mothers
                       (build_generation, partition_key, mother_ordinal, first_child_ordinal,
                        last_child_ordinal, first_seen, last_seen, event_count, child_count,
                        latest_event_json, semantic_kind, semantic_window_minutes,
                        semantic_window_start, semantic_window_end, semantic_window_key)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                   ON CONFLICT (build_generation, partition_key, mother_ordinal)
                   DO UPDATE SET first_child_ordinal = excluded.first_child_ordinal,
                                 last_child_ordinal = excluded.last_child_ordinal,
                                 first_seen = excluded.first_seen,
                                 last_seen = excluded.last_seen,
                                 event_count = excluded.event_count,
                                 child_count = excluded.child_count,
                                 latest_event_json = excluded.latest_event_json,
                                 semantic_kind = excluded.semantic_kind,
                                 semantic_window_minutes = excluded.semantic_window_minutes,
                                 semantic_window_start = excluded.semantic_window_start,
                                 semantic_window_end = excluded.semantic_window_end,
                                 semantic_window_key = excluded.semantic_window_key"#,
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
            .bind(latest_event_json)
            .bind(mother.semantic_kind.clone())
            .bind(mother.semantic_window_minutes)
            .bind(mother.semantic_window_start)
            .bind(mother.semantic_window_end)
            .bind(mother.semantic_window_key.clone())
            .execute(&mut *tx)
            .await?;

            let group = Self::semantic_mother_group_record(&mother);
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_groups
                       (build_generation, position, last_seen, total_count,
                        alert_type, group_id, payload_json)
                   VALUES ($1, $2, $3, $4, $5, $6, '')
                   ON CONFLICT (build_generation, position)
                   DO UPDATE SET last_seen = excluded.last_seen,
                                 total_count = excluded.total_count,
                                 alert_type = excluded.alert_type,
                                 group_id = excluded.group_id"#,
            )
            .bind(snapshot.build_generation)
            .bind(state.build_next_position + mother.ordinal - 1)
            .bind(group.last_seen)
            .bind(group.count)
            .bind(group.alert_type)
            .bind(group.id)
            .execute(&mut *tx)
            .await?;
        }

        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_partition_events_json = $1,
                      build_partition_finalize_fragment_position = $2
                WHERE singleton = 1 AND build_generation = $3
                  AND build_projection_revision = $4 AND build_phase = 'aggregating'
                  AND build_partition_key = $5
                  AND build_partition_finalize_fragment_position = $6
                  AND build_partition_events_json = $7
                  AND build_source_recent_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_state
                  )
                  AND build_source_history_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
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
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }
}
