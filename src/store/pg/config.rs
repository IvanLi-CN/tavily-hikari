//! Connection settings and the startup storage-mode decision for the PostgreSQL store.

/// Conservative default pool size while the runtime still serves from SQLite.
pub const DEFAULT_PG_MAX_CONNECTIONS: u32 = 4;

/// Resolved PostgreSQL connection settings.
///
/// The database URL is kept as supplied by the operator (query parameters included) so
/// that TLS and timeout options reach `sqlx` unchanged; use [`PgConfig::redacted_url`]
/// whenever the URL is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgConfig {
    database_url: String,
    auto_migrate: bool,
    max_connections: u32,
}

/// Which storage backend this process starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PgStartupPlan {
    /// No database URL configured: the existing SQLite path runs unchanged.
    Sqlite,
    /// PostgreSQL plumbing runs before the server starts.
    Postgres(PgConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PgConfigError {
    #[error("database URL is not a valid URL: {message}")]
    InvalidUrl { message: String },
    #[error("database URL must use the postgres:// or postgresql:// scheme, got '{scheme}'")]
    UnsupportedScheme { scheme: String },
}

impl PgConfig {
    pub fn new(database_url: &str, auto_migrate: bool) -> Result<Self, PgConfigError> {
        Ok(Self {
            database_url: validate_database_url(database_url.trim())?,
            auto_migrate,
            max_connections: DEFAULT_PG_MAX_CONNECTIONS,
        })
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn auto_migrate(&self) -> bool {
        self.auto_migrate
    }

    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }

    pub fn with_max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections.max(1);
        self
    }

    /// Database URL with the password and query parameters masked for logs.
    pub fn redacted_url(&self) -> String {
        let Ok(mut parsed) = url::Url::parse(&self.database_url) else {
            return "<unparsable database URL>".to_owned();
        };
        if parsed.password().is_some() {
            let _ = parsed.set_password(Some("***"));
        }
        parsed.set_query(None);
        parsed.to_string()
    }
}

impl PgStartupPlan {
    /// Resolves the startup storage mode from the CLI/env inputs.
    ///
    /// A blank database URL is treated as absent so an empty `PROXY_DATABASE_URL` keeps the
    /// SQLite path instead of failing startup.
    pub fn resolve(database_url: Option<&str>, auto_migrate: bool) -> Result<Self, PgConfigError> {
        let Some(database_url) = database_url.map(str::trim).filter(|url| !url.is_empty()) else {
            return Ok(Self::Sqlite);
        };
        PgConfig::new(database_url, auto_migrate).map(Self::Postgres)
    }
}

fn validate_database_url(raw: &str) -> Result<String, PgConfigError> {
    if raw.is_empty() {
        return Err(PgConfigError::InvalidUrl {
            message: "empty value".to_owned(),
        });
    }
    let parsed = url::Url::parse(raw).map_err(|err| PgConfigError::InvalidUrl {
        message: err.to_string(),
    })?;
    match parsed.scheme() {
        "postgres" | "postgresql" => Ok(raw.to_owned()),
        scheme => Err(PgConfigError::UnsupportedScheme {
            scheme: scheme.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PG_MAX_CONNECTIONS, PgConfig, PgConfigError, PgStartupPlan};

    const URL: &str = "postgres://app:pw@db.internal:5432/hikari";

    #[test]
    fn startup_plan_without_database_url_keeps_sqlite() {
        assert_eq!(
            PgStartupPlan::resolve(None, true).expect("absent url"),
            PgStartupPlan::Sqlite
        );
        assert_eq!(
            PgStartupPlan::resolve(None, false).expect("absent url"),
            PgStartupPlan::Sqlite
        );
        assert_eq!(
            PgStartupPlan::resolve(Some("   "), true).expect("blank url"),
            PgStartupPlan::Sqlite
        );
    }

    #[test]
    fn startup_plan_with_database_url_auto_migrates_by_default() {
        let PgStartupPlan::Postgres(config) =
            PgStartupPlan::resolve(Some(URL), true).expect("valid url")
        else {
            panic!("expected the postgresql startup plan");
        };
        assert!(config.auto_migrate());
        assert_eq!(config.database_url(), URL);
        assert_eq!(config.max_connections(), DEFAULT_PG_MAX_CONNECTIONS);
    }

    #[test]
    fn startup_plan_honours_no_auto_migrate() {
        let PgStartupPlan::Postgres(config) =
            PgStartupPlan::resolve(Some(URL), false).expect("valid url")
        else {
            panic!("expected the postgresql startup plan");
        };
        assert!(!config.auto_migrate());
        assert_eq!(config.database_url(), URL);
    }

    #[test]
    fn startup_plan_rejects_malformed_database_urls() {
        let err = PgStartupPlan::resolve(Some("sqlite:///var/lib/hikari.db"), true)
            .expect_err("sqlite url must be rejected");
        assert_eq!(
            err,
            PgConfigError::UnsupportedScheme {
                scheme: "sqlite".to_owned()
            }
        );
        assert!(err.to_string().contains("postgres://"));

        let err = PgStartupPlan::resolve(Some("not a url"), true).expect_err("garbage url");
        assert!(matches!(err, PgConfigError::InvalidUrl { .. }));
        assert!(err.to_string().contains("not a valid URL"));
    }

    #[test]
    fn pool_size_is_configurable_and_never_zero() {
        let config = PgConfig::new(URL, true)
            .expect("valid url")
            .with_max_connections(0);
        assert_eq!(config.max_connections(), 1);
        assert_eq!(
            PgConfig::new(URL, true)
                .expect("valid url")
                .with_max_connections(12)
                .max_connections(),
            12
        );
    }

    #[test]
    fn redacted_url_hides_credentials_and_query_parameters() {
        let config = PgConfig::new(
            "postgres://app:super-secret@db.internal:5432/hikari?sslmode=require",
            true,
        )
        .expect("valid url");
        let redacted = config.redacted_url();
        assert!(!redacted.contains("super-secret"), "{redacted}");
        assert!(!redacted.contains("sslmode"), "{redacted}");
        assert_eq!(redacted, "postgres://app:***@db.internal:5432/hikari");
    }
}
