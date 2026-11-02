/**
 * Tests for the model speed dial.
 *
 * The load-bearing claims: the chord is read from the physical digit key so
 * a Mac's Option+digit still counts, any extra modifier or an open
 * composition or focus in the settings pane is not a chord, a press sends
 * only the slot's non-secret model fields (never a key), and the rows land a
 * label through `textContent` because it is a person's own text.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  renderDialRows,
  slotByNumber,
  slotFields,
  slotFromChord,
  slotIsSet,
  slotName,
} from "../src/speed-dial.ts";
import type { DialSlot } from "../src/api.ts";

function chord(code: string, mods: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    code,
    ctrlKey: true,
    altKey: true,
    shiftKey: false,
    metaKey: false,
    isComposing: false,
    ...mods,
  } as KeyboardEvent;
}

test("slotFromChord reads Control+Alt+digit from the number row and the keypad", () => {
  assert.equal(slotFromChord(chord("Digit3"), false), 3);
  assert.equal(slotFromChord(chord("Numpad7"), false), 7);
  // Read from the physical key, so a Mac's Option+3 (key "£") still counts.
  assert.equal(slotFromChord(chord("Digit1"), false), 1);
});

test("slotFromChord rejects non-chords", () => {
  assert.equal(slotFromChord(chord("Digit3", { ctrlKey: false }), false), null);
  assert.equal(slotFromChord(chord("Digit3", { altKey: false }), false), null);
  assert.equal(slotFromChord(chord("Digit3", { shiftKey: true }), false), null);
  assert.equal(slotFromChord(chord("Digit3", { metaKey: true }), false), null);
  assert.equal(slotFromChord(chord("Digit0"), false), null); // only 1 to 9
  assert.equal(slotFromChord(chord("KeyA"), false), null);
  assert.equal(slotFromChord(chord("Digit3", { isComposing: true }), false), null);
  // Focus in the settings pane: a digit is being typed into a slot's fields.
  assert.equal(slotFromChord(chord("Digit3"), true), null);
});

const DIAL: DialSlot[] = [
  { slot: 1, label: "local", provider: "openai_compatible", base_url: "http://x/v1", model: "qwen3:8b" },
  { slot: 2, model: "anthropic/claude-sonnet-4.5", max_tokens: 8192 },
];

test("slotByNumber and slotIsSet", () => {
  assert.equal(slotByNumber(DIAL, 1)?.model, "qwen3:8b");
  assert.equal(slotByNumber(DIAL, 9), undefined);
  assert.equal(slotIsSet(slotByNumber(DIAL, 2)), true);
  assert.equal(slotIsSet(slotByNumber(DIAL, 9)), false);
  assert.equal(slotIsSet({ slot: 3, label: "x" }), false); // a label, no model
});

test("slotFields sends only the slot's non-secret model fields", () => {
  const f = slotFields(DIAL[0]);
  assert.deepEqual(f, {
    provider: "openai_compatible",
    base_url: "http://x/v1",
    model: "qwen3:8b",
  });
  // No label, no slot number, and there is no key field to leak.
  assert.equal("label" in f, false);
  assert.equal("slot" in f, false);
  assert.equal("api_key" in f, false);
  assert.equal(slotFields(DIAL[1]).max_tokens, 8192);
});

test("slotName prefers the label, else the model id", () => {
  assert.equal(slotName(DIAL[0]), "local");
  assert.equal(slotName(DIAL[1]), "anthropic/claude-sonnet-4.5");
});

test("renderDialRows draws nine rows and lands a label through textContent", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const host = dom.window.document.createElement("div");
  const injected: DialSlot = { slot: 1, label: "<img src=x onerror=alert(1)>", model: "m" };
  renderDialRows(dom.window.document, host, [injected], {
    useCurrent: () => {},
    clear: () => {},
    relabel: () => {},
  });
  assert.equal(host.querySelectorAll(".dial-row").length, 9);
  // The injected label is a value on an input, not parsed as HTML.
  assert.equal(dom.window.document.querySelector("img"), null);
  const firstLabel = host.querySelector<HTMLInputElement>(".dial-label");
  assert.equal(firstLabel?.value, "<img src=x onerror=alert(1)>");
  // Slot 9 is empty and its clear button is disabled.
  const rows = host.querySelectorAll(".dial-row");
  const lastClear = rows[8].querySelector<HTMLButtonElement>(".dial-clear");
  assert.equal(lastClear?.disabled, true);
});
