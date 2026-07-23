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

    pub(crate) async fn enqueue_request_stats_rollup_for_user_for_test(
        &self,
        user_id: &str,
        created_at: i64,
        outcome: &str,
    ) {
        let mut state = self.request_stats_coalescer.state.lock().await;
        let entry = state
            .pending_account_request_rollups
            .entry(AccountRequestRollupKey {
                user_id: user_id.to_string(),
                five_minute_bucket_start: created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES),
                day_bucket_start: local_day_bucket_start_utc_ts(created_at),
            })
            .or_default();
        entry.request_count += 1;
        match outcome {
            OUTCOME_SUCCESS => entry.primary_success += 1,
            OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => {}
            _ => {}
        }
        RequestStatsCoalescer::bump_request_stats_version(&mut state);
        RequestStatsCoalescer::note_pending_created_at(&mut state, created_at);
        RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
        drop(state);
        self.request_stats_coalescer.wake.notify_one();
    }
}
