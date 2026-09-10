/**
 * The seam where the older conversation became a summary.
 *
 * These exist because of what a summary is. It is a paragraph a model wrote
 * about a conversation that contained tool results and fetched pages, and
 * it goes on the page. That is the shape every injection test in this repo
 * is about, so it gets the same treatment as an answer: it lands as text or
 * it does not land.
 *
 * The second thing being pinned here is that the marker is closed and reads
 * as a marker. A summary drawn open, or drawn through the markdown
 * renderer, starts to look like part of the conversation, and the whole
 * point of this component is that it is not.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  MODEL_WRITTEN,
  compactionMarker,
  markerLabel,
} from "../src/compaction-marker.ts";

function fixture(): Document {
  const dom = new JSDOM("<!doctype html><body><div id=host></div></body>");
  return dom.window.document as unknown as Document;
}

function render(summary: string, over: Record<string, number> = {}): HTMLElement {
  const doc = fixture();
  const node = compactionMarker(doc, {
    messages: 12,
    tokens_before: 92_000,
    tokens_after: 12_400,
    summary,
    ...over,
  });
  doc.body.querySelector("#host")!.append(node);
  return node;
}

/* ------------------------------------------------------------------ */
/* a summary is model output                                           */
/* ------------------------------------------------------------------ */

test("a summary that looks like markup lands as text", () => {
  const node = render("## Requests and intent\n<img src=x onerror=alert(1)>");

  assert.equal(node.querySelectorAll("img").length, 0);
  assert.match(node.querySelector(".compaction-body")!.textContent!, /<img src=x onerror=alert\(1\)>/);
});

test("a summary is not run through the markdown renderer", () => {
  const node = render("## Requests and intent\n1. write hello.txt\n\n**bold**");

  // The headings stay as the characters they are. Drawn as headings, this
  // would start to look like part of the conversation.
  assert.equal(node.querySelectorAll("h1, h2, h3, strong, ol, li").length, 0);
  assert.match(node.querySelector(".compaction-body")!.textContent!, /## Requests and intent/);
  assert.match(node.querySelector(".compaction-body")!.textContent!, /\*\*bold\*\*/);
});

test("the summary is labelled as model-written", () => {
  const node = render("## Requests and intent\nNone.");

  assert.equal(node.querySelector(".compaction-source")!.textContent, MODEL_WRITTEN);
});

/* ------------------------------------------------------------------ */
/* it reads as a marker                                                */
/* ------------------------------------------------------------------ */

test("the marker is closed", () => {
  const node = render("## Requests and intent\nNone.") as HTMLDetailsElement;

  assert.equal(node.tagName, "DETAILS");
  assert.equal(node.open, false);
});

test("the label says what went and what it cost", () => {
  assert.equal(
    markerLabel({
      messages: 12,
      tokens_before: 92_000,
      tokens_after: 12_400,
      summary: "",
    }),
    "Context compacted: 12 messages summarized, about 92k tokens to 12.4k",
  );
});

/** These are byte estimates unless a provider reported usage, and the
 * `context` frame calls that `estimated`. A count that looked exact would
 * claim a precision it does not have. */
test("the label says about, because the numbers are estimates", () => {
  assert.match(
    markerLabel({ messages: 3, tokens_before: 900, tokens_after: 120, summary: "" }),
    /about 900 tokens to 120/,
  );
});

test("one message is not one messages", () => {
  assert.match(
    markerLabel({ messages: 1, tokens_before: 900, tokens_after: 120, summary: "" }),
    /1 message summarized/,
  );
});

/** A summary the server sent as an empty string still draws a marker: the
 * compaction happened, and a seam with nothing under it is more honest than
 * no seam at all. */
test("an empty summary still draws the marker", () => {
  const node = render("");

  assert.ok(node.querySelector(".compaction-label"));
  assert.equal(node.querySelector(".compaction-body")!.textContent, "");
});
