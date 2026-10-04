---
status: accepted
date: 2026-08-17
---

# CI decides what to run with git, not a downloaded action

**Decision:** the research-stack path filter is a `git diff` and a grep
instead of `dorny/paths-filter`.

**Why:** the action is fetched from codeload when the job starts, and
when codeload answers 429 or 503 the job fails during setup before
running anything. That happened on three runs in one morning, twice in a
row on a rerun of the same commit, on pull requests that had not touched
the research stack and would have skipped every step anyway. A
dependency that can be unavailable was sitting in the critical path of
every pull request in order to decide to do nothing.

**What it rules out:** the action's richer glob syntax. The filter is six
patterns; if it ever needs more than a grep can express, reconsider. A
diff that fails now assumes the research stack changed, because failing
towards running the tests is the only safe direction.
