---
status: accepted
date: 2026-08-14
---

# kill thresholds carry a direction, and are enforced

**Decision:** a pre-registration now records a threshold direction
(`lower-is-better` or `higher-is-better`) alongside the metric and the
number, and `investigate` compares each recorded attempt against it. A
breach kills the track. `--threshold-direction` is required whenever a
threshold is set, the direction lives in `prereg.md` (so the existing
SHA-256 hash and git commit cover it), and it has its own column in the
`preregistrations` table.

**Why:** the threshold was only ever formatted into a prompt string and
never compared to anything, so a track that badly missed its own
threshold stayed Active. That is the one guarantee the whole product
rests on. A bare number could not be enforced even in principle, since
nothing said which side of it was failure.

**What it rules out:** guessing. A breach is exempt from
`AutoApprove`/`--yes`, because auto-approving the one decision that
exists to stop a run defeats the point. A legacy pre-registration with
no recorded direction is skipped with a loud warning rather than
enforced against an assumed direction, since guessing wrong would kill
healthy tracks.
