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
[![Tests](https://img.shields.io/badge/tests-605%20passing-success?style=flat-square)](#development)
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

## Architecture

```
.
├── src/                 # zorp core crate: model transport, raw primitives (binary: zorp)
├── zorp-agent/          # the agent: tools, reasoning, verification, sessions, MCP, telemetry
├── zorp-mcp/            # MCP client/server integration
├── zorp-track/          # research foundation: tracks, evidence records, pre-registration, checkpoints
├── zorp-eval/           # deterministic evaluation harness
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
cargo run -- "Summarize the second law of thermodynamics in one sentence."
```

Or the full agent:

```bash
cargo run -p zorp-agent -- "<task>"
```

`./install.sh` builds release binaries and installs `zorp` and
`zorp-agent` to `~/.local/bin`. It builds only those two crates, so it
skips the `zorp-track` cold-build cost noted above.

### Using validate, investigate, co-write, deliver

Two of the four need an MCP tool connected first (behind `zorp-agent`'s
`research` feature): `validate` needs any MCP tool, to search for
evidence before scoring a question; `deliver` specifically needs a
huiban-prefixed tool, to match a draft against real venues (see
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)). Connect one with
`--mcp`, or configure it once in `.zorp/mcp.toml`:

```bash
# any MCP server satisfies validate; here, a real search server
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

Tools show up prefixed `mcp__<server>__<tool>`, so a server named
`huiban` satisfies `deliver`'s check. Without any MCP tool connected,
`validate` fails fast with "no search-capable tool is available" and
`deliver` with "no huiban-prefixed tool is available", rather than
running with no evidence.

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
