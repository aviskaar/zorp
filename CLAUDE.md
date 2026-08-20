# zorp: Claude Code Instructions

zorp investigates hard questions and delivers evidence-backed answers,
not just academic research, any question that can be turned into a
defensible answer using evidence. Part of the Aviskaar monorepo. See
`README.md` for positioning and `NOTICE.md` for licensing context, and
`AGENTS.md` for the tool-agnostic version of these instructions (keep
both in sync).

## What this repo is

A Rust workspace, forked from [quecto](https://github.com/adityak74/quecto)
(MIT) and renamed. Crates and binaries are `zorp`, `zorp-agent`,
`zorp-mcp`, `zorp-track`, and `zorp-eval`, not `quecto-*`.
`zorp-search`, `zorp-skill`, and `erbga` are further workspace members
and ship no binary; see the bullets below. Env vars use the `ZORP_`
prefix. It's being extended into a harness for
evidence-based
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
  behind the `research` feature. All four are built and tested
  (deliver most recently).
- `critique` (`zorp-agent/src/critique/`) is a gate on co-write's
  artifact, not a fifth capability. It audits `draft.md` against the
  track's own evidence record and revises what the record does not
  support, within a bound (`--critique-rounds`, `ZORP_CRITIQUE_ROUNDS`,
  default 2). The audit is code, the model only inventories claims, and
  the pass refuses if the record moved under it, so it cannot touch the
  Kill Threshold or anything else pre-registered. It lives behind
  `research` alongside the four capabilities and is covered by the same
  `cargo test -p zorp-agent --features research` run.
- New, zorp-specific capabilities should be clearly separated from
  inherited harness code as they're added. Use new crates or clearly
  named modules, not the inherited ones.
- `zorp-search/` is zorp's own web search capability: a
  `SearchProvider` trait with Tavily as the first provider. It depends on
  no other workspace member and knows nothing about agents or tools.
  `zorp-agent` exposes it as the `web_search` built-in behind the
  non-default `search` feature, which is the only built-in that sends
  anything over the network. `research` deliberately does not enable it;
  run `--features research,search` when you want it. The API key comes
  from `ZORP_TAVILY_API_KEY` and never from a flavor manifest.
- `zorp-skill/` is zorp's own skill capability: discovery and parsing
  for Claude Code compatible skills (`SKILL.md` in a directory, YAML
  frontmatter plus a markdown body). Like `zorp-search` it depends on no
  other workspace member and knows nothing about agents or tools.
  `zorp-agent` exposes it as the `skill` built-in, on by default, which
  reads local files only. A skill body is untrusted input: it can grant
  no tool, loosen no approval, and bypass no denylist entry, and
  `allowed-tools` in a skill's frontmatter is parsed, warned about, and
  ignored. See `docs/DECISIONS.md` (2026-08-18) before changing any of
  that. Skills are not capsules; the same entry says why both exist.
- `erbga/` is neither inherited harness code nor a zorp capability, so
  none of the bullets above applies to it. It is a standalone,
  zero-dependency implementation of published prior work (Rao, Janikow,
  Bhatia, Climer, MWAIS 2018): a genetic algorithm for graph community
  detection, validated against that work's four benchmarks. It knows
  nothing about zorp and nothing depends on it. It came out of a design
  that was then rejected (`docs/DECISIONS.md`, 2026-08-15) and is kept
  on its own terms as a validated artifact. Don't wire it into the
  research stack without a decision that says to, and don't delete it as
  dead code.
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
  Rust changes done. The tree is `cargo fmt` clean and CI gates on it, so
  run `cargo fmt --all` before committing.
- `zorp-agent/src/context_window.rs` is the one place that decides how large
  the context window is, how full it is, and what to drop when it fills.
  Compaction there is deterministic: it elides the oldest tool-result bodies
  and, on the seed path only, drops the oldest whole exchanges. No model
  writes a summary, and nothing in it ever writes to the store. The window is
  unknown unless `ZORP_CONTEXT_TOKENS` says otherwise, on purpose: no endpoint
  can be asked and no default is right for everyone. `zorp-web` and the CLI's
  `resume` both seed a turn through its `plan_seed`, which is what gives the
  browser conversational memory. See `docs/DECISIONS.md` (2026-08-19) before
  changing any of that.
- `web/` is TypeScript and no Rust job compiles a line of it. After
  changing anything in there run `npm run check`, `npm test` and
  `npm run build` from `web/`. The tests are jsdom plus `node:test`, and
  most of them are injection cases against `web/src/markdown.ts`, which
  renders model output, and against `web/src/streamed-message.ts`, which
  is the second path onto it now that answers stream. That renderer builds DOM nodes and must never
  assemble an HTML string: everything it puts on the page goes through
  `textContent`, because the text it is rendering came from a model that
  has been reading tool results and web pages. Reach for a markdown
  library and you have reached for `innerHTML`.
- `cargo test --workspace` does not exercise the `research` feature
  (validate, investigate, co-write, deliver). Run
  `cargo test -p zorp-agent --features research` explicitly whenever
  research-feature code changes. CI covers it nightly and on pull
  requests that touch the research stack, but that is a backstop, not a
  substitute for running it locally.
- The LanceDB vector library is behind a non-default `library` feature
  on both `zorp-track` and `zorp-agent`. `research` does not enable it.
  Leave it off unless you are working on retrieval; it pulls in the
  whole arrow tree.
- `Cargo.lock` is committed and CI builds `--locked`. Shared dependency
  versions live in `[workspace.dependencies]` in the root `Cargo.toml`,
  not in the member manifests. MSRV is 1.82.

## Writing style

Prose in this repo (README, docs, commit messages, comments) should read
as plainly and humanly as possible. No em dashes or en dashes as
punctuation; use a period, comma, colon, or a plain hyphenated compound
word instead. Prefer short, direct sentences over stacked clauses.

## Status

Early/pre-alpha. The base harness and the research foundation
(`zorp-track`) are built and tested. All four capabilities, validate,
investigate, co-write, and deliver, are built and tested.
