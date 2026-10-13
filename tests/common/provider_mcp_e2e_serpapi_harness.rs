// ---------------------------------------------------------------------------
// SerpApi MCP harness (real server binary + real mock_serpapi binary).
// ---------------------------------------------------------------------------

struct SerpapiMcpHarness {
    base_url: String,
    mock_base: String,
    token: String,
    token_id: String,
    db_path: PathBuf,
    db_str: String,
    tavily_counters: Option<TavilyMockCounters>,
    _server: ChildGuard,
    _mock: ChildGuard,
}

impl Drop for SerpapiMcpHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

#[derive(Default, Clone, Copy)]
struct HarnessOptions {
    /// MOCK_SERPAPI_SSE_ERRORS=1: /mcp + /search errors wrap in SSE frames.
    sse_errors: bool,
    /// Spawn the in-process Tavily mock and point TAVILY_UPSTREAM at it.
    tavily_mock: bool,
}

async fn spawn_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> SerpapiMcpHarness {
    spawn_harness_with_options(prefix, pool_keys, mock_keys, HarnessOptions::default()).await
}

async fn spawn_harness_with_options(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
    options: HarnessOptions,
) -> SerpapiMcpHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_harness(prefix, pool_keys, mock_keys, options).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    panic!("serpapi mcp harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_harness(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
    options: HarnessOptions,
) -> Option<SerpapiMcpHarness> {
    let mock_port = reserve_local_port();
    let mut mock_cmd = Command::new(support_binaries::resolve_support_binary(
        "MOCK_SERPAPI_TEST_BIN",
        env!("CARGO_BIN_EXE_mock_serpapi"),
    ));
    mock_cmd
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if options.sse_errors {
        mock_cmd.env("MOCK_SERPAPI_SSE_ERRORS", "1");
    }
    let mock = mock_cmd.spawn().expect("spawn mock_serpapi");

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    // Tavily surfaces must never contact production endpoints: default to a
    // dead local port unless the test needs the in-process Tavily mock.
    let dead_port = reserve_local_port();
    let (tavily_upstream, tavily_usage_base, tavily_counters) = if options.tavily_mock {
        let (base, counters) = spawn_tavily_mock_upstream().await;
        let usage_base = base.trim_end_matches("/mcp").to_string();
        (base, usage_base, Some(counters))
    } else {
        (
            format!("http://127.0.0.1:{dead_port}/mcp"),
            format!("http://127.0.0.1:{dead_port}"),
            None,
        )
    };
    let server = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ))
    .env("TAVILY_API_KEYS", "tvly-mcp-e2e-key")
    .env("TAVILY_UPSTREAM", &tavily_upstream)
    .env("TAVILY_USAGE_BASE", &tavily_usage_base)
    .env("SERPAPI_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
    // Path suffix under the mock host: exercises the ingress-aware upstream
    // URL builder (SERPAPI_MCP_UPSTREAM honored with a /mcp path).
    .env(
        "SERPAPI_MCP_UPSTREAM",
        format!("http://127.0.0.1:{mock_port}/mcp"),
    )
    .env("SERPAPI_API_KEYS", pool_keys.join(","))
    .env("PROXY_BIND", "127.0.0.1")
    .env("PROXY_PORT", port.to_string())
    .env("PROXY_DB_PATH", &db_str)
    .env("DEV_OPEN_ADMIN", "true")
    .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn tavily-hikari");

    if !health_ready_within(port, 25).await {
        drop(ChildGuard { child: server });
        drop(ChildGuard { child: mock });
        remove_sqlite_files(&db_path);
        return None;
    }
    wait_for_provider_keys(&db_str, "serpapi", i64::try_from(pool_keys.len()).unwrap()).await;
    let base_url = format!("http://127.0.0.1:{port}");
    let (token, token_id) = create_test_token(&base_url).await;

    Some(SerpapiMcpHarness {
        base_url,
        mock_base: format!("http://127.0.0.1:{mock_port}"),
        token,
        token_id,
        db_path,
        db_str,
        tavily_counters,
        _server: ChildGuard { child: server },
        _mock: ChildGuard { child: mock },
    })
}

