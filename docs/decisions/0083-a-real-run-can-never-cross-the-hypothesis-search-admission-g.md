---
status: accepted
date: 2026-08-31
---

# a real run can never cross the hypothesis-search admission gate, so a third condition is recorded

**Decision:** `record_conditions` in `zorp-agent/src/investigate/mod.rs` now
also records `max_steps`, alongside the existing `model` and
`checkpoint_mode`. It reads `agent.config().max_steps`, already public on
`AgentConfig`, so no new accessor was needed.

**Why:** before starting a real corpus run meant to build up the
anomaly ledger toward hypothesis search's admission gate (12 admitted
anomalies spanning at least 3 distinct condition keys, 2026-08-28), a
check of every non-test call site found exactly two: `model` and
`checkpoint_mode`. No number of real attempts could ever produce a third
distinct key, so the gate was unreachable by construction, independent
of how much the ledger got used. `max_steps` is a harness-observed fact
with no model text in it, the same shape the other two conditions
already have, so it costs nothing against integrity rule 5.

**Ruled out:** running the planned corpus anyway and hoping the gate
would somehow get crossed. It could not have, and the run would have
spent real model calls to learn nothing new about this gap.
