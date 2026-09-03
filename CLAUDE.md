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
`zorp-search`, `zorp-skill`, `zorp-voice`, and `erbga` are further workspace members
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
  record nobody has fed. The `anomalies` table has its own producer and
  its own switch: `zorp-agent/src/investigate/gate.rs` runs after an
  attempt, and only when the outcome fell outside its own stated
  interval, repeats the attempt and hands the repeats to
  `Store::rerun_gate`. It is off unless `ZORP_RERUN_GATE` is set, because
  each repeat is a whole agent run, and it truncates the transcript back
  to the seed before each one so a repeat cannot read the original's
  answer. It runs before the kill threshold, since a breach kills the
  track and repeats would then be impossible. See `docs/DECISIONS.md`
  (2026-09-01) before changing any of it. Two rules hold the whole thing
  up and
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
  but a caller still has to choose to ask for it. The hypothesis-search
  admission gate is four numbers read from the ledger by
  `zorp-track/src/admission.rs`: reproduced admissions, varying condition
  keys, at least one re-run rejection, and a calibration Go. All four must
  hold. `gate_status` prints them, and crossing is still a person's
  decision. See `docs/DECISIONS.md` (2026-09-02). If the stated intervals
  do not have real coverage, the right move is to stop and not build the
  ledger, and that is a decision a person makes. A band with
  too few forecasts to judge is its own no-go and never a miss: a gap
  computed over three rows is arithmetic about three rows, and reporting
  it as a demonstrated miss makes it look exactly like one. A band is a
  bin of adjacent stated confidences, not one per distinct confidence,
  and it is sized by `required_band_n` of its own mean so that free-form
  confidences do not shatter a run into pieces none of which can be
  judged. `bin_boundaries` is handed the stated confidences and never
  the outcomes, because a boundary chosen with the hits in view is
  fitted to the answer, and every scored row lands in exactly one bin
  because dropping the sparse ones biases the curve in silence. Pooling
  averages, which is why `CalibrationBand::parts` puts every stated
  confidence that went into a bin back on the page. See
  `docs/superpowers/specs/2026-08-19-anomaly-driven-inquiry-design.md`
  and `docs/DECISIONS.md` (2026-08-19, 2026-08-20, 2026-08-22) before
  changing any of it.
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
- The bolt in the composer (once "Zorp mode") is `investigate` attempts
  from the browser, the write-up they produce, and a read of what landed
  in the aryabhatta ledger. It is not a fifth capability and there is no
  aryabhatta engine behind it; `investigate` is the only thing that
  writes to the record. One press runs `ZORP_BOLT_ATTEMPTS` attempts,
  three by default and capped at ten, truncating the transcript back to
  the seed before each one so no attempt can read the previous answer,
  then hands the track to `co_write` and `critique` and reports the
  draft's path for the artifact pane. A track killed by its own
  threshold stops the loop and gets no write-up: that breach is the
  answer, and a document arguing for a hypothesis its own threshold just
  rejected is the one artifact this must never make. See
  `docs/DECISIONS.md` (2026-09-01) before changing any of it. It lives in
  `zorp-web/src/investigate.rs` behind a non-default `research` feature
  on `zorp-web`, mirrors `panel`'s shape at
  `POST /api/sessions/:id/investigate` on the existing event stream, and
  occupies the session exactly as a turn does. The routes are registered
  whatever the feature says and answer 501 without it, so the page can
  say why the button is off. Two rules are not negotiable. A run is
  launched by a person and never by a model, because an attempt writes
  to a pre-registered evidence record; there is no tool that starts one
  and both `agent.rs` and `zorp-web` have tests saying so. And the
  ledger reader names no model-authored text column, so
  `expectations.assumptions` is not in what it returns. Checkpoints are
  auto-approved from the browser because there is no terminal to ask,
  and the pre-registered kill threshold is still enforced in code
  regardless. Forecasting is reported by `GET /api/investigate/status`
  and can never be set from the browser. See `docs/DECISIONS.md`
  (2026-08-21) before changing any of that.
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
  from `ZORP_TAVILY_API_KEY` and never from a flavor manifest. `zorp-web`
  has its own opt-in `search` feature that turns the same built-in on for
  the browser, off by default for the same reason, and it reports whether
  the tool is really there at `GET /api/capabilities`. That answer is
  observed rather than re-derived: `zorp_agent::web_search_availability`
  shares one function with the registration site, and a test pins it to
  `tool_names()`. See `docs/DECISIONS.md` (2026-08-21) before changing
  either.
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
- `zorp-recall/` is zorp's own conversation search: a loopback guard, an
  embedder that talks to a local Ollama, and a SQLite vector index over the
  conversations in `zorp-agent`'s store. Like `zorp-search` and `zorp-skill`
  it depends on no other workspace member. `zorp-web` exposes it behind the
  non-default `recall` feature as three endpoints and a sidebar search box.
  One background worker sweeps at startup and every 300 seconds by default,
  configurable with `ZORP_RECALL_SWEEP_SECS`, where 0 disables automatic
  sweeps. A finished turn queues its session on that same worker. The worker
  serializes every pass, coalesces repeat session notices, runs outside server
  startup and turns, and relies on the existing fingerprint skip. It logs the
  first failure, stays quiet while it persists, and logs recovery. The Index
  button is gone, but `POST /api/recall/index` remains for tests and scripts
  that need to force a pass.
  One rule holds the whole thing up and it is not negotiable: conversation
  text goes to a loopback address or it goes nowhere. There is no remote
  embedding provider, no flag that adds one, and no fallback when the local
  model is missing, because this corpus is a person's whole history with an
  agent that reads their files. Four layers enforce it. The endpoint must
  pass `LoopbackUrl::parse`, which checks the written form and then the
  resolution. `LoopbackResolver` is the only resolver the HTTP client gets,
  it does no lookup, and it answers for one host and port. Redirects are off.
  Proxy-from-env is off. `tests/no_remote.rs` and `tests/no_proxy.rs` pin all
  of it by counting connections to a loopback canary, not by checking for an
  error, because a failed request and a request never made look the same from
  the caller's side. Run `cargo test -p zorp-web -p zorp-recall
  --features zorp-web/recall` whenever any of it changes. See
  `docs/DECISIONS.md` (2026-08-22, 2026-08-24) before changing any of that,
  especially the choice of SQLite over the LanceDB library in `zorp-track`.
- `zorp-voice/` is zorp's own voice transcription client and runtime bootstrap
  for Qwen3-ASR 0.0.6. It depends on no other workspace member. `zorp-web`
  exposes it behind the non-default `voice` feature. The status, readiness, and
  transcription routes exist in every build, and the mutating routes answer
  501 without it. Recorded voice has the same boundary as recall text: the
  endpoint passes written-form and resolution checks, the client gets a
  resolver for that host and port only, redirects are off, and proxy discovery
  is off. Tests count connections to loopback canaries. There is no cloud ASR
  provider or fallback. A person's microphone click starts setup through the
  readiness request while the browser asks for permission. Setup installs
  exactly `qwen-asr[vllm]==0.0.6` or `qwen-asr==0.0.6` in a marked virtual
  environment under the user's local data directory, refuses root, and falls
  back to the embedded Transformers server when pip cannot resolve the vLLM
  extra. The server binds only to loopback and readiness reports real create,
  install, download, load, ready, and error stages. `GET /api/voice/status`
  stays read-only. No command reaches the page.
  `ZORP_VOICE_AUTOSTART=0` disables setup and spawning. HTTPS and path-prefixed
  endpoints still need an operator-managed loopback proxy. No model tool can
  start a runtime or recording. While the microphone is open the composer
  draws a live level meter from the stream, so the page says it is listening
  through a setup that can take minutes. It reads amplitude only, keeps no
  sample and copies the audio nowhere, and a browser with no Web Audio gets an
  inert meter rather than a failed recording. While it is open the microphone
  is a stop button with a pulsing red dot and `aria-pressed`, the status line
  reads "Listening" with a ticking elapsed time, and Escape stops it the way
  the button does. The live transcript is segments: the recorder is stopped
  and restarted at a quiet moment after three seconds, or at eight regardless,
  and each finished segment goes in order, one request at a time, to the same
  `/api/voice/transcribe` route, its text joining a preview line under the
  composer that is one text node set through `textContent`; a failed segment
  leaves `[unclear]` and the recording goes on. On stop the joined text lands
  in the composer as a transcript always did. See `docs/DECISIONS.md`
  (2026-09-03). A transcript is untrusted
  editable composer text. It grants no tool, changes no approval, bypasses no
  denylist, and is never sent automatically. Run `cargo test -p zorp-web
  --features voice` and `cargo test -p zorp-voice` whenever it changes. See
  `docs/DECISIONS.md` (2026-08-24) first.
- `memory` (`zorp-web/src/memory.rs`, non-default `memory` feature, which
  turns on `recall`) is the second way that index gets read: not into the
  sidebar, into a live turn. Every finished turn indexes its own session, so
  every conversation feeds the memory without a button press, and a turn
  that asks for it gets earlier conversations quoted into its transcript.
  Four things are not negotiable. The unit is a verbatim message and there
  is no other kind: no model is asked to read the corpus and write down what
  it learned, so there is no claim table and no stored sentence a model
  composed, because that is the shape in which the agent's guesses become
  its own evidence. An assistant line is model-authored text and is labelled
  as such everywhere it surfaces. Recalled text is data: it sits inside a
  fence whose marker carries a per-turn nonce, under the same boundary
  sentence `zorp-skill` puts under a skill body, in a `user` message and
  never the system prompt, and it grants no tool, loosens no approval and
  bypasses no denylist entry. And the block is appended to the seed, so it
  reaches the model and never the store; persisted, it would be re-embedded
  and recalled next turn, which is the tail-eating this whole design avoids.
  Retrieval is per message and off by default, and the model cannot ask for
  it. Run `cargo test -p zorp-web --features memory` whenever any of it
  changes. See `docs/DECISIONS.md` (2026-08-22) first.
- `title` (`zorp-web/src/title.rs`) is the sidebar's session name: one
  model call per conversation, made after the first turn has both a
  question and an answer, on by default and off with
  `ZORP_SESSION_TITLES=0`. Three things are not negotiable. It writes to
  `sessions.display_title` and never to `sessions.task`, because `task`
  is the verbatim first message and `recall::index_one` reads it into the
  search index while `memory::block` quotes that title into a later turn
  and tells the model to cite it; a generated summary in `task` is the
  agent's own sentence coming back as evidence. Both halves of the
  material handed to the call are untrusted, so they are fenced under a
  boundary sentence with a per-call marker, the same shape `zorp-skill`
  and `memory` use. And whatever comes back is clamped in code on the one
  path to the column: one line, one short noun phrase, control and
  bidirectional characters stripped. Every failure writes nothing and
  leaves the first message showing. The sidebar catches up on the
  existing event stream via a `session_title` frame, and the browser puts
  it on the page through `textContent`. See `docs/DECISIONS.md`
  (2026-08-22) before changing any of it.
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
- `zorp::http_agent` is the one HTTP agent for model traffic and the one
  place timeouts are set: 30 seconds to connect, `ZORP_HTTP_TIMEOUT_SECS`
  to read, defaulting to 900. Anything in the workspace that talks to a
  model endpoint goes through it. It was private once and the streaming
  path reached for `ureq::agent()` instead, which has no timeouts at all,
  so every real model call ran unbounded and one of them sat on an
  established socket for 3 hours 18 minutes. `timeout_read` is per read,
  which on a streamed body makes it an idle timeout: it bounds silence,
  never the length of an answer. Do not add a second variable for the
  streaming path. `zorp-recall`, `zorp-search`, `zorp-mcp` and
  `zorp-web`'s settings probes each build their own agent on purpose and
  each sets its own timeouts; `zorp-recall`'s in particular carries the
  loopback resolver and must not be replaced by this one. See
  `docs/DECISIONS.md` (2026-08-22) before changing any of it.
- An agent loop multiplies that bound and the default is picked with the
  multiplication in mind. One attempt is up to 40 model calls and any one
  of them exceeding the bound kills the attempt, so a per-request stall
  rate of `p` leaves `(1 - p)^40` attempts alive: one request in twenty
  stalling loses seven attempts in eight. At 180 seconds a 300 attempt
  calibration run produced 9 usable forecasts; before any bound existed
  the same corpus produced 76 from 123. Two things follow and neither is
  negotiable. Exceeding the bound is loud: `stream_sse` names the timeout
  and `ZORP_HTTP_TIMEOUT_SECS` whatever ureq called the underlying error,
  because on a chunked body ureq reports a read timeout as "Error while
  decoding chunks" and a log full of those says nothing. And a stream that
  ends before the provider says it has finished, no `[DONE]` and no
  `finish_reason`, is an error and not a short answer, because a truncated
  answer that returns `Ok` is indistinguishable from a model that replied
  badly and that is how a nine hour run was misread twice. A reply with no
  tool calls whose `finish_reason` is `length` is the same thing with the
  provider saying so, and the loop ends it as an error rather than returning
  the cut-off text as an answer; see `docs/DECISIONS.md` (2026-09-03). See
  `docs/DECISIONS.md` (2026-08-23) before changing either.
- A 429 or a 503 is retried and nothing else is. `zorp::send_json` is the
  one place it happens, so the buffered path and `stream_sse` share it;
  the streaming path used to keep its own copy of the core's error
  handling, which is how it went months with no timeout at all. The bound
  is two sided, `ZORP_RETRY_ATTEMPTS` sends in total (4) and
  `ZORP_RETRY_BUDGET_SECS` of added waiting (30), and both numbers are
  picked for the person watching a browser rather than for a batch run. A
  `Retry-After` is waited out in full, with jitter on top and never
  inside, and one that will not fit the budget ends the retrying rather
  than being clamped. Three things are not negotiable. Nothing is retried
  once a response body has started arriving, because those payloads
  already went to the caller and a second send would replay the start of
  one answer over the middle of another; `stream_sse` gets that for free
  by retrying before the body exists and `retry_rate_limit.rs` counts
  connections to prove it. 400, 401 and 404 are never retried, because a
  slow misconfiguration reads like a network problem. And every retry
  says so on stderr, for the same reason the bound above is loud. It came
  from a 250 crate calibration run losing 25 of its first 48 attempts to
  one 429 whose own body said "Please retry shortly". See
  `docs/DECISIONS.md` (2026-08-23) before changing any of it.
- `zorp-agent/src/context_window.rs` is the one place that decides how large
  the context window is, how full it is, and what to drop when it fills.
  Compaction there is deterministic: it elides oldest tool-result bodies, then
  oldest assistant tool-call arguments, never a `command`, and on the seed path
  only drops oldest whole exchanges. A marker copied back as an argument is
  refused before it reaches a tool. No model writes a summary, and nothing in it ever writes to
  the store. The window is unknown unless `ZORP_CONTEXT_TOKENS` says otherwise,
  on purpose: no endpoint can be asked and no default is right for everyone.
  `zorp-web` and the CLI's `resume` both seed a turn through its `plan_seed`,
  which is what gives the browser conversational memory. A provider that
  states its window while refusing a request for its size has said it:
  `stated_window` reads the number out of the error, the agent loop adopts
  it, compacts, and sends the step once more, and a second refusal becomes
  a readable error naming both numbers and `ZORP_CONTEXT_TOKENS`. That is
  one retry and never a loop, and it neither guesses a window nor sends
  `num_ctx`. See
  `docs/DECISIONS.md` (2026-08-19, 2026-09-03) before changing any of that.
- `web/` is TypeScript and no Rust job compiles a line of it. After
  changing anything in there run `npm run check`, `npm test` and
  `npm run build` from `web/`. The tests are jsdom plus `node:test`, and
  most of them are injection cases against `web/src/markdown.ts`, which
  renders model output, and against `web/src/streamed-message.ts`, which
  is the second path onto it now that answers stream. That renderer builds DOM nodes and must never
  assemble an HTML string: everything it puts on the page goes through
  `textContent`, because the text it is rendering came from a model that
  has been reading tool results and web pages. Reach for a markdown
  library and you have reached for `innerHTML`. `web/src/onboarding.ts`
  is the first-run flow and the third path under the same rule: a model id
  and a model name come off a provider's listing, so they land through
  `textContent` too. That flow is a client for the settings endpoints and
  nothing more. It holds no settings of its own, it cannot start a turn, a
  panel or an investigate run, and it calls a model free only when the
  listing stated a price of zero. See `docs/DECISIONS.md` (2026-09-01).
- `cargo test --workspace` does not exercise the `research` feature
  (validate, investigate, co-write, deliver). Run
  `cargo test -p zorp-agent --features research` and
  `cargo test -p zorp-web --features research` explicitly whenever
  research-feature code changes. CI covers both nightly and on pull
  requests that touch the research stack, but that is a backstop, not a
  substitute for running them locally.
- The LanceDB vector library is behind a non-default `library` feature
  on both `zorp-track` and `zorp-agent`. `research` does not enable it.
  Leave it off unless you are working on retrieval; it pulls in the
  whole arrow tree.
- `Cargo.lock` is committed and CI builds `--locked`. Shared dependency
  versions live in `[workspace.dependencies]` in the root `Cargo.toml`,
  not in the member manifests. MSRV is 1.95, declared as `rust-version`
  in the root `Cargo.toml` and pinned by the `msrv` CI job. It was 1.82
  until the dependency tree walked past it, so read the manifest and
  not this line if the two ever disagree again.

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
the library. Run 7 scored 151 forecasts from a crates.io corpus, which
is the observed curve that set the calibration tolerance at 0.10; see
`docs/DECISIONS.md` (2026-08-24). Beyond that it is barely exercised.
