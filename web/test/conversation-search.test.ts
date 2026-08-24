/**
 * The conversation search results list.
 *
 * The first block is the one that matters, for the same reason
 * `markdown.test.ts` and `panel-view.test.ts` say it: a search result is a
 * snippet of a conversation, and a conversation is a model writing about
 * files and web pages it has been reading. A result list that assembles
 * markup is a cross-site scripting hole with a text box in front of it.
 *
 * The second block is about refusing a payload that is not the shape it
 * claims to be, which is the discipline `session-url.ts` applies to the
 * address bar, applied here to the response body.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  SNIPPET_CHARS,
  coerceHits,
  renderNotice,
  renderResults,
  summarize,
  type RecallHit,
} from "../src/conversation-search.ts";

function fixture(): { doc: Document; list: HTMLElement } {
  const dom = new JSDOM("<!doctype html><body><ul id='results'></ul></body>");
  const doc = dom.window.document as unknown as Document;
  return { doc, list: doc.querySelector("#results") as HTMLElement };
}

function hit(over: Partial<RecallHit> = {}): RecallHit {
  return {
    id: "conv-1",
    title: "Sorting out the account",
    seq: 0,
    role: "user",
    snippet: "the customer wants a refund",
    score: 0.9,
    ...over,
  };
}

/* ---------------------------------------------------------------- */
/* nothing on this page is ever markup                               */
/* ---------------------------------------------------------------- */

test("a script tag in a snippet becomes text, not a script", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ snippet: "<script>alert(1)</script>" })], () => {});
  assert.equal(list.querySelectorAll("script").length, 0);
  assert.ok(list.textContent?.includes("<script>alert(1)</script>"));
});

test("an img with an onerror handler never becomes an element", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ snippet: '<img src=x onerror="alert(1)">' })], () => {});
  assert.equal(list.querySelectorAll("img").length, 0);
  assert.equal(list.querySelectorAll("*[onerror]").length, 0);
});

test("a title carrying markup becomes text", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ title: "<b>bold</b>" })], () => {});
  assert.equal(list.querySelectorAll("b").length, 0);
  assert.ok(list.textContent?.includes("<b>bold</b>"));
});

test("a notice carrying markup becomes text", () => {
  const { doc, list } = fixture();
  renderNotice(doc, list, "<iframe src=javascript:alert(1)></iframe>");
  assert.equal(list.querySelectorAll("iframe").length, 0);
  assert.ok(list.textContent?.includes("<iframe"));
});

/* ---------------------------------------------------------------- */
/* a response is checked, not trusted                                */
/* ---------------------------------------------------------------- */

test("rows that are not the shape they claim are dropped", () => {
  const rows = coerceHits([
    null,
    "a string",
    42,
    {},
    { id: "" },
    { id: 7, title: "numeric id" },
    { id: "conv-ok", title: "fine", seq: 3, role: "user", snippet: "text", score: 0.4 },
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].id, "conv-ok");
});

test("a body that is not an array is no results, not a crash", () => {
  assert.deepEqual(coerceHits(null), []);
  assert.deepEqual(coerceHits({ hits: [] }), []);
  assert.deepEqual(coerceHits("hits"), []);
});

test("missing fields get usable defaults rather than undefined on the page", () => {
  const [row] = coerceHits([{ id: "conv-1" }]);
  assert.equal(row.title, "");
  assert.equal(row.snippet, "");
  assert.equal(row.role, "");
  assert.equal(row.seq, 0);
  assert.equal(row.score, 0);
});

test("a very long snippet is cut down before it reaches the page", () => {
  const long = "x".repeat(SNIPPET_CHARS * 4);
  const { doc, list } = fixture();
  renderResults(doc, list, coerceHits([{ id: "conv-1", snippet: long }]), () => {});
  const shown = list.querySelector(".recall-snippet")?.textContent ?? "";
  assert.ok(shown.length <= SNIPPET_CHARS + 1, `snippet was ${shown.length} characters`);
});

/* ---------------------------------------------------------------- */
/* the list itself                                                   */
/* ---------------------------------------------------------------- */

test("each result is a button that reports the conversation it names", () => {
  const { doc, list } = fixture();
  const picked: string[] = [];
  renderResults(doc, list, [hit({ id: "conv-a" }), hit({ id: "conv-b" })], (id) =>
    picked.push(id),
  );
  const buttons = Array.from(list.querySelectorAll("button"));
  assert.equal(buttons.length, 2);
  (buttons[1] as HTMLButtonElement).click();
  assert.deepEqual(picked, ["conv-b"]);
});

test("a result is reachable and named for a screen reader", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ title: "Sorting out the account" })], () => {});
  const button = list.querySelector("button") as HTMLButtonElement;
  assert.equal(button.type, "button");
  assert.ok(
    (button.getAttribute("aria-label") ?? "").includes("Sorting out the account"),
    "the button did not announce which conversation it opens",
  );
});

test("a conversation with no title still says something", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ title: "" })], () => {});
  assert.ok((list.textContent ?? "").includes("Untitled"));
});

test("no results is a sentence, not an empty list", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [], () => {});
  assert.equal(list.querySelectorAll("button").length, 0);
  assert.ok((list.textContent ?? "").trim().length > 0);
});

test("rendering replaces what was there rather than appending to it", () => {
  const { doc, list } = fixture();
  renderResults(doc, list, [hit({ id: "conv-a" })], () => {});
  renderResults(doc, list, [hit({ id: "conv-b" })], () => {});
  assert.equal(list.querySelectorAll("button").length, 1);
});

/* ---------------------------------------------------------------- */
/* what the status line says                                         */
/* ---------------------------------------------------------------- */

test("an unavailable search says why, in the server's own words", () => {
  const line = summarize({
    available: false,
    reason: "no local embedder answered at http://127.0.0.1:11434",
    endpoint: null,
    model: null,
    conversations: 0,
    indexed_conversations: 0,
    chunks: 0,
    running: false,
    ready: false,
  });
  assert.match(line, /no local embedder/);
});

test("an unavailable search with no reason still says something", () => {
  const line = summarize({
    available: false,
    reason: null,
    endpoint: null,
    model: null,
    conversations: 0,
    indexed_conversations: 0,
    chunks: 0,
    running: false,
    ready: false,
  });
  assert.ok(line.trim().length > 0);
});

test("catch-up says how much is indexed, rather than looking like no search results", () => {
  const line = summarize({
    available: true,
    reason: null,
    endpoint: "http://127.0.0.1:11434",
    model: "nomic-embed-text",
    conversations: 3,
    indexed_conversations: 0,
    chunks: 0,
    running: false,
    ready: false,
  });
  assert.match(line, /0 of 3 conversations indexed/i);
  assert.doesNotMatch(line, /nothing close enough/i);
});

test("a running pass says it is indexing and reports its counts", () => {
  const line = summarize({
    available: true,
    reason: null,
    endpoint: "http://127.0.0.1:11434",
    model: "nomic-embed-text",
    conversations: 3,
    indexed_conversations: 1,
    chunks: 2,
    running: true,
    ready: false,
  });
  assert.match(line, /indexing/i);
  assert.match(line, /1 of 3/);
});

test("a populated index reports its size and the model behind it", () => {
  const line = summarize({
    available: true,
    reason: null,
    endpoint: "http://127.0.0.1:11434",
    model: "nomic-embed-text",
    conversations: 1,
    indexed_conversations: 1,
    chunks: 4,
    running: false,
    ready: true,
  });
  assert.match(line, /ready/i);
  assert.match(line, /1 conversation\b/);
  assert.match(line, /nomic-embed-text/);
});
