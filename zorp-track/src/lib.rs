//! zorp-track, research-track storage and checkpoints for zorp-agent.
//!
//! Exposes a fully synchronous API. DuckDB is natively synchronous.
//! Behind the non-default `library` feature, LanceDB's async calls are
//! hidden behind an internal `tokio::Runtime::block_on`, the same
//! pattern `zorp-mcp` already uses for its own async transport.

pub mod anomalies;
pub mod calibration;
pub mod checkpoint;
pub mod conditions;
pub mod critique;
pub mod detectors;
pub mod error;
pub mod expectations;
pub mod experiment;
pub mod families;
pub mod id;
pub mod inquiry;
#[cfg(feature = "library")]
pub mod library;
pub mod partition;
pub mod prereg;
pub mod project;
pub mod rerun;
mod schema;
#[cfg(test)]
mod test_support;
pub mod track;
pub mod validation;

pub use critique::{CritiqueFinding, CritiqueRound};
pub use error::TrackError;
pub use project::Project;
pub use track::Store;
pub use validation::{Citation, Validation};
