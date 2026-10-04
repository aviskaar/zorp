---
status: accepted
date: 2026-09-18
---

# `bench` is the third part of `zorp-eval`, and it never gates

**Decision:** `zorp-eval bench` runs public benchmarks (MMLU, MMLU-Pro,
GPQA, TruthfulQA mc1, GSM8K) against every runtime in a manifest and
prints one table. It is a report and nothing more. No merge-gating job
runs it, its exit code does not track any score, and no nightly runs it
yet. See issue #257; the local-weights half is #259.

**Why it never gates.** For the reason `compat` does not. It talks to
live models over the real network, so a provider outage, a rate limit, a
deprecated model id or a gateway routing to a different checkpoint can
all change a row, and none of them is the code. A gate that fails for
reasons outside the pull request teaches people to re-run it until it
passes, which is worse than no gate. `harness` is the gate, because the
only thing that can fail it is the code. A score moving is a question for
a person reading the table, and an exit code that tracked accuracy would
invite something to start reading it instead.

**Why unevaluable is its own state.** A measurement that did not happen is
not a zero. If an endpoint that refused the connection scored 0.0, "this
model is bad at GPQA" and "the endpoint was down" would be the same row,
and an automatically generated table would propagate that quietly. Every
item ends correct, incorrect or unevaluable, the table shows attempted,
scored and unevaluable counts side by side, and a row that scored nothing
reads n/a. `ContractOutcome::Unevaluable` is the precedent. A reply that
arrived and commits to no readable answer is the model's failure and is
scored as wrong, counted separately as `unparsed`.

**Why the name.** `harness` already means the scripted provider on
loopback. A benchmark suite under that name would send the next reader to
`zorp-eval/src/harness/`, which is the opposite kind of measurement.

**Why in-process.** Bench sends through `zorp::http_agent` and
`zorp::send_json_retrying` rather than spawning `zorp-agent`, because a
benchmark item is one question and one answer, not an agent loop, and the
agent's system prompt and tools would be part of what got measured. It
clears every inherited `ZORP_` variable from its own process before the
first request and takes its timeout and retry bound from the case;
`zorp::Retrying::with_policy` was added so the bound is stated rather than
read from the environment.

**What it rules out:** vendoring any dataset (they are fetched and cached
outside the tree, and GPQA, which is gated and carries canary strings, is
never fetched at all); a model grading answers or authoring any cell;
comparing against published scores, since prompt format and grader move
those more than the model does; and perplexity, memory and
active-parameter metrics, which need local weights (#259).
