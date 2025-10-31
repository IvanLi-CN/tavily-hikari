mod server;

use std::net::SocketAddr;

use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{DEFAULT_UPSTREAM, TavilyProxy};

#[derive(Debug, Parser)]
#[command(author, version, about = "Tavily reverse proxy with key rotation")]
struct Cli {
    /// Tavily API keys（逗号分隔或重复传参）
    #[arg(
        long,
        value_delimiter = ',',
        env = "TAVILY_API_KEYS",
        hide_env_values = true,
        required = true
    )]
    keys: Vec<String>,

    /// 上游 Tavily MCP 端点
    #[arg(long, env = "TAVILY_UPSTREAM", default_value = DEFAULT_UPSTREAM)]
    upstream: String,

    /// 代理监听地址
    #[arg(long, env = "PROXY_BIND", default_value = "127.0.0.1")]
    bind: String,

    /// 代理监听端口
    #[arg(long, env = "PROXY_PORT", default_value_t = 8787)]
    port: u16,

    /// SQLite 数据库存储路径
    #[arg(long, env = "PROXY_DB_PATH", default_value = "tavily_proxy.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();

    let proxy = TavilyProxy::with_endpoint(cli.keys, &cli.upstream, &cli.db_path).await?;
    let addr: SocketAddr = format!("{}:{}", cli.bind, cli.port).parse()?;

    println!("Tavily proxy listening on http://{addr}");
    server::serve(addr, proxy).await?;

    Ok(())
}
