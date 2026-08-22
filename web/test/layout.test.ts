/**
 * Tests for the app shell's layout: the stylesheet rules the artifact pane
 * rests on, and the module that lets a reader resize and collapse the panes
 * around the conversation.
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
import {
  ARTIFACTS_MIN,
  MAIN_MIN,
  PaneResizer,
  SIDEBAR_MAX,
  SIDEBAR_MIN,
  artifactsBounds,
  clampTo,
  layoutStore,
  readLayout,
  saveCollapsed,
  saveWidth,
  setSidebarCollapsed,
  sidebarBounds,
  sidebarIsCollapsed,
} from "../src/layout.ts";

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

/**
 * The file popover has a `display: flex` of its own, so it has the same bug
 * waiting for it, and this one would leave the file list permanently on
 * screen over the conversation.
 */
test("hiding the file popover is spelled out in the stylesheet", () => {
  assert.match(
    css,
    /\.files-popover\[hidden\]\s*\{[^}]*display:\s*none/,
    "without this rule the closed popover still covers the conversation",
  );
});

/**
 * Collapsing has to be scoped to the wide layout. Below 820px the sidebar is
 * already a drawer held open by a class, and a `display: none` reaching down
 * there would make the drawer unopenable: the hamburger would toggle a state
 * with nothing to show for it.
 */
test("the collapsed sidebar is a wide-layout rule only", () => {
  const wide = css.slice(css.indexOf("@media (min-width: 821px)"));
  assert.match(
    wide,
    /\.app\[data-sidebar="collapsed"\]\s+\.sidebar\s*\{[^}]*visibility:\s*hidden/,
    "the collapsed column has to actually go away",
  );
  // And it must go away by hiding rather than by leaving the grid. A grid
  // item set to `display: none` stops being an item, so the conversation
  // auto-places into the collapsed 0px column and disappears with it. That
  // was the first version of this rule and it took the whole page down.
  assert.doesNotMatch(
    wide,
    /\.app\[data-sidebar="collapsed"\]\s+\.sidebar\s*\{[^}]*display:\s*none/,
    "hiding it with display would move the conversation into the empty column",
  );
  assert.match(
    wide,
    /\.app\[data-sidebar="collapsed"\]\s+\.menu-btn\s*\{[^}]*display:\s*inline-flex/,
    "with the column gone the hamburger is the only way back, so it must show",
  );
  const narrow = css.slice(
    css.indexOf("@media (max-width: 820px)"),
    css.indexOf("@media (min-width: 821px)"),
  );
  assert.doesNotMatch(
    narrow,
    /\[data-sidebar="collapsed"\]/,
    "the narrow layout must not know about collapsing",
  );
});

/* ------------------------------------------------------------------ */
/* pane widths                                                         */
/* ------------------------------------------------------------------ */

test("a pane cannot be dragged to nothing or past its own ceiling", () => {
  const room = { viewport: 1920, other: 0 };
  assert.equal(clampTo(0, sidebarBounds(room)), SIDEBAR_MIN);
  assert.equal(clampTo(-400, sidebarBounds(room)), SIDEBAR_MIN);
  assert.equal(clampTo(99999, sidebarBounds(room)), SIDEBAR_MAX);
  assert.equal(clampTo(Number.NaN, sidebarBounds(room)), SIDEBAR_MIN);
  assert.equal(clampTo(300, sidebarBounds(room)), 300);
});

/**
 * The point of the whole clamp. Whatever the two side panes are doing, the
 * conversation keeps `MAIN_MIN`, because a layout control that can squeeze
 * the thing being read down to a gutter is a way to break the page by
 * accident and then not know how to undo it.
 */
