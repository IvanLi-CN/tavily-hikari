use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn tavily_http_free_account_boundary_rejects_unsupported_public_api_surface() {
    let db_path = temp_db_path("http-free-account-boundary");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-free-account-boundary-key";
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/search",
            post({
                let upstream_hits = upstream_hits.clone();
                move || {
                    let upstream_hits = upstream_hits.clone();
                    async move {
                        upstream_hits.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({ "unexpected": "search hit" }))
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let upstream_hits = upstream_hits.clone();
                move || {
                    let upstream_hits = upstream_hits.clone();
                    async move {
                        upstream_hits.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({ "unexpected": "research hit" }))
                    }
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
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let access_token = proxy
        .create_access_token(Some("http-free-account-boundary"))
        .await
        .expect("create token");
    let proxy_addr = spawn_proxy_server(proxy.clone(), format!("http://{upstream_addr}")).await;
    let client = Client::new();

    let safe_search_resp = client
        .post(format!("http://{proxy_addr}/api/tavily/search"))
        .json(&serde_json::json!({
            "api_key": access_token.token,
            "query": "free account safe search",
            "safe_search": false
        }))
        .send()
        .await
        .expect("safe_search request");
    assert_eq!(safe_search_resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let safe_search_body: serde_json::Value =
        safe_search_resp.json().await.expect("safe_search error body");
    assert_eq!(
        safe_search_body["error"].as_str(),
        Some("invalid_request")
    );
    assert!(
        safe_search_body["message"]
            .as_str()
            .is_some_and(|message| message.contains("safe_search")),
        "safe_search rejection should name the unsupported parameter"
    );

    let stream_resp = client
        .post(format!("http://{proxy_addr}/api/tavily/research"))
        .json(&serde_json::json!({
            "api_key": access_token.token,
            "input": "streaming should be rejected",
            "stream": true
        }))
        .send()
        .await
        .expect("stream research request");
    assert_eq!(stream_resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let stream_body: serde_json::Value = stream_resp.json().await.expect("stream error body");
    assert_eq!(stream_body["error"].as_str(), Some("invalid_request"));
    assert!(
        stream_body["message"]
            .as_str()
            .is_some_and(|message| message.contains("stream=true")),
        "stream rejection should name stream=true"
    );

    let org_usage_resp = client
        .post(format!("http://{proxy_addr}/api/tavily/org-usage"))
        .json(&serde_json::json!({
            "api_key": access_token.token,
            "start_date": "2026-07-01",
            "end_date": "2026-07-07"
        }))
        .send()
        .await
        .expect("org usage request");
    assert_eq!(org_usage_resp.status(), reqwest::StatusCode::NOT_FOUND);
    assert_eq!(
        upstream_hits.load(Ordering::SeqCst),
        0,
        "unsupported free-account boundary requests must not hit upstream"
    );

    let verdict = proxy
        .peek_token_quota(&access_token.id)
        .await
        .expect("peek quota");
    assert_eq!(verdict.hourly_used, 0);

    let _ = std::fs::remove_file(db_path);
}
