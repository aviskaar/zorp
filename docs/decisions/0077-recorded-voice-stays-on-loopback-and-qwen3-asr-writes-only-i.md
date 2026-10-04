---
status: accepted
date: 2026-08-24
---

# recorded voice stays on loopback and Qwen3-ASR writes only into the composer

**Decision:** browser voice input uses Qwen3-ASR through a new standalone
`zorp-voice` crate and the local `qwen-asr-serve` 0.0.6 runtime. It is behind a
non-default `voice` feature on `zorp-web`. The HTTP client carries the same
four protections as `zorp-recall`: a written-form and resolution check, a
resolver pinned to one checked host and port, redirects off, and environment
proxies off. `ZORP_VOICE_URL` and `ZORP_VOICE_MODEL` are the only overrides,
and an off-device URL is refused. The transcript goes into the editable
composer and is never sent automatically.

The runtime registers Qwen3-ASR with its pinned vLLM 0.14.0 and uses
`GET /health`, `GET /v1/models`, and `POST /v1/chat/completions`. It has no
HTTP model pull endpoint. The page shows an explicit install and start command
and polls real readiness. It shows no download percentage or stage. Model
weights download when the operator starts the runtime. Zorp constructs that
command only for a root HTTP endpoint. HTTPS and path-prefixed endpoints need
an operator-managed loopback proxy.

Transformers 5.13.1 was checked and rejected. Its `/load_model` endpoint is
real, but its generic audio transcription handler does not use Qwen3-ASR's
required processor and output parser. A working loader paired with an
incompatible transcription endpoint is not a valid runtime contract.

**Why:** voice is at least as sensitive as stored conversation text, and it
must not acquire a quieter route off the machine. Qwen3-ASR has open weights
and multilingual inference in Python, but no Rust implementation. A small
checked loopback client keeps Python and model libraries outside the Rust
workspace while preserving the privacy boundary. Putting the transcript in
the composer keeps a person between recognition errors and every agent action.

**What it rules out.** No in-process Python, torch binding, Candle, ORT,
DashScope provider, cloud fallback, flavor-manifest endpoint, automatic send,
or language fixed to English. A transcript grants no tool, changes no
approval, and bypasses no denylist. A failed local runtime makes voice input
unavailable. It never sends audio somewhere else to keep the button working.

Design:
[`docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`](superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md).
