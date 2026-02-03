mod server;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

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
        hide_env_values = true
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
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Web 静态资源目录（指向打包后的前端 dist）
    #[arg(long, env = "WEB_STATIC_DIR")]
    static_dir: Option<PathBuf>,

    /// Forward proxy 用户标识请求头
    #[arg(long, env = "FORWARD_AUTH_HEADER")]
    forward_auth_header: Option<String>,

    /// Forward proxy 管理员标识值
    #[arg(long, env = "FORWARD_AUTH_ADMIN_VALUE")]
    forward_auth_admin_value: Option<String>,

    /// Forward proxy 昵称请求头
    #[arg(long, env = "FORWARD_AUTH_NICKNAME_HEADER")]
    forward_auth_nickname_header: Option<String>,

    /// 管理员模式昵称（覆盖前端显示）
    #[arg(long, env = "ADMIN_MODE_NAME")]
    admin_mode_name: Option<String>,

    /// Enable/disable ForwardAuth admin authentication (default true).
    #[arg(long, env = "ADMIN_AUTH_FORWARD_ENABLED", default_value_t = true)]
    admin_auth_forward_enabled: bool,

    /// Enable/disable built-in admin login (cookie session) (default false).
    #[arg(long, env = "ADMIN_AUTH_BUILTIN_ENABLED", default_value_t = false)]
    admin_auth_builtin_enabled: bool,

    /// Built-in admin password (required when ADMIN_AUTH_BUILTIN_ENABLED=true).
    #[arg(long, env = "ADMIN_AUTH_BUILTIN_PASSWORD", hide_env_values = true)]
    admin_auth_builtin_password: Option<String>,

    /// 开发模式：放开管理接口权限（仅本地验证使用）
    #[arg(long, env = "DEV_OPEN_ADMIN", default_value_t = false)]
    dev_open_admin: bool,

    /// Tavily Usage API base (for quota/usage sync)
    #[arg(
        long,
        env = "TAVILY_USAGE_BASE",
        default_value = "https://api.tavily.com"
    )]
    usage_base: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();

    // Ensure parent directory for database exists when using nested path like data/tavily_proxy.db
    let db_path = Path::new(&cli.db_path);
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    println!("Using database: {}", db_path.display());

    let proxy = TavilyProxy::with_endpoint(cli.keys, &cli.upstream, &cli.db_path).await?;
    let addr: SocketAddr = format!("{}:{}", cli.bind, cli.port).parse()?;

    let forward_auth_header = parse_header_name(cli.forward_auth_header, "FORWARD_AUTH_HEADER")?;
    let forward_auth_nickname_header = parse_header_name(
        cli.forward_auth_nickname_header,
        "FORWARD_AUTH_NICKNAME_HEADER",
    )?;
    let forward_auth_admin_value = cli
        .forward_auth_admin_value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let forward_auth = server::ForwardAuthConfig::new(
        forward_auth_header,
        forward_auth_admin_value,
        forward_auth_nickname_header,
        cli.admin_mode_name
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    );

    let builtin_password = cli
        .admin_auth_builtin_password
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if cli.admin_auth_builtin_enabled && builtin_password.is_none() {
        return Err(
            "ADMIN_AUTH_BUILTIN_PASSWORD must be set when ADMIN_AUTH_BUILTIN_ENABLED=true".into(),
        );
    }

    let admin_auth = server::AdminAuthOptions {
        forward_auth_enabled: cli.admin_auth_forward_enabled,
        builtin_auth_enabled: cli.admin_auth_builtin_enabled,
        builtin_auth_password: builtin_password,
    };

    let static_dir = cli.static_dir.or_else(|| {
        let default = PathBuf::from("web/dist");
        if default.exists() {
            Some(default)
        } else {
            None
        }
    });

    server::serve(
        addr,
        proxy,
        static_dir,
        forward_auth,
        admin_auth,
        cli.dev_open_admin,
        cli.usage_base,
    )
    .await?;

    Ok(())
}

fn parse_header_name(
    value: Option<String>,
    field: &str,
) -> Result<Option<axum::http::HeaderName>, Box<dyn std::error::Error>> {
    let Some(raw) = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };

    match raw.parse::<axum::http::HeaderName>() {
        Ok(parsed) => Ok(Some(parsed)),
        Err(err) => Err(format!("invalid header name for {field}: {err}").into()),
    }
}
