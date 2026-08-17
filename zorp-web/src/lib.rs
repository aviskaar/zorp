//! Local web UI server for the zorp agent.
//!
//! The server constructs a real `Agent` rather than shelling out to the CLI,
//! so flavors, approval presets, the hard denylist and session persistence all
//! apply unchanged. See
//! `docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`.

pub mod api;