/// One raw MCP-over-HTTP response as the client sees it.
struct McpCallResult {
    status: StatusCode,
    session_header: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl McpCallResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

impl SerpapiMcpHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    /// POST one JSON-RPC message to `/serpapi/mcp` with the Hikari Bearer
    /// token and (optionally) the proxy mcp-session-id header.
    async fn serpapi_mcp_call(&self, session_id: Option<&str>, message: &Value) -> McpCallResult {
        self.mcp_call("/serpapi/mcp", session_id, message).await
    }

    async fn mcp_call(
        &self,
        path: &str,
        session_id: Option<&str>,
        message: &Value,
    ) -> McpCallResult {
        let mut request = Client::new()
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json");
        if let Some(session_id) = session_id {
            request = request.header("mcp-session-id", session_id);
        }
        let response = request.json(message).send().await.expect("mcp request");
        McpCallResult {
            status: response.status(),
            session_header: response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            content_type: response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: response.bytes().await.expect("response body").to_vec(),
        }
    }

    fn initialize_message(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "provider-mcp-e2e", "version": "0.1.0" }
            }
        })
    }

    fn tools_call_message(id: u64, tool: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": { "q": "hello" } }
        })
    }

    /// Run initialize and return the proxy session id the gateway handed back.
    async fn initialize_session(&self, id: u64) -> String {
        let response = self
            .serpapi_mcp_call(None, &Self::initialize_message(id))
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "initialize must pass through: {}",
            response.body_text()
        );
        let session = response
            .session_header
            .clone()
            .expect("initialize must return the translated proxy mcp-session-id");
        assert_ne!(
            session, "mock-serpapi-mcp-session",
            "client must never see the raw upstream session id"
        );
        // notifications/initialized completes the handshake (non-billable).
        let notified = self
            .serpapi_mcp_call(
                Some(&session),
                &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            )
            .await;
        assert_eq!(notified.status, StatusCode::ACCEPTED, "notification 202");
        session
    }

    /// Snapshot of the mock's additive metrics endpoint.
    async fn mock_metrics(&self) -> Value {
        Client::new()
            .get(format!("{}/mock-metrics", self.mock_base))
            .send()
            .await
            .expect("mock metrics request")
            .json::<Value>()
            .await
            .expect("mock metrics json")
    }

    async fn mcp_session_row(&self, proxy_session_id: &str) -> sqlx::sqlite::SqliteRow {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT proxy_session_id, upstream_session_id, upstream_key_id, auth_token_id, \
             gateway_mode, experiment_variant, provider \
             FROM mcp_sessions WHERE proxy_session_id = ? LIMIT 1",
        )
        .bind(proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("mcp_sessions row for proxy session id");
        pool.close().await;
        row
    }

    async fn charged_ledger_rows(&self) -> Vec<(String, i64, Option<i64>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT billing_state, business_credits, request_log_id FROM billing_ledger \
             WHERE token_id = ? ORDER BY auth_token_log_id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("ledger rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("billing_state"),
                    row.get::<i64, _>("business_credits"),
                    row.get::<Option<i64>, _>("request_log_id"),
                )
            })
            .collect()
    }

    async fn token_log_rows(&self) -> Vec<(Option<String>, Option<i64>, Option<String>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT request_kind_key, business_credits, provider FROM auth_token_logs \
             WHERE token_id = ? ORDER BY id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("token log rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<Option<String>, _>("request_kind_key"),
                    row.get::<Option<i64>, _>("business_credits"),
                    row.get::<Option<String>, _>("provider"),
                )
            })
            .collect()
    }
}

fn mock_tools_list_body(id: u64) -> Value {
    fn tool(name: &str) -> Value {
        json!({
            "name": name,
            "description": format!("Mock SerpAPI {name}"),
            "inputSchema": {
                "type": "object",
                "properties": { "q": { "type": "string", "description": "search query" } },
                "required": ["q"]
            }
        })
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [tool("search"), tool("search_table"), tool("search_dashboard")]
        }
    })
}

fn mock_run_out_body() -> Value {
    json!({ "error": "Your account has run out of searches." })
}

fn mock_invalid_key_body() -> Value {
    json!({
        "error": "Invalid API key. Your API key should be here: https://serpapi.com/manage-api-key"
    })
}

