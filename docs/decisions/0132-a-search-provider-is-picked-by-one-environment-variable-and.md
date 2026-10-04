---
status: accepted
date: 2026-09-18
---

# a search provider is picked by one environment variable, and a wrong name is an error

**Decision:** SearXNG is the second `zorp_search::SearchProvider`
(`zorp-search/src/searxng.rs`), and `ZORP_SEARCH_PROVIDER` picks which one
`web_search` uses. It is read in `web_search_tool()` in
`zorp-agent/src/agent.rs` and nowhere else. Unset or blank means `tavily`,
so nothing that worked before stops working. `searxng` means SearXNG, at
`ZORP_SEARXNG_BASE_URL` or `http://localhost:8888` when that is unset. Any
other value is an error: the tool is not registered, stderr says why, and
`web_search_availability` reports the same sentence, naming the variable
and the values it takes. Matching is exact after trimming, one spelling
per provider, the lowercase identifier `SearchProvider::name` returns. From
issue #258.

**Why an unknown value is an error and never the default.** Somebody who
wrote `searxgn` meant to keep their queries on their own instance. Quietly
sending them to Tavily instead is the one outcome worse than no search at
all, because it looks like it worked. The tests set a Tavily key before
naming a provider that does not exist, so a fallback would register the
tool, and assert that it does not.

**Why the environment and not a flavor manifest.** For the reason the
Tavily key is not in one. A workspace flavor is a file the model can write,
and a file the model can write must not move where queries go. The same
holds for `ZORP_SEARXNG_BASE_URL`.

**`web_search_availability` still answers for one provider, whichever is
selected.** It calls the same `web_search_tool()` that registration calls, so
the answer and the registry cannot disagree about which provider that is,
and `web_search_availability_agrees_with_the_gates_it_reports_on` still
pins it to `tool_names()`. The detail now names the provider, since with
two of them "registered" alone does not say where a query goes.

**Not a loopback capability.** `zorp-recall` and `zorp-voice` guarantee
text reaches a loopback address or nowhere. This does not, and nothing
says it does. A local SearXNG happens to listen on loopback, but it
forwards every query to the engines its operator enabled, and the same
variable can name a public instance. So there is no `LoopbackUrl` guard,
the base URL may be plain HTTP and may resolve to a private address (an
instance on the operator's own network is the point), and the availability
sentence keeps saying every search leaves this machine for SearXNG as it
does for Tavily. A test asserts that sentence survives selecting SearXNG.

**No key, and no new error variant.** SearXNG takes no key and has a
default base URL, so there is nothing missing to report and its
constructor cannot fail. An instance that is not running is found out by
asking it, which is `SearchError::Transport`, never an empty `Vec`. A
comment in `searxng.rs` says this so the variant does not get added back.

**Two SearXNG specific rules.** An empty `results` beside a non-empty
`unresponsive_engines` is a `Transport` error rather than "nothing
matched", because engines that might have had the answer failed and an
empty list would let a caller read an outage as a novel idea. And a 403
says to add `json` to `search.formats` in the instance's settings.yml,
because a stock install serves HTML only and its 403 reads like an
authentication failure. SearXNG has no result count parameter, so
`max_results` is applied after parsing. The provider builds its own
`ureq` agent with its own timeouts, 10 seconds to connect and 30 to read,
for the reason the 2026-08-22 HTTP agent entry gives.

**Ruled out:** selecting from the flavor manifest or the settings file,
a fallback chain across providers, merging or ranking results across
providers, and a browser control for picking one. Two providers and an
environment variable first.
