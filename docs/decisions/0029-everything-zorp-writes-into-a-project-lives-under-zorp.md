---
status: accepted
date: 2026-08-16
---

# everything zorp writes into a project lives under .zorp/

**Decision:** the `take_note` and `search_notes` tools write to
`.zorp/notes/` instead of `.qkb/`. No migration is provided and no
fallback read of `.qkb/` is kept.

**Why:** `.qkb` is a quecto-era name that the rename missed. Crates,
binaries, env vars, and the config directory all became `zorp`, and the
research stack already writes `.zorp/flavor.toml`, `.zorp/mcp.toml`,
`.zorp/tracks/`, and `.zorp/zorp.duckdb`. A second, differently named
directory appearing in a user's repo the first time an agent takes a
note is a leak of the fork's history into a user's project. Found in the
first UAT (`docs/uat/UAT-report.md`, F2).

**What it rules out:** carrying a compatibility read of `.qkb/`. zorp is
pre-alpha and the notes tools have no install base worth a migration
path; a fallback would keep the old name alive in the code indefinitely
to serve nobody. Notes already written to `.qkb/` stay on disk and stop
being found. The bootstrap decision (2026-08-08) dropped quecto's own
`.qkb/` directory as a session artifact but left the tools that create
it; this finishes that.
