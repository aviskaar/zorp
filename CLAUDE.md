# zorp — Claude Code Instructions

zorp is a research agent for scientific discovery, part of the Aviskaar
monorepo. See `README.md` for positioning and `NOTICE.md` for licensing
context, and `AGENTS.md` for the tool-agnostic version of these
instructions (keep both in sync).

## What this repo is

A Rust workspace, currently a direct fork of
[quecto](https://github.com/adityak74/quecto) (MIT), being extended into a
harness for autonomous scientific research. Crates still carry their
`quecto-*` names from the upstream project — that's expected for now, not
a bug. Renaming to `zorp-*` is a deliberate future step, not something to
do incidentally while touching unrelated code.

## Working here

- `src/`, `quecto-agent/`, `quecto-mcp/`, `quecto-eval/` are inherited
  harness code — read `docs/UPSTREAM_QUECTO_README.md` before assuming
  behavior; it documents the original design intent.
- New, zorp-specific capabilities (research loop, experiment tracking,
  paper synthesis, etc.) should be clearly separated from inherited
  harness code as they're added — new crates or clearly-named modules, not
  mixed into the quecto crates.
- `reference/` is gitignored, local-only material (e.g. AI-Scientist-v2)
  used for design inspiration. Never copy code from it into tracked files
  — its license doesn't permit redistribution under zorp's terms. Read it
  for ideas, then implement independently.
- `docs/superpowers/specs/` holds design docs from brainstorming sessions.
  Check there before re-deriving design decisions already made.
- `cargo build --workspace` and `cargo test --workspace` before considering
  Rust changes done.

## Status

Early/pre-alpha. The base harness is vendored; zorp's actual research-agent
design (how it searches, experiments, and writes up findings) has not been
built yet.
