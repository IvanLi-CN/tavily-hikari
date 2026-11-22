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
              COALESCE(SUM(CASE WHEN result_status = $1 AND created_at >= $2 THEN 1 ELSE 0 END), 0)::bigint AS monthly_success,
              COALESCE(SUM(CASE WHEN result_status = $3 AND created_at >= $4 AND created_at < $5 THEN 1 ELSE 0 END), 0)::bigint AS daily_success,
              COALESCE(SUM(CASE WHEN result_status = $6 AND created_at >= $7 AND created_at < $8 THEN 1 ELSE 0 END), 0)::bigint AS daily_failure
            FROM core.auth_token_logs
            WHERE token_id = $9
              AND created_at >= $10
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
        .fetch_one(self.pg()?)
        .await?;

        Ok((
            row.try_get("monthly_success")?,
            row.try_get("daily_success")?,
            row.try_get("daily_failure")?,
        ))
    }
}
