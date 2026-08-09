# investigate: run one staged, pre-registered attempt

**Date:** 2026-08-09
**Status:** approved

## Purpose

The second of zorp's four capabilities, built on top of `zorp-track`'s
existing experiment/metric/pre-registration primitives. Given a question
(track) that's ready to be investigated, `investigate` runs a single,
staged attempt: it locks in a pre-registered metric and kill threshold
the first time it's called for a track, runs the attempt with zorp-agent's
normal tool-calling loop, records a typed metric to the run record, and
hands a checkpoint to a human to decide whether the track stays alive.
See `docs/superpowers/specs/2026-08-09-zorp-scope-and-positioning.md` for
why this applies to any evidence-based question, and
`docs/superpowers/specs/2026-08-09-zorp-validate-design.md` for the sibling
capability this one is built to match in shape.

## Where this lives

A new module in `zorp-agent`, `zorp-agent/src/investigate/mod.rs` (plus
`result.rs` and `error.rs`, mirroring `validate`'s file layout), exposed
as a new subcommand, `zorp-agent investigate "<question>" [--metric-name
<name>] [--kill-threshold <n>]`, behind the existing `research` feature.
It reuses:

- `zorp_track::Project` to open the track's store and library, same as
  `validate`.
- `zorp-agent`'s existing `Agent` and `attach_mcp_tools`, built the same
  way `validate` builds its dedicated agent. Unlike `validate`,
  `investigate` does not require a search-capable tool to be present:
  the task the agent is attempting may be a local coding/analysis task
  with no external search involved at all, so there's no equivalent of
  validate's `has_search_tool` gate.
- `zorp_track`'s already-built experiment foundation directly:
  `write_prereg`, `Store::create_experiment`, `Store::set_experiment_status`,
  `Store::record_metric`. No new `zorp-track` schema or storage code is
  needed; this capability is squarely what that foundation was built for.

## One attempt per invocation

Each call to `investigate` runs exactly one attempt: build the agent, run
it, record one metric, checkpoint, return. It does not loop internally
through multiple attempts before returning control. A human (or a script)
calls `investigate` again for another attempt on the same track. This
keeps every attempt visible at a checkpoint, rather than burning an
unbounded budget inside a single invocation before a human sees anything.

## Pre-registration

The first time `investigate` runs for a track (no `preregistrations` row
exists yet for it), it requires `--metric-name` and `--kill-threshold` on
the command line. These are not agent-proposed: the entire point of
pre-registration is a human committing the metric and threshold before
any evidence exists, so having the agent invent its own threshold would
defeat the purpose. `investigate` calls `zorp_track::write_prereg(store,
track_dir, track_id, hypothesis, metric_name, kill_threshold)`, where
`hypothesis` is the track's existing hypothesis (the question `validate`
or track creation already recorded), then checkpoints the newly written
prereg (`Store::record_checkpoint(track_id, "investigate-prereg", mode,
prompt)`, prompt showing the hypothesis, metric name, and threshold)
before running the first attempt. A rejected prereg checkpoint sets the
track `Killed` via `Store::set_track_status` and `investigate` returns
without running an attempt.

On a later call for a track that already has a prereg, `--metric-name`
and `--kill-threshold` are optional. If the caller passes them anyway,
they must match the recorded prereg's `metric_name` and `kill_threshold`
exactly; a mismatch is a clear error (`InvestigateError::PreregMismatch`),
the same discipline `validate`'s track-retry fix added for a question
mismatch, not a silent overwrite or a silent ignore.

## Running an attempt

Once a prereg exists (just written, or already on file) and its
checkpoint (if just written) was approved:

1. `Store::create_experiment(track_id, prereg_id)` — status `Planned`.
2. `Store::set_experiment_status(experiment_id, Running)`.
3. Build a dedicated `Agent` the same way `validate` does
   (`Agent::new`, `.register_builtins_filtered(...)`,
   `attach_mcp_tools(agent, ...)`), with a task prompt built from the
   track's hypothesis and the prereg's metric name, asking the agent to
   work the problem using whatever tools are available and end its
   answer with a single fenced JSON block:
   `{"metric_value": <number>, "summary": "<one sentence>"}`.
