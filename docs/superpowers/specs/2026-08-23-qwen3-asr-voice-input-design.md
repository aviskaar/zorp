# Qwen3-ASR voice input

## Purpose

The web composer gets a microphone button. A person can record a short
message, have a local Qwen3-ASR model transcribe it, and edit the result
before sending it. Nothing is sent to the agent until the person uses the
existing send control.

Qwen3-ASR detects the language. The browser and server do not select English
or any other language.

## Verified runtime contract

The local runtime is `qwen-asr-serve` from `qwen-asr` 0.0.6. That package
pins Transformers 4.57.6 and vLLM 0.14.0. Its serve command registers the
Qwen3-ASR classes and delegates to `vllm serve`.

Zorp uses three endpoints from that server:

- `GET /health` reports whether vLLM is ready.
- `GET /v1/models` reports the model the server loaded.
- `POST /v1/chat/completions` accepts one `audio_url` content part. Zorp uses
  a base64 data URL, so vLLM never fetches the recording from another server.

The completion text uses Qwen's `language ...<asr_text>...` envelope. Zorp
parses that into the detected language and the transcript. It does not supply
a language and does not translate the text.

These sources were checked before implementation:

- The official [Qwen3-ASR deployment guide](https://github.com/QwenLM/Qwen3-ASR#deployment-with-vllm)
  gives the `qwen-asr-serve` command and the chat-completions audio request.
- The official [`qwen-asr-serve` source](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/cli/serve.py)
  shows that it registers Qwen3-ASR and invokes vLLM serve.
- The official [`qwen-asr` package manifest](https://github.com/QwenLM/Qwen3-ASR/blob/main/pyproject.toml)
  gives version 0.0.6 and pins Transformers 4.57.6 and vLLM 0.14.0.
- The [vLLM 0.14.0 API source documentation](https://docs.vllm.ai/en/v0.14.0/api/vllm/entrypoints/openai/api_server/)
  defines `/health` and `/v1/models`.
- The [vLLM 0.14.0 chat types](https://docs.vllm.ai/en/v0.14.0/api/vllm/entrypoints/chat_utils/)
  define the `audio_url` content part.
- Qwen's [processor](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/core/transformers_backend/processing_qwen3_asr.py)
  and [output parser](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/inference/utils.py)
  define the model-specific input and output shapes.

Transformers 5.13.1 was checked and rejected. Its documented `/load_model`
and `/v1/audio/transcriptions` routes are real. Its generic transcription
handler calls an audio processor with audio alone, while the Qwen3-ASR
processor requires Qwen's text and audio preparation and custom output
parser. A mocked JSON response would hide that incompatibility. Zorp does
not use those endpoints. The route claims were checked in the versioned
[Transformers 5.13.1 Serve documentation](https://huggingface.co/docs/transformers/v5.13.1/serve-cli/serving).

The supported runtime has no API that pulls a new model into a running
server. Model weights download when the operator starts `qwen-asr-serve`.
The browser shows the exact install and start command, then polls the three
real readiness endpoints. It says that it is waiting. It shows no percentage
or download stage because the runtime provides neither through HTTP.

The default endpoint is `http://127.0.0.1:8000`. `ZORP_VOICE_URL` overrides
it. The default model is `Qwen/Qwen3-ASR-0.6B`. `ZORP_VOICE_MODEL` overrides
it. Neither value comes from a flavor manifest.

Zorp can construct the `qwen-asr-serve` command only for a root HTTP URL. An
HTTPS or path-prefixed URL needs an operator-managed loopback proxy. In that
case the status response explains the proxy requirement and does not offer a
command that would start a different endpoint.

## Loopback boundary

Recorded voice goes to a loopback address or it goes nowhere. `zorp-voice`
enforces the same four layers as `zorp-recall`:

1. The written endpoint is a loopback IP literal or exactly `localhost`, and
   every resolved address is loopback.
2. The HTTP agent uses a resolver that performs no lookup and answers only
   for the checked host and port.
3. Redirects are disabled.
4. Proxy discovery from environment variables is disabled.

There is no cloud provider, fallback, API key, or feature that relaxes this
boundary. Connection-counting canary tests cover redirects, proxies,
off-device configuration, and failed local requests.

## Crate boundary

`zorp-voice` is a workspace crate. It depends on no workspace member and
knows nothing about agents, tools, sessions, or the browser. It owns the
loopback types and the Qwen ASR client.

The client reports runtime and model status and transcribes audio. A
transcription contains editable text and the language tag emitted by
Qwen3-ASR.

## Web server

`zorp-web` has a non-default `voice` Cargo feature. The voice routes are
registered in every build:

- `GET /api/voice/status` reports build support, runtime reachability, model
  presence, and an operator command when zorp can construct one safely.
- `POST /api/voice/wait` polls observed readiness and streams `waiting`,
  `ready`, or `error` events. It requires a JSON content type so another
  origin cannot start it with a simple bodyless POST.
- `POST /api/voice/transcribe` accepts recorded audio and returns the
  transcript and language.

The POST routes answer 501 when the feature is absent. The status route still
explains why voice input is unavailable. `GET /api/capabilities` includes the
same observed status object returned by the status route. It does not repeat
the availability rules.

Audio uploads are bounded and restricted to browser recording media types.
The server forwards the bytes only to the checked local runtime.

## Browser flow

The browser logic lives in `web/src/voice-input.ts`. It uses
`navigator.mediaDevices.getUserMedia` and `MediaRecorder` with no runtime
dependency.

Pressing the microphone checks status first. A build without `voice`, an
unreachable runtime, an insecure context, and denied microphone permission
each produce a visible message. If the runtime or model is absent, the page
shows the command the operator must run. The person can then ask zorp to wait
for real readiness.

While recording, the page says so. The person can stop and transcribe or
cancel and discard the captured bytes. A successful transcript is inserted
at the current selection in the composer and remains ordinary editable text.
It is never sent automatically.

No transcript, language name, error, command, or runtime string is interpreted
as markup. Text nodes use `textContent`, and the transcript uses the
textarea's `value`. Voice input grants no tool, changes no approval, and
bypasses no denylist entry.

## Verification

Rust tests pin the endpoint shape, chat audio body, language parser, observed
readiness, JSON POST requirement, and all four loopback protections. Web tests
cover the insecure context, permission denial, cancellation, operator command,
truncated readiness stream, and untrusted transcript insertion.
