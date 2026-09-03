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
  /** The running transcript while recording. Untrusted text, one text node. */
  preview: HTMLElement;
  toast: HTMLElement;
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

// The live transcript is segments. The runtime only transcribes whole files,
// so the recorder is stopped and restarted to get one standalone blob per
// segment, and each finished segment goes to the same loopback endpoint the
// final recording always went to. A MediaRecorder timeslice is not used on
// purpose: its chunks are not files on their own.
//
// A segment ends at the first quiet moment after this long, so most words
// come out whole.
const MIN_SEGMENT_MS = 3000;
// How long the meter has to read quiet before a moment counts as one. A gap
// between words is shorter than this.
const QUIET_MS = 300;
// ponytail: a segment is cut here whether or not someone is mid-word, so a
// word can still split at a boundary and come back as two halves. The fix is
// streaming ASR on the server, which the runtime does not offer.
const MAX_SEGMENT_MS = 8000;
// What a segment that could not be transcribed leaves in its place.
const UNCLEAR = "[unclear]";

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

/** Elapsed time as m:ss. */
function clock(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

export function createVoiceInput(
  elements: VoiceInputElements,
  api: VoiceApi,
  environment: VoiceEnvironment = browserEnvironment(),
  // The level meter is passed in rather than built here, so a caller that
  // draws no meter costs nothing and recording never depends on one. Without
  // a meter there is no level, so segments end only at the ceiling.
  meter: VoiceMeter = { start: () => {}, stop: () => {} },
): VoiceInput {
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let cancelled = false;
  let stopping = false;
  let busy = false;
  let readiness: Promise<{ ready: boolean; error?: unknown }> | null = null;
  let gate: Promise<{ ready: boolean; error?: unknown }> = Promise.resolve({ ready: true });
  let readinessGeneration = 0;
  let visibleReadiness = 0;
  let recordingGeneration = 0;
  let primed = true;
  // Segments go to the server in order, one request at a time, by extending
  // one promise chain. A result is pushed when its request returns, so
  // `texts` is always the recording so far, in order.
  let texts: string[] = [];
  let language = "";
  let chain: Promise<void> = Promise.resolve();
  let starts = 0;
  let stops = 0;
  let segmentStart = 0;
  let quietSince: number | null = null;
  let forceCut: ReturnType<typeof setTimeout> | null = null;
  let recordingStart = 0;
  let timer: ReturnType<typeof setInterval> | null = null;

  const message = (text: string): void => {
    elements.status.hidden = false;
    elements.status.textContent = text;
  };

  const showToast = (text: string): void => {
    elements.toast.hidden = false;
    elements.toast.textContent = text;
  };

  const hideToast = (): void => {
    elements.toast.hidden = true;
    elements.toast.textContent = "";
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
    elements.microphone.setAttribute("aria-pressed", "false");
    elements.microphone.title = "Record a voice message";
    elements.microphone.disabled = false;
    elements.cancel.hidden = true;
    recorder = null;
    busy = false;
  };

  const recordingControls = (): void => {
    elements.microphone.dataset.state = "recording";
    elements.microphone.setAttribute("aria-label", "Stop and transcribe recording");
    elements.microphone.setAttribute("aria-pressed", "true");
    elements.microphone.title = "Stop and transcribe recording";
    // It is the stop button now, so it has to be clickable.
    elements.microphone.disabled = false;
    elements.cancel.hidden = false;
  };

  const stopTimer = (): void => {
    if (timer === null) return;
    clearInterval(timer);
    timer = null;
  };

  // "Listening" plus a ticking m:ss. The status line is a polite live
  // region and the time changes every second, so the time is visual only:
  // the region announces "Listening" once rather than counting out loud.
  const startTimer = (): void => {
    const document = elements.status.ownerDocument;
    const elapsed = document.createElement("span");
    elapsed.className = "voice-timer";
    elapsed.setAttribute("aria-hidden", "true");
    recordingStart = Date.now();
    const tick = (): void => {
      elapsed.textContent = clock(Date.now() - recordingStart);
    };
    tick();
    elements.status.hidden = false;
    elements.status.replaceChildren(document.createTextNode("Listening "), elapsed);
    timer = setInterval(tick, 1000);
  };

  const dispatchInputEvent = (): void => {
    const EventClass = elements.input.ownerDocument.defaultView?.Event ?? Event;
    elements.input.dispatchEvent(new EventClass("input", { bubbles: true }));
  };

  const insertTranscript = (text: string): void => {
    const start = elements.input.selectionStart ?? elements.input.value.length;
    const end = elements.input.selectionEnd ?? start;
    const { before, inserted, after } = paddedInsertion(elements.input.value, start, end, text);
    elements.input.value = `${before}${inserted}${after}`;
    const caret = before.length + inserted.length;
    elements.input.setSelectionRange(caret, caret);
    dispatchInputEvent();
    elements.input.focus();
  };

  // The preview came from a model that heard a microphone. It is set as one
  // text node and nothing else: no markup, no markdown, nothing run.
  const renderPreview = (): void => {
    const text = texts.join(" ");
    elements.preview.textContent = text;
    elements.preview.hidden = text === "";
  };

  const clearPreview = (): void => {
    texts = [];
    renderPreview();
  };

  const transcribeSegment = async (generation: number, segment: Blob): Promise<void> => {
    if (generation !== recordingGeneration) return;
    let text = UNCLEAR;
    try {
      // The first segment waits for readiness the way the single final call
      // always did; after that the gate is already settled.
      const outcome = await gate;
      if (!outcome.ready) throw outcome.error;
      const result = await api.transcribe(segment);
      text = result.text;
      language = result.language;
    } catch (error) {
      console.error("voice segment failed", error);
    }
    if (generation !== recordingGeneration) return;
    texts.push(text);
    renderPreview();
  };

  const enqueueSegment = (data: Blob): void => {
    const generation = recordingGeneration;
    const segment = new Blob([data], { type: recorder?.mimeType || data.type || "audio/webm" });
    chain = chain.then(() => transcribeSegment(generation, segment));
  };

  const beginSegment = (): void => {
    if (!recorder) return;
    recorder.start();
    starts += 1;
    segmentStart = Date.now();
    quietSince = null;
    if (forceCut !== null) clearTimeout(forceCut);
    forceCut = setTimeout(cut, MAX_SEGMENT_MS);
  };

  // stop() hands the segment over as one standalone blob and start() opens
  // the next one. The gap between them is the recorder's own, a few
  // milliseconds.
  const cut = (): void => {
    if (!recorder || recorder.state !== "recording" || stopping) return;
    recorder.stop();
    beginSegment();
  };

  const onQuiet = (quiet: boolean): void => {
    const now = Date.now();
    if (!quiet) {
      quietSince = null;
      return;
    }
    if (quietSince === null) quietSince = now;
    if (now - segmentStart >= MIN_SEGMENT_MS && now - quietSince >= QUIET_MS) cut();
  };

  const stopRecording = (): void => {
    if (!recorder || recorder.state !== "recording" || stopping) return;
    stopping = true;
    if (forceCut !== null) {
      clearTimeout(forceCut);
      forceCut = null;
    }
    recorder.stop();
  };

  const finishRecording = async (): Promise<void> => {
    const generation = recordingGeneration;
    const wasCancelled = cancelled;
    const pendingReadiness = readiness;
    readiness = null;
    stopTimer();
    releaseStream();
    idleControls();
    if (visibleReadiness === generation) visibleReadiness = 0;
    if (wasCancelled) {
      // Segments still queued or in flight belong to a recording that no
      // longer exists: a stale generation drops them before they are sent,
      // or on return, and nothing lands in the preview after it clears.
      recordingGeneration = 0;
      clearPreview();
      void reportBackgroundFailure(pendingReadiness);
      message("Recording cancelled.");
      return;
    }
    busy = true;
    elements.microphone.disabled = true;
    message("Transcribing on this machine…");
    try {
      // Every segment, the last one included, is already queued, so waiting
      // for the queue is waiting for the whole transcript.
      await chain;
      const transcript = texts.join(" ");
      clearPreview();
      if (transcript === "") {
        void reportBackgroundFailure(pendingReadiness);
        message("No audio was recorded. Try again.");
        return;
      }
      if (language === "") {
        message("The recording could not be transcribed. Try again.");
        return;
      }
      // Exactly where a transcript always landed: editable, at the caret,
      // and never sent from here.
      insertTranscript(transcript);
      message(`Transcript ready. Detected language: ${language}. Review it before sending.`);
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
    if (primed) readiness = Promise.resolve({ ready: true });
    let readinessError: unknown;
    let firstEventResolve: ((event: VoiceWaitEvent) => void) | null = null;
    let firstEventReject: ((error: unknown) => void) | null = null;
    const firstEvent = new Promise<VoiceWaitEvent>((resolve, reject) => {
      firstEventResolve = resolve;
      firstEventReject = reject;
    });
    if (!primed) {
      readiness = api
        .wait((event) => {
          if (visibleReadiness === generation) message(stageMessage(event.stage));
          if (firstEventResolve) {
            firstEventResolve(event);
            firstEventResolve = null;
            firstEventReject = null;
          }
          if (event.status === "error") readinessError = event.detail;
        })
        .then(
          () =>
            readinessError === undefined
              ? { ready: true }
              : { ready: false, error: readinessError },
          (error) => {
            if (firstEventReject) {
              firstEventReject(error);
              firstEventResolve = null;
              firstEventReject = null;
            }
            return { ready: false, error };
          },
        );
    }
    try {
      if (!primed) {
        const initial = await firstEvent;
        showToast(setupToast(initial.stage, initial.status === "ready"));
        const outcome = await readiness;
        if (visibleReadiness === generation) visibleReadiness = 0;
        primed = outcome?.ready === true;
        elements.microphone.disabled = false;
        busy = false;
        if (outcome?.ready) {
          showToast("Voice input is ready. Qwen3-ASR finished preparing on this machine. Click the microphone again to start recording.");
          message("Voice input is ready. Click the microphone again to start recording.");
        } else {
          console.error("voice setup failed", outcome?.error);
          showToast("Voice input could not be prepared on this machine.");
          message("Voice input is unavailable right now.");
        }
        return;
      }
      stream = await environment.mediaDevices.getUserMedia({ audio: true });
      // Show the level meter as soon as the microphone is live, so the page
      // says it is listening while the runtime is still waking up. Its read of
      // the level is also what finds a quiet moment to end a segment on.
      meter.start(stream, onQuiet);
      hideToast();
      const Recorder = environment.MediaRecorder;
      const mimeType = MIME_TYPES.find((type) => Recorder.isTypeSupported?.(type));
      recorder = mimeType ? new Recorder(stream, { mimeType }) : new Recorder(stream);
      cancelled = false;
      stopping = false;
      starts = 0;
      stops = 0;
      language = "";
      gate = readiness ?? gate;
      clearPreview();
      recorder.addEventListener("dataavailable", (event) => {
        // With no timeslice, data arrives once per stop(): one finished
        // segment, a file on its own.
        const data = (event as BlobEvent).data;
        if (data?.size) enqueueSegment(data);
      });
      recorder.addEventListener("stop", () => {
        stops += 1;
        // A cut's stop event can land after the person pressed stop, so the
        // recording is over only once every segment started has ended.
        if (stopping && stops === starts) void finishRecording();
      });
      beginSegment();
      recordingControls();
      startTimer();
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
      if (!recorder) {
        elements.microphone.disabled = false;
        busy = false;
      }
    }
  };

  const observe = (status: VoiceStatus): void => {
    if (!status.available) {
      message("Voice input is off in this zorp-web build.");
      return;
    }
    if (!status.runtime_reachable) {
      primed = false;
      message(
        status.setup_available
          ? "Voice input will be prepared on this machine when you use the microphone."
          : "Voice input is unavailable on this machine.",
      );
      return;
    }
    if (!status.model_present) {
      primed = false;
      message("The local runtime is not serving the configured Qwen3-ASR model.");
      return;
    }
    primed = true;
    message("Voice input is ready.");
  };

  elements.microphone.addEventListener("click", () => {
    if (recorder?.state === "recording") {
      stopRecording();
      return;
    }
    if (!busy) void startRecording();
  });

  elements.cancel.addEventListener("click", () => {
    if (!recorder || recorder.state !== "recording") return;
    cancelled = true;
    stopRecording();
  });

  // Escape stops the recording the same way the button does, from anywhere
  // on the page, since focus may be on the button or in the composer.
  elements.microphone.ownerDocument.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && recorder?.state === "recording") stopRecording();
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

function setupToast(stage: VoiceWaitEvent["stage"], ready: boolean): string {
  if (ready) {
    return "Voice input is ready. Qwen3-ASR finished preparing on this machine. Click the microphone again to start recording.";
  }
  switch (stage) {
    case "creating_environment":
      return "Preparing voice input on this machine. Creating a private runtime for Qwen3-ASR.";
    case "installing":
      return "Preparing voice input on this machine. Installing the local Qwen3-ASR runtime.";
    case "downloading_model":
      return "Preparing voice input on this machine. Downloading the Qwen3-ASR model. This first use can take a few minutes.";
    case "loading":
      return "Preparing voice input on this machine. Loading the Qwen3-ASR model.";
    case "ready":
      return "Voice input is ready. Qwen3-ASR finished preparing on this machine. Click the microphone again to start recording.";
    case "error":
      return "Voice input could not be prepared on this machine.";
  }
}

async function reportBackgroundFailure(
  pending: Promise<{ ready: boolean; error?: unknown }> | null,
): Promise<void> {
  const outcome = await pending;
  if (outcome && !outcome.ready) console.error("voice setup failed", outcome.error);
}
