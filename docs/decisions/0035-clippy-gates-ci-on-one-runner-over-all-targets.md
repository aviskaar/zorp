---
status: accepted
date: 2026-08-17
---

# clippy gates CI, on one runner, over all targets

**Decision:** CI runs `cargo clippy --workspace --exclude zorp-track
--all-targets --locked -- -D warnings` in a single ubuntu job. Warnings
fail the build.

**Why:** clippy had never run in CI at all. Nothing gated it, so
warnings accumulated unnoticed. Among them: dead test scaffolding, a
`mut` that was never needed, a length comparison to zero, an argument
list that had outgrown the lint's limit, and a std lock held across an
await in zorp-web's tests. That last one is test-only. It serialises
tests that write process-global env vars, and no production path
touches it. A check that only prints is a check nobody reads, which is
how the pile grew, so this one fails instead. Every warning was cleared
before the gate went in, so it starts from zero.

One runner rather than the two-OS test matrix: every lint clippy found
here is platform independent, and the macOS test leg still compiles the
platform-specific code. `--all-targets` because most of the warnings
were in test code, and linting the library alone would have gated the
easy half.

**What it rules out:** nothing lints `zorp-track` or the `research`
feature. That is deliberate, for the same reason the fast test job skips
zorp-track: it builds DuckDB from source and would dominate the job.
Closing that gap means adding a clippy step to the research jobs, which
already carry the warm shared cache for that tree. It is a known gap,
not an oversight.
