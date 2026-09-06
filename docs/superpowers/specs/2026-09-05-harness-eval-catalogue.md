# Harness eval catalogue

**Status:** work list, not a design. Nothing here is built.

This is the catalogue of behaviours a deterministic, whole-run evaluation
suite has to cover before it can be called a gate on the zorp harness. It
is meant to be worked one case at a time. Somebody else is building the
runner; this says what the runner has to be able to say yes or no to.

Every case here is derived from something this repo already decided or
already broke. The decision log is the source, and the failure history in
it is the ranking. A case that could not name a regression it would catch
was left out, and the ones that were left out are named at the end so
nobody adds them back by accident.

---

## What "whole run" means here

Not a unit test of the agent loop. A real `zorp-agent` process, in a
temporary directory, talking to a local listener that pretends to be an
OpenAI-compatible provider, with its store and trace file in that same
directory, judged on what it left behind: the exit code, stdout, stderr,
the SQLite store, the trace file, the files on disk, and how many times
it reached the provider.

`zorp-agent/tests/reask_dropped_stream.rs` is already exactly this shape
and is the model to copy. It spawns `CARGO_BIN_EXE_zorp-agent`, points
`--base-url` at `sse_stub::scripted_server`, sets `ZORP_STATE_DB` into a
tempdir, scrubs the developer's own environment so a connection count
means one thing, and then queries the store with `rusqlite`. Nothing in
this catalogue needs a mechanism that file does not already demonstrate,
except the fixture gaps listed below.

### What the runner needs that does not exist yet

- **A provider script that can emit tool calls.** `sse_stub::Reply`
  models statuses, finished streams, cut-off streams, in-stream error
  events, resets and silence, but `Finished { events }` only produces
  content deltas. Most of this catalogue needs a scripted reply that says
  "call `run_command` with these arguments", then a second that answers.
- **Request capture.** Several cases assert on what was *sent*, not on
  what came back: prompt size across a long run, one system message on a
  reseeded turn, an elided argument. `tests/common/mock_capture` does
  this for the buffered path; the SSE stub does not keep bodies.
- **A fixture workspace.** A tempdir, optionally `git init`ed, with a
  known file tree, plus `ZORP_STATE_DB`, `ZORP_TRACE_FILE` and a scrubbed
  environment. One helper, used by every case, or the cases will each
  configure it slightly differently and stop being comparable.
- **A store reader.** Message rows, tool call arguments, file changes,
  and `sessions.status`. Raw `rusqlite` is fine and is what the existing
  test uses.

### What already exists, so nothing here duplicates it

- `zorp-agent/src/agent.rs` carries roughly 78 tests driving a `Scripted`
  in-process `Model` through the whole run loop: tool dispatch, approval,
  compaction, repeat and denial guards, verification, termination,
  cancellation, the denylist under auto-approve, `finish_reason=length`,
  and the copied compaction marker.
- `zorp-agent/src/streaming.rs` covers the SSE decoder, `ThinkGate`
  across chunk boundaries, tool-call fragment joining, and that a
  streamed turn equals the buffered turn it describes.
- `src/lib.rs` covers `retry_reason`, the two-sided retry bound, the
  backoff, `Retry-After` including one that will not fit the budget, and
  the error-object parser.
- `zorp-agent/tests/retry_rate_limit.rs` and `streaming_timeout.rs` prove
  the transport rules against a real listener, by counting connections.
  They call `stream_sse` directly, not the binary.
- `zorp-agent/tests/reask_dropped_stream.rs` proves the re-ask at whole
  run level already.
- `zorp-agent/tests/cli.rs` covers argument parsing, subcommand
  precedence, flavors and trust, `undo`, and `diff`.
- Canned models exist per subsystem: `StubModel` in `validate`,
  `investigate`, `deliver` and `co_write`, `ScriptedModel` in `critique`,
  `Canned` in `panel`. `zorp-web/tests/` runs roughly 150 tests over
  whole turns through the API against a mock model.

Where a case below overlaps one of those, it says what the whole-run
version proves that the unit version cannot. Where it does not add
anything, it is in the "already proved" list and is not a case.

---

## The ten that matter most, and why

Value here is the cost of the regression times how likely it is, judged
from what has actually gone wrong in this repo rather than from what
could in principle.

**1. `exit_code_and_stream_contract`.** Everything else in an eval suite
is read through this. A benchmark harness decides pass or fail from the
exit code and reads the answer off stdout, and the runs in `jobs/` scored
zero on every trial while the real information came out as six decision
log entries about harness behaviour. Two of those entries exist because a
dead run looked finished: `finish_reason=length` returned cut-off text as
an answer with exit 0, and a stream that ended with no `[DONE]` returned
`Ok`. `finish` in `zorp-agent/src/main.rs` exits 1 for every outcome that
is not `Complete`, writes the answer to stdout only on `Complete`, sends
every other outcome to stderr, and writes a status word to
`sessions.status`. Nothing pins any of that. It is the cheapest case in
the catalogue and the one every other case depends on.

**2. `cut_off_reply_is_an_error`.** The same failure, at the level where
it was measured. A trial spent nine of its thirteen minutes writing one
line of Python 2,300 times, the provider stopped it at 32,768 tokens with
`finish_reason=length`, and the run exited 0 with the fragment as its
answer. `agent.rs` has a unit test for the `Outcome`. What it cannot
prove is that the process then exits 1 and prints nothing to stdout,
which is the only part a harness above it can see. This is the exact
shape of a regression that would be invisible for a whole benchmark run
again.

**3. `in_stream_error_before_delta_recovers_the_run`.** Nine of nine
trials against one OpenRouter model died inside 1 to 11 model calls to a
502 delivered inside an HTTP 200 stream, and the fix is recent enough
that nothing has run long against it. `retry_rate_limit.rs` proves the
transport sends again and that the caller sees exactly one answer, at the
`stream_sse` level. It does not prove that a run survives one, ends with
the right answer, exits 0, and leaves one assistant row rather than two.
The failure mode if this regresses is a whole benchmark going to zero,
which is precisely what happened.

**4. `prompt_bytes_stop_growing_over_a_long_run`.** One 21-task run made
896 model calls, 461 of the prompts were over 64k tokens and the largest
was 206k, one task grew from 3k at step 1 to 122k at step 60 without
falling once, and a task failed when the server refused a 196k prompt.
The reason nobody caught it earlier is that nothing was measuring the
size of what gets sent across a run. Unit tests assert that compaction
elides the right things given a transcript; they cannot assert that a
long run's requests stop growing, because that is a property of the run.
This is the one case in the catalogue that measures a trend rather than
an event, and it is the one that would have caught the most expensive
regression on record.

