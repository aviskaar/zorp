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
- `zorp_mcp::McpRegistry` (already built) to call whatever search-capable
  MCP tools the user has configured. `validate` does not know about a
  specific search provider; it discovers tools via `McpRegistry::discover()`
  and calls them via `McpRegistry::call_tool(prefixed_name, args)`.
- `zorp` core's existing `zorp_raw`, `join_url`, and `env_config`
  primitives for both the scoring LLM call and a new embeddings call
  (below). No changes to `zorp` core are needed; `validate` composes
  these primitives itself, the same way `src/main.rs` already does for
  chat completions.

## Search

`validate` calls `McpRegistry::discover()` to find available tools, and
treats any tool whose name or description matches search-shaped
keywords (search, web, lookup, query) as a candidate, calling each with
the question (or an LLM-refined query derived from it) as input. This is
a heuristic, not a strict contract: MCP tool naming isn't standardized
enough to assume a fixed shape. If no search-capable tool is discovered,
`validate` fails with a clear error naming what's missing (configure a
search-capable MCP server), rather than silently proceeding with no
evidence.

The number of search calls is soft-capped (a small default, several
queries), not hard-enforced, consistent with the no-hard-experiment-budget
decision already made: `validate` is meant to be a cheap filter, not an
exhaustive literature review, but a fixed hard ceiling would be wrong for
some domains and right for others.

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

Every retrieved source (the search result's title, snippet or fetched
content, and its URL or citation) is embedded and written into LanceDB's
`library` table (provisioned, empty, by the `zorp-track` foundation),
keyed by `track_id`, with a `kind` field (`validate-source`) so later
capabilities can distinguish what each row is for without a schema
change.

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

Both scores, their citations, and a short verdict come from a single LLM
call (`zorp::zorp_to` or an equivalent direct call, since `validate`
needs a specific model and doesn't want to re-read env config
mid-function; using the lower-level `zorp_raw` primitive directly, same
as `zorp_to`'s own implementation does, is the likely shape) given the
question and the retrieved sources as context.

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

- No search-capable MCP tool discovered: clear error naming the gap,
  not a silent empty-evidence proceed.
- `ZORP_EMBEDDING_MODEL` not set: clear error, same posture as a missing
  `ZORP_API_KEY` today.
- A search call fails: log and continue with whatever succeeded, rather
  than failing the whole run over one bad source, but if zero sources are
  retrieved after all attempts, that's a hard error (nothing to score
  against).

## Testing

- `Store::record_validation` and the new `validations` table: unit
  tests in `zorp-track`, following its existing conventions (real
  DuckDB, tempdir), same shape as `record_metric`'s tests.
- The embeddings call shape: a unit test against a stubbed HTTP
  response (matching how `zorp` core's existing tests construct
  responses by hand, e.g. `extract_content_ok`), not a live API call.
- The MCP tool-discovery heuristic (which tools "look like" search
  tools): a unit test with a fabricated `Vec<McpTool>` of mixed
  search-shaped and non-search-shaped names, asserting the right subset
  is selected.
- End-to-end: `zorp-mcp` already has the pattern for this
  (`zorp-mcp/tests/integration.rs`, a `#[ignore]`d test that spins up a
  real MCP server over stdio via `npx`, with a `has_npx()` guard that
  skips cleanly when the tool isn't available). `validate`'s end-to-end
  test follows the same shape, but against a small stub MCP stdio server
  written as a test fixture (canned search results, no real network
  call, no external package dependency), so the deterministic path runs
  in every CI environment. A second, genuinely `#[ignore]`d test against
  a real, npx-installable search MCP server is a reasonable addition for
  a manual smoke check, matching `zorp-mcp`'s existing convention
  exactly, but the stub-server test is what actually runs by default.

## Out of scope

- investigate, co-write, deliver themselves: each gets its own spec.
- A structured sources table (citations as free text for now, not
  joined rows); revisit if `co-write`'s claim-check needs more than
  that.
- Multiple embedding providers or a fallback when
  `ZORP_EMBEDDING_MODEL`'s provider doesn't support embeddings; the user
  is expected to configure a provider that does, same as they're
  expected to configure a chat-capable one today.
