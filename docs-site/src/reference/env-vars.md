# Environment variables

All zorp environment variables use the `ZORP_` prefix. A setting saved
in the web UI beats the matching variable, which beats the built-in
default.

## Model transport

| Variable | Default | What it does |
|---|---|---|
| `ZORP_BASE_URL` | none | OpenAI-compatible endpoint, hosted or local |
| `ZORP_API_KEY` | none | API key. Never written to disk by the web UI |
| `ZORP_MODEL` | none | Model name |
| `ZORP_HTTP_TIMEOUT_SECS` | 900 | Seconds of silence to wait for. On a streamed reply this bounds the gap between chunks, not the length of the answer |
| `ZORP_RETRY_ATTEMPTS` | 4 | Total sends for a request answered 429 or 503. Nothing else is retried |
| `ZORP_RETRY_BUDGET_SECS` | 30 | Seconds of added waiting the retries may spend in total |
| `ZORP_CONTEXT_TOKENS` | unset | Context window size. Unknown unless you say, on purpose |

## Research

| Variable | Default | What it does |
|---|---|---|
| `ZORP_FORECAST` | unset | Ask for a forecast before each investigate attempt and record it |
| `ZORP_CRITIQUE_ROUNDS` | 2 | Bound on critique's revision rounds |
| `ZORP_TAVILY_API_KEY` | none | Enables the `web_search` built-in (with the `search` feature) |

## Web UI

| Variable | Default | What it does |
|---|---|---|
| `ZORP_WEB_TOKEN` | none | Required when binding anything but loopback |
| `ZORP_WORKSPACE` | cwd | Directory the agent works in and the Files pane shows |
| `ZORP_SESSION_TITLES` | 1 | Set 0 to keep the verbatim first message as the sidebar label |
| `ZORP_SKILLS_DIR` | unset | Extra skills directory, wins over user and repo skills |

## Recall, memory, voice

| Variable | Default | What it does |
|---|---|---|
| `ZORP_RECALL_SWEEP_SECS` | 300 | Full-store index sweep interval. 0 disables sweeps |
| `ZORP_RECALL_DB` | next to the session store | Where the vector index lives |
| `ZORP_EMBED_URL` | local Ollama | Embedding endpoint. Must be loopback; a remote host gets a refusal, not a remote embedder |
| `ZORP_EMBED_MODEL` | `nomic-embed-text` | Embedding model |
| `ZORP_VOICE_URL` | `http://127.0.0.1:8000` | ASR endpoint. Loopback only |
| `ZORP_VOICE_MODEL` | `Qwen/Qwen3-ASR-0.6B` | ASR model |
| `ZORP_VOICE_AUTOSTART` | 1 | Set 0 to disable every install and spawn step |

## Install script

| Variable | Default | What it does |
|---|---|---|
| `ZORP_INSTALL_DIR` | `~/.local/bin` | Where binaries land |
| `ZORP_INSTALL_FROM_SOURCE` | unset | Set 1 to force a source build |

This table tracks the README and can lag it. When in doubt, the README
and `--help` win.
