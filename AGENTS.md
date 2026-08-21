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
- `aryabhatta` is zorp's discovery layer, and it lives in `zorp-track`
  as nine modules: `conditions`, `expectations`, `calibration`,
  `detectors`, `partition`, `rerun`, `anomalies`, `families`, and
  `inquiry`. It is record plus readers, not a fifth capability, and it
  ships no CLI command on purpose. `investigate` is what writes to it:
  every attempt records the conditions it ran under, and, when
  `ZORP_FORECAST` is set, asks for a forecast before doing the work and
  records that too. Both happen before the attempt runs, which is not a
  detail: a condition recorded afterwards describes a different run, and
  an expectation recorded afterwards is a postdiction. Forecasting is
  off by default because it costs a model call on every attempt. Leave
  it off and the ledger stays empty, which is the honest state for a
  record nobody has fed. Two rules hold the whole thing up and
  neither is negotiable. Detection is code and the model only
  interprets, the same split `critique` already uses. And no detector,
  and nothing in the search layer, may read a column holding
  model-authored text, or the agent's own speculation becomes tomorrow's
  observation. `expectations` refuses a forecast once its outcome
  exists, which is the one guarantee that stops a prediction being a
  postdiction; it has a mutation test and that test is the point of it.
  `calibration` is a go/no-go for whoever builds on the ledger, and no
  code enforces it. Nothing consults a calibration result before the
  anomaly ledger gets written, so bad coverage will not stop anything on
  its own. `CalibrationReport::verdict` turns a report into a go/no-go,
  but a caller still has to choose to ask for it. If the stated
  intervals do not have real coverage, the right move is to stop and not
  build the ledger, and that is a decision a person makes. See
  `docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-design.md`
  and `docs/DECISIONS.md` (2026-08-19, 2026-08-20) before changing any
  of it.
- `erbga` is wired into `zorp-track` as the large-graph backend of the
  search layer. That reverses part of the 2026-08-15 decision, so read
  the 2026-08-19 entry first. The exact backend is a standing regression
  check on it wherever both can run, and above the crossover a reported
  bundle is a floor on the confounding rather than the whole of it,
  because the search can split a true bundle but never invent one.
- `panel` (`zorp-agent/src/panel/`) is adversarial review: several
  reviewers read one target at once from code-defined lenses, none of
  them sees what the others said, and agreement is counted in code
  afterwards. It is a reader, not a gate, and it is not `critique`;
  critique audits a draft against a track's evidence record and refuses
  if the record moved, this produces opinions and changes nothing. Two
  rules are not negotiable. A reviewer gets strictly less than the panel
  that launched it: a read-only allow-list of tools, so an opinion can
  never edit what it is reviewing. And a panel is launched by a person,
  from the browser, never by a model; there is no `spawn_subagent` tool
  and `agent.rs` has a test saying so. `zorp-web` exposes it at
  `POST /api/sessions/:id/panel` on the existing event stream, and it
  occupies the session exactly as a turn does. See `docs/DECISIONS.md`
  (2026-08-20) before changing any of that.
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
- `erbga/` is a standalone, zero-dependency implementation of published
  prior work (Rao, Janikow, Bhatia, Climer, MWAIS 2018): a genetic
  algorithm for graph community detection, validated against that work's
  four benchmarks. It knows nothing about zorp. The dependency points
  from `zorp-track` to `erbga` and never the other way, as the search
  layer bullet above describes. It came out of a design that was then
  rejected (`docs/DECISIONS.md`, 2026-08-15) and is kept on its own
  terms as a validated artifact. The four benchmarks certify ERBGA on
  graphs and nothing else: a consumer reusing only its
  representation-agnostic scaffolding is running a new algorithm and
  needs its own validation.
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
- `docs/superpowers/specs/` holds the approved designs, one per
  capability. Check the relevant spec before assuming what zorp's
  capabilities are called or what they cover; both have changed at least
  once already. There is no separate architecture index; there was one and
  it drifted, see `docs/DECISIONS.md` (2026-08-20).
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
investigate, co-write, and deliver, are built and tested. aryabhatta,
the discovery layer, is built and tested as nine modules inside
`zorp-track`. It is record plus readers, not a fifth capability, and it
ships no CLI command on purpose. `investigate` records conditions on
every attempt and, behind `ZORP_FORECAST`, a forecast before each one,
so the record now has a producer. Reading it back is still Rust against
the library. It has not been run against real data yet.
