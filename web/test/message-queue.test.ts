/**
 * Tests for the queue that holds messages typed while a turn is running.
 *
 * `renderQueue` is the only piece worth testing at this level: it decides
 * what appears, in what order, and whether removing an item calls back with
 * the right index. Draining it into the next turn is `main.ts` wiring, which
 * this repo leaves untested at the unit level (see `session-row.test.ts`'s
 * `deleteSessionRow` precedent) because `main.ts` runs the whole app on
 * import.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";
import { queueView, renderQueue } from "../src/message-queue.ts";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function page(): { view: ReturnType<typeof queueView>; doc: Document } {
  const dom = new JSDOM(html);
  const doc = dom.window.document;
  return { view: queueView(doc), doc };
}

test("an empty queue stays hidden", () => {
  const { view, doc } = page();
  renderQueue(doc, view, [], () => {});
  assert.equal(view.container.hidden, true);
  assert.equal(view.list.children.length, 0);
});

test("queued messages render in order and unhide the panel", () => {
  const { view, doc } = page();
  renderQueue(doc, view, ["first", "second"], () => {});
  assert.equal(view.container.hidden, false);
  assert.equal(view.list.children.length, 2);
  const texts = [...view.list.querySelectorAll(".message-queue-text")].map(
    (node) => node.textContent,
  );
  assert.deepEqual(texts, ["first", "second"]);
});

test("removing an item calls back with its index", () => {
  const { view, doc } = page();
  const removed: number[] = [];
  renderQueue(doc, view, ["first", "second"], (index) => removed.push(index));
  const buttons = view.list.querySelectorAll(".message-queue-remove");
  (buttons[1] as HTMLButtonElement).click();
  assert.deepEqual(removed, [1]);
});

/** The rule this whole UI follows: queued text goes on through textContent. */
test("queued text is assigned, never parsed as markup", () => {
  const { view, doc } = page();
  renderQueue(doc, view, ["<img src=x onerror=alert(1)>"], () => {});
  const span = view.list.querySelector(".message-queue-text");
  assert.equal(span?.querySelectorAll("*").length, 0);
  assert.equal(span?.textContent, "<img src=x onerror=alert(1)>");
});

test("a second render fully replaces the first", () => {
  const { view, doc } = page();
  renderQueue(doc, view, ["one", "two", "three"], () => {});
  renderQueue(doc, view, ["only"], () => {});
  assert.equal(view.list.children.length, 1);
  assert.equal(view.list.textContent?.includes("only"), true);
});