**5. `command_argument_is_never_elided_and_a_marker_is_refused`.** The
amendment written the same day as the compaction entry, after trial 3 of
the second run: a 5 KB `python -c` command was elided, the model copied
the marker back as its next command, and the shell ran `[tool argument
elided: ...]` three times before the repeat detector stopped it. Two
rules came out of it and both are cheap to break by accident, because
"never elide the `command` key" is a special case inside a generic
elision pass. `agent.rs` tests the refusal against a `Scripted` model. A
whole run adds the part that actually failed, which is the shell being
handed a placeholder, and it can assert nothing on disk changed.

**6. `heredoc_with_an_apostrophe_is_not_denied`.** 18 of 21 tasks in one
Terminal-Bench run had `run_command` denied, and 15 of those were Python
heredocs with an apostrophe in a comment: the policy parsed the body as
shell words, saw an unclosed quote, and failed closed. `policy.rs` has
unit tests for the fix. What they cannot show is the thing that made it
expensive, which is that a run does not merely lose one call, it loses
the run: three denials in a row trip `DENIAL_STREAK_LIMIT` and end it as
`Blocked`. The whole-run case ties the policy decision to the run
outcome, and that link is what turned a parsing bug into 18 failures.

**7. `trace_event_types_are_pinned`.** There is no model-free test of the
trace format anywhere. `zorp-eval/tests/instrumentation_validation.rs` is
the only thing that checks it, and it is `#[ignore]`d and needs a release
build plus real model credentials. Renaming a `serde` rename in
`TraceEvent` would turn every contract in `zorp-eval` into
`Unevaluable`, which by that crate's own 2026-08-14 decision is an honest
non-result and therefore silent. The drift has already started:
`zorp-eval/src/contracts.rs` matches on an event type `observation` at
lines 139, 194 and 205, and `zorp-agent/src/agent.rs` emits no such
event. Pinning the emitted set costs one run and one sorted list.

**8. `denial_names_the_rule`.** From the same Terminal-Bench run as the
heredoc case, and the second half of the same decision. A denial that
does not say which rule fired leaves the model retrying the same shape
until the denial streak kills the run, so the message is not cosmetic,
it is what turns a denial into a recoverable step. The regression is
easy: a refactor that collapses `deny_reason`'s several messages into one
generic string would pass every existing policy test, because those
assert on `Decision::Deny(_)` matching and on specific reasons in
isolation, not on what reaches the model.

**9. `seed_sends_one_system_prompt_and_no_dangling_tool_call`.**
`plan_seed` is the single path both `resume` and every web turn go
through, and it has two properties that were bugs before they were rules:
the web server used to write one stored system message per turn, and a
transcript with a tool call and no result is one a provider is entitled
to reject. Both are properties of what gets sent on the second turn,
which nothing at unit level looks at end to end. A regression here does
not fail loudly, it makes long conversations start failing at some
provider-dependent depth.

**10. `write_outside_the_workspace_is_refused`.** The only case in the
top ten that is not on the failure record, and it is here because of what
it costs rather than how likely it is. `Context::resolve_existing` and
`resolve_for_create` canonicalize and then check `starts_with` against
the repo root, and `resolve_for_create` canonicalizes the parent rather
than the file, which is the subtle half. The 2026-09-05 workspace entry
exists because `zorp-web` was writing the agent's output into zorp's own
source tree, so files landing where nobody chose is a live theme here,
and a boundary that fails open fails quietly.

---

## The whole list, in order

Work down this. The sections below group the same cases by area for
reading, but this is the order they should be written in. Cost is how
expensive the case is to write, not to run.

| # | Case | Area | Cost |
|---|---|---|---|
| 1 | `exit_code_and_stream_contract` | I | cheap |
| 2 | `cut_off_reply_is_an_error` | I | cheap |
| 3 | `in_stream_error_before_delta_recovers_the_run` | A | cheap |
| 4 | `prompt_bytes_stop_growing_over_a_long_run` | C | expensive |
| 5 | `command_argument_is_never_elided_and_a_marker_is_refused` | C | cheap |
| 6 | `heredoc_with_an_apostrophe_is_not_denied` | D | cheap |
| 7 | `trace_event_types_are_pinned` | H | cheap |
| 8 | `denial_names_the_rule` | E | cheap |
| 9 | `seed_sends_one_system_prompt_and_no_dangling_tool_call` | G | cheap |
| 10 | `write_outside_the_workspace_is_refused` | F | cheap |
| 11 | `contracts_name_only_events_the_agent_emits` | H | cheap |
| 12 | `denial_streak_ends_the_run_as_blocked` | E | cheap |
| 13 | `status_retry_recovers_the_run` | A | cheap |
| 14 | `no_answer_is_ever_delivered_twice` | A | cheap |
| 15 | `stated_window_is_adopted_and_the_turn_finishes` | C | cheap |
| 16 | `compaction_never_shrinks_the_store` | C | cheap |
| 17 | `bare_heredoc_body_is_still_read_as_shell` | D | cheap |
| 18 | `read_timeout_ends_the_run_loudly` | A | cheap |
| 19 | `symlink_out_of_the_workspace_is_refused` | F | cheap |
| 20 | `tool_result_status_words_are_a_contract` | D | cheap |
| 21 | `every_feature_flag_compiles` | J | cheap |
| 22 | `research_suite_runs_when_the_loop_changes` | J | cheap |
| 23 | `undo_restores_what_diff_reported` | G | cheap |
| 24 | `reasoning_never_reaches_stdout` | B | cheap |
| 25 | `seed_drops_whole_exchanges_from_the_front` | G | cheap |
| 26 | `a_second_refusal_is_a_readable_error` | C | cheap |
| 27 | `streamed_tool_arguments_reach_the_tool_intact` | B | cheap |
| 28 | `everything_written_lands_under_dot_zorp` | F | cheap |
| 29 | `own_server_port_is_denied_through_a_shell_wrapper` | E | expensive |
| 30 | `reask_counts_against_the_step_limit` | B | cheap |
| 31 | `trace_carries_no_credential` | H | cheap |
| 32 | `background_process_does_not_outlive_the_run` | D | cheap, flaky |
| 33 | `no_ansi_when_piped` | I | cheap |
| 34 | `upstream_404_recovers_and_a_bare_404_does_not` | A | cheap |
| 35 | `denylist_beats_auto_approve` | E | cheap |
| 36 | `turn_tool_output_cap_withholds_and_the_run_continues` | D | cheap |

