---
status: accepted
date: 2026-09-05
---

# ensemble is a review loop with a return edge, and every decision in it is code

**Decision:** The ensemble worth building over free models is not "run five, pick one". Five free OpenRouter models on the nine hard-tail terminal-bench-science tasks each scored 0 of 9 and their union is 0 of 9, so a selector has nothing to select. What the trials show instead is partial credit spread across models: 16 of 17 checks here, 8 of 9 there, an output written in the wrong shape somewhere else. So `zorp-agent ensemble` runs one main model on the task, has reviewer models test it under three code-defined lenses (contract, reproduction, adversary), counts agreement in code, and sends corroborated findings back to the main model for a bounded revision. It lives in `zorp-agent/src/ensemble/` behind a non-default `ensemble` feature and reuses `panel`.

Three things hold it up. A reviewer may run commands, because the verifier's tests are hidden, but it has no write tool and the check that it wrote nothing is a hash comparison in code before and after each reviewer. A reviewer is dropped for altering an output, or for two unusable replies, and for nothing else: not for disagreeing with the others, and not for the main model rejecting its findings, because inside a run there is no ground truth and a loop that keeps the agreeable reviewers converges into one reviewer with extra cost. And the main model is one `Agent` for the whole run: `Agent::run` appends a user message to the live transcript, which is what chat does, so the main model keeps everything it learned without a stored session being resumed. Two more things are held by construction and never by an instruction to a model: a reviewer transcript is kept in memory and all of them are written when the run ends, so an earlier reviewer's transcript does not exist while a later reviewer, which has a shell, is running; and a dropped reviewer's edit stays in the file, since this hashes and never copies, so the main model is told which files were altered even when the round corroborated nothing, and that round stops with its own reason. Stopping quietly there would hand the verifier a file a reviewer wrote, and a contaminated reward looks exactly like a real one.

**Why:** The failures cluster into wrong numbers at the end of a mostly right pipeline and outputs never written or written in the wrong shape. Both are things a second reader can catch before submission and neither is caught by running the same model again. Memoized verdicts and the open/addressed ledger exist so a round costs only the reviews whose inputs changed; the free tier allows 1000 requests per UTC day per key, which is about five tasks a day at this shape.

**What it ruled out:** A tool that starts a run or a review. Reviewers that read each other. Any roster change on a model's opinion. A reader that selects on findings text; the record stores it as `claim_model_authored` and `evals/harbor/ensemble_report.py` never reads it. Per-role endpoints or keys. A browser route, for now. Concurrent reviewers: every role shares one free-tier key, so they run one at a time.

**Not decided yet:** Whether one lens per reviewer or every lens per reviewer corroborates better, and whether dots-3 belongs on the roster. The record answers both once it exists, and a person reads the table before anything changes.

See `docs/superpowers/specs/2026-09-05-ensemble-dag-design.md` and `docs/superpowers/plans/2026-09-05-ensemble-dag.md`.
