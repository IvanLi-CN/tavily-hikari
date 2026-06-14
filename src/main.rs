mod server;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use argon2::password_hash::PasswordHash;
use clap::Parser;
use dotenvy::dotenv;
use tavily_hikari::{
    DEFAULT_UPSTREAM, HaConfig, HaMode, LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT, TavilyProxy,
    TavilyProxyOptions,
};

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

    /// Xray binary path used for share-link based forward proxies.
    #[arg(long, env = "XRAY_BINARY", default_value = "xray")]
    xray_binary: String,

    /// Xray runtime directory for generated per-node configs.
    #[arg(long, env = "XRAY_RUNTIME_DIR")]
    xray_runtime_dir: Option<PathBuf>,

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

    /// Low remaining-credit threshold for suppressing monthly auto-restore after Tavily 432.
    #[arg(
        long,
        env = "LOW_QUOTA_DEPLETION_THRESHOLD",
        default_value_t = LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT.to_string()
    )]
    low_quota_depletion_threshold: String,

    /// Hosted API origin used to resolve registration IP geo metadata for imported API keys.
    #[arg(
        long,
        env = "API_KEY_IP_GEO_ORIGIN",
        default_value = "https://api.country.is"
    )]
    api_key_ip_geo_origin: String,

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

    /// Encryption key used to persist LinuxDo refresh tokens (32 raw bytes or base64/base64url).
    #[arg(
        long,
        env = "LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY",
        hide_env_values = true
    )]
    linuxdo_oauth_refresh_token_crypt_key: Option<String>,

    /// Enable/disable the daily LinuxDo user profile sync scheduler.
    #[arg(long, env = "LINUXDO_OAUTH_USER_SYNC_ENABLED", default_value_t = true)]
    linuxdo_oauth_user_sync_enabled: bool,

    /// Daily LinuxDo user profile sync time in server local time (`HH:mm`).
    #[arg(long, env = "LINUXDO_OAUTH_USER_SYNC_AT", default_value = "06:20")]
    linuxdo_oauth_user_sync_at: String,

    /// Max age for persisted user session cookie.
    #[arg(long, env = "USER_SESSION_MAX_AGE_SECS", default_value_t = 60 * 60 * 24 * 14)]
    user_session_max_age_secs: i64,

    /// One-time OAuth login state TTL.
    #[arg(long, env = "OAUTH_LOGIN_STATE_TTL_SECS", default_value_t = 600)]
    oauth_login_state_ttl_secs: i64,

    /// Enable LinuxDo Credit recharge payment flow.
    #[arg(long, env = "LINUXDO_CREDIT_ENABLED", default_value_t = false)]
    linuxdo_credit_enabled: bool,

    /// LinuxDo Credit application client id.
    #[arg(long, env = "LINUXDO_CREDIT_CLIENT_ID")]
    linuxdo_credit_client_id: Option<String>,

    /// LinuxDo Credit application client secret.
    #[arg(long, env = "LINUXDO_CREDIT_CLIENT_SECRET", hide_env_values = true)]
    linuxdo_credit_client_secret: Option<String>,

    /// Ed25519 merchant private key for LinuxDo Credit LDC signing.
    #[arg(
        long,
        env = "LINUXDO_CREDIT_MERCHANT_PRIVATE_KEY",
        hide_env_values = true
    )]
    linuxdo_credit_merchant_private_key: Option<String>,

    /// LinuxDo Credit LDC submit endpoint.
    #[arg(
        long,
        env = "LINUXDO_CREDIT_SUBMIT_URL",
        default_value = "https://credit.linux.do/epay/pay/submit.php"
    )]
    linuxdo_credit_submit_url: String,

    /// Optional order-level LinuxDo Credit notify URL.
    #[arg(long, env = "LINUXDO_CREDIT_NOTIFY_URL")]
    linuxdo_credit_notify_url: Option<String>,

    /// Optional order-level LinuxDo Credit return URL.
    #[arg(long, env = "LINUXDO_CREDIT_RETURN_URL")]
    linuxdo_credit_return_url: Option<String>,

    /// Enable test pricing: 1 LDC buys 1 monthly credit.
    #[arg(
        long,
        env = "LINUXDO_CREDIT_TEST_PRICE_ENABLED",
        default_value_t = false
    )]
    linuxdo_credit_test_price_enabled: bool,

    /// HA mode: single or active_standby.
    #[arg(long, env = "HA_MODE", default_value = "single")]
    ha_mode: String,

    /// Stable node id for HA state and audit messages.
    #[arg(long, env = "NODE_ID", default_value = "single")]
    node_id: String,

    /// Default HA source kind for this instance: direct or origin_group.
    #[arg(long, env = "HA_SOURCE_KIND")]
    ha_source_kind: Option<String>,

    /// Default EdgeOne origin group id for this instance.
    #[arg(long, env = "HA_SOURCE_ORIGIN_GROUP_ID")]
    ha_source_origin_group_id: Option<String>,

    /// Public EdgeOne origin scheme for this node: http, https, or follow.
    #[arg(long, env = "NODE_PUBLIC_SCHEME")]
    node_public_scheme: Option<String>,

    /// Public EdgeOne origin host for this node.
    #[arg(long, env = "NODE_PUBLIC_HOST")]
    node_public_host: Option<String>,

    /// Public EdgeOne origin port for this node. Defaults from NODE_PUBLIC_SCHEME when omitted.
    #[arg(long, env = "NODE_PUBLIC_PORT")]
    node_public_port: Option<u16>,

    /// Tencent EdgeOne zone id.
    #[arg(long, env = "EDGEONE_ZONE_ID")]
    edgeone_zone_id: Option<String>,

    /// Tencent EdgeOne acceleration domain.
    #[arg(long, env = "EDGEONE_DOMAIN")]
    edgeone_domain: Option<String>,

    /// Expected previous EdgeOne origin scheme before non-force promotion: http, https, or follow.
    #[arg(long, env = "EDGEONE_EXPECTED_ORIGIN_SCHEME")]
    edgeone_expected_origin_scheme: Option<String>,

    /// Expected previous EdgeOne origin host before non-force promotion.
    #[arg(long, env = "EDGEONE_EXPECTED_ORIGIN_HOST")]
    edgeone_expected_origin_host: Option<String>,

    /// Expected previous EdgeOne origin port before non-force promotion.
    #[arg(long, env = "EDGEONE_EXPECTED_ORIGIN_PORT")]
    edgeone_expected_origin_port: Option<u16>,

    /// Tencent Cloud secret id for EdgeOne API.
    #[arg(long, env = "EDGEONE_SECRET_ID", hide_env_values = true)]
    edgeone_secret_id: Option<String>,

    /// Tencent Cloud secret key for EdgeOne API.
    #[arg(long, env = "EDGEONE_SECRET_KEY", hide_env_values = true)]
    edgeone_secret_key: Option<String>,

    /// Tencent EdgeOne API endpoint.
    #[arg(
        long,
        env = "EDGEONE_API_ENDPOINT",
        default_value = "https://teo.intl.tencentcloudapi.com"
    )]
    edgeone_api_endpoint: String,

    /// Direct active/admin source URL used by standby pull-based HA sync.
    #[arg(long, env = "HA_SYNC_SOURCE_URL")]
    ha_sync_source_url: Option<String>,

    /// Shared internal token for node-to-node HA sync calls.
    #[arg(long, env = "HA_INTERNAL_TOKEN", hide_env_values = true)]
    ha_internal_token: Option<String>,

    /// HA standby pull sync interval in seconds, clamped to 5-15.
    #[arg(long, env = "HA_SYNC_INTERVAL_SECS", default_value_t = 15)]
    ha_sync_interval_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    reject_legacy_ha_origin_env_vars()?;

    // Ensure parent directory for database exists when using nested path like data/tavily_proxy.db
    let db_path = Path::new(&cli.db_path);
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    println!("Using database: {}", db_path.display());

    let proxy_options = TavilyProxyOptions {
        xray_binary: cli.xray_binary,
        xray_runtime_dir: cli.xray_runtime_dir.unwrap_or_else(|| {
            TavilyProxyOptions::from_database_path(&cli.db_path).xray_runtime_dir
        }),
        forward_proxy_trace_url: TavilyProxyOptions::from_database_path(&cli.db_path)
            .forward_proxy_trace_url,
        low_quota_depletion_threshold: tavily_hikari::parse_low_quota_depletion_threshold(
            Some(&cli.low_quota_depletion_threshold),
            "LOW_QUOTA_DEPLETION_THRESHOLD",
        ),
        health_readiness_grace_period: std::time::Duration::from_secs(90),
    };
    let proxy =
        TavilyProxy::with_options(cli.keys, &cli.upstream, &cli.db_path, proxy_options).await?;
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
    let linuxdo_oauth_refresh_token_crypt_key = match parse_linuxdo_refresh_token_crypt_key(
        cli.linuxdo_oauth_refresh_token_crypt_key,
    ) {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "warning: {err}; LinuxDo refresh-token persistence and daily user sync will stay disabled"
            );
            None
        }
    };
    let linuxdo_oauth_user_sync_at =
        parse_hhmm(&cli.linuxdo_oauth_user_sync_at).ok_or_else(|| {
            format!(
                "LINUXDO_OAUTH_USER_SYNC_AT must use HH:mm format, got '{}'",
                cli.linuxdo_oauth_user_sync_at
            )
        })?;

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
        refresh_token_crypt_key: linuxdo_oauth_refresh_token_crypt_key,
        user_sync_enabled: cli.linuxdo_oauth_user_sync_enabled,
        user_sync_at: linuxdo_oauth_user_sync_at,
        session_max_age_secs: cli.user_session_max_age_secs.max(60),
        login_state_ttl_secs: cli.oauth_login_state_ttl_secs.max(60),
    };
    let linuxdo_credit = server::LinuxDoCreditOptions {
        enabled: cli.linuxdo_credit_enabled,
        client_id: cli
            .linuxdo_credit_client_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        client_secret: cli
            .linuxdo_credit_client_secret
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        merchant_private_key: cli
            .linuxdo_credit_merchant_private_key
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        submit_url: cli.linuxdo_credit_submit_url.trim().to_string(),
        notify_url: cli
            .linuxdo_credit_notify_url
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        return_url: cli
            .linuxdo_credit_return_url
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        test_price_enabled: cli.linuxdo_credit_test_price_enabled,
    };
    let ha_config = HaConfig {
        mode: HaMode::parse(&cli.ha_mode),
        node_id: cli.node_id.trim().to_string(),
        database_path: Some(cli.db_path.clone()),
        source_kind: cli
            .ha_source_kind
            .as_deref()
            .and_then(tavily_hikari::parse_ha_source_kind),
        source_origin_group_id: trim_optional(cli.ha_source_origin_group_id),
        node_public_scheme: trim_optional(cli.node_public_scheme),
        node_public_host: trim_optional(cli.node_public_host),
        node_public_port: cli.node_public_port,
        edgeone_zone_id: trim_optional(cli.edgeone_zone_id),
        edgeone_domain: trim_optional(cli.edgeone_domain),
        edgeone_expected_origin_scheme: trim_optional(cli.edgeone_expected_origin_scheme),
        edgeone_expected_origin_host: trim_optional(cli.edgeone_expected_origin_host),
        edgeone_expected_origin_port: cli.edgeone_expected_origin_port,
        edgeone_secret_id: trim_optional(cli.edgeone_secret_id),
        edgeone_secret_key: trim_optional(cli.edgeone_secret_key),
        edgeone_api_endpoint: cli.edgeone_api_endpoint.trim().to_string(),
        sync_source_url: trim_optional(cli.ha_sync_source_url),
        internal_token: trim_optional(cli.ha_internal_token),
        sync_interval_secs: cli.ha_sync_interval_secs,
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
        cli.api_key_ip_geo_origin,
        ha_config,
        linuxdo_oauth,
        linuxdo_credit,
    )
    .await?;

    Ok(())
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn reject_legacy_ha_origin_env_vars() -> Result<(), Box<dyn std::error::Error>> {
    let legacy_vars = [
        "NODE_PUBLIC_ORIGIN",
        "EDGEONE_EXPECTED_ORIGIN",
        "HA_SYNC_PEER_URL",
    ];
    let configured = legacy_vars
        .into_iter()
        .filter(|name| {
            std::env::var(name)
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>();

    if configured.is_empty() {
        return Ok(());
    }

    Err(format!(
        "{} are no longer supported; use NODE_PUBLIC_SCHEME, NODE_PUBLIC_HOST, NODE_PUBLIC_PORT, EDGEONE_EXPECTED_ORIGIN_SCHEME, EDGEONE_EXPECTED_ORIGIN_HOST, EDGEONE_EXPECTED_ORIGIN_PORT, and HA_SYNC_SOURCE_URL",
        configured.join(", ")
    )
    .into())
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

fn parse_hhmm(raw: &str) -> Option<(u32, u32)> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let hour = parts.next()?;
    let minute = parts.next()?;
    if parts.next().is_some() || hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    (hour <= 23 && minute <= 59).then_some((hour, minute))
}

fn parse_linuxdo_refresh_token_crypt_key(
    value: Option<String>,
) -> Result<Option<[u8; 32]>, String> {
    fn decode_32_bytes(raw: &str) -> Option<[u8; 32]> {
        use base64::Engine as _;
        use base64::engine::general_purpose::{
            STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD,
        };

        for engine in [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD] {
            if let Ok(decoded) = engine.decode(raw)
                && decoded.len() == 32
            {
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                return Some(key);
            }
        }
        None
    }

    let Some(raw) = value
        .map(|it| it.trim().to_owned())
        .filter(|it| !it.is_empty())
    else {
        return Ok(None);
    };

    if raw.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(raw.as_bytes());
        return Ok(Some(key));
    }

    decode_32_bytes(&raw).map(Some).ok_or_else(|| {
        "LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY must be 32 raw bytes or decode to 32 bytes from base64/base64url".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hhmm_accepts_strict_two_digit_format() {
        assert_eq!(parse_hhmm("06:20"), Some((6, 20)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(parse_hhmm("6:20"), None);
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("06:60"), None);
    }

    #[test]
    fn parse_linuxdo_refresh_token_crypt_key_accepts_raw_and_base64_inputs() {
        use base64::Engine as _;

        let raw = "0123456789abcdef0123456789abcdef".to_string();
        let parsed = parse_linuxdo_refresh_token_crypt_key(Some(raw.clone()))
            .expect("parse raw key")
            .expect("raw key");
        assert_eq!(parsed, *b"0123456789abcdef0123456789abcdef");

        let base64_key = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        let parsed = parse_linuxdo_refresh_token_crypt_key(Some(base64_key))
            .expect("parse base64 key")
            .expect("decoded base64 key");
        assert_eq!(parsed, *b"0123456789abcdef0123456789abcdef");

        let url_safe_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
        let parsed = parse_linuxdo_refresh_token_crypt_key(Some(url_safe_key))
            .expect("parse base64url key")
            .expect("decoded base64url key");
        assert_eq!(parsed, *b"0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn parse_linuxdo_refresh_token_crypt_key_rejects_invalid_lengths() {
        let err = parse_linuxdo_refresh_token_crypt_key(Some("short-key".to_string()))
            .expect_err("invalid key should be rejected");
        assert!(err.contains("decode to 32 bytes"));
    }
}
