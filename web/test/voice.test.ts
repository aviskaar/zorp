/**
 * Voice input.
 *
 * The load-bearing test in this file is "a transcript never sends the
 * message". zorp runs shell commands and edits files. A misheard sentence
 * that becomes an instruction without a human reading it first is the one
 * failure this feature is not allowed to have, so the transcript goes into
 * the composer and stops there.
 *
 * The rest of the file is about the microphone itself: it never opens
 * without a deliberate call, it is released the moment recording ends
 * whatever else goes wrong, and no audio outlives the request that carried
 * it.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  MAX_RECORDING_MS,
  VoiceInput,
  insertTranscript,
  microphoneBlocked,
  wavFromPcm,
} from "../src/voice.ts";

// Same arrangement as the other suites: hand the real source the globals a
// browser would give it, so it runs unmodified. `Event` is here as well as
// `document` because node has an `Event` of its own and jsdom rejects it.
const shared = new JSDOM("<!doctype html><body></body>");
(globalThis as Record<string, unknown>).document = shared.window.document;
(globalThis as Record<string, unknown>).Event = shared.window.Event;

/** Let every already-resolved promise in the chain settle. */
const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

/* ------------------------------------------------------------------ */
/* the composer                                                        */
/* ------------------------------------------------------------------ */

/** A composer shaped like the real one: a form, a textarea, a submit button. */
function composer() {
  const doc = shared.window.document;
  const form = doc.createElement("form");
  const input = doc.createElement("textarea");
  const send = doc.createElement("button");
  send.type = "submit";
  form.append(input, send);
  doc.body.append(form);

  let submits = 0;
  const keys: string[] = [];
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    submits += 1;
  });
  input.addEventListener("keydown", (event) => keys.push(event.key));

  return {
    form,
    input,
    submits: () => submits,
    keys: () => keys,
  };
}

test("a transcript lands in the composer", () => {
  const { input } = composer();
  insertTranscript(input, "run the tests");
  assert.equal(input.value, "run the tests");
});

/*
 * The one that matters. Breaking it means a sentence the microphone
 * misheard reaches an agent that can run `rm`.
 */
test("a transcript never sends the message", () => {
  const box = composer();
  insertTranscript(box.input, "delete every file in this directory");
  assert.equal(box.submits(), 0, "the transcript submitted the composer");
  assert.deepEqual(box.keys(), [], "the transcript synthesised a keypress");
});

test("a transcript is added to what is already typed, not swapped for it", () => {
  const { input } = composer();
  input.value = "in web/src,";
  insertTranscript(input, "rename this function");
  assert.equal(input.value, "in web/src, rename this function");
});

test("a transcript that follows a newline does not gain a stray space", () => {
  const { input } = composer();
  input.value = "first line\n";
  insertTranscript(input, "second line");
  assert.equal(input.value, "first line\nsecond line");
});

test("a transcript fires input so the composer resizes", () => {
  const { input } = composer();
  let inputs = 0;
  input.addEventListener("input", () => {
    inputs += 1;
  });
  insertTranscript(input, "hello");
  assert.equal(inputs, 1);
});

test("an empty transcript changes nothing", () => {
  const { input } = composer();
  input.value = "typed by hand";
  insertTranscript(input, "   ");
  assert.equal(input.value, "typed by hand");
});

/* ------------------------------------------------------------------ */
/* the microphone                                                      */
/* ------------------------------------------------------------------ */

interface Recorded {
  bytes: ArrayBuffer;
  type: string;
}

/**
 * A whole voice stack with nothing real in it. Every call is counted, so a
 * test can assert on what was never done as easily as on what was.
 */
function stubbed(overrides: Record<string, unknown> = {}) {
  const opened: Array<{ stopped: number }> = [];
  const uploads: Uint8Array[] = [];
  const states: string[] = [];
  const transcripts: string[] = [];
  const problems: string[] = [];
  let timer: (() => void) | null = null;
  let timerMs = 0;
  let cleared = 0;
  // One distinct sample value per recording, so a test can tell which
  // recording an upload came from.
  let recording = 0;

  const deps = {
    environment: () => ({
      secureContext: true,
      hasMediaDevices: true,
      transcribeConfigured: true,
    }),
    openMicrophone: async () => {
      const tracks = [{ stopped: 0 }];
      opened.push(tracks[0]);
      return {
        getTracks: () => [
          {
            stop: () => {
              tracks[0].stopped += 1;
            },
          },
        ],
      };
    },
    record: () => {
      recording += 1;
      const mark = recording;
      return {
        stop: async (): Promise<Recorded> => ({
          bytes: new Uint8Array([mark]).buffer,
          type: "audio/webm",
        }),
      };
    },
    toPcm16k: async (bytes: ArrayBuffer) => {
      // Carry the recording's marker through as a sample value.
      const mark = new Uint8Array(bytes)[0] ?? 0;
      return new Float32Array([mark / 100]);
    },
    transcribe: async (wav: Uint8Array) => {
      uploads.push(wav);
      return "transcribed words";
    },
    setTimer: (fn: () => void, ms: number) => {
      timer = fn;
      timerMs = ms;
      return 1;
    },
    clearTimer: () => {
      cleared += 1;
      timer = null;
    },
    ...overrides,
  };

  const voice = new VoiceInput(deps as never, {
    onState: (state: string) => states.push(state),
    onTranscript: (text: string) => transcripts.push(text),
    onProblem: (message: string) => problems.push(message),
  });

  return {
    voice,
    opened,
    uploads,
    states,
    transcripts,
    problems,
    fireTimer: () => timer?.(),
    timerMs: () => timerMs,
    cleared: () => cleared,
  };
}

