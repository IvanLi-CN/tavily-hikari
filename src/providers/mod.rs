//! Static provider registry (ADR 0005).
//!
//! Metadata-only description of every supported search provider: REST/MCP
//! defaults, upstream key-injection shape, client auth slots, per-operation
//! credit tables, quota-sync capability, and recovery policy. Nothing here
//! rewires the existing Tavily request path, allowlists, or defaults.

mod id;
mod spec;

#[cfg(test)]
mod tests;

pub use id::{ProviderId, UnknownProviderId};
pub use spec::{
    BRAVE, EXA, KeyAuthInjection, McpMode, ProviderSpec, RecoveryPolicy, SERPAPI, TAVILY,
};
