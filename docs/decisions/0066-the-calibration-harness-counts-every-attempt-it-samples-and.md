---
status: accepted
date: 2026-08-22
---

# the calibration harness counts every attempt it samples, and prints why it dropped each one

**Decision:** `zorp-agent/tests/evidence_calibration.rs` accounts for
every sampled directory. Each attempt is either scored or discarded into
one of eight named categories, a `Tally` counts and prints in the same
call so neither can happen without the other, and the run asserts that
scored plus discarded equals sampled before it prints a calibration
report. The per attempt work moved into `record_attempt`, which takes
the store write as a closure, so the whole accounting path runs in tests
with no model and no database.

**Why:** the 60-directory registry run reported 25 discards. Three
discard paths printed three different phrasings and the summary used the
word from only one of them, so a grep for that word found 19 and the
other six looked like silent losses. They were not lost. One was a step
limit and five were dropped connections to the model endpoint, printed
as `no answer (...)`. A count nobody can check against the lines above
it is a count nobody can act on, and this run exists to produce a
defensible go/no-go.

**Categories report zero rather than disappearing.** PR #86 fixed both
parser reasons, so `no fenced json block` and `not the shape asked for`
should fall to zero. A category that vanishes when it stops firing takes
the evidence of the fix with it.

**A discard is never a way to raise the pass rate.** Nothing invents an
interval, an unreadable answer contributes nothing to `n`, and
`a_discarded_attempt_never_becomes_a_scored_one` says so.

**What a discard now carries:** an excerpt of the model's answer, next
to the reason. The two bugs #86 fixed could not be replayed because only
the error survived. That text is model authored, it goes to stdout and
nowhere else, and it is never written to the store, so no detector and
nothing in the search layer can read it back.

---
