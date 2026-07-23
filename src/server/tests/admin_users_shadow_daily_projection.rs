use super::*;
use super::core_support_and_parsing::temp_db_path;
use super::upstream_support_and_manual_jobs::spawn_admin_users_server;
use tavily_hikari::UpstreamProjectIdMode;

#[tokio::test]
async fn list_users_reports_shadow_daily_usage_as_confirmed_or_projected() {
    let db_path = temp_db_path("admin-users-shadow-daily-availability");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        vec!["tvly-admin-users-shadow-daily".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("proxy created");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-alice".to_string(),
            username: Some("shadow-alice".to_string()),
            name: Some("Shadow Alice".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");
    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-bob".to_string(),
            username: Some("shadow-bob".to_string()),
            name: Some("Shadow Bob".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert bob");
    let charlie = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-charlie".to_string(),
            username: Some("shadow-charlie".to_string()),
            name: Some("Shadow Charlie".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert charlie");
    let dana = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-dana".to_string(),
            username: Some("shadow-dana".to_string()),
            name: Some("Shadow Dana".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert dana");
    let erin = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-erin".to_string(),
            username: Some("shadow-erin".to_string()),
            name: Some("Shadow Erin".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert erin");
    let frank = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-frank".to_string(),
            username: Some("shadow-frank".to_string()),
            name: Some("Shadow Frank".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert frank");
    let grace = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-grace".to_string(),
            username: Some("shadow-grace".to_string()),
            name: Some("Shadow Grace".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert grace");

    let alice_token = proxy
        .ensure_user_token_binding(&alice.user_id, Some("shadow-alice-token"))
        .await
        .expect("bind alice token");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("shadow-bob-token"))
        .await
        .expect("bind bob token");
    let charlie_token = proxy
        .ensure_user_token_binding(&charlie.user_id, Some("shadow-charlie-token"))
        .await
        .expect("bind charlie token");
    let dana_token = proxy
        .ensure_user_token_binding(&dana.user_id, Some("shadow-dana-token"))
        .await
        .expect("bind dana token");
    let frank_token = proxy
        .ensure_user_token_binding(&frank.user_id, Some("shadow-frank-token"))
        .await
        .expect("bind frank token");
    let grace_token = proxy
        .ensure_user_token_binding(&grace.user_id, Some("shadow-grace-token"))
        .await
        .expect("bind grace token");

    proxy
        .charge_token_quota(&alice_token.id, 100)
        .await
        .expect("charge alice quota");
    proxy
        .charge_token_quota(&bob_token.id, 50)
        .await
        .expect("charge bob quota");
    proxy
        .charge_token_quota(&charlie_token.id, 10)
        .await
        .expect("charge charlie quota");
    proxy
        .charge_token_quota(&dana_token.id, 40)
        .await
        .expect("charge dana quota");
    proxy
        .charge_token_quota(&frank_token.id, 12)
        .await
        .expect("charge frank quota");
    proxy
        .charge_token_quota(&grace_token.id, 6)
        .await
        .expect("charge grace quota");

    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_str)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .expect("open shadow adjustment pool");
    let now = proxy.backend_time().now_ts();
    let current_period = tavily_hikari::business_period_for_timestamp(now);
    let current_date = current_period
        .code
        .split('/')
        .next()
        .expect("current reconciliation date")
        .to_string();
    let period_codes = [
        format!("{current_date}/S1"),
        format!("{current_date}/S2"),
        format!("{current_date}/S3"),
    ];
    for (token_id, key_id, period_code, project_id, billing_subject, period_start, period_end) in [
        (
            alice_token.id.as_str(),
            "key-shadow-alice",
            period_codes[0].as_str(),
            "project-shadow-alice",
            format!("account:{}", alice.user_id),
            current_period.starts_at,
            current_period.starts_at + 300,
        ),
        (
            bob_token.id.as_str(),
            "key-shadow-bob",
            period_codes[0].as_str(),
            "project-shadow-bob",
            format!("account:{}", bob.user_id),
            current_period.starts_at + 600,
            current_period.starts_at + 900,
        ),
        (
            dana_token.id.as_str(),
            "key-shadow-dana-a",
            period_codes[0].as_str(),
            "project-shadow-dana-a",
            format!("account:{}", dana.user_id),
            current_period.starts_at + 1_200,
            current_period.starts_at + 1_500,
        ),
        (
            dana_token.id.as_str(),
            "key-shadow-dana-b",
            period_codes[1].as_str(),
            "project-shadow-dana-b",
            format!("account:{}", dana.user_id),
            current_period.starts_at + 1_800,
            current_period.starts_at + 2_100,
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
                period_start, period_end, request_count, first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(key_id)
        .bind(period_code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&pool)
        .await
        .expect("insert shadow usage row");
    }
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'actual', ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(&frank_token.id)
    .bind("key-actual-frank")
    .bind(&period_codes[2])
    .bind("project-actual-frank")
    .bind(format!("account:{}", frank.user_id))
    .bind(current_period.starts_at + 2_400)
    .bind(current_period.starts_at + 2_700)
    .bind(current_period.starts_at + 2_400)
    .bind(current_period.starts_at + 2_700)
    .bind(current_period.starts_at + 2_700)
    .execute(&pool)
    .await
    .expect("insert frank actual usage row");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'actual', ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(&grace_token.id)
    .bind("key-actual-grace")
    .bind(&period_codes[2])
    .bind("project-actual-grace")
    .bind(format!("account:{}", grace.user_id))
    .bind(current_period.starts_at + 3_000)
    .bind(current_period.starts_at + 3_300)
    .bind(current_period.starts_at + 3_000)
    .bind(current_period.starts_at + 3_300)
    .bind(current_period.starts_at + 3_300)
    .execute(&pool)
    .await
    .expect("insert grace actual usage row");
    let attributed_at = now.saturating_sub(60);
    sqlx::query(
        r#"
        INSERT INTO billing_reconciliation_shadow_adjustments (
            settlement_key, token_id, billing_subject, period_code, delta_credits,
            attributed_at, degraded_reason, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(format!("v1:{}:{}", alice_token.id, period_codes[0]))
    .bind(&alice_token.id)
    .bind(format!("account:{}", alice.user_id))
    .bind(&period_codes[0])
    .bind(5_i64)
    .bind(attributed_at)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert alice shadow adjustment");
    sqlx::query(
        r#"
        INSERT INTO billing_reconciliation_shadow_adjustments (
            settlement_key, token_id, billing_subject, period_code, delta_credits,
            attributed_at, degraded_reason, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(format!("v1:{}:{}", bob_token.id, period_codes[0]))
    .bind(&bob_token.id)
    .bind(format!("account:{}", bob.user_id))
    .bind(&period_codes[0])
    .bind(0_i64)
    .bind(attributed_at)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert bob shadow adjustment");
    sqlx::query(
        r#"
        INSERT INTO billing_reconciliation_shadow_adjustments (
            settlement_key, token_id, billing_subject, period_code, delta_credits,
            attributed_at, degraded_reason, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(format!("v1:{}:{}", dana_token.id, period_codes[0]))
    .bind(&dana_token.id)
    .bind(format!("account:{}", dana.user_id))
    .bind(&period_codes[0])
    .bind(-2_i64)
    .bind(attributed_at)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert dana shadow adjustment");
    for (
        settlement_key,
        token_id,
        period_code,
        project_id,
        billing_subject,
        period_start,
        period_end,
        status,
        delta_credits,
        degraded_reason,
        next_attempt_at,
    ) in [
        (
            format!("v1:{}:{}", alice_token.id, period_codes[0]),
            alice_token.id.clone(),
            period_codes[0].clone(),
            "project-shadow-alice".to_string(),
            format!("account:{}", alice.user_id),
            current_period.starts_at,
            current_period.starts_at + 300,
            "shadow_settled".to_string(),
            5_i64,
            None,
            None,
        ),
        (
            format!("v1:{}:{}", bob_token.id, period_codes[0]),
            bob_token.id.clone(),
            period_codes[0].clone(),
            "project-shadow-bob".to_string(),
            format!("account:{}", bob.user_id),
            current_period.starts_at + 600,
            current_period.starts_at + 900,
            "shadow_degraded".to_string(),
            0_i64,
            Some("research_timeout_24h".to_string()),
            None,
        ),
        (
            format!("v1:{}:{}", dana_token.id, period_codes[0]),
            dana_token.id.clone(),
            period_codes[0].clone(),
            "project-shadow-dana-a".to_string(),
            format!("account:{}", dana.user_id),
            current_period.starts_at + 1_200,
            current_period.starts_at + 1_500,
            "shadow_settled".to_string(),
            -2_i64,
            None,
            None,
        ),
        (
            format!("v1:{}:{}", dana_token.id, period_codes[1]),
            dana_token.id.clone(),
            period_codes[1].clone(),
            "project-shadow-dana-b".to_string(),
            format!("account:{}", dana.user_id),
            current_period.starts_at + 1_800,
            current_period.starts_at + 2_100,
            "rate_limited".to_string(),
            0_i64,
            Some("upstream429".to_string()),
            Some(now + 300),
        ),
        (
            format!("v1:{}:{}", grace_token.id, period_codes[2]),
            grace_token.id.clone(),
            period_codes[2].clone(),
            "project-actual-grace".to_string(),
            format!("account:{}", grace.user_id),
            current_period.starts_at + 3_000,
            current_period.starts_at + 3_300,
            "settled".to_string(),
            0_i64,
            None,
            None,
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, upstream_usage, local_billed_credits,
                delta_credits, degraded_reason, next_attempt_at, attempt_count,
                created_at, updated_at, settled_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(settlement_key)
        .bind(token_id)
        .bind(period_code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(period_start)
        .bind(period_end)
        .bind(status)
        .bind(delta_credits)
        .bind(degraded_reason)
        .bind(next_attempt_at)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert reconciliation settlement");
    }

    let addr = spawn_admin_users_server(proxy, true).await;
    let client = Client::new();
    let response = client
        .get(format!("http://{addr}/api/users?page=1&per_page=20"))
        .send()
        .await
        .expect("list users request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("list users json");
    let items = body["items"].as_array().expect("items array");

    let alice_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(alice.user_id.as_str()))
        .expect("alice row");
    assert_eq!(alice_item["shadowDailyCreditsUsed"].as_i64(), Some(105));
    assert_eq!(
        alice_item["shadowDailyAvailability"].as_str(),
        Some("confirmed")
    );

    let bob_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(bob.user_id.as_str()))
        .expect("bob row");
    assert_eq!(bob_item["shadowDailyCreditsUsed"].as_i64(), Some(50));
    assert_eq!(
        bob_item["shadowDailyAvailability"].as_str(),
        Some("confirmed")
    );

    let charlie_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(charlie.user_id.as_str()))
        .expect("charlie row");
    assert_eq!(charlie_item["shadowDailyCreditsUsed"].as_i64(), Some(10));
    assert_eq!(
        charlie_item["shadowDailyAvailability"].as_str(),
        Some("projected")
    );

    let dana_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(dana.user_id.as_str()))
        .expect("dana row");
    assert_eq!(dana_item["shadowDailyCreditsUsed"].as_i64(), Some(38));
    assert_eq!(
        dana_item["shadowDailyAvailability"].as_str(),
        Some("projected")
    );

    let erin_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(erin.user_id.as_str()))
        .expect("erin row");
    assert_eq!(erin_item["shadowDailyCreditsUsed"].as_i64(), Some(0));
    assert_eq!(
        erin_item["shadowDailyAvailability"].as_str(),
        Some("confirmed")
    );

    let frank_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(frank.user_id.as_str()))
        .expect("frank row");
    assert_eq!(frank_item["shadowDailyCreditsUsed"].as_i64(), Some(12));
    assert_eq!(
        frank_item["shadowDailyAvailability"].as_str(),
        Some("projected")
    );

    let grace_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(grace.user_id.as_str()))
        .expect("grace row");
    assert_eq!(grace_item["shadowDailyCreditsUsed"].as_i64(), Some(6));
    assert_eq!(
        grace_item["shadowDailyAvailability"].as_str(),
        Some("confirmed")
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn list_users_hides_shadow_projection_until_compare_ready() {
    let db_path = temp_db_path("admin-users-shadow-daily-not-ready");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        vec!["tvly-admin-users-shadow-not-ready".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("proxy created");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings without shadow readiness");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-not-ready".to_string(),
            username: Some("shadow-not-ready".to_string()),
            name: Some("Shadow Not Ready".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("shadow-not-ready-token"))
        .await
        .expect("bind user token");
    proxy
        .charge_token_quota(&token.id, 9)
        .await
        .expect("charge user quota");

    let addr = spawn_admin_users_server(proxy, true).await;
    let response = Client::new()
        .get(format!("http://{addr}/api/users?page=1&per_page=20"))
        .send()
        .await
        .expect("list users request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("list users json");
    let items = body["items"].as_array().expect("items array");
    let user_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(user.user_id.as_str()))
        .expect("user row");
    assert!(user_item["shadowDailyCreditsUsed"].is_null());
    assert!(user_item["shadowDailyAvailability"].is_null());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn list_users_hides_actual_only_projection_until_compare_ready() {
    let db_path = temp_db_path("admin-users-shadow-daily-actual-only-not-ready");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        vec!["tvly-admin-users-shadow-actual-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("proxy created");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings without shadow readiness");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-actual-only-not-ready".to_string(),
            username: Some("shadow-actual-only-not-ready".to_string()),
            name: Some("Shadow Actual Only".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("shadow-actual-only-token"))
        .await
        .expect("bind user token");
    proxy
        .charge_token_quota(&token.id, 9)
        .await
        .expect("charge user quota");

    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_str)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .expect("open reconciliation pool");
    let current_period = tavily_hikari::business_period_for_timestamp(proxy.backend_time().now_ts());
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'actual', ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind("key-actual-only-not-ready")
    .bind(&current_period.code)
    .bind("project-actual-only-not-ready")
    .bind(format!("account:{}", user.user_id))
    .bind(current_period.starts_at)
    .bind(current_period.starts_at + 300)
    .bind(current_period.starts_at)
    .bind(current_period.starts_at + 300)
    .bind(current_period.starts_at + 300)
    .execute(&pool)
    .await
    .expect("insert actual usage row");

    let addr = spawn_admin_users_server(proxy, true).await;
    let response = Client::new()
        .get(format!("http://{addr}/api/users?page=1&per_page=20"))
        .send()
        .await
        .expect("list users request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("list users json");
    let items = body["items"].as_array().expect("items array");
    let user_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(user.user_id.as_str()))
        .expect("user row");
    assert!(user_item["shadowDailyCreditsUsed"].is_null());
    assert!(user_item["shadowDailyAvailability"].is_null());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn list_users_keeps_persisted_shadow_projection_after_gates_turn_off() {
    let db_path = temp_db_path("admin-users-shadow-daily-persisted-after-gates-off");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        vec!["tvly-admin-users-shadow-persisted".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("proxy created");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-users-shadow-persisted".to_string(),
            username: Some("shadow-persisted".to_string()),
            name: Some("Shadow Persisted".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("shadow-persisted-token"))
        .await
        .expect("bind user token");
    proxy
        .charge_token_quota(&token.id, 9)
        .await
        .expect("charge user quota");

    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_str)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .expect("open reconciliation pool");
    let now = proxy.backend_time().now_ts();
    let current_period = tavily_hikari::business_period_for_timestamp(now);

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind("key-shadow-persisted")
    .bind(&current_period.code)
    .bind("project-shadow-persisted")
    .bind(format!("account:{}", user.user_id))
    .bind(current_period.starts_at)
    .bind(current_period.starts_at + 300)
    .bind(current_period.starts_at)
    .bind(current_period.starts_at + 300)
    .bind(current_period.starts_at + 300)
    .execute(&pool)
    .await
    .expect("insert shadow usage row");
    sqlx::query(
        r#"
        INSERT INTO billing_reconciliation_shadow_adjustments (
            settlement_key, token_id, billing_subject, period_code, delta_credits,
            attributed_at, degraded_reason, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(format!("v1:{}:{}", token.id, current_period.code))
    .bind(&token.id)
    .bind(format!("account:{}", user.user_id))
    .bind(&current_period.code)
    .bind(4_i64)
    .bind(now.saturating_sub(60))
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert shadow adjustment");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_settlements (
            settlement_key, token_id, period_code, project_id, billing_subject,
            period_start, period_end, status, upstream_usage, local_billed_credits,
            delta_credits, degraded_reason, next_attempt_at, attempt_count,
            created_at, updated_at, settled_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 'shadow_settled', 0, 0, ?, NULL, NULL, 1, ?, ?, ?)
        "#,
    )
    .bind(format!("v1:{}:{}", token.id, current_period.code))
    .bind(&token.id)
    .bind(&current_period.code)
    .bind("project-shadow-persisted")
    .bind(format!("account:{}", user.user_id))
    .bind(current_period.starts_at)
    .bind(current_period.starts_at + 300)
    .bind(4_i64)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert shadow settlement");

    settings.api_rebalance_enabled = false;
    settings.api_rebalance_percent = 0;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("disable gate after shadow settles");

    let addr = spawn_admin_users_server(proxy, true).await;
    let response = Client::new()
        .get(format!("http://{addr}/api/users?page=1&per_page=20"))
        .send()
        .await
        .expect("list users request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("list users json");
    let items = body["items"].as_array().expect("items array");
    let user_item = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(user.user_id.as_str()))
        .expect("user row");
    assert_eq!(user_item["shadowDailyCreditsUsed"].as_i64(), Some(13));
    assert_eq!(
        user_item["shadowDailyAvailability"].as_str(),
        Some("confirmed")
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
