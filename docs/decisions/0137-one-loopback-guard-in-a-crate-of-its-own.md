---
status: accepted
date: 2026-10-01
---

# one loopback guard, in a crate of its own

**Decision:** `zorp-recall` and `zorp-voice` each carried a copy of the
same guard, and the copies differed only in their error wording and in one
voice flag. The guard now lives once, in `zorp-loopback`: `LoopbackUrl`,
`LoopbackError`, `LoopbackResolver` and the three private helpers, with
recall's comments. Both crates depend on it and re-export it under the
names they already had, so `zorp_recall::LoopbackUrl::parse` and every
other call site read exactly as before. See issue #121.

**Why a crate of its own, with no workspace dependencies.** Both callers
depend on no other workspace member, and that is what keeps each of them a
small thing a reviewer can hold in their head. Putting the guard in either
one would make the other depend on embedding or on voice setup. Putting it
anywhere bigger would drag both into the rest of the workspace. So it is a
crate whose only dependency is `ureq`, for the `Resolver` trait, and it must
stay that way.

**What stayed in each crate, and why.**

- The wording. A refusal has to say what it is protecting, and only the
  caller knows whether that is conversation text or a voice recording. Each
  crate names its `Phrases` through a unit type implementing `Wording`, and
  the shared code puts them into fixed sentences. Every message either crate
  produced before is pinned word for word in its own `tests/loopback.rs`.
  The type parameter also means a URL checked for recall cannot be handed to
  the voice client.
- Layers 3 and 4 from the 2026-08-22 entry, `redirects(0)` and
  `try_proxy_from_env(false)`. They are settings on an HTTP agent, and each
  crate builds its own agent with its own timeouts, next to the request it
  guards. A shared agent builder would put the four layers in two files
  when they are easiest to check in one.
- `supports_direct_runtime`. Whether `qwen-asr-serve` can bind an endpoint
  is voice's question. The shared type exposes the scheme and the path as
  facts, and voice's `DirectRuntime` trait draws the conclusion.
- The canary tests. `no_remote.rs` and `no_proxy.rs` count connections
  through each crate's own client, because that is the thing that can leak,
  and the guard alone cannot prove the agent around it is configured right.
  The pure parse and resolver cases moved to `zorp-loopback/tests` so they
  run once against the one copy.

**Checked to have teeth.** Making `is_loopback` answer yes for everything
turned both crates' `an_off_device_url_is_refused_before_any_request` red.
Making the resolver fall through to a real lookup turned both crates'
resolver tests red. With redirects also allowed, both crates' redirect
canaries counted a connection, and with proxy-from-env also on, both
crates' proxy canaries did.

**What it rules out:** a third copy of the guard in some future crate, and
moving any part of an HTTP agent's configuration into `zorp-loopback`.
