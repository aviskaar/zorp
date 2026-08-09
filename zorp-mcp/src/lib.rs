//! zorp-mcp — MCP client library for zorp-agent.
//!
//! Exposes a fully synchronous API; tokio is hidden inside via `Runtime::block_on`.

pub mod config;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod server;
pub mod tofu;
pub mod transport;

pub use config::{McpConfig, ServerConfig, TransportKind, TrustLevel};
pub use error::McpError;
pub use protocol::{McpTool, mcp_prefix};
pub use registry::McpRegistry;
pub use tofu::McpTofuStore;
