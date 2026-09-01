/**
 * Tests for the one thing that makes a lowered approval gate safe to offer:
 * you can see that it is down, from anywhere in the page, for as long as it
 * is down.
 *
 * These are DOM assertions rather than screenshots, so they cannot prove the
 * pill is legible. What they can prove is that no state leaves the page
 * looking like a session that still asks when it does not, which is the
 * failure that would matter.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";
import {
  ASKING_LABEL,
  AUTO_LABEL,
  autoApproveView,
  renderAutoApprove,
} from "../src/approval-mode.ts";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function page(): { view: ReturnType<typeof autoApproveView>; doc: Document } {
  const dom = new JSDOM(html);
  const doc = dom.window.document;
  return { view: autoApproveView(doc), doc };
}

test("the page starts with the gate up", () => {
  const { view } = page();
  assert.equal(view.button.getAttribute("aria-pressed"), "false");
  assert.equal(view.button.dataset.state, "asking");
  assert.equal(view.label.textContent, ASKING_LABEL);
});

test("turning it on says so in the toolbar", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  assert.equal(view.button.getAttribute("aria-pressed"), "true");
  assert.equal(view.button.dataset.state, "on");
  assert.equal(view.label.textContent, AUTO_LABEL);
});

test("turning it back off puts the page back exactly as it was", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  renderAutoApprove(view, false);
  assert.equal(view.button.getAttribute("aria-pressed"), "false");
  assert.equal(view.button.dataset.state, "asking");
  assert.equal(view.label.textContent, ASKING_LABEL);
});

/** Off is the state the page is built in, not one it has to be told about. */
test("the markup ships with the button unpressed", () => {
  assert.match(html, /id="auto-approve-btn"[\s\S]{0,200}aria-pressed="false"/);
});

/**
 * The rule the rest of this UI follows. Nothing in here renders model output,
 * but the next person to edit it should find text assignment, not markup
 * assembly, because this file is one import away from the transcript.
 */
test("the labels are assigned as text and never as markup", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  assert.equal(view.label.querySelectorAll("*").length, 0);
});
