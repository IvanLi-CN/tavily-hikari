mod server;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use argon2::password_hash::PasswordHash;
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

    /// Built-in admin password (legacy; prefer ADMIN_AUTH_BUILTIN_PASSWORD_HASH).
    #[arg(long, env = "ADMIN_AUTH_BUILTIN_PASSWORD", hide_env_values = true)]
    admin_auth_builtin_password: Option<String>,

    /// Built-in admin password hash (PHC string, recommended).
    #[arg(long, env = "ADMIN_AUTH_BUILTIN_PASSWORD_HASH", hide_env_values = true)]
    admin_auth_builtin_password_hash: Option<String>,

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

    /// Enable/disable LinuxDo OAuth2 login for user-facing flow.
    #[arg(long, env = "LINUXDO_OAUTH_ENABLED", default_value_t = false)]
    linuxdo_oauth_enabled: bool,

    /// LinuxDo OAuth2 client id.
    #[arg(long, env = "LINUXDO_OAUTH_CLIENT_ID")]
    linuxdo_oauth_client_id: Option<String>,

    /// LinuxDo OAuth2 client secret.
    #[arg(long, env = "LINUXDO_OAUTH_CLIENT_SECRET", hide_env_values = true)]
    linuxdo_oauth_client_secret: Option<String>,

    /// LinuxDo OAuth2 authorize endpoint.
    #[arg(
        long,
        env = "LINUXDO_OAUTH_AUTHORIZE_URL",
        default_value = "https://connect.linux.do/oauth2/authorize"
    )]
    linuxdo_oauth_authorize_url: String,

    /// LinuxDo OAuth2 token endpoint.
    #[arg(
        long,
        env = "LINUXDO_OAUTH_TOKEN_URL",
        default_value = "https://connect.linux.do/oauth2/token"
    )]
    linuxdo_oauth_token_url: String,

    /// LinuxDo OAuth2 userinfo endpoint.
    #[arg(
        long,
        env = "LINUXDO_OAUTH_USERINFO_URL",
        default_value = "https://connect.linux.do/api/user"
    )]
    linuxdo_oauth_userinfo_url: String,

    /// LinuxDo OAuth2 requested scope.
    #[arg(long, env = "LINUXDO_OAUTH_SCOPE", default_value = "user")]
    linuxdo_oauth_scope: String,

    /// OAuth callback URL for this service.
    #[arg(long, env = "LINUXDO_OAUTH_REDIRECT_URL")]
    linuxdo_oauth_redirect_url: Option<String>,

    /// Max age for persisted user session cookie.
    #[arg(long, env = "USER_SESSION_MAX_AGE_SECS", default_value_t = 60 * 60 * 24 * 14)]
    user_session_max_age_secs: i64,

    /// One-time OAuth login state TTL.
    #[arg(long, env = "OAUTH_LOGIN_STATE_TTL_SECS", default_value_t = 600)]
    oauth_login_state_ttl_secs: i64,
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
    let builtin_password_hash = cli
        .admin_auth_builtin_password_hash
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    if let Some(ref hash) = builtin_password_hash {
        PasswordHash::new(hash)
            .map(|_| ())
            .map_err(|_| "ADMIN_AUTH_BUILTIN_PASSWORD_HASH must be a valid PHC string")?;
    }

    if cli.admin_auth_builtin_enabled
        && builtin_password.is_none()
        && builtin_password_hash.is_none()
    {
        return Err(
            "ADMIN_AUTH_BUILTIN_PASSWORD (or ADMIN_AUTH_BUILTIN_PASSWORD_HASH) must be set when ADMIN_AUTH_BUILTIN_ENABLED=true"
                .into(),
        );
    }

    if cli.admin_auth_builtin_enabled {
        match (&builtin_password_hash, &builtin_password) {
            (Some(_), Some(_)) => println!(
                "Built-in auth: both password and password hash are set; using password hash"
            ),
            (None, Some(_)) => println!(
                "Built-in auth: using plaintext password (not recommended); prefer ADMIN_AUTH_BUILTIN_PASSWORD_HASH"
            ),
            _ => {}
        }
    }

    let admin_auth = server::AdminAuthOptions {
        forward_auth_enabled: cli.admin_auth_forward_enabled,
        builtin_auth_enabled: cli.admin_auth_builtin_enabled,
        builtin_auth_password: builtin_password,
        builtin_auth_password_hash: builtin_password_hash,
    };

    let linuxdo_oauth_client_id = cli
        .linuxdo_oauth_client_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let linuxdo_oauth_client_secret = cli
        .linuxdo_oauth_client_secret
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let linuxdo_oauth_redirect_url = cli
        .linuxdo_oauth_redirect_url
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let linuxdo_oauth_scope = {
        let scope = cli.linuxdo_oauth_scope.trim();
        if scope.is_empty() {
            "user".to_string()
        } else {
            scope.to_string()
        }
    };

    if cli.linuxdo_oauth_enabled
        && (linuxdo_oauth_client_id.is_none()
            || linuxdo_oauth_client_secret.is_none()
            || linuxdo_oauth_redirect_url.is_none())
    {
        return Err(
            "LINUXDO_OAUTH_CLIENT_ID, LINUXDO_OAUTH_CLIENT_SECRET and LINUXDO_OAUTH_REDIRECT_URL are required when LINUXDO_OAUTH_ENABLED=true"
                .into(),
        );
    }

    let linuxdo_oauth = server::LinuxDoOAuthOptions {
        enabled: cli.linuxdo_oauth_enabled,
        client_id: linuxdo_oauth_client_id,
        client_secret: linuxdo_oauth_client_secret,
        authorize_url: cli.linuxdo_oauth_authorize_url.trim().to_string(),
        token_url: cli.linuxdo_oauth_token_url.trim().to_string(),
        userinfo_url: cli.linuxdo_oauth_userinfo_url.trim().to_string(),
        scope: linuxdo_oauth_scope,
        redirect_url: linuxdo_oauth_redirect_url,
        session_max_age_secs: cli.user_session_max_age_secs.max(60),
        login_state_ttl_secs: cli.oauth_login_state_ttl_secs.max(60),
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
        linuxdo_oauth,
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
