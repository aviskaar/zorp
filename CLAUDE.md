# zorp: Claude Code Instructions

zorp is a research agent for scientific discovery, part of the Aviskaar
monorepo. See `README.md` for positioning and `NOTICE.md` for licensing
context, and `AGENTS.md` for the tool-agnostic version of these
instructions (keep both in sync).

## What this repo is

A Rust workspace, forked from [quecto](https://github.com/adityak74/quecto)
(MIT) and renamed. Crates and binaries are `zorp`, `zorp-agent`,
`zorp-mcp`, and `zorp-eval`, not `quecto-*`. Env vars use the `ZORP_`
prefix. It's being extended into a harness for autonomous scientific
research.

## Working here

- `src/`, `zorp-agent/`, `zorp-mcp/`, `zorp-eval/` are inherited harness
  code. Read `docs/UPSTREAM_QUECTO_README.md` before assuming behavior;
  it documents the original design intent under the old `quecto-*` names
  (mentally substitute `zorp-*`).
- New, zorp-specific capabilities (research loop, experiment tracking,
  paper synthesis, etc.) should be clearly separated from inherited
  harness code as they're added. Use new crates or clearly named modules,
  not the inherited ones.
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
- `cargo build --workspace` and `cargo test --workspace` before considering
  Rust changes done.

## Writing style

Prose in this repo (README, docs, commit messages, comments) should read
as plainly and humanly as possible. No em dashes or en dashes as
punctuation; use a period, comma, colon, or a plain hyphenated compound
word instead. Prefer short, direct sentences over stacked clauses.

## Status

Early/pre-alpha. The base harness is vendored and renamed. zorp's actual
research-agent design (how it searches, experiments, and writes up
findings) has not been built yet.
