---
status: accepted
date: 2026-08-24
---

# distribution happens at the capability boundary, over zorp-web's API, with git as the state bus

**Decision:** zorp's cluster model is a coordinator (a new `zorp-fleet`
crate) driving worker pods that each run `zorp-web` plus its agent. The
unit of distribution is a whole capability invocation on one track,
never a tool call inside a turn. The protocol is zorp-web's existing
HTTP API, versioned, plus two new worker endpoints (`/api/health`,
`/api/capabilities`). Track state moves through a central git remote
per track; workers clone, run one job, commit, and push, so the
tamper-evident git foundation doubles as the cluster's consistency
mechanism. Human checkpoints are relayed to the coordinator and decided
in one place; kill-threshold breaches still kill in code on the worker.

**Ruled out:** an MCP server mode on zorp-agent as the control protocol
(net-new server code, and request/response fits long streaming turns
worse than the SSE API that already exists), any new wire protocol
(gRPC, message brokers), distributing tool calls across machines, and
running workers with checkpoints auto-approved.

**Why:** the synchronous in-process agent loop is a feature worth
keeping, `zorp-web` already is a worker control surface (submit turn,
stream events, stop, token auth off loopback), and `zorp-track`'s
git-backed, one-attempt-per-invocation design makes the track the
natural shard. Design only; nothing is implemented yet.

**Full writeup:** `docs/superpowers/specs/2026-08-24-zorp-fleet-distributed-design.md`
