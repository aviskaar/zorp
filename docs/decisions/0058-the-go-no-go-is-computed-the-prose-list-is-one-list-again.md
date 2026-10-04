---
status: accepted
date: 2026-08-21
---

# the go/no-go is computed, the prose list is one list again

**Decision:** three things that aryabhatta claimed but did not do now hold.
`CalibrationReport::verdict(tolerance)` turns a report into `Go` or
`NoGo(reasons)`. `MODEL_AUTHORED_COLUMNS` covers all nine prose columns in the
schema instead of three, and every module that enforces integrity rule 5 reads
that one list. `record_metric` refuses a non-finite outcome, the way
`record_expectation` already refused a non-finite forecast.

**Why now:** the four merged pull requests were reviewed against their own
claims, and each of these was a place where a document asserted a property the
code did not have. That gap is worse than a missing feature, because a reader
stops checking a thing the instructions say is guaranteed.

**The go/no-go is computed and still not enforced.** Before this,
`calibration_report()` had no caller outside its own tests and the decision
existed only as a sentence. Nothing consults a verdict before the ledger is
written and nothing should: the ledger has to be buildable in order to be
measured, so a runtime block is circular, since a gate that refuses to run until
it has passed can never pass. The design agrees, framing the report as the
deliverable that decides "whether the rest of the architecture is worth
building", which is a decision for people. What changes is that the decision now
has arithmetic attached instead of a feeling, and CLAUDE.md no longer implies
enforcement that does not exist.

**The tolerance has no default, on purpose.** The design says the number "should
be set from the first observed curve rather than guessed here", so shipping a
constant would guess exactly what it refuses to guess. `Tolerance` is a newtype
whose constructor refuses anything outside (0.0, 1.0]: zero is refused because
no forecaster lands on its stated coverage exactly, so a zero tolerance is a
no-go wearing the costume of a measurement, and above one is refused because it
admits every curve that can exist, which is a go wearing the same costume.
`MIN_CALIBRATION_N` is 50 and is a constant, because that number is in the
design.

**Not a number is never a pass.** The verdict checks finiteness before every
comparison. A NaN gap loses every comparison including `gap > tolerance`, so
without that check a report made entirely of NaN bands returns `Go`. It is the
one failure mode where the wrong answer is also the reassuring one. Nine
mutations were run against the verdict and all nine were caught.

**One list, in one place, as the comment always said.** `MODEL_AUTHORED_COLUMNS`
carried three columns while a second, longer copy sat in a `partition` test
carrying five. The constant's own doc comment predicted this: "a second copy
would drift the first time a model-authored column was added, and the drift
would be silent." It had drifted, and the shared list was the shorter one, so
`hypothesis_snapshot` and `assumptions` were unguarded in `detectors` and
`families` while `partition` caught them. The list is now nine columns, each
annotated with its table, checked at runtime against `duckdb_columns()` so it
cannot guard a column that does not exist, and pinned by name so a member cannot
be swapped for a harmless string.

**A name check is not enough, because `SELECT *` has no names.** The rule was
enforced by substring search, which a star defeats completely, and `families`
already contained one. It was safe only because the table it read happens to
hold no prose. That query now names its columns and a bare star is refused
everywhere, with `COUNT(*)` still allowed.

**Testing for the column name tested the wrong thing.** The old inquiry test
asserted a brief did not contain the strings "decision_notes", "prompt_shown" or
"explanation", which are column names. A brief that interpolated the prose
stored in those columns would have passed. It now plants a sentence in every one
of the nine columns and asserts that sentence reaches no brief, and it fails if
a column joins the shared list without a fixture row to write into.

**An outcome that is not a number is refused where it enters.** `record_metric`
had no finiteness check while `record_expectation` had one, so a stored NaN
counted as a miss in every future calibration report, permanently, and could not
be told apart from a forecast that genuinely missed. The two refusals are a pair
and neither works alone. This also makes the finiteness claim in `rerun.rs`
true: repeats are read back out of `metrics`, and that comment now names the
writers providing the guarantee rather than gesturing at "the callers above this
function".
