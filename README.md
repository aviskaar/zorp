<div align="center">

# zorp

### Zorp investigates hard questions and delivers evidence-backed answers.

*LLMs made intelligence cheap. Zorp makes validated intelligence cheap.*

<br/>

[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition%202021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-519%20passing-success?style=flat-square)](#development)
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
> in place and fully tested. Of the four capabilities built on top,
> validate and investigate are built and tested; co-write and deliver
> are still being designed. See [Status & roadmap](#status--roadmap)
> below.

## Why zorp

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
tamper-evident pre-registration) are already built and tested. Of the
four capabilities on top, each a clearly bounded layer, validate and
investigate are built and tested; co-write and deliver are next.

## Architecture

```
.
├── src/                 # zorp core crate: model transport, raw primitives (binary: zorp)
├── zorp-agent/          # the agent: tools, reasoning, verification, sessions, MCP, telemetry
├── zorp-mcp/            # MCP client/server integration
├── zorp-track/          # research foundation: tracks, evidence records, pre-registration, checkpoints
├── zorp-eval/           # deterministic evaluation harness
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
cargo build --workspace
```

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
`zorp-agent` to `~/.local/bin`.

## Development

```bash
cargo build --workspace   # build everything
cargo test --workspace    # 462 tests across all crates
cargo run -p zorp-eval -- --help   # evaluation harness
```

Working in this repo? Read [`CLAUDE.md`](CLAUDE.md) and [`AGENTS.md`](AGENTS.md)
first. They cover the inherited vs. zorp-specific code boundary, where
design specs live, and repo conventions.

## Status & roadmap

- [x] Base execution harness (forked from quecto, renamed, fully tested)
- [x] Research foundation (`zorp-track`: multi-track evidence records, git-backed pre-registration, checkpoints, DuckDB + LanceDB)
- [x] **validate**: is this question worth investigating (novelty and feasibility check)
- [x] **investigate**: gather evidence through staged, pre-registered attempts, every attempt recorded
- [ ] **co-write**: zorp drafts the artifact, a human is always the author of record
- [ ] **deliver**: get the finished artifact into the right format for its audience (venue matching for a paper, stakeholder-ready memo for a decision)
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

This repo is currently private and under active early-stage development,
so it's not yet set up for external contributions. That will change as
the project matures. Check back, or reach out via
[Aviskaar](https://github.com/aviskaar) in the meantime.

## License

MIT. See [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md) for third-party
attribution.
