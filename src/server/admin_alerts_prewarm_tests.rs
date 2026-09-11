use super::{
    ADMIN_ALERTS_PREWARM_MIN_INTERVAL, AdminAlertsReadCacheValue, AlertCatalog,
    DashboardOverviewCacheState, PaginatedAlertEvents, PaginatedAlertGroups,
    publish_admin_alerts_canonical_into_cache,
};

fn empty_catalog() -> AlertCatalog {
    AlertCatalog {
        retention_days: 30,
        types: Vec::new(),
        request_kind_options: Vec::new(),
        users: Vec::new(),
        tokens: Vec::new(),
        keys: Vec::new(),
    }
}

fn empty_events() -> PaginatedAlertEvents {
    PaginatedAlertEvents {
        items: Vec::new(),
        total: 0,
        page: 1,
        per_page: 20,
    }
}

fn empty_groups() -> PaginatedAlertGroups {
    PaginatedAlertGroups {
        items: Vec::new(),
        total: 0,
        page: 1,
        per_page: 20,
    }
}

#[test]
fn admin_alerts_prewarm_coalesces_projection_updates_for_one_minute() {
    let mut cache = DashboardOverviewCacheState::default();
    let now = tokio::time::Instant::now();

    assert!(cache.try_start_admin_alerts_prewarm(now));
    assert!(
        !cache.try_start_admin_alerts_prewarm(now),
        "an in-flight prewarm must remain singleflight"
    );

    cache.finish_admin_alerts_prewarm();
    assert!(
        !cache.try_start_admin_alerts_prewarm(
            now + ADMIN_ALERTS_PREWARM_MIN_INTERVAL - std::time::Duration::from_secs(1)
        ),
        "a busy projection must not restart the full admin cache warmup every slice"
    );
    assert!(cache.try_start_admin_alerts_prewarm(
        now + ADMIN_ALERTS_PREWARM_MIN_INTERVAL + std::time::Duration::from_millis(1)
    ));
}

#[test]
fn admin_alerts_prewarm_backoff_is_bounded_and_recovers() {
    let mut cache = DashboardOverviewCacheState::default();
    let now = tokio::time::Instant::now();

    assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(5));
    assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(5));
    assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(30));

    cache.finish_admin_alerts_prewarm();
    assert_eq!(cache.admin_alerts_prewarm_defers, 0);
}

#[test]
fn initial_admin_alerts_prewarm_defer_keeps_the_worker_retryable() {
    let mut cache = DashboardOverviewCacheState::default();
    let now = tokio::time::Instant::now();

    assert!(cache.try_start_admin_alerts_prewarm(now));
    assert_eq!(
        cache.defer_admin_alerts_prewarm(now),
        std::time::Duration::from_secs(5)
    );
    assert!(cache.admin_alerts_prewarm_in_flight);
    assert_eq!(cache.admin_alerts_prewarm_defers, 1);
    assert!(
        !cache.try_start_admin_alerts_prewarm(now + std::time::Duration::from_secs(5)),
        "the original worker owns the first retry instead of losing its staged attempt"
    );
}

#[test]
fn admin_alerts_prewarm_gets_a_liveness_slot_after_two_minutes_without_progress() {
    let mut cache = DashboardOverviewCacheState::default();
    let now = tokio::time::Instant::now();

    assert!(cache.try_start_admin_alerts_prewarm(now));
    assert!(!cache.admin_alerts_prewarm_liveness_due(
        now + std::time::Duration::from_secs(119)
    ));
    assert!(cache.admin_alerts_prewarm_liveness_due(
        now + std::time::Duration::from_secs(120)
    ));

    cache.record_admin_alerts_prewarm_slice(now + std::time::Duration::from_secs(120));
    assert!(cache.admin_alerts_prewarm_liveness_due(
        now + std::time::Duration::from_secs(121)
    ));
    assert!(cache.admin_alerts_prewarm_liveness_due(
        now + std::time::Duration::from_secs(240)
    ));
}

#[test]
fn completed_admin_alerts_prewarm_keeps_publish_time_as_liveness_anchor() {
    let mut cache = DashboardOverviewCacheState::default();
    let started_at = tokio::time::Instant::now();
    let owner = cache
        .start_admin_alerts_prewarm(started_at)
        .expect("prewarm owner");
    let published_at = started_at + std::time::Duration::from_secs(7);

    cache.publish_admin_alerts_prewarm_owner(owner, published_at);

    assert_eq!(
        cache.admin_alerts_prewarm_last_progress_at,
        Some(published_at),
        "a complete canonical publish is the liveness anchor"
    );
    assert!(
        !cache.admin_alerts_prewarm_liveness_due(
            published_at + std::time::Duration::from_secs(119)
        ),
        "the next liveness slot waits 120s from the complete publish"
    );
    assert!(cache.admin_alerts_prewarm_liveness_due(
        published_at + std::time::Duration::from_secs(120)
    ));

    cache.admin_alerts_prewarm_not_before = None;
    let failed_owner = cache
        .start_admin_alerts_prewarm(tokio::time::Instant::now())
        .expect("retry owner after a completed publish");
    cache.finish_admin_alerts_prewarm_owner(failed_owner);
    assert_eq!(
        cache.admin_alerts_prewarm_last_progress_at,
        Some(published_at),
        "a failed retry must not erase the last complete publish anchor"
    );
}

#[tokio::test]
async fn cancelled_admin_alerts_flight_releases_its_owner() {
    let cache = std::sync::Arc::new(tokio::sync::Mutex::new(
        DashboardOverviewCacheState::default(),
    ));
    let owner = cache
        .lock()
        .await
        .start_admin_alerts_prewarm(tokio::time::Instant::now())
        .expect("flight owner");
    let task_cache = cache.clone();
    let guard = super::AdminAlertsFlightGuard::new(
        task_cache,
        super::AdminAlertsFlightKind::Prewarm,
        owner,
        None,
    );
    let task = tokio::spawn(async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    task.abort();
    let _ = task.await;
    assert!(!cache.lock().await.admin_alerts_prewarm_in_flight);
}

#[test]
fn canonical_publish_rejects_a_snapshot_after_projection_advance() {
    let mut cache = DashboardOverviewCacheState {
        alert_projection_generation: 2,
        ..Default::default()
    };

    assert!(!publish_admin_alerts_canonical_into_cache(
        &mut cache,
        1,
        123,
        tokio::time::Instant::now(),
        empty_catalog(),
        empty_events(),
        empty_groups(),
    ));
    assert!(cache.admin_alerts.entries.is_empty(), "a stale build must not publish any key");
    assert!(
        publish_admin_alerts_canonical_into_cache(
            &mut cache,
            2,
            124,
            tokio::time::Instant::now(),
            empty_catalog(),
            empty_events(),
            empty_groups(),
        ),
        "only a snapshot that matches the current projection generation may publish"
    );
    assert_eq!(cache.admin_alerts.entries.len(), 3);
    assert!(cache
        .admin_alerts
        .entries
        .iter()
        .all(|entry| entry.generation == 2 && entry.canonical));
    assert!(matches!(
        cache.admin_alerts.entries[0].value,
        AdminAlertsReadCacheValue::Groups(_)
    ));
}
