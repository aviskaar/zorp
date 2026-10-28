/**
 * Tests for the command-sandbox indicator.
 *
 * The pill makes one claim: the agent's shell commands run under an OS-level
 * sandbox, in this mode. `off` is the default and draws nothing, the way the
 * search pill draws nothing when search is absent. A mode that cannot
 * actually run is still shown, but as a warning, because a command under it
 * will refuse and that is worth knowing before running one.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import {
  renderSandboxIndicator,
  sandboxIndicatorView,
  type SandboxIndicatorView,
} from "../src/sandbox-indicator.ts";

const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function view(): { dom: JSDOM; indicator: SandboxIndicatorView } {
  const dom = new JSDOM(
    `<div class="search-indicator sandbox-indicator" id="sandbox-indicator" hidden>
       <span id="sandbox-indicator-text"></span>
     </div>`,
  );
  return { dom, indicator: sandboxIndicatorView(dom.window.document) };
}

test("with no answer the indicator stays away", () => {
  const { indicator } = view();
  renderSandboxIndicator(indicator, null);
  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
});

test("the default off mode draws nothing", () => {
  const { indicator } = view();
  renderSandboxIndicator(indicator, { mode: "off", available: true, detail: "srt 0.1.0" });
  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
});

test("a running mode shows its name and the reason in the tooltip", () => {
  const { indicator } = view();
  renderSandboxIndicator(indicator, {
    mode: "workspace",
    available: true,
    detail: "srt 0.1.0",
  });
  assert.equal(indicator.root.hidden, false);
  assert.equal(indicator.text.textContent, "Sandbox: workspace");
  assert.match(indicator.root.getAttribute("title") ?? "", /srt 0\.1\.0/);
  assert.equal(indicator.root.getAttribute("data-warn"), null);
});

test("a mode that cannot run is shown as a warning", () => {
  const { indicator } = view();
  renderSandboxIndicator(indicator, {
    mode: "build",
    available: false,
    detail: "missing: srt, ripgrep",
  });
  assert.equal(indicator.root.hidden, false);
  assert.equal(indicator.text.textContent, "Sandbox: build");
  assert.equal(indicator.root.getAttribute("data-warn"), "true");
  assert.match(indicator.root.getAttribute("title") ?? "", /cannot run/);
});

/** Turning it back to off has to take the pill down and clear the warning. */
test("an off mode after a real one clears everything", () => {
  const { indicator } = view();
  renderSandboxIndicator(indicator, { mode: "build", available: false, detail: "x" });
  renderSandboxIndicator(indicator, { mode: "off", available: true, detail: "y" });
  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
  assert.equal(indicator.root.getAttribute("data-warn"), null);
  assert.equal(indicator.root.getAttribute("title"), null);
});

/** A server-supplied detail is text, never markup. */
test("a hostile detail cannot put markup on the page", () => {
  const { dom, indicator } = view();
  renderSandboxIndicator(indicator, {
    mode: "workspace",
    available: false,
    detail: "<img src=x onerror=alert(1)>",
  });
  assert.equal(dom.window.document.querySelectorAll("img").length, 0);
  assert.match(indicator.root.getAttribute("title") ?? "", /<img src=x/);
});

test("hiding the indicator is spelled out in the stylesheet", () => {
  assert.match(
    css,
    /\.sandbox-indicator\[hidden\]\s*\{[^}]*display:\s*none/,
    "without this rule the pill claims a sandbox the mode may not have",
  );
});

test("index.html carries the elements the indicator writes into", () => {
  const doc = new JSDOM(html).window.document;
  for (const id of ["sandbox-indicator", "sandbox-indicator-text"]) {
    assert.ok(doc.getElementById(id), `index.html is missing #${id}`);
  }
  assert.equal(
    doc.getElementById("sandbox-indicator")?.hasAttribute("hidden"),
    true,
    "the indicator must start hidden: nothing has said a mode is on yet",
  );
});

/** Read-only. It reports the mode, it does not switch it. */
test("the indicator is not a control", () => {
  const doc = new JSDOM(html).window.document;
  const root = doc.getElementById("sandbox-indicator");
  assert.equal(root?.tagName, "DIV");
  assert.equal(root?.querySelector("button"), null);
});
