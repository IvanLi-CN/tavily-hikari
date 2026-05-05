async fn spawn_mock_mcp_upstream_for_tavily_search_failed_status_string(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );

                    assert_eq!(
                        body.get("method").and_then(|v| v.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call"
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str()),
                        Some("tavily-search"),
                        "expected tavily-search tool call"
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("arguments"))
                            .and_then(|a| a.get("include_usage"))
                            .and_then(|v| v.as_bool()),
                        None,
                        "proxy should not inject include_usage for MCP tools"
                    );

                    let args = body
                        .get("params")
                        .and_then(|p| p.get("arguments"))
                        .unwrap_or(&Value::Null);
                    let depth = args
                        .get("search_depth")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let credits = if depth.eq_ignore_ascii_case("advanced") {
                        2
                    } else {
                        1
                    };

                    // Simulate "HTTP 200 but structured failure" with a string `status`
                    // inside the JSON-RPC structuredContent envelope.
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                            "result": {
                                "structuredContent": {
                                    "status": "failed",
                                    "usage": { "credits": credits },
                                    "message": "mock structured failure",
                                }
                            }
                        })),
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
    (addr, hits)
}

async fn spawn_mock_mcp_upstream_for_unknown_tavily_tool(
    expected_api_key: String,
    tool_name: &'static str,
    credits: i64,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );

                    assert_eq!(
                        body.get("method").and_then(|v| v.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call"
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str()),
                        Some(tool_name),
                        "expected {} tool call",
                        tool_name
                    );

                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("arguments"))
                            .and_then(|a| a.get("include_usage"))
                            .and_then(|v| v.as_bool()),
                        None,
                        "proxy should not inject include_usage for unsupported Tavily tools"
                    );

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                            "result": {
                                "structuredContent": {
                                    "status": 200,
                                    "usage": { "credits": credits },
                                }
                            }
                        })),
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
    (addr, hits)
}

async fn spawn_mock_mcp_upstream_for_tavily_non_search_tools(
    expected_api_key: String,
    extract_credits: i64,
    crawl_credits: i64,
    map_credits: i64,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );

                    assert_eq!(
                        body.get("method").and_then(|v| v.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call"
                    );

                    let tool = body
                        .get("params")
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    assert!(
                        matches!(tool, "tavily-extract" | "tavily-crawl" | "tavily-map"),
                        "unexpected tool name: {tool}"
                    );

                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("arguments"))
                            .and_then(|a| a.get("include_usage"))
                            .and_then(|v| v.as_bool()),
                        None,
                        "proxy should not inject include_usage for MCP tools"
                    );

                    let credits = match tool {
                        "tavily-extract" => extract_credits,
                        "tavily-crawl" => crawl_credits,
                        "tavily-map" => map_credits,
                        _ => 0,
                    };

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                            "result": {
                                "structuredContent": {
                                    "status": 200,
                                    "usage": { "credits": credits },
                                }
                            }
                        })),
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
    (addr, hits)
}

async fn spawn_mock_mcp_upstream_for_tavily_extract_without_usage(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );

                    assert_eq!(
                        body.get("method").and_then(|v| v.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call"
                    );

                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str()),
                        Some("tavily-extract"),
                        "expected tavily-extract tool call"
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|p| p.get("arguments"))
                            .and_then(|a| a.get("include_usage"))
                            .and_then(|v| v.as_bool()),
                        None,
                        "proxy should not inject include_usage for MCP tools"
                    );

                    // Intentionally omit `usage.credits` to validate that non-search tools
                    // skip billing when usage is missing (we do not guess unpredictable costs).
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                            "result": {
                                "structuredContent": {
                                    "status": 200,
                                    "results": [],
                                }
                            }
                        })),
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
    (addr, hits)
}

fn assert_upstream_json_auth(
    headers: &HeaderMap,
    body: &Value,
    expected_api_key: &str,
    endpoint: &str,
) {
    let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        api_key, expected_api_key,
        "upstream api_key for {endpoint} should use Tavily key from pool"
    );
    assert!(
        !api_key.starts_with("th-"),
        "upstream {endpoint} api_key must not be Hikari token"
    );

    if matches!(endpoint, "/search" | "/extract" | "/crawl" | "/map") {
        assert_eq!(
            body.get("include_usage").and_then(|v| v.as_bool()),
            Some(true),
            "upstream {endpoint} should be forced to include usage"
        );
    }

    assert_upstream_bearer_auth(headers, expected_api_key, endpoint);
}