test("neither pane may be dragged over the conversation", () => {
  // A 1280px window with a 600px artifact pane leaves 680 for the sidebar
  // and the conversation, and the conversation is owed 420 of that, so the
  // sidebar's ceiling drops from 480 to 260.
  const bounds = sidebarBounds({ viewport: 1280, other: 600 });
  assert.equal(bounds.max, 1280 - 600 - MAIN_MIN);
  assert.ok(bounds.max < SIDEBAR_MAX);

  const artifacts = artifactsBounds({ viewport: 1440, other: 264 });
  assert.equal(artifacts.max, 1440 - 264 - MAIN_MIN);

  // A window with no room for both still refuses to return a ceiling under
  // the floor, which would invert the clamp.
  const cramped = artifactsBounds({ viewport: 700, other: 264 });
  assert.equal(cramped.max, ARTIFACTS_MIN);
  assert.equal(clampTo(50, cramped), ARTIFACTS_MIN);
});

/* ------------------------------------------------------------------ */
/* what survives a reload                                              */
/* ------------------------------------------------------------------ */

function fakeStore(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => (map.has(key) ? map.get(key)! : null),
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, value),
  } as unknown as Storage;
}

test("dragged widths and a collapsed sidebar come back after a reload", () => {
  const store = fakeStore();
  saveWidth(store, "sidebar", 331);
  saveWidth(store, "artifacts", 612.4);
  saveCollapsed(store, true);

  const back = readLayout(store);
  assert.equal(back.sidebar, 331);
  assert.equal(back.artifacts, 612, "a fractional drag is stored as whole pixels");
  assert.equal(back.collapsed, true);

  // Resetting a pane forgets its width rather than storing today's default,
  // so a later change to the stylesheet still reaches somebody who reset.
  saveWidth(store, "sidebar", null);
  saveCollapsed(store, false);
  const reset = readLayout(store);
  assert.equal(reset.sidebar, null);
  assert.equal(reset.artifacts, 612);
  assert.equal(reset.collapsed, false);
});

test("a missing or lying store costs the widths and nothing else", () => {
  assert.deepEqual(readLayout(null), { sidebar: null, artifacts: null, collapsed: false });
  // No throw, which is the whole assertion.
  saveWidth(null, "sidebar", 300);
  saveCollapsed(null, true);

  const angry = {
    getItem() {
      throw new Error("this browser refuses site data");
    },
    setItem() {
      throw new Error("quota");
    },
    removeItem() {
      throw new Error("quota");
    },
  } as unknown as Storage;
  assert.deepEqual(readLayout(angry), { sidebar: null, artifacts: null, collapsed: false });
  saveWidth(angry, "artifacts", 500);
  saveCollapsed(angry, true);

  // Node has no `window`, so even asking for the store is the throwing case.
  assert.equal(layoutStore(), null);
});

test("nonsense in the store is ignored rather than applied", () => {
  const store = fakeStore();
  store.setItem("zorp.layout.sidebar", "not a number");
  store.setItem("zorp.layout.artifacts", "-40");
  store.setItem("zorp.layout.sidebarCollapsed", "maybe");
  const back = readLayout(store);
  assert.equal(back.sidebar, null);
  assert.equal(back.artifacts, null, "a negative width is nonsense, not a width");
  assert.equal(back.collapsed, false);
});

/* ------------------------------------------------------------------ */
/* the collapse toggle                                                 */
/* ------------------------------------------------------------------ */

test("collapsing sets the shell's state and the button that undoes it", () => {
  const dom = new JSDOM(`<div id="app"></div><button id="menu" aria-expanded="true"></button>`);
  const app = dom.window.document.getElementById("app") as unknown as HTMLElement;
  const menu = dom.window.document.getElementById("menu") as unknown as HTMLElement;

  assert.equal(sidebarIsCollapsed(app), false);

  setSidebarCollapsed(app, true, menu);
  assert.equal(sidebarIsCollapsed(app), true);
  assert.equal(app.dataset.sidebar, "collapsed");
  assert.equal(
    menu.getAttribute("aria-expanded"),
    "false",
    "the button that brings the sidebar back has to say it is gone",
  );

  setSidebarCollapsed(app, false, menu);
  assert.equal(sidebarIsCollapsed(app), false);
  assert.equal(app.dataset.sidebar, undefined);
  assert.equal(menu.getAttribute("aria-expanded"), "true");
});

