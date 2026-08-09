# zorp-track: the research-loop foundation

**Date:** 2026-08-09
**Status:** approved

## Purpose

zorp's product is four standalone capabilities: validate an idea, run an
experiment, co-write a paper, find a venue. None of them exist yet, and
none of them can be built until the ground they stand on exists: a
record of research investigations (tracks), a durable and queryable
record of what was tried (the run record), and a mechanism for pausing
at critical steps so a human stays in control.

This spec covers that foundation only. It does not design validate,
experiment, co-write, or find a venue themselves; each of those gets its
own spec once this foundation exists. See `docs/ARCHITECTURE.md` for how
this fits the larger proposal, and `docs/DECISIONS.md` for the decisions
this spec builds on rather than re-derives (one binary, pre-registration
always required, no hard experiment budget, interactive checkpoints by
default, typed run-record metrics, live venue-API matching, multi-track
from day one, abstract-level venue matching).

## Where this lives

A new internal crate, `zorp-track`, added as a workspace member.
`zorp-agent` depends on it behind a new optional `research` feature,
the same pattern `zorp-mcp` already uses behind the `mcp` feature. This
keeps the DuckDB, LanceDB, and Arrow dependency tree opt-in rather than
pulling it into every build of `zorp-agent`, consistent with how the
`otel` feature already isolates its own heavier dependencies.

`zorp-track` owns: the track data model, the DuckDB run record, the
LanceDB store, pre-registration file and row management, and the
checkpoint primitive. It does not know about validate, experiment,
co-write, or find a venue; those are built on top of it later as
`zorp-agent` subcommands.

## On disk

Per project, created on first use:

```
.zorp/
  zorp.duckdb          # gitignored, regenerable index
  lancedb/              # gitignored, regenerable index
  tracks/
    <track-id>/
      prereg.md          # git-tracked, source of truth for integrity
```

