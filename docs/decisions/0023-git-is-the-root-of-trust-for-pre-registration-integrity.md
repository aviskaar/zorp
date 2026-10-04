---
status: accepted
date: 2026-08-14
---

# git is the root of trust for pre-registration integrity

**Decision:** rebuilding the evidence store from `prereg.md` files no
longer trusts the files on disk. The rebuild hashes the committed git
blob and compares it against the working tree; a mismatch is an
integrity error rather than a fresh row. A file with no commit behind it
is marked unverified instead of being presented as equivalent to a
committed one. `verify_prereg_integrity` now also checks the recorded
`git_commit_hash`, which was previously written but never read.

**Why:** the recovery path recomputed the hash from whatever was on disk
and stored that as authoritative, so deleting the DuckDB row or
corrupting one byte of the store turned a tampered pre-registration into
a verified one. The tamper-evidence guarantee was defeated by the
recovery path meant to protect it. Two existing tests asserted this
behavior as correct and were rewritten.
