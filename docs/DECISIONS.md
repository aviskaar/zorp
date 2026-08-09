# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

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
