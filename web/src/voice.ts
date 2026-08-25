/**
 * Speaking into the composer.
 *
 * The rule this module exists to keep: a transcript is a draft, never an
 * instruction. It lands in the composer, the human reads it, and the human
 * presses send. `insertTranscript` is the only thing here that touches the
 * composer and it cannot submit anything, which is what makes that a
 * property of the code rather than a promise in a comment.
 *
 * Where the audio goes: the browser records it, converts it here, and posts
 * it to the local zorp-web server, which forwards it to whatever
 * transcription endpoint the user configured. Nothing in this file talks to
 * a third party, and there is no fallback that would. The browser's own
 * SpeechRecognition would have been less code and it uploads audio to the
 * browser vendor, which is the one thing zorp says it does not do.
 *
 * Nothing here writes audio anywhere. The recorder's chunks live in a local
 * that goes out of scope when the upload finishes, and there is no
 * createObjectURL, no download, and no storage call in this file.
 */

/** What whisper wants, so the conversion happens once, here, in the browser. */
const TARGET_SAMPLE_RATE = 16000;

/**
 * How long one recording may run before it stops itself.
 *
 * A microphone left open because someone walked away is a worse outcome
 * than a dictation cut short, and the second is recoverable.
 */
export const MAX_RECORDING_MS = 120_000;

/** A live microphone, narrowed to the one thing this module does with it. */
export interface MicStream {
  getTracks(): Array<{ stop(): void }>;
}

export interface RecordedAudio {
  bytes: ArrayBuffer;
  /** The container the browser chose. webm/opus on Chrome, mp4 on Safari. */
  type: string;
}

/** A recording already in progress. Created started, because it always is. */
export interface Recorder {
  stop(): Promise<RecordedAudio>;
}

/** What decides whether the microphone can be offered at all. */
export interface VoiceEnvironment {
  /** getUserMedia needs one. http://127.0.0.1 counts, other plain HTTP does not. */
  secureContext: boolean;
  hasMediaDevices: boolean;
  /** Whether the server has a transcription endpoint to forward audio to. */
  transcribeConfigured: boolean;
}

/**
 * Why the microphone is unavailable, and which fix to offer. `action` is
 * what the caller turns into a button: "settings" opens the settings panel,
 * "address" is about the URL this page was loaded from and no button can
 * fix it from here.
 */
export interface Blocked {
  reason: string;
  action: "settings" | "address";
}

export type VoiceState = "idle" | "recording" | "transcribing";

export interface VoiceDeps {
  environment(): VoiceEnvironment;
  openMicrophone(): Promise<MicStream>;
  record(stream: MicStream): Recorder;
  /** Decode whatever the recorder produced down to 16 kHz mono samples. */
  toPcm16k(bytes: ArrayBuffer, type: string): Promise<Float32Array>;
  transcribe(wav: Uint8Array): Promise<string>;
  setTimer(fn: () => void, ms: number): number;
  clearTimer(id: number): void;
}

export interface VoiceHandlers {
  onState(state: VoiceState): void;
  /** Text the user should read. Never text to act on. */
  onTranscript(text: string): void;
  onProblem(message: string): void;
}

/**
 * Whether a page can dictate, and what to say when it cannot.
 *
 * The order matters. Without a secure context the browser will not hand
 * over a microphone at all, so telling someone to go and configure a
 * transcription server is sending them to do work they cannot use.
 */
export function microphoneBlocked(env: VoiceEnvironment): Blocked | null {
  if (!env.secureContext || !env.hasMediaDevices) {
    return {
      reason:
        "The browser only gives a page the microphone over HTTPS or on " +
        "localhost, and this page is on neither. Open zorp at " +
        "http://127.0.0.1:7777 on the machine running the server, or put " +
        "the server behind HTTPS.",
      action: "address",
    };
  }
  if (!env.transcribeConfigured) {
    return {
      reason:
        "Speech is transcribed on your machine, and no transcription " +
        "endpoint is configured yet, so there is nothing to transcribe it " +
        "with. Set one under Speech to text in settings.",
      action: "settings",
    };
  }
  return null;
}

/**
 * One dictation at a time, from pressing the microphone to text arriving.
 *
 * Deliberately knows nothing about the composer, the form, or the send
 * button. It reports a transcript and the caller decides what to do with
 * it, which is why no path through this class can send a message.
 */
export class VoiceInput {
  private readonly deps: VoiceDeps;
  private readonly handlers: VoiceHandlers;
  private current: VoiceState = "idle";
  private recorder: Recorder | null = null;
  private stream: MicStream | null = null;
  private timer: number | null = null;

