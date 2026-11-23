/// PostgreSQL twins of the dashboard read-model helpers.
///
/// The SQLite implementations stay in `key_store_dashboard_and_job_read_models.rs`; these
/// variants are reachable only through the attached PostgreSQL pool.
impl KeyStore {
    pub(crate) async fn fetch_summary_without_flush_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(total_requests), 0)::bigint AS total_requests,
                COALESCE(SUM(success_count), 0)::bigint AS success_count,
                COALESCE(SUM(error_count), 0)::bigint AS error_count,
                COALESCE(SUM(quota_exhausted_count), 0)::bigint AS quota_exhausted_count
            FROM observability.api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .fetch_one(&mut **tx)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN ak.status = $1 AND aq.key_id IS NULL AND tb.key_id IS NULL THEN 1 ELSE 0 END), 0)::bigint AS active_keys,
                COALESCE(SUM(CASE WHEN ak.status = $2 AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0)::bigint AS exhausted_keys,
                COALESCE(SUM(CASE WHEN aq.key_id IS NOT NULL THEN 1 ELSE 0 END), 0)::bigint AS quarantined_keys,
                COALESCE(SUM(CASE WHEN ak.status = $3 AND aq.key_id IS NULL AND tb.key_id IS NOT NULL THEN 1 ELSE 0 END), 0)::bigint AS temporary_isolated_keys
            FROM core.api_keys ak
            LEFT JOIN core.api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            LEFT JOIN (
                SELECT key_id, MAX(cooldown_until) AS cooldown_until
                FROM core.api_key_transient_backoffs
                WHERE cooldown_until > EXTRACT(EPOCH FROM now())::bigint
                  AND reason_code = 'upstream_unknown_403'
                GROUP BY key_id
            ) AS tb
              ON tb.key_id = ak.id
            WHERE ak.deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .bind(STATUS_ACTIVE)
        .fetch_one(&mut **tx)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(last_used_at) FROM core.api_keys WHERE deleted_at IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?
        .and_then(normalize_timestamp);

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0)::bigint AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0)::bigint AS total_quota_remaining
            FROM core.api_keys ak
            LEFT JOIN core.api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            quarantined_keys: key_counts_row.try_get("quarantined_keys")?,
            temporary_isolated_keys: key_counts_row.try_get("temporary_isolated_keys")?,
            last_activity,
            total_quota_limit: quotas_row.try_get("total_quota_limit")?,
            total_quota_remaining: quotas_row.try_get("total_quota_remaining")?,
        })
    }

}
