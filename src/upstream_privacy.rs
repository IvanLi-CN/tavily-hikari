use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Local, TimeZone, Timelike};
use ring::hmac;
use serde::{Deserialize, Serialize};

pub const UPSTREAM_PROJECT_ID_FIXED_MAX_BYTES: usize = 128;
pub const UPSTREAM_MCP_USER_AGENT_MAX_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UpstreamProjectIdMode {
    Passthrough,
    Fixed,
    #[default]
    AccessToken,
}

impl UpstreamProjectIdMode {
    pub(crate) const fn as_meta_value(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::Fixed => "fixed",
            Self::AccessToken => "accessToken",
        }
    }

    pub(crate) fn from_meta_value(value: &str) -> Option<Self> {
        match value {
            "passthrough" => Some(Self::Passthrough),
            "fixed" => Some(Self::Fixed),
            "accessToken" => Some(Self::AccessToken),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessPeriod {
    pub code: String,
    pub segment: &'static str,
    pub starts_at: i64,
    pub ends_at: i64,
}

pub fn business_period_for_timestamp(timestamp: i64) -> BusinessPeriod {
    let local = Local
        .timestamp_opt(timestamp, 0)
        .single()
        .unwrap_or_else(Local::now);
    business_period_for_local(local)
}

pub fn business_period_for_local(local: DateTime<Local>) -> BusinessPeriod {
    let (segment, start_hour, end_hour) = match local.hour() {
        0..=10 => ("S1", 0, 11),
        11..=21 => ("S2", 11, 22),
        _ => ("S3", 22, 24),
    };
    let date = local.date_naive();
    let start = Local
        .from_local_datetime(
            &date
                .and_hms_opt(start_hour, 0, 0)
                .expect("valid period hour"),
        )
        .earliest()
        .unwrap_or(local);
    let end_date = if end_hour == 24 {
        date.succ_opt().unwrap_or(date)
    } else {
        date
    };
    let normalized_end_hour = if end_hour == 24 { 0 } else { end_hour };
    let end = Local
        .from_local_datetime(
            &end_date
                .and_hms_opt(normalized_end_hour, 0, 0)
                .expect("valid period hour"),
        )
        .earliest()
        .unwrap_or(local);
    BusinessPeriod {
        code: format!("{date}/{segment}"),
        segment,
        starts_at: start.timestamp(),
        ends_at: end.timestamp(),
    }
}

pub fn derive_access_token_project_id(secret: &[u8], token_id: &str, period_code: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let input = format!("v1{token_id}{period_code}");
    URL_SAFE_NO_PAD.encode(hmac::sign(&key, input.as_bytes()).as_ref())
}

pub fn research_response_is_terminal(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let status = value
        .get("status")
        .or_else(|| value.get("state"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        status.as_str(),
        "completed" | "complete" | "failed" | "cancelled" | "canceled" | "error"
    ) || value.get("results").is_some()
        || value.get("answer").is_some()
        || value.get("content").is_some()
}

pub(crate) fn validate_upstream_header_setting(
    field: &str,
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if !allow_empty && value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field} must not exceed {max_bytes} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::LocalResult;

    fn local_time(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Local> {
        match Local.with_ymd_and_hms(year, month, day, hour, 0, 0) {
            LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => value,
            LocalResult::None => panic!("local time is unavailable"),
        }
    }

    #[test]
    fn business_period_uses_three_server_local_segments() {
        assert_eq!(
            business_period_for_local(local_time(2026, 7, 14, 0)).code,
            "2026-07-14/S1"
        );
        assert_eq!(
            business_period_for_local(local_time(2026, 7, 14, 10)).segment,
            "S1"
        );
        assert_eq!(
            business_period_for_local(local_time(2026, 7, 14, 11)).segment,
            "S2"
        );
        assert_eq!(
            business_period_for_local(local_time(2026, 7, 14, 22)).segment,
            "S3"
        );
    }

    #[test]
    fn access_token_identity_is_stable_and_period_scoped() {
        let secret = [7_u8; 32];
        let first = derive_access_token_project_id(&secret, "token-a", "2026-07-14/S1");
        assert_eq!(
            first,
            derive_access_token_project_id(&secret, "token-a", "2026-07-14/S1")
        );
        assert_ne!(
            first,
            derive_access_token_project_id(&secret, "token-b", "2026-07-14/S1")
        );
        assert_ne!(
            first,
            derive_access_token_project_id(&secret, "token-a", "2026-07-14/S2")
        );
        assert_eq!(first.len(), 43);
    }
}
