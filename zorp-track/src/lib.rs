//! zorp-track, research-track storage and checkpoints for zorp-agent.
//!
//! Exposes a fully synchronous API. DuckDB is natively synchronous;
//! LanceDB's async calls are hidden behind an internal
//! `tokio::Runtime::block_on`, the same pattern `zorp-mcp` already uses
//! for its own async transport.

pub mod error;
pub mod id;

pub use error::TrackError;
