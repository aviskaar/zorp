---
status: accepted
date: 2026-08-13
---

# CI excludes zorp-track from the default workspace test run

**Decision:** `.github/workflows/ci.yml` runs
`cargo test --workspace --exclude zorp-track` instead of
`cargo test --workspace`, and no longer runs
`cargo test -p zorp-agent --features research` at all. Both remain
required locally before considering Rust changes done (see CLAUDE.md);
CI just doesn't enforce them yet.

**Why:** `zorp-track` is the only workspace crate depending on `duckdb`
(bundled — compiles DuckDB's C++ amalgamation from source) and
`lancedb` (pulls in Arrow and DataFusion transitively). On a cold
cargo cache, compiling it took CI past 20+ minutes on GitHub's shared
runners, several times in a row, before `Swatinem/rust-cache` ever got
a chance to save a cache (it only saves on successful job completion,
so a run cancelled for taking too long guarantees the next run is cold
too). Excluding it keeps CI fast and cheap for every other crate, at
the cost of not catching zorp-track/research-feature regressions in CI
until a better strategy (self-hosted runner, a slower opt-in workflow,
or seeding the cache once) is worth the cost.

**Ruled out (for now):** letting one run finish uncapped to seed the
cache, and requesting a larger/paid runner — both reduce or shift cost
rather than remove it, so scope reduction was chosen instead.
