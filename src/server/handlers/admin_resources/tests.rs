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
            recharge: tavily_hikari::LinuxDoCreditRechargeSummary::default(),
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
            recent_ip_count_7d: 0,
        }
    }

    fn admin_test_db_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.db", nanoid!(8)))
    }

    fn admin_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forward-user", HeaderValue::from_static("admin"));
        headers
    }

    async fn totp_test_state(prefix: &str) -> (Arc<AppState>, PathBuf) {
        let db_path = admin_test_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), tavily_hikari::DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let mut settings = proxy.get_system_settings().await.expect("settings");
        settings.recharge_feature_enabled = true;
        proxy
            .set_system_settings(&settings)
            .await
            .expect("enable recharge feature");
        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth,
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions {
                enabled: true,
                client_id: Some("linuxdo-test-client-id".to_string()),
                client_secret: Some("linuxdo-test-client-secret".to_string()),
                authorize_url: "https://connect.linux.do/oauth2/authorize".to_string(),
                token_url: "https://connect.linux.do/oauth2/token".to_string(),
                userinfo_url: "https://connect.linux.do/api/user".to_string(),
                scope: "user".to_string(),
                redirect_url: Some("http://127.0.0.1/auth/linuxdo/callback".to_string()),
                refresh_token_crypt_key: Some(*b"0123456789abcdef0123456789abcdef"),
                user_sync_enabled: true,
                user_sync_at: (6, 20),
                session_max_age_secs: 3600,
                login_state_ttl_secs: 600,
            },
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });
        (state, db_path)
    }

    #[test]
    fn linuxdo_credit_refund_url_refuses_unknown_submit_url() {
        assert_eq!(
            linuxdo_credit_refund_url("https://credit.linux.do/epay/pay/submit.php")
                .expect("official URL derives"),
            "https://credit.linux.do/epay/api.php"
        );
        let err = linuxdo_credit_refund_url("http://127.0.0.1:9/linuxdo-credit/submit")
            .expect_err("unknown sandbox URL is refused");
        assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn linuxdo_credit_refund_params_select_refund_action() {
        let params = linuxdo_credit_refund_params(
            "client-id",
            "client-secret",
            "trade-123",
            "out-trade-123",
            "50.00",
        );
        assert_eq!(params[0], ("act", "refund".to_string()));
        assert!(params.contains(&("pid", "client-id".to_string())));
        assert!(params.contains(&("key", "client-secret".to_string())));
        assert!(params.contains(&("trade_no", "trade-123".to_string())));
        assert!(params.contains(&("out_trade_no", "out-trade-123".to_string())));
        assert!(params.contains(&("money", "50.00".to_string())));
    }

    #[tokio::test]
    async fn admin_totp_confirm_rejects_existing_binding() {
        let (state, db_path) = totp_test_state("admin-totp-confirm-existing").await;
        let first_secret = generate_totp_secret();
        let first_code = build_totp(&first_secret)
            .expect("build first totp")
            .generate_current()
            .expect("first code");
        let _ = post_admin_totp_confirm(
            State(state.clone()),
            admin_headers(),
            Json(AdminTotpConfirmPayload {
                secret: first_secret,
                code: first_code,
            }),
        )
        .await
        .expect("first bind succeeds");

        let next_secret = generate_totp_secret();
        let next_code = build_totp(&next_secret)
            .expect("build next totp")
            .generate_current()
            .expect("next code");
        let err = post_admin_totp_confirm(
            State(state),
            admin_headers(),
            Json(AdminTotpConfirmPayload {
                secret: next_secret,
                code: next_code,
            }),
        )
        .await
        .expect_err("confirm cannot overwrite existing binding");
        assert_eq!(err.0, StatusCode::CONFLICT);

        let _ = std::fs::remove_file(db_path);
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
