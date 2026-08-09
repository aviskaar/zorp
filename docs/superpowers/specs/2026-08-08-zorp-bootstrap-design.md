# zorp bootstrap: base harness setup

**Date:** 2026-08-08
**Status:** approved

## Purpose

Stand up the `zorp` repo (part of the Aviskaar monorepo, dedicated domain
`zorp.dev`) as a research agent for scientific discovery, using
[quecto](https://github.com/adityak74/quecto) (MIT) as the base execution
harness. [AI-Scientist-v2](https://github.com/SakanaAI/AI-Scientist-v2) is
pulled in as local-only reference/inspiration while designing zorp's
research-agent capabilities (experiment tree search, autonomous
hypothesis-to-paper loop), not as code we redistribute.

This is a scaffolding step only — it does not design zorp's actual
research-agent architecture (search loop, experiment tree, paper pipeline).
That comes later, once the base harness is in place.

## Decisions

1. **quecto vendoring** — Vendored as a source snapshot (not a git
   subtree/submodule) directly at the zorp repo root, as a single initial
   commit. quecto's own dev-tooling directories (`.claude/`, `.superpowers/`,
   `.qkb/`) are dropped since they're artifacts of quecto's own Claude Code
   sessions, not harness code. `LICENSE` (MIT) is kept, plus a `NOTICE.md`
   crediting quecto/adityak74 as upstream origin. Crate names stay
   `quecto-*` for now; renaming to `zorp-*` is a future, separate change.

2. **AI-Scientist-v2** — Cloned into `reference/AI-Scientist-v2/`, added to
   `.gitignore`. Its custom "Responsible AI Source Code License" is
   restrictive enough that we don't want it traveling with zorp's public
   repo; it stays local-only, for inspiration.

3. **New top-level docs**
   - `README.md` — zorp positioning (research agent for scientific
     discovery, built on quecto, part of Aviskaar, zorp.dev), quick start,
     status (early/pre-alpha).
   - `CLAUDE.md` — Claude Code working instructions for this repo (Rust
     workspace conventions, quecto-inherited vs. zorp-specific code,
     pointer to AGENTS.md).
   - `AGENTS.md` — tool-agnostic agent instructions (same spirit as
     CLAUDE.md).
   - `docs/paper/` — placeholder for the eventual arXiv writeup, not filled
     in yet.

## Out of scope

- Designing zorp's actual research-agent capabilities.
- Renaming quecto crates/packages.
- Setting up the zorp.dev website.
- CI/CD, publishing pipeline, arXiv paper content.
