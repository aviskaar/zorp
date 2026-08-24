# fleet: run zorp as a cluster of workers under one coordinator

**Date:** 2026-08-24
**Status:** approved design, implementation deferred

## Purpose

Run zorp on many servers or pods at once, controlled from one main
zorp. The main zorp (the coordinator) hands out work, watches progress,
collects results, and puts every human decision in one place. The
workers do what a single zorp does today: run one capability invocation
on one track.

This spec records the shape of that system before any of it is built,
so the decisions are made deliberately and not discovered mid-refactor.
Nothing in here is implemented yet.

## What exists today, and what it implies

The design leans on four facts about the current code.

First, the agent loop is synchronous and in-process on purpose.
`Agent` in `zorp-agent/src/agent.rs` owns its model handle, tool
registry, policy, and cancel token, and the core client in `src/lib.rs`
is blocking ureq with SSE streaming and 429 retry. So the unit of
distribution is a whole capability invocation: one `validate`, one
`investigate` attempt, one `co-write` pass, one `deliver`. Never a tool
call inside a turn. The loop stays exactly as it is.

Second, `zorp-web` is already most of a worker. Its API
(`zorp-web/src/api.rs`) exposes create session, start turn, stop turn,
and an SSE event stream, with `--bind` for non-loopback interfaces and
a mandatory `--token` off loopback. Submit work, stream progress,
cancel: that is a worker control surface, built and tested.

Third, `zorp-mcp` is a client only. Stdio and streamable HTTP
transports, fully synchronous, and `McpServer` is a handle to a remote
server, not a server implementation. There is no MCP server mode to
reuse, and this spec does not build one.

Fourth, `zorp-track` shards by track. Each track is a git repo with
tamper-evident pre-registration, plus a DuckDB run record on local
disk, single writer, with index rebuild already implemented. A track
is inherently sequential: investigate is one attempt per invocation and
pre-registration refuses changed parameters. One track runs on one
worker at a time. Parallelism in a zorp cluster comes from running many
tracks, not from splitting one.

## The shape

Two roles, one new crate.

**Workers.** One pod (or server) runs one `zorp-web` plus its agent,
tools, and a workspace volume. The pod boundary is the sandbox
boundary: the agent gets shell and filesystem scope on its own pod,
which is the threat model `zorp-web`'s token requirement already
assumes. Workers gain nothing new except two endpoints (below) and a
tighter self-call rule.

**Coordinator.** A new crate, `zorp-fleet`, following the standing rule
that new zorp capabilities get new crates and never graft onto
inherited harness code. It owns four things:

1. A worker registry. Workers are configured or discovered (in
   Kubernetes, via a Service), and polled for health and capability.
2. A job queue with track affinity. A job is `(track, capability,
   args)`. The scheduler assigns at most one in-flight job per track.
3. The checkpoint relay. See its own section; it is the only genuinely
   novel design work in this spec.
4. Result and artifact collection: verdicts, metrics, `draft.md`,
   `venues.md`, pulled back to the coordinator.

`zorp-fleet` starts as a CLI. A fleet view in the web UI can come
later; it is not part of this design.

## The worker API

The protocol between coordinator and worker is `zorp-web`'s existing
HTTP API, hardened and versioned, not a new protocol. It already
streams (SSE) and cancels (the stop button reaches the model), which a
request/response protocol fits poorly for long investigate attempts.

Two additions on the worker:

- `GET /api/health`: liveness plus whether a turn is in flight.
- `GET /api/capabilities`: which features the binary was compiled with
  and which MCP tools are attached. This matters because `validate`
  refuses to run without a search-capable tool and `deliver` refuses
  without a huiban-prefixed one. The scheduler must know, per worker,
  what can actually run there, instead of finding out from a refusal.

Once `zorp-fleet` depends on this API, the API is versioned and breaking
it is a decision, not a side effect of UI work.

## State: git is the bus

The track's git repo is the source of truth, and a central git remote
per track is how state moves between machines. Workers are stateless
with respect to tracks:

1. The coordinator assigns a job and names the track's remote.
2. The worker clones (or fetches) the track, rebuilds the DuckDB index
   if needed, runs the capability, commits, and pushes.
3. The coordinator's view of the cluster's state is the remotes, plus
   what workers report over the API.