The tail of this list is not padding, but it is close to it. Everything
from 33 down duplicates a unit test and adds only the process boundary.
If the suite has to stop somewhere, stop after 32 and say so, rather than
writing four cases that can only fail in ways something else already
catches.

---

## A. Provider transport and retries

### `in_stream_error_before_delta_recovers_the_run`
**Proves.** A 502 delivered as an error object inside an HTTP 200 stream,
before any delta, costs the run one extra request and nothing else.
**Provider script.** `ErrorEvent { after: 0, event: <the captured
OpenRouter 502> }`, then a stream carrying a tool call, then a finished
answer.
**Fixture.** Tempdir with one readable file, scrubbed environment,
`ZORP_RETRY_ATTEMPTS` set explicitly on the child.
**Assertion.** Exit 0; stdout is the final answer; the store holds
exactly the messages of a two-step run with no orphan assistant row; the
listener counted one more connection than there were steps; stderr
carries one retry line naming the code.
**Defends.** `docs/DECISIONS.md` 2026-09-04 (error inside a 200 stream);
`zorp::Retrying`, `stream_sse`.
**Regression it catches.** Someone moving the in-body error check after
the first payload is handed up, or keying the retry off the status line
again. Both give back nine dead trials out of nine.
**Cost.** Cheap.
**Existing coverage.** `retry_rate_limit.rs` at the `stream_sse` level.
The whole run adds the outcome, the exit code and the store contents,
none of which the transport test can see.

### `status_retry_recovers_the_run`
**Proves.** A 429 on the status line is absorbed by the run, not by the
person reading it.
**Provider script.** `Status { code: 429, retry_after: None, body: <the
captured "Please retry shortly" body> }`, then a finished answer.
**Fixture.** As above, with `ZORP_RETRY_BUDGET_SECS` large enough not to
bind and `ZORP_RETRY_ATTEMPTS` at its default.
**Assertion.** Exit 0, one answer, connection count exactly two, one
stderr line naming the status and the wait.
**Defends.** `docs/DECISIONS.md` 2026-08-23 (a provider asking to be
asked again).
**Regression it catches.** The retry being lost from the streaming path
again, which is how 25 of the first 48 attempts of a calibration run were
discarded.
**Cost.** Cheap.
**Existing coverage.** Transport level only, same gap as above.

### `upstream_404_recovers_and_a_bare_404_does_not`
**Proves.** The two shapes of 404 are told apart by the body, in a real
run, on both the status line and inside a stream.
**Provider script.** Four variants: 404 with `metadata.provider_name` on
the status line then an answer; the same as a chunk-level `provider`
inside a 200; a 404 with no provider named; a provider-named 404 after a
delta.
**Fixture.** As above.
**Assertion.** The first two exit 0 with two connections. The third exits
1 with one connection. The fourth is a re-ask by the loop, not a re-send
by the transport, so the connection count rises but the answer never
repeats.
**Defends.** `docs/DECISIONS.md` 2026-09-04 (a 404 that names an upstream
provider); `zorp::retry_reason`.
**Regression it catches.** Reverting to "a 404 is never retried", which
killed three attempts in one day, two of them 26 good model calls in. Or
the opposite, retrying our own wrong model id forever.
**Cost.** Cheap.
**Existing coverage.** `retry_rate_limit.rs` covers all four at transport
level. This is the lowest-value case in section A for that reason; it
earns its place only because the distinction is body-dependent and a
refactor of the body parser touches both layers.

### `read_timeout_ends_the_run_loudly`
**Proves.** A provider that goes quiet ends the run with an error that
names the timeout and `ZORP_HTTP_TIMEOUT_SECS`, and is not sent again.
**Provider script.** `Quiet { after: 2 }`.
**Fixture.** `ZORP_HTTP_TIMEOUT_SECS` set to one or two seconds on the
child, both framings.
**Assertion.** Exit 1; stderr names the timeout and the variable; stdout
empty; connection count exactly one.
**Defends.** `docs/DECISIONS.md` 2026-08-23 (a cut off stream is an
error), 2026-08-22 (one HTTP agent).
**Regression it catches.** The silent version of the bug, where a chunked
body's read timeout arrives as "Error while decoding chunks" and a log
full of those says nothing. That misattribution cost nine hours and was
made twice.
**Cost.** Cheap, but it is a case that spends real seconds. Keep the
configured timeout small and assert on the message, not the elapsed time.
**Existing coverage.** `streaming_timeout.rs` at transport level. The
whole run adds the exit code and that nothing was recorded.

### `no_answer_is_ever_delivered_twice`
**Proves.** Across every retry and re-ask path, the text on stdout is one
answer, not the front of an abandoned one followed by a fresh one.
**Provider script.** A table: rate-limited then answer; error event
before a delta then answer; error event after a delta then answer; reset
after a delta then answer.
**Fixture.** Each scripted answer carries a distinct sentinel string.
**Assertion.** Stdout contains exactly one sentinel, and it is the last
one the provider sent.
**Defends.** `docs/DECISIONS.md` 2026-08-23, 2026-09-04, 2026-09-05, all
three of which turn on this one property.
**Regression it catches.** Any future retry rule that forgets the "not
once a payload has reached the caller" clause. The failure is a
transcript that reads like a model repeating itself, which looks like a
model problem.
**Cost.** Cheap.
**Existing coverage.** None at this level. `retry_rate_limit.rs` counts
payloads for one case; this generalises it over every path and over the
process's own stdout.

---

## B. Streaming and re-asks

### `reask_counts_against_the_step_limit`
**Proves.** A re-ask is a step, so a run near `max_steps` that gets a
dropped stream ends at the step limit rather than getting free retries.
**Provider script.** A drop after a delta, on a run configured with a
small `--max-steps`.
**Fixture.** Small step limit passed on the command line.
**Assertion.** Exit 1, `sessions.status` is `step_limit`, and the
listener saw no more connections than the step limit allows.
**Defends.** `docs/DECISIONS.md` 2026-09-04 (`REASKS_PER_STEP`);
`agent.rs` `run_loop`, the `step += 1; continue 'run` on the drop path.
**Regression it catches.** Moving the re-ask into the inner loop, where
it would not be counted, cancellable, or bounded by `max_steps`. A
provider dropping every stream would then spin.
**Cost.** Cheap.
**Existing coverage.** `reask_dropped_stream.rs` covers the re-ask and
the bound. It does not cover the interaction with `max_steps`.

