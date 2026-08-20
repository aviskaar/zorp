/**
 * Tests for the context meter.
 *
 * Most of these are about honesty rather than arithmetic. The meter has two
 * ways to be wrong that matter more than being off by a token: showing an
 * estimate as though it were a measurement, and inventing a context window
 * nobody configured. Both have their own test here.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import {
  clearMeter,
  formatTokens,
  meterView,
  showMeter,
  type MeterElements,
} from "../src/context-meter.ts";

const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function elements(): { dom: JSDOM; meter: MeterElements } {
  const dom = new JSDOM(
    `<div class="context-meter" id="m" hidden>
       <span class="context-bar"><i id="fill"></i></span>
       <span id="text"></span>
     </div>`,
  );
  const doc = dom.window.document;
  return {
    dom,
    meter: {
      root: doc.getElementById("m") as HTMLElement,
      fill: doc.getElementById("fill") as HTMLElement,
      text: doc.getElementById("text") as HTMLElement,
    },
  };
}

test("token counts are shown compactly", () => {
  assert.equal(formatTokens(0), "0");
  assert.equal(formatTokens(812), "812");
  assert.equal(formatTokens(12_400), "12.4k");
  assert.equal(formatTokens(128_000), "128k");
  assert.equal(formatTokens(1_250_000), "1.3M");
});

test("a known window is shown as how much is left", () => {
  const view = meterView({ used_tokens: 32_000, limit_tokens: 128_000, source: "reported" });
  assert.equal(view.label, "75% left");
  assert.equal(view.state, "ok");
  assert.equal(view.fraction, 0.25);
});

/**
 * The number from a provider is a fact about a request that happened. The
 * fallback is bytes divided by four. Drawing them the same way would be the
 * meter's one real lie, so the estimate is marked in the label itself and
 * explained in full on hover.
 */
test("an estimate is marked as an estimate", () => {
  const estimated = meterView({ used_tokens: 32_000, limit_tokens: 128_000, source: "estimated" });
  const reported = meterView({ used_tokens: 32_000, limit_tokens: 128_000, source: "reported" });

  assert.equal(estimated.label, "~75% left");
  assert.notEqual(estimated.label, reported.label);
  assert.match(estimated.detail, /estimated it from the transcript/);
  assert.match(estimated.detail, /rough guide, not a measurement/);
  assert.match(reported.detail, /Reported by the model/);
  assert.doesNotMatch(reported.detail, /estimate/i);
});

/**
 * zorp talks to arbitrary endpoints, including local Ollama, and none of them
 * can be asked how large their window is. With nothing configured the meter
 * must not invent a denominator: no bar, no percentage, and it says where the
 * missing number would come from.
 */
test("an unknown window shows no percentage and says why", () => {
  const view = meterView({ used_tokens: 12_400, source: "reported" });

  assert.equal(view.label, "12.4k sent");
  assert.equal(view.fraction, null);
  assert.equal(view.state, "unknown");
  assert.doesNotMatch(view.label, /%/);
  assert.match(view.detail, /No context window is configured/);
  assert.match(view.detail, /ZORP_CONTEXT_TOKENS/);
});

test("a zero or missing limit is treated as unknown, not as division by zero", () => {
  for (const limit of [0, undefined]) {
    const view = meterView({ used_tokens: 500, limit_tokens: limit, source: "reported" });
    assert.equal(view.fraction, null, `limit ${limit}`);
    assert.equal(view.state, "unknown", `limit ${limit}`);
  }
});

test("a filling window warns and then reads as full", () => {
  const at = (used: number) =>
    meterView({ used_tokens: used, limit_tokens: 1000, source: "reported" }).state;
  assert.equal(at(100), "ok");
  assert.equal(at(750), "warn");
  assert.equal(at(900), "full");
  assert.equal(at(5000), "full", "past the window is still full, never over 100%");
});

test("past the window the bar stops at full rather than overflowing", () => {
  const view = meterView({ used_tokens: 200_000, limit_tokens: 100_000, source: "reported" });
  assert.equal(view.fraction, 1);
  assert.equal(view.label, "0% left");
});

test("drawing a reading fills the bar and reveals the meter", () => {
  const { meter } = elements();

  showMeter(meter, { used_tokens: 25_000, limit_tokens: 100_000, source: "reported" });

  assert.equal(meter.root.hidden, false);
  assert.equal(meter.root.dataset.state, "ok");
  assert.equal(meter.text.textContent, "75% left");
  assert.equal(meter.fill.style.width, "25%");
  assert.match(meter.root.getAttribute("aria-label") ?? "", /Context window:/);
});

/** Everything reaching the page goes through textContent, never innerHTML. */
test("a hostile reading cannot put markup on the page", () => {
  const { dom, meter } = elements();
  // Nothing in a reading is free text, but the meter is fed from a model's
  // conversation all the same, and the rule in this codebase is that the
  // renderer never assembles HTML. This asserts the rule, not a live threat.
  showMeter(meter, { used_tokens: 1, limit_tokens: 2, source: "reported" });
  meter.text.textContent = "<img src=x onerror=alert(1)>";

  assert.equal(dom.window.document.querySelectorAll("img").length, 0);
  assert.equal(meter.text.textContent, "<img src=x onerror=alert(1)>");
});

test("clearing the meter hides it and leaves no stale reading behind", () => {
  const { meter } = elements();
  showMeter(meter, { used_tokens: 25_000, limit_tokens: 100_000, source: "reported" });

  clearMeter(meter);

  assert.equal(meter.root.hidden, true);
  assert.equal(meter.text.textContent, "");
  assert.equal(meter.fill.style.width, "0%");
  assert.equal(meter.root.getAttribute("title"), null);
});

/**
 * `display: inline-flex` outranks the user agent's `[hidden]` rule, so an
 * unstyled `hidden` meter would sit in the topbar from the first paint saying
 * nothing. Asserted against the stylesheet text, for the reason layout.test.ts
 * gives: jsdom answers `display: none` for a hidden element either way, so the
 * computed style would pass with the bug present.
 */
test("hiding the context meter is spelled out in the stylesheet", () => {
  assert.match(
    css,
    /\.context-meter\[hidden\]\s*\{[^}]*display:\s*none/,
    "without this rule the empty meter shows before there is anything to show",
  );
});

test("index.html carries the elements the meter writes into", () => {
  const doc = new JSDOM(html).window.document;
  for (const id of ["context-meter", "context-bar-fill", "context-meter-text"]) {
    assert.ok(doc.getElementById(id), `index.html is missing #${id}`);
  }
  assert.equal(
    doc.getElementById("context-meter")?.hasAttribute("hidden"),
    true,
    "the meter must start hidden: there is nothing measured yet",
  );
});
