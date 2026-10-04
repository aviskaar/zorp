---
status: accepted
date: 2026-08-21
---

# the browser is told what search it has, and is never left to guess

**Decision:** `zorp-web` gained a non-default `search` feature
(`search = ["zorp-agent/search"]`) and a read-only route,
`GET /api/capabilities`, that reports whether the `web_search` tool is
actually there. The chat UI draws a small pill in the topbar when the answer
is yes and nothing at all when it is no. The pill is a report, not a switch.

**Why the feature:** `zorp-web` depended on `zorp-agent` with no features, so
`web_search` could never register under the browser however the environment
was set up. Adding it as a default would have been the easy fix and the wrong
one: `search` is the only built-in that sends anything off the machine, it is
opt-in on `zorp-agent` for exactly that reason, and `research` deliberately
does not pull it in either. Starting a local web UI should not acquire an
egress path by side effect. Run `cargo run -p zorp-web --features search` when
you want it.

**Why a server answer rather than a guess in the page:** three separate things
decide whether the tool exists, and the browser can see none of them. Whether
the binary was built with the feature is a fact about the build. Whether the
policy permits the tool is a fact about the code. Whether the search provider
found its key is a fact about the environment the server was started in, and
it can change without a restart, so the question is answered per request
rather than cached at startup.

**The answer is observed, not re-derived.** `web_search_availability` in
`zorp-agent` shares one function with the registration site, so the two cannot
drift, and it reads the real `Policy` rather than a copy of its reasoning. The
test that matters asserts the reported answer against `tool_names()`, which is
registration itself. A hand-written copy of the three conditions would have
been right on the day it was written and silently wrong afterwards, and the
failure mode is the worst one available here: a page saying nothing leaves
this machine while something does.

**What it does not claim.** That the tool is registered, not that searching
will work. Whether Tavily accepts the key is only knowable by spending a
search, and this question gets asked on page loads.

**A separate route rather than another field on `/api/settings`.** Settings
are things a person chose and can PUT back. Nothing here is choosable from a
browser, and putting it beside the settings would invite an attempt to set it.
