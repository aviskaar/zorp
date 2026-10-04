---
status: accepted
date: 2026-08-24
---

# one compose stack, extended, with an Ollama sidecar

**Decision:** `zorp-web/Dockerfile` builds with `--features voice,recall`
and carries python3 with venv and pip. `compose.yml` gains an `ollama`
service, and `server` joins its network namespace with
`network_mode: "service:ollama"`. Conversation state and the voice runtime
live on named volumes. No new Dockerfile and no second compose file.

**Why:** the repo already had a Docker setup and a second one was written
beside it without anyone noticing the first. Compose loads `compose.yml` in
preference to `docker-compose.yml`, so the newer file was never read and
the documented command silently started the older stack. The verification
that passed had used `-f docker-compose.yml`, which is the one thing the
docs did not tell anyone to type. One stack, extended, is the only
arrangement where what the docs say and what runs are the same thing.

The sidecar is not a convenience. `zorp-recall` requires its embedding
endpoint to be loopback and enforces that four ways, so an Ollama reached
over a compose network would be refused. Sharing a network namespace gives
both processes the same 127.0.0.1 and weakens no guard. A container in
another's namespace cannot publish its own ports, which is why 7777 is
declared on `ollama` and not on `server`.

The image runs as uid 1000 for a reason past hygiene: `zorp-voice` calls
`refuse_root_for(geteuid().is_root())`, so as root the microphone does not
work at all. The state and voice directories are created in the image so
the volumes mount onto paths that user owns; left to Docker they arrive
owned by root and the server cannot write its databases.

Two smaller things came out of the same read. The build stage was missing
four workspace members that arrived after it was written, and cargo cannot
parse a workspace whose members are absent, so the image had stopped
building. And `ZORP_BASE_URL` now defaults to the sidecar rather than
`host.docker.internal`; override it to point back at a model on the host.

The UI container shipped nginx with no configuration, so the whole stack had
never worked from a browser. `index.html` resolves the API base to the empty
string when the page is served over http, meaning same origin, and every API
request landed on the static file server and 404'd, which the page reports as
"no zorp server here". Neither earlier check caught it, because both curled
port 7777 directly rather than loading the page. `web/nginx.conf.template`
proxies `/api/` through, with buffering off and an hour's read timeout so the
event stream is not held back. It attaches the token itself, so the page never
holds one; `ZORP_API_TOKEN` had only ever been a commented-out line in
`index.html` and nothing injected it. Because the proxy authorizes on the
browser's behalf, both published ports are bound to the host's loopback:
whatever reaches port 8080 is already talking to the agent. Anyone wanting the
UI reachable from another machine puts their own authenticating proxy in
front, rather than widening these bindings.

**Ruled out:** baking the Qwen3-ASR runtime into the image. That is several
gigabytes of torch and weights for a feature many people never touch, and
`zorp-voice` already installs it on first use into a marked virtual
environment, which a volume then keeps across restarts.
