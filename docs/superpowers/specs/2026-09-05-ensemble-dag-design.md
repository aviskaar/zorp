# Ensemble: a review loop over free models, driven by code

**Date:** 2026-09-05
**Status:** approved in conversation, not yet built. The measurement that
motivates it is in the harbor job directories under `jobs/` dated
2026-09-04 and 2026-09-05, one per model.

## Purpose

The research question is whether free models, together, reach an answer
none of them reaches alone. Five free OpenRouter models were run one at a
time on the nine hard-tail terminal-bench-science tasks with the harness
fixes of 2026-09-04 and 2026-09-05 in place. Every model scored 0 of 9,
and the union across models, which is what a perfect selector would
score, is also 0 of 9. A selection ensemble has nothing to select from.

What the per-task results do show is that partial credit is spread
across models rather than concentrated in one:

| Task | Best partial result | Model |
|---|---|---|
| guided-wave-localization | 16 of 17 checks, error 39 mm against a 20 mm tolerance | nemotron-3-super |
| ont-tn-qc | 8 of 9 | dots-3 |
| small-area-equivalence | 24 of 25 | minimax-m3 |
| variable-star-vetting | CSV written, 6 of 8 | dots-3 |
| mendota-ice-phenology | result file written, values wrong | minimax-m2.7 |
| cilia-segmentation | 7 of 9 | minimax-m3 |

The failures cluster into two shapes. Wrong numbers at the end of a
mostly right pipeline, and required outputs never written or written in
the wrong shape. Both are things a second reader can catch before
submission, and neither is caught by running the same model again. So
the ensemble worth building is not "run five, pick one". It is one model
doing the work, other models testing, validating and attacking it, and
the findings going back to the first model for a revision. A DAG with a
return edge, driven by code.

## What exists already

`panel` (`zorp-agent/src/panel/`) runs several reviewers over one target
at once from code-defined lenses. No reviewer sees what another said,
and agreement is counted afterwards in code as an `Agreement` of one
locus with the lenses that raised it and the worst severity. It is a
reader and not a gate, reviewers get a read-only tool set, and a panel
is launched by a person and never by a model. `critique` audits a draft
against a track's evidence record and revises what the record does not
support, within a bound of rounds.

Ensemble reuses panel's independence, its lens shape, its verdict
parsing and its agreement counting. It adds three things panel does not
have: a return edge to the model that did the work, a reviewer that may
run commands, and a check in code that the reviewer changed nothing.

## Decisions

### Where it lives

A new module `zorp-agent/src/ensemble/` behind a non-default `ensemble`
feature, with a CLI subcommand:

```
zorp-agent ensemble --yes "<instruction>"
```

The subcommand runs the main model on the instruction exactly as a plain
run does, in the same workspace with the same tools, then runs the loop.
It is code that launches agent runs in sequence. No model launches any
of them. There is no tool that starts a run or a review, and `agent.rs`
carries a test under the feature asserting the filtered tool set has no
such tool, in the shape of the test panel already has.

### Roles

Roles come from a TOML file named by `ZORP_ENSEMBLE`, and never from the
instruction text. `rounds` must come before the first table header,
because TOML scopes a bare key to the most recently opened table:

```toml
rounds = 2

[main]
model = "nvidia/nemotron-3-super-120b-a12b:free"

[[reviewer]]
model = "minimax/minimax-m3:free"
[[reviewer]]
model = "dots-studio/dots-3-note-preview:free"
[[reviewer]]
model = "minimax/minimax-m2.7:free"
```

All roles share the one provider endpoint and key from the environment.
Every model measured so far is behind OpenRouter, and a per-role
endpoint is a feature nobody has asked for. The main model keeps
`ZORP_MAX_STEPS`. A reviewer gets a smaller step limit, 20 by default
under `ZORP_ENSEMBLE_REVIEW_STEPS`, because a review that needs sixty
steps is doing the task over.

The first roster is main nemotron-3-super with reviewers minimax-m3,
dots-3 and minimax-m2.7. Super ran the most complete pipelines in the
oracle run. The three reviewers are two model families super is not,
dots-3 showed the best domain judgement on ont-tn-qc, and reviewer
prompts are short so the provider's 65,536-token output cap that cost
dots-3 four trials does not bite.

### Lenses

Three, defined in code, assigned one per reviewer in TOML order:

- **Contract.** Every output the instruction requires exists at its path
  with the named schema, columns and units.
- **Reproduction.** Recompute one key number from the data by an
  independent route and compare it with what was submitted.
- **Adversary.** Assumptions, edge cases, unit and index errors, and a
  plain attempt to break the result.

One lens per reviewer means a corroborated finding was reached from two
different angles by two different models, which is the only agreement
worth counting. Each reviewer sees the instruction, the file list, the
outputs and nothing another reviewer said.

### What a reviewer may do

A reviewer gets the read tools plus `run_command`, because the
verifier's tests are hidden and the only way to test is to run checks in
the workspace. It gets no write tool and no patch tool. That is not
enough on its own, since a shell can write, so the check is in code:
before the reviewers start, the loop hashes every file the main run
recorded as changed and every file under the paths the instruction names
as outputs. After each reviewer finishes, the same set is hashed again.
A reviewer whose run altered any of them is dropped, its findings never
reach the main model, and the record says so with the file names.
Testing is possible, tampering is detected, and detection is code.