### `reasoning_never_reaches_stdout`
**Proves.** A qwen-family model's `<think>` block, split across chunk
boundaries in the worst places, never appears in the answer the process
prints.
**Provider script.** A finished stream whose content deltas split
`<think>`, the body, and `</think>` mid-tag and mid-multibyte-character.
**Fixture.** Standard.
**Assertion.** Stdout contains the answer and none of the reasoning text.
**Defends.** `docs/DECISIONS.md` 2026-08-18 (answers stream, and the
streaming path filters reasoning); `streaming::ThinkGate`.
**Regression it catches.** A rewrite of the gate that passes its unit
tests on whole chunks but leaks on a boundary the tests do not use. The
leak is a chain of thought presented as an answer, which an eval above it
would score as a bad answer.
**Cost.** Cheap.
**Existing coverage.** `streaming.rs` covers `ThinkGate` including split
tags. The whole run adds the path from the gate to the process's stdout,
which passes through the accumulator and `render_assistant_text`.

### `streamed_tool_arguments_reach_the_tool_intact`
**Proves.** Tool call arguments assembled from fragments arrive at the
tool byte for byte, including quotes, newlines and non-ASCII.
**Provider script.** One tool call whose JSON arguments are split across
many deltas at awkward offsets, calling `write_file` with a body
containing an apostrophe, a newline and a multibyte character.
**Fixture.** Tempdir, `--yes`.
**Assertion.** The file on disk has exactly the intended bytes.
**Defends.** `docs/DECISIONS.md` 2026-08-18; `streaming.rs` fragment
joining, `parse_assistant_completion`.
**Regression it catches.** A fragment joiner that concatenates by
arrival order instead of by index, which would corrupt content silently
and look like the model writing bad files.
**Cost.** Cheap.
**Existing coverage.** `streaming.rs` covers joining and equality with
the buffered turn. This adds dispatch and the filesystem, which is the
half nobody looks at.

---

## C. The context window and compaction

### `prompt_bytes_stop_growing_over_a_long_run`
**Proves.** Over a scripted run of many steps, each returning a large
tool result, the size of the request body stops growing.
**Provider script.** N steps, each asking for `read_file` on a large
fixture file, then a final answer. N large enough to cross the 512 KiB
tool-result floor twice.
**Fixture.** A large file in the tempdir; the stub records the byte
length of every request body.
**Assertion.** The recorded lengths rise and then plateau; the maximum is
under a stated bound; the run exits 0. Run it twice, once with
`ZORP_CONTEXT_TOKENS` set and once unset, because the unknown-window path
is the one that failed.
**Defends.** `docs/DECISIONS.md` 2026-09-03 (compaction elides tool-call
arguments), 2026-08-19 (a turn is seeded from the store);
`context_window::compact_tool_results`, `TOOL_RESULT_HISTORY_BUDGET_BYTES`.
**Regression it catches.** Exactly the measured one: 461 prompts over 64k
in a single run because assistant tool-call arguments were not counted.
Any future growth channel the byte floor does not see would show here as
a line that never flattens.
**Cost.** Expensive to write, because it needs request capture and a run
of real length. It is worth it: this is the only case that measures a
trend, and the trend is where the money went.
**Existing coverage.** None. `context_window.rs` and `agent.rs` test
compaction given a transcript, which is a different question.

### `command_argument_is_never_elided_and_a_marker_is_refused`
**Proves.** A `command` argument survives compaction, and a marker copied
back as an argument is refused before it reaches the shell.
**Provider script.** Enough steps with large `run_command` arguments to
force argument elision, then a step whose `command` is the marker text.
**Fixture.** Standard, `--yes`.
**Assertion.** The captured requests still carry every `command` string
whole; the marker call comes back as a tool error naming the placeholder;
no shell process ran for it, checked by the command being one that would
leave a file behind and the file not existing; the run continues.
**Defends.** `docs/DECISIONS.md` 2026-09-03, the same-day amendment;
`agent.rs` `copied_marker_argument`,
`context_window::ELIDED_ARGUMENT_MARKER_PREFIX`.
**Regression it catches.** A generic elision pass that stops special
casing `command`. The observed failure was the shell running
`[tool argument elided: ...]` three times and exiting 127 until the
repeat guard stopped the run.
**Cost.** Cheap once request capture exists.
**Existing coverage.** `agent.rs` tests the refusal against a `Scripted`
model and the notice wording. Neither reaches the shell.

### `stated_window_is_adopted_and_the_turn_finishes`
**Proves.** A provider that refuses a request and states its window in
the refusal gets the same step once more, compacted, and the run
completes.
**Provider script.** `Status { code: 400, body: <Ollama's nested
`exceed_context_size_error` with `n_ctx` and `n_prompt_tokens`> }`, then
a finished answer.
**Fixture.** `ZORP_CONTEXT_TOKENS` unset, a transcript large enough that
compaction has something to elide.
**Assertion.** Exit 0; connection count exactly two; the second request
is smaller than the first; stderr carries the raw refusal and a notice
saying what compaction took.
**Defends.** `docs/DECISIONS.md` 2026-09-03 (a provider that states its
context window); `context_window::stated_window`,
`agent.rs` `adopt_stated_window`.
**Regression it catches.** The measured failure, where a routine two-file
turn against a default 4096-token Ollama died showing raw JSON under
"Something went wrong".
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has three tests for this against a
`Scripted` model. The whole run adds the real body shape off the wire and
that the second request is genuinely smaller.

### `a_second_refusal_is_a_readable_error`
**Proves.** Adoption is one retry and never a loop, and the error a
person gets names both numbers and the variable.
**Provider script.** Two consecutive 400s stating the same window.
**Fixture.** As above.
**Assertion.** Exit 1; exactly two connections; stderr names the tokens
needed, the window served, `ZORP_CONTEXT_TOKENS`, and where to raise
Ollama's own limit.
**Defends.** Same entry.
**Regression it catches.** A retry loop that sends the same bytes until
something else breaks, which is the failure the "once" in that decision
exists to prevent.
**Cost.** Cheap.
**Existing coverage.** `agent.rs`, at `Scripted` level.