/* ------------------------------------------------------------------ */
/* the handles                                                         */
/* ------------------------------------------------------------------ */

interface Rig {
  dom: JSDOM;
  root: HTMLElement;
  handle: HTMLElement;
  resizer: PaneResizer;
  commits: (number | null)[];
  width(): string;
}

function rig(sign: 1 | -1, start = 264): Rig {
  const dom = new JSDOM(`<div id="app"><div id="handle" tabindex="0"></div></div>`);
  (globalThis as Record<string, unknown>).window = dom.window;
  const root = dom.window.document.getElementById("app") as unknown as HTMLElement;
  const handle = dom.window.document.getElementById("handle") as unknown as HTMLElement;
  const commits: (number | null)[] = [];
  const resizer = new PaneResizer({
    handle,
    root,
    property: "--w",
    sign,
    bounds: () => ({ min: 180, max: 480 }),
    measure: () => start,
    onCommit: (px) => commits.push(px),
  });
  return { dom, root, handle, resizer, commits, width: () => root.style.getPropertyValue("--w") };
}

/** jsdom has no PointerEvent, and a MouseEvent carries everything used here. */
function point(dom: JSDOM, type: string, clientX: number): Event {
  return new dom.window.MouseEvent(type, { clientX, bubbles: true }) as unknown as Event;
}

function press(dom: JSDOM, key: string, shiftKey = false): Event {
  const event = new dom.window.KeyboardEvent("keydown", { key, shiftKey, bubbles: true });
  return event as unknown as Event;
}

test("dragging the handle moves the pane edge and saves once, at the end", () => {
  const it = rig(1);
  it.handle.dispatchEvent(point(it.dom, "pointerdown", 264));
  assert.equal(it.root.dataset.resizing, "yes");
  assert.equal(it.handle.dataset.dragging, "yes");

  it.dom.window.dispatchEvent(point(it.dom, "pointermove", 324));
  assert.equal(it.width(), "324px");
  assert.deepEqual(it.commits, [], "a drag in progress must not hit the store every frame");

  it.dom.window.dispatchEvent(point(it.dom, "pointerup", 324));
  assert.deepEqual(it.commits, [324]);
  assert.equal(it.root.dataset.resizing, undefined);
  assert.equal(it.handle.dataset.dragging, undefined);

  // And nothing moves after the pointer is up.
  it.dom.window.dispatchEvent(point(it.dom, "pointermove", 400));
  assert.equal(it.width(), "324px");
});

test("a pane on the right of its handle grows when the pointer goes left", () => {
  const it = rig(-1, 300);
  it.handle.dispatchEvent(point(it.dom, "pointerdown", 1000));
  it.dom.window.dispatchEvent(point(it.dom, "pointermove", 900));
  assert.equal(it.width(), "400px", "dragging left widens the right-hand pane");
  it.dom.window.dispatchEvent(point(it.dom, "pointermove", 400));
  assert.equal(it.width(), "480px", "and it still stops at the ceiling");
  it.dom.window.dispatchEvent(point(it.dom, "pointerup", 400));
  assert.deepEqual(it.commits, [480]);
});

test("a drag past the limits stops at them", () => {
  const it = rig(1);
  it.handle.dispatchEvent(point(it.dom, "pointerdown", 264));
  it.dom.window.dispatchEvent(point(it.dom, "pointermove", -5000));
  assert.equal(it.width(), "180px");
  it.dom.window.dispatchEvent(point(it.dom, "pointermove", 5000));
  assert.equal(it.width(), "480px");
  it.dom.window.dispatchEvent(point(it.dom, "pointerup", 5000));
});

