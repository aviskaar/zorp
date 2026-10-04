---
status: accepted
date: 2026-08-14
---

# the vector library is opt-in, not part of research

**Decision:** LanceDB moves behind a non-default `library` feature in
`zorp-track`, with a matching opt-in feature in `zorp-agent` that
`research` deliberately does not enable. `Project::library` opens
lazily, and `validate` skips the embed-and-insert step when the feature
is off.

**Why:** it was a write-only sink. `validate` wrote cited sources into
it, nothing ever read them back, and the citations `co-write` actually
uses come from the DuckDB `validations` columns. It cost roughly 390 of
`zorp-track`'s dependencies (the whole arrow and datafusion tree) for no
behavior. It stays available rather than deleted, because a retrieval
story is a plausible future.
