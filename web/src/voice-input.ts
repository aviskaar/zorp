import type { VoiceStatus, VoiceTranscription, VoiceWaitEvent } from "./api";
import type { VoiceMeter } from "./voice-meter";

export interface VoiceApi {
  wait(onEvent: (event: VoiceWaitEvent) => void): Promise<void>;
  transcribe(recording: Blob): Promise<VoiceTranscription>;
}

export interface VoiceInputElements {
  input: HTMLTextAreaElement;
  microphone: HTMLButtonElement;
  cancel: HTMLButtonElement;
  status: HTMLElement;
}

export interface VoiceEnvironment {
  secureContext: boolean;
  mediaDevices?: Pick<MediaDevices, "getUserMedia">;
  MediaRecorder?: typeof MediaRecorder;
}

export interface VoiceInput {
  observe(status: VoiceStatus): void;
}

const MIME_TYPES = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg"];

// ponytail: each live tick re-sends the whole growing clip, not just the new
// audio, so request size and latency grow with recording length. Fine for a
// typical few-second-to-a-minute voice message; chunked transcription of
// fixed-length segments was rejected because it loses accuracy across chunk
// boundaries.
const LIVE_TRANSCRIBE_INTERVAL_MS = 3000;

/** A character range in the composer holding text this module inserted, so a later step can tell whether the user has since edited it. */
interface TrackedSpan {
  start: number;
  end: number;
  text: string;
}

function paddedInsertion(
  value: string,
  start: number,
  end: number,
  text: string,
): { before: string; inserted: string; after: string } {
  const before = value.slice(0, start);
  const after = value.slice(end);
  let inserted = text;
  if (before && !/\s$/.test(before) && !/^\s/.test(inserted)) inserted = ` ${inserted}`;
  if (after && !/^\s/.test(after) && !/\s$/.test(inserted)) inserted = `${inserted} `;
  return { before, inserted, after };
}

