# Metric provenance: who measured the number

**Date:** 2026-08-21
**Status:** proposed. Nothing here is built. The problem it addresses was
found by running `investigate` end to end for the first time, and that
run is recorded below rather than described in the abstract.

## Purpose

`investigate` takes the value of the pre-registered metric from the
model's own reported JSON. That number decides two things: whether the
pre-registered kill threshold is breached, and, once aryabhatta reads the
record, whether the attempt counts as an anomaly.

Nothing checks it. There is no second source for the number and no mark
on the record saying where it came from, so a wrong number and a right
one are indistinguishable after the fact.

This spec adds the second source where one can exist, and the mark
always.

## What the first real runs showed

On 2026-08-21, `investigate` was run against a real question with
`ZORP_FORECAST=1` and a local model, `qwen3.8:27b`. The hypothesis was
that the aryabhatta modules in `zorp-track` carry more lines inside
`#[cfg(test)]` blocks than outside them. Metric `test_to_impl_ratio`,
kill threshold 1.0, higher-is-better, so a value below 1.0 kills the
track.

The first run hit its step limit and recorded no outcome. It did record
its conditions and its forecast, `2.1 in [1.3, 4.5]` at 0.70, both before
the attempt, and the forecast survived the attempt's failure without
being scored. That is the correct behaviour and it is what the ordering
guarantee is for.

The second run, with a higher step limit, completed. Forecast `1.4 in
[0.8, 2.5]` at 0.72 recorded before the attempt, outcome
**1.129** recorded after, no breach, track stays active, calibration
n=1 covered=1.

Then the part worth writing the spec about.

Establishing what the number should have been took three attempts by two
different parties, and the first two were wrong:

| measurement | value | correct |
|---|---|---|
| this spec's author, first pass | 1.260 | no |
| the model's unfinished script, run prematurely | 0.003 | no |
| brace matching every `#[cfg(test)]` block | **1.1261** | yes |

The author's first pass assumed the first `#[cfg(test)]` in a file runs
to the end of it. That is true in eight of the nine modules and false in
`detectors.rs`, which has three separate test blocks with implementation
between them, so the impl lines in the gaps were counted as tests. The
0.003 came from executing a script the model had written and explicitly
not finished, and attributing its output to the model, which was not a
fair reading of what the model claimed.

The model's reported 1.129 is within 0.3 percent of the truth. On this
run the agent measured better than the person checking it.

That is the argument for this spec, and it is a different argument from
the one it looked like it would be. The hazard is not that models are
uniquely bad at measuring. It is that **an unreviewed measurement is
unreliable no matter who makes it**, and `investigate` currently has no
place to put a measurement anybody has reviewed. Nothing checked 1.129.
It happened to be right. Had it been 0.003, the track would have been
killed, the deviation from a `[0.8, 2.5]` forecast would have registered
as an anomaly, and the re-run gate would have admitted it, because a
deterministic error reproduces perfectly.

The near miss is the evidence, not a catastrophe that happened.

## Why the re-run gate cannot catch this

The gate exists to answer one question: is this deviation a phenomenon or
is it variance. It answers it by re-running and seeing whether the
deviation survives. On the erbga corpus that worked exactly as designed,
turning twelve deviations into three transient and nine volatile and
admitting none of them.

Reproducibility is the gate's evidence of a real effect. A deterministic
measurement error has perfect reproducibility. The gate is not weak here,
it is answering a different question correctly: the number really does
come out the same every time. Whether the number is *right* is not
something re-running can establish, because every run consults the same
broken instrument.

So the gate defends against variance and offers no defence at all against
a wrong instrument. Those are two halves of one picture and neither
substitutes for the other.

## The rule this follows from

Integrity rule 5 says no detector, and nothing in the search layer, may
read a column holding model-authored text, because the agent's own
speculation would become tomorrow's observation.

A model-reported number is the same hazard with the prose removed. It is
authored by the model, it enters the record unchecked, and a detector
reading it is reading the agent's claim about the world rather than the
world. The only reason it is not already covered is that rule 5 names
text columns, and the value in `metrics` is a number.

Today every metric `investigate` writes is model-authored in exactly this
sense. The rule that keeps the ledger honest has a hole in it the size of
the only producer the ledger has.

## Design

Two parts. Neither is sufficient alone.

### A. A pre-registered verification command

A pre-registration may carry a command that computes the metric.

```
Metric: test_to_impl_ratio
Kill threshold: 1
Threshold direction: higher-is-better
Verification: python3 scripts/test_impl_ratio.py
```

After an attempt completes and its result parses, `investigate` runs that
command, parses a single number from its stdout, and records **that** as
the metric value. The model's reported number is kept beside it, not
discarded.

Four properties, each load-bearing:

- **The command comes from the human, never the model.** This is the
  whole design. A verification command the agent can write is the agent
  marking its own work with extra steps. There must be no tool, no
  prompt, and no code path by which a model supplies or edits it, and a
  test should assert this the way `agent.rs` asserts there is no
  `spawn_subagent` tool.
- **It is pre-registered, so it is fixed before the answer is known.**
  It lands in `prereg.md`, which is git-committed, which means it cannot
  be adjusted after seeing a result you dislike. This is the same
  guarantee that already stops the Kill Threshold moving, applied to the
  instrument as well as the target.
