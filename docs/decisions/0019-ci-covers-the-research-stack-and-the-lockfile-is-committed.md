---
status: accepted
date: 2026-08-14
---

# CI covers the research stack, and the lockfile is committed

**Decision:** `Cargo.lock` is tracked and CI builds with `--locked`. The
research stack (`zorp-track` plus `zorp-agent --features research`) gets
its own job, running nightly and on pull requests that touch it, while
the per-PR fast path still excludes `zorp-track`. Added a macOS matrix
leg and a `cargo fmt --check` gate. `panic = "abort"` is gone from the
release profile.

**Why:** an entire crate and a feature-gated surface could stop
compiling while main stayed green, which is exactly what "excluded from
CI" means over time. An untracked lockfile made builds
non-reproducible and degraded cache hits. `panic = "abort"` silently
disabled the `catch_unwind` guard around subagent execution in every
release build, so a subagent panic killed the whole process in
production while passing in tests.
