# deliver: match a finished draft to real venues

**Date:** 2026-08-09
**Status:** approved

## Purpose

The fourth and last of zorp's four capabilities. Given a track with a
draft already written by `co-write`, `deliver` finds real academic
venues, conferences and journals, that fit the draft's scope and
contribution type, using a live venue database rather than a shipped,
staling catalog, and hands a ranked shortlist to a human for review. See
`docs/superpowers/specs/2026-08-09-zorp-co-write-design.md` for the
sibling capability this one consumes the output of, and the "eight
decisions" entry in `docs/DECISIONS.md` (2026-08-09) for the original
call to use a live venue API rather than a shipped dataset.

This is a deliberately narrower scope than the broader "get the
finished artifact into the right format for its audience" language used
elsewhere: for a first version, `deliver` only handles the academic-paper
case, matching against real conferences/journals. A non-academic
artifact (a decision memo, a competitive teardown) has no equivalent of
a "venue" in the same concrete sense, and inventing a generic
reformatting mechanism for arbitrary audiences is a different, much
larger problem than this capability solves. If that need becomes
concrete, it gets its own design, not a bolt-on to this one.

## Where this lives

A new module in `zorp-agent`, `zorp-agent/src/deliver/mod.rs` (plus
`error.rs`; no `result.rs`, same reasoning as `co-write`, the agent's
final answer is written directly as `venues.md`'s content, not parsed as
a scored JSON verdict), exposed as a new subcommand, `zorp-agent deliver
"<question>"`, behind the existing `research` feature. It reuses:

- `zorp_track::Project` and the existing `Agent`/`attach_mcp_tools`
  wiring, built the same way the other three capabilities build their
  agents.
- The huiban MCP server (already available in this environment,
  purpose-built for conference/journal search and detail lookups) as the
  live venue database. `deliver` does not implement its own venue
  catalog or call any HTTP API directly; it lets the agent call huiban's
  tools the same way `validate` lets the agent call whatever
  search-capable MCP tools are configured.

## Gates

Three checks run before the agent does anything, in this order:

1. **Track not killed.** Same posture as `investigate`/`co-write`:
   `Store::get_track` then a status check, refusing with
   `DeliverError::TrackKilled` before touching anything else.
2. **A draft exists.** `deliver` reads
   `project.track_dir(track_id).join("draft.md")`; if it doesn't exist,
   `DeliverError::NoDraft`, naming the missing path and that `co-write`
   needs to run first. There is nothing to deliver without a draft, the
   same reasoning `investigate`'s `NoMetrics` gate uses for "nothing to
   draft from."
3. **huiban is configured.** `deliver` checks `agent.tool_names()` for
   anything `mcp__huiban__`-prefixed, mirroring `validate`'s
   `has_search_tool` check exactly but naming the specific server this
   capability actually needs (not any generic search tool): a general
   web search would produce much weaker venue matches than a database
   purpose-built for this, so `deliver` doesn't fall back to whatever's
   available, it requires the right tool or refuses clearly with
   `DeliverError::NoVenueTool`, naming huiban as the missing
   configuration.

## Producing the shortlist

The task prompt hands the agent the draft's full content (read from
`draft.md`) and the track's hypothesis, and instructs it to determine
the draft's scope and contribution type, then use the available huiban
tools to search for and rank candidate conferences/journals that fit,
including each candidate's deadline and ranking (CCF/CORE) where huiban
provides one. Like `co-write`, there is no fenced-JSON-block contract
here: the agent's `Outcome::Complete(text)` is taken directly as the
shortlist's content and written to
`project.track_dir(track_id).join("venues.md")`.

## Checkpoint

After writing `venues.md`, `deliver` calls
`Store::record_checkpoint(track_id, "deliver", checkpoint_mode, prompt)`,
prompt showing the file path and how many candidates the shortlist
names (a simple line count of `## ` or `- ` headings is enough, no
structured parsing needed). Same as `co-write`, **rejecting this
checkpoint does not kill the track**: a shortlist not being good enough
yet isn't evidence anything upstream failed, the human can call `deliver`
again once they've refined `draft.md`, or research venues manually from
here. `deliver::run` returns `Result<bool, DeliverError>`, the same
approved/rejected shape as its siblings, no `set_track_status` call on
either branch.

## Error handling

- Track status is `Killed`: `DeliverError::TrackKilled`.
- No `draft.md` for this track: `DeliverError::NoDraft`.
- No `mcp__huiban__`-prefixed tool available: `DeliverError::NoVenueTool`.
- The agent's `Outcome` isn't `Complete`: `DeliverError::AgentOutcome`,
  the same six-variant handling every other capability already uses.

## Testing

- The three gates: unit tests in `zorp-agent` against a
  `Project`/`Store` in a tempdir, no agent involved, checked before the
  agent is built (killed track, missing `draft.md`, and a built agent
  with no huiban tool attached).
- End-to-end: an integration test following `co_write_integration.rs`'s
  shape. Since the real huiban MCP server isn't something a
  deterministic test should depend on, the test uses a small stub MCP
  stdio server (same pattern `validate`'s `stub_search_mcp_server` test
  fixture already establishes) whose tool names are prefixed
  `mcp__huiban__` so the gate check passes, returning canned search
  results; a stub `Model` that (like validate's stateful stub) calls the
  stub tool on its first turn then returns a final shortlist as its
  answer. Covers: the full round trip (a track with `draft.md` present,
  huiban stub attached, `deliver::run` called, `venues.md` written and
  checkpoint approved); a rejected checkpoint leaving the track's status
  unchanged (not `Killed`); each of the three gates independently.

## Out of scope

- Any non-academic delivery mechanism (see "Purpose" above).
- Calling huiban's credentialed detail endpoints (`get_conference`,
  `get_journal`, which need an API key) automatically; `deliver` only
  requires that some `mcp__huiban__`-prefixed tool exists, it doesn't
  mandate which ones the user has credentials for. The agent uses
  whichever huiban tools are actually available and configured, the
  no-credential search tools are enough to produce a useful shortlist on
  their own.
- A mtime-based hand-edit warning like `co-write`'s for `venues.md`.
  Unlike a co-authored draft, a shortlist is something a human is
  expected to just regenerate by calling `deliver` again rather than
  hand-edit in place, so the extra heuristic isn't worth the complexity
  here.
- Submitting to or otherwise interacting with any venue on the human's
  behalf; `deliver` only produces a shortlist for a human to act on.