  constructor(deps: VoiceDeps, handlers: VoiceHandlers) {
    this.deps = deps;
    this.handlers = handlers;
  }

  get state(): VoiceState {
    return this.current;
  }

  /** Open the microphone. Only ever called from a click. */
  async start(): Promise<void> {
    if (this.current !== "idle") {
      return;
    }
    const blocked = microphoneBlocked(this.deps.environment());
    if (blocked) {
      this.handlers.onProblem(blocked.reason);
      return;
    }

    let stream: MicStream;
    try {
      stream = await this.deps.openMicrophone();
    } catch (error) {
      this.handlers.onProblem(`Could not open the microphone: ${describe(error)}`);
      return;
    }

    try {
      this.recorder = this.deps.record(stream);
    } catch (error) {
      release(stream);
      this.handlers.onProblem(`Could not start recording: ${describe(error)}`);
      return;
    }

    this.stream = stream;
    this.timer = this.deps.setTimer(() => {
      this.handlers.onProblem(
        `Recording stopped at the ${Math.round(MAX_RECORDING_MS / 1000)} second limit.`,
      );
      void this.stop();
    }, MAX_RECORDING_MS);
    this.setState("recording");
  }

  /** Finish the recording and turn it into text. */
  async stop(): Promise<void> {
    if (this.current !== "recording") {
      return;
    }
    // Taken and cleared before the first await, so a second stop, a cancel,
    // or the cap firing mid-flight finds nothing left to send again.
    const recorder = this.recorder;
    const stream = this.stream;
    this.recorder = null;
    this.stream = null;
    this.clearTimer();
    this.setState("transcribing");

    try {
      let recorded: RecordedAudio;
      try {
        recorded = await recorder!.stop();
      } finally {
        // The moment the recorder has flushed, whatever happens next. This
        // is what turns off the browser and operating system indicators.
        release(stream);
      }
      const samples = await this.deps.toPcm16k(recorded.bytes, recorded.type);
      const text = (await this.deps.transcribe(wavFromPcm(samples))).trim();
      if (!text) {
        this.handlers.onProblem("Nothing was recognised in that recording.");
      } else {
        this.handlers.onTranscript(text);
      }
    } catch (error) {
      this.handlers.onProblem(describe(error));
    } finally {
      this.setState("idle");
    }
  }

  /** Throw the recording away. Nothing is decoded, uploaded, or reported. */
  cancel(): void {
    if (this.current !== "recording") {
      return;
    }
    const recorder = this.recorder;
    const stream = this.stream;
    this.recorder = null;
    this.stream = null;
    this.clearTimer();
    release(stream);
    // Stopping the recorder is housekeeping, not a step: its result is
    // dropped on the floor on purpose, including when it fails.
    void recorder?.stop().catch(() => undefined);
    this.setState("idle");
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      this.deps.clearTimer(this.timer);
      this.timer = null;
    }
  }

  private setState(state: VoiceState): void {
    this.current = state;
    this.handlers.onState(state);
  }
}

function release(stream: MicStream | null): void {
  for (const track of stream?.getTracks() ?? []) {
    track.stop();
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Put a transcript in the composer, and stop.
 *
 * This function is the whole safety story of the feature. It has no
 * reference to the form and calls nothing that could submit one: no
 * requestSubmit, no click, and no synthesised keydown, because the composer
 * sends on Enter and a dispatched key event would be indistinguishable from
 * a person pressing it. The only event it fires is `input`, which the
 * composer uses to resize itself.
 */
export function insertTranscript(input: HTMLTextAreaElement, text: string): void {
  const transcript = text.trim();
  if (!transcript) {
    return;
  }
  const existing = input.value;
  const separator = !existing || /\s$/.test(existing) ? "" : " ";
  input.value = existing + separator + transcript;
  input.selectionStart = input.value.length;
  input.selectionEnd = input.value.length;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.focus();
}

/**
 * 16 bit PCM samples wrapped in a WAV header.
 *
 * The format gap this closes: MediaRecorder produces webm/opus on Chrome
 * and Firefox and mp4/aac on Safari, and whisper.cpp's server decodes
 * neither unless it was built and started with ffmpeg. Converting here
 * means what leaves the browser is exactly what whisper reads, so a plain
 * `whisper-server` with no extra tooling works.
 */
export function wavFromPcm(
  samples: Float32Array,
  sampleRate: number = TARGET_SAMPLE_RATE,
): Uint8Array {
  const dataBytes = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataBytes);
  const view = new DataView(buffer);

  ascii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataBytes, true);
  ascii(view, 8, "WAVE");
  ascii(view, 12, "fmt ");
  view.setUint32(16, 16, true); // fmt chunk length
  view.setUint16(20, 1, true); // uncompressed PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // bytes per second
  view.setUint16(32, 2, true); // bytes per frame
  view.setUint16(34, 16, true); // bits per sample
  ascii(view, 36, "data");
  view.setUint32(40, dataBytes, true);

  for (let i = 0; i < samples.length; i += 1) {
    // Clamped, not wrapped. A sample past full scale that wraps turns a
    // loud syllable into the opposite-sign spike whisper hears as a click.
    const sample = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, Math.round(sample * (sample < 0 ? 32768 : 32767)), true);
  }
  return new Uint8Array(buffer);
}