4. Run the agent (`agent.run(&task) -> Outcome`). Any `Outcome` other
   than `Complete(text)` is surfaced as a distinct
   `InvestigateError::AgentOutcome(String)` (`StepLimit`,
   `VerificationFailed`, `Cancelled`, `RepeatedAction`, `Blocked`,
   `Error`, matching every variant `validate` already handles) and marks
   the experiment `Failed` via `Store::set_experiment_status` before
   returning the error.
5. Parse `Complete(text)` with `investigate::result::parse_attempt_result`,
   a new function built the same way `validate::result::
   parse_validation_result` is: scan every fenced block in the answer
   with the same `all_fenced_blocks` approach (duplicated into
   `investigate::result`, not shared, since `validate`'s copy is private
   to its own module and the two result shapes differ), try each until
   one deserializes into `{metric_value: f64, summary: String}`. No valid
   block, or a block missing `metric_value`, is a distinct
   `InvestigateError::Scoring(ParseError)`, and also marks the experiment
   `Failed` before returning.
6. `Store::record_metric(experiment_id, &prereg.metric_name,
   MetricValue::Number(metric_value))`.
7. `Store::set_experiment_status(experiment_id, Completed)`.

## Checkpoint, not auto-kill

`investigate` does not compare `metric_value` against `kill_threshold`
itself and does not know which direction (above or below) is favorable;
nothing in the pre-registration schema records a direction, and adding
one is out of scope (see below). Instead, after recording the metric,
`investigate` calls `Store::record_checkpoint(track_id, "investigate",
checkpoint_mode, prompt)` with a prompt showing the hypothesis, metric
name, recorded value, the pre-registered threshold, and the attempt's
summary, and lets a human decide whether the track stays alive. Rejected
sets the track `Killed` via `Store::set_track_status`, the same asymmetry
`validate` already has between its own record and the track's status: the
experiment row itself stays `Completed` (the attempt ran fine and
produced a real number), only the track dies.

`investigate` refuses to run at all if the track's current status is
already `Killed` (checked via `Store::get_track` before anything else),
returning `InvestigateError::TrackKilled` rather than silently attempting
a dead track.

## Error handling

- Track status is `Killed`: `InvestigateError::TrackKilled`, checked
  before creating an experiment or touching the agent.
- No prereg exists yet and `--metric-name`/`--kill-threshold` weren't
  both given: `InvestigateError::PreregRequired`, naming which flag(s)
  are missing.
- A prereg exists and the caller's `--metric-name`/`--kill-threshold`
  don't match it: `InvestigateError::PreregMismatch`, naming the
  recorded values and what was passed.
- The prereg checkpoint is rejected: track killed, `investigate` returns
  `Ok(false)` (mirroring `validate::run`'s `Result<bool, ..>` shape) with
  no attempt run, not an error — a human rejecting the plan is a normal
  outcome, not a failure.
- The agent's `Outcome` isn't `Complete`: `InvestigateError::AgentOutcome`,
  experiment marked `Failed`.
- No valid fenced JSON block, or the block is missing `metric_value`:
  `InvestigateError::Scoring`, experiment marked `Failed`.

## Testing

- Prereg-required and prereg-mismatch paths: unit tests in
  `zorp-agent` against a `Project`/`Store` in a tempdir, no agent
  involved (these checks run before the agent is built).
- `parse_attempt_result`: unit tests with fabricated strings, covering a
  well-formed block, a missing block, a block missing `metric_value`, and
  a decoy leading fenced block before the real one (matching validate's
  equivalent test).
- End-to-end: an integration test following `validate_integration.rs`'s
  shape exactly — a stub `Model` that returns a well-formed attempt JSON
  block, no MCP server needed (investigate doesn't require one), covering
  the full round trip: prereg write and checkpoint approval (auto-approve
  mode), attempt run, metric recorded, experiment status `Completed`,
  checkpoint approval or rejection both exercised.
- Track-killed and agent-outcome-failure paths: unit or integration
  tests with a stub agent/model forcing each `Outcome` variant, same
  pattern as `validate`'s tests.

## Out of scope

- Multi-attempt loops within one invocation (see "One attempt per
  invocation" above).
- Automatic threshold comparison or a stored kill direction; the human
  checkpoint is the decision point, not code.
- Parallel experiment workers (mentioned in the architecture spec as a
  future "zorp-agent spawns more copies of itself" mechanism, not part
  of this capability).
- Requiring a prior `validate` approval before `investigate` can run;
  per the existing "four standalone capabilities" decision, `investigate`
  only requires the track to exist and not be `Killed`.
