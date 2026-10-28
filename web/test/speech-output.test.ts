/**
 * Tests for reading answers aloud.
 *
 * Two claims matter. Speech is offered only when the server said all three of
 * its facts are true, so a button never promises audio the runtime cannot
 * produce. And speaking is a person's switch: the controls here start and
 * stop it, nothing else does.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import { SPEAK_LABEL, SpeechPlayer, speakerButton } from "../src/speech-output.ts";
import type { SpeechStatus } from "../src/api.ts";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function ready(overrides: Partial<SpeechStatus> = {}): SpeechStatus {
  return {
    available: true,
    runtime_reachable: true,
    model_present: true,
    endpoint: "http://127.0.0.1:11434",
    model: "phonon",
    voice: "default",
    voices: ["default"],
    detail: "ready",
    ...overrides,
  };
}

test("speech is available only when all three facts hold", () => {
  const player = new SpeechPlayer();
  assert.equal(player.available(), false, "nothing reported yet");
  player.observe(ready());
  assert.equal(player.available(), true);
  for (const off of [
    { available: false },
    { runtime_reachable: false },
    { model_present: false },
  ]) {
    player.observe(ready(off));
    assert.equal(player.available(), false, JSON.stringify(off));
  }
  player.observe(null);
  assert.equal(player.available(), false);
});

test("read-aloud only reports on when the runtime is also available", () => {
  const player = new SpeechPlayer();
  player.setReadAloud(true);
  assert.equal(player.isReadAloud(), false, "no runtime yet");
  player.observe(ready());
  assert.equal(player.isReadAloud(), true);
  player.setReadAloud(false);
  assert.equal(player.isReadAloud(), false);
});

test("the speaker button is hidden until the runtime can speak", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const player = new SpeechPlayer();
  const button = speakerButton(dom.window.document, player, () => "an answer");
  assert.equal(button.hidden, true);

  const player2 = new SpeechPlayer();
  player2.observe(ready());
  const button2 = speakerButton(dom.window.document, player2, () => "an answer");
  assert.equal(button2.hidden, false);
  assert.equal(button2.textContent, SPEAK_LABEL);
  assert.equal(button2.getAttribute("aria-pressed"), "false");
});

test("speaking plays each chunk and ends not speaking", async () => {
  // A fake audio sink that finishes immediately, and a stubbed fetch so no
  // real network or Audio is needed. The player is driven end to end.
  const played: number[] = [];
  const player = new SpeechPlayer((blob, onEnded) => {
    played.push(blob.size);
    onEnded();
    return { stop() {} };
  });
  player.observe(ready());
  const realFetch = globalThis.fetch;
  (globalThis as { fetch: unknown }).fetch = async (input: unknown): Promise<unknown> => {
    const path = String(input);
    if (path.includes("/api/speech/chunks")) {
      return {
        ok: true,
        async text() {
          return JSON.stringify({ chunks: ["one.", "two."] });
        },
      };
    }
    return { ok: true, async blob() {
      return new Blob([new Uint8Array([1, 2, 3])]);
    } };
  };
  try {
    await player.speak("one. two.");
  } finally {
    (globalThis as { fetch: unknown }).fetch = realFetch;
  }
  assert.equal(played.length, 2, "both chunks were played");
  assert.equal(player.speaking(), false, "playback ended");
});

test("stop cancels an in-flight run", async () => {
  const player = new SpeechPlayer((_blob, _onEnded) => {
    // Never calls onEnded, so the chunk "keeps playing" until stop.
    return { stop() {} };
  });
  player.observe(ready());
  const realFetch = globalThis.fetch;
  (globalThis as { fetch: unknown }).fetch = async (input: unknown): Promise<unknown> => {
    const path = String(input);
    if (path.includes("/api/speech/chunks")) {
      return { ok: true, async text() {
        return JSON.stringify({ chunks: ["a.", "b.", "c."] });
      } };
    }
    return { ok: true, async blob() {
      return new Blob([new Uint8Array([9])]);
    } };
  };
  try {
    const running = player.speak("a. b. c.");
    // Let it reach the first chunk, then stop.
    await new Promise((r) => setTimeout(r, 10));
    assert.equal(player.speaking(), true);
    player.stop();
    assert.equal(player.speaking(), false);
    await running;
  } finally {
    (globalThis as { fetch: unknown }).fetch = realFetch;
  }
});

test("index.html carries the read-aloud toggle, hidden and not pressed", () => {
  const doc = new JSDOM(html).window.document;
  const button = doc.getElementById("read-aloud-btn");
  assert.ok(button, "index.html is missing #read-aloud-btn");
  assert.equal(button?.hasAttribute("hidden"), true);
  assert.equal(button?.getAttribute("aria-pressed"), "false");
  assert.ok(doc.getElementById("read-aloud-label"));
});

test("index.html carries the speech settings pickers", () => {
  const doc = new JSDOM(html).window.document;
  for (const id of ["settings-speech-model", "settings-speech-voice"]) {
    assert.ok(doc.getElementById(id), `index.html is missing #${id}`);
  }
});
