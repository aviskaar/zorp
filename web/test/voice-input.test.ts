import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import {
  createVoiceInput,
  type VoiceApi,
  type VoiceEnvironment,
  type VoiceInputElements,
} from "../src/voice-input.ts";
import type { VoiceTranscription } from "../src/api.ts";
import type { VoiceMeter } from "../src/voice-meter.ts";

function fixture(): { window: Window; elements: VoiceInputElements } {
  const dom = new JSDOM(`
    <textarea id="input"></textarea>
    <button id="mic" type="button"></button>
    <button id="cancel" type="button" hidden></button>
    <p id="status" hidden></p>
  `);
  const doc = dom.window.document;
  return {
    window: dom.window as unknown as Window,
    elements: {
      input: doc.querySelector<HTMLTextAreaElement>("#input")!,
      microphone: doc.querySelector<HTMLButtonElement>("#mic")!,
      cancel: doc.querySelector<HTMLButtonElement>("#cancel")!,
      status: doc.querySelector<HTMLElement>("#status")!,
    },
  };
}

function api(overrides: Partial<VoiceApi> = {}): VoiceApi {
  return {
    wait: async (onEvent) => {
      onEvent({ status: "ready", stage: "ready", model: "qwen", detail: "ready" });
    },
    transcribe: async () => ({ text: "hello", language: "English" }),
    ...overrides,
  };
}

function click(window: Window, element: HTMLElement): void {
  element.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
}

test("an insecure context gets a visible explanation", async () => {
  const { window, elements } = fixture();
  createVoiceInput(elements, api(), {
    secureContext: false,
    mediaDevices: undefined,
    MediaRecorder: undefined,
  });
  click(window, elements.microphone);
  await Promise.resolve();
  assert.equal(elements.status.hidden, false);
  assert.match(elements.status.textContent ?? "", /secure|https|localhost/i);
});

test("permission denial is visible", async () => {
  const { window, elements } = fixture();
  let report: ((event: Parameters<Parameters<VoiceApi["wait"]>[0]>[0]) => void) | undefined;
  const environment: VoiceEnvironment = {
    secureContext: true,
    mediaDevices: {
      getUserMedia: async () => {
        throw new window.DOMException("denied", "NotAllowedError");
      },
    },
    MediaRecorder: class {} as typeof MediaRecorder,
  };
  createVoiceInput(
    elements,
    api({
      wait: async (onEvent) => {
        report = onEvent;
        await new Promise(() => {});
      },
    }),
    environment,
  );
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(elements.status.textContent ?? "", /permission/i);
  report?.({ status: "waiting", stage: "downloading_model", model: "qwen", detail: "raw" });
  assert.match(elements.status.textContent ?? "", /permission/i);
});

test("one click requests readiness before permission and records while setup is pending", async () => {
  const { window, elements } = fixture();
  const order: string[] = [];
  let ready: (() => void) | undefined;
  let uploads = 0;
  const voiceApi = api({
    wait: async (onEvent) => {
      order.push("readiness");
      onEvent({ status: "waiting", stage: "installing", model: "qwen", detail: "raw" });
      await new Promise<void>((resolve) => {
        ready = resolve;
      });
      onEvent({ status: "ready", stage: "ready", model: "qwen", detail: "ready" });
    },
    transcribe: async () => {
      uploads++;
      return { text: "hello", language: "English" };
    },
  });
  const { environment } = recordingEnvironment(() => order.push("permission"));
  createVoiceInput(elements, voiceApi, environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(order, ["readiness", "permission"]);
  assert.equal(FakeRecorder.instance.state, "recording");
  FakeRecorder.instance.finalData = new Blob(["audio"], { type: "audio/webm" });
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 0, "audio uploaded before the local model was ready");
  ready?.();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 1);
});

test("readiness stages use fixed short copy instead of server detail", async () => {
  const { window, elements } = fixture();
  const messages: string[] = [];
  createVoiceInput(
    elements,
    api({
      wait: async (onEvent) => {
        for (const stage of [
          "creating_environment",
          "installing",
          "downloading_model",
          "loading",
          "ready",
        ] as const) {
          onEvent({
            status: stage === "ready" ? "ready" : "waiting",
            stage,
            model: "qwen",
            detail: `<raw-${stage}>`,
          });
          messages.push(elements.status.textContent ?? "");
        }
      },
    }),
    recordingEnvironment().environment,
  );

  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(messages, [
    "Creating a private voice environment…",
    "Installing the local voice runtime…",
    "Downloading the Qwen3-ASR model…",
    "Loading the Qwen3-ASR model…",
    "Voice input is ready.",
  ]);
  assert.doesNotMatch(messages.join(" "), /<raw-/);
});

