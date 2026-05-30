#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminRechargeListQuery {
    user: Option<String>,
    status: Option<String>,
    start_at: Option<i64>,
    end_at: Option<i64>,
    sort: Option<String>,
    order: Option<String>,
    view: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRechargeOrderUserView {
    id: String,
    display_name: Option<String>,
    username: Option<String>,
    avatar_template: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRechargeOrderView {
    out_trade_no: String,
    user: AdminRechargeOrderUserView,
    status: String,
    credits: i64,
    months: i64,
    money_cents: i64,
    money: String,
    trade_no: Option<String>,
    payment_url: Option<String>,
    order_name: String,
    created_at: i64,
    updated_at: i64,
    paid_at: Option<i64>,
    refunded_at: Option<i64>,
    refund_actor: Option<String>,
    last_notify_at: Option<i64>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRechargeUserGroupView {
    user: AdminRechargeOrderUserView,
    order_count: i64,
    paid_order_count: i64,
    refunded_order_count: i64,
    total_credits: i64,
    total_money_cents: i64,
    latest_order_created_at: i64,
    latest_paid_at: Option<i64>,
    latest_refunded_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRechargeListResponse {
    has_recharge_orders: bool,
    items: Vec<AdminRechargeOrderView>,
    groups: Vec<AdminRechargeUserGroupView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminTotpCodePayload {
    totp_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminTotpConfirmPayload {
    secret: String,
    code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminTotpResetPayload {
    current_code: String,
    secret: String,
    code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminTotpStatusResponse {
    enabled: bool,
    available: bool,
    recharge_feature_enabled: bool,
    missing_crypto_key: bool,
    locked_until: Option<i64>,
    issuer: &'static str,
    account_name: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminTotpSetupResponse {
    secret: String,
    otp_auth_url: String,
    qr_png_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinuxDoCreditRefundResponse {
    code: i64,
    msg: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinuxDoCreditRefundExternalSuccessMarker {
    phase: String,
    next_status: String,
    revoke_entitlements: bool,
    refund_actor: String,
    response: String,
}

const LINUXDO_CREDIT_REFUND_EXTERNAL_SUCCEEDED_PHASE: &str = "externalSucceeded";

const ADMIN_TOTP_ISSUER: &str = "Tavily Hikari";
const ADMIN_TOTP_ACCOUNT: &str = "admin-recharge";
const ADMIN_TOTP_FAILURE_LOCK_THRESHOLD: i64 = 5;
const ADMIN_TOTP_LOCK_SECS: i64 = 300;

async fn get_admin_recharges(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AdminRechargeListQuery>,
) -> Result<Json<AdminRechargeListResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::UNAUTHORIZED, "admin required".to_string()));
    }
    let has_recharge_orders = state
        .proxy
        .has_linuxdo_credit_recharge_orders()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if !has_recharge_orders {
        return Ok(Json(AdminRechargeListResponse {
            has_recharge_orders,
            items: Vec::new(),
            groups: Vec::new(),
            total: 0,
            page: 1,
            per_page: query.per_page.unwrap_or(25).clamp(1, 100),
        }));
    }
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);
    let sort = query.sort.as_deref().unwrap_or("createdAt");
    let order = query.order.as_deref().unwrap_or("desc");
    let list_query = tavily_hikari::LinuxDoCreditRechargeAdminListQuery {
        user_query: query.user.clone(),
        status: query.status.clone(),
        start_at: query.start_at,
        end_at: query.end_at,
        sort: sort.to_string(),
        order: order.to_string(),
        page,
        per_page,
    };
    let (total, items, groups) = if query.view.as_deref() == Some("user") {
        let total = state
            .proxy
            .count_admin_linuxdo_credit_recharge_user_groups(&list_query)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        let groups = state
            .proxy
            .list_admin_linuxdo_credit_recharge_user_groups(&list_query)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .into_iter()
            .map(admin_recharge_group_view)
            .collect();
        (total, Vec::new(), groups)
    } else {
        let total = state
            .proxy
            .count_admin_linuxdo_credit_recharge_orders(&list_query)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        let items = state
            .proxy
            .list_admin_linuxdo_credit_recharge_orders(&list_query)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .into_iter()
            .map(admin_recharge_order_view)
            .collect();
        (total, items, Vec::new())
    };
    Ok(Json(AdminRechargeListResponse {
        has_recharge_orders,
        items,
        groups,
        total,
        page,
        per_page,
    }))
}

async fn post_admin_recharge_refund(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(out_trade_no): Path<String>,
    Json(payload): Json<AdminTotpCodePayload>,
) -> Result<Json<AdminRechargeOrderView>, (StatusCode, String)> {
    refund_admin_recharge_order(state, headers, out_trade_no, payload.totp_code, true).await
}

async fn post_admin_recharge_refund_only(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(out_trade_no): Path<String>,
    Json(payload): Json<AdminTotpCodePayload>,
) -> Result<Json<AdminRechargeOrderView>, (StatusCode, String)> {
    refund_admin_recharge_order(state, headers, out_trade_no, payload.totp_code, false).await
}

async fn get_admin_totp_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminTotpStatusResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::UNAUTHORIZED, "admin required".to_string()));
    }
    admin_totp_status_response(state.as_ref()).await.map(Json)
}

async fn post_admin_totp_setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminTotpSetupResponse>, (StatusCode, String)> {
    ensure_totp_management_allowed(state.as_ref(), &headers).await?;
    let secret = generate_totp_secret();
    let totp = build_totp(&secret)?;
    let otp_auth_url = totp.get_url();
    let qr_png_base64 = totp
        .get_qr_base64()
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(AdminTotpSetupResponse {
        secret,
        otp_auth_url,
        qr_png_base64,
    }))
}

