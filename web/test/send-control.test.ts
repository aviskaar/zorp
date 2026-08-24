/**
 * The composer's one button, which sends when idle and stops during a run.
 *
 * Two halves are tested here because the button is made of two halves. The
 * label and the state live in `src/send-control.ts` and are asserted through
 * jsdom. The icons live in `index.html` and `styles.css`, and the swap between
 * them is a cascade question, so those are read as text.
 *
 * The label is the part worth guarding. A button that shows a stop square
 * while announcing itself as "Send message" is not a smaller bug than one that
 * shows the wrong icon; it is the same bug, for the people who cannot see the
 * icon at all.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

import { SEND_LABEL, STOP_LABEL, STOPPING_LABEL, setSendControl } from "../src/send-control.ts";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

function button(): HTMLButtonElement {
  const dom = new JSDOM("<!doctype html><body><button id='send'></button></body>");
  return dom.window.document.querySelector("#send") as unknown as HTMLButtonElement;
}

test("the idle control says it sends", () => {
  const send = button();
  setSendControl(send, "send");
  assert.equal(send.getAttribute("aria-label"), SEND_LABEL);
  assert.equal(send.title, SEND_LABEL);
  assert.equal(send.classList.contains("is-stop"), false);
  assert.equal(send.disabled, false);
});

test("the running control says it stops", () => {
  const send = button();
  setSendControl(send, "stop");
  assert.equal(send.getAttribute("aria-label"), STOP_LABEL);
  assert.equal(send.title, STOP_LABEL);
  assert.match(STOP_LABEL.toLowerCase(), /stop/);
  assert.equal(send.classList.contains("is-stop"), true);
});

// The one thing a stop button must be. It replaces a control that was
// disabled for the whole run, so pressing it has to be possible.
test("the stop control is pressable, unlike the send button it replaces", () => {
  const send = button();
  setSendControl(send, "stop");
  assert.equal(send.disabled, false, "the stop button cannot be pressed");
});

// Between the click and the server's answer there is nothing useful a second
// press can do, and two stops racing each other is one more thing to reason
// about than is needed.
test("a stop already in flight cannot be pressed again", () => {
  const send = button();
  setSendControl(send, "stopping");
  assert.equal(send.disabled, true);
  assert.equal(send.getAttribute("aria-label"), STOPPING_LABEL);
  assert.equal(send.classList.contains("is-stop"), true, "the icon flipped back mid-stop");
});

// The regression that makes this a module rather than three lines inline: a
// label that is set once on the way into a run and never set back.
test("the label goes back to send when the turn ends", () => {
  const send = button();
  setSendControl(send, "stop");
  setSendControl(send, "stopping");
  setSendControl(send, "send");
  assert.equal(send.getAttribute("aria-label"), SEND_LABEL);
  assert.equal(send.title, SEND_LABEL);
  assert.equal(send.classList.contains("is-stop"), false);
  assert.equal(send.disabled, false, "the composer stayed locked after the turn ended");
});

test("the composer keeps one primary send and stop control", () => {
  const dom = new JSDOM(html);
  const doc = dom.window.document;
  const sends = doc.querySelectorAll(".composer button[type=submit]");
  assert.equal(sends.length, 1, "the composer grew a second submit control");
  const send = doc.querySelector("#send")!;
  assert.ok(send.querySelector(".send-icon"), "no send icon");
  assert.ok(send.querySelector(".stop-icon"), "no stop icon");
  assert.equal(doc.querySelector("#voice-mic")?.getAttribute("type"), "button");
});

/**
 * An approval card settles into one of four states and three of them are
 * quiet. `is-stopped` was the fourth, and with no rule of its own it kept the
 * amber highlight the card wears while it is still waiting for an answer: a
 * settled card that goes on looking live. Caught in a browser, not here, which
 * is why it is here now.
 */
test("a stopped approval card settles as quietly as a denied one", () => {
  assert.match(
    css,
    /\.card-approval\.is-stopped\s*[,{]/,
    "a stopped approval card keeps the highlight it wears while waiting",
  );
});

/**
 * Read as text rather than as a computed style. Both icons sit in the button
 * at once and exactly one is displayed, which is a cascade question, and
 * `layout.test.ts` already documents that jsdom answers cascade questions here
 * confidently and sometimes wrongly. A test that passes with the bug present
 * is worse than no test.
 */
test("only one icon shows at a time", () => {
  assert.match(
    css,
    /\.send\s+\.stop-icon\s*\{[^}]*display:\s*none/,
    "the stop icon is not hidden on an idle button, so both icons show at once",
  );
  assert.match(
    css,
    /\.send\.is-stop\s+\.send-icon\s*\{[^}]*display:\s*none/,
    "the send arrow is not hidden during a run, so both icons show at once",
  );
});