### `compaction_never_shrinks_the_store`
**Proves.** What is sent shrinks and what was said does not.
**Provider script.** Enough steps to force compaction, then an answer.
**Fixture.** Standard, plus request capture.
**Assertion.** A captured request carries an elision marker; the stored
message rows for the same turns carry the original bodies, at full
length.
**Defends.** `docs/DECISIONS.md` 2026-08-19, 2026-09-03; the recorder
receives the original before compaction rewrites the in-memory copy.
**Regression it catches.** Compaction rewriting the transcript before the
recorder sees it. `zorp-track` and the research capabilities treat the
store as evidence, so a record that moves under them is worthless, and
this is the failure that would not be noticed for months.
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has
`compaction_does_not_re_record_the_elided_transcript`. The whole run adds
the SQLite rows, which is where the evidence actually lives.

---

## D. Tool calls and their results

### `heredoc_with_an_apostrophe_is_not_denied`
**Proves.** A Python heredoc with an apostrophe in a comment runs, and
the run does not die of denials.
**Provider script.** Three `run_command` calls, each a
`python3 <<'PY' ... PY` block with an apostrophe in a comment, then an
answer.
**Fixture.** Tempdir, `--yes`, each script writing a distinct file.
**Assertion.** All three files exist; no tool result begins with
`denied:`; exit 0; `sessions.status` is `done`.
**Defends.** `docs/DECISIONS.md` 2026-09-03 (a quoted heredoc body is
data); `policy.rs` `split_heredocs`, `heredocs_on_line`.
**Regression it catches.** The measured one: the policy tokenizing a
quoted heredoc body as shell words, seeing an unclosed quote, failing
closed, and taking 18 of 21 tasks with it. The three-call shape is
deliberate, because that is the count at which `DENIAL_STREAK_LIMIT` ends
the run as `Blocked`, which is what turned a parse bug into lost runs.
**Cost.** Cheap.
**Existing coverage.** `policy.rs` has four heredoc unit tests. None of
them run a shell or reach the denial streak.

### `bare_heredoc_body_is_still_read_as_shell`
**Proves.** The other half of the same rule. An unquoted delimiter allows
expansion, so its body is still scanned, and a body handed to `sh` is a
script whatever its delimiter looks like.
**Provider script.** One `run_command` with an unquoted heredoc
containing `$(sudo ...)`, and one feeding a quoted heredoc to `bash`.
**Fixture.** Standard.
**Assertion.** Both come back denied, and the denial names the rule.
**Defends.** Same entry.
**Regression it catches.** A fix for the case above that goes too far and
skips every heredoc body, which would put `$(sudo rm -rf /)` back inside
the denylist's blind spot that the 2026-08-14 policy entry closed.
**Cost.** Cheap.
**Existing coverage.** `policy.rs` unit tests. Listed here because it is
the paired assertion for the case above and should be written with it, or
the fix will be tested only in the direction it was pushed.

### `turn_tool_output_cap_withholds_and_the_run_continues`
**Proves.** A turn whose tool output goes over budget gets a withheld
result that tells the model what to do, and the run keeps going.
**Provider script.** Three `read_file` calls on a large file in one
reply, then an answer.
**Fixture.** A file comfortably over `TURN_TOOL_OUTPUT_CAP` divided by
two.
**Assertion.** The third tool result is the withheld message naming the
tool and the path; the run exits 0.
**Defends.** `agent.rs` `TURN_TOOL_OUTPUT_CAP`.
**Regression it catches.** The cap failing the turn instead of the call,
or withholding without saying which call was cut, which leaves the model
with no way to recover.
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has
`one_turn_with_three_large_tool_results_withholds_the_third`. The whole
run adds only the outcome. Low value; write it late or fold it into
another case.

### `background_process_does_not_outlive_the_run`
**Proves.** A `start_background_process` child is not still running after
the process exits.
**Provider script.** One `start_background_process` call running a long
`sleep` that writes its pid, then an answer.
**Fixture.** Tempdir, `--yes`.
**Assertion.** After the run exits, the recorded pid is gone, polled with
a bound.
**Defends.** `tools/mod.rs` `start_background_process`,
`kill_background_process`; `sandbox::kill_process_group`.
**Regression it catches.** Orphaned processes accumulating across a
benchmark run, which is the kind of thing that makes the twentieth trial
behave differently from the first and looks like model variance.
**Cost.** Cheap to write, annoying to make reliable.
**Determinism.** Partly outside our control. Process teardown is
scheduler-dependent, so this must poll with a timeout rather than check
once, and even then it is the most likely case in the catalogue to go
flaky. Write it, but expect to give it a generous bound.

### `tool_result_status_words_are_a_contract`
**Proves.** The summary words a tool result carries are a fixed set, and
the ones the loop keys on keep their meaning.
**Provider script.** A run touching each built-in: a read, a search, a
write, a patch, a shell command that exits 0, one that exits non-zero,
one that is denied, and one naming a tool that does not exist.
**Fixture.** Tempdir with a git repo so `git_status` and `git_diff` work.
**Assertion.** The set of summary words seen matches a checked-in list.
**Defends.** `agent.rs`, where `succeeded` is
`!matches!(out.summary.as_str(), "denied" | "error" | "unknown tool")`,
and the trace's `tool.result.success` and the browser's line colour both
derive from it; `docs/DECISIONS.md` 2026-09-05 (the tool line carries its
result as colour).
**Regression it catches.** A tool changing its summary word, which
silently flips `tool.result.success` in every trace, repaints the browser
line, and changes what `zorp-eval` contracts see. Nothing today would
notice.
**Cost.** Cheap.

---

## E. Approvals and denials

### `denial_names_the_rule`
**Proves.** A denied call comes back with the rule that fired, in the
tool result the model reads.
**Provider script.** Four denied calls in separate turns, one per rule
family: a denylisted program, a redirect target outside the tree, a
destructive `rm`, an own-server URL.
**Fixture.** Standard, `--yes` so the denial is the policy's and not the
approval gate's.
**Assertion.** Each stored tool result names its rule, and the four names
differ.
**Defends.** `docs/DECISIONS.md` 2026-09-03 (a denial says which rule
fired), 2026-08-14 (command policy analyzes substitutions and redirect
targets).
**Regression it catches.** A refactor collapsing `deny_reason`'s messages
into one string. The model then cannot correct the command and retries
the same shape until the denial streak kills the run.
**Cost.** Cheap.
**Existing coverage.** `policy.rs` asserts specific reasons. Nothing
asserts they reach the model as distinguishable text.

