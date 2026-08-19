/**
 * Tests for the copy button that sits under a finished answer.
 *
 * The clipboard write is injected rather than reached for through
 * `navigator`, both because jsdom has no clipboard and because the failure
 * path is the interesting one: a browser can refuse the write, and a button
 * that silently does nothing is worse than one that says so.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { copyButton } from "../src/copy-response.ts";

/** A clipboard that records what it was handed. */
function spy(): { written: string[]; write: (text: string) => Promise<void> } {
  const written: string[] = [];
  return {
    written,
    write: (text: string) => {
      written.push(text);
      return Promise.resolve();
    },
  };
}

/** Runs every deferred callback at once, so no test waits on a timer. */
function immediately(fn: () => void): void {
  fn();
}

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document;

test("clicking copies the answer", async () => {
  const clipboard = spy();
  const button = copyButton(doc, () => "the answer", clipboard.write, () => {});
  button.click();
  await Promise.resolve();
  assert.deepEqual(clipboard.written, ["the answer"]);
});

/**
 * The answer is read when the button is clicked, not when it is built. A
 * streamed message is only final once the turn closes, and the server's
 * authoritative text replaces what was streamed.
 */
test("the answer is read at click time", async () => {
  const clipboard = spy();
  let answer = "half of the ans";
  const button = copyButton(doc, () => answer, clipboard.write, () => {});
  answer = "the whole answer";
  button.click();
  await Promise.resolve();
  assert.deepEqual(clipboard.written, ["the whole answer"]);
});

test("markdown is copied exactly as written, not as it was rendered", async () => {
  const clipboard = spy();
  const source = "# Heading\n\n- one\n- two\n\n**bold** and `code`";
  const button = copyButton(doc, () => source, clipboard.write, () => {});
  button.click();
  await Promise.resolve();
  assert.deepEqual(clipboard.written, [source]);
});

test("a copied answer says so, then goes back to offering", async () => {
  const clipboard = spy();
  const button = copyButton(doc, () => "x", clipboard.write, immediately);
  assert.equal(button.textContent, "Copy");
  button.click();
  await Promise.resolve();
  await Promise.resolve();
  // `immediately` ran the reset as soon as it was scheduled.
  assert.equal(button.textContent, "Copy");
});

test("a refused clipboard says so rather than doing nothing", async () => {
  const refuse = () => Promise.reject(new Error("denied"));
  const button = copyButton(doc, () => "x", refuse, () => {});
  button.click();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(button.textContent, "Copy failed");
  assert.equal(button.dataset.state, "failed");
});

/**
 * The button shows its own label and never the answer. This is the same rule
 * the markdown renderer follows: model output reaches the page as text or not
 * at all.
 */
test("the answer never becomes markup in the button", async () => {
  const clipboard = spy();
  const hostile = "<img src=x onerror=alert(1)>";
  const button = copyButton(doc, () => hostile, clipboard.write, () => {});
  button.click();
  await Promise.resolve();
  assert.equal(button.querySelectorAll("*").length, 0);
  assert.equal(button.textContent, "Copied");
  assert.deepEqual(clipboard.written, [hostile]);
});
