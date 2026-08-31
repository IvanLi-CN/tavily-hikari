impl KeyStore {
    fn decode_alert_group_projection_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<AlertGroupProjectionRow, sqlx::Error> {
        Ok(AlertGroupProjectionRow {
            grouping_kind: row.try_get("grouping_kind")?,
            row_sort_id: row.try_get("row_sort_id")?,
            alert_type: row.try_get("alert_type")?,
            subject_kind: row.try_get("subject_kind")?,
            subject_id: row.try_get("subject_id")?,
            count: row.try_get("total_count")?,
            first_seen: row.try_get("first_seen")?,
            last_seen: row.try_get("last_seen")?,
            semantic_window_kind: row.try_get("semantic_window_kind")?,
            semantic_window_minutes: row.try_get("semantic_window_minutes")?,
            semantic_window_start: row.try_get("semantic_window_start")?,
            semantic_window_end: row.try_get("semantic_window_end")?,
            child_count: row.try_get("child_count")?,
        })
    }

    async fn fetch_group_latest_events(
        &self,
        filters: AlertEventFilters<'_>,
        groups: &[AlertGroupProjectionRow],
        source: AlertReadSource,
    ) -> Result<HashMap<String, AlertEventRecord>, ProxyError> {
        self.fetch_group_latest_events_for_operation(
            filters,
            groups,
            source,
            SqliteOperation::AdminAlertsRead,
        )
        .await
    }

    async fn fetch_group_latest_events_for_operation(
        &self,
        filters: AlertEventFilters<'_>,
        groups: &[AlertGroupProjectionRow],
        source: AlertReadSource,
        operation: SqliteOperation,
    ) -> Result<HashMap<String, AlertEventRecord>, ProxyError> {
        if groups.is_empty() {
            return Ok(HashMap::new());
        }

        let mut query = QueryBuilder::new("");
        Self::push_alert_events_for_source_cte(&mut query, filters, source);
        query.push(" SELECT * FROM alerts WHERE row_sort_id IN (");
        {
            let mut separated = query.separated(", ");
            for group in groups {
                separated.push_bind(group.row_sort_id.as_str());
            }
        }
        query.push(")");

        let rows = self
            .fetch_alert_query_rows_for_operation(query, source, operation)
            .await?;
        let mut events_by_row_sort_id = HashMap::new();
        for row in rows {
            let decoded = Self::decode_alert_event_projection_row(row)?;
            let row_sort_id = decoded.row_sort_id.clone();
            if let Some(event) = Self::build_alert_event_from_projection(decoded) {
                events_by_row_sort_id.insert(row_sort_id, event);
            }
        }
        Ok(events_by_row_sort_id)
    }

    async fn fetch_projected_group_latest_events_in_admin_session(
        &self,
        filters: AlertEventFilters<'_>,
        groups: &[AlertGroupProjectionRow],
        session: &mut AdminAlertsReadSession,
    ) -> Result<HashMap<String, AlertEventRecord>, ProxyError> {
        if groups.is_empty() {
            return Ok(HashMap::new());
        }
        let mut query = QueryBuilder::new("");
        Self::push_projected_alert_events_cte(&mut query, filters);
        query.push(" SELECT * FROM alerts WHERE row_sort_id IN (");
        {
            let mut separated = query.separated(", ");
            for group in groups {
                separated.push_bind(group.row_sort_id.as_str());
            }
        }
        query.push(")");
        let result = query.build().fetch_all(&mut **session).await;
        let rows = session.query(result).await?;
        let mut events_by_row_sort_id = HashMap::new();
        for row in rows {
            let decoded = Self::decode_alert_event_projection_row(row)?;
            let row_sort_id = decoded.row_sort_id.clone();
            if let Some(event) = Self::build_alert_event_from_projection(decoded) {
                events_by_row_sort_id.insert(row_sort_id, event);
            }
        }
        Ok(events_by_row_sort_id)
    }

    fn build_alert_group_records(
        groups: Vec<AlertGroupProjectionRow>,
        latest_events_by_row_sort_id: HashMap<String, AlertEventRecord>,
    ) -> Vec<AlertGroupRecord> {
        groups
            .into_iter()
            .filter_map(|group| {
                let latest_event = latest_events_by_row_sort_id.get(&group.row_sort_id)?.clone();
                let semantic_window_kind = latest_event
                    .semantic_window
                    .as_ref()
                    .map(|value| value.kind.as_str().to_string())
                    .or_else(|| group.semantic_window_kind.clone());
                let semantic_window_minutes = latest_event
                    .semantic_window
                    .as_ref()
                    .and_then(|value| value.window_minutes)
                    .or(group.semantic_window_minutes);
                Some(AlertGroupRecord {
                    id: if group.grouping_kind == "mother" {
                        semantic_mother_id_from_child(&latest_event, 0)
                    } else {
                        alert_group_id(&latest_event)
                    },
                    alert_type: group.alert_type,
                    subject_kind: group.subject_kind,
                    subject_id: group.subject_id,
                    subject_label: latest_event.subject_label.clone(),
                    user: latest_event.user.clone(),
                    token: latest_event.token.clone(),
                    key: latest_event.key.clone(),
                    job: latest_event.job.clone(),
                    request_kind: (group.grouping_kind == "compat")
                        .then(|| latest_event.request_kind.clone())
                        .flatten(),
                    count: group.count,
                    first_seen: group.first_seen,
                    last_seen: group.last_seen,
                    latest_event,
                    grouping_kind: group.grouping_kind,
                    semantic_window_kind,
                    semantic_window_minutes,
                    semantic_window_start: group.semantic_window_start,
                    semantic_window_end: group.semantic_window_end,
                    semantic_window_key: None,
                    child_count: group.child_count,
                    event_count: group.count,
                    children: Vec::new(),
                    child_events: Vec::new(),
                })
            })
            .collect()
    }
}
