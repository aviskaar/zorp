---
status: accepted
date: 2026-10-27
---

# text to speech is loopback-only, and a person turns it on

**Decision:** zorp can read an answer aloud. A speech client in `zorp-voice`
(`OllamaSpeech`) speaks text through a local Ollama server's
OpenAI-compatible `POST /v1/audio/speech` with the `phonon` model, and lists a
model's voices from `POST /api/show`. `zorp-web` exposes it behind the
existing `voice` feature at `GET /api/speech/status`,
`GET /api/speech/voices`, `POST /api/speech/chunks` and `POST /api/speech`,
registered in every build and answering 501 without the feature, the way the
voice routes do. The browser gets a speaker button under every answer, a
read-aloud toggle in the toolbar, and model and voice pickers in Settings. The
TTS model and voice are saved settings (`config.rs`), resolved the same way
the chat model is.

**The text goes to a loopback address or nowhere** (ADR 0077, 0078, 0137). An
answer can quote a person's files, so TTS gets the same boundary as recorded
voice and recall: `zorp-loopback`'s `LoopbackUrl::parse` and
`LoopbackResolver`, redirects off, proxy-from-env off, worded for text to be
spoken (`SpeechWording`). There is no cloud TTS provider and no fallback. A
connection-counting canary (`zorp-voice/tests/no_remote.rs`) proves it by
counting hits on a canary, not by trusting an error, because a request never
made and a request that failed look the same from the caller's side.

**Speaking is a person's switch, never a model's.** No tool starts or stops
speech; the speaker button and the read-aloud toggle are the only starts, and
both are in the browser. Spoken text is only ever the rendered answer: the
read-aloud path speaks the finished answer text and never tool output or
hidden content, and only on the live streaming path, so reopening a
conversation does not read its history aloud.

**The model is a setting, not a constant.** Phonon is the only TTS model
today, but others are coming, so `speak` and `voices` take the model by name
and the picker is driven by what the model's `/api/show` reports, the way the
chat model listing drives the chat picker.

**Chunking is one chunker, server-side.** Long text is split on sentence
boundaries under phonon's per-request character limit by
`zorp_voice::chunk_for_speech`, exposed at `POST /api/speech/chunks` so the
browser starts playback on the first chunk without a second copy of the
splitter in TypeScript. A decimal point or a `v1.2` does not split a sentence.

**The voices field name is tolerant on purpose.** It was not possible to
confirm the exact `/api/show` field that lists a speech model's voices against
Ollama's docs when this was written, so `extract_voices` searches the answer
for the first string array under a `voices` or `*.voices` key and falls back
to the default voice when it finds none. That is the one place to pin once the
field is confirmed; it never fails the call.

**What is a follow-up.** The Mac app routes and request-body tests, and a
fully hands-free speaking mode that listens, sends and speaks without a key
press, are follow-ups. The read-aloud toggle plus the existing voice input
already give most of a speaking mode; the auto-send loop is the part left.
