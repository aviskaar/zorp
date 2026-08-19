# zorp-agent critique: by-hand run of the shipped binary

**Date:** 2026-08-18
**Build under test:** `zorp-agent 0.3.2` on `feat/self-critique`, debug
binary, built with `--features research`
**Backend:** a canned OpenAI-compatible completions endpoint (a 40 line
local HTTP server replaying a fixed script), not a live model

## Why a canned backend

The point of this run is the binary's plumbing: subcommand parsing, the
project store, the ledger built from real DuckDB rows, the files written
to `.zorp/tracks/<id>/`, and the model-call count. None of that is about
what a model says, and a scripted backend makes the call count checkable,
which a live model does not. Behaviour with a real model is not what this
run covers.

## Setup

A fresh git-initialised directory, then the normal sequence:

```
zorp-agent investigate "does caching help" \
  --metric-name latency_ms --kill-threshold 100 \
  --threshold-direction lower-is-better --yes --base-url <canned>
zorp-agent co-write "does caching help" --yes --base-url <canned>
```

The scripted `co-write` answer states one figure the record holds
(`latency_ms = 42`) and two it does not (`900` requests per second, `7`
errors per million), with the invented pair in an uncited sentence.

## Scenarios

| # | Scenario | Expected | Result |
|---|----------|----------|--------|
| 1 | `critique` on a project with no draft | refuses, names co-write, exit 1, no model call | pass |
| 2 | `critique --help` | lists `--critique-rounds` and says 0 means audit only | pass |
| 3 | `critique` on the drafted artifact | 3 findings, 1 revision round, draft revised | pass |
| 4 | model calls made by scenario 3 | 3 (audit, revise, audit) | pass |
| 5 | `draft.pre-critique.md` written | holds the pre-critique draft verbatim | pass |
| 6 | `diff draft.pre-critique.md draft.md` | shows exactly the invented sentence removed | pass |
| 7 | `critique.md` written | names each finding, its kind, the sentence, and the verdict on the revision | pass |
| 8 | `prereg.md` after the run | byte identical, kill threshold still 100 | pass |
| 9 | `critique` re-run on the now-clean draft | reports the draft is supported, changes nothing | pass |
| 10 | model calls made by scenario 9 | 1 (audit only, no revision requested) | pass |

## Scenario 3, verbatim

```
critique: 3 finding(s), 0 left after 1 revision round(s). draft.md revised,
original kept at .zorp/tracks/2026-08-18-does-caching-help/draft.pre-critique.md.
Notes at .zorp/tracks/2026-08-18-does-caching-help/critique.md
```

The two `number-not-in-record` findings were produced with no model
involvement at all: the critic's claim inventory named only the uncited
sentence, and the numeric audit found `900` and `7` on its own.

## Scenario 9, verbatim

```
critique: the draft is supported by the record as it stands; nothing changed.
Notes at .zorp/tracks/2026-08-18-does-caching-help/critique.md
```

## Not covered here

- Behaviour against a live model, including whether a real critic
  extracts claims faithfully.
- The record-immutability guard firing. That needs an agent with a write
  tool aimed at `prereg.md`, which is covered by
  `the_pass_cannot_move_the_kill_threshold` in
  `zorp-agent/src/critique/mod.rs` rather than by hand.
- Interactive checkpoints. Every scenario above ran with `--yes`.
