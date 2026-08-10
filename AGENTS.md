# zorp: Agent Instructions

zorp investigates hard questions and delivers evidence-backed answers,
not just academic research, any question that can be turned into a
defensible answer using evidence. Part of the Aviskaar monorepo. See
`README.md` for positioning and `NOTICE.md` for licensing context. This
file is the tool-agnostic counterpart to `CLAUDE.md`. Keep both in sync
when either changes.

## What this repo is

A Rust workspace, forked from [quecto](https://github.com/adityak74/quecto)
(MIT) and renamed. Crates and binaries are `zorp`, `zorp-agent`,
`zorp-mcp`, `zorp-track`, and `zorp-eval`, not `quecto-*`. Env vars use
the `ZORP_` prefix. It's being extended into a harness for evidence-based
investigation: validate a question, investigate it, co-write the
resulting artifact, deliver it in the right form.

## Working here

- `src/`, `zorp-agent/`, `zorp-mcp/`, `zorp-eval/` are inherited harness
  code. Read `docs/UPSTREAM_QUECTO_README.md` before assuming behavior;
  it documents the original design intent under the old `quecto-*` names
  (mentally substitute `zorp-*`).
- `zorp-track/` is zorp's own research foundation (multi-track evidence
  records, git-backed pre-registration, checkpoints, DuckDB + LanceDB),
  built, not inherited. The four capabilities (validate, investigate,
  co-write, deliver) sit on top of it as `zorp-agent` subcommands,
  behind the `research` feature. Validate, investigate, and co-write are
  built (co-write most recently); deliver is not.
- New, zorp-specific capabilities should be clearly separated from
  inherited harness code as they're added. Use new crates or clearly
  named modules, not the inherited ones.
- `reference/` is gitignored, local-only material (e.g. AI-Scientist-v2)
  used for design inspiration. Never copy code from it into tracked files;
  its license doesn't permit redistribution under zorp's terms. Read it
  for ideas, then implement independently.
- `docs/superpowers/specs/` holds zorp's own design docs from brainstorming
  sessions. `docs/upstream-quecto/` holds quecto's historical specs, plans,
  and changelog. Read those for context, but don't edit them; they're a
  record of the past, not of zorp.
- `docs/DECISIONS.md` is the product and architecture decision log. Add a
  short entry whenever a real decision gets made (not every change; use
  judgment), and check it before re-deriving a decision that's already
  there.
- `docs/ARCHITECTURE.md` points at the current, approved architecture and
  scope specs. Check there before assuming what zorp's capabilities are
  called or what they cover; both have changed at least once already.
- `cargo build --workspace` and `cargo test --workspace` before considering
  Rust changes done.
- `cargo test --workspace` does not exercise the `research` feature
  (validate, investigate, co-write, deliver). Run
  `cargo test -p zorp-agent --features research` explicitly whenever
  research-feature code changes.

## Writing style

Prose in this repo (README, docs, commit messages, comments) should read
as plainly and humanly as possible. No em dashes or en dashes as
punctuation; use a period, comma, colon, or a plain hyphenated compound
word instead. Prefer short, direct sentences over stacked clauses.

## Status

Early/pre-alpha. The base harness and the research foundation
(`zorp-track`) are built and tested. validate, investigate, and
co-write are built and tested; deliver has not been built yet.
