use std::{sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
use reqwest::Client;
use rust_mcp_schema::{
    RequestId,
    mcp_2025_06_18::schema_utils::{
        ClientJsonrpcRequest, ClientMessage, ClientMessages, RequestFromClient,
    },
    mcp_2025_06_18::{CallToolRequest, CallToolRequestParams, ListToolsRequest, PingRequest},
};
use serde_json::{Map, Value};
use tokio::time::sleep;

#[derive(ValueEnum, Clone, Debug)]
enum RequestMode {
    CallTool,
    ListTools,
    Ping,
}

#[derive(Parser, Debug, Clone)]
struct Cli {
    /// MCP endpoint (usually the proxy /mcp URL)
    #[arg(long, default_value = "http://127.0.0.1:58087/mcp")]
    endpoint: String,
    /// Access token in format th-<id>-<secret>
    #[arg(long)]
    token: String,
    /// Number of requests to send
    #[arg(long, default_value_t = 20)]
    count: usize,
    /// Delay between requests in milliseconds (per worker)
    #[arg(long, default_value_t = 500)]
    interval_ms: u64,
    /// Concurrent workers
    #[arg(long, default_value_t = 1)]
    workers: usize,
    /// Type of MCP request to emit
    #[arg(long, value_enum, default_value_t = RequestMode::CallTool)]
    mode: RequestMode,
    /// Tool name when mode=call-tool
    #[arg(long, default_value = "mock-tool")]
    tool: String,
    /// Text prompt for call-tool payload
    #[arg(long, default_value = "hello from mock client")]
    prompt: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if cli.workers == 0 {
        eprintln!("--workers must be at least 1");
        std::process::exit(1);
    }
    let client = Arc::new(Client::new());

    let worker_counts: Vec<usize> = (0..cli.workers)
        .map(|idx| {
            let base = cli.count / cli.workers;
            let extra = if idx < (cli.count % cli.workers) {
                1
            } else {
                0
            };
            base + extra
        })
        .collect();

    let tasks = worker_counts
        .into_iter()
        .enumerate()
        .map(|(worker_id, total)| {
            let cli = cli.clone();
            let client = Arc::clone(&client);
            tokio::spawn(async move {
                for seq in 0..total {
                    let body = build_payload(&cli, worker_id * 10_000 + seq);
                    match client
                        .post(&cli.endpoint)
                        .header("Authorization", format!("Bearer {}", cli.token))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            let status = resp.status();
                            let text = resp.text().await.unwrap_or_else(|_| "<empty>".into());
                            println!("[worker {worker_id}] {status} -> {text}");
                        }
                        Err(err) => eprintln!("[worker {worker_id}] request error: {err}"),
                    }
                    sleep(Duration::from_millis(cli.interval_ms)).await;
                }
            })
        });

    for task in tasks {
        let _ = task.await;
    }

    Ok(())
}

fn build_payload(cli: &Cli, seq: usize) -> Value {
    let id = RequestId::String(format!("req-{}", seq));
    let request = match cli.mode {
        RequestMode::CallTool => {
            let mut args = Map::new();
            args.insert("prompt".into(), Value::String(cli.prompt.clone()));
            let params = CallToolRequestParams {
                name: cli.tool.clone(),
                arguments: Some(args),
            };
            RequestFromClient::ClientRequest(CallToolRequest::new(params).into())
        }
        RequestMode::ListTools => {
            RequestFromClient::ClientRequest(ListToolsRequest::new(None).into())
        }
        RequestMode::Ping => RequestFromClient::ClientRequest(PingRequest::new(None).into()),
    };

    let message = ClientMessage::Request(ClientJsonrpcRequest::new(id, request));
    serde_json::to_value(ClientMessages::from(message)).expect("serialize client message")
}