- **It is measurement, so it is code.** Same split as `critique` and the
  detectors: the code produces the number, the model may interpret it.
- **A failed command does not fall back to the model's number.** If the
  command errors, times out, or writes something that is not a number,
  the attempt is recorded as unverifiable and the metric is not written.
  Falling back would mean the guarantee quietly disappears exactly when
  it fails, which is the worst time for it to disappear.

Divergence between the verified value and the reported one is worth
recording in its own right. A large gap says the model cannot measure
what it claims to measure, which is a fact about the agent rather than
about the hypothesis, and it is the first thing anyone debugging a run
would want.

### B. Provenance on every metric

Every metric row records where its value came from:

- `code-verified`, the value is a verification command's output
- `model-reported`, the value is the model's claim and nothing checked it

Provenance is not optional and has no default. A metric written without
one is a bug, not a `model-reported` metric.

Then the rule that makes B matter: **a detector, and anything in the
search layer, may only read metrics whose provenance is
`code-verified`.** This is rule 5 extended from text columns to unchecked
numbers, and it should be enforced the same way rule 5 is, by a test that
fails when a query touches the wrong thing.

## Schema

`preregistrations` gains:

| column | type | note |
|---|---|---|
| `verification_command` | TEXT NULL | null means no verification is possible for this metric |

`metrics` gains:

| column | type | note |
|---|---|---|
| `provenance` | TEXT NOT NULL | `code-verified` or `model-reported` |
| `reported_value` | DOUBLE NULL | the model's claim, when a verified value displaced it |

When provenance is `model-reported`, `value` is the model's number and
`reported_value` is null. When it is `code-verified`, `value` is the
command's output and `reported_value` is what the model said. Divergence
is derived, not stored, because a stored derived quantity is one more
thing that can disagree with its inputs.

The prereg file format gains an optional `Verification:` line. Since
`prereg.md` is the source of truth that the DuckDB store rebuilds from,
the parser must round-trip it, and the existing tamper check must cover
it: a verification command edited after the fact is exactly as serious as
a kill threshold edited after the fact.

## What this costs

Under this design the anomaly ledger admits nothing from an `investigate`
run that has no verification command. That is a real cost and it should
be stated plainly rather than discovered later.

It is the right cost. The alternative on offer is not "a fuller ledger",
it is "a ledger containing numbers nobody checked", and the whole
argument for the re-run gate is that a ledger you cannot trust is worse
than an empty one. An empty ledger is the honest state for a record
nobody has fed, and that is already how forecasting behaves when it is
switched off.

It also gives the researcher a clear lever. If you want your
investigation to produce anomalies, register a way to measure it. That is
a reasonable thing to ask of someone claiming to have discovered
something.

## What this does not cover

**Metrics no command can compute.** Plenty of real questions have
outcomes that are not a number a script can produce. Those investigations
still run, still record, still enforce their kill threshold from the
model's number, and are still excluded from the ledger. This spec does
not improve them. It stops them contaminating the record, which is a
smaller claim.

**A wrong verification command.** Pre-registration fixes the command in
advance, it does not make it correct. A human who registers a broken
script gets a confidently wrong number with `code-verified` on it. What
changes is that the error is now in a committed artifact that a person
wrote and a reviewer can read, rather than in a script a model wrote
mid-run and threw away. That is a real improvement and it is not the same
as a guarantee.

**The kill threshold still fires on model-reported numbers.** Deliberate.
Killing a track is a decision the researcher pre-registered and should
keep happening when verification is unavailable. Excluding those metrics
from the *ledger* is about what zorp may later claim to have discovered,
which is a higher bar than what an individual investigation does about
its own threshold. Worth revisiting if it proves wrong in practice.

## Testing

The interesting tests are the ones that fail when a guarantee is removed,
so each of these should be checked by mutation rather than assumed:

- A metric written with no provenance is refused.
- A detector query against `metrics` that does not constrain provenance
  fails the rule-5 style test. Mutate a detector to drop the constraint
  and the test must go red.
- A verification command that fails, times out, or prints a non-number
  records the attempt as unverifiable and writes no metric. Mutate the
  fallback back in and a test must go red.
- No tool and no prompt path lets a model set `verification_command`,
  asserted directly the way the absence of `spawn_subagent` is.
- `prereg.md` round-trips the `Verification:` line, and editing it after
  the fact trips the existing tamper check.

The end to end case is the run this spec came from. The same question
and a registered verification command that brace-matches the test
blocks: the recorded metric must be the command's 1.1261 and not
whatever the model reported, the model's number must be kept beside it,
and the track must survive. Worth keeping precisely because the model
was nearly right that day. A test that only proves the mechanism catches
a wildly wrong model proves less than one that shows the verified number
displacing a plausible one.

## Open questions

- **Where does the command run, and under what limits.** It is arbitrary
  code from a committed file, which is the same trust level as a build
  script, but it needs a working directory, a timeout, and a decision
  about whether it inherits the run's environment.
- **How the number is parsed from stdout.** Last line, whole stdout
  trimmed, or a fenced block. Whole stdout trimmed is the strictest and
  probably right, since a command that prints anything else has not been
  written for this purpose.
- **Whether `validate` should ask for a verification command** when it
  scores a question, so the absence is a known gap at the start rather
  than a discovery at ledger time.
