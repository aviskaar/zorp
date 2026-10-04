---
status: accepted
date: 2026-08-22
---

# conversations are searchable by meaning, and the vectors never leave the machine

**Decision:** `zorp-recall` is a new workspace member holding a loopback
guard, an embedder that talks to a local Ollama, and a SQLite vector index.
`zorp-web` gains `GET /api/recall/status`, `POST /api/recall/index` and
`GET /api/recall/search` behind a non-default `recall` feature, and the
sidebar gains a search box. The corpus indexed is `zorp_agent::Store`, the
conversations the browser already lists.

**Conversation text goes to a loopback address or it goes nowhere.** There
is no remote embedding provider, no flag that adds one, and no fallback when
the local model is missing. That is the whole decision and everything else
here is in service of it. This corpus is a person's entire history with an
agent that has been reading their files, so a capability that quietly keeps
working by posting it to an API is not a degraded version of this feature.
It is the worst thing the code could do, and a silent fallback is the shape
that failure always takes.

**Four things enforce it, and they are layered on purpose.**

1. `LoopbackUrl::parse` is the only way to name an endpoint. The written
   form has to be a loopback IP literal or exactly `localhost`, because a
   substring test for "127.0.0.1" accepts `127.0.0.1.evil.example`, which is
   a name somebody else owns. The name is then resolved once and every
   address it yields has to be loopback, which catches a `localhost` pointed
   elsewhere in `/etc/hosts`. A name that answers with one loopback address
   and one that is not is refused whole rather than filtered down to the
   safe half.
2. The addresses from step 1 are kept, and `LoopbackResolver` is the only
   thing the HTTP client can resolve through. It performs no lookup of its
   own and answers for exactly one host and port. Checking a name and then
   letting the client look it up again is a check with a gap in the middle,
   and the gap is where the answer changes.
3. `redirects(0)`. A 302 is a request to send the same body somewhere else,
   chosen by whatever answered.
4. `try_proxy_from_env(false)`. `ureq::AgentBuilder::new` turns proxy
   detection on when the `proxy-from-env` feature is enabled, Cargo unifies
   features across the whole graph, so another crate can enable it without
   this one asking. `HTTP_PROXY` on a managed laptop is somebody else's
   server. Note `ureq` routes a proxied connection through the resolver too,
   so step 2 covers this even when step 4 is wrong.

**The tests count connections, not errors.** A request that failed and a
request that was never made look identical from the caller's side, and only
one of them is the guarantee. Each case points a would-be escape at a
loopback socket that counts what reaches it and passes only at zero. Removing
`redirects(0)` and the resolver together was checked to make those cases fail,
so they have teeth.

**Ollama over loopback HTTP, not an in-process model.** `fastembed` and
`candle` are genuinely self-contained, and they are a large tree in a
workspace that pares `zip` down to two codecs and pins an MSRV job to stop
the floor drifting. They also download weights from a model host on first
use, which is a network call, a large one, and one this feature would have to
explain. Ollama is already the first entry in the settings panel's provider
list and already has a test driving it. The cost is an external process, and
the honest answer to not having one is the refusal, not a hosted API.

**SQLite, not the LanceDB library in `zorp-track`.** Reuse was the reflex and
it is wrong three times over. `Library` is keyed by track id because it holds
an investigation's evidence, and chat history is not evidence: the 2026-08-17
entry already drew that line for open-context. The `library` feature is
opt-in specifically because it pulls the whole Arrow tree, and this would pull
it into the web binary. And `rusqlite` with `bundled` is already linked by
`zorp-agent`, the crate that holds the conversations, so the entire capability
adds no crates to the tree at all. The scan is brute force for the same
reason: 93 conversations is 145 vectors, a dot product over that is
sub-millisecond, and an approximate index is a data structure, a build step,
and a recall tradeoff bought for a problem nobody has.

**Indexed on request, incrementally, one vector per message.** Embedding on
write would put a model call in the path of sending a message and make the
chat depend on Ollama being up. Embedding on first search would put minutes
behind a text box. So it is a button, and the button skips any conversation
whose text hash has not moved. A message rather than a whole conversation,
because a conversation averaged into one vector is a vector about nothing in
particular, and because a per-message hit gives the result list a line to
show. Tool results are left out: they are the largest thing in most sessions,
they are mostly files the agent read on the way to an answer, and indexing
them would fill the results with the same file in nine conversations.

**Ruled out:** a relevance floor. Measured against a real local model,
unrelated conversations score 0.47 to 0.58 and the right one scores 0.74, so
any fixed cutoff would be a number invented to look decisive. The list is
ranked and capped instead, weakest last.

**Known limit:** between resolving a name and connecting to it there is a
window the resolver closes for the destination but not for the operating
system's own routing. Nothing here defends against a machine whose loopback
interface has been reconfigured, and nothing can.
