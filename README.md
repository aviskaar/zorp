<div align="center">

# zorp

### A research agent for scientific discovery.

*Answers are cheap. Evidence is not.*

Investigation is scattered, and the AI version of it is neither grounded
nor validated. zorp turns a question into a pre-registered investigation,
an evidence record, and a report where every claim traces back to it.

<br/>

[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition%202021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-708%20passing-success?style=flat-square)](#development)
[![Status](https://img.shields.io/badge/status-pre--alpha-critical?style=flat-square)](#status--roadmap)
[![Part of Aviskaar](https://img.shields.io/badge/part%20of-Aviskaar-6f42c1?style=flat-square)](https://github.com/aviskaar)

**[zorp.dev](https://zorp.dev)** · [Aviskaar](https://github.com/aviskaar) · [Report an issue](../../issues)

</div>

---

zorp turns an uncertain question into a defensible answer, using
evidence: question, investigation, sources, evidence, conflicting
evidence, reasoning, validation, answer or artifact. That covers a lot
more than academic research: a technical decision (should we migrate off
Kafka), a competitive teardown, an investment thesis, a due-diligence
package, a market question, an engineering tradeoff, or an academic
hypothesis are all the same shape of problem to zorp. It's built by
[Aviskaar](https://github.com/aviskaar), an applied AI research lab.

> **Status: early / pre-alpha.** The base execution harness and the
> shared research foundation (tracks, evidence records, checkpoints) are
> in place and fully tested. All four capabilities built on top,
> validate, investigate, co-write, and deliver, are built and tested.
> See [Status & roadmap](#status--roadmap) below.

## Why zorp

A confident answer is not a defensible one. An LLM will produce a fluent
answer to a hard question in seconds. What it will not do is tell you
whether to believe it, what evidence it weighed, or what it found that
pointed the other way. zorp treats that gap as the actual problem. A
question becomes an investigation, the investigation produces an evidence
record, and the record is what the answer is accountable to.

The core primitive is the Kill Threshold: a number a human supplies that
says, in advance, what would prove the investigation wrong. Before zorp
gathers anything, the hypothesis, the metric, and the threshold are
written to a file, hashed, and committed to git, so a run cannot quietly
rewrite what it set out to test. The agent never proposes the threshold,
and only a human can move it. Every attempt is recorded, not just the one
that worked, and when a run crosses the line the record says why it was
killed.

Most "AI scientist" projects wire a large agent framework directly to
experiment code, which makes the harness and the research logic hard to
separate, test, or reason about independently, and most assume the
deliverable is a finished document an AI wrote end to end. zorp starts
from the opposite end on both counts: a minimal, dependency-light
execution core extended deliberately with the primitives evidence-based
investigation needs, and a human always in the loop as the author of
record for whatever gets produced, a decision memo, a competitive
landscape, a due-diligence package, or a paper. Long-running task loops,
verification gates, session persistence, tool/MCP integration, and the
research foundation (multi-track evidence records with git-backed,
tamper-evident pre-registration) are already built and tested. All four
capabilities on top, each a clearly bounded layer, validate, investigate,
co-write, and deliver, are built and tested; co-write drafts the
artifact from the track's recorded evidence, with a human as author of
record, and deliver matches the finished draft against real venues.
Between those two sits `critique`, a gate rather than a capability: it
audits the draft against the track's own evidence record, flags figures
and claims the record cannot account for, revises within a bound you set,
and writes what it found into the record. The auditing is done in code,
not by asking a model whether it likes its own draft, and the pass cannot
move the Kill Threshold.

## Architecture

```
.
├── src/                 # zorp core crate: model transport, raw primitives (binary: zorp)
├── zorp-agent/          # the agent: tools, reasoning, verification, sessions, MCP, telemetry
├── zorp-mcp/            # MCP client/server integration
├── zorp-track/          # research foundation: tracks, evidence records, pre-registration, checkpoints
├── zorp-eval/           # deterministic evaluation harness
├── zorp-skill/          # Claude Code compatible skill discovery and parsing (no zorp deps)
├── erbga/               # standalone genetic algorithm for graph community detection (no zorp deps)
├── evals/               # eval suites (smoke tests, Terminal-Bench, Harbor adapter)
├── examples/            # usage examples (e.g. OpenTelemetry tracing)
├── docs/
│   ├── paper/           # arXiv writeup (WIP)
│   ├── superpowers/     # zorp's own design specs and plans
│   └── upstream-quecto/ # preserved history of the upstream harness (see Origins)
└── reference/            # gitignored, local-only research material, not distributed
```

## Getting started

Requires a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```bash
git clone https://github.com/aviskaar/zorp.git
cd zorp
cargo build --workspace --exclude zorp-track
```

> `zorp-track` (the research foundation) bundles DuckDB, which compiles
> from source and takes a while on a cold cache. The command above skips
> it, which is enough for the core `zorp` and `zorp-agent` binaries
> below. Drop `--exclude zorp-track` (plain `cargo build --workspace`,
> or `cargo build --workspace --features research` for `zorp-agent`)
> once you need the `validate`/`investigate`/`co-write`/`deliver`
> capabilities, and budget time for that first build. The LanceDB vector
> library is behind a non-default `library` feature, so the Arrow and
> DataFusion tree is not built unless you ask for it.

Run the core transport directly:

```bash
export ZORP_BASE_URL="https://api.openai.com/v1"   # or a local endpoint (Ollama, LM Studio, vLLM)
export ZORP_API_KEY="sk-..."
export ZORP_MODEL="gpt-4o-mini"
# Optional. Seconds of silence to wait for, default 900. Loading a local model
# into memory can take minutes on modest hardware, and that wait happens
# before the first token. On a streamed reply this bounds the silence between
# chunks, not the length of the answer, so a long reply is never cut off and a
# provider that goes quiet stops being waited on. Exceeding it is an error
# that says so, and so is a stream that ends before the provider says it has
# finished.
export ZORP_HTTP_TIMEOUT_SECS=900
# Optional. A provider that answers 429 or 503 has not taken the request, so
# it is sent again: at most this many times in total, adding at most this many
# seconds of waiting. A Retry-After is waited out in full, and without one the
# wait is exponential backoff with jitter. Nothing else is retried, nothing is
# retried once an answer has started arriving, and every retry says so on
# stderr. Set either to 1 and 0 respectively to turn retrying off.
export ZORP_RETRY_ATTEMPTS=4
export ZORP_RETRY_BUDGET_SECS=30
cargo run -- "Summarize the second law of thermodynamics in one sentence."
```

Or the full agent:

```bash
cargo run -p zorp-agent -- "<task>"
```

### Install without a toolchain

```bash
curl -fsSL https://raw.githubusercontent.com/aviskaar/zorp/main/install.sh | bash
```

This downloads prebuilt `zorp`, `zorp-agent` and `zorp-web` binaries for
your platform from the latest release, verifies the published checksum, and
installs them to `~/.local/bin`. The chat UI's static files go to
`~/.local/share/zorp/web`. No Rust and no Node needed. Linux and macOS,
x86_64 and arm64.

If no prebuilt binary fits your platform, the same script falls back to
building from source, which does need a toolchain. `ZORP_INSTALL_FROM_SOURCE=1`
forces that path, and `ZORP_INSTALL_DIR` changes where the binaries land.

Prebuilt binaries carry the default feature set. The four research
capabilities are behind the `research` feature and still need a source
build, because `zorp-track` bundles DuckDB.

Or try it without installing anything:

```bash
docker run --rm -v "$PWD":/work \
  -e ZORP_BASE_URL -e ZORP_MODEL -e ZORP_API_KEY \
  ghcr.io/aviskaar/zorp "<your task>"
```

The image is about 150MB, runs as a non-root user, and mounts your project
at `/work`. `linux/amd64` and `linux/arm64`.

That pull does not work yet. The image is published but the package is
still private, so an anonymous `docker pull` answers `unauthorized`. See
[#30](https://github.com/aviskaar/zorp/issues/30). Until it is flipped,
use the install script above, or build the image yourself with
`docker build -t zorp .`.

### Using validate, investigate, co-write, deliver

Two of the four need an MCP tool connected first (behind `zorp-agent`'s
`research` feature): `validate` needs a search-capable tool, one whose
name carries a search verb (search, fetch, query, browse, find, lookup,
retrieve), to search for evidence before scoring a question; `deliver`
specifically needs a huiban-prefixed tool, to match a draft against real
venues (see the per-capability specs in
[`docs/superpowers/specs/`](docs/superpowers/specs/)). Connect one with
`--mcp`, or configure it once in `.zorp/mcp.toml`:

```bash
# a search server satisfies validate; its tools are named mcp__brave-search__*
cargo run -p zorp-agent --features research -- --yes \
  --mcp "stdio:brave-search:npx:-y:@modelcontextprotocol/server-brave-search" \
  validate "Should we migrate off Kafka to Redpanda?"
```

```toml
# .zorp/mcp.toml
[[server]]
name = "brave-search"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-brave-search"]
trust = "sandbox"
```

Tools show up prefixed `mcp__<server>__<tool>`, and both checks read
that name: a server named `huiban` satisfies `deliver`, and any tool
whose name carries one of the verbs above satisfies `validate`.
Without a matching tool connected, `validate` fails fast with "no
search-capable tool is available" and `deliver` with "no
huiban-prefixed tool is available", rather than running with no
evidence.

A tool that searches your own saved material does not count, even
though it carries a search verb. Scoring a question against notes you
wrote yourself, and calling that a search for evidence, is worse than
refusing to run.

### Memory across sessions, with open-context

[open-context](https://github.com/aviskaar/open-context) is a separate
MIT-licensed tool that keeps a portable store of context and exposes it
over MCP. Connecting it gives the agent memory that outlives a single
session, without zorp taking on a dependency: it is an MCP server like
any other.

Build it once, then point zorp at it:

```toml
# .zorp/mcp.toml
[[server]]
name = "opencontext"
transport = "stdio"
command = "node"
# The path to your checkout. Absolute, because the server is started
# from whatever directory the agent happens to be working in.
args = ["/path/to/open-context/dist/mcp/index.js"]
trust = "sandbox"
```

Eleven tools arrive, `mcp__opencontext__save_context` through
`mcp__opencontext__delete_bubble`. `/tools` in a chat session lists
them.

Two things worth knowing:

- The npm package named `opencontext` is an unrelated project by
  another author. There is no `npx` recipe here on purpose; installing
  by that name gets you someone else's code.
- `mcp__opencontext__search_contexts` deliberately does not satisfy
  `validate`'s search gate, for the reason above.

### Web UI

A chat interface for the agent, with tool activity streamed as it happens and
an approval prompt before anything is written or run. A long run can stand
those prompts down for one chat with auto-approve, which says so in the
toolbar the whole time it is on and still cannot get a denylisted command
past the policy. See [`web/README.md`](web/README.md).

```bash
cargo run -p zorp-web            # http://127.0.0.1:7777
```

Or the whole thing in containers: the UI, the server, and an Ollama
sidecar that serves both the chat model and the embeddings the
conversation search needs.

```bash
ZORP_WEB_TOKEN=$(openssl rand -hex 16) docker compose up --build
# UI on http://localhost:8080, server on http://localhost:7777
docker compose exec ollama ollama pull qwen3:4b
docker compose exec ollama ollama pull qwen3-embedding
```

The server binds loopback by default. Binding anything else requires
`--token` and refuses to start without it, because a reachable `zorp-web`
is agent-driven shell access to whatever the process can see. In the
compose file that mount is `./workspace`, so the agent sees that directory
and nothing else; set `ZORP_WORKSPACE` to point it elsewhere.

The image is built with voice and recall compiled in, so the microphone and
the conversation search work rather than answering 501. See
[`docs/docker.md`](docs/docker.md) for what happens on the first microphone
click, and for why the sidecar shares the server's network namespace.

**Choosing a model.** The gear button in the top bar opens a settings
panel: pick a provider, point it at a base URL, and choose from the models
that endpoint actually lists. Ollama is a preset rather than a special
case, since it serves an OpenAI-compatible `/v1/models`:

```bash
ollama serve
cargo run -p zorp-web    # then pick "Ollama (local)" in the panel
```

A setting saved here beats the matching `ZORP_*` environment variable,
which beats the built-in default, and every field says which of the three
it came from. The API key is the exception to what gets saved: it is held
in memory for the life of the server process and never written to disk.
Set `ZORP_API_KEY` in the environment if you want it to survive a restart.

**Naming a conversation.** The sidebar used to show the first message you
typed, cut off wherever the row ran out, which is a wall of "hello". Once a
session has a question and an answer in it, the model you are already using
is asked for a short name for it, once, and the sidebar updates in place.
Set `ZORP_SESSION_TITLES=0` to turn it off and get the first message back.

A title is a label and nothing else. It is stored in its own column, and
the conversation search index and the memory block both keep reading the
verbatim first message, because a sentence a model wrote must not become
something a later turn is told to cite. Everything the model says is
clamped in code on the way to the column, to one short line, and a call
that fails or declines leaves the first message showing.

**Speaking into the composer.** Voice input is opt-in. It records in the
browser, asks a local Qwen3-ASR model for a transcript, and puts the text in
the composer for you to read and edit. It never sends the message for you.
Qwen3-ASR detects the language, so there is no English setting to choose.

Start zorp-web with voice enabled:

```bash
cargo run -p zorp-web --features voice
```

One microphone click starts readiness and asks for browser permission at the
same time. Recording begins as soon as permission is granted, and the composer
draws a live level meter from your microphone so you can see it listening while
setup is still running. If no runtime is
available, zorp creates a versioned virtual environment below the platform's
local data directory and tries `qwen-asr[vllm]==0.0.6`. When pip cannot resolve
that extra, zorp recreates only its marked environment with
`qwen-asr==0.0.6` and starts its embedded Transformers server. The page reports
the real create, install, download, load, and ready stages without inventing a
percentage. If recording finishes first, it waits for readiness before sending
the audio to the checked loopback endpoint.

Set `ZORP_VOICE_AUTOSTART=0` to disable every install and spawn step. In that
compatibility mode the status API still reports the old operator start command,
but the browser never renders shell text. Setup also refuses to run as root.
Transcription uses the existing OpenAI-compatible
`POST /v1/chat/completions` audio request.

The defaults are `http://127.0.0.1:8000` and
`Qwen/Qwen3-ASR-0.6B`. Override them with `ZORP_VOICE_URL` and
`ZORP_VOICE_MODEL`. A URL override still has to be a loopback address or
`localhost`. Recorded voice goes to that checked local address or nowhere.
The client pins its resolver to the checked host and port, refuses redirects,
and ignores proxy environment variables. There is no cloud ASR fallback.
For an HTTPS or path-prefixed URL, put `qwen-asr-serve` behind your own
loopback proxy. Automatic setup cannot bind that endpoint and leaves it to the
operator.

Design:
[`docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`](docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md).

**Watching the answer arrive.** Answers stream. Text appears as the model
produces it rather than after it finishes, which is the difference between
a spinner and a page on a local 27B model. Reasoning is filtered out on
the way: a model that thinks in `<think>` tags has that thinking recorded
and not shown, the same as in the terminal. Providers that cannot stream,
which includes Anthropic today, still answer exactly as they did before.

**Reading what a run produced.** The Files button opens a pane listing the
files in the directory the server was started in, and renders them. It is
read-only. Paths are resolved against that directory and refused if they
land outside it, and only an allowlist of extensions is served at all, so
this is a window on the workspace rather than a file server.

| Format | Shown as |
|---|---|
| `.md`, `.markdown` | Rendered markdown |
| `.txt`, `.json`, `.csv` | Plain text |
| `.docx`, `.odt` | Extracted to markdown: headings, paragraphs, lists, tables |
| `.xlsx` | One markdown table per sheet |
| `.pptx` | One heading per slide, plus that slide's text |
| `.pdf` | Its text, extracted to markdown |
| `.png`, `.jpg`, `.gif`, `.webp` | Inline image |
| `.svg`, `.html` | Inside a sandboxed iframe |

The office formats and PDFs are read on the server and rendered by the same
markdown renderer the chat uses. The reading is deliberately plain: text
structure comes across, and images, fonts, colours and page layout do not.
It is for reading what a run produced, not for rendering a document. A PDF
gives up more than the rest, because it records where each glyph was drawn
rather than what the document said, so what comes back is the words and the
breaks between them and no headings at all. A scanned PDF holds pictures of
words and no words, and the pane says so instead of showing nothing.

`.svg` and `.html` are the two that can execute. They load into the pane's
iframe by URL and never into the page, because every served file carries
`X-Content-Type-Options: nosniff` and a bare
`Content-Security-Policy: sandbox`. That is a unique origin with scripting
off, so script inside one of these neither runs nor reaches the page that
framed it.

**Noticing what a run produced.** The pane no longer waits to be asked. The
browser takes a snapshot of the file listing when a turn starts and compares
it afterwards, so anything the run wrote or rewrote gets marked. With the
pane open the newest one opens in it; with the pane closed the Files button
gets a count, and nothing appears over what you are reading. This works by
diffing the directory rather than by reading tool output, so a PDF that
pandoc wrote under `run_command` is caught exactly like one `write_file`
wrote.

Design and plan:
[`docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`](docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md),
[`docs/superpowers/specs/2026-08-17-artifact-pane-design.md`](docs/superpowers/specs/2026-08-17-artifact-pane-design.md).

### Searching your own conversations, on this machine

The browser sidebar can search everything you have ever asked zorp, by
meaning rather than by spelling, behind the `recall` feature:

```bash
ollama pull nomic-embed-text
cargo run -p zorp-web --features recall
```

The server indexes existing conversations after startup, checks them again
every five minutes, and indexes an active conversation after each turn. It
asks the local model for one vector per changed message and writes the vectors
to `recall.db` next to the session store. An unchanged conversation makes no
embedding call. `ZORP_RECALL_SWEEP_SECS` changes the full-store interval, and
0 disables startup and periodic sweeps.

**Conversation text goes to a loopback address or it goes nowhere.** There
is no remote embedding provider, no flag that adds one, and no fallback
when the local model is missing: if nothing answers on `127.0.0.1`, the
search box says so and searches nothing. This corpus is your whole history
with an agent that has been reading your files, and a feature that stayed
working by posting it to an API would be worse than one that stops.

Four things hold that up, and they are layered because any one of them
could be wrong. The endpoint has to be a loopback literal or `localhost`,
and it has to still resolve to loopback. The addresses it resolved to are
the only ones the HTTP client can reach, through a resolver that performs
no lookup of its own. Redirects are refused rather than followed. Proxy
detection from the environment is switched off, so `HTTP_PROXY` cannot
route the text through somebody else's server.

`ZORP_EMBED_URL` and `ZORP_EMBED_MODEL` override the endpoint and the
model, and `ZORP_RECALL_DB` overrides where the index goes. Naming a remote
host in `ZORP_EMBED_URL` does not get you a remote embedder; it gets you a
refusal that names the host.

### Remembering earlier conversations inside a new one

The `memory` feature turns the same index into something a turn can read,
so a fact from a thread you finished in March can be recalled in a thread
you started today:

```bash
cargo run -p zorp-web --features memory
```

Every finished turn indexes its own session in the background, and the
periodic sweep catches anything a failed feed missed. Tick **Recall earlier
conversations for this message** next to the composer and the server embeds
what you typed, finds the closest handful of messages, and quotes them into
the transcript the model reads. Above the answer you get a card listing
exactly what was recalled: the conversation, the date, and whether each line
was written by you or by the assistant.

Three things about it are deliberate.

**The box is unticked on every message.** Retrieval is not a mode you leave
on. It spends context, and it puts text from old conversations in front of
the model, so it is a decision you make per message and can see the result
of. The model cannot ask for a recall on its own; there is no tool for it.

**A memory is a quotation, never a summary.** Nothing reads your history
and writes down what it learned. There is no fact table, no profile, and no
stored sentence a model composed about your past, because that is the shape
in which an agent's guesses turn into its own evidence. What gets recalled
is a message somebody actually sent, with the conversation, the position,
the author and the date attached. Half of any conversation was written by
an assistant, and those lines are labelled as a model's earlier output
rather than presented as fact.

**Recalled text is data.** It arrives inside a fence whose marker is minted
for that one turn, so a payload sitting in an old conversation cannot close
the quotation and start giving orders, and it arrives under the same
sentence a skill body gets: it cannot grant a tool, widen an approval, or
bypass the command denylist. It is a `user` message and never the system
prompt, and it is never written back into your conversation store, which is
what stops the recalled block being re-embedded and recalled again.

### Web search without an MCP server

`validate` also accepts a built-in `web_search` tool, behind the
`search` feature, so it can run with no MCP server at all:

```bash
export ZORP_TAVILY_API_KEY="tvly-..."
cargo run -p zorp-agent --features research,search -- --yes \
  validate "Should we migrate off Kafka to Redpanda?"
```

`search` is deliberately not part of `research`. It is the only built-in
that sends anything over the network, so it is opted into on its own.
The tool asks for approval like an MCP tool does, since a search sends
your question to a third party, and `--yes` answers that ask. A project
flavor can withhold it entirely by leaving `web_search` out of
`[tools] enabled`.

The browser gets the same tool the same way, from `zorp-web`'s own opt-in
`search` feature:

```bash
export ZORP_TAVILY_API_KEY="tvly-..."
cargo run -p zorp-web --features search
```

Off by default there too, for the same reason: starting a local web UI
should not acquire an egress path by side effect. A pill in the topbar
says when the tool is really there, and it is the server that decides
that, not the page. `GET /api/capabilities` reports it, and the answer
covers all three conditions: the feature, the policy, and the key.

Tavily is the first provider behind a small `SearchProvider` trait in the
`zorp-search` crate; the API key is read from the environment and never
from a manifest. See
[`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`](docs/superpowers/specs/2026-08-16-tavily-web-search-design.md).

### Skills

zorp reads skills in Claude Code's format, so skills you already have
work here without being ported. A skill is a directory holding a
`SKILL.md`: YAML frontmatter with a `name` and a `description`, then a
markdown body of instructions.

```
~/.claude/skills/code-review/SKILL.md     # yours, everywhere
<repo>/.claude/skills/code-review/SKILL.md # this project's, wins on a name clash
$ZORP_SKILLS_DIR/code-review/SKILL.md      # explicit for this run, wins over both
```

```markdown
---
name: code-review
description: Review a diff for correctness bugs. Use when asked to review changes.
---

Read the diff first, then the surrounding code. Report findings by
severity, and say when you are unsure.
```

The model sees only the names and descriptions, as one `skill` tool whose
description is the index. It loads a body by calling that tool, and the
body arrives as instructions for that turn. The two levels are the point:
descriptions are cheap enough to always carry, bodies are not.

Skills add guidance, never permissions. A skill cannot enable a tool,
loosen an approval preset, or reach past the `run_command` denylist, and
the `allowed-tools` field some skills carry is read, reported, and
ignored. A skill body is a markdown file that can arrive with a `git
clone`, and it is treated that way: names are single path components and
never joined onto a path, a `SKILL.md` that resolves outside its own
directory is skipped, files over 64 KiB are skipped, and a malformed one
is skipped with a message naming the file while its siblings still load.
A project flavor can withhold skills entirely by leaving `skill` out of
`[tools] enabled`.

Skills are not capsules. A capsule is something you load with `/load` to
put the whole session in a mode, and it stays in the system prompt. A
skill is something the model reaches for mid task and uses for that turn.

## Development

```bash
cargo build --workspace --exclude zorp-track   # fast path, see note above
cargo test --workspace --exclude zorp-track    # matches CI; see CONTRIBUTING.md for full coverage
cargo run -p zorp-eval -- --help               # evaluation harness
```

Working in this repo? Read [`CLAUDE.md`](CLAUDE.md) and [`AGENTS.md`](AGENTS.md)
first. They cover the inherited vs. zorp-specific code boundary, where
design specs live, and repo conventions.

## Status & roadmap

- [x] Base execution harness (forked from quecto, renamed, fully tested)
- [x] Research foundation (`zorp-track`: multi-track evidence records, git-backed pre-registration, checkpoints, DuckDB + LanceDB)
- [x] **validate**: is this question worth investigating (novelty and feasibility check)
- [x] **investigate**: gather evidence through staged, pre-registered attempts, every attempt recorded
- [x] **co-write**: zorp drafts the artifact, a human is always the author of record
- [x] **deliver**: match a finished draft against real academic venues (conferences and journals, via live huiban search), writing a ranked shortlist for a human to review
- [ ] A published investigation trace, start to finish
- [ ] A grounded-vs-baseline evaluation
- [ ] A systems paper about zorp itself, submitted to arXiv

## Origins

zorp's execution layer started as a fork of
[quecto](https://github.com/adityak74/quecto), a minimal, vendor-neutral
harness for LLM agents (MIT licensed). See [`NOTICE.md`](NOTICE.md) for
full attribution. We modify and extend it directly rather than depending
on it as an external crate, since zorp's needs (long-running research
loops, experiment tracking, paper synthesis) diverge substantially from a
general agent harness. Crates and binaries have been renamed from
`quecto-*` to `zorp-*`. [`docs/UPSTREAM_QUECTO_README.md`](docs/UPSTREAM_QUECTO_README.md)
and [`docs/upstream-quecto/`](docs/upstream-quecto/) preserve the original
project's documentation and design history for reference.

## Contributing

Contributions are welcome. zorp is early and still moving fast, so it's
worth opening an issue to discuss larger changes before sending a PR.
See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, testing, and PR
guidelines, and [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) for community
expectations.

## License

MIT. See [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md) for third-party
attribution.
