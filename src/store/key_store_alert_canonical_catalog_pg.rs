/// PostgreSQL branch of the canonical alert catalog builder.
///
/// The SQLite implementation in `key_store_alert_canonical_catalog.rs` owns the facet aggregation
/// and the response shape; this twin keeps that logic and changes only the three storage
/// interactions: the state read, the bounded page read, and the transactional advance.
///
/// Catalog facet identity and its accumulated item count.
type CatalogFacets = HashMap<(String, String, String, String), i64>;

impl KeyStore {
    async fn advance_admin_alert_canonical_catalog_snapshot_pg(
        &self,
        build_generation: i64,
    ) -> Result<(), ProxyError> {
        const CATALOG_BUILD_SLICE_ROWS: i64 = 50;
        let mut session = self.begin_pg_read_session().await?;
        let state = sqlx::query_as::<_, (i64, i64, String, bool)>(
            "SELECT build_generation, cursor_occurred_at, cursor_row_sort_id, source_complete \
             FROM observability.admin_alert_canonical_catalog_state WHERE singleton = 1",
        )
        .fetch_optional(&mut *session.connection())
        .await?;
        let rows = if state.is_some() {
            let (_, cursor_occurred_at, cursor_row_sort_id, _) =
                state.clone().expect("state is present");
            Some(
                sqlx::query(
                    "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
                     FROM observability.admin_alert_canonical_group_events \
                     WHERE build_generation = $1 AND occurred_at >= $2 \
                       AND (occurred_at > $3 OR (occurred_at = $3 AND row_sort_id > $4)) \
                     ORDER BY occurred_at ASC, row_sort_id ASC LIMIT $5",
                )
                .bind(build_generation)
                .bind(self.alert_projection_retention_since())
                .bind(cursor_occurred_at)
                .bind(&cursor_row_sort_id)
                .bind(CATALOG_BUILD_SLICE_ROWS)
                .fetch_all(&mut *session.connection())
                .await?,
            )
        } else {
            None
        };
        session.finish().await?;

        let Some((state_generation, cursor_occurred_at, cursor_row_sort_id, source_complete)) =
            state
        else {
            return Err(ProxyError::Other(
                "canonical alert catalog state is unavailable".to_string(),
            ));
        };
        if state_generation != build_generation {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        if source_complete {
            return Ok(());
        }
        let rows = rows.unwrap_or_default();
        let complete = rows.len() < CATALOG_BUILD_SLICE_ROWS as usize;
        let mut next_cursor = (cursor_occurred_at, cursor_row_sort_id.clone());
        for row in &rows {
            next_cursor = (
                sqlx::Row::try_get::<i64, _>(row, "occurred_at")?,
                sqlx::Row::try_get::<String, _>(row, "row_sort_id")?,
            );
        }
        let facets = Self::admin_alert_catalog_facets_pg(rows)?;
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_catalog_state
                  SET cursor_occurred_at = $1, cursor_row_sort_id = $2, source_complete = $3
                WHERE singleton = 1 AND build_generation = $4
                  AND cursor_occurred_at = $5 AND cursor_row_sort_id = $6
                  AND $4 = (SELECT active_generation
                              FROM observability.admin_alert_canonical_groups_state
                             WHERE singleton = 1)"#,
        )
        .bind(next_cursor.0)
        .bind(&next_cursor.1)
        .bind(complete)
        .bind(build_generation)
        .bind(cursor_occurred_at)
        .bind(&cursor_row_sort_id)
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        for ((facet_kind, facet_identity, facet_value, facet_label), item_count) in facets {
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_catalog_facets
                       (build_generation, facet_kind, facet_identity, facet_value, facet_label, item_count)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (build_generation, facet_kind, facet_identity)
                   DO UPDATE SET item_count = admin_alert_canonical_catalog_facets.item_count
                                   + excluded.item_count,
                                 facet_label = LEAST(admin_alert_canonical_catalog_facets.facet_label,
                                                     excluded.facet_label)"#,
            )
            .bind(build_generation)
            .bind(facet_kind)
            .bind(facet_identity)
            .bind(facet_value)
            .bind(facet_label)
            .bind(item_count)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        if complete {
            Ok(())
        } else {
            Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_build_in_progress".to_string(),
            })
        }
    }

    /// Facet aggregation shared by both dialects: decode each projected row and accumulate the
    /// distinct facet values with their item counts.
    fn admin_alert_catalog_facets_pg(
        rows: Vec<sqlx::postgres::PgRow>,
    ) -> Result<CatalogFacets, ProxyError> {
        let mut facets = CatalogFacets::new();
        for row in rows {
            let projection = Self::decode_alert_event_projection_row_pg(row)?;
            Self::accumulate_admin_alert_catalog_facets(projection, &mut facets);
        }
        Ok(facets)
    }
}
