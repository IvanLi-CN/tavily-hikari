#[test]
fn linuxdo_credit_recharge_adds_hourly_daily_and_monthly_quota() {
    let base = AccountQuotaLimits {
        hourly_any_limit: 10,
        hourly_limit: 20,
        daily_limit: 30,
        monthly_limit: 40,
        inherits_defaults: false,
    };
    let resolution = build_account_quota_resolution_with_recharge(
        base,
        Vec::new(),
        linuxdo_credit_recharge_quota_delta(2000),
    );

    assert_eq!(resolution.effective.hourly_any_limit, 10);
    assert_eq!(resolution.effective.hourly_limit, 60);
    assert_eq!(resolution.effective.daily_limit, 230);
    assert_eq!(resolution.effective.monthly_limit, 2040);
    let recharge = resolution
        .breakdown
        .iter()
        .find(|entry| entry.kind == "recharge")
        .expect("recharge row");
    assert_eq!(recharge.hourly_delta, 40);
    assert_eq!(recharge.daily_delta, 200);
    assert_eq!(recharge.monthly_delta, 2000);
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
    assert_eq!(
        linuxdo_credit_recharge_money_cents(21_000, 1, normal),
        None
    );
    assert_eq!(
        linuxdo_credit_recharge_money_cents(1000, 13, normal),
        None
    );

    let test = LinuxDoCreditRechargePriceConfig::test_price();
    assert_eq!(
        linuxdo_credit_recharge_money_cents(1, 1, test),
        Some(100)
    );
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
