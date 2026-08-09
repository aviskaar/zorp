# Architecture

## Approved

**[zorp architecture](superpowers/specs/2026-08-09-zorp-architecture-design.md)**
is the current, approved design: four standalone capabilities (validate,
experiment, co-write, find a venue), one binary (`zorp-agent`), a shared
foundation (`zorp-track`, specced separately) underneath all four.

This was iterated through an external artifact during design, but that
was always a working sketch, not the durable record; the spec above is
the source of truth now. Decisions behind it are in
[`DECISIONS.md`](DECISIONS.md), including the ones an earlier round of
this design got wrong before ORR and lab-engine/Catalyst were factored
in.

## What's still open

Implementation-plan detail, not architecture: the exact multi-track data
model, and each of the four capabilities' own internal design (each gets
its own spec once `zorp-track`, the foundation, is built, see
[`superpowers/specs/2026-08-09-zorp-track-foundation-design.md`](superpowers/specs/2026-08-09-zorp-track-foundation-design.md)).

Also locked in, and worth knowing about even though they're not part of
the core architecture: memory staying local for now (no Hypermemory
dependency yet), and the scope of zorp's own arXiv paper (a systems paper
about zorp itself, see [`paper/`](paper/)).
