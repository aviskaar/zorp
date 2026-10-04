---
status: accepted
date: 2026-08-14
---

# stdio MCP reads get a deadline, and unadvertised features are not probed

**Decision:** `StdioTransport` reads its child's stdout on a helper
thread and waits with a deadline, so `timeout_secs` now means the same
thing for stdio that it already meant for the HTTP transports. A blown
deadline is a real `McpError::Timeout` naming the server. The transport
also closes stdin and reaps the child on drop. Separately,
`list_prompt_names` returns empty without sending anything when
`initialize` did not advertise a `prompts` capability; a server that
reports no capabilities block at all is still asked, since "did not
say" is not "does not have".

**Why:** Found by end-to-end UAT. `timeout_secs` was accepted and
silently dropped on the stdio path, and a pipe read cannot carry its own
deadline the way a `ureq` call can. Any stdio server that took a request
and never answered wedged the whole CLI with no output and no way out
but Ctrl-C. That is not a hypothetical shape: zorp asked every server
for `prompts/list` whether or not it offered prompts, and a server is
free to ignore a method it never claimed. The capability gate removes
the common case, the deadline covers the rest, and reaping on drop keeps
a wedged server from outliving the process it was spawned for.

**Ruled out:** non-blocking pipes plus `poll`, which is Unix-only and
buys nothing over a reader thread here.