/**
 * A pointer-only resize is a resize some people cannot do. Arrow keys move
 * the edge, Home and End take it to its limits, and both of those mean the
 * far edge of the pane rather than a compass direction, which is why the
 * right-hand pane reads them the other way round.
 */
test("the handle answers the keyboard", () => {
  const left = rig(1);
  left.handle.dispatchEvent(press(left.dom, "ArrowRight"));
  assert.equal(left.width(), "280px");
  left.handle.dispatchEvent(press(left.dom, "ArrowLeft"));
  assert.equal(left.width(), "264px");
  left.handle.dispatchEvent(press(left.dom, "ArrowRight", true));
  assert.equal(left.width(), "328px", "Shift moves it further");
  left.handle.dispatchEvent(press(left.dom, "Home"));
  assert.equal(left.width(), "180px");
  left.handle.dispatchEvent(press(left.dom, "End"));
  assert.equal(left.width(), "480px");
  assert.deepEqual(left.commits, [280, 264, 328, 180, 480], "each keypress is a saved change");

  const right = rig(-1, 400);
  right.handle.dispatchEvent(press(right.dom, "ArrowLeft"));
  assert.equal(right.width(), "416px");
  right.handle.dispatchEvent(press(right.dom, "Home"));
  assert.equal(right.width(), "480px", "Home is the far edge, so for this pane it is widest");

  // A key the handle has no use for is left alone for the page to handle.
  const other = rig(1);
  other.handle.dispatchEvent(press(other.dom, "Tab"));
  assert.equal(other.width(), "");
  assert.deepEqual(other.commits, []);
});

test("a double click gives the stylesheet its default back", () => {
  const it = rig(1);
  it.handle.dispatchEvent(press(it.dom, "ArrowRight"));
  assert.equal(it.width(), "280px");

  const dblclick = new it.dom.window.MouseEvent("dblclick", { bubbles: true });
  it.handle.dispatchEvent(dblclick as unknown as Event);
  assert.equal(it.width(), "", "the property is removed, not set to a number");
  assert.equal(it.commits.at(-1), null, "and the store forgets rather than remembering 264");

  // Enter does the same thing, for somebody who never picked up a pointer.
  it.handle.dispatchEvent(press(it.dom, "ArrowRight"));
  assert.equal(it.width(), "280px");
  it.handle.dispatchEvent(press(it.dom, "Enter"));
  assert.equal(it.width(), "");
});

test("the handle reports where it stands", () => {
  const it = rig(1);
  assert.equal(it.handle.getAttribute("aria-valuemin"), "180");
  assert.equal(it.handle.getAttribute("aria-valuemax"), "480");
  assert.equal(
    it.handle.getAttribute("aria-valuenow"),
    "264",
    "with no width set it reports the one the browser is using",
  );
  it.handle.dispatchEvent(press(it.dom, "ArrowRight"));
  assert.equal(it.handle.getAttribute("aria-valuenow"), "280");
});

/**
 * A window that got smaller can leave a saved pane sitting over the
 * conversation, which is the one case where a width nobody touched this
 * session has to change on its own.
 */
test("a shrinking window pulls a too-wide pane back inside its limits", () => {
  const dom = new JSDOM(`<div id="app"><div id="handle"></div></div>`);
  (globalThis as Record<string, unknown>).window = dom.window;
  const root = dom.window.document.getElementById("app") as unknown as HTMLElement;
  const handle = dom.window.document.getElementById("handle") as unknown as HTMLElement;
  let ceiling = 900;
  const resizer = new PaneResizer({
    handle,
    root,
    property: "--w",
    sign: 1,
    bounds: () => ({ min: 180, max: ceiling }),
    measure: () => 264,
    onCommit: () => {},
  });

  resizer.set(800);
  assert.equal(root.style.getPropertyValue("--w"), "800px");
  ceiling = 400;
  resizer.reclamp();
  assert.equal(root.style.getPropertyValue("--w"), "400px");
  assert.equal(handle.getAttribute("aria-valuemax"), "400");
});
