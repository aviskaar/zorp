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
import {
  answerActions,
  copyButton,
  SHARE_TARGETS,
  shareMenu,
  shareText,
  type WriteText,
} from "../src/copy-response.ts";

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

/* ------------------------------------------------------------------ *
 * Copying an answer for another assistant.
 * ------------------------------------------------------------------ */

/**
 * The payload is a pure function of the destination and the answer, so it is
 * checked here rather than through the DOM. These assertions are the whole
 * argument for the feature existing. If the three ever collapse into each
 * other, one menu entry is wearing three hats and should go back to being one.
 */

test("every destination quotes the answer verbatim and says where it came from", () => {
  const answer = "The half life is 12.3 years.\n\n- source one\n- source two";
  for (const target of SHARE_TARGETS) {
    const payload = shareText(target, answer);
    assert.ok(payload.includes(answer), `${target} dropped the answer`);
    assert.match(payload, /zorp/);
  }
});

test("the answer is never fenced, because answers contain fences", () => {
  const answer = "Here is the fix:\n\n```rust\nfn main() {}\n```\n";
  for (const target of SHARE_TARGETS) {
    const payload = shareText(target, answer);
    // The only backticks in the payload are the ones the answer brought.
    const added = payload.split("```").length - answer.split("```").length;
    assert.equal(added, 0, `${target} wrapped the answer in a fence`);
  }
});

test("Claude gets tag delimiters, which is Anthropic's own documented convention", () => {
  const payload = shareText("claude", "an answer");
  assert.match(payload, /<zorp-answer>\nan answer\n<\/zorp-answer>/);
  assert.ok(!payload.includes("## zorp answer"));
});

test("Codex is told the answer is reference material and not a work order", () => {
  const payload = shareText("codex", "an answer");
  assert.match(payload, /not a work order/i);
  assert.ok(!payload.includes("<zorp-answer>"));
});