### `denylist_beats_auto_approve`
**Proves.** `--yes` answers the approval gate and does not move the
denylist.
**Provider script.** One `sudo` call and one `git push`, then an answer.
**Fixture.** Standard, `--yes`.
**Assertion.** Both denied, no process ran, run continues.
**Defends.** `docs/DECISIONS.md` 2026-08-19 (the browser can stand its
approvals down), which pins the ordering of `Policy::decide` and the
approval gate.
**Regression it catches.** Swapping `decide` and the gate, which turns
one approval into a standing one.
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has
`a_denylisted_command_is_refused_even_under_auto_approve`, which is the
ordering test that decision asked for. This adds the CLI flag path and
the filesystem. Low added value; keep it, it is three lines.

### `denial_streak_ends_the_run_as_blocked`
**Proves.** Three consecutive denials end the run promptly, with the
outcome and the advice a person can act on.
**Provider script.** Three different denied calls, then a call that would
succeed.
**Fixture.** Standard, no `--yes`, non-interactive.
**Assertion.** Exit 1; `sessions.status` is `blocked`; stderr names
`--yes` and the approval preset; the fourth call never reached a tool.
**Defends.** `agent.rs` `DENIAL_STREAK_LIMIT`; `main.rs` `report_outcome`.
**Regression it catches.** The bound going away, which would leave a run
that cannot make progress grinding to the step limit. It is also the case
that gives the heredoc case its teeth, and it should be written first for
that reason.
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has `varying_denials_stop_early_with_blocked`.
The whole run adds the exit code, the status word and the stderr advice.

### `own_server_port_is_denied_through_a_shell_wrapper`
**Proves.** Under `zorp-web`, a `run_command` naming the server's own
loopback port is denied, and wrapping it in `sh -c` or a substitution
does not get past.
**Provider script.** Three calls: a bare `curl` at the server's port, the
same inside `sh -c`, the same inside `$(...)`. Plus one at a different
loopback port, which must be allowed.
**Fixture.** A running `zorp-web` on a known port, driven through its
turn endpoint.
**Assertion.** The first three denied, the fourth allowed.
**Defends.** `docs/DECISIONS.md` 2026-08-20 (a command may not call the
server it is running under); `policy.rs` `with_own_server`, which rides
on `deny_reason` for the recursion.
**Regression it catches.** One approved command turning into a standing
approval by curling `/api/sessions/:id/auto-approve`.
**Cost.** Expensive, because it needs the server and not just the binary.
**Existing coverage.** `policy.rs` has unit tests including the shell
wrapper and the allowed other port, and `zorp-web/tests/` covers turns.
The gap is the wiring: nothing proves `zorp-web` actually calls
`with_own_server` with its own port. That is the whole value of the case,
so write it as a narrow assertion on that wiring rather than as four
policy cases repeated at a higher level.

---

## F. The workspace boundary

### `write_outside_the_workspace_is_refused`
**Proves.** A file tool cannot write above the root it was given.
**Provider script.** `write_file` at `../escaped.txt`, then
`../../escaped.txt`, then an absolute path outside the tree, then an
answer.
**Fixture.** A tempdir nested two levels deep so there is somewhere to
escape to, with a sentinel file above it.
**Assertion.** Each call comes back with the escape error; no file exists
outside the root; the sentinel is untouched; the run continues.
**Defends.** `tools/mod.rs` `resolve_for_create`, which canonicalizes the
parent and checks `starts_with`.
**Regression it catches.** A refactor that canonicalizes the joined path
instead of its parent, which for a file that does not exist yet fails
outright, and the obvious fix for that failure is to skip the check.
**Cost.** Cheap.

### `symlink_out_of_the_workspace_is_refused`
**Proves.** The check survives a symlink, which is the case
canonicalization exists for.
**Provider script.** `read_file` and `write_file` through a symlink in
the tempdir pointing at a file above it.
**Fixture.** A tempdir containing a symlink to a sentinel outside it.
**Assertion.** Both refused with the escape error; the sentinel's
contents and mtime are unchanged.
**Defends.** `tools/mod.rs` `resolve_existing`.
**Regression it catches.** Replacing `canonicalize` with a lexical
`..`-collapsing normalizer, which is a natural-looking change for
Windows support or for paths that do not exist yet, and which reopens the
boundary.
**Cost.** Cheap.
**Determinism.** Fine on Unix. Skip on Windows rather than pretending.

### `everything_written_lands_under_dot_zorp`
**Proves.** A run that takes a note leaves it under `.zorp/notes/` and
creates no other top-level directory.
**Provider script.** `take_note`, then `search_notes`, then an answer.
**Fixture.** A clean tempdir.
**Assertion.** `.zorp/notes/` exists and holds the note; the round trip
finds it; the only new top-level entries are `.zorp` and whatever the
fixture put there.
**Defends.** `docs/DECISIONS.md` 2026-08-16 (everything zorp writes into
a project lives under .zorp/), which exists because the notes tools were
still writing `.qkb/` after the rename. Found by hand in UAT run 001, F2.
**Regression it catches.** Any tool acquiring its own dot-directory in a
user's repo. It has happened once and was caught by a person reading a
directory listing.
**Cost.** Cheap.

---

## G. The store and transcript replay

### `seed_sends_one_system_prompt_and_no_dangling_tool_call`
**Proves.** A resumed session sends exactly one system message and a
repaired transcript.
**Provider script.** Turn one makes a tool call and answers. Then the
process is run again with `resume <id>`.
**Fixture.** Shared `ZORP_STATE_DB` across both invocations; request
capture on the second.
**Assertion.** The second run's first request has exactly one `system`
message, and it is the current prompt; every assistant tool call in it
has a matching tool result.
**Defends.** `docs/DECISIONS.md` 2026-08-19 (a turn is seeded from the
store); `context_window::plan_seed`, `repair_tool_calls`.
**Regression it catches.** Stored system messages being replayed, which
the web server used to do once per turn, and a dangling tool call
surviving into a request, which a provider is entitled to reject. Both
fail at some provider-dependent depth rather than immediately.
**Cost.** Cheap once request capture exists.
**Existing coverage.** `context_window.rs` tests `plan_seed` and
`repair_tool_calls` on constructed inputs. Nothing tests the round trip
through the real store and the real `resume` path.

