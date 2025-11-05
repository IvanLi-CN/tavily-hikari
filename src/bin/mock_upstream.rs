use axum::{Json, Router, extract::Query, http::StatusCode, response::IntoResponse, routing::any};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use tokio::task::JoinHandle;

#[derive(Serialize)]
struct StructuredContent {
    status: i64,
}

#[derive(Serialize)]
struct ResultBody {
    #[serde(rename = "structuredContent")]
    structured_content: StructuredContent,
}

#[derive(Serialize)]
struct ResponseBody {
    result: ResultBody,
}

async fn handle(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    // Accept optional `status` query; default to 200
    let status: i64 = q
        .get("status")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(200);

    let body = Json(ResponseBody {
        result: ResultBody {
            structured_content: StructuredContent { status },
        },
    });

    (StatusCode::OK, body)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start upstream mock server
    let app = Router::new()
        .route("/mcp", any(handle))
        .route("/mcp/*path", any(handle));
    let bind_addr =
        std::env::var("MOCK_UPSTREAM_ADDR").unwrap_or_else(|_| "127.0.0.1:58088".to_string());
    let addr: SocketAddr = bind_addr.parse()?;
    println!("Mock upstream on http://{addr}");

    // Kick off background traffic generator
    let generator = spawn_generator();

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;

    // Ensure generator is awaited if server exits
    if let Some(h) = generator {
        let _ = h.await;
    }
    Ok(())
}

fn spawn_generator() -> Option<JoinHandle<()>> {
    let proxy_base =
        std::env::var("PROXY_BASE").unwrap_or_else(|_| "http://127.0.0.1:58087".to_string());
    let admin_header_name = std::env::var("ADMIN_HEADER_NAME").ok();
    let admin_header_value = std::env::var("ADMIN_HEADER_VALUE").ok();
    let provided_token = std::env::var("ACCESS_TOKEN").ok();
    let interval_ms: u64 = std::env::var("GEN_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    Some(tokio::spawn(async move {
        let client = Client::new();

        // Try to obtain token once
        let mut token = provided_token;
        if token.is_none()
            && let (Some(hname), Some(hval)) =
                (admin_header_name.as_deref(), admin_header_value.as_deref())
        {
            match create_token(&client, &proxy_base, hname, hval).await {
                Ok(t) => {
                    println!("[mock-gen] created token: {}", t);
                    token = Some(t);
                }
                Err(e) => {
                    eprintln!("[mock-gen] create token failed: {e}");
                }
            }
        }

        let Some(token) = token else {
            eprintln!("[mock-gen] no ACCESS_TOKEN and admin creation failed; generator idle");
            return;
        };

        let mut i = 0u64;
        loop {
            let statuses = [200, 500, 432];
            let code = statuses[(i as usize) % statuses.len()];
            let url = format!("{}/mcp?status={}", proxy_base, code);
            match client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(resp) => {
                    let sc = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    println!("[mock-gen] {} -> {}", url, sc);
                    if !sc.is_success() {
                        eprintln!("[mock-gen] response body: {}", body);
                    }
                }
                Err(err) => eprintln!("[mock-gen] request error: {}", err),
            }

            i = i.wrapping_add(1);
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }))
}

#[derive(Deserialize)]
struct TokenSecret {
    token: String,
}

async fn create_token(
    client: &Client,
    base: &str,
    admin_header_name: &str,
    admin_header_value: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = format!("{}/api/tokens", base);
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header(admin_header_name, admin_header_value)
        .body("{}")
        .send()
        .await?;
    let resp = resp.error_for_status()?;
    let ts: TokenSecret = resp.json().await?;
    Ok(ts.token)
}
