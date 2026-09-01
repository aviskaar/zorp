/**
 * A row in the session sidebar.
 *
 * These exist because of what a title is now. It used to be the first
 * message the user typed, cut off. It is now, when the server managed to
 * write one, a short name a model wrote after reading that message and the
 * reply to it. A model that has been reading tool results and fetched pages
 * writing a string that goes on the page is the shape every injection test
 * in this repo is about, so a title gets the same treatment as an answer:
 * it lands as text or it does not land.
 *
 * The second block is about the fallback. A titling call can fail, be
 * refused, or decline, and the server then sends the verbatim first message
 * instead. A row must never come out blank, and it must never come out
 * showing a placeholder in place of a message the user can recognise.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { UNTITLED, emptySessionRow, sessionRow } from "../src/session-row.ts";
import type { SessionSummary } from "../src/api.ts";

function fixture(): Document {
  const dom = new JSDOM("<!doctype html><body><ul id=list></ul></body>");
  return dom.window.document as unknown as Document;
}

function summary(over: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id: "s1",
    title: "ERBGA library walkthrough",
    updated_at: "2026-08-22T10:00:00Z",
    ...over,
  };
}

function render(doc: Document, session: SessionSummary, active = false): HTMLElement {
  const row = sessionRow(doc, session, {
    active,
    when: "2m ago",
    onOpen: () => {},
    onDelete: () => {},
  });
  doc.body.querySelector("#list")!.append(row);
  return row;
}

/* ------------------------------------------------------------------ */
/* a title is model output                                             */
/* ------------------------------------------------------------------ */

test("a title that looks like markup lands as text", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: "<img src=x onerror=alert(1)>" }));

  assert.equal(row.querySelectorAll("img").length, 0);
  assert.equal(
    row.querySelector(".session-title")!.textContent,
    "<img src=x onerror=alert(1)>",
  );
});

test("a title carrying a script tag creates no script element", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: "<script>alert(1)</script>" }));

  assert.equal(row.querySelectorAll("script").length, 0);
  assert.equal(row.querySelector(".session-title")!.textContent, "<script>alert(1)</script>");
});

test("a title carrying an anchor creates no link out of the sidebar", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: '<a href="https://evil.example">click</a>' }));

  assert.equal(row.querySelectorAll("a").length, 0);
});

/** The row has exactly two children and both are spans. A renderer that
 * started assembling HTML would show up here before it showed up as a
 * missing escape. */
test("a row is two text spans and nothing else", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: "<b>bold</b> &amp; <i>italic</i>" }));

  const button = row.querySelector(".session-button")!;
  assert.equal(button.children.length, 2);
  assert.equal(button.children[0].tagName, "SPAN");
  assert.equal(button.children[1].tagName, "SPAN");
  assert.equal(button.querySelectorAll("b, i").length, 0);
  assert.equal(button.children[0].textContent, "<b>bold</b> &amp; <i>italic</i>");
});

/* ------------------------------------------------------------------ */
/* the fallback                                                        */
/* ------------------------------------------------------------------ */

/**
 * The server sends the verbatim first message when nothing named the
 * session, so this is what a failed or declined titling call looks like
 * from here: exactly what the sidebar showed before the feature existed.
 */
test("a session with no generated title shows whatever the server sent", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: "read erbga/src/lib.rs and tell me what it does" }));

  assert.equal(
    row.querySelector(".session-title")!.textContent,
    "read erbga/src/lib.rs and tell me what it does",
  );
});

test("a session with no title at all is labelled rather than left blank", () => {
  const doc = fixture();
  const row = render(doc, summary({ title: "" }));

  assert.equal(row.querySelector(".session-title")!.textContent, UNTITLED);
});

test("an empty list says so", () => {
  const doc = fixture();
  assert.equal(emptySessionRow(doc).textContent, "No sessions yet.");
});

/* ------------------------------------------------------------------ */
/* the rest of the row                                                 */
/* ------------------------------------------------------------------ */

test("the open session is marked for a screen reader as well as for the eye", () => {
  const doc = fixture();
  const button = render(doc, summary(), true).querySelector(".session-button")!;

  assert.ok(button.classList.contains("is-active"));
  assert.equal(button.getAttribute("aria-current"), "true");
});

test("a row that is not the open one claims nothing", () => {
  const doc = fixture();
  const button = render(doc, summary(), false).querySelector(".session-button")!;

  assert.ok(!button.classList.contains("is-active"));
  assert.equal(button.getAttribute("aria-current"), null);
});

