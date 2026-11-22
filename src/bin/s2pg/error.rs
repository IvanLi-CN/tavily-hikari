//! Error type for the migration tool. Messages always carry the table and primary key (or
//! table and column) that failed, so an operator can act on the first failure without
//! re-running with extra logging.

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

pub fn sqlite_error(context: impl std::fmt::Display, err: sqlx::Error) -> Error {
    Error::msg(format!("{context}: {err}"))
}

pub fn postgres_error(context: impl std::fmt::Display, err: sqlx::Error) -> Error {
    Error::msg(format!("{context}: {err}"))
}
