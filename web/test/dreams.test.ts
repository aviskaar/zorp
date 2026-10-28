/**
 * The "While you were away" card.
 *
 * The block that matters: a suggestion's `wording` is model-authored and the
 * model wrote it after reading the person's own conversations, so it is as
 * attacker-shaped as a recalled snippet. Every case here checks it lands as
 * text, never as markup, and that the lead-in saying which kind it is comes
 * from code rather than from that wording.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { suggestionItem } from "../src/dreams.ts";
import type { DreamSuggestion } from "../src/api.ts";

function fixture(): Document {
  const dom = new JSDOM("<!doctype html><body></body>");
  return dom.window.document as unknown as Document;
}

function suggestion(over: Partial<DreamSuggestion> = {}): DreamSuggestion {
  return {
    id: "sug-1",
    kind: "recurring_request",
    wording: "You keep asking for the Monday report. Want a flavor for it?",
    evidence_ids: ["conv-a", "conv-b"],
    created: 0,
    ...over,
  };
}

/* ---------------------------------------------------------------- */
/* nothing a model wrote is ever markup                              */
/* ---------------------------------------------------------------- */

test("a script tag in the wording becomes text, not a script", () => {
  const doc = fixture();
  const item = suggestionItem(
    doc,
    suggestion({ wording: "<script>alert(1)</script>" }),
    () => {},
    () => {},
  );
  assert.equal(item.querySelectorAll("script").length, 0);
  assert.ok(item.textContent?.includes("<script>alert(1)</script>"));
});

test("an img with an onerror handler never becomes an element", () => {
  const doc = fixture();
  const item = suggestionItem(
    doc,
    suggestion({ wording: "<img src=x onerror=alert(1)>" }),
    () => {},
    () => {},
  );
  assert.equal(item.querySelectorAll("img").length, 0);
});

/* ---------------------------------------------------------------- */
/* the kind is said in code, not taken from the wording             */
/* ---------------------------------------------------------------- */

test("the lead-in names the kind from code", () => {
  const doc = fixture();
  const cluster = suggestionItem(doc, suggestion({ kind: "unfiled_cluster" }), () => {}, () => {});
  assert.ok(cluster.textContent?.includes("Conversations that look like one topic"));
  const failure = suggestionItem(doc, suggestion({ kind: "repeated_failure" }), () => {}, () => {});
  assert.ok(failure.textContent?.includes("A command that keeps failing"));
});

test("the accept label is chosen by kind", () => {
  const doc = fixture();
  const cluster = suggestionItem(doc, suggestion({ kind: "unfiled_cluster" }), () => {}, () => {});
  const clusterAccept = cluster.querySelector(".dream-accept");
  assert.equal(clusterAccept?.textContent, "File them");
  const recurring = suggestionItem(doc, suggestion(), () => {}, () => {});
  const recurringAccept = recurring.querySelector(".dream-accept");
  assert.equal(recurringAccept?.textContent, "Save a flavor");
});

/* ---------------------------------------------------------------- */
/* evidence opens the cited conversation                            */
/* ---------------------------------------------------------------- */

test("each evidence button opens its conversation id", () => {
  const doc = fixture();
  const opened: string[] = [];
  const item = suggestionItem(doc, suggestion(), (id) => opened.push(id), () => {});
  const buttons = item.querySelectorAll<HTMLButtonElement>(".dream-open");
  assert.equal(buttons.length, 2);
  buttons[0].dispatchEvent(new (doc.defaultView as unknown as { Event: typeof Event }).Event("click"));
  buttons[1].dispatchEvent(new (doc.defaultView as unknown as { Event: typeof Event }).Event("click"));
  assert.deepEqual(opened, ["conv-a", "conv-b"]);
});

test("a suggestion with no evidence draws no evidence row", () => {
  const doc = fixture();
  const item = suggestionItem(doc, suggestion({ evidence_ids: [] }), () => {}, () => {});
  assert.equal(item.querySelector(".dream-evidence"), null);
});
