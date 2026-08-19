/**
 * Tests for the two stylesheet rules the artifact pane's layout rests on.
 * Both were missing, and each broke something a user could see.
 *
 * jsdom has no layout engine, so nothing here measures a scroll position or a
 * column height. Worse, it does not model the cascade the way a browser does
 * in the one case that matters below, which is why the second test reads the
 * stylesheet as text instead of asking for a computed value. Where a computed
 * value is trustworthy, it is used.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

/** The app shell, cut down to the grid and the pane inside it. */
function shell(): JSDOM {
  return new JSDOM(
    `<style>${css}</style>
     <div class="app">
       <aside class="sidebar"></aside>
       <section class="main"></section>
       <aside class="artifacts" id="artifacts"></aside>
     </div>`,
  );
}

/**
 * A grid item defaults to `min-height: auto`, which refuses to shrink below
 * its content. Without an explicit zero, a long document grew the pane to the
 * full height of the file: `overflow-y` on `.artifact-doc` had nothing left to
 * clip so the document would not scroll, and the stretched grid row dragged
 * the conversation column down with it.
 */
test("the artifact pane can shrink below its content", () => {
  const dom = shell();
  const pane = dom.window.document.querySelector("#artifacts")!;
  assert.equal(
    dom.window.getComputedStyle(pane).minHeight,
    "0px",
    "the pane will grow to the height of whatever document it is showing",
  );
});

/**
 * `display: flex` on `.artifacts` outranks the user agent's
 * `[hidden] { display: none }`, so hiding the pane has to be spelled out.
 * Without it the closed pane kept its place in the grid, and with the grid
 * down to two columns it wrapped onto a second row underneath the sidebar:
 * closing the files put a copy of them below the session list.
 *
 * This one asserts on the stylesheet's text rather than on a computed style,
 * because jsdom answers `display: none` for a `hidden` element whether or not
 * the rule is there. Asking it the question a browser would answer correctly
 * gets an answer that is right for the wrong reason, and a test that passes
 * with the bug present is worse than no test.
 */
test("hiding the artifact pane is spelled out in the stylesheet", () => {
  assert.match(
    css,
    /\.artifacts\[hidden\]\s*\{[^}]*display:\s*none/,
    "without this rule the closed pane still takes a grid slot",
  );
});
