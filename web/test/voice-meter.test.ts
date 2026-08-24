import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { createVoiceMeter, type VoiceMeterEnvironment } from "../src/voice-meter.ts";

/** A hand-cranked Web Audio, so a frame happens when the test says so. */
function harness(level: number) {
  const frames: FrameRequestCallback[] = [];
  const cancelled: number[] = [];
  let closed = false;
  let connected = 0;
  let disconnected = 0;
  let sample = level;

  class FakeAnalyser {
    fftSize = 2048;
    smoothingTimeConstant = 0;
    getByteTimeDomainData(target: Uint8Array): void {
      target.fill(sample);
    }
    disconnect(): void {
      disconnected += 1;
    }
  }

  const analyser = new FakeAnalyser();

  class FakeAudioContext {
    createAnalyser(): FakeAnalyser {
      return analyser;
    }
    createMediaStreamSource(): { connect(): void; disconnect(): void } {
      return {
        connect: () => {
          connected += 1;
        },
        disconnect: () => {
          disconnected += 1;
        },
      };
    }
    async close(): Promise<void> {
      closed = true;
    }
  }

  const environment: VoiceMeterEnvironment = {
    AudioContext: FakeAudioContext as unknown as typeof AudioContext,
    requestAnimationFrame: (callback) => {
      frames.push(callback);
      return frames.length;
    },
    cancelAnimationFrame: (handle) => {
      cancelled.push(handle);
    },
  };

  return {
    environment,
    analyser,
    state: {
      get closed() {
        return closed;
      },
      get connected() {
        return connected;
      },
      get disconnected() {
        return disconnected;
      },
      get cancelled() {
        return cancelled;
      },
    },
    speak(value: number): void {
      sample = value;
    },
    /** Run exactly one pending frame. */
    step(): void {
      const next = frames.shift();
      assert.ok(next, "expected a scheduled frame");
      next(0);
    },
  };
}

function container(): HTMLElement {
  const dom = new JSDOM(`<div id="meter" hidden></div>`);
  return dom.window.document.querySelector<HTMLElement>("#meter")!;
}

const stream = {} as MediaStream;

function heights(element: HTMLElement): number[] {
  return [...element.children].map((bar) => {
    const match = /scaleY\(([\d.]+)\)/.exec((bar as HTMLElement).style.transform);
    assert.ok(match, "expected a scaleY on every bar");
    return Number(match[1]);
  });
}

test("a meter with no container does nothing rather than throwing", () => {
  const meter = createVoiceMeter(undefined, harness(128).environment);
  meter.start(stream);
  meter.stop();
});

test("a browser with no Web Audio gets an inert meter", () => {
  const element = container();
  const meter = createVoiceMeter(element, {
    AudioContext: undefined,
    requestAnimationFrame: (callback) => {
      callback(0);
      return 1;
    },
    cancelAnimationFrame: () => {},
  });
  meter.start(stream);
  assert.equal(element.hidden, true, "an inert meter stays off the page");
  assert.equal(element.children.length, 0);
});

test("starting builds the bars, shows them, and reads the stream", () => {
  const element = container();
  const rig = harness(128);
  createVoiceMeter(element, rig.environment).start(stream);
  assert.equal(element.hidden, false);
  assert.ok(element.children.length > 1, "expected a row of bars");
  assert.equal(rig.state.connected, 1);
  assert.equal(rig.analyser.fftSize, 1024);
});

test("a loud frame raises the newest bar and quiet frames carry it left", () => {
  const element = container();
  const rig = harness(255);
  const meter = createVoiceMeter(element, rig.environment);
  meter.start(stream);

  rig.step();
  const loud = heights(element);
  assert.ok(loud[loud.length - 1] > loud[0], "the newest level enters on the right");
  assert.ok(loud.slice(0, -1).every((value) => value === loud[0]), "older bars are untouched");

  rig.speak(128);
  rig.step();
  const later = heights(element);
  assert.equal(later[later.length - 2], loud[loud.length - 1], "the loud level moved one left");
  assert.equal(later[later.length - 1], later[0], "silence rests the newest bar");
});

test("stopping cancels the frame, closes the context, and clears the meter", async () => {
  const element = container();
  const rig = harness(255);
  const meter = createVoiceMeter(element, rig.environment);
  meter.start(stream);
  rig.step();
  const loud = heights(element);

  meter.stop();
  await Promise.resolve();

  assert.equal(rig.state.cancelled.length, 1, "the animation frame is released");
  assert.ok(rig.state.disconnected >= 2, "the source and the analyser are disconnected");
  assert.equal(rig.state.closed, true, "the audio context is closed");
  assert.equal(element.hidden, true);
  const rested = heights(element);
  assert.ok(rested.every((value) => value === rested[0]), "every bar is back at rest");
  assert.ok(rested[rested.length - 1] < loud[loud.length - 1]);
});

test("a Web Audio failure leaves the meter off rather than failing the recording", () => {
  const element = container();
  const rig = harness(128);
  const meter = createVoiceMeter(element, {
    ...rig.environment,
    AudioContext: class {
      constructor() {
        throw new Error("no audio for you");
      }
    } as unknown as typeof AudioContext,
  });
  meter.start(stream);
  assert.equal(element.hidden, true);
});