async fn post_admin_totp_confirm(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminTotpConfirmPayload>,
) -> Result<Json<AdminTotpStatusResponse>, (StatusCode, String)> {
    ensure_totp_management_allowed(state.as_ref(), &headers).await?;
    if state
        .proxy
        .get_admin_totp_secret_record()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            "admin TOTP is already bound; use reset with current TOTP".to_string(),
        ));
    }
    if !check_totp_code(&payload.secret, &payload.code)? {
        return Err((StatusCode::BAD_REQUEST, "invalid TOTP code".to_string()));
    }
    let now = Utc::now().timestamp();
    let (ciphertext, nonce) = encrypt_admin_totp_secret(state.as_ref(), &payload.secret)?;
    state
        .proxy
        .set_admin_totp_secret_record(&ciphertext, &nonce, now)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    admin_totp_status_response(state.as_ref()).await.map(Json)
}

async fn post_admin_totp_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminTotpResetPayload>,
) -> Result<Json<AdminTotpStatusResponse>, (StatusCode, String)> {
    ensure_totp_management_allowed(state.as_ref(), &headers).await?;
    verify_admin_totp_for_sensitive_action(state.as_ref(), &payload.current_code).await?;
    if !check_totp_code(&payload.secret, &payload.code)? {
        return Err((StatusCode::BAD_REQUEST, "invalid new TOTP code".to_string()));
    }
    let now = Utc::now().timestamp();
    let (ciphertext, nonce) = encrypt_admin_totp_secret(state.as_ref(), &payload.secret)?;
    state
        .proxy
        .set_admin_totp_secret_record(&ciphertext, &nonce, now)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    admin_totp_status_response(state.as_ref()).await.map(Json)
}

async fn post_admin_totp_disable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminTotpCodePayload>,
) -> Result<Json<AdminTotpStatusResponse>, (StatusCode, String)> {
    ensure_totp_management_allowed(state.as_ref(), &headers).await?;
    verify_admin_totp_for_sensitive_action(state.as_ref(), &payload.totp_code).await?;
    state
        .proxy
        .clear_admin_totp_secret_record()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    admin_totp_status_response(state.as_ref()).await.map(Json)
}

