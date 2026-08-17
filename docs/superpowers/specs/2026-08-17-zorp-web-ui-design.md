# Design: a web UI for the agent

**Date:** 2026-08-17
**Status:** drafted, awaiting review
**Author of record:** human, drafted with Claude Opus 5

## The problem

zorp is a terminal program. That is correct for the people it was built
for and wrong for everyone else. A person evaluating an agent wants to
type a sentence and watch it work, and today that requires installing a
binary, learning four environment variables, and reading `--help`.

The goal is a chat interface of the kind people already understand from
Claude and ChatGPT, with one difference that matters: this agent touches
real files and runs real commands, so the interface has to show what it
is about to do and let a human stop it.

## Decisions

### D1. The server runs on the user's machine by default

`zorp-web` binds `127.0.0.1`. The agent keeps full access to the user's
files and shell, which is what makes it useful, and no account system,
no per-user sandbox, and no hosting cost are required.

Listening on a network interface is possible and deliberate: it takes
both `--bind 0.0.0.0` and `--token <value>`, and refuses to start with
one and not the other. The reason is blunt. A reachable zorp-web is
agent-driven shell access to the machine it runs on. That is a fine thing
to do inside a private network and a serious mistake to do accidentally.

### D2. Server and UI are separate artifacts

The server is a Rust binary in its own crate. The UI is static files. They
communicate over documented HTTP with CORS, so the UI can be served by
the server itself, by nginx in a second container, or by a CDN such as
Cloudflare Pages while the server runs elsewhere.

**Cloudflare Workers cannot host the server.** Workers are a V8 isolate
with no filesystem and no process spawning, and the agent's entire value
is that it reads files and runs commands. The UI on Pages is fine. The
server needs a container runtime.

### D3. The agent is constructed, not wrapped

`zorp-web` builds an `Agent` the same way `main.rs` does and substitutes
two things: a `WebRenderer` for the terminal renderer, and a web approval
gate for the terminal prompt. It does not shell out to `zorp-agent` and
parse stdout.

This matters because everything the CLI enforces then applies unchanged:
flavor resolution, the approval preset, the hard denylist, tool
allow-lists, session persistence. A subprocess wrapper would have to
re-derive all of it from text, and would drift the first time an activity
line changed.

### D4. Streaming is server-sent events, not WebSockets

The traffic is one-directional: the agent produces events, the browser
consumes them. The one message going the other way, an approval decision,
is a normal POST. SSE reconnects on its own, survives proxies that
mishandle WebSocket upgrades, and needs no framing protocol.

### D5. Approvals are part of v1

An agent UI that cannot edit a file or run a command is a search box. The
mechanism is small: the agent thread parks on a channel, the server emits
an `approval_request` event, the browser renders the tool name and its
arguments, and a POST resolves it. An unanswered request denies after a
timeout, matching what the CLI already does non-interactively.

Deferring this would mean rebuilding the request lifecycle later, because
approvals are what make a turn long-lived rather than request-response.

### D6. Plain TypeScript, no framework

A message list, an input box, and an event stream do not need a component
framework, and this repository has a stated preference for dependency
minimalism that a `node_modules` tree would sit awkwardly against. Build
is `esbuild` to a single bundle, which keeps the UI container to static
files with no runtime.

Revisit if the UI grows a track browser or an artifact viewer. That is a
different UI and can bring its own tooling.

### D7. The session sidebar ships in v1

The `messages` table already stores `session_id`, `seq`, `role`, and
`content`, and `zorp-agent resume` already reconstructs a conversation
from it. History is therefore mostly a read, and it is a large part of
what makes a chat interface feel finished rather than like a demo.

## Shape

```
zorp-web/                     new workspace member, binary `zorp-web`
├── src/main.rs               CLI: --bind, --port, --token, --open
├── src/api.rs                routes
├── src/renderer.rs           WebRenderer: Renderer -> typed events
├── src/approval.rs           web approval gate, parks on a oneshot
├── src/session.rs            reads the existing sessions database
└── Dockerfile

web/                          static UI
├── src/main.ts               event stream, message list, approvals
├── index.html
├── Dockerfile                nginx serving the bundle
└── compose.yml               both containers, workspace bind-mounted
```

### API

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/api/sessions` | start a session, returns its id |
| `GET` | `/api/sessions` | list sessions for the sidebar |
| `GET` | `/api/sessions/:id` | replay a conversation |
| `POST` | `/api/sessions/:id/turn` | send a user message |
| `GET` | `/api/sessions/:id/events` | SSE stream for that session |
| `POST` | `/api/sessions/:id/approve` | resolve a pending approval |

### Events

One JSON object per SSE frame, tagged by `type`: `assistant`, `tool`,
`verify`, `notice`, `working`, `working_done`, `approval_request`,
`error`, `done`. The first seven map one-to-one onto the `Renderer`
trait, which is the point: the browser sees exactly what the terminal
sees.

Every event carries a monotonic `seq` so a reconnecting client can send
`Last-Event-ID` and receive only what it missed.

## Data flow, one turn

1. Browser POSTs a message.
2. Server spawns the agent on `spawn_blocking`, since the agent loop is
   synchronous and would otherwise stall the runtime.
3. `WebRenderer` pushes events onto a channel as the agent works.
4. SSE forwards them as they arrive.
5. On an approval-gated tool the agent parks; the server emits
   `approval_request`; the browser renders it; the POST resolves it.
6. `done` closes the turn. The session stays open for the next message.

## Error handling

- A dropped SSE connection does not cancel the run. Events buffer and the
  client resumes with `Last-Event-ID`.
- A model transport failure becomes an `error` event. It is never a
  silent stall, which is the failure mode a chat UI is worst at showing.
- An approval that is never answered denies on timeout.
- A second turn on a session that is already running is rejected rather
  than queued, because interleaved turns on one agent would corrupt the
  transcript.

## Testing

The workspace already scripts model responses over a local socket
(`zorp-agent/tests/common/mod.rs`), so a full turn is testable with no
network and no key: start the server on an ephemeral port, POST a turn,
read the SSE stream, assert the event sequence. The approval round trip
gets the same treatment, with a scripted model that requests a write.

The UI gets no test framework in v1. It is a few hundred lines against a
documented API, and the API is where the behavior lives.

## What this rules out

- **Hosting it for strangers.** Not a deployment mode this design
  supports. It would need per-user sandboxing of an agent that runs
  shell, which is a different product with a different threat model.
- **A second transport.** No WebSocket fallback until something concrete
  needs one.
- **Exposing the research capabilities.** `validate`, `investigate`,
  `co-write`, and `deliver` are long-running and human-gated, and they
  deserve their own interface rather than being forced into a chat box.
  Chat first; that surface can follow.
