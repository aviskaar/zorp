---
status: accepted
date: 2026-09-01
---

# the fence is the model's punctuation, not its answer

**Decision:** `parse_attempt_result` and `parse_validation_result` now
read the same way `parse_forecast` does. They share one set of
extractors (`zorp-agent/src/blocks.rs`), read backwards to the last
block that is actually the answer, accept a fence left open at end of
input, and fall back to a bare `{...}` object when no fence carries one.

Run 7's corpus lost 10 of 20 attempts to
`no fenced JSON block found in the agent's answer`. Every failure was
the same small local model and every one had a complete result object in
its raw text with no backticks around it. That was recorded on
2026-08-31 as an output-contract problem, which it is, but the contract
that was wrong was ours: the model answered the question and we threw
the answer away over punctuation. `parse_forecast` had already been
through this twice, once for unclosed fences and once for bare objects,
and the fix here is that fix applied to the other two parsers rather
than a third invention.

Two things beyond the reported bug came out of it.

**Backwards, not forwards.** The old attempt parser took the *first*
block carrying a `metric_value`. A model that fills in the requested
shape while narrating ("I'll report `{"metric_value": 0, ...}` once I've
measured") would have that placeholder recorded. This is the worse
failure of the two, because it does not fail the attempt: it writes a
wrong number into the evidence record and carries on. Nobody had
observed it, and nothing would have shown it if it had happened.

**`validate` had the same defect** and was fixed with it. Its parser is
where the attempt parser was forked from, it faces the same models, and
fixing only the path that was reported would have left the sibling
broken while looking finished.

Widening where an answer may be found does not widen what counts as one.
Every candidate still faces a shape check before it is selected, and
then every check that was already there: `metric_value` must be present
and finite, and `validate`'s "no citation, no claim" rule is untouched.
An answer that cannot be read must never become one that was invented.

Not fixed, and deliberately named rather than quietly left:
`critique/claims.rs`, `panel/verdict.rs` and `capsule.rs` each still
carry their own fence scanner with the same blind spots. None of them
writes a number to the evidence record, which is why they were not
bundled in here, and they should move to `blocks.rs` when someone next
touches them.
