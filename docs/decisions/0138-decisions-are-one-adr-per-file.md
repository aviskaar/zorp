---
status: accepted
date: 2026-10-04
---

# Decisions are one ADR per file

**Decision:** The decision log moves from one `docs/DECISIONS.md` to one
record per decision in `docs/decisions/`, in the MADR layout from
adr.github.io. Bodies are copied unchanged. `docs/DECISIONS.md` stays as a
table from date to record, so citations by date still resolve.

**Why:** One file meant every PR that added a decision conflicted with every
other one, and no entry could be linked on its own.

**What it rules out:** Rewriting old entries while moving them, and adding
new decisions to `docs/DECISIONS.md`. See issue 277.
