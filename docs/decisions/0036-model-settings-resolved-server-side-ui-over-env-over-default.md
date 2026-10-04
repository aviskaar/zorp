---
status: accepted
date: 2026-08-17
---

# model settings resolved server-side, UI over env over default

**Decision:** `zorp-web` gained `GET/PUT /api/settings`,
`GET /api/settings/models`, and `POST /api/settings/test`, all behind the
existing token gate. Settings are resolved on the server with a fixed
precedence per field: a value saved through the settings panel beats the
matching `ZORP_*` env var (`ZORP_PROVIDER`, `ZORP_BASE_URL`, `ZORP_MODEL`,
`ZORP_API_KEY`, `ZORP_MAX_TOKENS`), which beats the hardcoded default
(`https://api.openai.com/v1`, `gpt-4o`). `GET /api/settings` reports
`configured: false` only when every field is still at its hardcoded
default and no key is set anywhere, which is exactly the shape that used
to fail silently: `turn.rs` called `HttpModel::try_from_env()`, which
defaults quietly and let the first message a fresh install ever sent die
on a raw 401 deep inside the provider call. Ollama is not a third
`Provider` variant; it is `OpenAiCompatible` pointed at
`http://localhost:11434/v1`, offered in the UI as a preset. One code path
(`GET {base_url}/models`) lists models for Ollama and OpenAI alike,
verified against a real local Ollama instance during this change.

**Why:** The bug was not "no Ollama support," it was "no way to tell the
server what to talk to from the one place a user actually is: the chat
UI." Browser-shipped provider config per turn was rejected in favor of
server-side settings so the API key never has to leave the machine
running the server, and so the UI stays a thin client for state the
server owns.

**Secrets are held in memory only; non-secrets persist.** `PersistedSettings`
(provider, base_url, model, max_tokens) is written to
`~/.config/zorp/web.toml` on every successful `PUT`. It has no `api_key`
field, so there is nothing on that type to accidentally serialize a key
through. The API key lives in `SettingsState::api_key`, seeded once from
`ZORP_API_KEY` when the server process starts and replaced in memory by a
UI save; it is never written to disk and `GET /api/settings` reports only
`has_api_key: bool`, never the key. This matches the existing stance in
`zorp-agent/src/main.rs` that `api_key` is never read from a flavor
manifest either: secrets stay out of anything that gets committed, synced,
or read back over HTTP.

**What it rules out:** the settings file surviving a restart requires
`main.rs` to load it into `AppState` explicitly at startup; `AppState::new`
and `AppState::with_token` deliberately do not do this themselves, so
`zorp-web`'s existing tests stay hermetic and are not affected by whatever
a developer's own machine happens to have saved.
