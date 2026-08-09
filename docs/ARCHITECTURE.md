# Architecture

## Approved

**[zorp architecture](superpowers/specs/2026-08-09-zorp-architecture-design.md)**
is the current, approved design: four standalone capabilities, one
binary (`zorp-agent`), a shared foundation (`zorp-track`, specced
separately and built) underneath all four.

**[zorp scope and positioning](superpowers/specs/2026-08-09-zorp-scope-and-positioning.md)**
amends the capability names and what zorp is actually for: validate,
investigate, co-write, deliver, targeting any evidence-based question,
not academic research specifically. Read this one for current names and
scope; the architecture spec above for structure, which this doesn't
change.

This was iterated through an external artifact during design, but that
was always a working sketch, not the durable record; the specs above are
the source of truth now. Decisions behind them are in
[`DECISIONS.md`](DECISIONS.md), including the ones earlier rounds of
this design got wrong before ORR and lab-engine/Catalyst were factored
in, and before the scope broadened past academia.

## What's built

`zorp-track`: the multi-track data model, DuckDB run record, git-backed
pre-registration with tamper evidence, index rebuild, typed
experiments/metrics, the checkpoint primitive, and LanceDB provisioning.
Wired into `zorp-agent` behind an optional `research` feature. See
[`superpowers/specs/2026-08-09-zorp-track-foundation-design.md`](superpowers/specs/2026-08-09-zorp-track-foundation-design.md).

## What's still open

Each of the four capabilities (validate, investigate, co-write, deliver)
gets its own spec, written against the broadened scope, before any of
them get built.

Also locked in, and worth knowing about even though they're not part of
the core architecture: memory staying local for now (no Hypermemory
dependency yet), and the scope of zorp's own arXiv paper (a systems paper
about zorp itself, unaffected by the scope broadening, see
[`paper/`](paper/)).
