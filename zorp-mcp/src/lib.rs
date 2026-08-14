//! zorp-mcp: MCP client library for zorp-agent.
//!
//! Exposes a fully synchronous API built on blocking std::process pipes
//! and blocking HTTP (ureq). No async runtime is involved.

pub mod config;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod server;
pub mod tofu;
pub mod transport;

pub use config::{McpConfig, ServerConfig, TransportKind, TrustLevel};
pub use error::McpError;
pub use protocol::{mcp_prefix, McpTool};
pub use registry::McpRegistry;
pub use tofu::McpTofuStore;
