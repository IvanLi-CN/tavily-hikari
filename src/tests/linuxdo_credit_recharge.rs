use super::*;

#[test]
fn linuxdo_credit_recharge_adds_hourly_daily_and_monthly_quota() {
    let base = AccountQuotaLimits {
        business_calls_1h_limit: 20,
        daily_credits_limit: 30,
        monthly_credits_limit: 40,
        inherits_defaults: false,
    };
    let resolution = build_account_quota_resolution_with_recharge(
        base,
        Vec::new(),
        linuxdo_credit_recharge_quota_delta(2000),
    );

    assert_eq!(resolution.effective.business_calls_1h_limit, 60);
    assert_eq!(resolution.effective.daily_credits_limit, 230);
    assert_eq!(resolution.effective.monthly_credits_limit, 2040);
    let recharge = resolution
        .breakdown
        .iter()
        .find(|entry| entry.kind == "recharge")
        .expect("recharge row");
    assert_eq!(recharge.business_calls_1h_delta, 40);
    assert_eq!(recharge.daily_credits_delta, 200);
    assert_eq!(recharge.monthly_credits_delta, 2000);
}

#[test]
fn linuxdo_credit_recharge_price_config_enforces_normal_and_test_ranges() {
    let normal = LinuxDoCreditRechargePriceConfig::normal();
    assert_eq!(
        linuxdo_credit_recharge_money_cents(1000, 1, normal),
        Some(5_000)
    );
    assert_eq!(
        linuxdo_credit_recharge_money_cents(2000, 3, normal),
        Some(30_000)
    );
    assert_eq!(
        linuxdo_credit_recharge_money_cents(20_000, 12, normal),
        Some(1_200_000)
    );
    assert_eq!(linuxdo_credit_recharge_money_cents(1, 1, normal), None);
    assert_eq!(linuxdo_credit_recharge_money_cents(21_000, 1, normal), None);
    assert_eq!(linuxdo_credit_recharge_money_cents(1000, 13, normal), None);

    let test = LinuxDoCreditRechargePriceConfig::test_price();
    assert_eq!(linuxdo_credit_recharge_money_cents(1, 1, test), Some(100));
    assert_eq!(linuxdo_credit_recharge_money_cents(2, 1, test), None);
    assert_eq!(linuxdo_credit_recharge_money_cents(1, 2, test), None);
    assert_eq!(
        linuxdo_credit_recharge_money_cents(1000, 1, test),
        Some(5_000)
    );
    assert_eq!(linuxdo_credit_recharge_quota_delta(1).hourly_delta, 1);
    assert_eq!(linuxdo_credit_recharge_quota_delta(1).daily_delta, 1);
}

