//! zorp-track, research-track storage and checkpoints for zorp-agent.
//!
//! Exposes a fully synchronous API. DuckDB is natively synchronous.
//! Behind the non-default `library` feature, LanceDB's async calls are
//! hidden behind an internal `tokio::Runtime::block_on`, the same
//! pattern `zorp-mcp` already uses for its own async transport.

pub mod checkpoint;
pub mod error;
pub mod experiment;
pub mod id;
#[cfg(feature = "library")]
pub mod library;
pub mod prereg;
pub mod project;
mod schema;
pub mod track;
pub mod validation;

pub use error::TrackError;
pub use project::Project;
pub use track::Store;
pub use validation::{Citation, Validation};
