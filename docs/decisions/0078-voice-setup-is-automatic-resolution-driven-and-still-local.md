---
status: accepted
date: 2026-08-24
---

# voice setup is automatic, resolution-driven, and still local

**Decision:** a microphone click now starts local voice setup through the
existing readiness request when the configured runtime is absent. Zorp creates
and owns a virtual environment in the user's local data directory, pins
`qwen-asr==0.0.6`, and executes commands as argument vectors without a shell. It
first asks pip to resolve the vLLM extra. If that fails, it recreates the owned
environment with the plain package and starts an embedded Transformers-backed
HTTP server. Backend selection follows the dependency resolver instead of an OS
list, so a future machine that can install vLLM gets it without a code change.

This supersedes the same-day decision to only show an operator command. That
command cannot install on macOS arm64 because vLLM's dependency tree has no
matching wheel there. The plain package includes the Qwen3-ASR Transformers
backend and the audio dependencies needed for a working local server. The page
shows real setup stages and no command. `ZORP_VOICE_AUTOSTART=0` disables every
setup and spawn path. The crate keeps `start_command()` for operator-facing
callers. The status API carries it only in that explicit compatibility mode,
and the browser never renders it.

**New power and its bounds:** the voice crate may now download and execute one
pinned third-party package on a person's microphone click. It installs only in
the marked environment zorp owns, refuses to run setup or pip as root, binds a
spawned server only to a validated loopback target, and keeps redirects and
environment proxies off. HTTPS and path-prefixed endpoints remain the
operator's responsibility. No agent tool can start setup or recording, and
there is still no remote transcription provider or fallback.

**The page says it is listening:** while the microphone is open the composer
draws a live level meter from the stream, because a click that starts a
minutes-long setup with no visible sign of the microphone reads as a broken
button. The meter reads amplitude and nothing else. It keeps no sample, copies
the audio nowhere, holds nothing after it stops, and it is built so that a
browser without Web Audio, or one where the audio context fails, gets an inert
meter rather than a failed recording. It is decoration and is hidden from
assistive technology; the status line is what a screen reader is told.

**Why permission comes first:** requesting microphone permission and starting
setup are concurrent parts of the same click. The browser must not hide its
permission prompt behind a model download that can take minutes. Recording can
begin while setup continues, then waits for readiness before it sends the audio
to the checked loopback endpoint.