async fn refund_admin_recharge_order(
    state: Arc<AppState>,
    headers: HeaderMap,
    out_trade_no: String,
    totp_code: String,
    revoke_entitlements: bool,
) -> Result<Json<AdminRechargeOrderView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::UNAUTHORIZED, "admin required".to_string()));
    }
    if state.dev_open_admin {
        return Err((
            StatusCode::FORBIDDEN,
            "DEV_OPEN_ADMIN cannot execute recharge refunds".to_string(),
        ));
    }
    verify_admin_totp_for_sensitive_action(state.as_ref(), &totp_code).await?;
    let actor = admin_maintenance_actor(state.as_ref(), &headers, None).await;
    let actor_display = actor
        .actor_display_name
        .or(actor.actor_user_id)
        .unwrap_or_else(|| "admin".to_string());
    let next_status = if revoke_entitlements {
        tavily_hikari::LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED
    } else {
        tavily_hikari::LINUXDO_CREDIT_RECHARGE_STATUS_REFUND_ONLY
    };
    let order = match state
        .proxy
        .reserve_linuxdo_credit_recharge_order_refund(&out_trade_no, Utc::now().timestamp())
        .await
    {
        Ok(order) => order,
        Err(err) => {
            let existing = state
                .proxy
                .get_linuxdo_credit_recharge_order(&out_trade_no)
                .await
                .map_err(|fetch_err| (StatusCode::INTERNAL_SERVER_ERROR, fetch_err.to_string()))?;
            if let Some(existing) = existing
                && existing.status == tavily_hikari::LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING
                && let Some(marker) =
                    decode_refund_external_success_marker(existing.refund_payload.as_deref())
            {
                return finalize_admin_refund_from_external_success(
                    state,
                    out_trade_no,
                    next_status,
                    revoke_entitlements,
                    marker,
                )
                .await;
            }
            return Err((StatusCode::CONFLICT, err.to_string()));
        }
    };
    let trade_no = order
        .trade_no
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| (StatusCode::CONFLICT, "recharge order has no trade number".to_string()))?;
    let refund_payload = match post_linuxdo_credit_full_refund(state.as_ref(), &order, trade_no).await {
        Ok(payload) => payload,
        Err(err) => {
            let message = err.1.clone();
            let _ = state
                .proxy
                .release_linuxdo_credit_recharge_order_refund_reservation(
                    &out_trade_no,
                    &message,
                    Utc::now().timestamp(),
                )
                .await;
            return Err(err);
        }
    };
    let marker = LinuxDoCreditRefundExternalSuccessMarker {
        phase: LINUXDO_CREDIT_REFUND_EXTERNAL_SUCCEEDED_PHASE.to_string(),
        next_status: next_status.to_string(),
        revoke_entitlements,
        refund_actor: actor_display.clone(),
        response: refund_payload,
    };
    let marker_result = persist_refund_external_success_marker_with_retry(
        state.as_ref(),
        &out_trade_no,
        &actor_display,
        &marker,
    )
    .await;
    let finalize_result = finalize_admin_refund_from_external_success_with_retry(
        state,
        out_trade_no,
        next_status,
        revoke_entitlements,
        marker,
    )
    .await;
    match (marker_result, finalize_result) {
        (_, Ok(response)) => Ok(response),
        (Ok(()), Err(err)) => Err(err),
        (Err(marker_err), Err(finalize_err)) => Err((
            finalize_err.0,
            format!("{}; success marker also failed: {}", finalize_err.1, marker_err.1),
        )),
    }
}

async fn finalize_admin_refund_from_external_success(
    state: Arc<AppState>,
    out_trade_no: String,
    next_status: &str,
    revoke_entitlements: bool,
    marker: LinuxDoCreditRefundExternalSuccessMarker,
) -> Result<Json<AdminRechargeOrderView>, (StatusCode, String)> {
    if marker.phase != LINUXDO_CREDIT_REFUND_EXTERNAL_SUCCEEDED_PHASE
        || marker.next_status != next_status
        || marker.revoke_entitlements != revoke_entitlements
    {
        return Err((
            StatusCode::CONFLICT,
            "recharge refund recovery intent does not match this endpoint".to_string(),
        ));
    }
    let updated = state
        .proxy
        .refund_linuxdo_credit_recharge_order(
            &out_trade_no,
            next_status,
            &marker.refund_actor,
            &marker.response,
            Utc::now().timestamp(),
            revoke_entitlements,
        )
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("external refund succeeded; local finalize pending: {err}"),
            )
        })?;
    Ok(Json(AdminRechargeOrderView {
        user: AdminRechargeOrderUserView {
            id: updated.user_id.clone(),
            display_name: None,
            username: None,
            avatar_template: None,
        },
        out_trade_no: updated.out_trade_no,
        status: updated.status,
        credits: updated.credits,
        months: updated.months,
        money_cents: updated.money_cents,
        money: tavily_hikari::format_linuxdo_credit_money(updated.money_cents),
        trade_no: updated.trade_no,
        payment_url: updated.payment_url,
        order_name: updated.order_name,
        created_at: updated.created_at,
        updated_at: updated.updated_at,
        paid_at: updated.paid_at,
        refunded_at: updated.refunded_at,
        refund_actor: updated.refund_actor,
        last_notify_at: updated.last_notify_at,
        last_error: updated.last_error,
    }))
}