test("constructing VoiceInput touches no microphone", () => {
  const rig = stubbed();
  assert.equal(rig.opened.length, 0, "a microphone opened without being asked");
  assert.equal(rig.voice.state, "idle");
});

test("start opens the microphone and says it is recording", async () => {
  const rig = stubbed();
  await rig.voice.start();
  assert.equal(rig.opened.length, 1);
  assert.equal(rig.voice.state, "recording");
  assert.deepEqual(rig.states, ["recording"]);
});

test("start twice does not open a second microphone", async () => {
  const rig = stubbed();
  await rig.voice.start();
  await rig.voice.start();
  assert.equal(rig.opened.length, 1);
});

test("stop releases the microphone before returning", async () => {
  const rig = stubbed();
  await rig.voice.start();
  await rig.voice.stop();
  assert.equal(rig.opened[0].stopped, 1, "the microphone track was left running");
  assert.equal(rig.voice.state, "idle");
  assert.deepEqual(rig.transcripts, ["transcribed words"]);
});

test("the microphone is released even when transcription fails", async () => {
  const rig = stubbed({
    transcribe: async () => {
      throw new Error("no transcription server");
    },
  });
  await rig.voice.start();
  await rig.voice.stop();
  assert.equal(rig.opened[0].stopped, 1, "a failure left the microphone open");
  assert.equal(rig.voice.state, "idle");
  assert.equal(rig.problems.length, 1);
  assert.match(rig.problems[0], /no transcription server/);
});

test("the microphone is released even when the audio cannot be decoded", async () => {
  const rig = stubbed({
    toPcm16k: async () => {
      throw new Error("this browser could not decode the recording");
    },
  });
  await rig.voice.start();
  await rig.voice.stop();
  assert.equal(rig.opened[0].stopped, 1);
  assert.equal(rig.uploads.length, 0, "undecodable audio was uploaded anyway");
});

test("cancel releases the microphone and transcribes nothing", async () => {
  const rig = stubbed();
  await rig.voice.start();
  rig.voice.cancel();
  await flush();
  assert.equal(rig.opened[0].stopped, 1);
  assert.equal(rig.uploads.length, 0, "a discarded recording was uploaded");
  assert.deepEqual(rig.transcripts, []);
  assert.equal(rig.voice.state, "idle");
});

test("recording stops itself at the cap rather than running on", async () => {
  const rig = stubbed();
  await rig.voice.start();
  assert.equal(rig.timerMs(), MAX_RECORDING_MS);
  rig.fireTimer();
  await flush();
  assert.equal(rig.opened[0].stopped, 1, "the cap did not release the microphone");
  assert.equal(rig.voice.state, "idle");
  assert.equal(rig.problems.length, 1, "the cap fired without saying so");
});

test("a finished recording clears its own timer", async () => {
  const rig = stubbed();
  await rig.voice.start();
  await rig.voice.stop();
  assert.ok(rig.cleared() > 0, "the cap timer outlived the recording");
});

/* ---- audio is not kept ---- */

test("stopping twice does not upload the same audio twice", async () => {
  const rig = stubbed();
  await rig.voice.start();
  await rig.voice.stop();
  await rig.voice.stop();
  assert.equal(rig.uploads.length, 1, "the recording was still there to send again");
});

test("a second dictation carries none of the first recording's audio", async () => {
  const rig = stubbed();
  await rig.voice.start();
  await rig.voice.stop();
  await rig.voice.start();
  await rig.voice.stop();
  assert.equal(rig.uploads.length, 2);
  assert.notDeepEqual(
    Array.from(rig.uploads[0]),
    Array.from(rig.uploads[1]),
    "the second upload was the first recording again",
  );
});

test("a recording is never turned into a URL something could download", async () => {
  const url = shared.window.URL as unknown as Record<string, unknown>;
  let created = 0;
  const previous = url.createObjectURL;
  url.createObjectURL = () => {
    created += 1;
    return "blob:none";
  };
  (globalThis as Record<string, unknown>).URL = url;
  try {
    const rig = stubbed();
    await rig.voice.start();
    await rig.voice.stop();
    assert.equal(created, 0, "the recording was handed a URL");
  } finally {
    url.createObjectURL = previous;
  }
});

/* ---- failures ---- */

test("a refused microphone is reported and leaves nothing recording", async () => {
  const rig = stubbed({
    openMicrophone: async () => {
      throw new Error("Permission denied");
    },
  });
  await rig.voice.start();
  assert.equal(rig.voice.state, "idle");
  assert.equal(rig.problems.length, 1);
  assert.match(rig.problems[0], /Permission denied/);
});

