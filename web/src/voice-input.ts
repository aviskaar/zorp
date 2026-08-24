import type { VoiceStatus, VoiceTranscription, VoiceWaitEvent } from "./api";

export interface VoiceApi {
  status(): Promise<VoiceStatus>;
  wait(onEvent: (event: VoiceWaitEvent) => void): Promise<void>;
  transcribe(recording: Blob): Promise<VoiceTranscription>;
}

export interface VoiceInputElements {
  input: HTMLTextAreaElement;
  microphone: HTMLButtonElement;
  cancel: HTMLButtonElement;
  status: HTMLElement;
  download: HTMLButtonElement;
  command: HTMLElement;
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
): VoiceInput {
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let chunks: Blob[] = [];
  let cancelled = false;
  let busy = false;

  const message = (text: string): void => {
    elements.status.hidden = false;
    elements.status.textContent = text;
  };

  const releaseStream = (): void => {
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
    const wasCancelled = cancelled;
    const recording = new Blob(chunks, {
      type: recorder?.mimeType || chunks[0]?.type || "audio/webm",
    });
    releaseStream();
    idleControls();
    if (wasCancelled) {
      message("Recording cancelled.");
      return;
    }
    if (recording.size === 0) {
      message("No audio was recorded. Try again.");
      return;
    }
    busy = true;
    elements.microphone.disabled = true;
    message("Transcribing on this machine…");
    try {
      const transcript = await api.transcribe(recording);
      insertTranscript(transcript.text);
      message(`Transcript ready. Detected language: ${transcript.language}. Review it before sending.`);
    } catch (error) {
      message(`Could not transcribe the recording: ${describe(error)}`);
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
    busy = true;
    elements.microphone.disabled = true;
    try {
      const status = await api.status();
      observe(status);
      if (!status.available || !status.runtime_reachable || !status.model_present) return;
      if (!environment.mediaDevices?.getUserMedia || !environment.MediaRecorder) {
        message("This browser does not provide microphone recording here.");
        return;
      }
      stream = await environment.mediaDevices.getUserMedia({ audio: true });
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
      releaseStream();
      if (
        typeof error === "object" &&
        error !== null &&
        "name" in error &&
        error.name === "NotAllowedError"
      ) {
        message("Microphone permission was denied. Allow it in the browser and try again.");
      } else {
        message(`Could not start recording: ${describe(error)}`);
      }
    } finally {
      elements.microphone.disabled = false;
      busy = false;
    }
  };

  const observe = (status: VoiceStatus): void => {
    elements.download.hidden = true;
    elements.command.hidden = true;
    elements.command.textContent = "";
    if (!status.available) {
      message(status.detail || "Voice input is off. Rebuild zorp-web with the voice feature.");
      return;
    }
    if (!status.runtime_reachable || !status.model_present) {
      message(status.detail || `Start the local runtime for ${status.model ?? "Qwen3-ASR"}.`);
      if (status.command) {
        elements.command.textContent = status.command;
        elements.command.hidden = false;
      }
      elements.download.textContent = "I started it. Wait for the model";
      elements.download.hidden = false;
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

  elements.download.addEventListener("click", () => {
    if (busy) return;
    busy = true;
    elements.download.disabled = true;
    message("Waiting for the local Qwen3-ASR model…");
    void api
      .wait((event) => {
        if (event.status === "ready") {
          elements.download.hidden = true;
          elements.command.hidden = true;
          message("Voice model ready. Press the microphone to record.");
        } else if (event.status === "error") {
          message(`The local runtime could not report readiness: ${event.detail}`);
        } else {
          message(`Waiting for the local Qwen3-ASR model. ${event.detail}`);
        }
      })
      .catch((error) => {
        message(`Could not wait for the voice model: ${describe(error)}`);
      })
      .finally(() => {
        elements.download.disabled = false;
        busy = false;
      });
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

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
