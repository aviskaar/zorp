---
status: accepted
date: 2026-08-17
---

# open-context connects as an MCP server, and searching your own material is not evidence

**Decision:** open-context is documented as an MCP server that a user
connects, not something zorp depends on, embeds, or starts. The README
carries a `.zorp/mcp.toml` recipe pointing at a local checkout.

Separately, `validate`'s search gate now rejects tool names that carry a
search verb over the user's own stored material (`context`, `memor`,
`note`, `recall`), not only names with no verb at all.

**Why:** zorp already has a general MCP client, so "use open-context in
zorp" needs no new coupling: eleven tools show up prefixed
`mcp__opencontext__` and the agent gets memory that outlives a session.
Wiring a Node package into a Rust product as a soft runtime dependency
would buy nothing this does not.

The gate change is not cosmetic. `mcp__opencontext__search_contexts`
carries "search", so before this, connecting a memory server silently
satisfied the evidence gate, and `validate` would score a question
against notes the user wrote themselves while reporting that it had
searched. A test now pins that, and it failed before the fix.

**Ruled out:** an `npx` recipe. The npm package named `opencontext` is an
unrelated project by another author, so that recipe would have zorp users
download and run a stranger's code.

**Known limit:** the gate is a heuristic over tool names, so it only
covers the case it can see. The durable version is an explicit per-server
flag in `.zorp/mcp.toml`, which needs the server config threaded from
`zorp-mcp`'s registry down into `validate`. Not done here: it changes
public API for a hazard the name check already closes in practice.
