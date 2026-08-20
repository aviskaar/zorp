/**
 * Tests for the one thing that makes a lowered approval gate safe to offer:
 * you can see that it is down, from anywhere in the page, for as long as it
 * is down.
 *
 * These are DOM assertions rather than screenshots, so they cannot prove the
 * banner is legible. What they can prove is that no state leaves the page
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
  assert.equal(view.banner.hidden, true);
});

test("turning it on says so in the toolbar and in a banner", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  assert.equal(view.button.getAttribute("aria-pressed"), "true");
  assert.equal(view.button.dataset.state, "on");
  assert.equal(view.label.textContent, AUTO_LABEL);
  assert.equal(view.banner.hidden, false);
});

/**
 * The banner is the whole reason this is allowed to exist, so it has to say
 * what is off and what is still on. A user who reads it and believes the
 * denylist is off too will be more careful, not less, which is the safe way
 * to be wrong; the reverse is not.
 */
test("the banner says what it does and what it does not do", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  const text = (view.banner.textContent ?? "").toLowerCase();
  assert.match(text, /without asking|no longer ask|runs without/);
  assert.match(text, /denylist/);
});

test("turning it back off puts the page back exactly as it was", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  renderAutoApprove(view, false);
  assert.equal(view.button.getAttribute("aria-pressed"), "false");
  assert.equal(view.button.dataset.state, "asking");
  assert.equal(view.label.textContent, ASKING_LABEL);
  assert.equal(view.banner.hidden, true);
});

/** Off is the state the page is built in, not one it has to be told about. */
test("the markup ships with the banner hidden and the button unpressed", () => {
  assert.match(html, /id="auto-approve-banner"[^>]*hidden/);
  assert.match(html, /id="auto-approve-btn"[\s\S]{0,200}aria-pressed="false"/);
});

/**
 * The way out has to be on the page whenever the mode is, and it has to be a
 * button rather than something that only works between turns.
 */
test("the banner carries its own off switch", () => {
  const { view } = page();
  renderAutoApprove(view, true);
  assert.ok(view.bannerOff, "the banner has no off switch");
  assert.equal(view.bannerOff.disabled, false);
  assert.ok(view.banner.contains(view.bannerOff));
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