test("clicking a row opens that session", () => {
  const doc = fixture();
  const opened: string[] = [];
  const session = summary({ id: "s7" });
  const row = sessionRow(doc, session, {
    active: false,
    when: "2m ago",
    onOpen: (chosen) => opened.push(chosen.id),
    onDelete: () => {},
  });

  (row.querySelector(".session-button") as HTMLButtonElement).click();

  assert.deepEqual(opened, ["s7"]);
});

/** The id goes in a data attribute, which `markActiveSession` reads back
 * to move the highlight without redrawing the list. */
test("the row carries its session id", () => {
  const doc = fixture();
  const button = render(doc, summary({ id: "18cdefd3142f21b0-9dc2" })).querySelector(
    ".session-button",
  ) as HTMLButtonElement;

  assert.equal(button.dataset.id, "18cdefd3142f21b0-9dc2");
});

/* ------------------------------------------------------------------ */
/* the three-dot menu and delete                                       */
/* ------------------------------------------------------------------ */

test("the menu is closed until the kebab button is clicked", () => {
  const doc = fixture();
  const row = render(doc, summary());

  assert.equal(row.querySelector(".session-menu")!.hasAttribute("hidden"), true);
  (row.querySelector(".session-menu-btn") as HTMLButtonElement).click();
  assert.equal(row.querySelector(".session-menu")!.hasAttribute("hidden"), false);
});

test("clicking the kebab button a second time closes the menu again", () => {
  const doc = fixture();
  const row = render(doc, summary());
  const menuBtn = row.querySelector(".session-menu-btn") as HTMLButtonElement;

  menuBtn.click();
  menuBtn.click();

  assert.equal(row.querySelector(".session-menu")!.hasAttribute("hidden"), true);
});

test("opening the menu never opens the session", () => {
  const doc = fixture();
  const opened: string[] = [];
  const row = sessionRow(doc, summary(), {
    active: false,
    when: "2m ago",
    onOpen: (chosen) => opened.push(chosen.id),
    onDelete: () => {},
  });

  (row.querySelector(".session-menu-btn") as HTMLButtonElement).click();

  assert.deepEqual(opened, []);
});

/**
 * Delete asks first, in the browser's own dialog, and only tells the
 * caller once that comes back true. `window.confirm` is stubbed rather
 * than driven for real: jsdom's own implementation is "not implemented"
 * and this test needs to control what it returns.
 */
test("declining the confirmation deletes nothing", () => {
  const doc = fixture();
  const view = doc.defaultView as unknown as { confirm: () => boolean };
  view.confirm = () => false;
  const deleted: string[] = [];
  const row = sessionRow(doc, summary({ id: "s9" }), {
    active: false,
    when: "2m ago",
    onOpen: () => {},
    onDelete: (chosen) => deleted.push(chosen.id),
  });

  (row.querySelector(".session-menu-btn") as HTMLButtonElement).click();
  (row.querySelector(".session-delete") as HTMLButtonElement).click();

  assert.deepEqual(deleted, []);
});

test("confirming the dialog calls onDelete with the session and closes the menu", () => {
  const doc = fixture();
  const view = doc.defaultView as unknown as { confirm: () => boolean };
  view.confirm = () => true;
  const deleted: string[] = [];
  const row = sessionRow(doc, summary({ id: "s9" }), {
    active: false,
    when: "2m ago",
    onOpen: () => {},
    onDelete: (chosen) => deleted.push(chosen.id),
  });

  (row.querySelector(".session-menu-btn") as HTMLButtonElement).click();
  (row.querySelector(".session-delete") as HTMLButtonElement).click();

  assert.deepEqual(deleted, ["s9"]);
  assert.equal(row.querySelector(".session-menu")!.hasAttribute("hidden"), true);
});

/** The confirmation text names the conversation rather than reading as a
 * generic warning, so deleting the wrong row from a long list is caught
 * before the dialog is even dismissed. */
test("the confirmation names the conversation being deleted", () => {
  const doc = fixture();
  const view = doc.defaultView as unknown as { confirm: (message?: string) => boolean };
  let asked = "";
  view.confirm = (message) => {
    asked = message ?? "";
    return false;
  };
  const row = sessionRow(doc, summary({ title: "Writing hello.txt" }), {
    active: false,
    when: "2m ago",
    onOpen: () => {},
    onDelete: () => {},
  });

  (row.querySelector(".session-menu-btn") as HTMLButtonElement).click();
  (row.querySelector(".session-delete") as HTMLButtonElement).click();

  assert.match(asked, /Writing hello\.txt/);
});
