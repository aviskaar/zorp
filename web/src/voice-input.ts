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

  const insertTranscript = (text: string): void => {
    const start = elements.input.selectionStart ?? elements.input.value.length;
    const end = elements.input.selectionEnd ?? start;
    const before = elements.input.value.slice(0, start);
    const after = elements.input.value.slice(end);
    let inserted = text;
    if (before && !/\s$/.test(before) && !/^\s/.test(inserted)) inserted = ` ${inserted}`;
    if (after && !/^\s/.test(after) && !/\s$/.test(inserted)) inserted = `${inserted} `;
    elements.input.value = `${before}${inserted}${after}`;
    const caret = before.length + inserted.length;
    elements.input.setSelectionRange(caret, caret);
    const EventClass = elements.input.ownerDocument.defaultView?.Event ?? Event;
    elements.input.dispatchEvent(new EventClass("input", { bubbles: true }));
    elements.input.focus();
  };

  const finishRecording = async (): Promise<void> => {
    const generation = recordingGeneration;
    const wasCancelled = cancelled;
    const pendingReadiness = readiness;
    readiness = null;
    const recording = new Blob(chunks, {
      type: recorder?.mimeType || chunks[0]?.type || "audio/webm",
    });
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
      insertTranscript(transcript.text);
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
      recorder.addEventListener("dataavailable", (event) => {
        const data = (event as BlobEvent).data;
        if (data?.size) chunks.push(data);
      });
      recorder.addEventListener("stop", () => void finishRecording(), { once: true });
      recorder.start();
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
