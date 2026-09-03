import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
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
    <form id="form">
      <textarea id="input"></textarea>
      <button id="mic" type="button"></button>
      <button id="cancel" type="button" hidden></button>
      <p id="status" hidden></p>
      <p id="preview" hidden></p>
      <div id="toast" hidden></div>
    </form>
  `);
  const doc = dom.window.document;
  return {
    window: dom.window as unknown as Window,
    elements: {
      input: doc.querySelector<HTMLTextAreaElement>("#input")!,
      microphone: doc.querySelector<HTMLButtonElement>("#mic")!,
      cancel: doc.querySelector<HTMLButtonElement>("#cancel")!,
      status: doc.querySelector<HTMLElement>("#status")!,
      preview: doc.querySelector<HTMLElement>("#preview")!,
      toast: doc.querySelector<HTMLElement>("#toast")!,
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

/** Let every pending promise settle. A macrotask, so it works under fake timers too. */
const settle = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

/** Fake setTimeout, setInterval and Date for one test; setImmediate stays real for settle(). */
function fakeClock(t: TestContext): (ms: number) => void {
  t.mock.timers.enable({ apis: ["setTimeout", "setInterval", "Date"] });
  return (ms) => t.mock.timers.tick(ms);
}

const audio = (): Blob => new Blob(["audio"], { type: "audio/webm" });

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

test("the first click on an unprepared machine opens setup UI and waits to record until a later click", async () => {
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
  const voice = createVoiceInput(elements, voiceApi, environment);
  voice.observe({
    available: true,
    runtime_reachable: false,
    model_present: false,
    setup_available: true,
    endpoint: null,
    model: null,
    stage: null,
    detail: "not ready yet",
  });
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(order, ["readiness"]);
  assert.equal(elements.toast.hidden, false);
  assert.match(elements.toast.textContent ?? "", /download|prepare|voice/i);
  ready?.();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(elements.toast.textContent ?? "", /ready|click again/i);
  assert.equal(order.includes("permission"), false, "permission should wait until setup is complete");
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(order, ["readiness", "permission"]);
  assert.equal(FakeRecorder.instance.state, "recording");
  FakeRecorder.instance.finalData = audio();
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 1);
});

test("readiness stages use fixed short copy instead of server detail", async () => {
  const { window, elements } = fixture();
  const messages: string[] = [];
  const voice = createVoiceInput(
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
  voice.observe({
    available: true,
    runtime_reachable: false,
    model_present: false,
    setup_available: true,
    endpoint: null,
    model: null,
    stage: null,
    detail: "not ready yet",
  });

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
    const voice = createVoiceInput(
      elements,
      api({
        wait: async (onEvent) => {
          onEvent({ status: "error", stage: "error", model: "qwen", detail: raw });
          throw new Error(raw);
        },
      }),
      recordingEnvironment().environment,
    );
    voice.observe({
      available: true,
      runtime_reachable: false,
      model_present: false,
      setup_available: true,
      endpoint: null,
      model: null,
      stage: null,
      detail: "not ready yet",
    });
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
  /** Every start() call: one per segment, since a cut restarts the recorder. */
  starts = 0;
  timeslices: Array<number | undefined> = [];
  // Audio to attach to the dataavailable that stop() fires on its own. It
  // stays set across restarts, so every segment of a recording carries it.
  finalData: Blob | null = null;

  constructor(_stream: MediaStream, _options?: MediaRecorderOptions) {
    super();
    FakeRecorder.instance = this;
  }

  start(timeslice?: number): void {
    this.state = "recording";
    this.starts += 1;
    this.timeslices.push(timeslice);
  }

  stop(): void {
    this.state = "inactive";
    const event = new Event("dataavailable");
    if (this.finalData) Object.defineProperty(event, "data", { value: this.finalData });
    this.dispatchEvent(event);
    this.dispatchEvent(new Event("stop"));
  }
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
  assert.equal(elements.microphone.disabled, false, "the microphone is usable again after cancel");
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
  FakeRecorder.instance.finalData = audio();
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

/** A meter whose frames the test hands over by hand: speak(true) is a loud frame, speak(false) a quiet one. */
function levelMeter(): { meter: VoiceMeter; speak: (loud: boolean) => void } {
  let listener: ((quiet: boolean) => void) | null = null;
  return {
    meter: {
      start: (_stream, onQuiet) => {
        listener = onQuiet ?? null;
      },
      stop: () => {
        listener = null;
      },
    },
    speak: (loud) => listener?.(!loud),
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

test("the button and status say it is listening, and the timer ticks", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  createVoiceInput(elements, api(), environment);

  click(window, elements.microphone);
  await settle();
  assert.equal(elements.microphone.dataset.state, "recording");
  assert.equal(elements.microphone.getAttribute("aria-pressed"), "true");
  assert.equal(elements.microphone.disabled, false, "it is the stop button now, so it must be clickable");
  assert.match(elements.microphone.getAttribute("aria-label") ?? "", /stop/i);
  assert.equal(elements.status.hidden, false);
  assert.equal(elements.status.textContent, "Listening 0:00");

  tick(1000);
  assert.equal(elements.status.textContent, "Listening 0:01");
  tick(60_000);
  assert.equal(elements.status.textContent, "Listening 1:01");

  FakeRecorder.instance.finalData = audio();
  click(window, elements.microphone);
  await settle();
  assert.equal(elements.microphone.dataset.state, "idle");
  assert.equal(elements.microphone.getAttribute("aria-pressed"), "false");
  assert.equal(elements.microphone.disabled, false);
  assert.equal(
    elements.status.textContent,
    "Transcript ready. Detected language: English. Review it before sending.",
  );
  tick(1000);
  assert.equal(elements.status.textContent?.startsWith("Transcript ready"), true, "the timer stopped with the recording");
});

test("a quiet moment after three seconds cuts a segment and the next one starts at once", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  const { meter, speak } = levelMeter();
  const uploads: Blob[] = [];
  createVoiceInput(
    elements,
    api({
      transcribe: async (segment) => {
        uploads.push(segment);
        return { text: "one", language: "English" };
      },
    }),
    environment,
    meter,
  );
  click(window, elements.microphone);
  await settle();
  const recorder = FakeRecorder.instance;
  recorder.finalData = audio();

  speak(true);
  tick(2500);
  speak(false);
  tick(400);
  speak(false);
  assert.equal(recorder.starts, 1, "quiet before three seconds is not a cut");
  tick(200);
  speak(false);
  assert.equal(recorder.starts, 2, "quiet after three seconds ends the segment");
  assert.equal(recorder.state, "recording", "the next segment is already recording");
  await settle();
  assert.equal(uploads.length, 1, "the finished segment went to transcribe on its own");
  assert.equal(elements.preview.hidden, false);
  assert.equal(elements.preview.textContent, "one");

  // A gap between words is shorter than the quiet threshold and does not cut.
  tick(3000);
  speak(false);
  tick(100);
  speak(false);
  assert.equal(recorder.starts, 2);
  speak(true);
  tick(250);
  speak(false);
  assert.equal(recorder.starts, 2, "a short quiet after speech is not a cut");
});

test("eight seconds forces a cut with no quiet moment, and no timeslice is ever used", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let uploads = 0;
  createVoiceInput(
    elements,
    api({
      transcribe: async () => {
        uploads++;
        return { text: `part ${uploads}`, language: "English" };
      },
    }),
    environment,
  );
  click(window, elements.microphone);
  await settle();
  const recorder = FakeRecorder.instance;
  recorder.finalData = audio();

  tick(7999);
  assert.equal(recorder.starts, 1);
  tick(1);
  assert.equal(recorder.starts, 2, "the ceiling cuts a segment on its own");
  await settle();
  assert.equal(uploads, 1);
  tick(8000);
  await settle();
  assert.equal(recorder.starts, 3);
  assert.equal(elements.preview.textContent, "part 1 part 2");
  assert.ok(
    recorder.timeslices.every((slice) => slice === undefined),
    "a timeslice chunk is not a file on its own, so start() never takes one",
  );
});

test("segments are transcribed in order, one request at a time", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  const pending: Array<(value: VoiceTranscription) => void> = [];
  createVoiceInput(
    elements,
    api({
      transcribe: () =>
        new Promise<VoiceTranscription>((resolve) => {
          pending.push(resolve);
        }),
    }),
    environment,
  );
  click(window, elements.microphone);
  await settle();
  FakeRecorder.instance.finalData = audio();

  tick(8000);
  await settle();
  assert.equal(pending.length, 1, "the first segment is in flight");
  tick(8000);
  await settle();
  assert.equal(pending.length, 1, "the second waits for the first, it does not overlap it");

  pending[0]({ text: "one", language: "English" });
  await settle();
  assert.equal(elements.preview.textContent, "one");
  assert.equal(pending.length, 2, "the second went out once the first came back");
  pending[1]({ text: "two", language: "English" });
  await settle();
  assert.equal(elements.preview.textContent, "one two");
});

test("a hostile segment renders as plain text in the preview and creates no element", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  const hostile = `<script>globalThis.pwned = true</script><img src=x onerror="globalThis.pwned=true"> **bold** [link](https://example.invalid) # heading`;
  createVoiceInput(
    elements,
    api({ transcribe: async () => ({ text: hostile, language: "English" }) }),
    environment,
  );
  click(window, elements.microphone);
  await settle();
  FakeRecorder.instance.finalData = audio();

  tick(8000);
  await settle();
  assert.equal(elements.preview.textContent, hostile, "shown exactly as text");
  assert.equal(elements.preview.children.length, 0, "one text node, no elements");
  assert.equal(elements.preview.ownerDocument.querySelector("script, img, strong, a, h1"), null);
  assert.equal((globalThis as { pwned?: unknown }).pwned, undefined);
});

