---
status: accepted
date: 2026-09-11
---

# the library feature gets its own runner rather than more free disk

**Decision:** `cargo test -p zorp-track --features library` moved out of the
`research` job into a `research-library` job of its own. Same trigger,
nightly and on pushes to main, and still not on `research-pr`.

**What went wrong:** the step died with `collect2: fatal error: ld
terminated with signal 7 [Bus error]`. That is a full disk seen from inside
the linker, which maps its output file and then faults on the write, so it
arrives looking like a broken toolchain. It is the second time the research
stack has gone red for disk in a different costume: the first was `ar` reporting "No
space left on device" while linking libduckdb.a, which reads like a
compiler error and is not one either.

**Why a second runner and not a bigger reclaim.** The pressure is
cumulative and freeing is not. `research` resolves four feature sets, each
one gives `libduckdb-sys` a different metadata hash and so builds the
bundled amalgamation again, and this feature puts LanceDB and the whole
arrow tree on top of all four. Deleting the unused toolchains buys about 22
GB once, which is a constant against something that grows every time a
dependency does. A second runner is one feature set in an empty target
directory, and it does not creep back over the line.

The job keeps its own `shared-key`. Writing this feature set under
`research-stack` would leave an entry none of the other jobs can restore
and that is large enough to evict one they can.

**What it rules out:** freeing more disk as the answer to this, and merging
the research jobs back together. If a fifth feature set ever needs building
it gets a runner too. What this does not change is what is gated: the
`library` feature is still off by default everywhere, `research-pr` still
does not build it, and `zorp-agent`'s one `library` call site is still
ungated, for runner disk and not for anything about the code.
