#[cfg(test)]
impl KeyStore {
    pub(crate) async fn enqueue_request_stats_rollup_for_test(
        &self,
        api_key_id: Option<&str>,
        created_at: i64,
        outcome: &str,
    ) {
        let mut counts = DashboardRequestRollupCounts {
            total_requests: 1,
            api_billable: 1,
            ..DashboardRequestRollupCounts::default()
        };
        match outcome {
            OUTCOME_SUCCESS => {
                counts.success_count = 1;
                counts.valuable_success_count = 1;
            }
            OUTCOME_ERROR => {
                counts.error_count = 1;
                counts.valuable_failure_count = 1;
            }
            OUTCOME_QUOTA_EXHAUSTED => {
                counts.quota_exhausted_count = 1;
                counts.valuable_failure_count = 1;
            }
            _ => {
                counts.unknown_count = 1;
            }
        }
        self.request_stats_pipeline
            .enqueue_request_log_rollups(RequestLogRollupInput {
                api_key_id,
                auth_token_id: "test-auth-token",
                request_user_id: None,
                request_log_id: None,
                created_at,
                dashboard_counts: counts,
                request_log_catalog_key: None,
            })
            .await;
    }

    pub(crate) async fn enqueue_request_stats_rollup_for_user_for_test(
        &self,
        user_id: &str,
        created_at: i64,
        outcome: &str,
    ) {
        self.request_stats_pipeline
            .enqueue_account_request_rollup(
                user_id,
                created_at,
                i64::from(outcome == OUTCOME_SUCCESS),
            )
            .await;
    }
}
