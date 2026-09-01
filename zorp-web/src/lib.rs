//! Local web UI server for the zorp agent.
//!
//! The server constructs a real `Agent` rather than shelling out to the CLI,
//! so flavors, approval presets, the hard denylist and session persistence all
//! apply unchanged. See
//! `docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`.

pub mod api;
pub mod approval;
pub mod artifacts;
pub mod auth;
pub mod documents;
pub mod event;
#[cfg(feature = "research")]
pub mod investigate;
#[cfg(feature = "memory")]
pub mod memory;
pub mod panel;
pub mod pdf;
#[cfg(feature = "recall")]
pub mod recall;
pub mod renderer;
pub mod settings;
pub mod state;
pub mod title;
pub mod tool_safety;
pub mod turn;
pub mod voice;