fn assert_upstream_bearer_auth(headers: &HeaderMap, expected_api_key: &str, endpoint: &str) {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected_auth = format!("Bearer {}", expected_api_key);
    assert_eq!(
        authorization, expected_auth,
        "upstream Authorization for {endpoint} should use Tavily key"
    );
    assert!(
        !authorization.starts_with("Bearer th-"),
        "upstream Authorization for {endpoint} must not use Hikari token"
    );
}

async fn spawn_http_search_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                async move {
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
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

type SeenUpstreamIdentity = Arc<Mutex<Vec<(String, Option<String>)>>>;

async fn spawn_http_search_mock_recording_upstream_identity(
    seen: SeenUpstreamIdentity,
) -> SocketAddr {
    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let seen = seen.clone();
                async move {
                    let api_key = body
                        .get("api_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let authorization = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("Bearer "))
                        .unwrap_or("")
                        .to_string();
                    assert_eq!(
                        api_key, authorization,
                        "upstream JSON/body api_key and Authorization should match",
                    );

                    let project_id = headers
                        .get("x-project-id")
                        .and_then(|value| value.to_str().ok())
                        .map(|value| value.to_string());
                    seen.lock()
                        .expect("upstream identity lock should not be poisoned")
                        .push((api_key, project_id));

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                            "usage": { "credits": 1 },
                        })),
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

#[derive(Debug, Clone)]
struct RecordedRebalanceGatewayCall {
    path: String,
    headers: HeaderMap,
    body: Value,
}

type RecordedRebalanceGatewayCalls = Arc<Mutex<Vec<RecordedRebalanceGatewayCall>>>;

async fn spawn_rebalance_gateway_mock(
    expected_api_key: String,
    seen: RecordedRebalanceGatewayCalls,
) -> SocketAddr {
    let seen_for_mcp = seen.clone();
    let seen_for_search = seen.clone();
    let app = Router::new()
        .route(
            "/mcp",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let seen = seen_for_mcp.clone();
                    async move {
                        seen.lock().expect("rebalance gateway mcp calls lock").push(
                            RecordedRebalanceGatewayCall {
                                path: "/mcp".to_string(),
                                headers,
                                body,
                            },
                        );
                        (
                            StatusCode::OK,
                            Json(json!({
                                "jsonrpc": "2.0",
                                "id": "unexpected-upstream-mcp",
                                "result": { "ok": true }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/search",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let seen = seen_for_search.clone();
                    async move {
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");
                        seen.lock()
                            .expect("rebalance gateway search calls lock")
                            .push(RecordedRebalanceGatewayCall {
                                path: "/search".to_string(),
                                headers: headers.clone(),
                                body: body.clone(),
                            });
                        (
                            StatusCode::OK,
                            Json(json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": 1 }
                            })),
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

async fn spawn_rebalance_gateway_http_error_mock(
    expected_api_key: String,
    seen: RecordedRebalanceGatewayCalls,
    status: StatusCode,
    body: Value,
) -> SocketAddr {
    let seen_for_search = seen.clone();
    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(request_body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let seen = seen_for_search.clone();
                let response_body = body.clone();
                async move {
                    assert_upstream_json_auth(
                        &headers,
                        &request_body,
                        &expected_api_key,
                        "/search",
                    );
                    seen.lock()
                        .expect("rebalance gateway error calls lock")
                        .push(RecordedRebalanceGatewayCall {
                            path: "/search".to_string(),
                            headers: headers.clone(),
                            body: request_body.clone(),
                        });
                    (status, Json(response_body))
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

async fn spawn_http_search_mock_with_usage(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/search",
        post({
            let hits = hits.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                    let search_depth = body
                        .get("search_depth")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                        2
                    } else {
                        1
                    };

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                            "usage": { "credits": credits },
                        })),
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
    (addr, hits)
}

async fn spawn_http_search_mock_with_usage_delayed(
    expected_api_key: String,
    arrived: Arc<Notify>,
    release: Arc<Notify>,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/search",
        post({
            let hits = hits.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                let arrived = arrived.clone();
                let release = release.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                    let search_depth = body
                        .get("search_depth")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                        2
                    } else {
                        1
                    };

                    arrived.notify_one();
                    release.notified().await;

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                            "usage": { "credits": credits },
                        })),
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
    (addr, hits)
}

async fn spawn_http_search_mock_without_usage(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/search",
        post({
            let hits = hits.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");
                    // Intentionally omit `usage.credits` to exercise the handler-side
                    // fallback to expected cost (based on request search_depth).
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
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
    (addr, hits)
}

async fn spawn_http_search_mock_with_usage_and_failed_status(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/search",
        post({
            let hits = hits.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                    let search_depth = body
                        .get("search_depth")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                        2
                    } else {
                        1
                    };

                    // Simulate "HTTP 200 but structured failure" so AttemptAnalysis.status != "success".
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": "failed",
                            "results": [],
                            "usage": { "credits": credits },
                            "message": "mock structured failure",
                        })),
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
    (addr, hits)
}

async fn spawn_http_json_endpoints_mock_with_usage(
    expected_api_key: String,
    extract_credits: i64,
    crawl_credits: i64,
    map_credits: i64,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let expected_api_key_extract = expected_api_key.clone();
    let expected_api_key_crawl = expected_api_key.clone();
    let expected_api_key_map = expected_api_key;
    let app = Router::new()
        .route(
            "/extract",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_extract.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/extract");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": extract_credits },
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/crawl",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_crawl.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/crawl");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": crawl_credits },
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/map",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_map.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": map_credits },
                            })),
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
    (addr, hits)
}

async fn spawn_http_extract_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
    let app = Router::new().route(
        "/extract",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                async move {
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/extract");
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
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

async fn spawn_http_crawl_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
    let app = Router::new().route(
        "/crawl",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                async move {
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/crawl");
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
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

async fn spawn_http_map_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
    let app = Router::new().route(
        "/map",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                async move {
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
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

async fn spawn_http_map_mock_returning_500(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/map",
        post({
            let hits = hits.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Body::from("mock map upstream error"),
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
    (addr, hits)
}

async fn spawn_http_research_mock_with_usage_diff(
    expected_api_key: String,
    base_research_usage: i64,
    delta: i64,
) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let research_calls = Arc::new(AtomicUsize::new(0));
    let expected_api_key_usage = expected_api_key.clone();
    let expected_api_key_research = expected_api_key;
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move |headers: HeaderMap| {
                    let expected_api_key = expected_api_key_usage.clone();
                    let usage_calls = usage_calls.clone();
                    async move {
                        let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                        assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                        // First call: base, second call: base + delta.
                        let research_usage = if call_index <= 1 {
                            base_research_usage
                        } else {
                            base_research_usage + delta
                        };
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "key": { "research_usage": research_usage }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let research_calls = research_calls.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_research.clone();
                    let research_calls = research_calls.clone();
                    async move {
                        research_calls.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": "mock-research-request",
                                "status": "pending",
                            })),
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
    (addr, usage_calls, research_calls)
}

async fn spawn_http_research_mock_with_usage_diff_string_float(
    expected_api_key: String,
    base_research_usage: i64,
    delta: i64,
) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let research_calls = Arc::new(AtomicUsize::new(0));
    let expected_api_key_usage = expected_api_key.clone();
    let expected_api_key_research = expected_api_key;
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move |headers: HeaderMap| {
                    let expected_api_key = expected_api_key_usage.clone();
                    let usage_calls = usage_calls.clone();
                    async move {
                        let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                        assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                        let research_usage = if call_index <= 1 {
                            base_research_usage
                        } else {
                            base_research_usage + delta
                        };
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "key": { "research_usage": format!("{research_usage}.0") }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let research_calls = research_calls.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_research.clone();
                    let research_calls = research_calls.clone();
                    async move {
                        research_calls.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": "mock-research-request",
                                "status": "pending",
                            })),
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
    (addr, usage_calls, research_calls)
}

async fn spawn_http_research_mock_with_usage_probe_failure(
    expected_api_key: String,
) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let research_calls = Arc::new(AtomicUsize::new(0));
    let expected_api_key_usage = expected_api_key.clone();
    let expected_api_key_research = expected_api_key;
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move |headers: HeaderMap| {
                    let expected_api_key = expected_api_key_usage.clone();
                    let usage_calls = usage_calls.clone();
                    async move {
                        usage_calls.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Body::from("mock usage probe failure"),
                        )
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let research_calls = research_calls.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_research.clone();
                    let research_calls = research_calls.clone();
                    async move {
                        research_calls.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": "mock-research-request",
                                "status": "pending",
                            })),
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
    (addr, usage_calls, research_calls)
}

async fn spawn_http_research_mock_with_follow_up_usage_probe_failure(
    expected_api_key: String,
    base_research_usage: i64,
) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let research_calls = Arc::new(AtomicUsize::new(0));
    let expected_api_key_usage = expected_api_key.clone();
    let expected_api_key_research = expected_api_key;
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move |headers: HeaderMap| {
                    let expected_api_key = expected_api_key_usage.clone();
                    let usage_calls = usage_calls.clone();
                    async move {
                        let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                        assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                        if call_index == 1 {
                            (
                                StatusCode::OK,
                                Body::from(
                                    serde_json::json!({
                                        "key": { "research_usage": base_research_usage }
                                    })
                                    .to_string(),
                                ),
                            )
                        } else {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Body::from("mock follow-up usage probe failure"),
                            )
                        }
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let research_calls = research_calls.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key_research.clone();
                    let research_calls = research_calls.clone();
                    async move {
                        research_calls.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": "mock-research-request",
                                "status": "pending",
                            })),
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
    (addr, usage_calls, research_calls)
}

async fn spawn_http_research_result_mock_asserting_bearer_at_path(
    expected_api_key: String,
    expected_request_id: String,
    route_path: &str,
) -> SocketAddr {
    let route_path = route_path.to_string();
    let app = Router::new().route(
        route_path.as_str(),
        get({
            move |headers: HeaderMap, Path(request_id): Path<String>| {
                let expected_api_key = expected_api_key.clone();
                let expected_request_id = expected_request_id.clone();
                async move {
                    assert_eq!(
                        request_id, expected_request_id,
                        "upstream research result path should contain the request id"
                    );
                    assert_upstream_bearer_auth(
                        &headers,
                        &expected_api_key,
                        "/research/:request_id",
                    );
                    assert!(
                        headers.get("x-hikari-routing-key").is_none(),
                        "internal Hikari routing key must not be forwarded upstream"
                    );
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "request_id": request_id,
                            "status": "pending",
                        })),
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

async fn spawn_http_research_result_mock_asserting_bearer(
    expected_api_key: String,
    expected_request_id: String,
) -> SocketAddr {
    spawn_http_research_result_mock_asserting_bearer_at_path(
        expected_api_key,
        expected_request_id,
        "/research/:request_id",
    )
    .await
}

async fn spawn_http_research_mock_requiring_same_key_for_result() -> SocketAddr {
    let request_key_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move |headers: HeaderMap| {
                    let usage_calls = usage_calls.clone();
                    async move {
                        let api_key = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("Bearer "))
                            .unwrap_or("")
                            .to_string();
                        assert!(
                            !api_key.is_empty(),
                            "upstream Authorization for /usage should include bearer key"
                        );
                        let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                        let research_usage = if call_index <= 1 { 10 } else { 14 };
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "key": { "research_usage": research_usage }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let request_key_map = request_key_map.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let request_key_map = request_key_map.clone();
                    async move {
                        let api_key = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("Bearer "))
                            .unwrap_or("")
                            .to_string();
                        assert!(
                            !api_key.is_empty(),
                            "upstream Authorization for /research should include bearer key"
                        );
                        let request_id = body
                            .get("input")
                            .and_then(|v| v.as_str())
                            .map(|v| format!("req-{v}"))
                            .unwrap_or_else(|| "req-same-key".to_string());
                        {
                            let mut guard = request_key_map
                                .lock()
                                .expect("request key map lock should not be poisoned");
                            guard.insert(request_id.clone(), api_key);
                        }
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": request_id,
                                "status": "pending",
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/research/:request_id",
            get({
                let request_key_map = request_key_map.clone();
                move |headers: HeaderMap, Path(request_id): Path<String>| {
                    let request_key_map = request_key_map.clone();
                    async move {
                        let api_key = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("Bearer "))
                            .unwrap_or("")
                            .to_string();
                        let expected_api_key = {
                            let guard = request_key_map
                                .lock()
                                .expect("request key map lock should not be poisoned");
                            guard.get(&request_id).cloned()
                        };
                        match expected_api_key {
                            Some(expected) if expected == api_key => (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "request_id": request_id,
                                    "status": "pending",
                                })),
                            ),
                            Some(_) => (
                                StatusCode::UNAUTHORIZED,
                                Json(serde_json::json!({
                                    "detail": { "error": "Unauthorized: key mismatch." }
                                })),
                            ),
                            None => (
                                StatusCode::NOT_FOUND,
                                Json(serde_json::json!({
                                    "detail": { "error": "Research task not found." }
                                })),
                            ),
                        }
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

async fn spawn_proxy_server(proxy: TavilyProxy, usage_base: String) -> SocketAddr {
    spawn_proxy_server_with_dev(proxy, usage_base, false).await
}

async fn spawn_proxy_server_with_dev(
    proxy: TavilyProxy,
    usage_base: String,
    dev_open_admin: bool,
) -> SocketAddr {
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base,
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/mcp", any(proxy_handler))
        .route("/mcp/*path", any(mcp_subpath_reject_handler))
        .route("/api/tavily/search", post(tavily_http_search))
        .route("/api/tavily/extract", post(tavily_http_extract))
        .route("/api/tavily/crawl", post(tavily_http_crawl))
        .route("/api/tavily/map", post(tavily_http_map))
        .route("/api/tavily/research", post(tavily_http_research))
        .route(
            "/api/tavily/research/:request_id",
            get(tavily_http_research_result),
        )
        .route("/api/tavily/usage", get(tavily_http_usage))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_keys_admin_server(
    proxy: TavilyProxy,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
) -> SocketAddr {
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth,
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/bulk-actions", post(post_api_key_bulk_actions))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/admin/login", post(post_admin_login))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_keys_admin_server_with_usage_base(
    proxy: TavilyProxy,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
    usage_base: String,
) -> SocketAddr {
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth,
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base,
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/bulk-actions", post(post_api_key_bulk_actions))
        .route("/api/keys/validate", post(post_validate_api_keys))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/admin/login", post(post_admin_login))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_keys_admin_server_with_geo_origin(
    proxy: TavilyProxy,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
    geo_origin: String,
) -> SocketAddr {
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth,
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: geo_origin,
    });

    let app = Router::new()
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/bulk-actions", post(post_api_key_bulk_actions))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/admin/login", post(post_admin_login))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_keys_admin_server_with_usage_and_geo(
    proxy: TavilyProxy,
    forward_auth: ForwardAuthConfig,
    dev_open_admin: bool,
    usage_base: String,
    geo_origin: String,
) -> SocketAddr {
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth,
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base,
        api_key_ip_geo_origin: geo_origin,
    });

    let app = Router::new()
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/bulk-actions", post(post_api_key_bulk_actions))
        .route("/api/keys/validate", post(post_validate_api_keys))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/admin/login", post(post_admin_login))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_usage_mock_server() -> SocketAddr {
    let app = Router::new().route(
        "/usage",
        get(|headers: HeaderMap| async move {
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            match auth {
                // ok: remaining > 0
                "Bearer tvly-ok" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 10 },
                    })),
                )
                    .into_response(),
                "Bearer tvly-ok-active" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 10 },
                    })),
                )
                    .into_response(),
                "Bearer tvly-ok-disabled" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 11 },
                    })),
                )
                    .into_response(),
                "Bearer tvly-ok-exhausted" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 12 },
                    })),
                )
                    .into_response(),
                "Bearer tvly-ok-quarantined" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 13 },
                    })),
                )
                    .into_response(),
                // ok_exhausted: remaining == 0
                "Bearer tvly-exhausted" => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "key": { "limit": 1000, "usage": 1000 },
                    })),
                )
                    .into_response(),
                // unauthorized
                "Bearer tvly-unauth" => {
                    (StatusCode::UNAUTHORIZED, Body::from("unauthorized")).into_response()
                }
                // forbidden
                "Bearer tvly-forbidden" => {
                    (StatusCode::FORBIDDEN, Body::from("forbidden")).into_response()
                }
                // rate-limited transient client error
                "Bearer tvly-rate-limited" => {
                    (StatusCode::TOO_MANY_REQUESTS, Body::from("rate limited")).into_response()
                }
                _ => (StatusCode::BAD_REQUEST, Body::from("unknown key")).into_response(),
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

