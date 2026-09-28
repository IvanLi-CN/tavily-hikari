//! Plan todo 20 (M18): additive provider-dimension READ surface for the store.
//!
//! The catalog/dashboard rollup tables intentionally stay provider-agnostic,
//! so every provider-aware read here is a bounded direct scan:
//! `observability.request_logs` scoped by the retention window (logs page
//! count, provider facet list) or by fixed summary windows (window provider
//! counts), and the pre-aggregated `api_key_usage_buckets` joined to
//! `api_keys.provider` (summary breakdown — the same source the legacy totals
//! already use, segmented by key-join per M18). Legacy rows with a NULL
//! provider are coalesced to `tavily` in every query. This module also keeps
//! the byte-stable `fetch_request_logs_page` wrapper so the pre-existing
//! Tavily call sites never observe the new provider parameter.

use super::{KeyStore, ProxyError, RequestLogFilterParams};
use crate::analysis::{
    legacy_request_kind_stored_predicate_sql, request_log_counts_business_quota_sql,
    request_log_operational_class_case_sql, request_log_request_kind_key_sql,
    result_bucket_case_sql,
};
use crate::models::{LogFacetOption, RequestLogsPage};
use crate::{OUTCOME_ERROR, OUTCOME_QUOTA_EXHAUSTED, OUTCOME_SUCCESS};
use sqlx::sqlite::Sqlite;
use sqlx::{QueryBuilder, Row};

/// Plan todo 20: per-provider counter tuple crossing the bin boundary in
/// declaration order: provider, total_requests, success_count, error_count,
/// quota_exhausted_count.
pub(crate) type ProviderCounterRow = (String, i64, i64, i64, i64);

