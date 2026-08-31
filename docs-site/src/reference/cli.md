# CLI

Three binaries ship prebuilt: `zorp`, `zorp-agent`, and `zorp-web`.
Each answers `--help` with the full, current flag list; this page is the
map, not the territory.

## zorp

The core transport. One prompt in, one answer out, against any
OpenAI-compatible endpoint.

```bash
zorp "Summarize the second law of thermodynamics in one sentence."
```

## zorp-agent

The full agent: tools, reasoning, verification, sessions, MCP.

```bash
zorp-agent "<task>"                 # run a task
zorp-agent resume                   # continue the previous session
zorp-agent --yes "<task>"           # pre-approve tool prompts
zorp-agent --mcp "stdio:<name>:<cmd>:<args...>" "<task>"
```

With the `research` feature (source build), the four capabilities are
subcommands:

```bash
zorp-agent validate "<question>"     # needs a search-capable tool
zorp-agent investigate "<track>"     # staged, pre-registered attempts
zorp-agent co-write "<track>"        # draft from the evidence record
zorp-agent deliver "<track>"         # needs a huiban-prefixed tool
```

`co-write`'s critique pass is bounded by `--critique-rounds` (default
2).

MCP servers can also be configured once in `.zorp/mcp.toml` instead of
per run.

## zorp-web

The web UI server.

```bash
zorp-web                             # http://127.0.0.1:7777
zorp-web --bind 0.0.0.0 --token ...  # non-loopback requires a token
```

Optional features (`search`, `recall`, `memory`, `voice`, `research`)
are compile-time flags; see [The web UI](../web-ui.md).
