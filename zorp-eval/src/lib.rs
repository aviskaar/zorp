// Library entrypoint

/// Shared boxed error, mirroring the core so `?` composes across crates.
pub type BoxErr = Box<dyn std::error::Error + Send + Sync>;

pub mod bench;
pub mod config;
pub mod contracts;
pub mod grader;
pub mod harness;
pub mod manifest;
pub mod runner;
pub mod snapshot;