async fn finalize_admin_refund_from_external_success_with_retry(
    state: Arc<AppState>,
    out_trade_no: String,
    next_status: &str,
    revoke_entitlements: bool,
    marker: LinuxDoCreditRefundExternalSuccessMarker,
) -> Result<Json<AdminRechargeOrderView>, (StatusCode, String)> {
    let mut last_error = None;
    for delay_ms in [0, 50, 200] {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        match finalize_admin_refund_from_external_success(
            state.clone(),
            out_trade_no.clone(),
            next_status,
            revoke_entitlements,
            marker.clone(),
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "external refund succeeded; local finalize pending".to_string(),
    )))
}

async fn persist_refund_external_success_marker_with_retry(
    state: &AppState,
    out_trade_no: &str,
    actor_display: &str,
    marker: &LinuxDoCreditRefundExternalSuccessMarker,
) -> Result<(), (StatusCode, String)> {
    let marker_payload = serde_json::to_string(marker).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to encode refund success marker: {err}"),
        )
    })?;
    let mut last_error = None;
    for delay_ms in [0, 50, 200] {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        match state
            .proxy
            .mark_linuxdo_credit_recharge_order_refund_external_succeeded(
                out_trade_no,
                actor_display,
                &marker_payload,
                Utc::now().timestamp(),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }
    Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        format!(
            "external refund succeeded but local success marker failed: {}",
            last_error
                .map(|err| err.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ),
    ))
}

fn decode_refund_external_success_marker(
    payload: Option<&str>,
) -> Option<LinuxDoCreditRefundExternalSuccessMarker> {
    let marker = serde_json::from_str::<LinuxDoCreditRefundExternalSuccessMarker>(payload?).ok()?;
    (marker.phase == LINUXDO_CREDIT_REFUND_EXTERNAL_SUCCEEDED_PHASE).then_some(marker)
}

async fn post_linuxdo_credit_full_refund(
    state: &AppState,
    order: &tavily_hikari::LinuxDoCreditRechargeOrder,
    trade_no: &str,
) -> Result<String, (StatusCode, String)> {
    let client_id = state
        .linuxdo_credit
        .client_id
        .as_deref()
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "Linux.do Credit client id missing".to_string()))?;
    let client_secret = state
        .linuxdo_credit
        .client_secret
        .as_deref()
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "Linux.do Credit client secret missing".to_string()))?;
    let endpoint = linuxdo_credit_refund_url(&state.linuxdo_credit.submit_url)?;
    let money = tavily_hikari::format_linuxdo_credit_money(order.money_cents);
    let params = linuxdo_credit_refund_params(client_id, client_secret, trade_no, &order.out_trade_no, &money);
    let response = reqwest::Client::new()
        .post(endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;
    if !status.is_success() {
        return Err((StatusCode::BAD_GATEWAY, format!("refund endpoint returned {status}: {text}")));
    }
    let parsed: LinuxDoCreditRefundResponse = serde_json::from_str(&text)
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("invalid refund response: {err}")))?;
    if parsed.code != 1 {
        return Err((
            StatusCode::BAD_GATEWAY,
            parsed.msg.unwrap_or_else(|| "refund failed".to_string()),
        ));
    }
    Ok(text)
}

