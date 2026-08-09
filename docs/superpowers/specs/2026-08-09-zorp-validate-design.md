# validate: is this question worth investigating

**Date:** 2026-08-09
**Status:** approved

## Purpose

The first of zorp's four capabilities, and the first thing built on top
of the `zorp-track` foundation. Given a question, `validate` searches for
existing answers, scores whether the question is already settled
(redundancy) and whether it can actually be investigated (feasibility),
and hands a checkpoint to a human before anything more expensive
happens. See `docs/superpowers/specs/2026-08-09-zorp-scope-and-positioning.md`
for why this applies to any evidence-based question, not just academic
ideas, and `docs/superpowers/specs/2026-08-09-zorp-architecture-design.md`
for how it fits the four-capability shape.

## Where this lives

A new module in `zorp-agent`, `zorp-agent/src/validate.rs`, exposed as a
new subcommand, `zorp-agent validate "<question>"`, behind the existing
`research` feature (so it depends on `zorp-track` the same way the rest
of the research-loop code will). It uses:

- `zorp_track::Project` to open or create a track for the question.
- `zorp-agent`'s own existing `Agent`, not a bespoke MCP orchestration
  layer. `zorp-agent` already wires MCP tools into the normal
  tool-calling loop (`attach_mcp_tools` in `main.rs`, used by both `chat`
  and the oneshot task path): `Agent::new(...)`,
  `.register_builtins_filtered(...)`, `attach_mcp_tools(agent, ...)`,
  then `.run(task) -> Outcome`. `validate` builds a dedicated `Agent`
  the same way and lets the model decide which available tools to call,
  MCP-provided search tools included, exactly like any other task. No
  new MCP discovery or heuristic-matching code is needed; that would
  have duplicated infrastructure that already exists and already works.
- `zorp` core's existing `join_url` and a raw HTTP primitive for one new
  call this capability needs that doesn't exist yet: embeddings (below).

This is a real design correction from the first draft of this spec,
found while grounding the plan against the actual `zorp-agent` codebase
rather than assumed: an earlier version of this document had `validate`
reimplementing MCP tool discovery and calling from scratch, which would
have duplicated `attach_mcp_tools` and the whole `Agent` tool loop.

## Search

`validate` runs a dedicated `Agent` with a system prompt that states the
research and citation discipline (find sources, cite what's found, no
claim without a citation), and a task prompt built from the question,
asking the model to research it using whatever tools are available and
report back redundancy and feasibility findings as a fenced JSON block
at the end of its answer (parsed with the existing `extract_fenced_block`
helper plus `serde_json`, not a new parsing mechanism).

Before running, `validate` checks `agent.tool_names()` (already a public
method) for anything that looks search-capable (an MCP-prefixed tool
name, `mcp__*`, is sufficient signal; `zorp-agent`'s built-in tools are
all local file/shell/git/notes tools with no external search among
them). If nothing search-capable is available, `validate` fails with a
clear error naming the gap (configure a search-capable MCP server, e.g.
via `--mcp` or `.zorp/mcp.toml`) rather than silently running with no
way to find anything.

The agent's own `max_steps` (already configurable via `--max-steps` or
`ZORP_MAX_STEPS`, default 20) is the natural soft cap on how much
searching happens; `validate` doesn't need a second, separate budget
mechanism on top of one that already exists.

## Embeddings

A new environment variable, `ZORP_EMBEDDING_MODEL`, read alongside the
existing `ZORP_BASE_URL` and `ZORP_API_KEY` (`zorp::env_config()` already
reads these two; `validate` reads the new one itself, the same pattern
`env_config` already uses). Embeddings are requested via
`zorp::join_url(base, "embeddings")` and `zorp::zorp_raw(url, headers, body)`,
body shaped `{"model": ZORP_EMBEDDING_MODEL, "input": [...text...]}`,
response read from `response["data"][n]["embedding"]`. This mirrors how
chat completions already work in `src/main.rs`; no new primitive is
needed in `zorp` core, `validate` just calls the existing ones with a
different path and body shape.

Every source cited in the parsed verdict (its text or snippet, and its
URL or citation) is embedded and written into LanceDB. The `library`
table the foundation's `Library::open` provisions today has no vector
column (it was deliberately left as a placeholder with no producers
yet); `validate` is the first producer, and needs `Library` to grow a
new method, `insert_source(track_id, kind, text, embedding: &[f32])`,
that lazily creates a `sources` table on first call (schema inferred
from the embedding's own length, a `FixedSizeList<Float32>` column
alongside `track_id`/`kind`/`text`, all `Utf8`) and appends to it on
later calls. `kind` is `"validate-source"` for these rows, a plain
string, not a fixed enum, so later capabilities can add their own kinds
without a schema change, same reasoning as `checkpoints.kind`.

## Scoring

Two dimensions, not Catalyst's four, since "novelty" doesn't fit a
non-academic question well and "prior-art distance" and "mechanism
novelty" are really two views of the same thing once the domain isn't
fixed:

- **Redundancy**: has this question already been answered with enough
  confidence by what was found (a settled best practice, a prior
  analysis, an existing benchmark)? Scored with a required citation from
  the retrieved sources; a redundancy claim with no citation scores 0,
  same discipline as the pre-registration integrity story, a claim
  without evidence doesn't count.
