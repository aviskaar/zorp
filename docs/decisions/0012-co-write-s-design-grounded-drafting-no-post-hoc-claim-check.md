---
status: accepted
date: 2026-08-09
---

# co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track

**Decision:** `co-write` hands the agent the track's actual recorded
evidence (validate's verdict if present, every metric investigate
recorded) as structured data in the prompt and instructs it to cite only
those figures, rather than drafting freely and then scanning the output
to verify numeric claims afterward. Requires at least one recorded
metric to run at all. The agent's answer is written directly to
`draft.md`, no scored JSON block. Unlike validate and investigate,
rejecting co-write's checkpoint does not kill the track: a draft not
being ready isn't evidence the investigation failed.

**Why:** Grounding at the input side (only real numbers ever reach the
model) is simpler and more reliable than extracting and re-verifying
numeric claims from free-form prose after the fact, which is a much
harder problem with its own false-positive/negative risk. Requiring a
metric to exist keeps co-write from drafting off a validate pass alone,
which is a go/no-go check, not evidence. Not killing the track on
rejection matches the normal expected path once a draft exists: a human
takes over editing `draft.md` directly, or the call runs again.

**Ruled out:** A post-hoc claim-check pass over the drafted prose.
Tamper-evidence hashing of `draft.md` like `prereg.md`'s SHA-256 (a
mtime-based warning only, not an integrity guarantee). Killing the track
on a rejected co-write checkpoint.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-co-write-design.md`
