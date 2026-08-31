# HTTP API

`zorp-web`'s API, served from the same origin as the UI. Loopback by
default; any other bind requires a token, and every route below sits
behind that check.

Routes for optional features exist in every build and answer with "off,
and here is why" (or 501) when the feature is not compiled in, so a
client can say why a button is disabled instead of interpreting a 404.

## Sessions and turns

| Route | Method | What it does |
|---|---|---|
| `/api/health` | GET | Liveness |
| `/api/sessions` | GET, POST | List sessions, create one |
| `/api/sessions/:id` | GET | One session |
| `/api/sessions/:id/turn` | POST | Start a turn |
| `/api/sessions/:id/stop` | POST | Stop the running turn |
| `/api/sessions/:id/events` | GET | The event stream (SSE) |
| `/api/sessions/:id/approve` | POST | Answer a tool approval prompt |
| `/api/sessions/:id/auto-approve` | GET, POST | Read or set auto-approve for one chat |

## Settings

| Route | Method | What it does |
|---|---|---|
| `/api/settings` | GET, PUT | Read and update settings. The key is never sent back out, only `has_api_key` |
| `/api/settings/models` | GET, POST | List the endpoint's models. A candidate key travels in the POST body, never a query string |
| `/api/settings/test` | POST | Probe the endpoint with a minimal real completion |
| `/api/capabilities` | GET | Which optional tools are really there |

## Research

| Route | Method | What it does |
|---|---|---|
| `/api/sessions/:id/panel` | POST | Launch an adversarial review panel |
| `/api/panel/lenses` | GET | The code-defined review lenses |
| `/api/sessions/:id/investigate` | POST | Run one investigate attempt |
| `/api/investigate/status` | GET | Whether investigate is available, and whether forecasting is on |
| `/api/investigate/ledger` | GET | Read what landed in the aryabhatta ledger. Names no model-authored text column |

## Files, recall, voice

| Route | Method | What it does |
|---|---|---|
| `/api/artifacts` | GET | List workspace files |
| `/api/artifacts/raw` | GET | Read one, allowlisted extensions only |
| `/api/recall/status` | GET | Whether conversation search is on |
| `/api/recall/index` | POST | Force an index pass |
| `/api/recall/search` | GET | Semantic search over conversations |
| `/api/voice/status` | GET | Voice runtime status, read-only |
| `/api/voice/wait` | POST | Start readiness and wait for it |
| `/api/voice/transcribe` | POST | Transcribe recorded audio (25 MB body limit) |

The authoritative list is the router in
[`zorp-web/src/api.rs`](https://github.com/aviskaar/zorp/blob/main/zorp-web/src/api.rs).
