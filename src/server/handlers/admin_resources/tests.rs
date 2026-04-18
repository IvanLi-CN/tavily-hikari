#[cfg(test)]
mod admin_resources_tests {
    use super::*;

    fn mock_user(user_id: &str, last_login_at: Option<i64>) -> tavily_hikari::AdminUserIdentity {
        tavily_hikari::AdminUserIdentity {
            user_id: user_id.to_string(),
            display_name: Some(user_id.to_string()),
            username: Some(user_id.to_string()),
            active: true,
            last_login_at,
            token_count: 1,
        }
    }

    fn mock_summary() -> tavily_hikari::UserDashboardSummary {
        tavily_hikari::UserDashboardSummary {
            request_rate: default_request_rate_view(tavily_hikari::RequestRateScope::User),
            hourly_any_used: 0,
            hourly_any_limit: 0,
            quota_hourly_used: 0,
            quota_hourly_limit: 0,
            quota_daily_used: 0,
            quota_daily_limit: 0,
            quota_monthly_used: 0,
            quota_monthly_limit: 0,
            daily_success: 0,
            daily_failure: 0,
            monthly_success: 0,
            monthly_failure: 0,
            last_activity: None,
        }
    }

    fn mock_row(
        user_id: &str,
        last_login_at: Option<i64>,
        configure: impl FnOnce(&mut tavily_hikari::UserDashboardSummary),
    ) -> AdminUserSummaryRow {
        let mut summary = mock_summary();
        configure(&mut summary);
        AdminUserSummaryRow {
            user: mock_user(user_id, last_login_at),
            summary,
            monthly_broken_count: 0,
            monthly_broken_limit: USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
        }
    }

    #[test]
    fn build_forward_proxy_validation_view_preserves_readable_display_name() {
        let view = build_forward_proxy_validation_view(tavily_hikari::ForwardProxyValidationResponse {
            ok: true,
            normalized_values: vec![
                "vless://user@example.com:443?encryption=none#%E9%A6%99%E6%B8%AF%20%F0%9F%87%AD%F0%9F%87%B0"
                    .to_string(),
            ],
            discovered_nodes: 1,
            latency_ms: Some(42.0),
            results: vec![tavily_hikari::ForwardProxyValidationProbeResult {
                value: "subscription".to_string(),
                normalized_value: Some(
                    "vless://user@example.com:443?encryption=none#%E9%A6%99%E6%B8%AF%20%F0%9F%87%AD%F0%9F%87%B0"
                        .to_string(),
                ),
                ok: true,
                discovered_nodes: Some(1),
                latency_ms: Some(42.0),
                error_code: None,
                message: "subscription validation succeeded".to_string(),
                nodes: vec![tavily_hikari::ForwardProxyValidationNodeResult {
                    display_name: "香港 🇭🇰".to_string(),
                    protocol: "vless".to_string(),
                    ok: true,
                    latency_ms: Some(42.0),
                    ip: Some("203.0.113.8".to_string()),
                    location: Some("HK / HKG".to_string()),
                    message: None,
                }],
            }],
            first_error: None,
        });

        let payload = serde_json::to_value(&view).expect("serialize view");
        assert_eq!(payload["nodes"][0]["displayName"].as_str(), Some("香港 🇭🇰"));
    }

    #[test]
    fn admin_user_rows_default_to_last_login_desc_with_nulls_last() {
        let mut rows = [
            mock_row("usr_none", None, |_| {}),
            mock_row("usr_old", Some(10), |_| {}),
            mock_row("usr_new", Some(20), |_| {}),
        ];

        rows.sort_by(|left, right| compare_admin_user_rows(left, right, None, None));

        let ordered_ids: Vec<&str> = rows.iter().map(|row| row.user.user_id.as_str()).collect();
        assert_eq!(ordered_ids, vec!["usr_new", "usr_old", "usr_none"]);
    }

    #[test]
    fn success_rate_sort_keeps_zero_sample_rows_last() {
        let mut rows = [
            mock_row("usr_zero", Some(10), |summary| {
                summary.daily_success = 0;
                summary.daily_failure = 0;
            }),
            mock_row("usr_mid", Some(11), |summary| {
                summary.daily_success = 6;
                summary.daily_failure = 2;
            }),
            mock_row("usr_best", Some(12), |summary| {
                summary.daily_success = 9;
                summary.daily_failure = 1;
            }),
        ];

        rows.sort_by(|left, right| {
            compare_admin_user_rows(
                left,
                right,
                Some(AdminUsersSortField::DailySuccessRate),
                Some(AdminUsersSortDirection::Desc),
            )
        });

        let ordered_ids: Vec<&str> = rows.iter().map(|row| row.user.user_id.as_str()).collect();
        assert_eq!(ordered_ids, vec!["usr_best", "usr_mid", "usr_zero"]);
    }

    #[test]
    fn success_rate_sort_uses_failure_count_as_ascending_tiebreaker() {
        let mut rows = [
            mock_row("usr_many_failures", Some(10), |summary| {
                summary.daily_success = 9;
                summary.daily_failure = 9;
            }),
            mock_row("usr_few_failures", Some(11), |summary| {
                summary.daily_success = 1;
                summary.daily_failure = 1;
            }),
        ];

        rows.sort_by(|left, right| {
            compare_admin_user_rows(
                left,
                right,
                Some(AdminUsersSortField::DailySuccessRate),
                Some(AdminUsersSortDirection::Desc),
            )
        });

        let ordered_ids: Vec<&str> = rows.iter().map(|row| row.user.user_id.as_str()).collect();
        assert_eq!(ordered_ids, vec!["usr_few_failures", "usr_many_failures"]);
    }

    #[test]
    fn quota_sort_uses_limit_as_secondary_tiebreaker() {
        let mut rows = [
            mock_row("usr_b", Some(10), |summary| {
                summary.quota_hourly_used = 40;
                summary.quota_hourly_limit = 200;
            }),
            mock_row("usr_a", Some(12), |summary| {
                summary.quota_hourly_used = 40;
                summary.quota_hourly_limit = 100;
            }),
        ];

        rows.sort_by(|left, right| {
            compare_admin_user_rows(
                left,
                right,
                Some(AdminUsersSortField::QuotaHourlyUsed),
                Some(AdminUsersSortDirection::Asc),
            )
        });

        let ordered_ids: Vec<&str> = rows.iter().map(|row| row.user.user_id.as_str()).collect();
        assert_eq!(ordered_ids, vec!["usr_a", "usr_b"]);
    }
}