test("setup failure keeps raw detail in the console and shows fixed copy", async () => {
  const { window, elements } = fixture();
  const raw = "pip failed in /private/path";
  const errors: unknown[][] = [];
  const original = console.error;
  console.error = (...values: unknown[]) => errors.push(values);
  try {
    createVoiceInput(
      elements,
      api({
        wait: async (onEvent) => {
          onEvent({ status: "error", stage: "error", model: "qwen", detail: raw });
          throw new Error(raw);
        },
      }),
      recordingEnvironment().environment,
    );
    click(window, elements.microphone);
    await new Promise((resolve) => setTimeout(resolve, 0));
    FakeRecorder.instance.finalData = new Blob(["audio"], { type: "audio/webm" });
    click(window, elements.microphone);
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(
      elements.status.textContent,
      "Voice input is unavailable right now.",
    );
    assert.doesNotMatch(elements.status.textContent ?? "", /pip|private/);
    assert.match(String(errors[0]?.[1]), /pip failed.*private\/path/);
  } finally {
    console.error = original;
  }
});

test("an unavailable configured runtime is one fixed sentence", () => {
  const { elements } = fixture();
  const voice = createVoiceInput(elements, api(), {
    secureContext: true,
    mediaDevices: undefined,
    MediaRecorder: undefined,
  });
  voice.observe({
    available: true,
    runtime_reachable: false,
    model_present: false,
    setup_available: false,
    endpoint: null,
    model: null,
    stage: null,
    detail: "run this command from /private/path",
  });
  assert.equal(elements.status.textContent, "Voice input is unavailable on this machine.");
});

class FakeRecorder extends EventTarget {
  static instance: FakeRecorder;
  static isTypeSupported(): boolean {
    return true;
  }
  readonly mimeType = "audio/webm";
  state: RecordingState = "inactive";
  // Audio to attach to the dataavailable that stop() fires on its own, so a
  // test can seed a non-empty recording without dispatching a "recording"
  // state dataavailable (which now also means a live-preview tick).
  finalData: Blob | null = null;

  constructor(_stream: MediaStream, _options?: MediaRecorderOptions) {
    super();
    FakeRecorder.instance = this;
  }

  start(_timeslice?: number): void {
    this.state = "recording";
  }

  stop(): void {
    this.state = "inactive";
    const event = new Event("dataavailable");
    if (this.finalData) Object.defineProperty(event, "data", { value: this.finalData });
    this.dispatchEvent(event);
    this.dispatchEvent(new Event("stop"));
  }
}

/** Dispatch a mid-recording dataavailable carrying audio, as a live-preview tick would. */
function tick(data: Blob = new Blob(["audio"], { type: "audio/webm" })): void {
  const event = new Event("dataavailable");
  Object.defineProperty(event, "data", { value: data });
  FakeRecorder.instance.dispatchEvent(event);
}

function recordingEnvironment(onPermission: () => void = () => {}): {
  environment: VoiceEnvironment;
  stopped: () => number;
} {
  let tracksStopped = 0;
  const stream = {
    getTracks: () => [{ stop: () => tracksStopped++ }],
  } as unknown as MediaStream;
  return {
    environment: {
      secureContext: true,
      mediaDevices: {
        getUserMedia: async () => {
          onPermission();
          return stream;
        },
      },
      MediaRecorder: FakeRecorder as unknown as typeof MediaRecorder,
    },
    stopped: () => tracksStopped,
  };
}

test("cancel discards the recording without transcribing", async () => {
  const { window, elements } = fixture();
  let uploads = 0;
  const { environment, stopped } = recordingEnvironment();
  createVoiceInput(
    elements,
    api({
      transcribe: async () => {
        uploads++;
        return { text: "should not appear", language: "English" };
      },
    }),
    environment,
  );
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.cancel.hidden, false);
  click(window, elements.cancel);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 0);
  assert.equal(stopped(), 1);
  assert.match(elements.status.textContent ?? "", /cancelled/i);
});

test("a hostile transcript becomes editable textarea text and is not sent", async () => {
  const { window, elements } = fixture();
  const hostile = `<img src=x onerror="globalThis.pwned=true"> مرحبا`;
  let uploads = 0;
  const { environment } = recordingEnvironment();
  elements.input.value = "Before after";
  elements.input.setSelectionRange(7, 7);
  createVoiceInput(
    elements,
    api({
      transcribe: async () => {
        uploads++;
        return { text: hostile, language: "العربية" };
      },
    }),
    environment,
  );
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  FakeRecorder.instance.finalData = new Blob(["audio"], { type: "audio/webm" });
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 1);
  assert.equal(elements.input.value, `Before ${hostile} after`);
  assert.equal(elements.input.ownerDocument.querySelector("img"), null);
  assert.match(elements.status.textContent ?? "", /العربية/);
});

function recordingMeter(): { meter: VoiceMeter; events: string[] } {
  const events: string[] = [];
  return {
    events,
    meter: {
      start: (stream) => {
        events.push(typeof stream?.getTracks === "function" ? "start" : "start with no stream");
      },
      stop: () => events.push("stop"),
    },
  };
}

test("the level meter runs on the live microphone and stops with it", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  const { meter, events } = recordingMeter();
  createVoiceInput(elements, api(), environment, meter);

  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(events, ["start"], "the meter reads the stream the microphone handed over");

  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(events, ["start", "stop"], "the meter stops when the stream is released");
});

