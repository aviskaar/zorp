<div align="center">

# zorp

### A research agent for scientific discovery.

*Answers are cheap. Evidence is not.*

Investigation is scattered, and the AI version of it is neither grounded
nor validated. zorp turns a question into a pre-registered investigation,
an evidence record, and a report where every claim traces back to it.

<br/>

[![CI](https://github.com/aviskaar/zorp/actions/workflows/ci.yml/badge.svg)](https://github.com/aviskaar/zorp/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition%202021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Status](https://img.shields.io/badge/status-pre--alpha-critical?style=flat-square)](#status-and-roadmap)
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
> See [Status and roadmap](#status-and-roadmap) below.

## Contents

- [Why zorp](#why-zorp)
- [Quick start](#quick-start)
- [What you can do with it](#what-you-can-do-with-it)
- [Repository layout](#repository-layout)
- [Development](#development)
- [Status and roadmap](#status-and-roadmap)
- [Origins](#origins)
- [Contributing](#contributing)
- [License](#license)

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

## Quick start

Install prebuilt `zorp`, `zorp-agent` and `zorp-web` binaries (Linux and
macOS, x86_64 and arm64):

```bash
curl -fsSL https://raw.githubusercontent.com/aviskaar/zorp/main/install.sh | bash
```

Point zorp at a model. Any OpenAI-compatible endpoint works, including a
local one (Ollama, LM Studio, vLLM):

```bash
export ZORP_BASE_URL="https://api.openai.com/v1"
export ZORP_API_KEY="sk-..."
export ZORP_MODEL="gpt-4o-mini"
```

Then use it from the terminal or the browser:

```bash
zorp-agent "<task>"                      # the agent, in the terminal
zorp-web --workspace ~/research          # the chat UI on http://127.0.0.1:7777
```

On a Mac you can instead download `Zorp_<version>_universal.dmg` from the
latest GitHub release and drag `Zorp.app` to `/Applications`.

To build from source, run from Docker, or tune timeouts and retries, see
[Getting started](docs/getting-started.md). The four research capabilities
need a source build with the `research` feature.

## What you can do with it

| Guide | What it covers |
|---|---|
| [Getting started](docs/getting-started.md) | Building from source, the install script, `Zorp.app`, Docker, timeout and retry settings |
| [Research capabilities](docs/research.md) | Running validate, investigate, co-write and deliver; connecting MCP search tools; memory with open-context; built-in web search |
| [Web UI](docs/web-ui.md) | The browser chat: workspaces, choosing a model, voice input, streaming answers, the Files pane, running it in containers |
| [Recall and memory](docs/recall-and-memory.md) | Searching your own conversations, and quoting earlier ones into a new turn, on this machine only |
| [Skills](docs/skills.md) | Using Claude Code compatible skills, and what a skill can and cannot do |
| [Docker](docs/docker.md) | The container image and the compose stack in detail |
| [Benchmarks](docs/benchmarks.md) | Running zorp-agent against Terminal-Bench |
| [Decision log](docs/DECISIONS.md) | Why things are the way they are |

## Repository layout

```
.
├── src/                 # zorp core crate: model transport, raw primitives (binary: zorp)
├── zorp-agent/          # the agent: tools, reasoning, verification, sessions, MCP, telemetry
├── zorp-web/            # the browser chat server (binary: zorp-web)
├── web/                 # the browser UI, TypeScript
├── zorp-desktop/        # Zorp.app, the native Mac app (outside the Cargo workspace)
├── zorp-mcp/            # MCP client/server integration
├── zorp-track/          # research foundation: tracks, evidence records, pre-registration, checkpoints
├── zorp-eval/           # evaluation: compat, the gating harness suite, and bench
├── zorp-stub/           # scripted model provider used by tests and the harness suite
├── zorp-search/         # web search providers (Tavily, SearXNG)
├── zorp-skill/          # Claude Code compatible skill discovery and parsing
├── zorp-recall/         # local conversation search
├── zorp-voice/          # local voice transcription client
├── zorp-train/          # developer mode: local pretraining
├── erbga/               # standalone genetic algorithm for graph community detection
├── evals/               # eval suites (smoke tests, Terminal-Bench, Harbor adapter)
├── examples/            # usage examples (e.g. OpenTelemetry tracing)
├── docs-site/           # the documentation site
├── docs/
│   ├── paper/           # arXiv writeup (WIP)
│   ├── superpowers/     # zorp's own design specs and plans
│   └── upstream-quecto/ # preserved history of the upstream harness (see Origins)
└── reference/           # gitignored, local-only research material, not distributed
```

## Development

```bash
cargo build --workspace --exclude zorp-track   # fast path, see docs/getting-started.md
cargo test --workspace --exclude zorp-track    # matches CI; see CONTRIBUTING.md for full coverage
cargo run -p zorp-eval -- --help               # evaluation harness
```

To run zorp-agent against Terminal-Bench, see
[`docs/benchmarks.md`](docs/benchmarks.md).

Working in this repo? Read [`CLAUDE.md`](CLAUDE.md) and [`AGENTS.md`](AGENTS.md)
first. They cover the inherited vs. zorp-specific code boundary, where
design specs live, and repo conventions.

## Status and roadmap

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
