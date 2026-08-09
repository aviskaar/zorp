# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

---

## 2026-08-09: zorp's own arXiv paper is about the harness, not a discovery it made

**Decision:** The paper zorp itself publishes to arXiv is a systems paper
describing zorp: its minimal-harness design, its lineage from quecto, and
its evals/benchmarks (including comparisons against heavier frameworks
like Sakana's AI-Scientist-v2 and Aviskaar's own lab-engine/Catalyst
pipeline). It is not a scientific-discovery paper produced by using zorp
to research some unrelated topic.

**Why:** Stated directly. This scopes `docs/paper/` and keeps it separate
from the product itself: what zorp offers users (validate an idea, run
experiments, co-write a paper, find a venue) is general-purpose and not
about zorp. What zorp publishes about itself is a tools paper, the same
genre as the papers describing AI-Scientist-v2 or other agent harnesses.

**Ruled out:** Treating "zorp writes a research paper" as meaning zorp
needs to autonomously produce a novel scientific result end to end before
it counts as done. That's a much larger, different bar than a systems
paper needs, and conflating the two would have quietly inflated scope.

---

## 2026-08-09: zorp's product is four standalone capabilities, human-authored papers only

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

**Full writeup:** proposal artifact from this session (not yet a spec in
`docs/superpowers/specs/`; still pending brainstorming and a real design
pass before implementation).

---

## 2026-08-08: No em dashes or en dashes in repo prose

**Decision:** README, docs, commit messages, and comments in this repo
use plain punctuation (periods, commas, colons, plain hyphens) instead of
em dashes or en dashes, so the writing reads as plainly human as the code
itself.

**Why:** Requested directly, to keep the project's public-facing writing
from reading as AI-generated.

**Recorded in:** `CLAUDE.md` and `AGENTS.md` under "Writing style," so it
applies to all future writing in this repo, not just this pass.

---

## 2026-08-08: README rewritten as a full project front page

**Decision:** README expanded to badges, a why/architecture/getting-started/
status structure, with every command and env var in it checked against
the actual source rather than assumed.

**Why:** The repo will be public eventually and needs a front page that
holds up, not a stub.

---

## 2026-08-08: Harness renamed from quecto to zorp

**Decision:** All crates, binaries, env vars, and CLI/log strings renamed
from `quecto*` / `QUECTO_*` to `zorp*` / `ZORP_*`. quecto's own historical
docs (changelog, UAT reports, issue log) moved to `docs/upstream-quecto/`
and left untouched, since they document quecto's own history under its
own name.

**Why:** Deferred at bootstrap time on purpose (see the entry below), then
done once the base harness was in place and it was clear what was
actually being kept versus rebuilt.

---

## 2026-08-08: quecto vendored as the base harness, AI-Scientist-v2 kept local-only

**Decision:**
- [quecto](https://github.com/adityak74/quecto) (MIT) vendored as a
  source snapshot at the zorp repo root, fresh git history, crate names
  left as `quecto-*` initially (renamed later, see above).
- [AI-Scientist-v2](https://github.com/SakanaAI/AI-Scientist-v2) cloned
  into `reference/`, gitignored, used for design inspiration only and
  never committed.

**Why:** quecto is MIT licensed, so forking and modifying it directly is
safe. AI-Scientist-v2 uses a custom, restrictive "Responsible AI Source
Code License" that shouldn't travel with zorp's public repo, so it stays
local-only.

**Full writeup:** `docs/superpowers/specs/2026-08-08-zorp-bootstrap-design.md`
