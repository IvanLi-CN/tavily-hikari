const DASHBOARD_QUOTA_RECOVERY_MAX_SELECTS_PER_ROUND: usize = 8;
const DASHBOARD_QUOTA_RECOVERY_ROUND_BUDGET: Duration = Duration::from_millis(250);
const DASHBOARD_QUOTA_RECOVERY_RESUME_DELAY: Duration = Duration::from_secs(1);
struct DashboardQuotaRecoveryDraft {
    target: DashboardQuotaSampleWatermark,
    target_source_id: i64,
    model: DashboardQuotaChargeReadModel,
    source_page_cursor: Option<DashboardQuotaRecoveryCursor>,
    current_page: Option<DashboardQuotaRecoveryPageDraft>,
    terminal_page_staged: bool,
    staged_key_ids: std::collections::HashSet<String>,
}

#[derive(Debug)]
struct DashboardQuotaRecoveryPageDraft {
    samples: Vec<DashboardQuotaSample>,
    baseline_keys: Vec<String>,
    next_baseline_index: usize,
    baselines: Vec<(String, Option<DashboardQuotaSample>)>,
    baseline_query: Option<DashboardQuotaRecoveryBaselineDraft>,
}

#[derive(Debug)]
struct DashboardQuotaRecoveryBaselineDraft {
    key_id: String,
    phase: DashboardQuotaRecoveryBaselinePhase,
    captured_at: Option<i64>,
    after_id: i64,
    latest_sample: Option<DashboardQuotaSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardQuotaRecoveryBaselinePhase {
    PreWindowTimestamp,
    HistoricalTimestamp,
    Samples,
}

#[derive(Debug)]
struct DashboardQuotaRecoveryRoundBudget {
    deadline: tokio::time::Instant,
    select_count: usize,
}

impl DashboardQuotaRecoveryRoundBudget {
    fn new(backend_time: &BackendTime) -> Self {
        Self {
            deadline: backend_time.deadline_after(DASHBOARD_QUOTA_RECOVERY_ROUND_BUDGET),
            select_count: 0,
        }
    }

    fn can_start_selects(&self, count: usize) -> bool {
        self.select_count.saturating_add(count) <= DASHBOARD_QUOTA_RECOVERY_MAX_SELECTS_PER_ROUND
            && tokio::time::Instant::now() < self.deadline
    }

    fn start_select(&mut self) -> bool {
        if !self.can_start_selects(1) {
            return false;
        }
        self.select_count = self.select_count.saturating_add(1);
        record_dashboard_quota_recovery_select();
        true
    }
}

enum DashboardQuotaRecoveryQuery<T> {
    Complete(T),
    Continue,
    Deferred,
}

enum DashboardQuotaRecoveryBaselineStep {
    Complete(Option<DashboardQuotaSample>),
    Advance,
    Continue,
    Deferred,
}

struct DashboardQuotaRecoveryRoundResult {
    status: DashboardQuotaRecoveryRoundStatus,
    model: Option<DashboardQuotaChargeReadModel>,
}

impl DashboardQuotaRecoveryRoundResult {
    fn complete(model: DashboardQuotaChargeReadModel) -> Self {
        Self {
            status: DashboardQuotaRecoveryRoundStatus::Complete,
            model: Some(model),
        }
    }

    fn status(status: DashboardQuotaRecoveryRoundStatus) -> Self {
        Self {
            status,
            model: None,
        }
    }
}

#[cfg(test)]
tokio::task_local! {
    static DASHBOARD_QUOTA_RECOVERY_SELECT_TRACE: std::sync::Arc<std::sync::Mutex<Vec<usize>>>;
}

#[cfg(test)]
fn start_dashboard_quota_recovery_round() {
    let _ = DASHBOARD_QUOTA_RECOVERY_SELECT_TRACE.try_with(|trace| {
        trace
            .lock()
            .expect("quota recovery select trace mutex")
            .push(0);
    });
}

#[cfg(not(test))]
fn start_dashboard_quota_recovery_round() {}

#[cfg(test)]
fn record_dashboard_quota_recovery_select() {
    let _ = DASHBOARD_QUOTA_RECOVERY_SELECT_TRACE.try_with(|trace| {
        let mut trace = trace
            .lock()
            .expect("quota recovery select trace mutex");
        if let Some(count) = trace.last_mut() {
            *count = count.saturating_add(1);
        }
    });
}

#[cfg(not(test))]
fn record_dashboard_quota_recovery_select() {}

#[cfg(test)]
pub(crate) async fn with_dashboard_quota_recovery_select_trace<F, T>(
    trace: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    future: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    DASHBOARD_QUOTA_RECOVERY_SELECT_TRACE
        .scope(trace, future)
        .await
}

fn dashboard_quota_read_was_deferred(error: &ProxyError) -> bool {
    matches!(
        error,
        ProxyError::Deferred {
            operation: "dashboard_quota_read",
            ..
        }
    )
}

impl KeyStore {