test("Gemini gets the plain frame, with no instruction aimed at an agent", () => {
  const payload = shareText("gemini", "an answer");
  assert.ok(!/work order/i.test(payload));
  assert.ok(!payload.includes("<zorp-answer>"));
  assert.match(payload, /## zorp answer/);
});

test("no two destinations produce the same bytes", () => {
  const payloads = SHARE_TARGETS.map((target) => shareText(target, "an answer"));
  assert.equal(new Set(payloads).size, payloads.length);
});

test("the frame closes cleanly around a ragged answer", () => {
  const payload = shareText("claude", "  an answer\n\n\n");
  assert.ok(payload.endsWith("an answer\n</zorp-answer>"));
});

/** A menu on its own page, so focus has somewhere real to go. */
function menu(answer: () => string, write: WriteText, after: (fn: () => void, ms: number) => void) {
  const page = new JSDOM("<!doctype html><body></body>");
  const wrapper = shareMenu(page.window.document, answer, write, after);
  page.window.document.body.append(wrapper);
  const toggle = wrapper.querySelector("button") as HTMLButtonElement;
  const items = Array.from(wrapper.querySelectorAll(".share-item")) as HTMLButtonElement[];
  const list = wrapper.querySelector(".share-list") as HTMLElement;
  return { page, wrapper, toggle, items, list };
}

test("the menu starts shut and says so", () => {
  const { toggle, list, items } = menu(() => "x", spy().write, () => {});
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(list.hidden, true);
  assert.equal(items.length, 3);
  assert.deepEqual(
    items.map((item) => item.textContent),
    ["Claude", "Codex", "Gemini"],
  );
});

test("the toggle opens and shuts it", () => {
  const { toggle, list } = menu(() => "x", spy().write, () => {});
  toggle.click();
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(list.hidden, false);
  toggle.click();
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(list.hidden, true);
});

test("choosing a destination copies that destination's payload", async () => {
  for (const [index, target] of SHARE_TARGETS.entries()) {
    const clipboard = spy();
    const { toggle, items } = menu(() => "an answer", clipboard.write, () => {});
    toggle.click();
    items[index].click();
    await Promise.resolve();
    assert.deepEqual(clipboard.written, [shareText(target, "an answer")]);
  }
});

/** The same rule the plain copy button follows, for the same reason. */
test("the menu reads the answer at click time", async () => {
  const clipboard = spy();
  let answer = "half of the ans";
  const { toggle, items } = menu(() => answer, clipboard.write, () => {});
  answer = "the whole answer";
  toggle.click();
  items[0].click();
  await Promise.resolve();
  assert.deepEqual(clipboard.written, [shareText("claude", "the whole answer")]);
});

test("choosing shuts the menu and reports on the toggle", async () => {
  const clipboard = spy();
  const { toggle, items, list } = menu(() => "x", clipboard.write, () => {});
  toggle.click();
  items[1].click();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(list.hidden, true);
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(toggle.textContent, "Copied for Codex");
  assert.equal(toggle.dataset.state, "done");
});

test("a refused clipboard says so here too", async () => {
  const refuse = () => Promise.reject(new Error("denied"));
  const { toggle, items } = menu(() => "x", refuse, () => {});
  toggle.click();
  items[2].click();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(toggle.textContent, "Copy failed");
  assert.equal(toggle.dataset.state, "failed");
});

test("the toggle goes back to offering", async () => {
  const clipboard = spy();
  const { toggle, items } = menu(() => "x", clipboard.write, immediately);
  toggle.click();
  items[0].click();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(toggle.textContent, "Copy for…");
  assert.equal(toggle.dataset.state, undefined);
});

test("every control here carries a label longer than its one word", () => {
  const { toggle, items } = menu(() => "x", spy().write, () => {});
  assert.match(toggle.getAttribute("aria-label") ?? "", /answer/);
  for (const [index, target] of SHARE_TARGETS.entries()) {
    const label = items[index].getAttribute("aria-label") ?? "";
    assert.match(label, /answer/);
    assert.ok(label.toLowerCase().includes(target), `${target} is not named in its label`);
  }
});

test("escape shuts the menu and hands focus back", () => {
  const { page, wrapper, toggle, list } = menu(() => "x", spy().write, () => {});
  toggle.click();
  wrapper.dispatchEvent(new page.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  assert.equal(list.hidden, true);
  assert.equal(page.window.document.activeElement, toggle);
});

test("focus leaving the menu shuts it, focus moving inside it does not", () => {
  const { page, wrapper, toggle, items, list } = menu(() => "x", spy().write, () => {});
  const outside = page.window.document.createElement("button");
  page.window.document.body.append(outside);

  toggle.click();
  wrapper.dispatchEvent(
    new page.window.FocusEvent("focusout", { bubbles: true, relatedTarget: items[0] }),
  );
  assert.equal(list.hidden, false);

  wrapper.dispatchEvent(
    new page.window.FocusEvent("focusout", { bubbles: true, relatedTarget: outside }),
  );
  assert.equal(list.hidden, true);
});

/**
 * Same rule as everywhere else in this UI. The answer is model output and it
 * reaches the page as text or not at all, whichever control is holding it.
 */
test("a hostile answer never becomes markup in the menu", async () => {
  const clipboard = spy();
  const hostile = "<img src=x onerror=alert(1)>";
  const { wrapper, toggle, items } = menu(() => hostile, clipboard.write, () => {});
  toggle.click();
  items[0].click();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(wrapper.querySelectorAll("img").length, 0);
  assert.ok(clipboard.written[0].includes(hostile));
});

test("the actions row keeps the plain copy alongside the menu", async () => {
  const clipboard = spy();
  const page = new JSDOM("<!doctype html><body></body>");
  const row = answerActions(page.window.document, () => "an answer", clipboard.write, () => {});
  const plain = row.querySelector(".copy-btn:not(.share-toggle)") as HTMLButtonElement;
  assert.ok(plain, "the plain copy button is gone");
  assert.ok(row.querySelector(".share-list"), "the menu is gone");
  plain.click();
  await Promise.resolve();
  // Still the raw answer, with nothing wrapped around it.
  assert.deepEqual(clipboard.written, ["an answer"]);
});
