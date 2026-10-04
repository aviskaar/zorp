---
status: accepted
date: 2026-08-20
---

# a command may not call the server it is running under

**Decision:** when `zorp-agent` runs under `zorp-web`, `Policy` is given the
server's port and denies any `run_command` naming a loopback address on it.
The CLI configures no port, so nothing there changes.

**Why:** one approved `run_command` was enough to `curl` the API and turn
auto-approve on, after which nothing the agent did was reviewed again. That
turns a single approval into a standing one, which is the one escalation the
approval gate exists to prevent. It is a hard deny rather than a prompt, so
the call never reaches the gate and cannot be waved through by a user who is
clicking quickly.

The check rides on `deny_reason`, so it inherits that function's recursion
into `sh -c` payloads and `$(...)` bodies. Wrapping the call in a shell was
otherwise the whole bypass, and there is a test for it.

**What this is not.** It is not a boundary against an adversary, and the doc
comment on `with_own_server` says so in as many words. A shell is a shell:
anything holding one can obfuscate a URL past a literal string check without
effort, and can do considerably worse things than call an HTTP endpoint. What
this defends against is the model doing it, which is the case that actually
occurs: a curl to an address sitting in its own context is an obvious next
step for a model that has been told, or has talked itself into, wanting fewer
prompts.

The real control is authentication the local process cannot replay, for
instance a per-session secret handed to the browser at creation and never
written to disk. That is a larger change and deliberately not this one.

**Deliberately still allowed:** every other loopback port. Denying all of
them would break talking to a local model, which is how most of zorp is run.
`http://localhost:11434` has a test saying so.
