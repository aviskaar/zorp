/**
 * Tests for turning a file the answer named into a button.
 *
 * Two things are being pinned. The listing is the only thing that says what
 * is a file, so a code span it does not know must survive as a code span and
 * an ambiguous basename must resolve to nothing. And the text is model
 * output, so a name carrying markup has to reach the page as words, the same
 * rule `markdown.test.ts` exists for.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { renderMarkdown } from "../src/markdown.ts";
import { FILE_LINK_TITLE, linkFiles } from "../src/file-links.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document;
// The markdown renderer calls document.createElement, so it needs a document.
(globalThis as Record<string, unknown>).document = doc;
(globalThis as Record<string, unknown>).Node = dom.window.Node;

/** Render an answer and run the pass over it, collecting what got opened. */
function answer(source: string, paths: string[]) {
  const host = doc.createElement("div") as unknown as HTMLElement;
  renderMarkdown(host, source);
  const opened: string[] = [];
  linkFiles(host, paths, (path) => opened.push(path));
  return { host, opened, paths };
}

/** Click a node, and say whether the handler took the event. */
function click(node: Element): boolean {
  const event = new dom.window.MouseEvent("click", { bubbles: true, cancelable: true });
  node.dispatchEvent(event);
  return event.defaultPrevented;
}

test("a code span naming a listed file becomes a button that opens it", () => {
  const { host, opened } = answer("The PDF is at `report.pdf` (51 KB).", ["report.pdf"]);
  const button = host.querySelector("button.file-link") as HTMLElement | null;
  assert.ok(button, "the code span should have become a button");
  assert.equal(button?.tagName, "BUTTON");
  assert.equal(button?.getAttribute("type"), "button");
  assert.equal(button?.textContent, "report.pdf");
  assert.equal(button?.dataset.path, "report.pdf");
  assert.equal(button?.getAttribute("title"), FILE_LINK_TITLE);
  assert.equal(host.querySelectorAll("code.inline-code").length, 0);

  click(button as unknown as Element);
  assert.deepEqual(opened, ["report.pdf"]);
});

test("a code span naming nothing in the listing is left alone", () => {
  const { host, opened } = answer("Use a `ClusterIP` and not a `NodePort`.", ["report.pdf"]);
  assert.equal(host.querySelectorAll("button.file-link").length, 0);
  assert.equal(host.querySelectorAll("code.inline-code").length, 2);
  assert.deepEqual(opened, []);
});

test("a basename matching one listed file resolves to that file's full path", () => {
  const { host, opened } = answer("Written to `report.pdf`.", ["scratch/report.pdf"]);
  const button = host.querySelector("button.file-link") as HTMLElement | null;
  assert.ok(button, "the basename should have resolved");
  // The words are the answer's, the path is the listing's.
  assert.equal(button?.textContent, "report.pdf");
  assert.equal(button?.dataset.path, "scratch/report.pdf");
  click(button as unknown as Element);
  assert.deepEqual(opened, ["scratch/report.pdf"]);
});

test("an ambiguous basename is left alone", () => {
  const { host, opened } = answer("Written to `report.pdf`.", [
    "scratch/report.pdf",
    "old/report.pdf",
  ]);
  assert.equal(host.querySelectorAll("button.file-link").length, 0);
  assert.equal(host.querySelector("code.inline-code")?.textContent, "report.pdf");
  assert.deepEqual(opened, []);
});

test("a leading ./ resolves against the listing", () => {
  const { host, opened } = answer("It is at `./notes.md`.", ["notes.md"]);
  const button = host.querySelector("button.file-link") as HTMLElement | null;
  assert.equal(button?.dataset.path, "notes.md");
  click(button as unknown as Element);
  assert.deepEqual(opened, ["notes.md"]);
});

test("a markdown link to a listed file opens the pane instead of navigating", () => {
  const { host, opened } = answer("See [the report](/scratch/report.pdf).", [
    "scratch/report.pdf",
  ]);
  assert.equal(host.querySelectorAll("a").length, 0, "no anchor should be left to navigate");
  const button = host.querySelector("button.file-link") as HTMLElement | null;
  assert.ok(button, "the link should have become a button");
  assert.equal(button?.textContent, "the report");
  assert.equal(button?.dataset.path, "scratch/report.pdf");
  assert.equal(click(button as unknown as Element), true, "the click should be taken");
  assert.deepEqual(opened, ["scratch/report.pdf"]);
});

test("a link that leaves this site is never captured by a matching name", () => {
  const { host, opened } = answer("See [it](https://example.com/report.pdf).", ["report.pdf"]);
  assert.equal(host.querySelectorAll("button.file-link").length, 0);
  assert.equal(host.querySelector("a")?.getAttribute("href"), "https://example.com/report.pdf");
  assert.deepEqual(opened, []);
});

test("running the pass twice does not wrap the same reference twice", () => {
  const { host, opened, paths } = answer("At `report.pdf`.", ["report.pdf"]);
  linkFiles(host, paths, (path) => opened.push(path));
  const buttons = host.querySelectorAll("button.file-link");
  assert.equal(buttons.length, 1);
  assert.equal(buttons[0].querySelectorAll("button").length, 0, "no button inside a button");
  assert.equal(buttons[0].textContent, "report.pdf");
});

test("a file name carrying markup lands as text and builds no element", () => {
  const name = '<img src=x onerror="alert(1)">.md';
  const { host } = answer("Written to `" + name + "`.", [name]);
  const button = host.querySelector("button.file-link") as HTMLElement | null;
  assert.ok(button, "the listing knows this name, so it links");
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(button?.querySelectorAll("*").length, 0, "the name is text, not markup");
  assert.equal(button?.textContent, name);
  assert.equal(button?.dataset.path, name);
});

test("an empty listing changes nothing", () => {
  const { host } = answer("At `report.pdf`.", []);
  assert.equal(host.querySelectorAll("button.file-link").length, 0);
  assert.equal(host.querySelectorAll("code.inline-code").length, 1);
});
