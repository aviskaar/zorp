---
status: accepted
date: 2026-08-16
---

# web search is a capability with a provider behind it, not a Tavily integration

**Decision:** search becomes a new workspace crate, `zorp-search`,
holding a `SearchProvider` trait with Tavily as the first
implementation. `zorp-agent` exposes it as a built-in `web_search` tool
behind a non-default `search` feature, gated at `Decision::Ask`, and
`validate`'s search gate accepts it by exact name alongside the existing
`mcp__` verb matching. The API key comes from `ZORP_TAVILY_API_KEY` and
never from a flavor manifest. Design:
`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`.

**Why:** `validate` is zorp's entry point and it could not run without
an MCP server, which means installing node, choosing a server, and
writing `.zorp/mcp.toml` before asking the first question. That barrier
showed up in the first-run walkthrough (#2) and forced the first two
UATs to validate against a stub server. Tavily removes the setup chain:
one endpoint, one key, results already extracted. Putting a trait in
front of it keeps the "vendor-neutral harness" claim honest and lets a
second provider land without touching the agent.

**Why `search` is separate from `research`:** this is the first built-in
tool that sends anything over the network, and the thing it sends is the
user's hypothesis. Running `investigate` locally should not acquire an
egress path by side effect, so the feature is opted into on its own.
For the same reason the tool asks rather than being allowed like a local
read, and a failed search is an error rather than an empty result set,
since a validate novelty score cannot tell those apart and would record
a wrong number.

**What it rules out:** bundling an MCP server (Tavily publishes one and
it stays a fine option); implicit searching outside a visible tool call;
and a result cache, which is deferred because caching changes what "the
evidence said" means across a re-run and interacts with pre-registration
in ways that need their own decision.
