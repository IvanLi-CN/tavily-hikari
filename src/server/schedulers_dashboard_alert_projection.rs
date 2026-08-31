fn spawn_dashboard_alert_projection_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut last_error = None::<String>;
        // An existing projection may have become stale while this scheduler was down.
        // Refresh once after the initial idle slice, then keep the observation write sparse.
        let mut last_idle_observation_at = state
            .proxy
            .backend_time()
            .now_ts()
            .saturating_sub(DASHBOARD_ALERT_PROJECTION_IDLE_OBSERVATION_SECS);
        state
            .proxy
            .backend_time()
            .sleep(Duration::from_secs(
                DASHBOARD_ALERT_PROJECTION_INITIAL_DELAY_SECS,
            ))
            .await;
        loop {
            let mut next_delay_secs = DASHBOARD_ALERT_PROJECTION_INTERVAL_SECS;
            match state
                .proxy
                .advance_dashboard_alert_projection_scheduler_step()
                .await
            {
                Ok((true, _)) => {
                    mark_dashboard_overview_alert_projection_dirty(state.as_ref()).await;
                    prewarm_admin_alerts(state.clone()).await;
                    if last_error.take().is_some() {
                        tracing::info!(
                            component = "dashboard_alert_projection",
                            event = "recovered",
                            "alert projection recovered and advanced a durable slice"
                        );
                    }
                }
                Ok((false, true)) => {
                    next_delay_secs = DASHBOARD_ALERT_PROJECTION_IDLE_INTERVAL_SECS;
                    let now = state.proxy.backend_time().now_ts();
                    if now.saturating_sub(last_idle_observation_at)
                        >= DASHBOARD_ALERT_PROJECTION_IDLE_OBSERVATION_SECS
                    {
                        match state
                            .proxy
                            .refresh_dashboard_alert_projection_observation()
                            .await
                        {
                            Ok(true) => last_idle_observation_at = now,
                            Ok(false) => {}
                            Err(err) => tracing::debug!(
                                component = "dashboard_alert_projection",
                                event = "idle_observation_deferred",
                                err = %err,
                                "skipped an idle alert projection observation refresh"
                            ),
                        }
                    }
                }
                Ok((false, false)) => {}
                Err(err) => {
                    let error = err.to_string();
                    if last_error.as_deref() != Some(error.as_str()) {
                        tracing::warn!(
                            component = "dashboard_alert_projection",
                            event = "slice_failed",
                            err = %error,
                            "alert projection slice failed"
                        );
                    } else {
                        tracing::debug!(
                            component = "dashboard_alert_projection",
                            event = "slice_retry",
                            err = %error,
                            "alert projection remains deferred after the same error"
                        );
                    }
                    last_error = Some(error);
                }
            }
            state
                .proxy
                .backend_time()
                .sleep(Duration::from_secs(next_delay_secs))
                .await;
        }
    });
}