    #[cfg(test)]
    pub(crate) async fn fetch_dashboard_quota_charge_read_model(
        &self,
        bounds: SummaryWindowBounds,
        stale_key_count: i64,
    ) -> Result<DashboardQuotaChargeReadModel, ProxyError> {
        self.fetch_dashboard_quota_charge_read_model_internal(bounds, stale_key_count, false)
            .await
    }

    pub(crate) async fn fetch_dashboard_quota_charge_read_model_for_bounds(
        &self,
        bounds: SummaryWindowBounds,
        stale_key_count: i64,
    ) -> Result<DashboardQuotaChargeReadModel, ProxyError> {
        self.fetch_dashboard_quota_charge_read_model_internal(
            bounds,
            stale_key_count,
            self.dashboard_quota_recovery_bounds_are_current(bounds),
        )
            .await
    }

    async fn fetch_dashboard_quota_charge_read_model_internal(
        &self,
        bounds: SummaryWindowBounds,
        stale_key_count: i64,
        enforce_current_window: bool,
    ) -> Result<DashboardQuotaChargeReadModel, ProxyError> {
        let SummaryWindowBounds {
            today_end,
            yesterday_start,
            month_quota_charge_start,
            ..
        } = bounds;
        let sample_window_start = yesterday_start.min(month_quota_charge_start);
        let mut draft = None;
        let mut invalidation_attempts: usize = 0;
        let mut deferred_rounds: usize = 0;

        loop {
            if enforce_current_window && !self.dashboard_quota_recovery_bounds_are_current(bounds) {
                return Err(self.dashboard_quota_read_budget_deferred());
            }
            let mut budget = DashboardQuotaRecoveryRoundBudget::new(&self.backend_time);
            start_dashboard_quota_recovery_round();
            let started_new_draft = draft.is_none();
            if started_new_draft {
                let target = match self
                    .fetch_dashboard_quota_sample_watermark_for_recovery(today_end, &mut budget)
                    .await?
                {
                    DashboardQuotaRecoveryQuery::Complete(target) => target,
                    DashboardQuotaRecoveryQuery::Continue => {
                        deferred_rounds = 0;
                        self.backend_time
                            .sleep(DASHBOARD_QUOTA_RECOVERY_RESUME_DELAY)
                            .await;
                        continue;
                    }
                    DashboardQuotaRecoveryQuery::Deferred => {
                        deferred_rounds = deferred_rounds.saturating_add(1);
                        if deferred_rounds >= DASHBOARD_QUOTA_RECOVERY_MAX_ATTEMPTS {
                            return Err(self.dashboard_quota_read_budget_deferred());
                        }
                        self.backend_time
                            .sleep(DASHBOARD_QUOTA_RECOVERY_RESUME_DELAY)
                            .await;
                        continue;
                    }
                };
                draft = Some(DashboardQuotaRecoveryDraft {
                    target,
                    target_source_id: target.source_id,
                    model: DashboardQuotaChargeReadModel::empty(
                        bounds,
                        stale_key_count,
                        target,
                    ),
                    source_page_cursor: None,
                    current_page: None,
                    terminal_page_staged: false,
                    staged_key_ids: std::collections::HashSet::new(),
                });
            }

            let round = self
                .fetch_dashboard_quota_recovery_round(
                    bounds,
                    sample_window_start,
                    today_end,
                    draft.as_mut().expect("quota recovery draft is present"),
                    &mut budget,
                    !started_new_draft,
                )
                .await?;
            match round.status {
                DashboardQuotaRecoveryRoundStatus::Complete => {
                    return Ok(round.model.expect("complete recovery has a model"));
                }
                DashboardQuotaRecoveryRoundStatus::Continue
                | DashboardQuotaRecoveryRoundStatus::Deferred => {
                    if round.status == DashboardQuotaRecoveryRoundStatus::Continue {
                        deferred_rounds = 0;
                    } else {
                        deferred_rounds = deferred_rounds.saturating_add(1);
                        if deferred_rounds >= DASHBOARD_QUOTA_RECOVERY_MAX_ATTEMPTS {
                            return Err(self.dashboard_quota_read_budget_deferred());
                        }
                    }
                    self.backend_time
                        .sleep(DASHBOARD_QUOTA_RECOVERY_RESUME_DELAY)
                        .await;
                }
                DashboardQuotaRecoveryRoundStatus::Invalidated => {
                    deferred_rounds = 0;
                    draft = None;
                    invalidation_attempts = invalidation_attempts.saturating_add(1);
                    if invalidation_attempts >= DASHBOARD_QUOTA_RECOVERY_MAX_ATTEMPTS {
                        return Err(self.dashboard_quota_read_budget_deferred());
                    }
                }
            }
        }
    }

