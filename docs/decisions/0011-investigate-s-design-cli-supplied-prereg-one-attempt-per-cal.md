---
status: accepted
date: 2026-08-09
---

# investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill

**Decision:** `investigate` takes `--metric-name`/`--kill-threshold` as CLI
arguments (not agent-proposed) the first time it runs for a track, writes
and checkpoints the pre-registration, then runs exactly one attempt per
invocation, records a typed metric via the existing `zorp-track`
experiment tables, and hands the kill/keep decision to a human checkpoint
rather than comparing the metric to the threshold in code.

**Why:** A human-committed threshold is the whole point of
pre-registration; an agent-proposed one would defeat it. One attempt per
invocation keeps every attempt visible at a checkpoint instead of burning
budget inside a single call before a human sees anything. No stored
"kill direction" (above/below is favorable) means no risk of that logic
guessing wrong; the checkpoint prompt shows the human the number and the
threshold and lets them decide, matching the existing "no hard experiment
budget" decision.

**Ruled out:** Multi-attempt loops within a single invocation. Automatic
threshold comparison deciding kill/keep without a human. Requiring a
prior `validate` approval before `investigate` can run (the existing
standalone-capabilities decision already rules this out).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`
