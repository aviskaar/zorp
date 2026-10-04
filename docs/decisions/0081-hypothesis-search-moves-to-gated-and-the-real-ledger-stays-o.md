---
status: superseded
date: 2026-08-28
---

# hypothesis search moves to Gated, and the real ledger stays out of reach

**Decision:** the structured-genome hypothesis search that the
2026-08-19 design left Proposed is built, in
`zorp-track/src/hypotheses.rs`, on a new representation-agnostic
`Problem` trait in `erbga`. It is validated against synthetic ledgers
with planted structure and against nothing else. No code reads the real
anomaly ledger into it; the adapter does not exist. The admission gate:
hypothesis search may run on real data only once the ledger holds at
least 12 admitted anomalies spanning at least 3 distinct condition
keys, and crossing that gate is a person's decision, recorded here when
it happens. In the registry of the 2026-08-19 spec, the entry moves
from Proposed to Gated; that spec stays as written.

**Superseded by** the 2026-09-02 entry above for the statement of the
gate. The algorithm, its validation and the atoms vocabulary stand.

**Why:** the registry admits an idea on evidence, and the evidence for
running on real data has not arrived, because the ledger is empty. What
could be admitted now is the algorithm itself, so that is what was
built and gated. Three choices carry the weight. The vocabulary is
condition atoms, key and value pairs, not bare keys, because a real
ledger records the same keys on every row with differing values, so the
key set separates nothing. The parsimony weight lambda is swept and
never chosen, the same discipline `families` applies to theta, and only
claim sets stable across a band are reported. And the erbga refactor is
pinned: a seeded run is asserted bit-identical across the trait
extraction, because the four benchmarks certify the graph path under
its recorded seeds and a reordered RNG draw would quietly retire that
certification. The trait's own documentation says the benchmarks
certify nothing about any other representation; hypothesis search's
validation is its planted-structure tests, which certify it on
synthetic ledgers only. 12 matches the minimum support every
non-habituation boredom detector already uses, for the same reason: a
structure fitted to fewer rows is arithmetic about those rows.

One caveat from the validation gate belongs in this record, because the
person at the admission gate should read it. On rows with no planted
structure the search often reports one atom claimed in both directions
at once: a two-direction claim covers an anomaly whichever way it
deviated, so on small random samples it buys real coverage for two bits
of parsimony cost, and in a sweep of 37 fixture seeds only 4 stayed
silent. The committed no-plant test pins one of those quiet seeds and
says so in a comment. Silence on unstructured data is therefore a
property to demand of a real run through a permutation null at that
time, not something the synthetic gate can promise on its own.
