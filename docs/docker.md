# Running zorp in containers

`compose.yml` runs the whole thing: the chat UI, the agent server, and an
Ollama sidecar that serves both the chat model and the embeddings the
conversation search needs.

```bash
ZORP_WEB_TOKEN=$(openssl rand -hex 16) docker compose up --build
```

The UI is then on <http://localhost:8080> and the API on
<http://localhost:7777>. There is nothing to paste: the UI attaches the token
for you, as described below.

There is one compose file and no `-f` flag to remember. If you ever see a
warning about multiple config files, something has added a second stack and
one of them is being ignored; see the 2026-08-24 entry in
`docs/DECISIONS.md` for why that is worth fixing rather than working around.

## The token is not optional

The server binds `0.0.0.0` inside its own network namespace, which is what
makes the published port reachable at all. Anything other than loopback
requires `--token` and the binary refuses to start without one, because a
reachable `zorp-web` is agent-driven shell access to whatever the process can
see. Compose uses a required variable, so the stack refuses to come up rather
than coming up unprotected:

```
$ docker compose up
error while interpolating services.server.environment.ZORP_WEB_TOKEN:
required variable ZORP_WEB_TOKEN is missing a value
```

The API answering 401 without the token is the expected reading, not a fault:

```bash
curl -o /dev/null -w '%{http_code}\n' http://127.0.0.1:7777/api/capabilities
# 401
curl -o /dev/null -w '%{http_code}\n' \
  -H "Authorization: Bearer $ZORP_WEB_TOKEN" \
  http://127.0.0.1:7777/api/capabilities
# 200
```

This is a separate thing from the loopback rule that recall and voice
enforce. That rule is untouched: those still talk to `127.0.0.1` inside the
shared namespace, and nothing here loosens it.

## How the page reaches the API

`index.html` resolves the API base to the empty string when the page is served
over http, meaning same origin. So the UI container is not only a static file
server: nginx carries `/api/` through to the server. Without that the page
loads and then says "no zorp server here", because every API request lands on
the file server and 404s.

nginx attaches the token to each proxied request, so the page itself never
holds one and there is nothing to paste. `/api/sessions/:id/events` is server
sent events, so buffering is off on that path and the read timeout is an hour;
a turn can think for a long time before it says anything.

That is also why both published ports are bound to the host's loopback
(`127.0.0.1:8080` and `127.0.0.1:7777`). Anything that can reach port 8080 is
already talking to the agent, because the proxy authorizes on its behalf. If
you want the UI reachable from another machine, put a proxy you control in
front of it and do the authenticating there; do not widen these bindings.

## Pull the models

The sidecar starts empty. Pull a chat model and an embedding model into it:

```bash
docker compose exec ollama ollama pull qwen3:4b
docker compose exec ollama ollama pull qwen3-embedding
```

Until the embedding model is there the conversation search answers with the
embedder's own error rather than results:

```
the local embedder answered 404: model "qwen3-embedding:latest" not found
```

Point `ZORP_MODEL` and `ZORP_EMBED_MODEL` at whatever you pulled if you want
something else. `ZORP_BASE_URL` can go anywhere an OpenAI-compatible endpoint
lives, including a model already running on the host at
`http://host.docker.internal:11434/v1`. `ZORP_EMBED_URL` cannot: recall checks
the written form and then the resolution, so a non-loopback value is refused.

## Why the sidecar shares a network namespace

`zorp-recall` requires its embedding endpoint to be loopback and enforces that
four ways. A normal compose network is not loopback, so recall would refuse to
talk to a separate `ollama` service by name. `server` joins the sidecar's
network namespace with `network_mode: "service:ollama"`, which gives both
processes the same `127.0.0.1` without weakening a single guard.

That is also why port 7777 is published on `ollama` and not on `server`. A
container sharing another's network namespace cannot declare its own ports, so
the namespace owner has to.

## The first microphone click

The image ships `python3` and its venv and pip, not the Qwen3-ASR runtime.
Several gigabytes of model weights and torch are not baked into the image; the
runtime installs itself into `/home/zorp/voice` the first time someone clicks
the microphone, and that path is a named volume so a restart does not download
it again. Expect the first click to take minutes. The page reports the real
create, install, download, load, and ready stages while it happens.

The container runs as uid 1000 rather than root, which is not just good
practice here: `zorp-voice` refuses to set itself up as root on purpose, so a
root container would leave the microphone broken with no obvious reason.

Set `ZORP_VOICE_AUTOSTART=0` if you want the endpoints present but nothing
installed or spawned.

## What is on disk

Three named volumes, so nothing important lives in the container:

| Volume | Holds |
|---|---|
| `ollama_models` | pulled model weights |
| `zorp_state` | `sessions.db` and `recall.db` |
| `zorp_voice` | the Qwen3-ASR virtual environment and its weights |

The agent's working directory is a bind mount, `./workspace` by default. It
sees that directory and nothing else. Set `ZORP_WORKSPACE` to point it
somewhere real.
