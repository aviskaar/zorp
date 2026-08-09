# zorp — Agent Instructions

zorp is a research agent for scientific discovery, part of the Aviskaar
monorepo. See `README.md` for positioning and `NOTICE.md` for licensing
context. This file is the tool-agnostic counterpart to `CLAUDE.md` — keep
both in sync when either changes.

## What this repo is

A Rust workspace, forked from [quecto](https://github.com/adityak74/quecto)
(MIT) and renamed: crates and binaries are `zorp` / `zorp-agent` /
`zorp-mcp` / `zorp-eval`, not `quecto-*`. Env vars use the `ZORP_` prefix.
It's being extended into a harness for autonomous scientific research.

## Working here

- `src/`, `zorp-agent/`, `zorp-mcp/`, `zorp-eval/` are inherited harness
  code — read `docs/UPSTREAM_QUECTO_README.md` before assuming behavior;
  it documents the original design intent (under the old `quecto-*` names
  — mentally substitute `zorp-*`).
- New, zorp-specific capabilities (research loop, experiment tracking,
  paper synthesis, etc.) should be clearly separated from inherited
  harness code as they're added — new crates or clearly-named modules, not
  mixed into the inherited ones.
- `reference/` is gitignored, local-only material (e.g. AI-Scientist-v2)
  used for design inspiration. Never copy code from it into tracked files
  — its license doesn't permit redistribution under zorp's terms. Read it
  for ideas, then implement independently.
- `docs/superpowers/specs/` holds zorp's own design docs from brainstorming
  sessions. `docs/upstream-quecto/` holds quecto's historical specs/plans/
  changelog — read for context, don't edit (it's a record of the past, not
  of zorp).
- `cargo build --workspace` and `cargo test --workspace` before considering
  Rust changes done.

## Status

Early/pre-alpha. The base harness is vendored and renamed; zorp's actual
research-agent design (how it searches, experiments, and writes up
findings) has not been built yet.
