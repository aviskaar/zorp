/**
 * Tests for branching a chat at an answer.
 *
 * The count is the contract with the server: it resolves `{"answer": N}`
 * as the Nth assistant message with text, so the page has to count exactly
 * those and nothing else.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { AnswerCount, branchButton } from "../src/branch.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document;

test("only a text-bearing answer the store has gets an ordinal", () => {
  const count = new AnswerCount();
  assert.equal(count.next("first"), 1);
  // A turn that only called a tool has no text and is not an answer.
  assert.equal(count.next(""), null);
  assert.equal(count.next(" \n\t"), null);
  // A row cut off by a stop or an error was never recorded.
  assert.equal(count.next("half an ans", false), null);
  assert.equal(count.next("second"), 2);
});

test("opening another chat starts the count again", () => {
  const count = new AnswerCount();
  count.next("one");
  count.next("two");
  count.reset();
  assert.equal(count.next("one again"), 1);
});

test("the button goes down while branching and comes back if it fails", async () => {
  let resolve: () => void = () => {};
  const pending = new Promise<void>((r) => {
    resolve = r;
  });
  let calls = 0;
  const button = branchButton(doc, () => {
    calls += 1;
    return pending;
  });
  assert.equal(button.textContent, "Branch");
  assert.equal(button.title, "Start a new chat from this answer");
  button.click();
  assert.equal(calls, 1);
  assert.equal(button.disabled, true);
  // Disabled, so a second click while the first is in flight does nothing.
  button.click();
  assert.equal(calls, 1);
  resolve();
  await pending;
  await Promise.resolve();
  assert.equal(button.disabled, false);

  const failing = branchButton(doc, () => Promise.reject(new Error("409")));
  failing.click();
  assert.equal(failing.disabled, true);
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(failing.disabled, false);
});

test("the button holds its own label as text and nothing else", () => {
  const button = branchButton(doc, () => Promise.resolve());
  assert.equal(button.querySelectorAll("*").length, 0);
  assert.equal(button.type, "button");
});
