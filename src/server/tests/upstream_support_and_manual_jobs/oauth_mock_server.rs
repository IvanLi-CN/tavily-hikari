pub(super) async fn spawn_linuxdo_oauth_mock_server_with_behavior(
    behavior: LinuxDoOauthMockBehavior,
) -> SocketAddr {
    let app = Router::new()
        .route(
            "/oauth2/token",
            post({
                let behavior = behavior.clone();
                move |Form(form): Form<HashMap<String, String>>| {
                    let behavior = behavior.clone();
                    async move {
                        match form.get("grant_type").map(String::as_str) {
                            Some("authorization_code") => {
                                let mut payload = json!({
                                    "access_token": behavior.authorization_access_token,
                                });
                                if let Some(refresh_token) =
                                    behavior.authorization_refresh_token.as_deref()
                                {
                                    payload["refresh_token"] = json!(refresh_token);
                                }
                                (StatusCode::OK, Json(payload))
                            }
                            Some("refresh_token") => {
                                if let Some((status, payload)) = behavior.refresh_error.clone() {
                                    return (status, Json(payload));
                                }
                                let mut payload = json!({
                                    "access_token": behavior.refresh_access_token,
                                });
                                if let Some(refresh_token) =
                                    behavior.refresh_refresh_token.as_deref()
                                {
                                    payload["refresh_token"] = json!(refresh_token);
                                }
                                (StatusCode::OK, Json(payload))
                            }
                            _ => (
                                StatusCode::BAD_REQUEST,
                                Json(json!({ "error": "unsupported_grant_type" })),
                            ),
                        }
                    }
                }
            }),
        )
        .route(
            "/api/user",
            get({
                let behavior = behavior.clone();
                move |headers: HeaderMap| {
                    let behavior = behavior.clone();
                    async move {
                        let authorization = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok());
                        let auth_expected =
                            format!("Bearer {}", behavior.authorization_access_token);
                        let refresh_expected = format!("Bearer {}", behavior.refresh_access_token);
                        if authorization == Some(auth_expected.as_str()) {
                            return (StatusCode::OK, Json(behavior.authorization_profile));
                        }
                        if authorization == Some(refresh_expected.as_str()) {
                            return (StatusCode::OK, Json(behavior.refresh_profile));
                        }
                        (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({ "error": "invalid_token" })),
                        )
                    }
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}