#[tokio::test]
async fn linuxdo_credit_recharge_entitlement_starts_from_payment_month() {
    let db_path = temp_db_path("linuxdo-recharge-payment-month");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-payment-month".to_string(),
            username: Some("payment_month".to_string()),
            name: Some("Payment Month".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let payment_month = start_of_local_month_utc_ts(Local::now());
    let previous_month = shift_local_month_start_utc_ts(payment_month, -1);
    let next_month = shift_local_month_start_utc_ts(payment_month, 1);
    let order = LinuxDoCreditRechargeOrder {
        out_trade_no: "ldc_payment_month".to_string(),
        user_id: user.user_id.clone(),
        status: LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: 1000,
        months: 2,
        money_cents: 10_000,
        quote_month_start: payment_month,
        final_money_cents: 10_000,
        final_hourly_delta: 20,
        final_daily_delta: 100,
        final_monthly_delta: 1000,
        month_end_clamp_applied: false,
        quote_snapshot_json: None,
        trade_no: None,
        payment_url: None,
        order_name: "Payment month recharge".to_string(),
        notify_payload: None,
        created_at: payment_month - 60,
        updated_at: payment_month - 60,
        paid_at: None,
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };
    proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .expect("create recharge order");
    proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-payment-month",
            "ok=1",
            payment_month + 60,
        )
        .await
        .expect("apply recharge payment");
    let audit = proxy
        .linuxdo_credit_recharge_admin_audit(&user.user_id)
        .await
        .expect("load recharge audit");
    let months: Vec<i64> = audit
        .entitlements
        .iter()
        .map(|entry| entry.month_start)
        .collect();
    assert!(!months.contains(&previous_month));
    assert!(months.contains(&payment_month));
    assert!(months.contains(&next_month));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_recharge_clamp_only_applies_to_current_month_entitlement() {
    let db_path = temp_db_path("linuxdo-recharge-clamp-schedule-entitlement");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_751_269_200);
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-clamp-schedule".to_string(),
            username: Some("clamp_schedule".to_string()),
            name: Some("Clamp Schedule".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");

    let quote_month_start = start_of_local_month_utc_ts(manual_clock.local_now());
    let quote = linuxdo_credit_recharge_quote(
        1000,
        2,
        LinuxDoCreditRechargePriceConfig::normal(),
        quote_month_start,
        manual_clock.now_ts(),
    )
    .expect("quote");
    assert!(quote.month_end_clamp_applied);
    assert!(quote.current_month_final_monthly_delta < quote.full_month_monthly_delta);
    assert_eq!(quote.schedule.len(), 2);

    let order = LinuxDoCreditRechargeOrder {
        out_trade_no: "ldc_clamp_schedule".to_string(),
        user_id: user.user_id.clone(),
        status: LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: 1000,
        months: 2,
        money_cents: quote.full_order_money_cents,
        quote_month_start,
        final_money_cents: quote.final_order_money_cents,
        final_hourly_delta: quote.current_month_final_hourly_delta,
        final_daily_delta: quote.current_month_final_daily_delta,
        final_monthly_delta: quote.current_month_final_monthly_delta,
        month_end_clamp_applied: true,
        quote_snapshot_json: Some(
            serde_json::to_string(&serde_json::json!({
                "version": 1,
                "source": "server_quote",
                "request": {
                    "credits": 1000,
                    "months": 2,
                },
                "quote": quote,
            }))
            .expect("serialize recharge quote snapshot"),
        ),
        trade_no: None,
        payment_url: None,
        order_name: "Clamp schedule recharge".to_string(),
        notify_payload: None,
        created_at: manual_clock.now_ts(),
        updated_at: manual_clock.now_ts(),
        paid_at: None,
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };
    proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .expect("create recharge order");
    proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-clamp-schedule",
            "ok=1",
            manual_clock.now_ts() + 60,
        )
        .await
        .expect("apply recharge payment");

    let audit = proxy
        .linuxdo_credit_recharge_admin_audit(&user.user_id)
        .await
        .expect("load recharge audit");
    assert_eq!(audit.entitlements.len(), 2);
    let current = audit
        .entitlements
        .iter()
        .find(|entry| entry.month_start == quote.schedule[0].month_start)
        .expect("current month entitlement");
    assert_eq!(current.credits, order.credits);
    assert_eq!(current.monthly_delta, quote.schedule[0].monthly_delta);
    let next = audit
        .entitlements
        .iter()
        .find(|entry| entry.month_start == quote.schedule[1].month_start)
        .expect("next month entitlement");
    assert_eq!(next.credits, order.credits);
    assert_eq!(next.monthly_delta, quote.schedule[1].monthly_delta);
    assert_eq!(next.monthly_delta, quote.full_month_monthly_delta);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_recharge_expired_orders_ignore_duplicate_success_callbacks() {
    let db_path = temp_db_path("linuxdo-recharge-expired-duplicate-notify");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_751_269_200);
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-expired-duplicate".to_string(),
            username: Some("expired_duplicate".to_string()),
            name: Some("Expired Duplicate".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");

    let quote_month_start = start_of_local_month_utc_ts(manual_clock.local_now());
    let quote = linuxdo_credit_recharge_quote(
        1000,
        1,
        LinuxDoCreditRechargePriceConfig::normal(),
        quote_month_start,
        manual_clock.now_ts(),
    )
    .expect("quote");
    let order = LinuxDoCreditRechargeOrder {
        out_trade_no: "ldc_expired_duplicate".to_string(),
        user_id: user.user_id.clone(),
        status: LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: 1000,
        months: 1,
        money_cents: quote.full_order_money_cents,
        quote_month_start,
        final_money_cents: quote.final_order_money_cents,
        final_hourly_delta: quote.current_month_final_hourly_delta,
        final_daily_delta: quote.current_month_final_daily_delta,
        final_monthly_delta: quote.current_month_final_monthly_delta,
        month_end_clamp_applied: quote.month_end_clamp_applied,
        quote_snapshot_json: Some(
            serde_json::to_string(&serde_json::json!({
                "version": 1,
                "source": "server_quote",
                "request": {
                    "credits": 1000,
                    "months": 1,
                },
                "quote": quote,
            }))
            .expect("serialize recharge quote snapshot"),
        ),
        trade_no: None,
        payment_url: None,
        order_name: "Expired duplicate recharge".to_string(),
        notify_payload: None,
        created_at: manual_clock.now_ts(),
        updated_at: manual_clock.now_ts(),
        paid_at: None,
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };
    proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .expect("create recharge order");

    let next_month = shift_local_month_start_utc_ts(quote_month_start, 1);
    let first_paid_at = next_month + 60;
    let expired = proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-expired-duplicate",
            "ok=1",
            first_paid_at,
        )
        .await
        .expect("expire recharge order");
    assert_eq!(expired.status, LINUXDO_CREDIT_RECHARGE_STATUS_EXPIRED);

    let retried = proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-expired-duplicate",
            "ok=2",
            first_paid_at + 120,
        )
        .await
        .expect("replay recharge notify");
    assert_eq!(retried.status, LINUXDO_CREDIT_RECHARGE_STATUS_EXPIRED);

    let audit = proxy
        .linuxdo_credit_recharge_admin_audit(&user.user_id)
        .await
        .expect("load recharge audit");
    assert!(audit.entitlements.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_admin_recharge_user_groups_are_paginated() {
    let db_path = temp_db_path("linuxdo-recharge-admin-group-pagination");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let now = Utc::now().timestamp();
    let mut user_ids = Vec::new();
    for index in 0..3 {
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: format!("linuxdo-recharge-admin-group-{index}"),
                username: Some(format!("group_user_{index}")),
                name: Some(format!("Group User {index}")),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert oauth user");
        user_ids.push(user.user_id.clone());
        proxy
            .create_linuxdo_credit_recharge_order(&LinuxDoCreditRechargeOrder {
                out_trade_no: format!("ldc_group_page_{index}"),
                user_id: user.user_id,
                status: LINUXDO_CREDIT_RECHARGE_STATUS_PAID.to_string(),
                credits: 1000,
                months: 1,
                money_cents: 5_000,
                quote_month_start: now,
                final_money_cents: 5_000,
                final_hourly_delta: 20,
                final_daily_delta: 100,
                final_monthly_delta: 1000,
                month_end_clamp_applied: false,
                quote_snapshot_json: None,
                trade_no: Some(format!("trade-group-page-{index}")),
                payment_url: None,
                order_name: "Grouped pagination recharge".to_string(),
                notify_payload: None,
                created_at: now - index,
                updated_at: now - index,
                paid_at: Some(now - index),
                refunded_at: None,
                refund_actor: None,
                refund_payload: None,
                last_notify_at: None,
                last_error: None,
            })
            .await
            .expect("create recharge order");
    }

    let query = LinuxDoCreditRechargeAdminListQuery {
        user_query: None,
        status: None,
        start_at: None,
        end_at: None,
        sort: "createdAt".to_string(),
        order: "desc".to_string(),
        page: 2,
        per_page: 1,
    };
    let total_groups = proxy
        .count_admin_linuxdo_credit_recharge_user_groups(&query)
        .await
        .expect("count grouped recharge users");
    assert_eq!(total_groups, 3);
    let groups = proxy
        .list_admin_linuxdo_credit_recharge_user_groups(&query)
        .await
        .expect("list grouped recharge users");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].user_id, user_ids[1]);
    assert_eq!(groups[0].order_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_refund_reservation_blocks_duplicate_refunds() {
    let db_path = temp_db_path("linuxdo-recharge-refund-reservation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-refund-reservation".to_string(),
            username: Some("refund_reservation".to_string()),
            name: Some("Refund Reservation".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let now = Utc::now().timestamp();
    let order = LinuxDoCreditRechargeOrder {
        out_trade_no: "ldc_refund_reservation".to_string(),
        user_id: user.user_id.clone(),
        status: LINUXDO_CREDIT_RECHARGE_STATUS_PAID.to_string(),
        credits: 1000,
        months: 1,
        money_cents: 5_000,
        quote_month_start: now,
        final_money_cents: 5_000,
        final_hourly_delta: 20,
        final_daily_delta: 100,
        final_monthly_delta: 1000,
        month_end_clamp_applied: false,
        quote_snapshot_json: None,
        trade_no: Some("trade-refund-reservation".to_string()),
        payment_url: None,
        order_name: "Refund reservation recharge".to_string(),
        notify_payload: None,
        created_at: now - 60,
        updated_at: now - 60,
        paid_at: Some(now - 30),
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };
    proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .expect("create recharge order");

    let reserved = proxy
        .reserve_linuxdo_credit_recharge_order_refund(&order.out_trade_no, now)
        .await
        .expect("reserve refund");
    assert_eq!(reserved.status, LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING);
    let duplicate = proxy
        .reserve_linuxdo_credit_recharge_order_refund(&order.out_trade_no, now + 1)
        .await
        .expect_err("duplicate refund reservation rejected");
    assert!(
        duplicate.to_string().contains("refunding"),
        "unexpected duplicate error: {duplicate}"
    );

    proxy
        .release_linuxdo_credit_recharge_order_refund_reservation(
            &order.out_trade_no,
            "refund endpoint unavailable",
            now + 2,
        )
        .await
        .expect("release reservation");
    let released = proxy
        .get_linuxdo_credit_recharge_order(&order.out_trade_no)
        .await
        .expect("read released order")
        .expect("order exists");
    assert_eq!(released.status, LINUXDO_CREDIT_RECHARGE_STATUS_PAID);
    assert_eq!(
        released.last_error.as_deref(),
        Some("refund endpoint unavailable")
    );

    proxy
        .reserve_linuxdo_credit_recharge_order_refund(&order.out_trade_no, now + 3)
        .await
        .expect("reserve refund again");
    let marked = proxy
        .mark_linuxdo_credit_recharge_order_refund_external_succeeded(
            &order.out_trade_no,
            "admin",
            "{\"phase\":\"externalSucceeded\",\"response\":{\"code\":1}}",
            now + 4,
        )
        .await
        .expect("mark external refund success");
    assert_eq!(marked.status, LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING);
    assert_eq!(marked.refund_actor.as_deref(), Some("admin"));
    assert_eq!(
        marked.last_error.as_deref(),
        Some("external refund succeeded; local finalize pending")
    );
    let refunded = proxy
        .refund_linuxdo_credit_recharge_order(
            &order.out_trade_no,
            LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED,
            "admin",
            "{\"code\":1}",
            now + 5,
            false,
        )
        .await
        .expect("complete refund");
    assert_eq!(refunded.status, LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED);

    let after_refund = proxy
        .reserve_linuxdo_credit_recharge_order_refund(&order.out_trade_no, now + 6)
        .await
        .expect_err("refunded order cannot be reserved again");
    assert!(
        after_refund.to_string().contains("refunded"),
        "unexpected refunded error: {after_refund}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_payment_callback_does_not_resurrect_refunded_order() {
    let db_path = temp_db_path("linuxdo-recharge-refund-no-resurrect");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-refund-no-resurrect".to_string(),
            username: Some("refund_no_resurrect".to_string()),
            name: Some("Refund No Resurrect".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let now = Utc::now().timestamp();
    let quote_month_start = start_of_local_month_utc_ts(Local::now());
    let order = LinuxDoCreditRechargeOrder {
        out_trade_no: "ldc_refund_no_resurrect".to_string(),
        user_id: user.user_id.clone(),
        status: LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: 1000,
        months: 1,
        money_cents: 5_000,
        quote_month_start,
        final_money_cents: 5_000,
        final_hourly_delta: 20,
        final_daily_delta: 100,
        final_monthly_delta: 1000,
        month_end_clamp_applied: false,
        quote_snapshot_json: None,
        trade_no: None,
        payment_url: None,
        order_name: "Refund no resurrect recharge".to_string(),
        notify_payload: None,
        created_at: now - 60,
        updated_at: now - 60,
        paid_at: None,
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };
    proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .expect("create recharge order");
    proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-refund-no-resurrect",
            "paid=1",
            now - 30,
        )
        .await
        .expect("apply payment");
    proxy
        .reserve_linuxdo_credit_recharge_order_refund(&order.out_trade_no, now)
        .await
        .expect("reserve refund");
    proxy
        .refund_linuxdo_credit_recharge_order(
            &order.out_trade_no,
            LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED,
            "admin",
            "{\"code\":1}",
            now + 1,
            true,
        )
        .await
        .expect("complete refund");

    let after_refund_audit = proxy
        .linuxdo_credit_recharge_admin_audit(&user.user_id)
        .await
        .expect("audit after refund");
    assert!(after_refund_audit.entitlements.is_empty());

    let replayed = proxy
        .apply_linuxdo_credit_recharge_payment(
            &order.out_trade_no,
            "trade-refund-no-resurrect",
            "paid=1&replay=1",
            now + 2,
        )
        .await
        .expect("replayed callback");
    assert_eq!(replayed.status, LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED);
    assert_eq!(replayed.notify_payload.as_deref(), Some("paid=1&replay=1"));
    let replayed_audit = proxy
        .linuxdo_credit_recharge_admin_audit(&user.user_id)
        .await
        .expect("audit after replay");
    assert!(replayed_audit.entitlements.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_credit_recharge_backfill_uses_historical_order_month() {
    let db_path = temp_db_path("linuxdo-recharge-backfill-historical-month");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-recharge-backfill-legacy".to_string(),
            username: Some("legacy_backfill".to_string()),
            name: Some("Legacy Backfill".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let created_at = 1_735_718_400;
    let paid_at = created_at + 3_600;
    let expected_quote_month_start = start_of_local_month_utc_ts(
        Utc.timestamp_opt(paid_at, 0)
            .single()
            .expect("paid_at timestamp")
            .with_timezone(&Local),
    );

    sqlx::query(
        r#"
        INSERT INTO linuxdo_credit_recharge_orders (
            out_trade_no, user_id, status, credits, months, money_cents,
            trade_no, payment_url, order_name, notify_payload, created_at, updated_at,
            paid_at, refunded_at, refund_actor, refund_payload, last_notify_at, last_error
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("ldc_legacy_backfill")
    .bind(&user.user_id)
    .bind(LINUXDO_CREDIT_RECHARGE_STATUS_PAID)
    .bind(1000_i64)
    .bind(1_i64)
    .bind(5_000_i64)
    .bind("trade-legacy-backfill")
    .bind(Option::<String>::None)
    .bind("Legacy recharge")
    .bind(Option::<String>::None)
    .bind(created_at)
    .bind(created_at)
    .bind(paid_at)
    .bind(Option::<i64>::None)
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind(Option::<i64>::None)
    .bind(Option::<String>::None)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert legacy recharge order");

    proxy
        .key_store
        .ensure_linuxdo_credit_recharge_schema()
        .await
        .expect("backfill recharge schema");
    let order = proxy
        .get_linuxdo_credit_recharge_order("ldc_legacy_backfill")
        .await
        .expect("load backfilled order")
        .expect("order exists");
    assert_eq!(order.quote_month_start, expected_quote_month_start);
    assert_eq!(order.final_money_cents, 5_000);
    assert!(!order.month_end_clamp_applied);

    let _ = std::fs::remove_file(db_path);
}
