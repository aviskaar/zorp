---
status: accepted
date: 2026-08-24
---

# conversation indexing is a quiet background loop, not a button

**Decision:** `zorp-web` starts one conversation-index worker when the
`recall` feature is enabled. It sweeps the store once after startup, repeats
every 300 seconds by default, and accepts a session as soon as a turn finishes.
`ZORP_RECALL_SWEEP_SECS` changes the interval and 0 disables automatic sweeps.
The worker serializes startup, periodic, per-session and forced passes. None of
them can interleave, and neither server startup nor a turn waits for embedding.
Duplicate notices for one session collapse into one pending pass, and a due
periodic sweep takes priority over queued session work.
The existing conversation fingerprint remains the only change detector, so an
unchanged sweep makes no embedding calls.

The sidebar has no Index button. Status compares conversations in the source
store with conversations in the derived index, reports a running pass, and
reports ready only after a complete pass has caught up. It refreshes every 15
seconds while the page is visible. `POST /api/recall/index` remains for tests
and scripts that need to force a pass.

**Failure is quiet and survivable.** A missing local Ollama is normal on a
machine that has not set recall up. The first failed pass is logged, later
failures stay quiet, a recovery is logged once, and the next tick retries. The
status endpoint carries the last failure so the page can name the missing local
embedder. If the worker itself cannot start, status reports that failure and no
unmanaged per-turn worker takes its place. No fallback was added. Conversation
text still goes to a checked loopback address or nowhere.
Only a successful full sweep clears a failed sweep. An embedder failure clears
only after that sweep makes a successful embedding call, so a fingerprint-only
pass cannot claim recovery without contacting Ollama.

**Cost:** every interval reads session headers and message text to check the
stored fingerprints. Unchanged text costs no model call and no vector write.
The read is accepted because it catches conversations that existed before this
server started and changes made outside a live turn, which the per-session path
cannot see.

**Supersedes:** the button-driven indexing paragraph in the 2026-08-22
conversation-search entry below. Its corpus, fingerprint, message-unit and
loopback decisions still stand.
