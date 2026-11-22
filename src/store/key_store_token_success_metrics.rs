impl KeyStore {

    pub(crate) async fn fetch_token_success_failure(
        &self,
        token_id: &str,
        month_since: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let scan_floor = month_since.min(day_start);
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS daily_success,
              COALESCE(SUM(CASE WHEN result_status = ? AND created_at >= ? AND created_at < ? THEN 1 ELSE 0 END), 0) AS daily_failure
            FROM auth_token_logs
            WHERE token_id = ?
              AND created_at >= ?
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(day_end)
        .bind(OUTCOME_ERROR)
        .bind(day_start)
        .bind(day_end)
        .bind(token_id)
        .bind(scan_floor)
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }
}
