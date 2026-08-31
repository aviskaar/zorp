# Getting started

## Install without a toolchain

```bash
curl -fsSL https://raw.githubusercontent.com/aviskaar/zorp/main/install.sh | bash
```

This downloads prebuilt `zorp`, `zorp-agent`, and `zorp-web` binaries for
your platform, verifies the published checksum, and installs them to
`~/.local/bin`. Linux and macOS, x86_64 and arm64. No Rust and no Node
needed.

Prebuilt binaries carry the default feature set. The four research
capabilities are behind the `research` feature and need a source build,
because `zorp-track` bundles DuckDB.

## Build from source

Requires a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```bash
git clone https://github.com/aviskaar/zorp.git
cd zorp
cargo build --workspace --exclude zorp-track
```

`zorp-track` bundles DuckDB, which compiles from source and takes a while
on a cold cache. The command above skips it, which is enough for the core
`zorp` and `zorp-agent` binaries. Drop the exclusion (or add
`--features research`) once you need validate, investigate, co-write, or
deliver.

## Point it at a model

zorp talks to any OpenAI-compatible endpoint: a hosted API, or a local
one (Ollama, LM Studio, vLLM).

```bash
export ZORP_BASE_URL="https://api.openai.com/v1"
export ZORP_API_KEY="sk-..."
export ZORP_MODEL="gpt-4o-mini"
```

See [Environment variables](reference/env-vars.md) for timeouts, retries,
and the rest.

## Run it

The core transport, one prompt in, one answer out:

```bash
cargo run -- "Summarize the second law of thermodynamics in one sentence."
```

The full agent, with tools, sessions, and verification:

```bash
cargo run -p zorp-agent -- "<task>"
```

The web UI:

```bash
cargo run -p zorp-web    # http://127.0.0.1:7777
```

See [The web UI](web-ui.md) for the settings panel, streaming, and the
optional features.

## Run a research capability

`validate` needs a search-capable tool connected, and `deliver` needs a
huiban-prefixed one. Connect either over MCP with `--mcp`, or configure
it once in `.zorp/mcp.toml`:

```bash
cargo run -p zorp-agent --features research -- --yes \
  --mcp "stdio:brave-search:npx:-y:@modelcontextprotocol/server-brave-search" \
  validate "Should we migrate off Kafka to Redpanda?"
```

See [The four capabilities](concepts/capabilities.md) for what each one
does and what it needs.
