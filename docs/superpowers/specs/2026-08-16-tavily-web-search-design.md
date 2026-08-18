# Design: native web search, with Tavily as the first provider

**Date:** 2026-08-16
**Status:** approved, being built
**Author of record:** human, drafted with Claude Opus 5

## The problem

`validate` cannot run without a search tool, and today the only way to
give it one is an MCP server:

```
zorp-agent: no search-capable tool is available; configure an MCP search
server (--mcp or .zorp/mcp.toml)
```

That is a real barrier. It was the first thing a new user hit in the
first-run walkthrough (PR #2), and the first UAT had to run `validate`
against a stub MCP server because standing up a real one is a separate
project: install node, pick a server, find its API key, write
`.zorp/mcp.toml`. zorp's headline capability does not work out of the
box.

Tavily is a search API built for agents: one HTTP endpoint, one API key,
results already extracted rather than raw HTML. It removes the whole
setup chain.

## What this is not

This is not "add Tavily to zorp". zorp's README sells a
"minimal, vendor-neutral harness", and hardcoding one vendor into the
agent contradicts that. What gets added is a **search capability with a
provider interface**, and Tavily is the first thing plugged into it.

## Decisions

### D1. A new crate, `zorp-search`, not a built-in under `zorp-agent/src/tools/`

`AGENTS.md` and `CLAUDE.md` are explicit: `zorp-agent/` is inherited
harness code, and new zorp-specific capability belongs in new crates or
clearly named modules. Search is new capability, so it gets
`zorp-search`, a sixth workspace member with no dependency on
`zorp-agent`.

The crate holds the provider trait, the Tavily implementation, the
request and response types, and their tests. It knows nothing about
agents, tools, or approval.

### D2. A provider trait, with Tavily behind it

```rust
pub trait SearchProvider {
    fn name(&self) -> &str;
    fn search(&self, query: &Query) -> Result<Vec<SearchResult>, SearchError>;
}
```

A `SearchResult` carries `title`, `url`, `snippet`, and an optional
`score`. That shape is the intersection of what Tavily, Brave, and Exa
return, so a second provider needs no changes above the trait.

Tavily specifics (`search_depth`, `include_answer`, topic filters) live
in `TavilyProvider`, not in the trait.

### D3. Off by default, behind a `search` feature

Every other optional weight in this workspace is feature-gated:
`research`, `library`, `mcp`, `otel`. Search is the first built-in that
sends anything over the network, which is a bigger deal than a build-time
cost, so it does not arrive silently in a default build.

`research` does **not** enable `search`. Someone running `investigate`
locally should not acquire an internet egress path by side effect. They
opt in with `--features research,search`.

### D4. The tool asks before it runs, like MCP tools do

`Policy::decide` currently maps `mcp__*` to `Decision::Ask` and denies
unknown tool names outright. The new tool is named `web_search` and maps
to `Ask` as well.

This matters more than it looks. A search sends the user's question, which
in this product is a research hypothesis and may be confidential, to a
third party. Allowing that silently because it is "just a read" would be
wrong. Under `--yes` it proceeds, consistent with every other gated
operation, and the hard denylist is not involved because no shell command
is being run.

`web_search` is also an ordinary entry in the `[tools] enabled`
allow-list, so a project flavor can withhold it.

### D5. `validate`'s gate learns about native providers

`name_can_search` requires an `mcp__` prefix today:

```rust
let Some(rest) = name.strip_prefix("mcp__") else { return false };
```

A native `web_search` would be invisible to `validate`, which defeats the
purpose. The predicate is widened to accept `web_search` by exact name,
keeping the existing MCP verb matching untouched. `deliver`'s
huiban-prefix gate is not touched: venue matching is a different job and
Tavily is not a venue database.

### D6. The API key comes from the environment only

`ZORP_TAVILY_API_KEY`, read at provider construction. Never from a flavor
manifest: `Flavor` already refuses an `api_key` field via
`deny_unknown_fields`, and this follows that rule rather than inventing an
exception.

A missing key names the variable at startup and skips registering the
tool, rather than failing the process. Amended during implementation: an
earlier draft called for a hard startup error, which would have meant a
binary built with `--features search` could not answer "what is 2+2"
without a Tavily account. Warning and continuing matches how an
unreachable MCP server is already handled, and the capabilities that
genuinely need search, `validate` above all, still fail closed through
their own gate.

The key is never logged, never written to the trace, and never included
in a tool result. `TavilyProvider`'s `Debug` is written by hand for that
reason, and any provider message built from response text is redacted,
because a rejected-key response can echo the key back.

### D7. Failure is reported, not swallowed

A provider error (bad key, rate limit, network failure, malformed
response) becomes a `ToolError` whose message names the provider and the
condition. The agent sees it as a failed tool call and can react. zorp
does not substitute empty results for a failed search, because an empty
result set and a failed request mean different things to a `validate`
novelty score, and conflating them would put a wrong number into an
evidence record.

This is the same principle as the 2026-08-14 decision that measurement
code fails loudly instead of guessing.

## Shape

```
zorp-search/                     new crate, no zorp-agent dependency
├── src/lib.rs                   SearchProvider, Query, SearchResult, SearchError
├── src/tavily.rs                TavilyProvider: request build, response parse
└── tests/                       stub-HTTP tests, no network, no key

zorp-agent/src/search_tool.rs    thin Tool adapter, feature = "search"
                                 (clearly named, not folded into tools/)
```

Wiring:

- `zorp-agent/Cargo.toml`: `search = ["dep:zorp-search"]`
- `builtin_tools_filtered`: registers `web_search` when the feature is on
- `Policy::decide`: `"web_search" => Decision::Ask`
- `validate::name_can_search`: accepts `web_search`
- `Cargo.toml` workspace members: `zorp-search`

## Testing

No test hits the network or needs a key. The workspace already has the
pattern: `zorp-agent/tests/common/mod.rs` serves scripted HTTP responses
on a local socket, which is how the model client is tested. `zorp-search`
gets the same treatment, with the base URL injectable so tests point it
at `127.0.0.1`.

`ZORP_TAVILY_BASE_URL` overrides the endpoint. Added during
implementation: without it only the crate's own tests could avoid the
network, and every CLI-level check (approval gating, the validate path,
failure handling) would have to spend real API quota against the live
service. It also serves anyone behind a proxy or a self-hosted gateway.
Unset means the real API.

Covered: a well-formed response parses; a response missing `results`
errors rather than returning empty; a non-200 errors with the status
named; a missing key errors at construction naming `ZORP_TAVILY_API_KEY`;
the key appears in the request header and in no output.

Acceptance against the real API is a UAT, run once a key exists, and is
specified separately in `docs/uat/UAT-tavily-plan.md`.

## What this rules out

- **Bundling an MCP server.** Tavily publishes one, and it stays a
  perfectly good option for anyone who prefers it. This is about the
  default path working without node.
- **Making search implicit.** No automatic searching on the model's
  behalf outside the tool call. The agent asks, the policy decides, the
  user can see it in the activity line.
- **A cache.** Deliberately deferred. A cache changes what "the evidence
  said" means across a re-run, and that interacts with pre-registration
  in ways that need their own decision.
