use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DashboardQuotaSampleWatermark {
    pub source_id: i64,
    pub source_captured_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct DashboardQuotaSample {
    pub id: i64,
    pub key_id: String,
    pub quota_remaining: i64,
    pub captured_at: i64,
    pub previous_quota_remaining: Option<i64>,
}

/// Rebuildable quota-only state for Dashboard's append-only sample source.
///
/// The overview cache owns presentation data. This model retains just enough
/// per-Key history to apply a bounded sequence of appended samples without
/// recomputing the complete Dashboard payload.
#[derive(Debug, Clone)]
pub(crate) struct DashboardQuotaChargeReadModel {
    pub snapshot: DashboardQuotaChargeSnapshot,
    pub watermark: DashboardQuotaSampleWatermark,
    bounds: SummaryWindowBounds,
    last_sample_by_key: HashMap<String, DashboardQuotaSample>,
    today_sampled_keys: HashSet<String>,
    yesterday_sampled_keys: HashSet<String>,
    month_sampled_keys: HashSet<String>,
}

impl DashboardQuotaChargeReadModel {
    pub(crate) fn from_samples(
        bounds: SummaryWindowBounds,
        stale_key_count: i64,
        watermark: DashboardQuotaSampleWatermark,
        samples: Vec<DashboardQuotaSample>,
    ) -> Self {
        let mut model = Self {
            snapshot: DashboardQuotaChargeSnapshot::default(),
            watermark,
            bounds,
            last_sample_by_key: HashMap::new(),
            today_sampled_keys: HashSet::new(),
            yesterday_sampled_keys: HashSet::new(),
            month_sampled_keys: HashSet::new(),
        };
        for sample in samples {
            model.apply_sample(&sample);
        }
        model.set_stale_key_count(stale_key_count);
        model
    }

    pub(crate) fn has_same_windows(&self, bounds: SummaryWindowBounds) -> bool {
        self.bounds.today_start == bounds.today_start
            && self.bounds.yesterday_start == bounds.yesterday_start
            && self.bounds.month_quota_charge_start == bounds.month_quota_charge_start
    }

    pub(crate) fn can_hydrate(
        &self,
        next_watermark: DashboardQuotaSampleWatermark,
        samples: &[DashboardQuotaSample],
    ) -> bool {
        if next_watermark.source_id < self.watermark.source_id
            || next_watermark.source_captured_at < self.watermark.source_captured_at
        {
            return false;
        }
        if next_watermark.source_id == self.watermark.source_id {
            return next_watermark == self.watermark && samples.is_empty();
        }
        if samples.is_empty()
            || samples.first().map(|sample| sample.id)
                != Some(self.watermark.source_id.saturating_add(1))
            || samples.last().map(|sample| sample.id) != Some(next_watermark.source_id)
        {
            return false;
        }

        let mut captured_at = self.watermark.source_captured_at;
        for sample in samples {
            if sample.captured_at < captured_at {
                return false;
            }
            captured_at = sample.captured_at;
        }
        true
    }

    pub(crate) fn hydrate(
        &mut self,
        bounds: SummaryWindowBounds,
        next_watermark: DashboardQuotaSampleWatermark,
        stale_key_count: i64,
        samples: &[DashboardQuotaSample],
    ) {
        self.bounds = bounds;
        for sample in samples {
            self.apply_sample(sample);
        }
        self.watermark = next_watermark;
        self.set_stale_key_count(stale_key_count);
    }

    fn apply_sample(&mut self, sample: &DashboardQuotaSample) {
        let previous = self
            .last_sample_by_key
            .get(&sample.key_id)
            .map(|previous| previous.quota_remaining)
            .or(sample.previous_quota_remaining);
        let delta = previous
            .map(|previous| previous.saturating_sub(sample.quota_remaining).max(0))
            .unwrap_or_default();

        if sample.captured_at >= self.bounds.month_quota_charge_start
            && sample.captured_at < self.bounds.today_end
        {
            self.snapshot.month.upstream_actual_credits = self
                .snapshot
                .month
                .upstream_actual_credits
                .saturating_add(delta);
            self.month_sampled_keys.insert(sample.key_id.clone());
            update_latest_sync_at(&mut self.snapshot.month, sample.captured_at);
        }
        if sample.captured_at >= self.bounds.today_start
            && sample.captured_at < self.bounds.today_end
        {
            self.snapshot.today.upstream_actual_credits = self
                .snapshot
                .today
                .upstream_actual_credits
                .saturating_add(delta);
            self.today_sampled_keys.insert(sample.key_id.clone());
            update_latest_sync_at(&mut self.snapshot.today, sample.captured_at);
        }
        if sample.captured_at >= self.bounds.yesterday_start
            && sample.captured_at < self.bounds.yesterday_end
        {
            self.snapshot.yesterday.upstream_actual_credits = self
                .snapshot
                .yesterday
                .upstream_actual_credits
                .saturating_add(delta);
            self.yesterday_sampled_keys.insert(sample.key_id.clone());
            update_latest_sync_at(&mut self.snapshot.yesterday, sample.captured_at);
        }

        self.last_sample_by_key
            .insert(sample.key_id.clone(), sample.clone());
    }

    fn set_stale_key_count(&mut self, stale_key_count: i64) {
        self.snapshot.today.sampled_key_count = self.today_sampled_keys.len() as i64;
        self.snapshot.today.stale_key_count = stale_key_count;
        self.snapshot.yesterday.sampled_key_count = self.yesterday_sampled_keys.len() as i64;
        self.snapshot.yesterday.stale_key_count = stale_key_count;
        self.snapshot.month.sampled_key_count = self.month_sampled_keys.len() as i64;
        self.snapshot.month.stale_key_count = stale_key_count;
    }
}

fn update_latest_sync_at(charge: &mut SummaryQuotaCharge, captured_at: i64) {
    if charge
        .latest_sync_at
        .map(|latest| captured_at > latest)
        .unwrap_or(true)
    {
        charge.latest_sync_at = Some(captured_at);
    }
}

#[derive(Debug, Clone)]
pub struct AdminQuotaLimitSet {
    pub business_calls_1h_limit: i64,
    pub daily_credits_limit: i64,
    pub monthly_credits_limit: i64,
    pub inherits_defaults: bool,
}

#[derive(Debug, Clone)]
pub struct AdminUserTag {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub system_key: Option<String>,
    pub effect_kind: String,
    pub business_calls_1h_delta: i64,
    pub daily_credits_delta: i64,
    pub monthly_credits_delta: i64,
    pub user_count: i64,
}

#[derive(Debug, Clone)]
pub struct AdminUserTagBinding {
    pub tag_id: String,
    pub name: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub system_key: Option<String>,
    pub effect_kind: String,
    pub business_calls_1h_delta: i64,
    pub daily_credits_delta: i64,
    pub monthly_credits_delta: i64,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct AdminUserQuotaBreakdownEntry {
    pub kind: String,
    pub label: String,
    pub tag_id: Option<String>,
    pub tag_name: Option<String>,
    pub source: Option<String>,
    pub effect_kind: String,
    pub business_calls_1h_delta: i64,
    pub daily_credits_delta: i64,
    pub monthly_credits_delta: i64,
}

#[derive(Debug, Clone)]
pub struct AdminUserQuotaDetails {
    pub base: AdminQuotaLimitSet,
    pub effective: AdminQuotaLimitSet,
    pub breakdown: Vec<AdminUserQuotaBreakdownEntry>,
    pub tags: Vec<AdminUserTagBinding>,
}

#[derive(Debug, Clone)]
pub struct UserDashboardSummary {
    pub debug_info_shared: bool,
    pub request_rate: RequestRateView,
    pub business_calls_1h: BusinessCalls1hSummary,
    pub daily_credits_used: i64,
    pub daily_credits_limit: i64,
    pub monthly_credits_used: i64,
    pub monthly_credits_limit: i64,
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
    pub recharge: LinuxDoCreditRechargeSummary,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BusinessCalls1hSummary {
    pub success_count: i64,
    pub failure_count: i64,
    pub total_count: i64,
    pub limit: i64,
    pub window_minutes: i64,
}

#[derive(Debug, Clone)]
pub struct BusinessCalls1hLimitVerdict {
    pub allowed: bool,
    pub summary: BusinessCalls1hSummary,
}

impl BusinessCalls1hLimitVerdict {
    pub fn new(summary: BusinessCalls1hSummary) -> Self {
        let limit = summary.limit.max(0);
        let total_count = summary.total_count.max(0);
        Self {
            allowed: limit > 0 && total_count < limit,
            summary,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UserLogMetricsSummary {
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenLogMetricsSummary {
    pub daily_success: i64,
    pub daily_failure: i64,
    pub monthly_success: i64,
    pub monthly_failure: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminUserUsageSeriesKind {
    Rate5m,
    BusinessCalls1h,
    DailyCredits,
    MonthlyCredits,
}

impl AdminUserUsageSeriesKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "rate5m" => Some(Self::Rate5m),
            "businessCalls1h" => Some(Self::BusinessCalls1h),
            "dailyCredits" => Some(Self::DailyCredits),
            "monthlyCredits" => Some(Self::MonthlyCredits),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserUsageSeriesPoint {
    pub bucket_start: i64,
    pub display_bucket_start: Option<i64>,
    pub value: Option<i64>,
    pub limit_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserBusinessCalls1hBarsPoint {
    pub success: Option<i64>,
    pub failure: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserBusinessCalls1hPoint {
    pub bucket_start: i64,
    pub display_bucket_start: Option<i64>,
    pub bars: AdminUserBusinessCalls1hBarsPoint,
    pub pressure: Option<i64>,
    pub limit_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserUsageSeries {
    pub limit: i64,
    pub points: Vec<AdminUserUsageSeriesPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserBusinessCalls1hSeries {
    pub limit: i64,
    pub points: Vec<AdminUserBusinessCalls1hPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDashboardOverviewSeriesPoint {
    pub bucket_start: i64,
    pub display_bucket_start: Option<i64>,
    pub value: Option<i64>,
    pub limit_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDashboardProgressCard {
    pub used: i64,
    pub limit: i64,
    pub points: Vec<UserDashboardOverviewSeriesPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDashboardOverviewProgress {
    pub request_rate: UserDashboardProgressCard,
    pub business_calls_1h: UserDashboardProgressCard,
    pub daily_credits: UserDashboardProgressCard,
    pub monthly_credits: UserDashboardProgressCard,
}

#[derive(Debug, Clone)]
pub struct UserDashboardOverviewSnapshot {
    pub summary: UserDashboardSummary,
    pub progress: UserDashboardOverviewProgress,
}
