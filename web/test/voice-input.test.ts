import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import {
  createVoiceInput,
  type VoiceApi,
  type VoiceEnvironment,
  type VoiceInputElements,
} from "../src/voice-input.ts";

function fixture(): { window: Window; elements: VoiceInputElements } {
  const dom = new JSDOM(`
    <textarea id="input"></textarea>
    <button id="mic" type="button"></button>
    <button id="cancel" type="button" hidden></button>
    <p id="status" hidden></p>
    <button id="download" type="button" hidden></button>
    <code id="command" hidden></code>
  `);
  const doc = dom.window.document;
  return {
    window: dom.window as unknown as Window,
    elements: {
      input: doc.querySelector<HTMLTextAreaElement>("#input")!,
      microphone: doc.querySelector<HTMLButtonElement>("#mic")!,
      cancel: doc.querySelector<HTMLButtonElement>("#cancel")!,
      status: doc.querySelector<HTMLElement>("#status")!,
      download: doc.querySelector<HTMLButtonElement>("#download")!,
      command: doc.querySelector<HTMLElement>("#command")!,
    },
  };
}

function api(overrides: Partial<VoiceApi> = {}): VoiceApi {
  return {
    status: async () => ({
      available: true,
      runtime_reachable: true,
      model_present: true,
      endpoint: "http://127.0.0.1:8000",
      model: "Qwen/Qwen3-ASR-0.6B",
      command: 'qwen-asr-serve Qwen/Qwen3-ASR-0.6B --host 127.0.0.1 --port 8000',
      detail: "ready",
    }),
    wait: async () => {},
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
  const environment: VoiceEnvironment = {
    secureContext: true,
    mediaDevices: {
      getUserMedia: async () => {
        throw new window.DOMException("denied", "NotAllowedError");
      },
    },
    MediaRecorder: class {} as typeof MediaRecorder,
  };
  createVoiceInput(elements, api(), environment);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(elements.status.textContent ?? "", /permission/i);
});

test("a missing model shows the operator command and waits for real readiness", async () => {
  const { window, elements } = fixture();
  const voiceApi = api({
    status: async () => ({
      available: true,
      runtime_reachable: true,
      model_present: false,
      endpoint: "http://127.0.0.1:8000",
      model: "Qwen/Qwen3-ASR-0.6B",
      command: 'qwen-asr-serve Qwen/Qwen3-ASR-0.6B --host 127.0.0.1 --port 8000',
      detail: "missing",
    }),
    wait: async (onEvent) => {
      onEvent({ status: "waiting", model: "qwen", detail: "not ready" });
      assert.match(elements.status.textContent ?? "", /waiting/i);
      onEvent({ status: "ready", model: "qwen", detail: "ready" });
    },
  });
  createVoiceInput(elements, voiceApi, {
    secureContext: true,
    mediaDevices: undefined,
    MediaRecorder: undefined,
  });
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(elements.download.hidden, false);
  assert.equal(elements.command.hidden, false);
  assert.match(elements.command.textContent ?? "", /qwen-asr-serve/);
  click(window, elements.download);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(elements.status.textContent ?? "", /ready/i);
  assert.equal(elements.command.hidden, true);
});

class FakeRecorder extends EventTarget {
  static instance: FakeRecorder;
  static isTypeSupported(): boolean {
    return true;
  }
  readonly mimeType = "audio/webm";
  state: RecordingState = "inactive";

  constructor(_stream: MediaStream, _options?: MediaRecorderOptions) {
    super();
    FakeRecorder.instance = this;
  }

  start(): void {
    this.state = "recording";
  }

  stop(): void {
    this.state = "inactive";
    this.dispatchEvent(new Event("dataavailable"));
    this.dispatchEvent(new Event("stop"));
  }
}

function recordingEnvironment(): { environment: VoiceEnvironment; stopped: () => number } {
  let tracksStopped = 0;
  const stream = {
    getTracks: () => [{ stop: () => tracksStopped++ }],
  } as unknown as MediaStream;
  return {
    environment: {
      secureContext: true,
      mediaDevices: { getUserMedia: async () => stream },
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
  const data = new Event("dataavailable");
  Object.defineProperty(data, "data", {
    value: new Blob(["audio"], { type: "audio/webm" }),
  });
  FakeRecorder.instance.dispatchEvent(data);
  click(window, elements.microphone);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(uploads, 1);
  assert.equal(elements.input.value, `Before ${hostile} after`);
  assert.equal(elements.input.ownerDocument.querySelector("img"), null);
  assert.match(elements.status.textContent ?? "", /العربية/);
});