`zorp.duckdb` and `lancedb/` are derived indexes over the tracked
`prereg.md` files: if either store is lost or corrupted, it can be
rebuilt by re-reading every `prereg.md` under `tracks/`. The prereg files
are the actual record, chosen specifically because a git-committed,
human-readable file gives the same tamper-evidence guarantee Catalyst's
`prereg.md` convention already relies on (a git commit timestamp that
can't be quietly moved after seeing results), which a binary DuckDB row
alone cannot provide.

`.zorp/.gitignore` (or an entry in the project's own `.gitignore`) covers
`zorp.duckdb` and `lancedb/`. `tracks/*/prereg.md` is meant to be
committed by the user as part of their normal repo history, one commit
per pre-registration, before any experiment code exists for it.

## Track identity

A track id is a date-prefixed slug generated from the hypothesis text,
e.g. `2026-08-09-adaptive-memory-consolidation`, matching the convention
ORR and lab-engine's idea triage already use. Sorts chronologically,
human-readable in directory listings and DuckDB query results alike.

## DuckDB schema

- **tracks**: `id` (text, primary key), `hypothesis` (text), `status`
  (text: `active` / `paused` / `completed` / `killed`), `created_at`,
  `updated_at`.
- **preregistrations**: `id`, `track_id` (foreign key), `hypothesis_snapshot`
  (text, copied at prereg time so later edits to `tracks.hypothesis`
  don't retroactively change what was registered), `metric_name` (text),
  `kill_threshold` (double), `file_path` (text, path to the `prereg.md`),
  `file_hash` (text, SHA-256 of the file at commit time), `git_commit_hash`
  (text), `committed_at`.
- **experiments**: `id`, `track_id`, `prereg_id`, `status` (text:
  `planned` / `running` / `completed` / `failed` / `killed`),
  `started_at`, `completed_at`.
- **metrics**: `id`, `experiment_id`, `key` (text), `value_type` (text:
  `number` / `string` / `bool`), `value_number` (double, nullable),
  `value_string` (text, nullable), `value_bool` (boolean, nullable),
  `recorded_at`. DuckDB has no single variant column, so the value lives
  in whichever of the three typed columns matches `value_type`; the other
  two stay null. Typed key-value pairs, not narrative logs, per the
  run-record decision already logged.
- **checkpoints**: `id`, `track_id`, `kind` (text: which capability this
  checkpoint belongs to, left open-ended rather than a fixed enum, since
  capabilities beyond the four already named may add their own), `status`
  (text: `pending` / `approved` / `rejected`), `prompt_shown` (text),
  `decision_notes` (text), `created_at`, `resolved_at`.

## LanceDB

Provisioned as part of this foundation (the `.zorp/lancedb/` store, the
connection and schema-creation code), but with no producers or consumers
yet. What actually goes into it (literature embeddings for validate,
figures and plots for co-write, venue-scope embeddings for find a venue)
is each capability's own concern, specced when that capability is
specced. This spec only guarantees the store exists and is reachable
from `zorp-track`, keyed by `track_id` so later capabilities can filter
to one track's content.

## Checkpoint mechanism

A `Checkpoint` type in `zorp-track`, shaped like `zorp-agent`'s existing
`Approver` trait and `ApprovalMode` enum (`approval.rs`), but at track
granularity rather than per-tool-call: `Interactive` (default, blocks
synchronously in the terminal and prompts), `AutoApprove` (for unattended
runs, explicit opt-in). No `NonInteractive` variant at this granularity;
unlike a tool call, a research checkpoint has no safe default to fall
back to when nobody's there to answer, so a non-interactive terminal
without `AutoApprove` set is an error, not a silent skip.

Each checkpoint records what was shown to the human (`prompt_shown`) and
their decision (`decision_notes`) in the `checkpoints` table, so a
track's history shows not just what was tried but what a human was asked
and how they answered.

## Integrity check on load

Whenever `zorp-track` opens a track, it verifies that every
`preregistrations` row has a corresponding `prereg.md` on disk, and every
`prereg.md` under `tracks/<id>/` has a corresponding row. A mismatch
(missing file, missing row, or file content that doesn't hash-match what
was recorded at commit time) is a hard error, not a warning, mirroring
Catalyst's `verify_prereg_order`. This is what makes the tamper-evidence
guarantee real rather than decorative.

The hash is a SHA-256 of the `prereg.md` file's raw bytes at commit time,
stored in `preregistrations.file_hash` (added to the schema above),
re-computed and compared against the current file on every load. Hashing
file bytes alone is sufficient: the file is the source of truth, and the
DB row's snapshot fields are themselves written from the file at commit
time, so a hash mismatch on the file is exactly the signal that matters.

## Error handling

- `.zorp/` doesn't exist yet: created on first use, same as `catalyst.db`
  auto-initializes on first run.
- `zorp.duckdb` or `lancedb/` missing or corrupted but `tracks/*/prereg.md`
  files exist: rebuild the index from the files rather than failing.
  (Building this rebuild path is in scope for this spec; it's the reason
  the files are the source of truth in the first place.)
- Prereg integrity mismatch (see above): hard error, refuse to proceed,
  clear message naming the specific track and what didn't match.
- Checkpoint reached with no `AutoApprove` set and no interactive
  terminal available: hard error, not a silent default in either
  direction.

## Testing

Unit tests in `zorp-track`, following `zorp-agent`'s existing test
conventions (`tests/` directory, `cargo test --workspace`):

- Schema creation and migration on a fresh `.zorp/` directory.
- Track CRUD (create, list, load, status transitions).
- Pre-registration: writing the file and the row together, and the
  integrity check catching each of the mismatch cases above.
- Checkpoint state transitions (`pending` to `approved` / `rejected`),
  and the `AutoApprove` / no-terminal error case.
- Index rebuild from `prereg.md` files alone, with `zorp.duckdb` deleted.

## Out of scope

- validate, experiment, co-write, find a venue themselves. Each gets its
  own spec once this foundation exists.
- What goes into LanceDB and how it's queried; only that the store is
  provisioned.
- The exact shape of `zorp-agent`'s new subcommands (`zorp-agent validate`,
  etc.); this spec is about the storage and checkpoint layer underneath
  them, not the CLI surface.
- Hypermemory integration (already decided as deferred, see
  `docs/DECISIONS.md`).
