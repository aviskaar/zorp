---
status: accepted
date: 2026-09-03
---

# voice shows that it is listening, and the live transcript is segments through the same loopback endpoint

**Finding:** ZOR-22. Clicking the microphone gave no unmistakable sign that
recording had started. The level meter was the only signal, and the button
itself had been disabled while recording since setup-on-click landed
(74bb6671): `startRecording` disabled it and only re-enabled it when no
recorder existed, so a live recording showed a dimmed microphone that a real
click could not stop, and cancel left it disabled afterwards. The reporter
also wanted dictation: words on the page while speaking, and that text as the
editable prompt when they stop. What was there was #124's live preview, which
re-sent the whole growing clip to the transcribe endpoint every three seconds
and wrote the interim result into the composer itself.

**Decision:** two things. Recording state is now unmistakable. The
microphone becomes a stop button with a pulsing red dot, it carries
`aria-pressed`, the status line reads "Listening" with an elapsed m:ss that
ticks, Escape stops the recording the way the button does, and the meter
stays as one signal of several. The time is visual only, because a polite
live region that counts every second is noise; the region announces
"Listening" once.

The live transcript is segments. The runtime transcribes whole files and
nothing else, so the recorder is stopped and restarted to get one standalone
blob per segment: at the first quiet moment after three seconds, where the
meter reports quiet and 300 ms of it is longer than a gap between words, and
at eight seconds regardless. Each finished segment goes in order, one request
at a time through a client-side queue, to the existing
`/api/voice/transcribe`, and its text joins a preview line under the
composer. The preview is one text node set through `textContent`. A segment
that fails leaves `[unclear]` in its place and the recording goes on. When
the person stops, the last segment is transcribed, the joined text lands in
the composer exactly where a transcript always did, editable and unsent, and
the preview clears. The eight second ceiling can split a word across a
boundary; the fix is streaming ASR on the server, and the ceiling is marked
in the code as such. `zorp-web/src/voice.rs` neither locks nor rejects
overlapping requests, so the one-at-a-time rule lives in the browser. The
first segment waits for readiness the way the single final call did.

**What it ruled out:** streaming ASR now, since the runtime does not offer
it. A cloud provider or fallback, for the reason the 2026-08-24 entry gives.
MediaRecorder timeslice chunks, since a chunk is not a decodable file on its
own, and the whole-clip re-send they forced grew with the recording. Sending
the preview or the joined text automatically.

**Not changed:** the loopback boundary; every request goes to the same
checked route and nowhere else. The meter reads amplitude only; it now also
reports one boolean per frame, whether the level is under its noise floor,
and keeps no sample for it. Transcripts are untrusted editable composer text
that grant no tool, change no approval, and bypass no denylist. The runtime
bootstrap, the endpoint checks, and `GET /api/voice/status`.