export function createVoiceInput(
  elements: VoiceInputElements,
  api: VoiceApi,
  environment: VoiceEnvironment = browserEnvironment(),
  // The level meter is passed in rather than built here, so a caller that
  // draws no meter costs nothing and recording never depends on one.
  meter: VoiceMeter = { start: () => {}, stop: () => {} },
): VoiceInput {
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let chunks: Blob[] = [];
  let cancelled = false;
  let busy = false;
  let readiness: Promise<{ ready: boolean; error?: unknown }> | null = null;
  let readinessGeneration = 0;
  let visibleReadiness = 0;
  let recordingGeneration = 0;
  // The live preview's own state: the last-inserted interim span, whether a
  // live request is already in flight (so the next tick is skipped rather
  // than queued), and whether the user has edited the span away (which
  // retires live preview for the rest of this recording).
  let liveSpan: TrackedSpan | null = null;
  let liveBusy = false;
  let liveAbandoned = false;

  const message = (text: string): void => {
    elements.status.hidden = false;
    elements.status.textContent = text;
  };

  const releaseStream = (): void => {
    // The meter reads the same stream, so it stops wherever the stream does.
    meter.stop();
    if (!stream) return;
    for (const track of stream.getTracks()) track.stop();
    stream = null;
  };

  const idleControls = (): void => {
    elements.microphone.dataset.state = "idle";
    elements.microphone.setAttribute("aria-label", "Record a voice message");
    elements.microphone.title = "Record a voice message";
    elements.cancel.hidden = true;
    recorder = null;
    busy = false;
  };

  const dispatchInputEvent = (): void => {
    const EventClass = elements.input.ownerDocument.defaultView?.Event ?? Event;
    elements.input.dispatchEvent(new EventClass("input", { bubbles: true }));
  };

  // Replaces exactly [start, end) with text (padded to keep a word boundary
  // against whatever surrounds it), moves the caret to the end of what was
  // inserted, and reports the resulting span so a caller can track it.
  const replaceSpan = (start: number, end: number, text: string): TrackedSpan => {
    const { before, inserted, after } = paddedInsertion(elements.input.value, start, end, text);
    elements.input.value = `${before}${inserted}${after}`;
    const caret = before.length + inserted.length;
    elements.input.setSelectionRange(caret, caret);
    dispatchInputEvent();
    return { start: before.length, end: caret, text: inserted };
  };

  const insertTranscript = (text: string): TrackedSpan => {
    const start = elements.input.selectionStart ?? elements.input.value.length;
    const end = elements.input.selectionEnd ?? start;
    const span = replaceSpan(start, end, text);
    elements.input.focus();
    return span;
  };

  const currentBlob = (): Blob =>
    new Blob(chunks, { type: recorder?.mimeType || chunks[0]?.type || "audio/webm" });

  // One periodic, non-final re-transcribe of the whole clip recorded so far.
  // Failures are swallowed on purpose (point 5 of the design): only the
  // final, stop-triggered transcribe is allowed to surface an error.
  const runLiveTick = async (generation: number): Promise<void> => {
    liveBusy = true;
    try {
      const result = await api.transcribe(currentBlob());
      // The recording this tick belongs to may have stopped, been
      // cancelled, or already been superseded by a new recording while the
      // request was in flight. Any of those makes the result stale.
      if (generation !== recordingGeneration || cancelled || recorder?.state !== "recording") return;
      if (liveSpan === null) {
        liveSpan = insertTranscript(result.text);
      } else if (elements.input.value.slice(liveSpan.start, liveSpan.end) === liveSpan.text) {
        liveSpan = replaceSpan(liveSpan.start, liveSpan.end, result.text);
      } else {
        // The user edited the span since the last live update. Abandon live
        // preview for the rest of this recording rather than clobber it.
        liveSpan = null;
        liveAbandoned = true;
      }
    } catch {
      // Silent on purpose: see the function comment above.
    } finally {
      liveBusy = false;
    }
  };

  const finishRecording = async (): Promise<void> => {
    const generation = recordingGeneration;
    const wasCancelled = cancelled;
    const pendingReadiness = readiness;
    readiness = null;
    const spanAtStop = liveSpan;
    liveSpan = null;
    const recording = currentBlob();
    releaseStream();
    idleControls();
    if (wasCancelled) {
      if (visibleReadiness === generation) visibleReadiness = 0;
      void reportBackgroundFailure(pendingReadiness);
      message("Recording cancelled.");
      return;
    }
    if (recording.size === 0) {
      if (visibleReadiness === generation) visibleReadiness = 0;
      void reportBackgroundFailure(pendingReadiness);
      message("No audio was recorded. Try again.");
      return;
    }
    busy = true;
    elements.microphone.disabled = true;
    try {
      const outcome = await pendingReadiness;
      if (visibleReadiness === generation) visibleReadiness = 0;
      if (!outcome?.ready) {
        console.error("voice setup failed", outcome?.error);
        message("Voice input is unavailable right now.");
        return;
      }
      message("Transcribing on this machine…");
      const transcript = await api.transcribe(recording);
      // If a live preview span is still sitting untouched in the composer,
      // the final, authoritative transcript replaces it rather than being
      // appended after it. Otherwise this is exactly today's cursor insert.
      if (
        spanAtStop &&
        elements.input.value.slice(spanAtStop.start, spanAtStop.end) === spanAtStop.text
      ) {
        replaceSpan(spanAtStop.start, spanAtStop.end, transcript.text);
        elements.input.focus();
      } else {
        insertTranscript(transcript.text);
      }
      message(`Transcript ready. Detected language: ${transcript.language}. Review it before sending.`);
    } catch (error) {
      console.error("voice transcription failed", error);
      message("The recording could not be transcribed. Try again.");
    } finally {
      elements.microphone.disabled = false;
      busy = false;
    }
  };

  const startRecording = async (): Promise<void> => {
    if (!environment.secureContext) {
      message("Microphone access needs a secure context. Open zorp-web on localhost or over HTTPS.");
      return;
    }
    if (!environment.mediaDevices?.getUserMedia || !environment.MediaRecorder) {
      message("This browser does not provide microphone recording here.");
      return;
    }
    busy = true;
    elements.microphone.disabled = true;
    const generation = ++readinessGeneration;
    visibleReadiness = generation;
    recordingGeneration = generation;
    let readinessError: unknown;
    readiness = api
      .wait((event) => {
        if (visibleReadiness === generation) message(stageMessage(event.stage));
        if (event.status === "error") readinessError = event.detail;
      })
      .then(
        () =>
          readinessError === undefined
            ? { ready: true }
            : { ready: false, error: readinessError },
        (error) => ({ ready: false, error }),
      );
    try {
      stream = await environment.mediaDevices.getUserMedia({ audio: true });
      // Show the level meter as soon as the microphone is live, so the page
      // says it is listening while the runtime is still waking up.
      meter.start(stream);
      const Recorder = environment.MediaRecorder;
      const mimeType = MIME_TYPES.find((type) => Recorder.isTypeSupported?.(type));
      recorder = mimeType ? new Recorder(stream, { mimeType }) : new Recorder(stream);
      chunks = [];
      cancelled = false;
      liveSpan = null;
      liveBusy = false;
      liveAbandoned = false;
      recorder.addEventListener("dataavailable", (event) => {
        const data = (event as BlobEvent).data;
        if (data?.size) chunks.push(data);
        // A tick fired by the timeslice while still actively recording (not
        // the final dataavailable stop() fires, whose state is already
        // "inactive" by the time it arrives). Skip rather than queue if a
        // live request is already in flight or live preview was abandoned.
        if (recorder?.state === "recording" && !cancelled && !liveBusy && !liveAbandoned) {
          void runLiveTick(generation);
        }
      });
      recorder.addEventListener("stop", () => void finishRecording(), { once: true });
      recorder.start(LIVE_TRANSCRIBE_INTERVAL_MS);
      elements.microphone.dataset.state = "recording";
      elements.microphone.setAttribute("aria-label", "Stop and transcribe recording");
      elements.microphone.title = "Stop and transcribe recording";
      elements.cancel.hidden = false;
      message("Recording. Press the microphone to stop, or cancel to discard it.");
    } catch (error) {
      if (visibleReadiness === generation) visibleReadiness = 0;
      releaseStream();
      if (
        typeof error === "object" &&
        error !== null &&
        "name" in error &&
        error.name === "NotAllowedError"
      ) {
        message("Microphone permission was denied. Allow it in the browser and try again.");
      } else {
        console.error("voice recording failed", error);
        message("Audio could not be recorded. Try again.");
      }
      void reportBackgroundFailure(readiness);
      readiness = null;
    } finally {
      elements.microphone.disabled = false;
      busy = false;
    }
  };

  const observe = (status: VoiceStatus): void => {
    if (!status.available) {
      message("Voice input is off in this zorp-web build.");
      return;
    }
    if (!status.runtime_reachable) {
      message(
        status.setup_available
          ? "Voice input will be prepared on this machine when you use the microphone."
          : "Voice input is unavailable on this machine.",
      );
      return;
    }
    if (!status.model_present) {
      message("The local runtime is not serving the configured Qwen3-ASR model.");
      return;
    }
    message("Voice input is ready.");
  };

  elements.microphone.addEventListener("click", () => {
    if (recorder?.state === "recording") {
      recorder.stop();
      return;
    }
    if (!busy) void startRecording();
  });

  elements.cancel.addEventListener("click", () => {
    if (!recorder || recorder.state !== "recording") return;
    cancelled = true;
    releaseStream();
    // Splice out a still-untouched live preview span rather than leaving it
    // behind; an edited span is left alone.
    if (liveSpan && elements.input.value.slice(liveSpan.start, liveSpan.end) === liveSpan.text) {
      const before = elements.input.value.slice(0, liveSpan.start);
      const after = elements.input.value.slice(liveSpan.end);
      elements.input.value = `${before}${after}`;
      dispatchInputEvent();
    }
    liveSpan = null;
    recorder.stop();
  });

  return { observe };
}

function browserEnvironment(): VoiceEnvironment {
  return {
    secureContext: window.isSecureContext === true,
    mediaDevices: navigator.mediaDevices,
    MediaRecorder: window.MediaRecorder,
  };
}

function stageMessage(stage: VoiceWaitEvent["stage"]): string {
  switch (stage) {
    case "creating_environment":
      return "Creating a private voice environment…";
    case "installing":
      return "Installing the local voice runtime…";
    case "downloading_model":
      return "Downloading the Qwen3-ASR model…";
    case "loading":
      return "Loading the Qwen3-ASR model…";
    case "ready":
      return "Voice input is ready.";
    case "error":
      return "Voice input is unavailable right now.";
  }
}

async function reportBackgroundFailure(
  pending: Promise<{ ready: boolean; error?: unknown }> | null,
): Promise<void> {
  const outcome = await pending;
  if (outcome && !outcome.ready) console.error("voice setup failed", outcome.error);
}
