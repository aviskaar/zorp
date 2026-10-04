---
status: accepted
date: 2026-08-14
---

# command policy analyzes substitutions and redirect targets

**Decision:** the run_command denylist now recurses into `$(...)`,
`<(...)`, and `>(...)` bodies the same way it already did for `sh -c`
payloads, tokenizes redirect operators as distinct tokens and checks
their targets, and denies destructive `rm` whose targets escape the
repository root. Unbalanced substitution syntax fails closed. `>
/dev/null` is now explicitly allowed, where the old substring check
denied it.

**Why:** `$` was an ordinary word character to the tokenizer, so
`echo $(sudo rm -rf /)` parsed as a call to `echo`, resolved to Ask, and
ran under `--yes`. The redirect check matched four literal spellings and
missed `> ~/.ssh/authorized_keys`. The root-rm guard matched only a bare
`/`, so `rm -rf /*` passed. These were holes in an otherwise careful
fail-closed design, not a missing design.
