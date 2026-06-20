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
        self.request_stats_coalescer
            .enqueue_request_log_rollups(
                api_key_id,
                "test-auth-token",
                None,
                created_at,
                counts,
                None,
            )
            .await;
    }
}
