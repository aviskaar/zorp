---
status: accepted
date: 2026-08-08
---

# quecto vendored as the base harness, AI-Scientist-v2 kept local-only

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
