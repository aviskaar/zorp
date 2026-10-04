---
status: accepted
date: 2026-10-03
---

# local-weights measurements get a sibling table, and the table refuses a missing value

**Decision:** perplexity, peak host memory, device memory and parameter
counts go in `bench_local_results`, beside `bench_results` in the same
telemetry database, keyed by session, runtime and metric. Not new columns
on `bench_results`: those rows are one per item with a correct, incorrect
or unevaluable outcome, and perplexity is neither correct nor incorrect,
while memory and parameter counts belong to a runtime and not an item.
First slice of #259: the table and its reader and writer, with no producer
yet.

**The 2026-09-18 rule, enforced in the database.** A measurement that did
not happen is not a zero. `Measurement` has no variant that holds both a
number and a reason. The table also refuses a measured row with no value,
an unevaluable row with a value or without a reason, and a measured row
that does not say its unit and method. A writer that skips the type still
cannot store a zero for a runtime that failed to load. SQLite stores a NaN
as NULL, so a measured NaN is refused by the same check.

**Every value says how it was measured.** `method` is required on a
measured row, because peak memory on Linux and macOS is read two different
ways, and a parameter count derived from the architecture is not the same
claim as one read from a model card.

**Still open on #259, not decided here:** which runner comes first, how
memory is read on each platform, and the order of the remaining work. The
proposal on the issue asks for sign-off on those.