function ascii(view: DataView, at: number, text: string): void {
  for (let i = 0; i < text.length; i += 1) {
    view.setUint8(at + i, text.charCodeAt(i));
  }
}

/* ------------------------------------------------------------------ */
/* the real browser                                                    */
/* ------------------------------------------------------------------ */

/**
 * The implementations that touch actual browser APIs, kept in one place so
 * everything above stays testable in jsdom, which has no media stack.
 */
export function browserDeps(options: {
  transcribe(wav: Uint8Array): Promise<string>;
  transcribeConfigured(): boolean;
}): VoiceDeps {
  return {
    environment: () => ({
      secureContext: typeof window !== "undefined" && window.isSecureContext === true,
      hasMediaDevices:
        typeof navigator !== "undefined" &&
        typeof navigator.mediaDevices?.getUserMedia === "function",
      transcribeConfigured: options.transcribeConfigured(),
    }),
    openMicrophone: () =>
      navigator.mediaDevices.getUserMedia({
        // Mono at the source: the file is going to end up mono anyway, and
        // a browser that can do it in hardware does it better than a
        // downmix afterwards.
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
      }),
    record: recordStream,
    toPcm16k: decodeToPcm16k,
    transcribe: options.transcribe,
    setTimer: (fn, ms) => window.setTimeout(fn, ms),
    clearTimer: (id) => window.clearTimeout(id),
  };
}

function recordStream(stream: MicStream): Recorder {
  const recorder = new MediaRecorder(stream as unknown as MediaStream, recorderOptions());
  let chunks: Blob[] = [];
  recorder.addEventListener("dataavailable", (event) => {
    if (event.data.size > 0) {
      chunks.push(event.data);
    }
  });
  recorder.start();

  return {
    async stop(): Promise<RecordedAudio> {
      const type = recorder.mimeType || "audio/webm";
      if (recorder.state !== "inactive") {
        await new Promise<void>((resolve) => {
          recorder.addEventListener("stop", () => resolve(), { once: true });
          recorder.stop();
        });
      }
      const blob = new Blob(chunks, { type });
      // The only reference to the recording, dropped as soon as the bytes
      // are read out. Nothing else in this file ever held one.
      chunks = [];
      return { bytes: await blob.arrayBuffer(), type };
    },
  };
}

/** Whatever this browser will actually record. Chrome and Firefox take the
 * first, Safari falls through to mp4, and an unknown browser gets its own
 * default rather than a rejected constructor. */
function recorderOptions(): MediaRecorderOptions {
  for (const mimeType of ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"]) {
    if (
      typeof MediaRecorder.isTypeSupported === "function" &&
      MediaRecorder.isTypeSupported(mimeType)
    ) {
      return { mimeType };
    }
  }
  return {};
}

/**
 * Decode the recording and resample it to 16 kHz mono.
 *
 * Both steps are the browser's own audio stack, which already ships a
 * decoder for whatever its own MediaRecorder produced. That is why this
 * feature needs no library: the conversion nobody wants to write by hand is
 * already installed.
 */
async function decodeToPcm16k(bytes: ArrayBuffer, _type: string): Promise<Float32Array> {
  const Ctor =
    window.AudioContext ??
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) {
    throw new Error("This browser has no audio decoder, so speech cannot be converted.");
  }
  const context = new Ctor();
  let decoded: AudioBuffer;
  try {
    // decodeAudioData detaches the buffer it is given, and the caller still
    // owns the original.
    decoded = await context.decodeAudioData(bytes.slice(0));
  } finally {
    void context.close();
  }

  const frames = Math.max(1, Math.ceil(decoded.duration * TARGET_SAMPLE_RATE));
  // A multi channel source connected to a mono destination is downmixed by
  // the graph, so this resamples and folds to mono in one pass.
  const offline = new OfflineAudioContext(1, frames, TARGET_SAMPLE_RATE);
  const source = offline.createBufferSource();
  source.buffer = decoded;
  source.connect(offline.destination);
  source.start();
  const rendered = await offline.startRendering();
  return rendered.getChannelData(0);
}
