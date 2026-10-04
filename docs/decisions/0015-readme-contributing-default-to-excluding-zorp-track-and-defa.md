---
status: accepted
date: 2026-08-13
---

# README/CONTRIBUTING default to excluding zorp-track, and default-run fixes the ambiguous zorp-agent binary

**Decision:** README and CONTRIBUTING now lead with
`cargo build --workspace --exclude zorp-track` / `cargo test --workspace
--exclude zorp-track` instead of the plain `--workspace` forms, with a
note on the 20-30 minute cold build if `zorp-track` is included. README
also gained a short section on connecting an MCP tool for
`validate`/`deliver`, since neither can run without one and the failure
mode gave no pointer to the fix. Separately, `zorp-agent/Cargo.toml`
now sets `default-run = "zorp-agent"`, since the test-only
`stub_search_mcp_server` binary made `cargo run -p zorp-agent -- "<task>"`
(the exact command in the README) fail with an ambiguous-binary error.
`src/main.rs`'s one-shot path also now prints a hint to set
`ZORP_API_KEY` when a request fails with no key configured, instead of
just surfacing the raw HTTP status.

**Why:** usability testing of the public-release candidate found that a
first-time user following the README verbatim hit all four of these in
the first few minutes: an unexplained 20-30 minute build, a broken
documented command, an unrunnable flagship capability with no
documented fix, and an opaque 401 with no actionable next step. Each is
a small, targeted fix; none change behavior for anyone already working
around them.
