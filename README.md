<div align="center">

# zorp

### A research agent for scientific discovery.

*Forming hypotheses, running experiments, evaluating results, and writing up findings, autonomously.*

<br/>

[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition%202021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-462%20passing-success?style=flat-square)](#development)
[![Status](https://img.shields.io/badge/status-pre--alpha-critical?style=flat-square)](#status--roadmap)
[![Part of Aviskaar](https://img.shields.io/badge/part%20of-Aviskaar-6f42c1?style=flat-square)](https://github.com/aviskaar)

**[zorp.dev](https://zorp.dev)** · [Aviskaar](https://github.com/aviskaar) · [Report an issue](../../issues)

</div>

---

zorp is an agent harness aimed at autonomous scientific research. It forms
hypotheses, runs experiments, evaluates results, and writes up findings,
with the goal of eventually publishing its own output as a paper. It's
built by [Aviskaar](https://github.com/aviskaar), an applied AI research
lab.

> **Status: early / pre-alpha.** The base execution harness is in place
> and fully tested. The research-agent capabilities (experiment tree
> search, autonomous hypothesis-to-paper loop, etc.) are still being
> designed. See [Status & roadmap](#status--roadmap) below.

## Why zorp

Most "AI scientist" projects wire a large agent framework directly to
experiment code, which makes the harness and the research logic hard to
separate, test, or reason about independently. zorp starts from the
opposite end: a minimal, dependency-light execution core, extended
deliberately with the primitives autonomous research needs. Long-running
task loops, verification gates, session persistence, and tool/MCP
integration are already there, with experiment tracking and paper
synthesis coming soon, each added as a clearly bounded layer on top of a
harness that stays legible on its own.

## Architecture

```
.
├── src/                 # zorp core crate: model transport, raw primitives (binary: zorp)
├── zorp-agent/          # the agent: tools, reasoning, verification, sessions, MCP, telemetry
├── zorp-mcp/            # MCP client/server integration
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
- [ ] Idea validation (literature search, novelty and feasibility check)
- [ ] Experiment orchestration (staged, sandboxed, every attempt recorded)
- [ ] Collaborative paper writing (zorp drafts, a human authors and finalizes)
- [ ] Venue matching (conference and journal fit, given a finished paper)
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