test("a denied microphone never starts the meter", async () => {
  const { window, elements } = fixture();
  const { meter, events } = recordingMeter();
  createVoiceInput(
    elements,
    api(),
    {
      secureContext: true,
      mediaDevices: {
        getUserMedia: async () => {
          throw new window.DOMException("denied", "NotAllowedError");
        },
      },
      MediaRecorder: class {} as unknown as typeof MediaRecorder,
    },
    meter,
  );

  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(events.includes("start"), false, "no permission means no meter");
});

test("periodic live ticks replace the previous interim text instead of appending", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let calls = 0;
  const voiceApi = api({
    transcribe: async () => {
      calls++;
      return { text: `interim ${calls}`, language: "English" };
    },
  });
  createVoiceInput(elements, voiceApi, environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  const recordingMessage = elements.status.textContent;

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "interim 1");
  assert.equal(elements.status.textContent, recordingMessage, "a live tick must not touch the status message");

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 2);
  assert.equal(elements.input.value, "interim 2", "the second interim result replaces the first, it does not append");
});

test("a live update is abandoned once the user edits the inserted interim text", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let calls = 0;
  const voiceApi = api({
    transcribe: async () => {
      calls++;
      return { text: `interim ${calls}`, language: "English" };
    },
  });
  createVoiceInput(elements, voiceApi, environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "interim 1");

  // Edit inside the tracked span, not merely after it.
  elements.input.value = "interim EDITED 1";

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 2, "the tick still fires and calls transcribe");
  assert.equal(
    elements.input.value,
    "interim EDITED 1",
    "an edited span must not be overwritten by a stale live result",
  );

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 2, "once abandoned, no further live ticks fire for the rest of this recording");
  assert.equal(elements.input.value, "interim EDITED 1");
});

test("cancelling recording removes the tracked live span from the textarea", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  elements.input.value = "Before after";
  elements.input.setSelectionRange(7, 7);
  createVoiceInput(elements, api(), environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "Before hello after");

  click(window, elements.cancel);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "Before after", "the live preview span is spliced back out on cancel");
  assert.match(elements.status.textContent ?? "", /cancelled/i);
});

test("cancelling leaves the textarea alone when the user has edited the live span", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  createVoiceInput(elements, api(), environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "hello");

  elements.input.value = "Hxllo edited by hand";
  click(window, elements.cancel);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "Hxllo edited by hand", "an edited span is left alone on cancel");
});

test("a failing live tick is silent: no console.error, no status change, no textarea change", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  const errors: unknown[][] = [];
  const original = console.error;
  console.error = (...values: unknown[]) => errors.push(values);
  try {
    const voiceApi = api({
      transcribe: async () => {
        throw new Error("live tick network hiccup");
      },
    });
    createVoiceInput(elements, voiceApi, environment);
    click(window, elements.microphone);
    await new Promise((resolve) => setTimeout(resolve, 0));
    const recordingMessage = elements.status.textContent;

    tick();
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(errors.length, 0, "a failing live tick must not call console.error");
    assert.equal(
      elements.status.textContent,
      recordingMessage,
      "a failing live tick must not change the status message",
    );
    assert.equal(elements.input.value, "", "a failing live tick must not touch the textarea");
  } finally {
    console.error = original;
  }
});

test("a live request in flight suppresses the next tick instead of queuing it", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let calls = 0;
  let resolveFirst: ((value: VoiceTranscription) => void) | undefined;
  const voiceApi = api({
    transcribe: async () => {
      calls++;
      if (calls === 1) {
        return new Promise<VoiceTranscription>((resolve) => {
          resolveFirst = resolve;
        });
      }
      return { text: `interim ${calls}`, language: "English" };
    },
  });
  createVoiceInput(elements, voiceApi, environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  tick(); // starts a slow, in-flight live request
  await new Promise((resolve) => setTimeout(resolve, 0));
  tick(); // arrives while the first is still pending; must be skipped, not queued
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 1, "a tick that arrives mid-flight must be skipped");

  resolveFirst?.({ text: "interim 1", language: "English" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.input.value, "interim 1");

  tick(); // now that the first has resolved, a new tick may fire
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 2);
  assert.equal(elements.input.value, "interim 2");
});

test("final transcribe-on-stop behavior is unchanged when no live ticks ever fire", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let uploads = 0;
  const voiceApi = api({
    transcribe: async () => {
      uploads++;
      return { text: "hello", language: "English" };
    },
  });
  createVoiceInput(elements, voiceApi, environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    elements.status.textContent,
    "Recording. Press the microphone to stop, or cancel to discard it.",
  );

  // Audio arrives only in the final, stop-triggered dataavailable, exactly as
  // it did before live preview existed. No timeslice tick ever fires.
  FakeRecorder.instance.finalData = new Blob(["audio"], { type: "audio/webm" });
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(uploads, 1, "exactly one transcribe call: the final one");
  assert.equal(elements.input.value, "hello");
  assert.equal(
    elements.status.textContent,
    "Transcript ready. Detected language: English. Review it before sending.",
  );
});