fn linuxdo_credit_refund_params(
    client_id: &str,
    client_secret: &str,
    trade_no: &str,
    out_trade_no: &str,
    money: &str,
) -> [(&'static str, String); 6] {
    [
        ("act", "refund".to_string()),
        ("pid", client_id.to_string()),
        ("key", client_secret.to_string()),
        ("trade_no", trade_no.to_string()),
        ("out_trade_no", out_trade_no.to_string()),
        ("money", money.to_string()),
    ]
}

fn linuxdo_credit_refund_url(submit_url: &str) -> Result<String, (StatusCode, String)> {
    if submit_url.ends_with("/epay/pay/submit.php") {
        return Ok(submit_url.replace("/epay/pay/submit.php", "/epay/api.php"));
    }
    if submit_url.ends_with("/pay/submit.php") {
        return Ok(submit_url.replace("/pay/submit.php", "/api.php"));
    }
    if let Some((base, _)) = submit_url.rsplit_once("/pay/") {
        return Ok(format!("{base}/api.php"));
    }
    Err((
        StatusCode::SERVICE_UNAVAILABLE,
        "Linux.do Credit refund endpoint cannot be derived from submit URL".to_string(),
    ))
}

async fn ensure_totp_management_allowed(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, String)> {
    if !is_admin_request(state, headers) {
        return Err((StatusCode::UNAUTHORIZED, "admin required".to_string()));
    }
    if state.dev_open_admin {
        return Err((
            StatusCode::FORBIDDEN,
            "DEV_OPEN_ADMIN cannot manage recharge TOTP".to_string(),
        ));
    }
    let settings = state
        .proxy
        .get_system_settings()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if !settings.recharge_feature_enabled {
        return Err((StatusCode::CONFLICT, "recharge feature is disabled".to_string()));
    }
    if !state.linuxdo_oauth.has_refresh_token_crypt_key() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY is required".to_string(),
        ));
    }
    Ok(())
}

async fn admin_totp_status_response(
    state: &AppState,
) -> Result<AdminTotpStatusResponse, (StatusCode, String)> {
    let settings = state
        .proxy
        .get_system_settings()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let secret_record = state
        .proxy
        .get_admin_totp_secret_record()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let (_, locked_until) = state
        .proxy
        .get_admin_totp_failure_state()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(AdminTotpStatusResponse {
        enabled: secret_record.is_some(),
        available: settings.recharge_feature_enabled
            && state.linuxdo_oauth.has_refresh_token_crypt_key()
            && !state.dev_open_admin,
        recharge_feature_enabled: settings.recharge_feature_enabled,
        missing_crypto_key: !state.linuxdo_oauth.has_refresh_token_crypt_key(),
        locked_until: (locked_until > Utc::now().timestamp()).then_some(locked_until),
        issuer: ADMIN_TOTP_ISSUER,
        account_name: ADMIN_TOTP_ACCOUNT,
    })
}

async fn verify_admin_totp_for_sensitive_action(
    state: &AppState,
    code: &str,
) -> Result<(), (StatusCode, String)> {
    if state.dev_open_admin {
        return Err((
            StatusCode::FORBIDDEN,
            "DEV_OPEN_ADMIN cannot execute sensitive recharge actions".to_string(),
        ));
    }
    let now = Utc::now().timestamp();
    let (fail_count, locked_until) = state
        .proxy
        .get_admin_totp_failure_state()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if locked_until > now {
        return Err((StatusCode::TOO_MANY_REQUESTS, "TOTP is temporarily locked".to_string()));
    }
    let Some((ciphertext, nonce, _)) = state
        .proxy
        .get_admin_totp_secret_record()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    else {
        return Err((StatusCode::FORBIDDEN, "admin TOTP is not bound".to_string()));
    };
    let secret = decrypt_admin_totp_secret(state, &ciphertext, &nonce)?;
    if check_totp_code(&secret, code)? {
        state
            .proxy
            .clear_admin_totp_failures()
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        return Ok(());
    }
    let next_count = fail_count + 1;
    let next_locked_until = if next_count >= ADMIN_TOTP_FAILURE_LOCK_THRESHOLD {
        now + ADMIN_TOTP_LOCK_SECS
    } else {
        0
    };
    state
        .proxy
        .set_admin_totp_failure_state(next_count, next_locked_until)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Err((StatusCode::FORBIDDEN, "invalid TOTP code".to_string()))
}