#[derive(Clone)]
struct ProxyRelayState {
    upstream_base: String,
    hits: Arc<AtomicUsize>,
    client: Client,
}

async fn proxy_relay_handler(
    State(state): State<ProxyRelayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    state.hits.fetch_add(1, Ordering::SeqCst);
    if headers.get("authorization").is_none() {
        return (
            reqwest::StatusCode::OK,
            Json(serde_json::json!({
                "key": { "limit": 1000, "usage": 0 }
            })),
        )
            .into_response();
    }
    let target = format!(
        "{}{}",
        state.upstream_base,
        uri.path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/")
    );
    let mut req = state.client.request(method, target);
    for (name, value) in &headers {
        if name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        req = req.header(name, value);
    }
    match req.send().await {
        Ok(response) => {
            let status = response.status();
            let body = response.bytes().await.unwrap_or_default();
            (status, body).into_response()
        }
        Err(err) => (
            reqwest::StatusCode::BAD_GATEWAY,
            format!("proxy relay error: {err}"),
        )
            .into_response(),
    }
}

async fn spawn_usage_proxy_relay_server(
    upstream_base: String,
    hits: Arc<AtomicUsize>,
) -> SocketAddr {
    let app = Router::new()
        .fallback(any(proxy_relay_handler))
        .with_state(ProxyRelayState {
            upstream_base,
            hits,
            client: Client::new(),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_api_key_geo_mock_server() -> SocketAddr {
    let app = Router::new().route(
        "/geo",
        post(|Json(ips): Json<Vec<String>>| async move {
            let entries = ips
                .into_iter()
                .map(|ip| match ip.as_str() {
                    "8.8.8.8" => serde_json::json!({
                        "ip": ip,
                        "country": "US",
                        "city": null,
                        "subdivision": null,
                    }),
                    "1.1.1.1" => serde_json::json!({
                        "ip": ip,
                        "country": "US",
                        "city": "Westfield",
                        "subdivision": "MA",
                    }),
                    "18.183.246.69" => serde_json::json!({
                        "ip": ip,
                        "country": "JP",
                        "city": "Tokyo",
                        "subdivision": "13",
                    }),
                    _ => serde_json::json!({
                        "ip": ip,
                        "country": null,
                        "city": null,
                        "subdivision": null,
                    }),
                })
                .collect::<Vec<_>>();
            (StatusCode::OK, Json(entries))
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

fn hash_admin_password_for_test(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};

    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash builtin admin password")
        .to_string()
}

async fn spawn_builtin_keys_admin_server(proxy: TavilyProxy, password: &str) -> SocketAddr {
    let password_hash = hash_admin_password_for_test(password);
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(true, None, Some(password_hash)),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/api/admin/login", post(post_admin_login))
        .route("/api/admin/logout", post(post_admin_logout))
        .route("/api/events", get(sse_dashboard))
        .route("/api/dashboard/overview", get(get_dashboard_overview))
        .route("/api/summary", get(fetch_summary))
        .route("/api/summary/windows", get(fetch_summary_windows))
        .route("/api/logs", get(list_logs))
        .route("/api/logs/list", get(list_logs_cursor))
        .route("/api/logs/catalog", get(get_logs_catalog))
        .route("/api/logs/:log_id/details", get(get_log_details))
        .route("/api/alerts/catalog", get(get_alert_catalog))
        .route("/api/alerts/events", get(get_alert_events))
        .route("/api/alerts/groups", get(get_alert_groups))
        .route("/api/keys", get(list_keys))
        .route("/api/keys/:id", get(get_api_key_detail))
        .route("/api/keys/:id/logs", get(get_key_logs))
        .route("/api/keys/:id/logs/list", get(get_key_logs_list))
        .route("/api/keys/:id/logs/catalog", get(get_key_logs_catalog))
        .route("/api/keys/:id/logs/page", get(get_key_logs_page))
        .route(
            "/api/keys/:id/logs/:log_id/details",
            get(get_key_log_details),
        )
        .route("/api/tokens/:id/logs/list", get(get_token_logs_list))
        .route("/api/tokens/:id/logs/catalog", get(get_token_logs_catalog))
        .route("/api/keys/batch", post(create_api_keys_batch))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

fn linuxdo_oauth_options_for_test() -> LinuxDoOAuthOptions {
    LinuxDoOAuthOptions {
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
    }
}

async fn spawn_user_oauth_server_with_options(
    proxy: TavilyProxy,
    linuxdo_oauth: LinuxDoOAuthOptions,
) -> SocketAddr {
    let static_dir = temp_static_dir("linuxdo-user-oauth");
    let state = Arc::new(AppState {
        proxy,
        static_dir: Some(static_dir),
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth,
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/console", get(serve_console_index))
        .route("/console/*path", get(serve_console_index))
        .route("/registration-paused", get(serve_registration_paused_index))
        .route(
            "/auth/linuxdo",
            get(get_linuxdo_auth).post(post_linuxdo_auth),
        )
        .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
        .route("/api/profile", get(get_profile))
        .route("/api/user/token", get(get_user_token))
        .route("/api/user/dashboard", get(get_user_dashboard))
        .route("/api/user/tokens", get(get_user_tokens))
        .route("/api/user/tokens/:id", get(get_user_token_detail))
        .route("/api/user/tokens/:id/secret", get(get_user_token_secret))
        .route("/api/user/tokens/:id/logs", get(get_user_token_logs))
        .route("/api/user/tokens/:id/events", get(sse_user_token))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_user_oauth_server(proxy: TavilyProxy) -> SocketAddr {
    spawn_user_oauth_server_with_options(proxy, linuxdo_oauth_options_for_test()).await
}

#[test]
fn linuxdo_user_sync_scheduler_requires_oauth_configuration() {
    assert!(
        !LinuxDoOAuthOptions::disabled().is_user_sync_scheduler_enabled(),
        "disabled LinuxDo OAuth should not enqueue daily sync jobs"
    );

    let mut missing_redirect = linuxdo_oauth_options_for_test();
    missing_redirect.redirect_url = None;
    assert!(
        !missing_redirect.is_user_sync_scheduler_enabled(),
        "incomplete LinuxDo OAuth config should not enqueue daily sync jobs"
    );

    let mut configured = linuxdo_oauth_options_for_test();
    assert!(configured.is_user_sync_scheduler_enabled());
    configured.user_sync_enabled = false;
    assert!(!configured.is_user_sync_scheduler_enabled());
}

async fn spawn_admin_users_server(proxy: TavilyProxy, dev_open_admin: bool) -> SocketAddr {
    let static_dir = temp_static_dir("admin-users");
    let state = Arc::new(AppState {
        proxy,
        static_dir: Some(static_dir),
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route(
            "/api/admin/registration",
            get(get_admin_registration_settings),
        )
        .route(
            "/api/admin/registration",
            patch(patch_admin_registration_settings),
        )
        .route("/api/user-tags", get(list_user_tags))
        .route("/api/user-tags", post(create_user_tag))
        .route("/api/user-tags/:tag_id", patch(update_user_tag))
        .route("/api/user-tags/:tag_id", delete(delete_user_tag))
        .route("/api/users", get(list_users))
        .route("/api/users/:id", get(get_user_detail))
        .route("/api/users/:id/quota", patch(update_user_quota))
        .route("/api/users/:id/tags", post(bind_user_tag))
        .route("/api/users/:id/tags/:tag_id", delete(unbind_user_tag))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_admin_tokens_server(proxy: TavilyProxy, dev_open_admin: bool) -> SocketAddr {
    let static_dir = temp_static_dir("admin-tokens");
    let state = Arc::new(AppState {
        proxy,
        static_dir: Some(static_dir),
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let app = Router::new()
        .route("/api/tokens", get(list_tokens))
        .route("/api/tokens/unbound-usage", get(list_unbound_token_usage))
        .route("/api/tokens/:id", get(get_token_detail))
        .route("/api/tokens/:id/logs", get(get_token_logs))
        .route("/api/tokens/:id/logs/page", get(get_token_logs_page))
        .route(
            "/api/tokens/:id/logs/:log_id/details",
            get(get_token_log_details),
        )
        .route("/api/tokens/:id/events", get(sse_token))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_admin_forward_proxy_server(
    proxy: TavilyProxy,
    usage_base: String,
    dev_open_admin: bool,
) -> SocketAddr {
    spawn_admin_forward_proxy_server_with_geo_origin(
        proxy,
        usage_base,
        dev_open_admin,
        "https://api.country.is".to_string(),
    )
    .await
}

async fn spawn_admin_forward_proxy_server_with_geo_origin(
    proxy: TavilyProxy,
    usage_base: String,
    dev_open_admin: bool,
    api_key_ip_geo_origin: String,
) -> SocketAddr {
    let static_dir = temp_static_dir("admin-forward-proxy");
    let state = Arc::new(AppState {
        proxy,
        static_dir: Some(static_dir),
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        dev_open_admin,
        usage_base,
        api_key_ip_geo_origin,
    });

    let app = Router::new()
        .route("/api/settings", get(get_settings))
        .route("/api/settings/system", put(put_system_settings))
        .route(
            "/api/settings/forward-proxy",
            put(put_forward_proxy_settings),
        )
        .route(
            "/api/settings/forward-proxy/validate",
            post(post_forward_proxy_candidate_validation),
        )
        .route(
            "/api/settings/forward-proxy/revalidate",
            post(post_forward_proxy_revalidate),
        )
        .route(
            "/api/stats/forward-proxy/summary",
            get(get_forward_proxy_dashboard_summary),
        )
        .route(
            "/api/stats/forward-proxy",
            get(get_forward_proxy_live_stats),
        )
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_forward_proxy_probe_upstream() -> SocketAddr {
    let app = Router::new()
        .route("/usage", get(|| async { StatusCode::NOT_FOUND }))
        .route("/mcp", any(|| async { StatusCode::NOT_FOUND }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_fake_forward_proxy(status: StatusCode) -> SocketAddr {
    spawn_fake_forward_proxy_with_body(status, String::new()).await
}

async fn spawn_fake_forward_proxy_with_body(status: StatusCode, body: String) -> SocketAddr {
    let app = Router::new().fallback(any(move || {
        let body = body.clone();
        async move { (status, body) }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_fake_forward_proxy_with_stalled_body(status: StatusCode) -> SocketAddr {
    let app = Router::new().fallback(any(move || async move {
            let stream = async_stream::stream! {
                yield Ok::<Bytes, Infallible>(Bytes::from_static(b"ip=203.0.113.8\nloc=JP\ncolo=NRT\n"));
                pending::<()>().await;
            };
            (status, Body::from_stream(stream))
        }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_counted_fake_forward_proxy(
    status: StatusCode,
    delay: Duration,
    hits: Arc<AtomicUsize>,
) -> SocketAddr {
    let app = Router::new().fallback(any(move || {
        let hits = hits.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            status
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_forward_proxy_subscription_server(body: String) -> SocketAddr {
    let app = Router::new().route(
        "/subscription",
        get(move || {
            let body = body.clone();
            async move { (StatusCode::OK, body) }
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

async fn spawn_mutable_forward_proxy_subscription_server(
    state: Arc<Mutex<(StatusCode, String)>>,
) -> SocketAddr {
    let app = Router::new().route(
        "/subscription",
        get(move || {
            let state = state.clone();
            async move {
                let (status, body) = {
                    let guard = state.lock().expect("subscription state lock");
                    (guard.0, guard.1.clone())
                };
                (status, body)
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

async fn spawn_counted_forward_proxy_subscription_server(
    state: Arc<Mutex<(StatusCode, String)>>,
    hits: Arc<AtomicUsize>,
) -> SocketAddr {
    let app = Router::new().route(
        "/subscription",
        get(move || {
            let state = state.clone();
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                let (status, body) = {
                    let guard = state.lock().expect("subscription state lock");
                    (guard.0, guard.1.clone())
                };
                (status, body)
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

async fn spawn_linuxdo_authorize_method_probe_server(
    method_probe: Arc<Mutex<Option<Method>>>,
) -> SocketAddr {
    let app = Router::new().route(
        "/oauth2/authorize",
        any({
            let method_probe = method_probe.clone();
            move |method: Method| {
                let method_probe = method_probe.clone();
                async move {
                    *method_probe.lock().expect("method probe lock poisoned") =
                        Some(method.clone());
                    if method == Method::GET {
                        StatusCode::OK
                    } else {
                        StatusCode::METHOD_NOT_ALLOWED
                    }
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

async fn spawn_linuxdo_oauth_mock_server(
    provider_user_id: &str,
    username: &str,
    display_name: &str,
) -> SocketAddr {
    let profile = json!({
        "id": provider_user_id,
        "username": username,
        "name": display_name,
        "active": true,
        "trust_level": 3
    });
    spawn_linuxdo_oauth_mock_server_with_behavior(LinuxDoOauthMockBehavior {
        authorization_access_token: "mock-linuxdo-access-token".to_string(),
        authorization_refresh_token: Some("mock-linuxdo-refresh-token".to_string()),
        authorization_profile: profile.clone(),
        refresh_access_token: "mock-linuxdo-refresh-access-token".to_string(),
        refresh_refresh_token: Some("mock-linuxdo-refresh-token-rotated".to_string()),
        refresh_profile: profile,
        refresh_error: None,
    })
    .await
}

#[derive(Clone)]
struct LinuxDoOauthMockBehavior {
    authorization_access_token: String,
    authorization_refresh_token: Option<String>,
    authorization_profile: Value,
    refresh_access_token: String,
    refresh_refresh_token: Option<String>,
    refresh_profile: Value,
    refresh_error: Option<(StatusCode, Value)>,
}

async fn spawn_linuxdo_oauth_mock_server_with_behavior(
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
