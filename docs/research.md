# Research capabilities

How to run validate, investigate, co-write and deliver, and how to connect the tools they need.

## Using validate, investigate, co-write, deliver

Two of the four need an MCP tool connected first (behind `zorp-agent`'s
`research` feature): `validate` needs a search-capable tool, one whose
name carries a search verb (search, fetch, query, browse, find, lookup,
retrieve), to search for evidence before scoring a question; `deliver`
specifically needs a huiban-prefixed tool, to match a draft against real
venues (see the per-capability specs in
[`docs/superpowers/specs/`](superpowers/specs/)). Connect one with
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

## Memory across sessions, with open-context

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

## Web search without an MCP server

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
[`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`](superpowers/specs/2026-08-16-tavily-web-search-design.md).