### `seed_drops_whole_exchanges_from_the_front`
**Proves.** When a seeded transcript will not fit, whole exchanges go
from the front, a user message and everything that answered it together,
and the newest exchange never goes.
**Provider script.** Several recorded turns, then a resume under a small
`ZORP_CONTEXT_TOKENS`.
**Fixture.** As above.
**Assertion.** The captured request contains no reply to a question that
is no longer in it, and the last exchange is whole.
**Defends.** Same entry.
**Regression it catches.** Half-dropped exchanges, which leave the model
reading answers to questions it cannot see. This produces confidently
wrong continuations rather than an error.
**Cost.** Cheap.
**Existing coverage.** `context_window.rs` unit tests. The whole run adds
the real store and the real env var.

### `undo_restores_what_diff_reported`
**Proves.** The recorder's ordering and `take_last_change` agree with
each other across a process boundary.
**Provider script.** A run that edits a file twice, then answers.
**Fixture.** Tempdir with a known file, shared store.
**Assertion.** `diff` names two changes with the right before and after
line counts; two `undo` calls restore the original bytes exactly; a third
says there is nothing to undo and exits 1.
**Defends.** `session.rs` `record_change`, `take_last_change`,
`render_change_summary`; UAT run 001 Area C tests 3 to 6, checked by hand.
**Regression it catches.** Change rows going in the wrong order, so undo
restores an intermediate version and reports success. Silent data loss in
a user's repo.
**Cost.** Cheap.
**Existing coverage.** `cli.rs` has `undo_restores_prior_file_contents`
for one change. The two-change case is what tests the ordering.

---

## H. The trace file and the eval contract

### `trace_event_types_are_pinned`
**Proves.** A model-free run emits exactly the set of `event_type` values
this repo has agreed on.
**Provider script.** A run that reaches every emitting site: a tool call,
a tool result, a file mutation, a verifier pass and fail, an assistant
claim, a termination, an infrastructure error.
**Fixture.** `ZORP_TRACE_FILE` in the tempdir, the identity env vars set.
**Assertion.** The sorted set of distinct `event_type` values equals a
checked-in list; every line parses; `TraceIdentity`'s fields are present
and flattened as expected.
**Defends.** `agent.rs` `TraceEvent` and its `serde` renames;
`docs/DECISIONS.md` 2026-08-14 (measurement code fails loudly).
**Regression it catches.** Renaming an event, which turns every
`zorp-eval` contract into `Unevaluable`. By that same decision an
unevaluable result is honest rather than wrong, which means it is also
silent, and a whole eval suite would report nothing while looking fine.
**Cost.** Cheap. This is the highest value-per-line case in the
catalogue.
**Existing coverage.** None that runs.
`zorp-eval/tests/instrumentation_validation.rs` is the only check and it
is `#[ignore]`d behind a release build and real model credentials.

### `contracts_name_only_events_the_agent_emits`
**Proves.** Every `event_type` string `zorp-eval` matches on is one the
agent actually produces.
**Provider script.** None. This is a comparison between the pinned list
above and the strings in `zorp-eval/src/contracts.rs`.
**Fixture.** None.
**Assertion.** The set difference is empty, or is exactly a checked-in
list of known-unemitted names with a reason beside each.
**Defends.** The same pair of files.
**Regression it catches.** A contract silently matching nothing, which
scores as a violated requirement or a satisfied forbid depending on which
side it sits, and either way is a fabricated result.
**Cost.** Cheap.
**Note, and it is a finding rather than a hypothetical.**
`zorp-eval/src/contracts.rs` matches on `"observation"` at lines 139, 194
and 205, and `zorp-agent/src/agent.rs` emits no such event. This case
fails today. That is the correct first result for it.

### `trace_carries_no_credential`
**Proves.** A run configured with an API key writes no copy of it into
the trace file.
**Provider script.** Any run that makes a tool call.
**Fixture.** `ZORP_API_KEY` set to a distinctive sentinel.
**Assertion.** The sentinel appears nowhere in the trace file.
**Defends.** `agent.rs` `TraceEvent`, which today carries tool names and
never arguments; `sandbox::redact_secrets`.
**Regression it catches.** Somebody adding an `arguments` field to
`TraceEvent::ToolCall` for debuggability. Trace files are collected by
`zorp-eval` and land in `jobs/`, so a key in one is a key in a directory
nobody treats as secret.
**Cost.** Cheap.
**Honest limit.** This pins the trace only. The store and the next
request do carry whatever a command printed, including an `env` dump, and
that is current behaviour rather than a bug this case should assert
against. If it should change, that is a decision, not a test.

---

## I. The process surface a harness reads

### `exit_code_and_stream_contract`
**Proves.** For each terminal outcome, the exit code, which stream the
text went to, and the status word written to `sessions.status`.
**Provider script.** One scripted run per outcome: an answer; a model
that never stops, against a small step limit; a failing verifier; a
transport error; three varying denials; three identical calls; a
cancellation.
**Fixture.** Standard, shared store, `--no-verify` off where the verifier
is the point.
**Assertion.** A table. `Complete` is exit 0, answer on stdout, status
`done`. Every other outcome is exit 1, stdout empty, a message on stderr,
and the status word from `main.rs`: `step_limit`,
`verification_failed`, `error`, `blocked`, `repeated_action`,
`cancelled`. A usage error is exit 2.
**Defends.** `main.rs` `report_outcome` and `finish`; `Outcome::describe`;
UAT run 001 Area A tests 1, 3, 9, 10 and 11, all checked by hand.
**Regression it catches.** Any change that makes a dead run look
finished, which has happened twice: `finish_reason=length` and the
cut-off stream. It also catches the answer moving to stderr or activity
moving to stdout, which would corrupt every downstream parse at once.
**Cost.** Cheap, and it is the case everything else in an eval suite
rests on. Write it first.
**Existing coverage.** `main.rs` has `finish_tests` over `report_outcome`
in process. `cli.rs` checks a few exit codes. Nothing covers the table.

### `cut_off_reply_is_an_error`
**Proves.** A reply with no tool calls and `finish_reason` of `length`
ends the run as an error, and the partial text is in the transcript but
not on stdout.
**Provider script.** A finished stream whose last chunk carries
`finish_reason: "length"` and a long content body.
**Fixture.** Standard.
**Assertion.** Exit 1; stdout empty; stderr names the output limit and a
byte count; the store holds the partial text as an assistant message.
**Defends.** `docs/DECISIONS.md` 2026-09-03 (a reply the provider cut off
at its output limit is an error).
**Regression it catches.** The measured one. A trial ended exit 0 with no
terminal line after nine minutes of the model writing one line of Python
2,300 times, and the benchmark scored it as an answer.
**Cost.** Cheap.
**Existing coverage.** `agent.rs` has
`a_reply_cut_off_at_the_output_limit_is_an_error_not_an_answer`, at
`Scripted` level. The whole run adds the exit code and the empty stdout,
which is the only part a harness above can see.

