---
status: accepted
date: 2026-08-09
---

# zorp's product is four standalone capabilities, human-authored papers only

**Decision:** zorp offers four capabilities that each work standalone,
chained by human checkpoints when used as a full loop: validate an idea
(literature/novelty check), run experiments (staged, sandboxed, every
attempt recorded), co-write a paper (zorp drafts from the run record, a
human edits and is the author of record, zorp never outputs a paper as
"done" on its own), and find a venue (match a finished paper against a
conference/journal catalog). zorp does not take a hard dependency on
Aviskaar-private infrastructure (ORR, lab-engine/Catalyst); an ORR
adapter can be optional and later, not the foundation.

**Why:** Most "AI scientist" agents assume the deliverable is a finished,
autonomously written paper. That's the wrong shape: AI-authored papers
are rejected outright at most venues, so the paper step has to be
collaborative, with the human as author, not a generator with a
claim-check pass bolted on. The standalone-capabilities framing also
matches how the harness will actually get used: someone validating one
idea, or just running an experiment, without wanting the full loop.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-architecture-design.md`.