test("stop joins the segments into the composer, clears the preview, and sends nothing", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let uploads = 0;
  let submits = 0;
  elements.input.form!.addEventListener("submit", () => submits++);
  elements.input.value = "Before after";
  elements.input.setSelectionRange(7, 7);
  createVoiceInput(
    elements,
    api({
      transcribe: async () => {
        uploads++;
        return { text: `part ${uploads}`, language: "English" };
      },
    }),
    environment,
  );
  click(window, elements.microphone);
  await settle();
  FakeRecorder.instance.finalData = audio();

  tick(8000);
  await settle();
  tick(8000);
  await settle();
  assert.equal(elements.preview.textContent, "part 1 part 2");
  assert.equal(elements.input.value, "Before after", "the composer is untouched while recording");

  click(window, elements.microphone);
  await settle();
  assert.equal(uploads, 3, "the final segment was transcribed too");
  assert.equal(elements.input.value, "Before part 1 part 2 part 3 after");
  assert.equal(elements.input.ownerDocument.activeElement, elements.input, "the text is there to edit");
  assert.equal(elements.preview.hidden, true);
  assert.equal(elements.preview.textContent, "");
  assert.equal(
    elements.status.textContent,
    "Transcript ready. Detected language: English. Review it before sending.",
  );
  assert.equal(submits, 0, "nothing is sent automatically");
});