- **Feasibility**: can this question actually be investigated further,
  given what sources and tools are available? Also requires a citation
  or concrete reasoning tied to what was found, not a bare assertion.

Both scores, their citations, and a short verdict come from the same
`Agent::run(task)` call that does the searching, not a second LLM call.
The task prompt asks for a fenced JSON block as the final answer, after
whatever tool calls the model makes; `validate` parses that block with
`extract_fenced_block` (already used elsewhere in `zorp-agent`) and
`serde_json`, into a `ValidationResult { redundancy_score: f64,
redundancy_citations: Vec<Citation>, feasibility_score: f64,
feasibility_citations: Vec<Citation>, verdict: String }`, where
`Citation { text: String, source: String }` is what gets embedded into
`sources` per citation. A response with no valid JSON block, or with a
score present but its citation list empty, is treated as a scoring
failure (see Error handling), the same "no citation, no claim"
discipline enforced at parse time, not just prompted for.

## Storage

A new DuckDB table, `validations`, added to `zorp-track`'s schema:

- `id` (text, primary key)
- `track_id` (text)
- `redundancy_score` (double)
- `redundancy_citations` (text, the cited source(s), free text is
  sufficient for a first version, not a structured join to a sources
  table)
- `feasibility_score` (double)
- `feasibility_citations` (text)
- `verdict` (text, the short human-readable summary)
- `created_at` (bigint)

This is `zorp-track`'s schema to own, not `zorp-agent`'s; `validate`
calls into a new `Store` method (e.g. `Store::record_validation`) the
same way `experiment.rs`'s methods already work, rather than issuing raw
SQL from `zorp-agent`.

## Checkpoint

After scoring, `validate` calls `Store::record_checkpoint(track_id,
"validate", &mode, prompt)`, where `prompt` includes both scores, their
citations, and the verdict. Interactive by default, `AutoApprove` for
unattended runs, same as the foundation already provides. Approved means
the track stays active and ready for `investigate`; rejected means
`validate` sets the track's status to `Killed` via the existing
`Store::set_track_status`.

## Error handling

- No search-capable tool available on the built agent (`tool_names()`
  has nothing `mcp__`-prefixed): clear error naming the gap (configure a
  search-capable MCP server), not a silent run with no way to find
  anything.
- `ZORP_EMBEDDING_MODEL` not set: clear error, same posture as a missing
  `ZORP_API_KEY` today.
- The agent's `Outcome` is anything other than `Complete(text)`
  (`StepLimit`, `VerificationFailed`, `Cancelled`, `RepeatedAction`,
  `Blocked`, `Error`): surfaced as a `validate` failure naming which
  outcome occurred, not silently treated as "no verdict."
- `Complete(text)` has no valid fenced JSON block, or the JSON parses
  but a score's citation list is empty: a scoring failure, distinct from
  a tool/agent failure, since the agent ran fine but didn't produce a
  usable verdict. Worth a distinct error variant so a caller (and a
  human reading the error) can tell "the agent couldn't research this"
  apart from "the agent researched it but wouldn't commit to a citable
  verdict."

## Testing

- `Store::record_validation` and the new `validations` table: unit
  tests in `zorp-track`, following its existing conventions (real
  DuckDB, tempdir), same shape as `record_metric`'s tests.
- `Library::insert_source`: unit tests in `zorp-track`, real LanceDB in
  a tempdir, covering lazy table creation on first insert, a second
  insert appending rather than recreating, and row count after both.
- The embeddings call shape: a unit test against a stubbed HTTP
  response (matching how `zorp` core's existing tests construct
  responses by hand, e.g. `extract_content_ok`), not a live API call.
- The JSON-block parsing and citation-required validation: unit tests
  in `zorp-agent` with fabricated `Outcome::Complete` strings, covering
  a well-formed block, a missing block, and a block with an empty
  citation list for one score.
- End-to-end: `zorp-mcp` already has the pattern for this
  (`zorp-mcp/tests/integration.rs`, a `#[ignore]`d test that spins up a
  real MCP server over stdio via `npx`, with a `has_npx()` guard that
  skips cleanly when the tool isn't available). `validate`'s end-to-end
  test follows the same shape, but against a small stub MCP stdio server
  written as a test fixture (canned search results, no real network
  call, no external package dependency), so the deterministic path runs
  in every CI environment. A second, genuinely `#[ignore]`d test against
  a real, npx-installable search MCP server and a real embedding
  endpoint is a reasonable addition for a manual smoke check, matching
  `zorp-mcp`'s existing convention exactly, but the stub-server test is
  what actually runs by default.

## Out of scope

- investigate, co-write, deliver themselves: each gets its own spec.
- A structured sources table (citations as free text for now, not
  joined rows); revisit if `co-write`'s claim-check needs more than
  that.
- Multiple embedding providers or a fallback when
  `ZORP_EMBEDDING_MODEL`'s provider doesn't support embeddings; the user
  is expected to configure a provider that does, same as they're
  expected to configure a chat-capable one today.
