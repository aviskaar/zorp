---
status: accepted
date: 2026-08-14
---

# measurement code fails loudly instead of guessing

**Decision:** `zorp-eval` gained three honest non-result states rather
than folding unevaluable runs into pass or fail. An unreadable trace
records `trace_unavailable` and skips contract evaluation entirely,
malformed lines inside a valid trace are skipped and counted in a new
`runs.trace_malformed_lines` column, and ordering predicates over
seq-less events report `unevaluable`. Unknown predicate ids are a
load-time hard error. The unimplemented LLM grader and the `eval`
subcommand now return not-implemented errors instead of reporting
success.

**Why:** every one of these paths previously produced a confident,
recorded result from evidence that was never actually evaluated. A
truncated final trace line became "all contracts failed"; a typo in a
contract id became a permanent violation or a silent pass. For a harness
whose only purpose is trustworthy measurement, a fabricated result is
worse than a missing one.
