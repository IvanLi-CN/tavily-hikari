const DASHBOARD_QUOTA_SOURCE_PAGE_SIZE: i64 = 32;
const DASHBOARD_QUOTA_SOURCE_MAX_PAGES: usize = 4;

impl KeyStore {
    pub(crate) async fn fetch_dashboard_quota_sample_watermark(
        &self,
        today_end: i64,
    ) -> Result<DashboardQuotaSampleWatermark, ProxyError> {
        let source_id = self.fetch_dashboard_quota_source_id(today_end).await?;
        let source_captured_at = self
            .fetch_dashboard_quota_source_captured_at(today_end)
            .await?;
        Ok(DashboardQuotaSampleWatermark {
            source_id,
            source_captured_at,
            // Retain the existing private token shape without an unbounded
            // count probe. This is an append-only source revision, not a row count.
            source_count: source_id,
        })
    }

    fn dashboard_quota_read_budget_deferred(&self) -> ProxyError {
        self.sqlite_runtime.record_deferred(
            SqliteOperation::AdminAlertsCacheWarm,
            SqliteAdmissionDeferReason::QueryDeadline,
        );
        ProxyError::Deferred {
            operation: "admin_alerts_read",
            reason: "read_budget".to_string(),
        }
    }

    async fn fetch_dashboard_quota_source_id(&self, today_end: i64) -> Result<i64, ProxyError> {
        let mut before_id = None;
        for _ in 0..DASHBOARD_QUOTA_SOURCE_MAX_PAGES {
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
                .await?;
            let query_result = match before_id {
                Some(before_id) => sqlx::query(
                    r#"
                    SELECT id, captured_at
                    FROM api_key_quota_sync_samples
                    WHERE id > 0 AND id < ?
                    ORDER BY id DESC
                    LIMIT ?
                    "#,
                )
                .bind(before_id)
                .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                .fetch_all(&mut *session)
                .await,
                None => sqlx::query(
                    r#"
                    SELECT id, captured_at
                    FROM api_key_quota_sync_samples
                    WHERE id > 0
                    ORDER BY id DESC
                    LIMIT ?
                    "#,
                )
                .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                .fetch_all(&mut *session)
                .await,
            };
            let rows = session.query(query_result).await;
            let finish = session.finish().await;
            finish?;
            let samples = rows?
                .into_iter()
                .map(|row| Ok::<_, sqlx::Error>((row.try_get("id")?, row.try_get("captured_at")?)))
                .collect::<Result<Vec<(i64, i64)>, _>>()?;

            if let Some((id, _)) = samples
                .iter()
                .copied()
                .find(|(_, captured_at)| *captured_at < today_end)
            {
                return Ok(id);
            }
            let Some((last_id, _)) = samples.last().copied() else {
                return Ok(0);
            };
            if samples.len() < DASHBOARD_QUOTA_SOURCE_PAGE_SIZE as usize {
                return Ok(0);
            }
            before_id = Some(last_id);
        }

        Err(self.dashboard_quota_read_budget_deferred())
    }

    async fn fetch_dashboard_quota_source_captured_at(
        &self,
        today_end: i64,
    ) -> Result<i64, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let query_result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT captured_at
            FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
            WHERE captured_at < ?
            ORDER BY captured_at DESC, key_id ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(today_end)
        .fetch_optional(&mut *session)
        .await;
        let source_captured_at = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        Ok(source_captured_at?.unwrap_or_default())
    }
}