This is the payoff of the git-backed foundation: the tamper evidence
built for pre-registration doubles as consistency checking across
machines, for free. A worker that dies mid-attempt loses at most one
uncommitted attempt, and the track is re-assignable because nothing
about it lives only on that pod.

The alternative, sticky workers with a persistent volume per pod and
tracks pinned to pods, is simpler on day one and acceptable as a
stepping stone, but a dead pod strands its tracks. It is not the
destination.

The DuckDB run record travels inside the track. Single-writer stays
true because the scheduler enforces one worker per track at a time; the
database never needs to be multi-writer.

## The checkpoint relay

This is the part distribution must not break. zorp's capabilities are
chained by human checkpoints, and investigate's kill threshold is
enforced in code so `--yes` cannot wave a breach through.

Rules:

- A kill-threshold breach kills the track on the worker, in code,
  exactly as today. No relay, no coordinator override, no appeal.
- Every other checkpoint travels. The worker pauses at the checkpoint,
  reports it through the fleet API, and waits (or parks the track and
  frees itself, see open questions). The human decides at the
  coordinator, in one place, and the decision goes back to the worker
  or into the track for the next assignment.
- Workers never run with checkpoints auto-approved. Distribution is
  not a license to remove the human. If a deployment wants unattended
  batches, that is a future decision with its own entry, not a flag
  someone flips on a pod.

## Security

- One token per worker, not one shared token for the fleet. The token
  is a shared secret over plain HTTP today, so inside a cluster the
  links get TLS (or a service mesh that provides it).
- The 2026-08-20 rule, a command may not call the server it is running
  under, gets a cluster-shaped extension: a worker's policy also denies
  `run_command` calls naming sibling workers or the coordinator. An
  agent on one pod must not be able to drive another pod's agent
  through the fleet's own control plane.
- Secrets stay pod environment (`ZORP_TAVILY_API_KEY` and friends),
  never in flavor manifests, matching the standing rule. In Kubernetes
  that is a Secret mounted as env, per worker.

## Rate limits and model access

Every worker calls the model provider directly through the core client,
and the 429 retry-with-backoff is per-process. N workers on one shared
key multiply pressure on the same pool and will thrash it. The fleet
scheduler therefore carries a global concurrency cap on model-calling
jobs, and deployments are encouraged to use per-worker keys where the
provider allows it. No shared token bucket service; the cap lives in
the scheduler, which is already the single place that decides what runs.

## What this rules out

- **No distribution inside a turn.** The synchronous loop is a feature.
  Tool calls never cross machines.
- **No MCP server mode on zorp-agent.** It was considered as the
  control protocol, since the client, transports, and TOFU trust store
  already exist. Rejected: the server side would be net-new code, and
  MCP's request/response shape fits long streaming turns worse than the
  SSE API zorp-web already has.
- **No new wire protocol.** No gRPC, no message broker. The existing
  HTTP API, versioned, is enough until proven otherwise.
- **No auto-approved checkpoints on workers.** Stated above; repeated
  here because it is the easiest thing for a future change to erode.

## Phases

1. **Phase 0, containers only.** The Dockerfile and compose file exist.
   Run N `zorp-web` pods with `--bind 0.0.0.0 --token ...`. No
   coordinator; humans drive workers directly. Proves the worker shape.
2. **Phase 1, the worker contract.** Add `/api/health` and
   `/api/capabilities`, version the API, add per-worker tokens and the
   sibling-call denial.
3. **Phase 2, the coordinator.** Build `zorp-fleet`: registry, queue
   with track affinity, checkpoint relay, result collection. CLI first.
4. **Phase 3, stateless workers.** Git-remote state sync, track
   re-assignment, artifact collection to the coordinator.

Each phase is independently useful and stops cleanly. Phase 0 needs no
new code at all.

## Open questions

- Does a worker block while a checkpoint waits for a human, or park the
  track and take other work? Blocking is simpler and wastes a worker;
  parking needs a resume path through the queue. Phase 2 decides.
- Where do track remotes live: one bare repo per track on the
  coordinator, or an external git host? Phase 3 decides.
- Whether `zorp-fleet` reuses the session store shape from `zorp-web`
  or keeps its own job log. Phase 2 decides.
