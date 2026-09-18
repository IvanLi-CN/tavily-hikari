impl SqliteRuntime {
    pub(crate) fn admin_alerts_cache_warm_defer_reason(
        &self,
    ) -> Option<SqliteAdmissionDeferReason> {
        self.admin_alerts_cache_warm_defer_reason_with_liveness(true)
    }

    pub(crate) fn admin_alerts_cache_warm_defer_reason_without_liveness(
        &self,
    ) -> Option<SqliteAdmissionDeferReason> {
        self.admin_alerts_cache_warm_defer_reason_with_liveness(false)
    }

    fn admin_alerts_cache_warm_defer_reason_with_liveness(
        &self,
        allow_liveness: bool,
    ) -> Option<SqliteAdmissionDeferReason> {
        let liveness = allow_liveness
            && self
                .inner
                .admin_alerts_cache_warm_liveness
                .load(AtomicOrdering::Acquire)
            && (self.admin_alerts_cache_warm_liveness_permit_active()
                || self.admin_alerts_cache_warm_liveness_stage_active());
        if !liveness && self.foreground_activity_rps() > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            Some(SqliteAdmissionDeferReason::ForegroundPressure)
        } else if self.recent_contention_active() {
            Some(SqliteAdmissionDeferReason::RecentContention)
        } else if self.admin_alerts_cache_warm_has_pool_pressure(liveness) {
            Some(SqliteAdmissionDeferReason::PoolPressure)
        } else {
            None
        }
    }

    pub(crate) fn admin_alerts_cache_warm_pressure_reason(&self) -> Option<&'static str> {
        let liveness = self
            .inner
            .admin_alerts_cache_warm_liveness
            .load(AtomicOrdering::Acquire)
            && (self.admin_alerts_cache_warm_liveness_permit_active()
                || self.admin_alerts_cache_warm_liveness_stage_active());
        if !liveness && self.foreground_activity_rps() > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            Some(SqliteAdmissionDeferReason::ForegroundPressure.as_str())
        } else if self.recent_contention_active() {
            Some(SqliteAdmissionDeferReason::RecentContention.as_str())
        } else if self.admin_alerts_cache_warm_has_pool_pressure(liveness) {
            Some(SqliteAdmissionDeferReason::PoolPressure.as_str())
        } else {
            None
        }
    }

    fn admin_alerts_cache_warm_has_pool_pressure(&self, liveness: bool) -> bool {
        let has_open_connection = self.inner.pool.size() > 0;
        // An aged liveness slot must reach the bounded pool acquire even when
        // foreground waiters already exist. The 100ms waiter is the safety
        // boundary; pre-admission deferral would starve the slot under
        // sustained foreground traffic.
        !liveness
            && (self.inner.acquire_waiters.load(AtomicOrdering::Acquire) > 0
                || (has_open_connection && self.inner.pool.num_idle() == 0))
    }

    pub(crate) fn set_admin_alerts_cache_warm_liveness(&self, enabled: bool) {
        let was_enabled = self
            .inner
            .admin_alerts_cache_warm_liveness
            .swap(enabled, AtomicOrdering::AcqRel);
        if !enabled || !was_enabled {
            self.inner
                .admin_alerts_cache_warm_liveness_permit
                .store(enabled, AtomicOrdering::Release);
            self.inner
                .admin_alerts_cache_warm_liveness_projection_turn
                .store(false, AtomicOrdering::Release);
        } else if !self
            .inner
            .admin_alerts_cache_warm_liveness_projection_turn
            .load(AtomicOrdering::Acquire)
        {
            // A retry may re-arm the canonical controller after it handed one
            // liveness turn to projection. Preserve that handoff while making
            // an unclaimed turn available to the next canonical stage.
            self.inner
                .admin_alerts_cache_warm_liveness_permit
                .store(true, AtomicOrdering::Release);
        }
        self.inner
            .admin_alerts_cache_warm_liveness_stage_active
            .store(false, AtomicOrdering::Release);
    }

    pub(crate) fn begin_admin_alerts_cache_warm_liveness_stage(&self) {
        if !self
            .inner
            .admin_alerts_cache_warm_liveness
            .load(AtomicOrdering::Acquire)
        {
            return;
        }
        if self
            .inner
            .admin_alerts_cache_warm_liveness_permit
            .compare_exchange(true, false, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
        {
            self.inner
                .admin_alerts_cache_warm_liveness_stage_active
                .store(true, AtomicOrdering::Release);
        }
    }

    pub(crate) fn finish_admin_alerts_cache_warm_liveness_stage(&self) {
        self.inner
            .admin_alerts_cache_warm_liveness_stage_active
            .store(false, AtomicOrdering::Release);
    }

    pub(crate) fn retain_admin_alerts_cache_warm_liveness_for_retry(&self) {
        self.finish_admin_alerts_cache_warm_liveness_stage();
        self.transfer_admin_alerts_cache_warm_liveness_to_projection();
    }

    pub(crate) fn transfer_admin_alerts_cache_warm_liveness_to_projection(&self) {
        if !self
            .inner
            .admin_alerts_cache_warm_liveness
            .load(AtomicOrdering::Acquire)
        {
            return;
        }
        self.inner
            .admin_alerts_cache_warm_liveness_stage_active
            .store(false, AtomicOrdering::Release);
        self.inner
            .admin_alerts_cache_warm_liveness_projection_turn
            .store(true, AtomicOrdering::Release);
        self.inner
            .admin_alerts_cache_warm_liveness_permit
            .store(true, AtomicOrdering::Release);
    }

    /// Test-only knob for asserting the spent-permit pressure path. Production
    /// warm attempts keep the reservation for their controller-owned attempt
    /// and clear it when that attempt yields or publishes.
    #[cfg(test)]
    pub(crate) fn consume_admin_alerts_cache_warm_liveness_permit(&self) {
        let _ = self
            .inner
            .admin_alerts_cache_warm_liveness_permit
            .compare_exchange(true, false, AtomicOrdering::AcqRel, AtomicOrdering::Acquire);
    }

    fn admin_alerts_cache_warm_liveness_permit_active(&self) -> bool {
        self.inner
            .admin_alerts_cache_warm_liveness_permit
            .load(AtomicOrdering::Acquire)
    }

    pub(crate) fn admin_alerts_cache_warm_liveness_stage_active(&self) -> bool {
        self.inner
            .admin_alerts_cache_warm_liveness_stage_active
            .load(AtomicOrdering::Acquire)
    }

    pub(crate) fn admin_alerts_cache_warm_liveness_admission_active(&self) -> bool {
        self.inner
            .admin_alerts_cache_warm_liveness
            .load(AtomicOrdering::Acquire)
            && (self.admin_alerts_cache_warm_liveness_permit_active()
                || self.admin_alerts_cache_warm_liveness_stage_active())
    }

    pub(crate) fn claim_admin_alerts_cache_warm_liveness_for_projection(
        &self,
    ) -> Option<SqliteAlertProjectionLivenessPermit> {
        if !self
            .inner
            .admin_alerts_cache_warm_liveness
            .load(AtomicOrdering::Acquire)
            || !self
                .inner
                .admin_alerts_cache_warm_liveness_projection_turn
                .load(AtomicOrdering::Acquire)
        {
            return None;
        }
        if self.admin_alerts_cache_warm_liveness_stage_active() {
            // The canonical controller owns the liveness slot for its active
            // stage. Projection may claim a transferred permit only after the
            // stage has deferred and released that ownership.
            return None;
        }
        self.inner
            .admin_alerts_cache_warm_liveness_permit
            .compare_exchange(true, false, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
            .then(|| SqliteAlertProjectionLivenessPermit {
                runtime: self.clone(),
            })
    }
}
