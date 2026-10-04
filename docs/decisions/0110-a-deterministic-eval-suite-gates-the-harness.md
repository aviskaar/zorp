---
status: accepted
date: 2026-09-05
---

# a deterministic eval suite gates the harness

**Decision:** `zorp-eval` grows a second half, `harness`, which runs the
real `zorp-agent` binary against a scripted provider on loopback. A case
is a TOML file: a prompt, a script of replies, and what must be true
afterwards. `zorp-eval/src/harness/` is the runner, `zorp-stub/` is the
scripted provider lifted out of `zorp-agent/tests/sse_stub/` so both the
transport tests and the suite serve the same bytes, and a `harness`
continuous integration job gates every pull request on it. Four seed
cases ship with it and the catalogue of the rest is
`docs/superpowers/specs/2026-09-05-harness-eval-catalogue.md`.

**Why:** everything that gates today stops at the `Model` trait.
`agent.rs` drives a scripted model through the run loop in about eighty
in-process tests, and those are the right shape for tool calls,
approval, compaction and termination. What no gate reached was the layer
underneath: the HTTP client, the streaming parser, the retry bound, the
read timeout, and the store on disk once the process is gone. That is
where every expensive bug of the last month lived. The streaming path
ran with no read timeout at all for months and one call sat on a socket
for three hours. A provider error delivered inside an HTTP 200 killed
nine of nine benchmark trials. A truncated stream returned `Ok` and a
nine hour run was misread twice.

**The other half deliberately does not gate.** `compat` spawns the agent
against a live provider, which answers a question about models and
cannot answer a question about code: a recent nine task run lost four
tasks to upstream 404s. A gate that a provider outage can turn red
teaches people to ignore it. This half has no network, no key and no
model, so the only thing that can fail a case is the code.

**A case is data, and an unknown field is an error.** No script runs, so
what a case can express is exactly what `case.rs` can parse. A
misspelled expectation that is quietly dropped is a case that passes
having checked nothing, and a transport field the kind does not read is
a case that is not testing what it says it tests. Both are refused at
load. An empty case directory is refused for the same reason: a suite
that exits zero having run nothing is the one failure mode worth
refusing outright.

**Connections are counted, not inferred.** A retry and a slow first send
are indistinguishable from the caller's side, which is why the transport
promises in `docs/DECISIONS.md` (2026-08-23, 2026-09-04) are pinned by
counting connections rather than by checking for an error. The suite
keeps that discipline: any case about the retry bound states its own
connection count, and every inherited `ZORP_` variable is cleared before
a case sets its own, because a count means nothing if the developer's
shell gets to choose the bound.

**What does not belong here.** A case that could have been an in-process
test in `agent.rs` belongs there instead: spawning a process to re-prove
something a scripted `Model` already proves is slower and no more true.
The catalogue names what is already covered at that level so nobody
writes it twice.
