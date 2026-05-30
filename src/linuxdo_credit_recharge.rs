use chrono::{Datelike, Local, TimeZone, Utc};

pub const LINUXDO_CREDIT_RECHARGE_STATUS_PENDING: &str = "pending";
pub const LINUXDO_CREDIT_RECHARGE_STATUS_PAID: &str = "paid";
pub const LINUXDO_CREDIT_RECHARGE_STATUS_FAILED: &str = "failed";
pub const LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING: &str = "refunding";
pub const LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED: &str = "refunded";
pub const LINUXDO_CREDIT_RECHARGE_STATUS_REFUND_ONLY: &str = "refundOnly";
pub const LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS: i64 = 1000;
pub const LINUXDO_CREDIT_RECHARGE_UNIT_PRICE_CENTS: i64 = 5_000;
pub const LINUXDO_CREDIT_RECHARGE_MIN_MONTHS: i64 = 1;
pub const LINUXDO_CREDIT_RECHARGE_MAX_MONTHS: i64 = 12;
pub const LINUXDO_CREDIT_RECHARGE_MIN_CREDITS: i64 = 1000;
pub const LINUXDO_CREDIT_RECHARGE_MAX_CREDITS: i64 = 20_000;
pub const LINUXDO_CREDIT_RECHARGE_DEFAULT_CREDITS: i64 = 1000;
pub const LINUXDO_CREDIT_RECHARGE_TEST_CREDITS: i64 = 1;
pub const LINUXDO_CREDIT_RECHARGE_TEST_MONTHS: i64 = 1;
pub const LINUXDO_CREDIT_RECHARGE_TEST_PRICE_CENTS: i64 = 100;
pub const LINUXDO_CREDIT_RECHARGE_HOURLY_PER_1000_CREDITS: i64 = 20;
pub const LINUXDO_CREDIT_RECHARGE_DAILY_PER_1000_CREDITS: i64 = 100;

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeOrder {
    pub out_trade_no: String,
    pub user_id: String,
    pub status: String,
    pub credits: i64,
    pub months: i64,
    pub money_cents: i64,
    pub trade_no: Option<String>,
    pub payment_url: Option<String>,
    pub order_name: String,
    pub notify_payload: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub paid_at: Option<i64>,
    pub refunded_at: Option<i64>,
    pub refund_actor: Option<String>,
    pub refund_payload: Option<String>,
    pub last_notify_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeAdminOrder {
    pub order: LinuxDoCreditRechargeOrder,
    pub user_display_name: Option<String>,
    pub user_username: Option<String>,
    pub user_avatar_template: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeAdminUserGroup {
    pub user_id: String,
    pub user_display_name: Option<String>,
    pub user_username: Option<String>,
    pub user_avatar_template: Option<String>,
    pub order_count: i64,
    pub paid_order_count: i64,
    pub refunded_order_count: i64,
    pub total_credits: i64,
    pub total_money_cents: i64,
    pub latest_order_created_at: i64,
    pub latest_paid_at: Option<i64>,
    pub latest_refunded_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeAdminListQuery {
    pub user_query: Option<String>,
    pub status: Option<String>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub sort: String,
    pub order: String,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeEntitlement {
    pub id: i64,
    pub out_trade_no: String,
    pub user_id: String,
    pub month_start: i64,
    pub credits: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct LinuxDoCreditRechargeSummary {
    pub current_month_start: i64,
    pub current_month_entitlement_credits: i64,
    pub effective_until_month_start: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LinuxDoCreditRechargeAdminAudit {
    pub current_month_entitlement_credits: i64,
    pub effective_until_month_start: Option<i64>,
    pub orders: Vec<LinuxDoCreditRechargeOrder>,
    pub entitlements: Vec<LinuxDoCreditRechargeEntitlement>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxDoCreditRechargeQuotaDelta {
    pub hourly_delta: i64,
    pub daily_delta: i64,
    pub monthly_delta: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct LinuxDoCreditRechargePriceConfig {
    pub unit_credits: i64,
    pub unit_price_cents: i64,
    pub min_credits: i64,
    pub max_credits: i64,
    pub credits_step: i64,
    pub min_months: i64,
    pub max_months: i64,
    pub default_credits: i64,
    pub test_price_enabled: bool,
}

impl LinuxDoCreditRechargePriceConfig {
    pub fn normal() -> Self {
        Self {
            unit_credits: LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
            unit_price_cents: LINUXDO_CREDIT_RECHARGE_UNIT_PRICE_CENTS,
            min_credits: LINUXDO_CREDIT_RECHARGE_MIN_CREDITS,
            max_credits: LINUXDO_CREDIT_RECHARGE_MAX_CREDITS,
            credits_step: LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
            min_months: LINUXDO_CREDIT_RECHARGE_MIN_MONTHS,
            max_months: LINUXDO_CREDIT_RECHARGE_MAX_MONTHS,
            default_credits: LINUXDO_CREDIT_RECHARGE_DEFAULT_CREDITS,
            test_price_enabled: false,
        }
    }

    pub fn test_price() -> Self {
        Self {
            unit_credits: LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
            unit_price_cents: LINUXDO_CREDIT_RECHARGE_UNIT_PRICE_CENTS,
            min_credits: LINUXDO_CREDIT_RECHARGE_MIN_CREDITS,
            max_credits: LINUXDO_CREDIT_RECHARGE_MAX_CREDITS,
            credits_step: LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
            min_months: LINUXDO_CREDIT_RECHARGE_MIN_MONTHS,
            max_months: LINUXDO_CREDIT_RECHARGE_MAX_MONTHS,
            default_credits: LINUXDO_CREDIT_RECHARGE_TEST_CREDITS,
            test_price_enabled: true,
        }
    }
}

pub fn linuxdo_credit_recharge_quota_delta(credits: i64) -> LinuxDoCreditRechargeQuotaDelta {
    let credits = credits.max(0);
    let hourly_numerator = credits.saturating_mul(LINUXDO_CREDIT_RECHARGE_HOURLY_PER_1000_CREDITS);
    let daily_numerator = credits.saturating_mul(LINUXDO_CREDIT_RECHARGE_DAILY_PER_1000_CREDITS);
    LinuxDoCreditRechargeQuotaDelta {
        hourly_delta: hourly_numerator.saturating_add(LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS - 1)
            / LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
        daily_delta: daily_numerator.saturating_add(LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS - 1)
            / LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS,
        monthly_delta: credits,
    }
}

pub fn linuxdo_credit_recharge_money_cents(
    credits: i64,
    months: i64,
    price: LinuxDoCreditRechargePriceConfig,
) -> Option<i64> {
    if price.test_price_enabled
        && credits == LINUXDO_CREDIT_RECHARGE_TEST_CREDITS
        && months == LINUXDO_CREDIT_RECHARGE_TEST_MONTHS
    {
        return Some(LINUXDO_CREDIT_RECHARGE_TEST_PRICE_CENTS);
    }
    if credits <= 0
        || credits < price.min_credits
        || credits > price.max_credits
        || months < price.min_months
        || months > price.max_months
        || credits % price.credits_step != 0
    {
        return None;
    }
    let units = credits.checked_div(price.unit_credits)?;
    units
        .checked_mul(months)?
        .checked_mul(price.unit_price_cents)
}

pub fn format_linuxdo_credit_money(money_cents: i64) -> String {
    let cents = money_cents.max(0);
    format!("{}.{:02}", cents / 100, cents % 100)
}

pub(crate) fn shift_local_month_start_utc_ts(
    current_month_start_utc_ts: i64,
    delta_months: i32,
) -> i64 {
    let Some(current_utc) = Utc.timestamp_opt(current_month_start_utc_ts, 0).single() else {
        return current_month_start_utc_ts;
    };
    let current_local = current_utc.with_timezone(&Local);
    let zero_indexed = current_local.month0() as i32 + delta_months;
    let year = current_local.year() + zero_indexed.div_euclid(12);
    let month0 = zero_indexed.rem_euclid(12) as u32;
    let Some(date) = chrono::NaiveDate::from_ymd_opt(year, month0 + 1, 1) else {
        return current_month_start_utc_ts;
    };
    crate::local_date_start_utc_ts(date, current_local)
}