test("a failed transcription puts nothing in the composer", async () => {
  const box = composer();
  const rig = stubbed({
    transcribe: async () => {
      throw new Error("the endpoint refused");
    },
  });
  await rig.voice.start();
  await rig.voice.stop();
  for (const text of rig.transcripts) insertTranscript(box.input, text);
  assert.equal(box.input.value, "", "a failure still wrote to the composer");
});

test("a transcription that comes back empty is reported, not silently dropped", async () => {
  const rig = stubbed({ transcribe: async () => "   " });
  await rig.voice.start();
  await rig.voice.stop();
  assert.deepEqual(rig.transcripts, [], "whitespace was pushed into the composer");
  assert.equal(rig.problems.length, 1);
});

/* ------------------------------------------------------------------ */
/* whether the microphone can be used at all                           */
/* ------------------------------------------------------------------ */

test("a page without a secure context is told why, before any prompt", () => {
  const blocked = microphoneBlocked({
    secureContext: false,
    hasMediaDevices: false,
    transcribeConfigured: true,
  });
  assert.ok(blocked, "a page with no microphone reported none of this");
  assert.match(blocked.reason, /127\.0\.0\.1|localhost|HTTPS/);
  assert.equal(blocked.action, "address");
});

test("a page with no transcription endpoint is told what is missing", () => {
  const blocked = microphoneBlocked({
    secureContext: true,
    hasMediaDevices: true,
    transcribeConfigured: false,
  });
  assert.ok(blocked);
  assert.equal(blocked.action, "settings");
});

test("a working setup reports no reason to refuse", () => {
  assert.equal(
    microphoneBlocked({
      secureContext: true,
      hasMediaDevices: true,
      transcribeConfigured: true,
    }),
    null,
  );
});

/*
 * The browser cannot reach the microphone at all without a secure context,
 * so that is the answer to give even when the endpoint is also missing.
 * Telling someone to configure a server they then cannot use is worse than
 * telling them nothing.
 */
test("a page with neither is told about the address first", () => {
  const blocked = microphoneBlocked({
    secureContext: false,
    hasMediaDevices: false,
    transcribeConfigured: false,
  });
  assert.equal(blocked?.action, "address");
});

test("start refuses without a secure context and never prompts", async () => {
  const rig = stubbed({
    environment: () => ({
      secureContext: false,
      hasMediaDevices: false,
      transcribeConfigured: true,
    }),
  });
  await rig.voice.start();
  assert.equal(rig.opened.length, 0, "a blocked page still asked for the microphone");
  assert.equal(rig.voice.state, "idle");
  assert.equal(rig.problems.length, 1);
});

/* ------------------------------------------------------------------ */
/* wav encoding                                                        */
/* ------------------------------------------------------------------ */

function readAscii(bytes: Uint8Array, at: number, length: number): string {
  return String.fromCharCode(...bytes.slice(at, at + length));
}

function readU32(bytes: Uint8Array, at: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset).getUint32(at, true);
}

function readU16(bytes: Uint8Array, at: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset).getUint16(at, true);
}

/*
 * MediaRecorder hands over webm/opus, and whisper.cpp's server reads WAV.
 * These assertions are the contract between the two, spelled out, because
 * getting the header wrong produces a file that is accepted and transcribed
 * as noise rather than one that is rejected.
 */
test("the encoder writes a 16 kHz mono 16-bit WAV header", () => {
  const wav = wavFromPcm(new Float32Array(8));
  assert.equal(readAscii(wav, 0, 4), "RIFF");
  assert.equal(readAscii(wav, 8, 4), "WAVE");
  assert.equal(readAscii(wav, 12, 4), "fmt ");
  assert.equal(readU32(wav, 16), 16, "fmt chunk size");
  assert.equal(readU16(wav, 20), 1, "not uncompressed PCM");
  assert.equal(readU16(wav, 22), 1, "not mono");
  assert.equal(readU32(wav, 24), 16000, "not 16 kHz");
  assert.equal(readU32(wav, 28), 32000, "byte rate does not match");
  assert.equal(readU16(wav, 32), 2, "block align does not match");
  assert.equal(readU16(wav, 34), 16, "not 16 bits per sample");
  assert.equal(readAscii(wav, 36, 4), "data");
  assert.equal(readU32(wav, 40), 16, "data length does not match the samples");
  assert.equal(wav.length, 44 + 16);
});

test("the RIFF size covers everything after it", () => {
  const wav = wavFromPcm(new Float32Array(10));
  assert.equal(readU32(wav, 4), wav.length - 8);
});

test("samples are clamped rather than wrapping around", () => {
  const wav = wavFromPcm(new Float32Array([1, -1, 2, -2, 0]));
  const view = new DataView(wav.buffer, wav.byteOffset);
  assert.equal(view.getInt16(44, true), 32767);
  assert.equal(view.getInt16(46, true), -32768);
  assert.equal(view.getInt16(48, true), 32767, "a sample above 1 wrapped");
  assert.equal(view.getInt16(50, true), -32768, "a sample below -1 wrapped");
  assert.equal(view.getInt16(52, true), 0);
});
