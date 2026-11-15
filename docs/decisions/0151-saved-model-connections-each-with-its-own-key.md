---
status: accepted
date: 2026-11-15
---

# Saved model connections, each with its own key

**Decision:** `zorp.toml` keeps a list of named model connections beside the
main model (`Saved.connections`, `Saved.default_connection` in
`zorp-agent/src/config.rs`). A connection is an id, a label, a provider, a
base URL, a model, a token limit and optionally the name of an environment
variable that holds its key. A conversation records the connection and model
it runs on in `sessions.connection` and `sessions.connection_model`, a turn
can change them with `connection` and `model` in its body, and a branch
copies them. A fresh conversation with nothing stored starts on the default
connection. Every model call made for a conversation resolves through that
choice: the turn, its title, compaction summaries, the tool safety check and
the skill offer, through `SettingsState::model_for` and `model_for_session`.

**Bring your own key.** Each connection uses only its own key: one typed into
the browser or the Mac app, held in the server's memory and, in the Mac app,
the login keychain under `connection.<id>`, or one read from the variable the
connection names. It never falls back to `ZORP_API_KEY` or the main model's
key, because sending one provider's key to another provider's endpoint leaks
it. The settings file has no field for a key, and a test checks that neither
the persisted file nor `GET /api/settings` carries one; the listing says only
`has_api_key`.

**Why:** People run the same work against several endpoints at once: this
Mac, a remote Ollama or oMLX, OpenRouter, Ollama Cloud, DeepSeek. Switching
the one saved model each time lost the previous endpoint and moved every open
conversation with it.

**Not covered:** A panel and an investigate run still use the main model,
since their requests carry no conversation choice. The CLI has no
`--connection` flag yet. A flavor that sets a model still wins, as it does
over the saved file. No tool reads, picks or edits a connection.