### `no_ansi_when_piped`
**Proves.** Piped stdout carries no escape sequences.
**Provider script.** A run whose answer contains markdown.
**Fixture.** Output captured through a pipe, which `Command::output`
already does.
**Assertion.** No ESC byte in stdout.
**Defends.** `main.rs` `render_assistant_text`, which is passed
`is_terminal()`; UAT run 001 Area A test 10.
**Regression it catches.** A renderer change that colours unconditionally
and corrupts every captured answer in a benchmark.
**Cost.** Cheap, one line inside another case.

---

## J. Build gates, not runs

These two are not whole-run cases. They are listed because they are the
largest uncovered gaps found while writing this, and they belong to
whoever owns CI rather than to the runner.

### `every_feature_flag_compiles`
Eight features are never compiled in CI at all: `otel`, `search`,
`clipboard` and `library` on `zorp-agent`, `search`, `memory` and `voice`
on `zorp-web`, and `library` on `zorp-track`. The regression is a feature
that stops compiling and nobody finds out until somebody turns it on. A
`cargo check` per feature is enough and does not need to run tests.
`library` pulls the arrow tree and is slow, so it belongs on the nightly
leg with the research stack rather than on every pull request.

### `research_suite_runs_when_the_loop_changes`
The research job runs per pull request only when a path filter matches,
and the filter does not include `zorp-agent/src/agent.rs`,
`model.rs` or `streaming.rs`. Those three are exactly what the four
capabilities are built on, so a change to the loop that breaks
`investigate` is caught after merge, on the nightly. Adding the three
files to the filter costs a full research build on the pull requests that
touch them, which is the pull requests that most need it.

---

## Already proved at this level, do not write these again

- **A stream dropped after delivery is re-asked, and the dead step leaves
  no assistant row.** `zorp-agent/tests/reask_dropped_stream.rs`, six
  tests, real binary, both framings, connection counted. Covers the
  bound, the peer-reset variant, and the before-a-reply re-send.
- **A cancel during a long answer records no half message.**
  `agent.rs` `a_cancel_during_a_long_answer_ends_the_run_without_recording_a_half_message`,
  which already runs against a local SSE endpoint.
- **The pooled connection losing its read timeout.**
  `streaming_timeout.rs` `a_second_request_cannot_lose_the_header_read_timeout_in_the_pool`,
  with a purpose-built two-request server.
- **A global flag before a subcommand.** `cli.rs`
  `a_global_flag_before_a_subcommand_does_not_turn_it_into_a_task`.
- **Project flavor trust on first use.** `cli.rs`
  `untrusted_project_verify_is_not_applied_noninteractively` and
  `project_flavor_approval_tightening_is_applied`.
- **The repeat guard, including a reworded description.** `agent.rs`,
  four tests.
- **`ThinkGate` across chunk boundaries and tool-call fragment joining.**
  `streaming.rs`, ten tests.
- **The two-sided retry bound, backoff, jitter and `Retry-After`.**
  `src/lib.rs`, five tests.

---

## What this suite deliberately does not cover

**Anything that needs a real model.** Every case here scripts the
provider. That is the whole point: a suite that gates the harness must
fail only when the harness changed. The moment a real model is in the
loop, a red result means either a regression or a bad day at the
provider, and a gate that cannot tell those apart gets ignored within a
week. `zorp-eval/tests/instrumentation_validation.rs` is the existing
example, and it is `#[ignore]`d for exactly this reason.

**Answer quality.** Whether the model answered well, whether it chose a
sensible tool, whether the draft `co_write` produced is any good. None of
that is harness behaviour. The runs in `jobs/` scored zero on every trial
and their real return was six decision log entries, all about the
harness. That is the lesson to take: this suite measures the thing those
runs were accidentally measuring, and leaves the thing they were nominally
measuring to the model-facing half of `zorp-eval`.

**The `zorp-eval` contract semantics.** Whether a contract's predicates
express what somebody meant, whether a task's `test_runner.sh` is a fair
test, whether the grader agrees with a person. `zorp-eval` has its own
tests for its own logic, and it already made the right decision about
honest non-results in 2026-08-14. The one place the two meet is the trace
format, which is section H, and that is a contract about names rather
than about meaning.

**The research capabilities' own behaviour.** `validate`, `investigate`,
`co_write`, `deliver`, `critique`, `panel` and the aryabhatta ledger all
have canned models and their own suites, and most of what they guarantee
is about a record rather than about a run. `investigate`'s kill threshold
enforcement and the pre-registration integrity check are gates that
matter enormously, and they are already tested where they live. The only
research-adjacent item here is the CI path filter in section J, which is
about whether those suites run at all.

**The browser.** `zorp-web`'s roughly 150 tests and `web/`'s jsdom suite
cover the API and the renderer. Two cases here touch the server
(`own_server_port_is_denied_through_a_shell_wrapper`) and both are marked
expensive for that reason. Rendering, streaming to the page, the artifact
pane, voice, recall and memory are not harness behaviour in the sense
this suite means, and pulling them in would double the runner's
dependencies for cases that already have a home.

**Timing as a pass condition.** Several cases spend real seconds waiting
on a configured timeout. None of them assert on elapsed time, and none
should. The 2026-08-23 timeout entry is a record of how badly a
time-shaped assumption can go wrong, and `retry_rate_limit.rs` already
established the right technique: count connections, because a re-send and
a slow first send look identical from the client's side.

**Windows.** `symlink_out_of_the_workspace_is_refused` and
`background_process_does_not_outlive_the_run` are Unix-shaped. Skip them
there rather than weakening them.

**Anything that cannot be made deterministic, named honestly.** Only one
case in this catalogue is genuinely at risk:
`background_process_does_not_outlive_the_run`, because process teardown
is the scheduler's business and not ours. It should poll with a bound and
will still be the flakiest thing here. Everything else is deterministic
given a scripted provider and a scrubbed environment, and where a case
looked otherwise it is because it was really a timing assertion in
disguise, which is why there are none.