test("a failing segment shows [unclear] and recording continues", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let calls = 0;
  const original = console.error;
  console.error = () => {};
  try {
    createVoiceInput(
      elements,
      api({
        transcribe: async () => {
          calls++;
          if (calls === 1) throw new Error("runtime hiccup");
          return { text: `part ${calls}`, language: "English" };
        },
      }),
      environment,
    );
    click(window, elements.microphone);
    await settle();
    const recorder = FakeRecorder.instance;
    recorder.finalData = audio();

    tick(8000);
    await settle();
    assert.equal(elements.preview.textContent, "[unclear]");
    assert.equal(recorder.state, "recording", "one bad segment does not end the recording");
    assert.equal(elements.status.textContent?.startsWith("Listening"), true);

    tick(8000);
    await settle();
    assert.equal(elements.preview.textContent, "[unclear] part 2");

    click(window, elements.microphone);
    await settle();
    assert.equal(elements.input.value, "[unclear] part 2 part 3");
    assert.equal(elements.preview.hidden, true);
  } finally {
    console.error = original;
  }
});

test("cancel drops the segments already previewed and leaves the composer alone", async (t) => {
  const tick = fakeClock(t);
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  elements.input.value = "Before after";
  createVoiceInput(elements, api(), environment);
  click(window, elements.microphone);
  await settle();
  FakeRecorder.instance.finalData = audio();

  tick(8000);
  await settle();
  assert.equal(elements.preview.textContent, "hello");

  click(window, elements.cancel);
  await settle();
  assert.equal(elements.preview.hidden, true);
  assert.equal(elements.preview.textContent, "");
  assert.equal(elements.input.value, "Before after");
  assert.match(elements.status.textContent ?? "", /cancelled/i);
});

test("Escape stops the recording the same way the button does", async () => {
  const { window, elements } = fixture();
  const { environment } = recordingEnvironment();
  let uploads = 0;
  createVoiceInput(
    elements,
    api({
      transcribe: async () => {
        uploads++;
        return { text: "hello", language: "English" };
      },
    }),
    environment,
  );
  const document = elements.microphone.ownerDocument;
  document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(uploads, 0, "Escape while idle does nothing");

  click(window, elements.microphone);
  await settle();
  FakeRecorder.instance.finalData = audio();
  document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await settle();
  assert.equal(FakeRecorder.instance.state, "inactive");
  assert.equal(uploads, 1);
  assert.equal(elements.input.value, "hello");
  assert.equal(elements.microphone.dataset.state, "idle");
});
