use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Typed error returned by [`ProviderId::from_str`] for unrecognized keys.
///
/// Carries the offending input so callers can build user-facing messages
/// without matching on strings. Parsing NEVER panics on malformed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProviderId {
    pub input: String,
}

impl fmt::Display for UnknownProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown provider id: {:?}", self.input)
    }
}

impl std::error::Error for UnknownProviderId {}

/// Stable identity of every provider this proxy fronts.
///
/// The lowercase keys (`tavily`, `serpapi`, `exa`, `brave`) are the ONE
/// wire/DB spelling shared by serde, [`fmt::Display`], [`FromStr`], and
/// [`ProviderId::as_str`] (the SQLTEXT binding helper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Tavily,
    Serpapi,
    Exa,
    Brave,
}

impl ProviderId {
    /// Stable lowercase key for SQLTEXT binding and serialization. This is the
    /// single source of truth every other textual form delegates to.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tavily => "tavily",
            Self::Serpapi => "serpapi",
            Self::Exa => "exa",
            Self::Brave => "brave",
        }
    }

    /// Resolve a provider from its stable SQLTEXT/wire key.
    ///
    /// Kept as an inherent method so DB readers can parse without importing
    /// [`FromStr`]; both paths yield the same typed error.
    pub fn from_sql_text(value: &str) -> Result<Self, UnknownProviderId> {
        value.parse()
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderId {
    type Err = UnknownProviderId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tavily" => Ok(Self::Tavily),
            "serpapi" => Ok(Self::Serpapi),
            "exa" => Ok(Self::Exa),
            "brave" => Ok(Self::Brave),
            _ => Err(UnknownProviderId {
                input: value.to_owned(),
            }),
        }
    }
}
