---
status: accepted
date: 2026-08-20
---

# the architecture index is deleted, the specs are the index

**Decision:** `docs/ARCHITECTURE.md` is removed. The per-capability specs in
`docs/superpowers/specs/` are the architecture record, and this log is the
decision record. There is no third document summarizing them.

**Why:** it was a summary of things that each already had a durable home, so
it could only ever be a second copy going stale. It had started to: it still
described `erbga` as off any critical path the day after 2026-08-19 wired it
in. A pointer file that lies is worse than no pointer file, and the failure
mode is built in rather than accidental, because nothing makes a summary
update when the thing it summarizes changes.

**What went with it, honestly:** the only index of which spec covers which
capability, and the only place recording that the systems paper is written
but unposted and that hand-checked runs live in `uat/`. Nothing else stated
those. If the paper and UAT status need a home, it is `README.md` or their
own directories, not a revived index.

**What did not go:** the constraint that four capabilities is the whole set.
That is recorded here, 2026-08-14 and 2026-08-15, where the fifth was
designed and turned down. The 2026-08-19 entry below refers to
`docs/ARCHITECTURE.md` in the present tense; per this log's convention that
entry is left as written, and this one supersedes the pointer.

**Superseded by this entry:** the `docs/ARCHITECTURE.md` references in the
2026-08-19 entry below.
