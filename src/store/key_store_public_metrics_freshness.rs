impl KeyStore {
    async fn flush_request_stats_writes_if_public_metrics_stale(
        &self,
        month_start: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<(), ProxyError> {
        let Some((oldest_pending_created_at, newest_pending_created_at)) = self
            .request_stats_coalescer
            .freshness_created_at_bounds()
            .await
        else {
            return Ok(());
        };

        let pending_overlaps_day =
            oldest_pending_created_at < day_end && newest_pending_created_at >= day_start;
        let pending_overlaps_month = newest_pending_created_at >= month_start;

        if !(pending_overlaps_day || pending_overlaps_month) {
            return Ok(());
        }

        self.flush_request_stats_writes().await?;
        Ok(())
    }
}