    fn dashboard_quota_recovery_bounds_are_current(&self, bounds: SummaryWindowBounds) -> bool {
        let now = self.backend_time.local_now();
        start_of_local_day_utc_ts(now) == bounds.today_start
            && previous_local_day_start_utc_ts(now) == bounds.yesterday_start
            && start_of_local_month_utc_ts(now) == bounds.month_start
    }

    async fn fetch_dashboard_quota_recovery_round(
        &self,
        bounds: SummaryWindowBounds,
        sample_window_start: i64,
        today_end: i64,
        draft: &mut DashboardQuotaRecoveryDraft,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
        check_target: bool,
    ) -> Result<DashboardQuotaRecoveryRoundResult, ProxyError> {
        if !draft.model.has_same_windows(bounds) {
            return Ok(DashboardQuotaRecoveryRoundResult::status(
                DashboardQuotaRecoveryRoundStatus::Invalidated,
            ));
        }
        if check_target {
            match self
                .fetch_dashboard_quota_sample_watermark_for_recovery(today_end, budget)
                .await?
            {
                DashboardQuotaRecoveryQuery::Complete(watermark) if watermark == draft.target => {}
                DashboardQuotaRecoveryQuery::Complete(_) => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Invalidated,
                    ));
                }
                DashboardQuotaRecoveryQuery::Continue => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Continue,
                    ));
                }
                DashboardQuotaRecoveryQuery::Deferred => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Deferred,
                    ));
                }
            }
        }

        if draft.terminal_page_staged {
            draft.model.finish_recovery();
            return Ok(DashboardQuotaRecoveryRoundResult::complete(
                draft.model.clone(),
            ));
        }

        loop {
            if draft.current_page.is_none() {
                let samples = match self
                    .fetch_dashboard_quota_recovery_page_for_recovery(
                        sample_window_start,
                        today_end,
                        draft.target_source_id,
                        draft.source_page_cursor.as_ref(),
                        budget,
                    )
                    .await?
                {
                    DashboardQuotaRecoveryQuery::Complete(samples) => samples,
                    DashboardQuotaRecoveryQuery::Continue => {
                        return Ok(DashboardQuotaRecoveryRoundResult::status(
                            DashboardQuotaRecoveryRoundStatus::Continue,
                        ));
                    }
                    DashboardQuotaRecoveryQuery::Deferred => {
                        return Ok(DashboardQuotaRecoveryRoundResult::status(
                            DashboardQuotaRecoveryRoundStatus::Deferred,
                        ));
                    }
                };
                let baseline_keys = samples
                    .iter()
                    .filter_map(|sample| {
                        draft
                            .staged_key_ids
                            .insert(sample.key_id.clone())
                            .then_some(sample.key_id.clone())
                    })
                    .collect::<Vec<_>>();
                draft.current_page = Some(DashboardQuotaRecoveryPageDraft {
                    samples,
                    baseline_keys,
                    next_baseline_index: 0,
                    baselines: Vec::new(),
                    baseline_query: None,
                });
            }

            let needs_baseline = draft
                .current_page
                .as_ref()
                .is_some_and(|page| page.next_baseline_index < page.baseline_keys.len());
            if needs_baseline {
                if draft
                    .current_page
                    .as_ref()
                    .is_some_and(|page| page.baseline_query.is_none())
                {
                    let page = draft
                        .current_page
                        .as_mut()
                        .expect("quota recovery page is present");
                    let key_id = page.baseline_keys[page.next_baseline_index].clone();
                    page.baseline_query = Some(DashboardQuotaRecoveryBaselineDraft {
                        key_id,
                        phase: DashboardQuotaRecoveryBaselinePhase::PreWindowTimestamp,
                        captured_at: None,
                        after_id: 0,
                        latest_sample: None,
                    });
                }

                let baseline_step = {
                    let page = draft
                        .current_page
                        .as_mut()
                        .expect("quota recovery page is present");
                    let baseline = page
                        .baseline_query
                        .as_mut()
                        .expect("quota recovery baseline query is present");
                    self.fetch_dashboard_quota_recovery_baseline_step(
                        sample_window_start,
                        draft.target_source_id,
                        baseline,
                        budget,
                    )
                    .await?
                };
                match baseline_step {
                    DashboardQuotaRecoveryBaselineStep::Complete(baseline) => {
                        let page = draft
                            .current_page
                            .as_mut()
                            .expect("quota recovery page is present");
                        let baseline_query = page
                            .baseline_query
                            .take()
                            .expect("quota recovery baseline query is present");
                        page.baselines.push((baseline_query.key_id, baseline));
                        page.next_baseline_index = page.next_baseline_index.saturating_add(1);
                    }
                    DashboardQuotaRecoveryBaselineStep::Advance => continue,
                    DashboardQuotaRecoveryBaselineStep::Continue => {
                        return Ok(DashboardQuotaRecoveryRoundResult::status(
                            DashboardQuotaRecoveryRoundStatus::Continue,
                        ));
                    }
                    DashboardQuotaRecoveryBaselineStep::Deferred => {
                        return Ok(DashboardQuotaRecoveryRoundResult::status(
                            DashboardQuotaRecoveryRoundStatus::Deferred,
                        ));
                    }
                }
                continue;
            }

            let page = draft
                .current_page
                .take()
                .expect("quota recovery page is present");
            let page_is_terminal = page.samples.len() < DASHBOARD_QUOTA_RECOVERY_PAGE_SIZE as usize;
            if !page_is_terminal {
                draft.source_page_cursor = page.samples.last().map(DashboardQuotaRecoveryCursor::from);
            }
            if page_is_terminal {
                draft.terminal_page_staged = true;
            }
            draft.model.stage_recovery_page_desc(&page.baselines, &page.samples);
            #[cfg(debug_assertions)]
            self.wait_for_dashboard_overview_read_pause_if_installed().await;

            match self
                .fetch_dashboard_quota_sample_watermark_for_recovery(today_end, budget)
                .await?
            {
                DashboardQuotaRecoveryQuery::Complete(watermark) if watermark == draft.target => {}
                DashboardQuotaRecoveryQuery::Complete(_) => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Invalidated,
                    ));
                }
                DashboardQuotaRecoveryQuery::Continue => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Continue,
                    ));
                }
                DashboardQuotaRecoveryQuery::Deferred => {
                    return Ok(DashboardQuotaRecoveryRoundResult::status(
                        DashboardQuotaRecoveryRoundStatus::Deferred,
                    ));
                }
            }
            if page_is_terminal {
                draft.model.finish_recovery();
                return Ok(DashboardQuotaRecoveryRoundResult::complete(
                    draft.model.clone(),
                ));
            }
        }
    }

    async fn fetch_dashboard_quota_sample_watermark_for_recovery(
        &self,
        today_end: i64,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
    ) -> Result<DashboardQuotaRecoveryQuery<DashboardQuotaSampleWatermark>, ProxyError> {
        let source_id = match self
            .fetch_dashboard_quota_source_id_for_recovery(today_end, budget)
            .await?
        {
            DashboardQuotaRecoveryQuery::Complete(source_id) => source_id,
            DashboardQuotaRecoveryQuery::Continue => {
                return Ok(DashboardQuotaRecoveryQuery::Continue);
            }
            DashboardQuotaRecoveryQuery::Deferred => {
                return Ok(DashboardQuotaRecoveryQuery::Deferred);
            }
        };
        let source_captured_at = match self
            .fetch_dashboard_quota_source_captured_at_for_recovery(today_end, budget)
            .await?
        {
            DashboardQuotaRecoveryQuery::Complete(source_captured_at) => source_captured_at,
            DashboardQuotaRecoveryQuery::Continue => {
                return Ok(DashboardQuotaRecoveryQuery::Continue);
            }
            DashboardQuotaRecoveryQuery::Deferred => {
                return Ok(DashboardQuotaRecoveryQuery::Deferred);
            }
        };
        Ok(DashboardQuotaRecoveryQuery::Complete(
            DashboardQuotaSampleWatermark {
                source_id,
                source_captured_at,
                source_count: source_id,
            },
        ))
    }

    async fn fetch_dashboard_quota_source_id_for_recovery(
        &self,
        today_end: i64,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
    ) -> Result<DashboardQuotaRecoveryQuery<i64>, ProxyError> {
        let mut before_id = None;
        for _ in 0..DASHBOARD_QUOTA_SOURCE_MAX_PAGES {
            if !budget.start_select() {
                return Ok(DashboardQuotaRecoveryQuery::Continue);
            }
            let mut session = match self
                .begin_admin_alerts_read_session_for_operation(SqliteOperation::DashboardQuotaRead)
                .await
            {
                Ok(session) => session,
                Err(error) if dashboard_quota_read_was_deferred(&error) => {
                    return Ok(DashboardQuotaRecoveryQuery::Deferred);
                }
                Err(error) => return Err(error),
            };
            let query_result = match before_id {
                Some(before_id) => sqlx::query(
                    r#"
                    SELECT id, captured_at
                    FROM api_key_quota_sync_samples
                    WHERE id > 0 AND id < ?
                    ORDER BY id DESC
                    LIMIT ?
                    "#,
                )
                .bind(before_id)
                .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                .fetch_all(&mut *session)
                .await,
                None => sqlx::query(
                    r#"
                    SELECT id, captured_at
                    FROM api_key_quota_sync_samples
                    WHERE id > 0
                    ORDER BY id DESC
                    LIMIT ?
                    "#,
                )
                .bind(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE)
                .fetch_all(&mut *session)
                .await,
            };
            let rows = session.query(query_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = match rows {
                Ok(rows) => rows,
                Err(error) if dashboard_quota_read_was_deferred(&error) => {
                    return Ok(DashboardQuotaRecoveryQuery::Deferred);
                }
                Err(error) => return Err(error),
            };
            let samples = rows
                .into_iter()
                .map(|row| Ok::<_, sqlx::Error>((row.try_get("id")?, row.try_get("captured_at")?)))
                .collect::<Result<Vec<(i64, i64)>, _>>()?;
            if let Some((id, _)) = samples
                .iter()
                .copied()
                .find(|(_, captured_at)| *captured_at < today_end)
            {
                return Ok(DashboardQuotaRecoveryQuery::Complete(id));
            }
            let Some((last_id, _)) = samples.last().copied() else {
                return Ok(DashboardQuotaRecoveryQuery::Complete(0));
            };
            if samples.len() < DASHBOARD_QUOTA_SOURCE_PAGE_SIZE as usize {
                return Ok(DashboardQuotaRecoveryQuery::Complete(0));
            }
            before_id = Some(last_id);
        }
        Ok(DashboardQuotaRecoveryQuery::Deferred)
    }

    async fn fetch_dashboard_quota_source_captured_at_for_recovery(
        &self,
        today_end: i64,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
    ) -> Result<DashboardQuotaRecoveryQuery<i64>, ProxyError> {
        if !budget.start_select() {
            return Ok(DashboardQuotaRecoveryQuery::Continue);
        }
        let mut session = match self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::DashboardQuotaRead)
            .await
        {
            Ok(session) => session,
            Err(error) if dashboard_quota_read_was_deferred(&error) => {
                return Ok(DashboardQuotaRecoveryQuery::Deferred);
            }
            Err(error) => return Err(error),
        };
        let query_result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT captured_at
            FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
            WHERE captured_at < ?
            ORDER BY captured_at DESC, key_id ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(today_end)
        .bind(1_i64)
        .fetch_optional(&mut *session)
        .await;
        let source_captured_at = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        match source_captured_at {
            Ok(source_captured_at) => Ok(DashboardQuotaRecoveryQuery::Complete(
                source_captured_at.unwrap_or_default(),
            )),
            Err(error) if dashboard_quota_read_was_deferred(&error) => {
                Ok(DashboardQuotaRecoveryQuery::Deferred)
            }
            Err(error) => Err(error),
        }
    }

    async fn fetch_dashboard_quota_recovery_page_for_recovery(
        &self,
        sample_window_start: i64,
        today_end: i64,
        target_source_id: i64,
        cursor: Option<&DashboardQuotaRecoveryCursor>,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
    ) -> Result<DashboardQuotaRecoveryQuery<Vec<DashboardQuotaSample>>, ProxyError> {
        if !budget.start_select() {
            return Ok(DashboardQuotaRecoveryQuery::Continue);
        }
        let mut session = match self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::DashboardQuotaRead)
            .await
        {
            Ok(session) => session,
            Err(error) if dashboard_quota_read_was_deferred(&error) => {
                return Ok(DashboardQuotaRecoveryQuery::Deferred);
            }
            Err(error) => return Err(error),
        };
        let page_size = DASHBOARD_QUOTA_RECOVERY_PAGE_SIZE;
        let query_result = match cursor {
            Some(cursor) => sqlx::query(
                r#"
                SELECT id, key_id, quota_remaining, captured_at
                FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
                WHERE captured_at >= ?
                  AND captured_at < ?
                  AND id <= ?
                  AND (
                    captured_at < ?
                    OR (
                        captured_at = ?
                        AND (
                            key_id > ?
                            OR (key_id = ? AND id > ?)
                        )
                    )
                  )
                ORDER BY captured_at DESC, key_id ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(sample_window_start)
            .bind(today_end)
            .bind(target_source_id)
            .bind(cursor.captured_at)
            .bind(cursor.captured_at)
            .bind(&cursor.key_id)
            .bind(&cursor.key_id)
            .bind(cursor.id)
            .bind(page_size)
            .fetch_all(&mut *session)
            .await,
            None => sqlx::query(
                r#"
                SELECT id, key_id, quota_remaining, captured_at
                FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
                WHERE captured_at >= ?
                  AND captured_at < ?
                  AND id <= ?
                ORDER BY captured_at DESC, key_id ASC, id ASC
                LIMIT ?
                "#,
            )
            .bind(sample_window_start)
            .bind(today_end)
            .bind(target_source_id)
            .bind(page_size)
            .fetch_all(&mut *session)
            .await,
        };
        let rows = session.query(query_result).await;
        let finish = session.finish().await;
        finish?;
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) if dashboard_quota_read_was_deferred(&error) => {
                return Ok(DashboardQuotaRecoveryQuery::Deferred);
            }
            Err(error) => return Err(error),
        };
        rows.into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(DashboardQuotaSample {
                    id: row.try_get("id")?,
                    key_id: row.try_get("key_id")?,
                    quota_remaining: row.try_get("quota_remaining")?,
                    captured_at: row.try_get("captured_at")?,
                    previous_quota_remaining: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(DashboardQuotaRecoveryQuery::Complete)
            .map_err(ProxyError::Database)
    }

    async fn fetch_dashboard_quota_recovery_baseline_step(
        &self,
        sample_window_start: i64,
        target_source_id: i64,
        baseline: &mut DashboardQuotaRecoveryBaselineDraft,
        budget: &mut DashboardQuotaRecoveryRoundBudget,
    ) -> Result<DashboardQuotaRecoveryBaselineStep, ProxyError> {
        match baseline.phase {
            DashboardQuotaRecoveryBaselinePhase::PreWindowTimestamp
            | DashboardQuotaRecoveryBaselinePhase::HistoricalTimestamp => {
                if !budget.start_select() {
                    return Ok(DashboardQuotaRecoveryBaselineStep::Continue);
                }
                let lower_bound = match baseline.phase {
                    DashboardQuotaRecoveryBaselinePhase::PreWindowTimestamp => sample_window_start
                        .saturating_sub(DASHBOARD_QUOTA_RECOVERY_PRE_WINDOW_SECS),
                    DashboardQuotaRecoveryBaselinePhase::HistoricalTimestamp => i64::MIN,
                    DashboardQuotaRecoveryBaselinePhase::Samples => unreachable!(),
                };
                let mut session = match self
                    .begin_admin_alerts_read_session_for_operation(SqliteOperation::DashboardQuotaRead)
                    .await
                {
                    Ok(session) => session,
                    Err(error) if dashboard_quota_read_was_deferred(&error) => {
                        return Ok(DashboardQuotaRecoveryBaselineStep::Deferred);
                    }
                    Err(error) => return Err(error),
                };
                let query_result = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT captured_at
                    FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
                    WHERE key_id = ?
                      AND captured_at >= ?
                      AND captured_at < ?
                      AND id <= ?
                    ORDER BY captured_at DESC
                    LIMIT ?
                    "#,
                )
                .bind(&baseline.key_id)
                .bind(lower_bound)
                .bind(sample_window_start)
                .bind(target_source_id)
                .bind(1_i64)
                .fetch_optional(&mut *session)
                .await;
                let captured_at = session.query(query_result).await;
                let finish = session.finish().await;
                finish?;
                let captured_at = match captured_at {
                    Ok(captured_at) => captured_at,
                    Err(error) if dashboard_quota_read_was_deferred(&error) => {
                        return Ok(DashboardQuotaRecoveryBaselineStep::Deferred);
                    }
                    Err(error) => return Err(error),
                };
                if let Some(captured_at) = captured_at {
                    baseline.captured_at = Some(captured_at);
                    baseline.after_id = 0;
                    baseline.phase = DashboardQuotaRecoveryBaselinePhase::Samples;
                    return Ok(DashboardQuotaRecoveryBaselineStep::Advance);
                }
                if baseline.phase == DashboardQuotaRecoveryBaselinePhase::PreWindowTimestamp {
                    baseline.phase = DashboardQuotaRecoveryBaselinePhase::HistoricalTimestamp;
                    return Ok(DashboardQuotaRecoveryBaselineStep::Advance);
                }
                Ok(DashboardQuotaRecoveryBaselineStep::Complete(
                    baseline.latest_sample.take(),
                ))
            }
            DashboardQuotaRecoveryBaselinePhase::Samples => {
                if !budget.start_select() {
                    return Ok(DashboardQuotaRecoveryBaselineStep::Continue);
                }
                let captured_at = baseline
                    .captured_at
                    .expect("sample baseline phase has a timestamp");
                let mut session = match self
                    .begin_admin_alerts_read_session_for_operation(SqliteOperation::DashboardQuotaRead)
                    .await
                {
                    Ok(session) => session,
                    Err(error) if dashboard_quota_read_was_deferred(&error) => {
                        return Ok(DashboardQuotaRecoveryBaselineStep::Deferred);
                    }
                    Err(error) => return Err(error),
                };
                let query_result = sqlx::query(
                    r#"
                    SELECT id, key_id, quota_remaining, captured_at
                    FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
                    WHERE key_id = ?
                      AND captured_at = ?
                      AND id > ?
                      AND id <= ?
                    ORDER BY id ASC
                    LIMIT ?
                    "#,
                )
                .bind(&baseline.key_id)
                .bind(captured_at)
                .bind(baseline.after_id)
                .bind(target_source_id)
                .bind(DASHBOARD_QUOTA_RECOVERY_PAGE_SIZE)
                .fetch_all(&mut *session)
                .await;
                let rows = session.query(query_result).await;
                let finish = session.finish().await;
                finish?;
                let rows = match rows {
                    Ok(rows) => rows,
                    Err(error) if dashboard_quota_read_was_deferred(&error) => {
                        return Ok(DashboardQuotaRecoveryBaselineStep::Deferred);
                    }
                    Err(error) => return Err(error),
                };
                let samples = rows
                    .into_iter()
                    .map(|row| {
                        Ok::<_, sqlx::Error>(DashboardQuotaSample {
                            id: row.try_get("id")?,
                            key_id: row.try_get("key_id")?,
                            quota_remaining: row.try_get("quota_remaining")?,
                            captured_at: row.try_get("captured_at")?,
                            previous_quota_remaining: None,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let Some(last_sample) = samples.last().cloned() else {
                    return Ok(DashboardQuotaRecoveryBaselineStep::Complete(
                        baseline.latest_sample.take(),
                    ));
                };
                baseline.after_id = last_sample.id;
                baseline.latest_sample = Some(last_sample);
                if samples.len() < DASHBOARD_QUOTA_RECOVERY_PAGE_SIZE as usize {
                    return Ok(DashboardQuotaRecoveryBaselineStep::Complete(
                        baseline.latest_sample.take(),
                    ));
                }
                Ok(DashboardQuotaRecoveryBaselineStep::Advance)
            }
        }
    }
}