### Return edge

Reviewer answers are the fenced JSON panel already parses: locus,
severity, reason. Code counts agreement. A finding reaches the main
model when two lenses raised the same locus, or when one lens raised it
at the highest severity. Everything else is recorded and not sent.

The corroborated findings go to the main model as one user message in
its own session, resumed through `plan_seed` so it keeps everything it
learned. The message is a fence with a per-round marker under the same
boundary sentence `memory` and `zorp-skill` use, labelled as reviewer
text that grants no tool, loosens no approval and bypasses no denylist
entry. The main model is asked to address each finding that is right and
to say why where one is wrong.

Rounds stop at the bound, 2 by default. They stop earlier when a round
corroborates nothing, and earlier still when a revision changed no
output hash, because the main model declined every finding and asking
again is asking the same question.

### Memoization and pruning

Within a run, two things are memoized and both are keyed in code:

- **A reviewer's verdict is a function of what it saw.** The loop hashes
  the files a reviewer examined. If a later round would show a reviewer
  the same hashes, its earlier findings are reused and no request is
  spent.
- **The findings ledger persists across rounds.** Each corroborated
  finding has a locus and a status, open or addressed, and the status
  flips when the hash of the file the finding names changes. The main
  model is told what is still open and is not re-told what it already
  fixed. A finding that survives every revision is reported as such at
  the end.

Pruning happens only on failures code can see. A reviewer that altered
an output is dropped for the run. A reviewer that returned nothing
parseable, or whose reply was cut off at the provider's output limit, is
dropped for the round, and dropped for the run after the second time.

No reviewer is scored on whether the main model accepted its findings,
or on whether it agreed with the others, and no roster changes inside a
run for any reason but the three above. Inside a run there is no ground
truth, so "good reviewer" could only mean "reviewer the main model
liked", and a loop that keeps the agreeable ones converges into one
reviewer with extra cost. That is the failure panel's design note warns
about, and it is the repo's oldest rule in a new shape: a model's
judgement of a model must not become the next round's evidence.

### Record and reader

Every run writes one JSON file under `<workspace>/scratch/ensemble/`,
named by the run id: the roster, the rounds, every finding with its lens,
locus, severity, whether it was corroborated and whether it was
addressed, every prune with its code-visible reason, and the request
count per role. Findings text is stored and labelled as model-authored.

The reader is a short Python script in `evals/harbor/` that joins those
files with harbor's rewards. It answers which reviewer's findings
predicted the score, from the code-derived columns only. That is where
the roster gets decided, across runs against the verifier, and a person
reads the table before anything changes.

## Deliberately absent

- No tool starts a run or a review.
- No reviewer writes. The hash check drops one that did.
- No reviewer reads another reviewer.
- No roster changes on a model's opinion.
- No reader consults model-authored text. Findings text is stored under
  that label and the reader never selects on it.
- No per-role provider endpoint or key.
- No browser route yet. The CLI and a direct call are the only two ways
  in, the same as panel.

## Harness

`evals/harbor/zorp_agent.py` uploads the roles file when `ZORP_ENSEMBLE`
is set on the host, sets the variable in the container, and invokes
`zorp-agent ensemble --yes` instead of the plain run. Nothing else in the
adapter changes. The main model's transcript stays at
`zorp-agent.txt`, and each reviewer's transcript is written beside it,
named by role and round, so a trial can be read the way trials are read
today.

## Cost

One task costs roughly the main run, three reviewer runs of up to 20
steps each, and one revision, per round. With two rounds and the
memoization above that is about 150 to 200 requests. The free tier
allows 1000 requests per UTC day per key, so about five tasks a day.

## Measurement

The nine tasks with the first roster, against two baselines already
measured: nemotron-3-super alone (0 of 9) and the oracle union (0 of 9).
Pass or fail per task is the headline. Verifier checks passed per task
is the finer signal and is reported beside it, because a loop that moves
guided-wave from 39 mm to 19 mm and ont-tn-qc from 8 of 9 to 9 of 9 has
answered the question even if a third task stays at zero.

## Tests

Run with `cargo test -p zorp-agent --features ensemble`. Agreement and
selection reuse panel's tests. Against the SSE stub, each of the
following counts connections or reads the record rather than checking
for an error:

- A reviewer that edits an output is dropped and its finding is absent
  from the message the main model receives.
- Unchanged hashes skip the second reviewer run.
- A reviewer that returns nothing parseable twice is absent from the
  third round's roster.
- A revision that changes no output hash ends the loop before the bound.
- A finding raised by one lens at a low severity never reaches the main
  model; the same finding from two lenses does.
- The filtered tool set under the feature has no tool that starts a run.

## Open questions

None that block the plan. Two are noted for the reader, not for the
build: whether one lens per reviewer or every lens per reviewer gives
better corroboration, which the record can answer once it exists; and
whether dots-3 belongs on the roster at all, which the same record
answers.
