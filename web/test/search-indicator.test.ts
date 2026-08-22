/**
 * Tests for the web search indicator.
 *
 * The indicator makes one claim: this agent can search the web. The tests
 * here are mostly about the claim being false by default and only becoming
 * true because the server said so. A pill that appeared on a hopeful guess
 * would tell someone their questions are being answered from the live web
 * when they are not, or, worse, imply nothing leaves the machine when
 * something does.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import {
  SEARCH_LABEL,
  renderSearchIndicator,
  searchIndicatorView,
  type SearchIndicatorView,
} from "../src/search-indicator.ts";

const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

function view(): { dom: JSDOM; indicator: SearchIndicatorView } {
  const dom = new JSDOM(
    `<div class="search-indicator" id="search-indicator" hidden>
       <span id="search-indicator-text"></span>
     </div>`,
  );
  return { dom, indicator: searchIndicatorView(dom.window.document) };
}

test("with no answer from the server the indicator stays away", () => {
  const { indicator } = view();

  renderSearchIndicator(indicator, null);

  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
});

/**
 * The default build compiles `web_search` out, so this is what almost every
 * page load draws. Nothing on screen, and no half-lit "search: off" pill
 * either: an absent capability is not news.
 */
test("an unavailable tool hides the indicator", () => {
  const { indicator } = view();

  renderSearchIndicator(indicator, {
    available: false,
    detail: "this zorp-agent was built without the search feature.",
  });

  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
  assert.equal(indicator.root.getAttribute("title"), null);
});

test("an available tool shows the indicator and says why it is there", () => {
  const { indicator } = view();

  renderSearchIndicator(indicator, {
    available: true,
    detail: "web_search is registered, and every search asks first.",
  });

  assert.equal(indicator.root.hidden, false);
  assert.equal(indicator.text.textContent, SEARCH_LABEL);
  assert.match(indicator.root.getAttribute("title") ?? "", /every search asks first/);
  assert.match(indicator.root.getAttribute("aria-label") ?? "", /Web search/);
});

/** Turning it off again has to actually take the pill down. */
test("an availability that goes away takes the indicator with it", () => {
  const { indicator } = view();

  renderSearchIndicator(indicator, { available: true, detail: "on" });
  renderSearchIndicator(indicator, { available: false, detail: "off" });

  assert.equal(indicator.root.hidden, true);
  assert.equal(indicator.text.textContent, "");
  assert.equal(indicator.root.getAttribute("title"), null);
  assert.equal(indicator.root.getAttribute("aria-label"), null);
});

/**
 * `detail` is a string from the server, and the rule in this codebase is
 * that no string reaching the page is ever assembled into HTML. It goes to
 * `title` and `aria-label` as text, never into markup.
 */
test("a hostile detail cannot put markup on the page", () => {
  const { dom, indicator } = view();

  renderSearchIndicator(indicator, {
    available: true,
    detail: "<img src=x onerror=alert(1)>",
  });

  assert.equal(dom.window.document.querySelectorAll("img").length, 0);
  assert.equal(indicator.root.getAttribute("title"), "<img src=x onerror=alert(1)>");
});

/**
 * Same trap as the context meter: `display: inline-flex` outranks the user
 * agent's `[hidden]` rule, so an unstyled hidden pill would sit in the
 * topbar claiming a capability the build does not have. Asserted against the
 * stylesheet text because jsdom answers `display: none` for a hidden element
 * either way, so a computed style would pass with the bug present.
 */
test("hiding the indicator is spelled out in the stylesheet", () => {
  assert.match(
    css,
    /\.search-indicator\[hidden\]\s*\{[^}]*display:\s*none/,
    "without this rule the pill claims web search in a build that has none",
  );
});

test("index.html carries the elements the indicator writes into", () => {
  const doc = new JSDOM(html).window.document;
  for (const id of ["search-indicator", "search-indicator-text"]) {
    assert.ok(doc.getElementById(id), `index.html is missing #${id}`);
  }
  assert.equal(
    doc.getElementById("search-indicator")?.hasAttribute("hidden"),
    true,
    "the indicator must start hidden: nothing has said the tool is there yet",
  );
});

/** Read-only. It reports a capability, it does not hand anyone a switch. */
test("the indicator is not a control", () => {
  const doc = new JSDOM(html).window.document;
  const root = doc.getElementById("search-indicator");
  assert.equal(root?.tagName, "DIV", "a button would invite clicking it");
  assert.equal(root?.querySelector("button"), null);
});