fn generate_totp_secret() -> String {
    use rand::RngCore as _;

    let mut bytes = [0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    data_encoding::BASE32_NOPAD.encode(&bytes)
}

fn build_totp(secret: &str) -> Result<totp_rs::TOTP, (StatusCode, String)> {
    let secret_bytes = totp_rs::Secret::Encoded(secret.to_string())
        .to_bytes()
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some(ADMIN_TOTP_ISSUER.to_string()),
        ADMIN_TOTP_ACCOUNT.to_string(),
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

fn check_totp_code(secret: &str, code: &str) -> Result<bool, (StatusCode, String)> {
    let normalized = code.trim();
    if normalized.len() != 6 || !normalized.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(false);
    }
    let totp = build_totp(secret)?;
    Ok(totp.check(normalized, Utc::now().timestamp() as u64))
}

fn encrypt_admin_totp_secret(
    state: &AppState,
    secret: &str,
) -> Result<(String, String), (StatusCode, String)> {
    use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

    let key_bytes = state.linuxdo_oauth.refresh_token_crypt_key().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY is required".to_string(),
        )
    })?;
    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key_bytes)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "invalid crypt key".to_string()))?;
    let key = LessSafeKey::new(unbound_key);
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill(&mut nonce_bytes);
    let mut in_out = secret.trim().as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::empty(),
        &mut in_out,
    )
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "failed to encrypt TOTP secret".to_string()))?;
    Ok((
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(in_out),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce_bytes),
    ))
}

fn decrypt_admin_totp_secret(
    state: &AppState,
    ciphertext: &str,
    nonce: &str,
) -> Result<String, (StatusCode, String)> {
    use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

    let key_bytes = state.linuxdo_oauth.refresh_token_crypt_key().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY is required".to_string(),
        )
    })?;
    let mut ciphertext_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(ciphertext)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let nonce_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(nonce)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let nonce_bytes: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "invalid nonce length".to_string()))?;
    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key_bytes)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "invalid crypt key".to_string()))?;
    let key = LessSafeKey::new(unbound_key);
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut ciphertext_bytes,
        )
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "failed to decrypt TOTP secret".to_string()))?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

fn admin_recharge_order_view(
    item: tavily_hikari::LinuxDoCreditRechargeAdminOrder,
) -> AdminRechargeOrderView {
    AdminRechargeOrderView {
        user: AdminRechargeOrderUserView {
            id: item.order.user_id.clone(),
            display_name: item.user_display_name,
            username: item.user_username,
            avatar_template: item.user_avatar_template,
        },
        out_trade_no: item.order.out_trade_no,
        status: item.order.status,
        credits: item.order.credits,
        months: item.order.months,
        money_cents: item.order.money_cents,
        money: tavily_hikari::format_linuxdo_credit_money(item.order.money_cents),
        trade_no: item.order.trade_no,
        payment_url: item.order.payment_url,
        order_name: item.order.order_name,
        created_at: item.order.created_at,
        updated_at: item.order.updated_at,
        paid_at: item.order.paid_at,
        refunded_at: item.order.refunded_at,
        refund_actor: item.order.refund_actor,
        last_notify_at: item.order.last_notify_at,
        last_error: item.order.last_error,
    }
}

fn admin_recharge_group_view(
    item: tavily_hikari::LinuxDoCreditRechargeAdminUserGroup,
) -> AdminRechargeUserGroupView {
    AdminRechargeUserGroupView {
        user: AdminRechargeOrderUserView {
            id: item.user_id,
            display_name: item.user_display_name,
            username: item.user_username,
            avatar_template: item.user_avatar_template,
        },
        order_count: item.order_count,
        paid_order_count: item.paid_order_count,
        refunded_order_count: item.refunded_order_count,
        total_credits: item.total_credits,
        total_money_cents: item.total_money_cents,
        latest_order_created_at: item.latest_order_created_at,
        latest_paid_at: item.latest_paid_at,
        latest_refunded_at: item.latest_refunded_at,
    }
}