impl KeyStore {
    /// Byte-stable legacy entry point: identical to the pre-todo-20
    /// `fetch_request_logs_page` (provider filter absent).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_request_logs_page(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        request_user_id: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
        include_token_facets: bool,
        include_key_facets: bool,
        include_bodies: bool,
    ) -> Result<RequestLogsPage, ProxyError> {
        self.fetch_request_logs_page_core(
            scoped_key_id,
            since,
            request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            request_user_id,
            auth_token_id,
            key_id,
            operational_class,
            None,
            page,
            per_page,
            include_token_facets,
            include_key_facets,
            include_bodies,
        )
        .await
    }

    /// Plan todo 20 (M18): row total for a provider-filtered logs page. The
    /// catalog rollups carry no provider dimension, so this COUNT mirrors the
    /// bounded items predicate of `fetch_request_logs_page_core` exactly.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn count_request_logs_page_matching_provider(
        &self,
        scoped_key_id: Option<&str>,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        request_user_id: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        provider: &str,
    ) -> Result<i64, ProxyError> {
        let normalized_request_kinds = Self::normalize_request_kind_filters(request_kinds);
        let stored_request_kind_sql = "request_kind_key";
        let legacy_request_kind_predicate_sql =
            legacy_request_kind_stored_predicate_sql(stored_request_kind_sql);
        let legacy_request_kind_sql =
            request_log_request_kind_key_sql("path", "request_body", "request_kind_key");
        let effective_request_kind_sql = format!(
            "CASE WHEN {legacy_request_kind_predicate_sql} THEN {legacy_request_kind_sql} ELSE {stored_request_kind_sql} END"
        );
        let stored_counts_business_quota_sql = format!(
            "COALESCE(counts_business_quota, {})",
            request_log_counts_business_quota_sql(stored_request_kind_sql, "request_body")
        );
        let stored_operational_class_case_sql = request_log_operational_class_case_sql(
            stored_request_kind_sql,
            &stored_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let legacy_counts_business_quota_sql =
            request_log_counts_business_quota_sql(&legacy_request_kind_sql, "request_body");
        let legacy_operational_class_case_sql = request_log_operational_class_case_sql(
            &legacy_request_kind_sql,
            &legacy_counts_business_quota_sql,
            "result_status",
            "COALESCE(failure_kind, '')",
        );
        let stored_result_bucket_sql =
            result_bucket_case_sql(&stored_operational_class_case_sql, "result_status");
        let legacy_result_bucket_sql =
            result_bucket_case_sql(&legacy_operational_class_case_sql, "result_status");

        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM observability.request_logs");
        let has_where = Self::push_request_logs_scope(&mut query, scoped_key_id, since, None);
        Self::push_request_logs_filters(
            &mut query,
            RequestLogFilterParams {
                request_kinds: &[],
                result_status: None,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                request_user_id,
                auth_token_id,
                key_id,
                stored_request_kind_sql,
                legacy_request_kind_predicate_sql: &legacy_request_kind_predicate_sql,
                legacy_request_kind_sql: &legacy_request_kind_sql,
                has_where,
            },
        );
        if !normalized_request_kinds.is_empty() {
            query.push(" AND ");
            Self::push_effective_request_kind_filter_clause(
                &mut query,
                &effective_request_kind_sql,
                &normalized_request_kinds,
            );
        }
        if let Some(result_status) = result_status {
            query.push(" AND ");
            Self::push_result_bucket_filter_clause(
                &mut query,
                result_status,
                &legacy_request_kind_predicate_sql,
                &stored_result_bucket_sql,
                &legacy_result_bucket_sql,
            );
        }
        if let Some(operational_class) = operational_class {
            query.push(" AND ");
            Self::push_operational_class_filter_clause(
                &mut query,
                operational_class,
                &legacy_request_kind_predicate_sql,
                &stored_operational_class_case_sql,
                &legacy_operational_class_case_sql,
            );
        }
        query.push(" AND COALESCE(provider, 'tavily') = ");
        query.push_bind(provider.to_string());
        query
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::from)
    }

    /// Plan todo 20: provider facet list for the /api/logs surface, bounded
    /// by the caller-supplied retention floor. Ordered like the existing
    /// rollup facets (count DESC, value ASC).
    pub(crate) async fn fetch_request_log_provider_facet_options(
        &self,
        since: Option<i64>,
    ) -> Result<Vec<LogFacetOption>, ProxyError> {
        type ProviderFacetRow = (String, i64);
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT COALESCE(provider, 'tavily') AS value, COUNT(*) AS count \
             FROM observability.request_logs WHERE visibility = ",
        );
        query.push_bind(crate::REQUEST_LOG_VISIBILITY_VISIBLE);
        if let Some(since) = since {
            query.push(" AND created_at >= ");
            query.push_bind(since);
        }
        query.push(" GROUP BY 1 ORDER BY count DESC, value ASC");
        let rows = query
            .build_query_as::<ProviderFacetRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(ProxyError::from)?;
        Ok(rows
            .into_iter()
            .map(|(value, count)| LogFacetOption { value, count })
            .collect())
    }

    /// Plan todo 20 (M18): per-provider totals for GET /api/summary.
    /// Segments the SAME pre-aggregated source the legacy totals use
    /// (86400s `api_key_usage_buckets`) by key-join to `api_keys.provider`;
    /// no new rollup dimension is introduced.
    pub(crate) async fn fetch_summary_provider_breakdown(
        &self,
    ) -> Result<Vec<ProviderCounterRow>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT
                COALESCE(ak.provider, 'tavily') AS provider,
                COALESCE(SUM(b.total_requests), 0) AS total_requests,
                COALESCE(SUM(b.success_count), 0) AS success_count,
                COALESCE(SUM(b.error_count), 0) AS error_count,
                COALESCE(SUM(b.quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets b
            LEFT JOIN api_keys ak ON ak.id = b.api_key_id
            WHERE b.bucket_secs = 86400
            GROUP BY 1
            ORDER BY 1 ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)?;
        rows.iter()
            .map(Self::map_provider_counter_row)
            .collect::<Result<Vec<_>, _>>()
    }

    /// Plan todo 20 (M18): per-provider counts for the /api/summary/windows
    /// buckets. The dashboard rollups have no key/provider dimension, so each
    /// window is ONE bounded scan over `observability.request_logs`
    /// (`windows` = [today, yesterday, month], each (start, end-exclusive)).
    pub(crate) async fn fetch_summary_window_provider_counts(
        &self,
        windows: [(i64, i64); 3],
    ) -> Result<[Vec<ProviderCounterRow>; 3], ProxyError> {
        let [today, yesterday, month] = windows;
        Ok([
            self.fetch_window_provider_counts(today).await?,
            self.fetch_window_provider_counts(yesterday).await?,
            self.fetch_window_provider_counts(month).await?,
        ])
    }

    async fn fetch_window_provider_counts(
        &self,
        (window_start, window_end): (i64, i64),
    ) -> Result<Vec<ProviderCounterRow>, ProxyError> {
        if window_end <= window_start {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"
            SELECT
                COALESCE(provider, 'tavily') AS provider,
                COUNT(*) AS total_requests,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS success_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS error_count,
                COALESCE(SUM(CASE WHEN result_status = ? THEN 1 ELSE 0 END), 0) AS quota_exhausted_count
            FROM observability.request_logs
            WHERE visibility = ?
              AND created_at >= ? AND created_at < ?
            GROUP BY 1
            ORDER BY 1 ASC
            "#,
        )
        .bind(OUTCOME_SUCCESS)
        .bind(OUTCOME_ERROR)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(crate::REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(window_start)
        .bind(window_end)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)?;
        rows.iter()
            .map(Self::map_provider_counter_row)
            .collect::<Result<Vec<_>, _>>()
    }

    fn map_provider_counter_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<ProviderCounterRow, ProxyError> {
        Ok((
            row.try_get("provider")?,
            row.try_get("total_requests")?,
            row.try_get("success_count")?,
            row.try_get("error_count")?,
            row.try_get("quota_exhausted_count")?,
        ))
    }
}
