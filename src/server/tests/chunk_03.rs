    fn find_cookie_pair(headers: &reqwest::header::HeaderMap, cookie_name: &str) -> Option<String> {
        headers
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .map(str::trim)
            .find(|pair| {
                pair.split_once('=')
                    .is_some_and(|(name, _)| name == cookie_name)
            })
            .map(str::to_string)
    }

    async fn fetch_linuxdo_oauth_account_snapshot(
        pool: &sqlx::SqlitePool,
        provider_user_id: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    ) {
        sqlx::query_as(
            r#"SELECT
                    refresh_token_ciphertext,
                    refresh_token_nonce,
                    trust_level,
                    last_profile_sync_attempt_at,
                    last_profile_sync_success_at,
                    last_profile_sync_error
               FROM oauth_accounts
               WHERE provider = 'linuxdo' AND provider_user_id = ?
               LIMIT 1"#,
        )
        .bind(provider_user_id)
        .fetch_one(pool)
        .await
        .expect("fetch linuxdo oauth account snapshot")
    }

    async fn latest_scheduled_job(pool: &sqlx::SqlitePool) -> (String, String, Option<String>) {
        sqlx::query_as(
            r#"SELECT job_type, status, message
               FROM scheduled_jobs
               ORDER BY id DESC
               LIMIT 1"#,
        )
        .fetch_one(pool)
        .await
        .expect("fetch latest scheduled job")
    }

    async fn fetch_linuxdo_system_tag_keys(pool: &sqlx::SqlitePool, user_id: &str) -> Vec<String> {
        sqlx::query_scalar(
            r#"SELECT ut.system_key
               FROM user_tag_bindings ub
               JOIN user_tags ut ON ut.id = ub.tag_id
               WHERE ub.user_id = ?
                 AND ut.system_key LIKE 'linuxdo_l%'
               ORDER BY ut.system_key ASC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .expect("fetch linuxdo system tag keys")
    }

    async fn login_builtin_admin_cookie(
        admin_addr: SocketAddr,
        password: &str,
    ) -> (Client, String) {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        (client, admin_cookie)
    }

    #[test]
    fn linuxdo_refresh_token_crypto_round_trip() {
        let cfg = linuxdo_oauth_options_for_test();
        let refresh_token = "linuxdo-refresh-token-round-trip";

        let (ciphertext, nonce) = encrypt_linuxdo_refresh_token(&cfg, refresh_token)
            .expect("encrypt refresh token")
            .expect("encrypted payload");
        assert!(!ciphertext.is_empty());
        assert!(!nonce.is_empty());

        let decrypted = decrypt_linuxdo_refresh_token(&cfg, &ciphertext, &nonce)
            .expect("decrypt refresh token");
        assert_eq!(decrypted, refresh_token);
    }

    #[test]
    fn linuxdo_user_sync_scheduler_uses_next_future_local_window() {
        use chrono::Timelike as _;

        let today = Local::now().date_naive();
        let before_naive = today
            .and_hms_opt(5, 0, 0)
            .expect("valid local test time before");
        let before_now = match Local.from_local_datetime(&before_naive) {
            chrono::LocalResult::Single(dt) => dt,
            chrono::LocalResult::Ambiguous(dt, _) => dt,
            chrono::LocalResult::None => Local::now(),
        };
        let before_next = next_local_daily_run_after(before_now, 6, 20);
        assert_eq!(before_next.date_naive(), today);
        assert_eq!(before_next.hour(), 6);
        assert_eq!(before_next.minute(), 20);
        assert!(duration_until_next_local_daily_run(before_now, 6, 20) > Duration::from_secs(0));

        let after_naive = today
            .and_hms_opt(7, 0, 0)
            .expect("valid local test time after");
        let after_now = match Local.from_local_datetime(&after_naive) {
            chrono::LocalResult::Single(dt) => dt,
            chrono::LocalResult::Ambiguous(dt, _) => dt,
            chrono::LocalResult::None => Local::now(),
        };
        let expected_tomorrow = today.succ_opt().unwrap_or_else(|| {
            today
                .checked_add_days(chrono::Days::new(1))
                .expect("next day")
        });
        let after_next = next_local_daily_run_after(after_now, 6, 20);
        assert_eq!(after_next.date_naive(), expected_tomorrow);
        assert_eq!(after_next.hour(), 6);
        assert_eq!(after_next.minute(), 20);
    }

    #[tokio::test]
    async fn linuxdo_callback_persists_refresh_token_and_sync_metadata() {
        let db_path = temp_db_path("linuxdo-callback-persists-refresh-token");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "callback-access-token".to_string(),
                authorization_refresh_token: Some("callback-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-callback-user",
                    "username": "linuxdo_callback_user",
                    "name": "LinuxDO Callback User",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_access_token: "unused-refresh-access-token".to_string(),
                refresh_refresh_token: Some("unused-rotated-refresh-token".to_string()),
                refresh_profile: json!({
                    "id": "linuxdo-callback-user",
                    "username": "linuxdo_callback_user",
                    "name": "LinuxDO Callback User",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options.clone()).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{addr}/auth/linuxdo"))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);
        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{addr}/auth/linuxdo/callback?code=test-code&state={state}"
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let (ciphertext, nonce, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-callback-user").await;
        assert_eq!(trust_level, Some(2));
        assert!(attempted_at.is_some());
        assert!(success_at.is_some());
        assert_eq!(sync_error, None);
        let decrypted = decrypt_linuxdo_refresh_token(
            &oauth_options,
            ciphertext.as_deref().expect("stored ciphertext"),
            nonce.as_deref().expect("stored nonce"),
        )
        .expect("decrypt stored refresh token");
        assert_eq!(decrypted, "callback-refresh-token");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_rejects_inactive_users_before_creating_session() {
        let db_path = temp_db_path("linuxdo-callback-inactive-user");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "callback-access-token".to_string(),
                authorization_refresh_token: Some("callback-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-inactive-callback-user",
                    "username": "linuxdo_inactive_callback_user",
                    "name": "LinuxDO Inactive Callback User",
                    "active": false,
                    "trust_level": 2
                }),
                refresh_access_token: "unused-refresh-access-token".to_string(),
                refresh_refresh_token: Some("unused-rotated-refresh-token".to_string()),
                refresh_profile: json!({
                    "id": "linuxdo-inactive-callback-user",
                    "username": "linuxdo_inactive_callback_user",
                    "name": "LinuxDO Inactive Callback User",
                    "active": false,
                    "trust_level": 2
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options.clone()).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{addr}/auth/linuxdo"))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);
        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{addr}/auth/linuxdo/callback?code=test-code&state={state}"
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(callback_resp.status(), reqwest::StatusCode::FORBIDDEN);
        assert!(
            callback_resp
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .any(|value| value.starts_with(&format!("{OAUTH_LOGIN_BINDING_COOKIE_NAME}="))),
            "inactive callbacks should still clear the OAuth binding cookie"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let (_, _, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-inactive-callback-user").await;
        assert_eq!(trust_level, Some(2));
        assert!(attempted_at.is_some());
        assert!(success_at.is_some());
        assert_eq!(sync_error, None);
        let user_id: String = sqlx::query_scalar(
            "SELECT user_id FROM oauth_accounts WHERE provider = 'linuxdo' AND provider_user_id = ? LIMIT 1",
        )
        .bind("linuxdo-inactive-callback-user")
        .fetch_one(&pool)
        .await
        .expect("fetch inactive callback user id");
        assert_eq!(
            fetch_user_active(&pool, &user_id).await,
            0,
            "seeded inactive LinuxDo profile should remain inactive locally"
        );
        let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_sessions")
            .fetch_one(&pool)
            .await
            .expect("count user sessions");
        assert_eq!(session_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_records_sync_failure_when_refresh_token_persistence_fails() {
        let db_path = temp_db_path("linuxdo-callback-refresh-token-persist-error");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;
        install_refresh_token_write_failure_trigger(&pool).await;

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "callback-access-token".to_string(),
                authorization_refresh_token: Some("callback-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-callback-error-user",
                    "username": "linuxdo_callback_error_user",
                    "name": "LinuxDO Callback Error User",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_access_token: "unused-refresh-access-token".to_string(),
                refresh_refresh_token: Some("unused-rotated-refresh-token".to_string()),
                refresh_profile: json!({
                    "id": "linuxdo-callback-error-user",
                    "username": "linuxdo_callback_error_user",
                    "name": "LinuxDO Callback Error User",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{addr}/auth/linuxdo"))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);
        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{addr}/auth/linuxdo/callback?code=test-code&state={state}"
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );

        let (ciphertext, nonce, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-callback-error-user").await;
        assert_eq!(ciphertext, None);
        assert_eq!(nonce, None);
        assert_eq!(trust_level, Some(2));
        assert!(attempted_at.is_some());
        assert_eq!(success_at, None);
        let sync_error = sync_error.expect("sync error recorded");
        assert!(sync_error.contains("refresh-token storage error"));
        assert!(sync_error.contains("refresh token persistence failed"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_user_sync_job_refreshes_trust_level_and_keeps_old_refresh_token_when_provider_does_not_rotate_it()
     {
        let db_path = temp_db_path("linuxdo-user-sync-success");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-sync-user".to_string(),
                username: Some("linuxdo_sync".to_string()),
                name: Some("LinuxDO Sync".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let (ciphertext, nonce) =
            encrypt_linuxdo_refresh_token(&linuxdo_oauth_options_for_test(), "seed-refresh-token")
                .expect("encrypt refresh token")
                .expect("encrypted refresh token");
        proxy
            .set_oauth_account_refresh_token("linuxdo", "linuxdo-sync-user", &ciphertext, &nonce)
            .await
            .expect("persist refresh token");
        let pool = connect_sqlite_test_pool(&db_str).await;
        let original_last_login_at = 1_700_000_123i64;
        sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?")
            .bind(original_last_login_at)
            .bind(&user.user_id)
            .execute(&pool)
            .await
            .expect("set fixed last_login_at");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "unused-auth-access-token".to_string(),
                authorization_refresh_token: Some("unused-auth-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-sync-user",
                    "username": "linuxdo_sync",
                    "name": "LinuxDO Sync",
                    "active": true,
                    "trust_level": 1
                }),
                refresh_access_token: "sync-refresh-access-token".to_string(),
                refresh_refresh_token: None,
                refresh_profile: json!({
                    "id": "linuxdo-sync-user",
                    "username": "linuxdo_sync",
                    "name": "LinuxDO Sync Updated",
                    "active": true,
                    "trust_level": 4
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let state = Arc::new(AppState {
            proxy: proxy.clone(),
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        run_linuxdo_user_status_sync_job(state.clone()).await;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let (new_ciphertext, new_nonce, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-sync-user").await;
        assert_eq!(trust_level, Some(4));
        assert_eq!(new_ciphertext.as_deref(), Some(ciphertext.as_str()));
        assert_eq!(new_nonce.as_deref(), Some(nonce.as_str()));
        assert_eq!(
            fetch_user_last_login_at(&pool, &user.user_id).await,
            Some(original_last_login_at)
        );
        assert!(attempted_at.is_some());
        assert!(success_at.is_some());
        assert_eq!(sync_error, None);
        assert_eq!(
            fetch_linuxdo_system_tag_keys(&pool, &user.user_id).await,
            vec!["linuxdo_l4".to_string()]
        );
        let latest_job = latest_scheduled_job(&pool).await;
        assert_eq!(latest_job.0, LINUXDO_USER_STATUS_SYNC_JOB_TYPE);
        assert_eq!(latest_job.1, "success");
        let latest_job_message = latest_job.2.expect("scheduled job message");
        assert!(latest_job_message.contains("attempted=1"));
        assert!(latest_job_message.contains("success=1"));
        assert!(latest_job_message.contains("failure=0"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_user_sync_job_rejects_existing_browser_sessions_for_deactivated_users() {
        let db_path = temp_db_path("linuxdo-user-sync-deactivate-session");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-deactivated-user".to_string(),
                username: Some("linuxdo_deactivated".to_string()),
                name: Some("LinuxDO Deactivated".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");
        let bound_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_deactivated"))
            .await
            .expect("create bound access token");
        assert!(
            proxy
                .validate_access_token(&bound_token.token)
                .await
                .expect("validate active bound token"),
            "active LinuxDo users should keep their bound API token before sync deactivation"
        );
        let (ciphertext, nonce) =
            encrypt_linuxdo_refresh_token(&linuxdo_oauth_options_for_test(), "seed-refresh-token")
                .expect("encrypt refresh token")
                .expect("encrypted refresh token");
        proxy
            .set_oauth_account_refresh_token(
                "linuxdo",
                "linuxdo-deactivated-user",
                &ciphertext,
                &nonce,
            )
            .await
            .expect("persist refresh token");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "unused-auth-access-token".to_string(),
                authorization_refresh_token: Some("unused-auth-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-deactivated-user",
                    "username": "linuxdo_deactivated",
                    "name": "LinuxDO Deactivated",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_access_token: "sync-refresh-access-token".to_string(),
                refresh_refresh_token: None,
                refresh_profile: json!({
                    "id": "linuxdo-deactivated-user",
                    "username": "linuxdo_deactivated",
                    "name": "LinuxDO Deactivated",
                    "active": false,
                    "trust_level": 2
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let state = Arc::new(AppState {
            proxy: proxy.clone(),
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        run_linuxdo_user_status_sync_job(state.clone()).await;

        let pool = connect_sqlite_test_pool(&db_str).await;
        assert_eq!(fetch_user_active(&pool, &user.user_id).await, 0);
        assert!(
            proxy
                .get_user_session(&session.token)
                .await
                .expect("lookup user session")
                .is_none(),
            "inactive LinuxDo users should no longer resolve an authenticated browser session"
        );
        assert!(
            !proxy
                .validate_access_token(&bound_token.token)
                .await
                .expect("validate inactive bound token"),
            "inactive LinuxDo users should no longer authenticate with previously bound API tokens"
        );
        let latest_job = latest_scheduled_job(&pool).await;
        assert_eq!(latest_job.0, LINUXDO_USER_STATUS_SYNC_JOB_TYPE);
        assert_eq!(latest_job.1, "success");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_user_sync_job_records_invalid_grant_and_preserves_existing_trust_level() {
        let db_path = temp_db_path("linuxdo-user-sync-invalid-grant");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-invalid-grant-user".to_string(),
                username: Some("linuxdo_invalid".to_string()),
                name: Some("LinuxDO Invalid".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let (ciphertext, nonce) = encrypt_linuxdo_refresh_token(
            &linuxdo_oauth_options_for_test(),
            "invalid-grant-refresh-token",
        )
        .expect("encrypt refresh token")
        .expect("encrypted refresh token");
        proxy
            .set_oauth_account_refresh_token(
                "linuxdo",
                "linuxdo-invalid-grant-user",
                &ciphertext,
                &nonce,
            )
            .await
            .expect("persist refresh token");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "unused-auth-access-token".to_string(),
                authorization_refresh_token: Some("unused-auth-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-invalid-grant-user",
                    "username": "linuxdo_invalid",
                    "name": "LinuxDO Invalid",
                    "active": true,
                    "trust_level": 2
                }),
                refresh_access_token: "unused-refresh-access-token".to_string(),
                refresh_refresh_token: None,
                refresh_profile: json!({
                    "id": "linuxdo-invalid-grant-user",
                    "username": "linuxdo_invalid",
                    "name": "LinuxDO Invalid Updated",
                    "active": true,
                    "trust_level": 4
                }),
                refresh_error: Some((StatusCode::BAD_REQUEST, json!({ "error": "invalid_grant" }))),
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let state = Arc::new(AppState {
            proxy: proxy.clone(),
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        run_linuxdo_user_status_sync_job(state.clone()).await;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let (_, _, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-invalid-grant-user").await;
        assert_eq!(trust_level, Some(2));
        assert!(attempted_at.is_some());
        assert_eq!(success_at, None);
        let sync_error = sync_error.expect("sync error recorded");
        assert!(sync_error.contains("invalid_grant"));
        assert_eq!(
            fetch_linuxdo_system_tag_keys(&pool, &user.user_id).await,
            vec!["linuxdo_l2".to_string()]
        );
        let latest_job = latest_scheduled_job(&pool).await;
        assert_eq!(latest_job.0, LINUXDO_USER_STATUS_SYNC_JOB_TYPE);
        assert_eq!(latest_job.1, "error");
        let latest_job_message = latest_job.2.expect("scheduled job message");
        assert!(latest_job_message.contains("attempted=1"));
        assert!(latest_job_message.contains("success=0"));
        assert!(latest_job_message.contains("failure=1"));
        assert!(latest_job_message.contains("first_failure="));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_user_sync_job_marks_run_error_when_rotated_refresh_token_persist_fails() {
        let db_path = temp_db_path("linuxdo-user-sync-rotated-refresh-token-persist-error");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-rotated-refresh-error-user".to_string(),
                username: Some("linuxdo_rotated_error".to_string()),
                name: Some("LinuxDO Rotated Error".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");
        let bound_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_rotated_error"))
            .await
            .expect("create bound access token");
        assert!(
            proxy
                .validate_access_token(&bound_token.token)
                .await
                .expect("validate active bound token"),
            "active LinuxDo users should keep their bound API token before sync deactivation"
        );
        let (ciphertext, nonce) =
            encrypt_linuxdo_refresh_token(&linuxdo_oauth_options_for_test(), "seed-refresh-token")
                .expect("encrypt refresh token")
                .expect("encrypted refresh token");
        proxy
            .set_oauth_account_refresh_token(
                "linuxdo",
                "linuxdo-rotated-refresh-error-user",
                &ciphertext,
                &nonce,
            )
            .await
            .expect("persist refresh token");

        let pool = connect_sqlite_test_pool(&db_str).await;
        install_refresh_token_write_failure_trigger(&pool).await;

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
                authorization_access_token: "unused-auth-access-token".to_string(),
                authorization_refresh_token: Some("unused-auth-refresh-token".to_string()),
                authorization_profile: json!({
                    "id": "linuxdo-rotated-refresh-error-user",
                    "username": "linuxdo_rotated_error",
                    "name": "LinuxDO Rotated Error",
                    "active": true,
                    "trust_level": 1
                }),
                refresh_access_token: "sync-refresh-access-token".to_string(),
                refresh_refresh_token: Some("rotated-refresh-token".to_string()),
                refresh_profile: json!({
                    "id": "linuxdo-rotated-refresh-error-user",
                    "username": "linuxdo_rotated_error",
                    "name": "LinuxDO Rotated Error Updated",
                    "active": false,
                    "trust_level": 4
                }),
                refresh_error: None,
            })
            .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let state = Arc::new(AppState {
            proxy: proxy.clone(),
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        run_linuxdo_user_status_sync_job(state.clone()).await;

        let (new_ciphertext, new_nonce, trust_level, attempted_at, success_at, sync_error) =
            fetch_linuxdo_oauth_account_snapshot(&pool, "linuxdo-rotated-refresh-error-user").await;
        assert_eq!(trust_level, Some(1));
        assert_eq!(new_ciphertext.as_deref(), Some(ciphertext.as_str()));
        assert_eq!(new_nonce.as_deref(), Some(nonce.as_str()));
        assert_eq!(fetch_user_active(&pool, &user.user_id).await, 0);
        assert!(
            proxy
                .get_user_session(&session.token)
                .await
                .expect("lookup user session")
                .is_none(),
            "failed rotated-token persistence should still deactivate sessions when LinuxDo marks the user inactive"
        );
        assert!(
            !proxy
                .validate_access_token(&bound_token.token)
                .await
                .expect("validate inactive bound token"),
            "failed rotated-token persistence should still deactivate bound API tokens when LinuxDo marks the user inactive"
        );
        assert!(attempted_at.is_some());
        assert_eq!(success_at, None);
        let sync_error = sync_error.expect("sync error recorded");
        assert!(sync_error.contains("upsert oauth account error"));
        assert!(sync_error.contains("refresh token persistence failed"));
        assert_eq!(
            fetch_linuxdo_system_tag_keys(&pool, &user.user_id).await,
            vec!["linuxdo_l1".to_string()]
        );
        let latest_job = latest_scheduled_job(&pool).await;
        assert_eq!(latest_job.0, LINUXDO_USER_STATUS_SYNC_JOB_TYPE);
        assert_eq!(latest_job.1, "error");
        let latest_job_message = latest_job.2.expect("scheduled job message");
        assert!(latest_job_message.contains("attempted=1"));
        assert!(latest_job_message.contains("success=0"));
        assert!(latest_job_message.contains("failure=1"));
        assert!(latest_job_message.contains("first_failure="));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_user_sync_job_noops_without_refresh_token_crypt_key() {
        let db_path = temp_db_path("linuxdo-user-sync-no-key");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.refresh_token_crypt_key = None;
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        run_linuxdo_user_status_sync_job(state.clone()).await;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let latest_job = latest_scheduled_job(&pool).await;
        assert_eq!(latest_job.0, LINUXDO_USER_STATUS_SYNC_JOB_TYPE);
        assert_eq!(latest_job.1, "success");
        assert_eq!(
            latest_job.2.as_deref(),
            Some(
                "attempted=0 success=0 skipped=0 failure=0 reason=missing_refresh_token_crypt_key"
            )
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_returns_401_without_token() {
        let db_path = temp_db_path("http-search-401-missing");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-any-limit-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "query": "test" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String("missing token".into()))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_dev_open_admin_does_not_fail_foreign_key() {
        let db_path = temp_db_path("http-search-dev-open-admin-fk");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-dev-open-admin-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server_with_dev(proxy, usage_base, true).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "query": "dev-open-admin fk" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_dev_open_admin_fallback_keeps_project_header_without_primary_pin() {
        let db_path = temp_db_path("http-search-dev-open-admin-project-disabled");
        let db_str = db_path.to_string_lossy().to_string();

        let project_id = "dev-open-admin-project";
        let seen = Arc::new(Mutex::new(Vec::<(String, Option<String>)>::new()));
        let upstream_addr = spawn_http_search_mock_recording_upstream_identity(seen.clone()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-http-search-dev-project-a".to_string(),
                "tvly-http-search-dev-project-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;

        let key_rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
        )
        .fetch_all(&pool)
        .await
        .expect("fetch key rows");
        let primary_key = key_rows[0].clone();
        let project_key = key_rows[1].clone();

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO token_primary_api_key_affinity (
                token_id,
                user_id,
                api_key_id,
                created_at,
                updated_at
            )
            VALUES (?, NULL, ?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                user_id = excluded.user_id,
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind("dev")
        .bind(&primary_key.0)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed dev primary affinity");
        sqlx::query(
            r#"
            INSERT INTO http_project_api_key_affinity (
                owner_subject,
                project_id_hash,
                api_key_id,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(owner_subject, project_id_hash) DO UPDATE SET
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind("token:dev")
        .bind(sha256_hex(project_id))
        .bind(&project_key.0)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed dev project affinity");

        let proxy_addr = spawn_proxy_server_with_dev(proxy, usage_base, true).await;

        let client = Client::new();
        let resp = client
            .post(format!("http://{}/api/tavily/search", proxy_addr))
            .header("X-Project-ID", project_id)
            .json(&serde_json::json!({ "query": "dev project affinity should be ignored" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("status").and_then(|value| value.as_i64()),
            Some(200)
        );

        let seen = seen.lock().expect("seen lock should not be poisoned");
        assert_eq!(seen.len(), 1);
        assert!(
            key_rows.iter().any(|(_, secret)| secret == &seen[0].0),
            "dev-open-admin fallback should use an available upstream pool key"
        );
        assert_eq!(seen[0].1.as_deref(), Some(project_id));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_hikari_routing_key_is_internal_and_takes_affinity_precedence() {
        let db_path = temp_db_path("http-search-hikari-routing-key");
        let db_str = db_path.to_string_lossy().to_string();

        let seen = Arc::new(Mutex::new(Vec::<(Option<String>, Option<String>)>::new()));
        let upstream_seen = seen.clone();
        let app = Router::new().route(
            "/search",
            post(move |headers: HeaderMap, Json(_body): Json<Value>| {
                let upstream_seen = upstream_seen.clone();
                async move {
                    let project_id = headers
                        .get("x-project-id")
                        .and_then(|value| value.to_str().ok())
                        .map(|value| value.to_string());
                    let hikari_routing_key = headers
                        .get("x-hikari-routing-key")
                        .and_then(|value| value.to_str().ok())
                        .map(|value| value.to_string());
                    upstream_seen
                        .lock()
                        .expect("upstream seen lock should not be poisoned")
                        .push((project_id, hikari_routing_key));

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
                    )
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-http-search-hikari-routing".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-hikari-routing"))
            .await
            .expect("create token");
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let resp = client
            .post(format!("http://{}/api/tavily/search", proxy_addr))
            .header("Authorization", format!("Bearer {}", token.token))
            .header("X-Hikari-Routing-Key", "hikari-route-alpha")
            .header("X-Project-ID", "legacy-project-alpha")
            .json(&serde_json::json!({ "query": "hikari routing key" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        {
            let seen = seen.lock().expect("seen lock should not be poisoned");
            assert_eq!(
                seen.as_slice(),
                &[(Some("legacy-project-alpha".to_string()), None)]
            );
        }

        let pool = connect_sqlite_test_pool(&db_str).await;
        let hikari_binding_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM http_project_api_key_affinity WHERE owner_subject = ? AND project_id_hash = ?",
        )
        .bind(format!("token:{}", token.id))
        .bind(sha256_hex("hikari-route-alpha"))
        .fetch_one(&pool)
        .await
        .expect("count hikari route bindings");
        assert_eq!(
            hikari_binding_count, 1,
            "Hikari routing key should create the generic API route binding"
        );
        let legacy_project_binding_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM http_project_api_key_affinity WHERE owner_subject = ? AND project_id_hash = ?",
        )
        .bind(format!("token:{}", token.id))
        .bind(sha256_hex("legacy-project-alpha"))
        .fetch_one(&pool)
        .await
        .expect("count legacy project route bindings");
        assert_eq!(
            legacy_project_binding_count, 0,
            "X-Hikari-Routing-Key should take precedence over X-Project-ID affinity input"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_dev_open_admin_explicit_token_charges_bound_account() {
        let db_path = temp_db_path("http-search-dev-open-admin-explicit-token");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-dev-open-admin-explicit-token-key";
        let (upstream_addr, hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "dev-open-admin-explicit-token-user".to_string(),
                username: Some("devopenadmin".to_string()),
                name: Some("Dev Open Admin".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let access_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:dev-open-admin-explicit"))
            .await
            .expect("bind token");

        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), usage_base, true).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .json(&serde_json::json!({
                "query": "dev-open-admin explicit token",
                "search_depth": "basic"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let pool = connect_sqlite_test_pool(&db_str).await;
        let row = sqlx::query(
            r#"
            SELECT token_id, billing_subject, billing_state, business_credits
            FROM auth_token_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let token_id: String = row.try_get("token_id").expect("token_id");
        let billing_subject: Option<String> = row.try_get("billing_subject").expect("subject");
        let billing_state: String = row.try_get("billing_state").expect("state");
        let business_credits: Option<i64> = row.try_get("business_credits").expect("credits");
        let expected_subject = format!("account:{}", user.user_id);
        assert_eq!(token_id, access_token.id);
        assert_eq!(billing_subject.as_deref(), Some(expected_subject.as_str()));
        assert_eq!(billing_state, "charged");
        assert_eq!(business_credits, Some(1));

        let account_month: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
        )
        .bind(&user.user_id)
        .fetch_one(&pool)
        .await
        .expect("account monthly quota exists");
        assert_eq!(account_month.1, 1);

        let token_month: Option<(i64, i64)> = sqlx::query_as(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
        )
        .bind(&access_token.id)
        .fetch_optional(&pool)
        .await
        .expect("fetch token monthly quota");
        assert_eq!(
            token_month.map(|(_, month_count)| month_count).unwrap_or(0),
            0
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_returns_403_for_non_admin() {
        let db_path = temp_db_path("keys-batch-403-non-admin");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_bulk_actions_returns_403_for_non_admin() {
        let db_path = temp_db_path("keys-bulk-actions-403-non-admin");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/bulk-actions", addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "action": "delete",
                "key_ids": ["key-1"]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_bulk_actions_soft_delete_selected_keys() {
        let db_path = temp_db_path("keys-bulk-actions-delete");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-delete-a".to_string(),
                "tvly-delete-b".to_string(),
                "tvly-delete-c".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = fetch_api_key_rows(&pool).await;
        let delete_a_id = find_api_key_id(&rows, "tvly-delete-a");
        let delete_b_id = find_api_key_id(&rows, "tvly-delete-b");
        let keep_id = find_api_key_id(&rows, "tvly-delete-c");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/bulk-actions", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "action": "delete",
                "key_ids": [delete_a_id, delete_b_id]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.pointer("/summary/requested")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            body.pointer("/summary/succeeded")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            body.pointer("/summary/failed")
                .and_then(|value| value.as_u64()),
            Some(0)
        );

        let deleted_a: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM api_keys WHERE id = ?")
                .bind(find_api_key_id(&rows, "tvly-delete-a"))
                .fetch_one(&pool)
                .await
                .expect("fetch deleted_at for delete-a");
        let deleted_b: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM api_keys WHERE id = ?")
                .bind(find_api_key_id(&rows, "tvly-delete-b"))
                .fetch_one(&pool)
                .await
                .expect("fetch deleted_at for delete-b");
        let keep_deleted_at: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM api_keys WHERE id = ?")
                .bind(&keep_id)
                .fetch_one(&pool)
                .await
                .expect("fetch deleted_at for keep key");

        assert!(deleted_a.is_some());
        assert!(deleted_b.is_some());
        assert_eq!(keep_deleted_at, None);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_bulk_actions_clear_quarantine_skips_non_quarantined_keys() {
        let db_path = temp_db_path("keys-bulk-actions-clear-quarantine");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-clear-active".to_string(),
                "tvly-clear-quarantined".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = fetch_api_key_rows(&pool).await;
        let active_id = find_api_key_id(&rows, "tvly-clear-active");
        let quarantined_id = find_api_key_id(&rows, "tvly-clear-quarantined");

        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&quarantined_id)
        .bind("/api/tavily/search")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("deactivated")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("seed quarantine");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/bulk-actions", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "action": "clear_quarantine",
                "key_ids": [quarantined_id, active_id]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.pointer("/summary/requested")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            body.pointer("/summary/succeeded")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            body.pointer("/summary/skipped")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            body.pointer("/summary/failed")
                .and_then(|value| value.as_u64()),
            Some(0)
        );

        let results = body
            .get("results")
            .and_then(|value| value.as_array())
            .expect("results array");
        let statuses: HashMap<String, String> = results
            .iter()
            .map(|item| {
                (
                    item.get("key_id")
                        .and_then(|value| value.as_str())
                        .expect("result key_id")
                        .to_string(),
                    item.get("status")
                        .and_then(|value| value.as_str())
                        .expect("result status")
                        .to_string(),
                )
            })
            .collect();
        assert_eq!(
            statuses.get(&find_api_key_id(&rows, "tvly-clear-quarantined")),
            Some(&"success".to_string())
        );
        assert_eq!(
            statuses.get(&find_api_key_id(&rows, "tvly-clear-active")),
            Some(&"skipped".to_string())
        );

        let active_quarantine_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(find_api_key_id(&rows, "tvly-clear-active"))
        .fetch_one(&pool)
        .await
        .expect("count active key quarantine rows");
        let quarantined_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
        )
        .bind(find_api_key_id(&rows, "tvly-clear-quarantined"))
        .fetch_one(&pool)
        .await
        .expect("count quarantined key quarantine rows");
        assert_eq!(active_quarantine_count, 0);
        assert_eq!(quarantined_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn manual_sync_usage_endpoint_allows_non_active_keys_and_preserves_failed_quota_data() {
        let db_path = temp_db_path("keys-manual-sync-status-agnostic");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-ok-disabled".to_string(),
                "tvly-ok-exhausted".to_string(),
                "tvly-ok-quarantined".to_string(),
                "tvly-unauth".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = fetch_api_key_rows(&pool).await;
        let disabled_id = find_api_key_id(&rows, "tvly-ok-disabled");
        let exhausted_id = find_api_key_id(&rows, "tvly-ok-exhausted");
        let quarantined_id = find_api_key_id(&rows, "tvly-ok-quarantined");
        let failing_id = find_api_key_id(&rows, "tvly-unauth");

        proxy
            .disable_key_by_id(&disabled_id)
            .await
            .expect("disable key");
        proxy
            .mark_key_quota_exhausted_by_secret("tvly-ok-exhausted")
            .await
            .expect("mark exhausted key");
        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&quarantined_id)
        .bind("/api/tavily/usage")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("deactivated")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("seed quarantined key");

        sqlx::query(
            "UPDATE api_keys SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ? WHERE id = ?",
        )
        .bind(555_i64)
        .bind(444_i64)
        .bind(333_i64)
        .bind(&failing_id)
        .execute(&pool)
        .await
        .expect("seed failing key quota snapshot");

        let usage_addr = spawn_usage_mock_server().await;
        let usage_base = format!("http://{usage_addr}");
        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr =
            spawn_keys_admin_server_with_usage_base(proxy, forward_auth, false, usage_base).await;

        let client = Client::new();
        for key_id in [&disabled_id, &exhausted_id, &quarantined_id] {
            let resp = client
                .post(format!("http://{}/api/keys/{}/sync-usage", addr, key_id))
                .header("x-forward-user", "admin")
                .send()
                .await
                .expect("manual sync request succeeds");
            assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
        }

        for key_id in [&disabled_id, &exhausted_id, &quarantined_id] {
            let (_, _, quota_synced_at) = fetch_key_quota_snapshot(&pool, key_id).await;
            assert!(
                quota_synced_at.is_some(),
                "quota should be synced for {key_id}"
            );
            assert_eq!(fetch_key_quota_sample_count(&pool, key_id).await, 1);
        }

        let failing_resp = client
            .post(format!(
                "http://{}/api/keys/{}/sync-usage",
                addr, failing_id
            ))
            .header("x-forward-user", "admin")
            .send()
            .await
            .expect("failing manual sync request succeeds");
        assert_eq!(failing_resp.status(), reqwest::StatusCode::UNAUTHORIZED);
        let failing_body: serde_json::Value = failing_resp
            .json()
            .await
            .expect("parse failing response body");
        assert_eq!(
            failing_body.get("error").and_then(|value| value.as_str()),
            Some("usage_http")
        );

        assert_eq!(
            fetch_key_quota_snapshot(&pool, &failing_id).await,
            (Some(555), Some(444), Some(333))
        );
        assert_eq!(fetch_key_quota_sample_count(&pool, &failing_id).await, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_bulk_actions_sync_usage_handles_mixed_statuses_and_failures() {
        let db_path = temp_db_path("keys-bulk-actions-sync-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-ok-active".to_string(),
                "tvly-ok-disabled".to_string(),
                "tvly-ok-exhausted".to_string(),
                "tvly-ok-quarantined".to_string(),
                "tvly-unauth".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = fetch_api_key_rows(&pool).await;
        let active_id = find_api_key_id(&rows, "tvly-ok-active");
        let disabled_id = find_api_key_id(&rows, "tvly-ok-disabled");
        let exhausted_id = find_api_key_id(&rows, "tvly-ok-exhausted");
        let quarantined_id = find_api_key_id(&rows, "tvly-ok-quarantined");
        let failing_id = find_api_key_id(&rows, "tvly-unauth");

        proxy
            .disable_key_by_id(&disabled_id)
            .await
            .expect("disable key");
        proxy
            .mark_key_quota_exhausted_by_secret("tvly-ok-exhausted")
            .await
            .expect("mark exhausted key");
        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&quarantined_id)
        .bind("/api/tavily/usage")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("deactivated")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("seed quarantined key");

        sqlx::query(
            "UPDATE api_keys SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ? WHERE id = ?",
        )
        .bind(777_i64)
        .bind(666_i64)
        .bind(555_i64)
        .bind(&failing_id)
        .execute(&pool)
        .await
        .expect("seed failing key quota snapshot");

        let usage_addr = spawn_usage_mock_server().await;
        let usage_base = format!("http://{usage_addr}");
        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr =
            spawn_keys_admin_server_with_usage_base(proxy, forward_auth, false, usage_base).await;

        let client = Client::new();
        let resp = client
            .post(format!("http://{}/api/keys/bulk-actions", addr))
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "action": "sync_usage",
                "key_ids": [active_id, disabled_id, exhausted_id, quarantined_id, failing_id]
            }))
            .send()
            .await
            .expect("bulk sync request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.pointer("/summary/requested")
                .and_then(|value| value.as_u64()),
            Some(5)
        );
        assert_eq!(
            body.pointer("/summary/succeeded")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
        assert_eq!(
            body.pointer("/summary/failed")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            body.pointer("/summary/skipped")
                .and_then(|value| value.as_u64()),
            Some(0)
        );

        let results = body
            .get("results")
            .and_then(|value| value.as_array())
            .expect("results array");
        let statuses: HashMap<String, String> = results
            .iter()
            .map(|item| {
                (
                    item.get("key_id")
                        .and_then(|value| value.as_str())
                        .expect("result key_id")
                        .to_string(),
                    item.get("status")
                        .and_then(|value| value.as_str())
                        .expect("result status")
                        .to_string(),
                )
            })
            .collect();
        assert_eq!(statuses.get(&active_id), Some(&"success".to_string()));
        assert_eq!(statuses.get(&disabled_id), Some(&"success".to_string()));
        assert_eq!(statuses.get(&exhausted_id), Some(&"success".to_string()));
        assert_eq!(statuses.get(&quarantined_id), Some(&"success".to_string()));
        assert_eq!(statuses.get(&failing_id), Some(&"failed".to_string()));

        for key_id in [&active_id, &disabled_id, &exhausted_id, &quarantined_id] {
            let (_, _, quota_synced_at) = fetch_key_quota_snapshot(&pool, key_id).await;
            assert!(
                quota_synced_at.is_some(),
                "quota should be synced for {key_id}"
            );
            assert_eq!(fetch_key_quota_sample_count(&pool, key_id).await, 1);
        }

        assert_eq!(
            fetch_key_quota_snapshot(&pool, &failing_id).await,
            (Some(777), Some(666), Some(555))
        );
        assert_eq!(fetch_key_quota_sample_count(&pool, &failing_id).await, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_bulk_actions_sync_usage_streams_progress_events() {
        let db_path = temp_db_path("keys-bulk-actions-sync-usage-sse");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-ok-active".to_string(),
                "tvly-ok-disabled".to_string(),
                "tvly-unauth".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = fetch_api_key_rows(&pool).await;
        let ok_a_id = find_api_key_id(&rows, "tvly-ok-active");
        let ok_b_id = find_api_key_id(&rows, "tvly-ok-disabled");
        let failing_id = find_api_key_id(&rows, "tvly-unauth");

        let usage_addr = spawn_usage_mock_server().await;
        let usage_base = format!("http://{usage_addr}");
        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr =
            spawn_keys_admin_server_with_usage_base(proxy, forward_auth, false, usage_base).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{}/api/keys/bulk-actions", addr))
            .header(reqwest::header::ACCEPT, "text/event-stream; charset=utf-8")
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "action": "sync_usage",
                "key_ids": [ok_a_id, ok_b_id, failing_id]
            }))
            .send()
            .await
            .expect("bulk sync sse request succeeds");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/event-stream"),
            "expected event stream response, got {content_type}"
        );

        let body = response.text().await.expect("read sse body");
        let normalized_body = body.replace("\r\n", "\n");
        let events: Vec<serde_json::Value> = normalized_body
            .split("\n\n")
            .filter_map(|chunk| {
                let data = chunk
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    None
                } else {
                    Some(
                        serde_json::from_str::<serde_json::Value>(&data)
                            .expect("decode bulk sync sse event"),
                    )
                }
            })
            .collect();

        assert!(
            events.len() >= 6,
            "expected prepare + sync phase + 3 item events + completion, got: {events:?}"
        );
        assert_eq!(events[0]["type"].as_str(), Some("phase"));
        assert_eq!(events[0]["phaseKey"].as_str(), Some("prepare_request"));
        assert_eq!(events[0]["total"].as_u64(), Some(3));
        assert_eq!(events[1]["type"].as_str(), Some("phase"));
        assert_eq!(events[1]["phaseKey"].as_str(), Some("sync_usage"));
        assert_eq!(events[1]["current"].as_u64(), Some(0));
        assert_eq!(events[1]["total"].as_u64(), Some(3));

        let item_events: Vec<&serde_json::Value> = events
            .iter()
            .filter(|event| event["type"].as_str() == Some("item"))
            .collect();
        assert_eq!(item_events.len(), 3, "expected one item event per key");
        assert_eq!(item_events[0]["current"].as_u64(), Some(1));
        assert_eq!(item_events[2]["current"].as_u64(), Some(3));
        assert_eq!(item_events[2]["summary"]["failed"].as_u64(), Some(1));

        let refresh_event = events
            .iter()
            .find(|event| {
                event["type"].as_str() == Some("phase")
                    && event["phaseKey"].as_str() == Some("refresh_ui")
            })
            .expect("refresh_ui phase event");
        assert_eq!(refresh_event["current"].as_u64(), Some(3));

        let complete = events
            .iter()
            .find(|event| event["type"].as_str() == Some("complete"))
            .expect("complete event");
        assert_eq!(
            complete["payload"]["summary"]["requested"].as_u64(),
            Some(3)
        );
        assert_eq!(
            complete["payload"]["summary"]["succeeded"].as_u64(),
            Some(2)
        );
        assert_eq!(complete["payload"]["summary"]["failed"].as_u64(), Some(1));
        assert_eq!(
            complete["payload"]["results"]
                .as_array()
                .map(|items| items.len()),
            Some(3)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_validate_reports_ok_exhausted_and_duplicates() {
        let db_path = temp_db_path("keys-validate-ok-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let _existing_id = proxy
            .add_or_undelete_key("tvly-ok-active")
            .await
            .expect("existing active key created");
        let deleted_id = proxy
            .add_or_undelete_key("tvly-ok-disabled")
            .await
            .expect("deleted key created");
        proxy
            .soft_delete_key_by_id(&deleted_id)
            .await
            .expect("key soft deleted");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let usage_addr = spawn_usage_mock_server().await;
        let usage_base = format!("http://{}", usage_addr);
        let addr =
            spawn_keys_admin_server_with_usage_base(proxy, forward_auth, false, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/validate", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "api_keys": [
                    "tvly-ok",
                    "tvly-exhausted",
                    "tvly-unauth",
                    "tvly-rate-limited",
                    "tvly-ok-active",
                    "tvly-ok-disabled",
                    "tvly-ok-active",
                    "tvly-ok"
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary");
        assert_eq!(summary.get("input_lines").and_then(|v| v.as_u64()), Some(8));
        assert_eq!(summary.get("valid_lines").and_then(|v| v.as_u64()), Some(8));
        assert_eq!(
            summary.get("unique_in_input").and_then(|v| v.as_u64()),
            Some(6)
        );
        assert_eq!(
            summary.get("duplicate_in_input").and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(
            summary.get("already_exists").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(summary.get("ok").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(summary.get("exhausted").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("invalid").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("error").and_then(|v| v.as_u64()), Some(1));

        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 8);
        assert_eq!(
            results[0].get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            results[1].get("status").and_then(|v| v.as_str()),
            Some("ok_exhausted")
        );
        assert_eq!(
            results[2].get("status").and_then(|v| v.as_str()),
            Some("unauthorized")
        );
        assert_eq!(
            results[3].get("status").and_then(|v| v.as_str()),
            Some("error")
        );
        assert_eq!(
            results[4].get("status").and_then(|v| v.as_str()),
            Some("already_exists")
        );
        assert_eq!(
            results[5].get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            results[6].get("status").and_then(|v| v.as_str()),
            Some("duplicate_in_input")
        );
        assert_eq!(
            results[7].get("status").and_then(|v| v.as_str()),
            Some("duplicate_in_input")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_validate_items_return_registration_region_metadata() {
        let db_path = temp_db_path("keys-validate-registration-region");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let usage_addr = spawn_usage_mock_server().await;
        let geo_addr = spawn_api_key_geo_mock_server().await;
        let addr = spawn_keys_admin_server_with_usage_and_geo(
            proxy,
            forward_auth,
            false,
            format!("http://{usage_addr}"),
            format!("http://{geo_addr}/geo"),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/validate", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    { "api_key": "tvly-ok", "registration_ip": "8.8.8.8" },
                    { "api_key": "tvly-exhausted", "registration_ip": "1.1.1.1" },
                    { "api_key": "tvly-ok", "registration_ip": "8.8.8.8" }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(
            results[0].get("registration_ip").and_then(|v| v.as_str()),
            Some("8.8.8.8")
        );
        assert_eq!(
            results[0]
                .get("registration_region")
                .and_then(|v| v.as_str()),
            Some("US")
        );
        assert_eq!(
            results[0]
                .get("assigned_proxy_key")
                .and_then(|v| v.as_str()),
            Some("__direct__")
        );
        assert_eq!(
            results[0]
                .get("assigned_proxy_label")
                .and_then(|v| v.as_str()),
            Some("Direct")
        );
        assert_eq!(
            results[0]
                .get("assigned_proxy_match_kind")
                .and_then(|v| v.as_str()),
            Some("other")
        );
        assert_eq!(
            results[1].get("registration_ip").and_then(|v| v.as_str()),
            Some("1.1.1.1")
        );
        assert_eq!(
            results[1]
                .get("registration_region")
                .and_then(|v| v.as_str()),
            Some("US Westfield (MA)")
        );
        assert_eq!(
            results[1]
                .get("assigned_proxy_match_kind")
                .and_then(|v| v.as_str()),
            Some("other")
        );
        assert_eq!(
            results[2]
                .get("registration_region")
                .and_then(|v| v.as_str()),
            Some("US")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_validate_items_probe_usage_through_selected_proxy_node() {
        let db_path = temp_db_path("keys-validate-via-forward-proxy");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let usage_addr = spawn_usage_mock_server().await;
        let relay_hits = Arc::new(AtomicUsize::new(0));
        let relay_addr =
            spawn_usage_proxy_relay_server(format!("http://{usage_addr}"), relay_hits.clone())
                .await;
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![format!("http://{relay_addr}")],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,

                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");
        relay_hits.store(0, Ordering::SeqCst);

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let geo_addr = spawn_api_key_geo_mock_server().await;
        let addr = spawn_keys_admin_server_with_usage_and_geo(
            proxy,
            forward_auth,
            false,
            "http://usage.test".to_string(),
            format!("http://{geo_addr}/geo"),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/validate", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    { "api_key": "tvly-ok", "registration_ip": "18.183.246.69" }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        let proxy_attempt_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM forward_proxy_attempts WHERE proxy_key = ?")
                .bind(format!("http://{relay_addr}"))
                .fetch_one(&pool)
                .await
                .expect("count proxy attempts");
        assert!(
            proxy_attempt_count > 0,
            "usage probe should record attempts against the selected forward proxy node"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_can_mark_exhausted_by_secret() {
        let db_path = temp_db_path("keys-batch-mark-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let addr = spawn_keys_admin_server_with_usage_base(
            proxy.clone(),
            forward_auth,
            false,
            "http://127.0.0.1:58088".to_string(),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "api_keys": ["tvly-mark-exhausted"],
                "exhausted_api_keys": ["tvly-mark-exhausted"],
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].get("status").and_then(|v| v.as_str()),
            Some("created")
        );
        assert_eq!(
            results[0].get("marked_exhausted").and_then(|v| v.as_bool()),
            Some(true)
        );

        let metrics = proxy.list_api_key_metrics().await.expect("list keys");
        assert!(!metrics.is_empty(), "expected at least one key metric row");

        let mut found = None;
        for m in metrics {
            let secret = proxy
                .get_api_key_secret(&m.id)
                .await
                .expect("fetch secret")
                .unwrap_or_default();
            if secret == "tvly-mark-exhausted" {
                found = Some(m);
                break;
            }
        }
        let found = found.expect("find inserted key");
        assert_eq!(found.status, "exhausted");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_rejects_over_limit() {
        let db_path = temp_db_path("keys-batch-400-over-limit");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let api_keys: Vec<String> = (0..=API_KEYS_BATCH_LIMIT)
            .map(|i| format!("tvly-{i}"))
            .collect();

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": api_keys }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String("too_many_items".into()))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_reports_statuses_and_is_partial_success() {
        let db_path = temp_db_path("keys-batch-mixed");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        // Pre-create: one active existing key, and one soft-deleted key.
        let _existing_id = proxy
            .add_or_undelete_key("tvly-existing")
            .await
            .expect("existing key created");
        let deleted_id = proxy
            .add_or_undelete_key("tvly-deleted")
            .await
            .expect("deleted key created");
        proxy
            .soft_delete_key_by_id(&deleted_id)
            .await
            .expect("key soft deleted");

        // Create a trigger that forces a deterministic failure for one key.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        sqlx::query(
            r#"
            CREATE TRIGGER fail_insert_api_key
            BEFORE INSERT ON api_keys
            WHEN NEW.api_key = 'tvly-fail'
            BEGIN
                SELECT RAISE(ABORT, 'boom');
            END;
            "#,
        )
        .execute(&pool)
        .await
        .expect("create trigger");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let input = vec![
            "  tvly-new  ".to_string(),
            "tvly-fail".to_string(),
            "tvly-new-2".to_string(),
            "tvly-existing".to_string(),
            "tvly-deleted".to_string(),
            "tvly-existing".to_string(),
            "tvly-new-2".to_string(),
            "".to_string(),
            "   ".to_string(),
        ];

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": input, "group": "team-a" }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("created").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(summary.get("undeleted").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("existed").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            summary.get("duplicate_in_input").and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(summary.get("failed").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            summary.get("ignored_empty").and_then(|v| v.as_u64()),
            Some(2)
        );

        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 7, "empty items are ignored in results");

        let statuses: Vec<(&str, &str)> = results
            .iter()
            .map(|r| {
                (
                    r.get("api_key").and_then(|v| v.as_str()).unwrap_or(""),
                    r.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("tvly-new", "created"),
                ("tvly-fail", "failed"),
                ("tvly-new-2", "created"),
                ("tvly-existing", "existed"),
                ("tvly-deleted", "undeleted"),
                ("tvly-existing", "duplicate_in_input"),
                ("tvly-new-2", "duplicate_in_input"),
            ]
        );

        // id is present only when we hit the DB successfully.
        for (idx, expected_has_id) in [
            (0, true),
            (1, false),
            (2, true),
            (3, true),
            (4, true),
            (5, false),
            (6, false),
        ] {
            let has_id = results[idx].get("id").and_then(|v| v.as_str()).is_some();
            assert_eq!(has_id, expected_has_id, "result[{idx}] id presence");
        }

        // error is required for failed.
        let err = results[1]
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            err.contains("boom"),
            "failed error should include trigger message"
        );

        // DB side effects: soft-deleted key should be restored, failed key should not exist.
        let deleted_at: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM api_keys WHERE api_key = ?")
                .bind("tvly-deleted")
                .fetch_one(&pool)
                .await
                .expect("tvly-deleted exists");
        assert!(deleted_at.is_none(), "tvly-deleted should be undeleted");

        for key in ["tvly-new", "tvly-new-2", "tvly-existing", "tvly-deleted"] {
            let group_name: Option<String> =
                sqlx::query_scalar("SELECT group_name FROM api_keys WHERE api_key = ?")
                    .bind(key)
                    .fetch_one(&pool)
                    .await
                    .expect("key exists");
            assert_eq!(
                group_name.as_deref(),
                Some("team-a"),
                "{key} should have group_name=team-a"
            );
        }

        let fail_row: Option<String> =
            sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
                .bind("tvly-fail")
                .fetch_optional(&pool)
                .await
                .expect("query fail key");
        assert!(fail_row.is_none(), "tvly-fail should not be inserted");

        let _ = std::fs::remove_file(db_path);
    }
