/**
 * Tests for the review panel's display rules.
 *
 * Two things matter here and neither is cosmetic.
 *
 * A reviewer that ran must appear. A panel that quietly drops one is a
 * panel that reports a smaller, more agreeable set of views than the one
 * that actually ran, and the reader has no way to tell.
 *
 * And nothing a model wrote may reach the page as markup. A reviewer is
 * quoting material it was handed, so its `claim` is the shortest path
 * from an arbitrary document to the DOM.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import type { PanelDoneEvent, PanelFinding } from "../src/api.ts";
import { completenessLine, PanelView } from "../src/panel-view.ts";

function page(): { view: PanelView; transcript: HTMLElement; doc: Document } {
  const dom = new JSDOM("<!doctype html><div id='transcript'></div>");
  const doc = dom.window.document;
  const transcript = doc.getElementById("transcript") as HTMLElement;
  return { view: new PanelView(doc, transcript), transcript, doc };
}

function finding(severity: PanelFinding["severity"], claim: string): PanelFinding {
  return { severity, claim, locus: "section 3" };
}

function doneEvent(over: Partial<PanelDoneEvent> = {}): PanelDoneEvent {
  return {
    seq: 9,
    type: "panel_done",
    target: "draft.md",
    lenses_requested: 2,
    verdicts: 2,
    complete: true,
    agreements: [],
    ...over,
  };
}

test("the first reviewer opens the block", () => {
  const { view, transcript } = page();
  assert.equal(transcript.querySelectorAll(".card-panel").length, 0);
  view.start("evidence");
  assert.equal(transcript.querySelectorAll(".card-panel").length, 1);
  assert.equal(transcript.querySelectorAll(".panel-reviewer").length, 1);
});

test("each reviewer gets its own row with its own state", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.start("method");
  view.finish("evidence", [finding("concern", "0.91 is not in the record")]);

  const rows = transcript.querySelectorAll(".panel-reviewer");
  assert.equal(rows.length, 2);
  assert.equal((rows[0] as HTMLElement).dataset.state, "finished");
  assert.equal((rows[1] as HTMLElement).dataset.state, "running");
});

test("findings are shown worst first", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", [
    finding("note", "minor"),
    finding("blocking", "unusable"),
    finding("concern", "middling"),
  ]);
  const severities = [...transcript.querySelectorAll(".panel-finding")].map(
    (n) => (n as HTMLElement).dataset.severity,
  );
  assert.deepEqual(severities, ["blocking", "concern", "note"]);
});

test("a reviewer that found nothing says so rather than showing a blank row", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", []);
  const none = transcript.querySelector(".panel-finding-none");
  assert.ok(none, "an empty verdict must be stated, not left blank");
  assert.match(none!.textContent ?? "", /No findings/);
});

test("a reviewer that failed is drawn, not dropped", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.start("method");
  view.finish("evidence", []);
  view.fail("method", "no fenced JSON block found in the reviewer's answer");

  const rows = transcript.querySelectorAll(".panel-reviewer");
  assert.equal(rows.length, 2);
  assert.equal((rows[1] as HTMLElement).dataset.state, "failed");
  assert.match(rows[1].textContent ?? "", /no fenced JSON block/);
});

/**
 * The `started` frame can be lost to a reconnect while a verdict still
 * arrives. A reviewer that ran and is not on the page is the one failure
 * this view must not have.
 */
test("a verdict for a reviewer that was never seen to start still appears", () => {
  const { view, transcript } = page();
  view.finish("alternatives", [finding("blocking", "unsupported")]);
  const rows = transcript.querySelectorAll(".panel-reviewer");
  assert.equal(rows.length, 1);
  assert.match(rows[0].textContent ?? "", /alternatives/);
  assert.equal((rows[0] as HTMLElement).dataset.state, "finished");
});

test("a complete panel says so", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", []);
  view.done(doneEvent({ lenses_requested: 1, verdicts: 1 }));

  const summary = transcript.querySelector(".panel-summary") as HTMLElement;
  assert.equal(summary.dataset.complete, "true");
  assert.match(summary.textContent ?? "", /All 1 reviewers reported/);
});

/**
 * The number a reader needs before any other one. Two of two agreeing is
 * a weaker claim than two of five, and a corroboration count cannot say
 * which it is.
 */
test("an incomplete panel says how many views its agreements really cover", () => {
  const line = completenessLine(
    doneEvent({ complete: false, lenses_requested: 5, verdicts: 3 }),
  );
  assert.match(line, /3 of 5/);
  assert.match(line, /2 reviewers did not/);
  assert.match(line, /covering 3 views and not 5/);
});

test("one missing reviewer is described in the singular", () => {
  const line = completenessLine(
    doneEvent({ complete: false, lenses_requested: 2, verdicts: 1 }),
  );
  assert.match(line, /1 reviewer did not/);
});

/**
 * A stopped panel has as many verdicts as it was asked for only by
 * coincidence, but `complete` is still false. Saying "all reported"
 * there would be the one wrong thing to say.
 */
test("a panel that did not finish never claims everyone reported", () => {
  const line = completenessLine(
    doneEvent({ complete: false, lenses_requested: 2, verdicts: 2 }),
  );
  assert.doesNotMatch(line, /All 2 reviewers reported/);
  assert.match(line, /did not finish/);
});

test("agreements list the lenses that raised them", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", [finding("concern", "x")]);
  view.done(
    doneEvent({
      lenses_requested: 1,
      verdicts: 1,
      agreements: [
        { locus: "section 3", lenses: ["evidence", "method"], highest: "blocking" },
      ],
    }),
  );
  const item = transcript.querySelector(".panel-agreement") as HTMLElement;
  assert.equal(item.dataset.severity, "blocking");
  assert.match(item.textContent ?? "", /section 3/);
  assert.match(item.textContent ?? "", /2 reviewers/);
  assert.match(item.textContent ?? "", /evidence, method/);
});

test("a panel with no agreements shows no agreement list", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", []);
  view.done(doneEvent({ lenses_requested: 1, verdicts: 1 }));
  assert.equal(transcript.querySelectorAll(".panel-agreements").length, 0);
});

test("a second panel starts a new block rather than growing the first", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.finish("evidence", []);
  view.done(doneEvent({ lenses_requested: 1, verdicts: 1 }));
  view.start("method");
  assert.equal(transcript.querySelectorAll(".card-panel").length, 2);
});

test("close leaves the drawn block in place and starts the next one fresh", () => {
  const { view, transcript } = page();
  view.start("evidence");
  view.close();
  view.start("method");
  assert.equal(transcript.querySelectorAll(".card-panel").length, 2);
  // The stopped panel keeps its reviewer in whatever state it reached.
  const first = transcript.querySelector(".card-panel") as HTMLElement;
  assert.equal(
    (first.querySelector(".panel-reviewer") as HTMLElement).dataset.state,
    "running",
  );
});

// ---- injection ----
//
// A reviewer is quoting material it was handed, so its claim is the
// shortest path from an arbitrary document to the page.

const HOSTILE = [
  "<img src=x onerror=alert(1)>",
  "<script>alert(1)</script>",
  "</li></ul><script>alert(1)</script>",
  "<svg/onload=alert(1)>",
  '"><iframe src="javascript:alert(1)">',
];

for (const payload of HOSTILE) {
  test(`a claim containing ${payload.slice(0, 24)} is text, not markup`, () => {
    const { view, transcript } = page();
    view.start("evidence");
    view.finish("evidence", [finding("blocking", payload)]);

    assert.equal(transcript.querySelectorAll("script").length, 0);
    assert.equal(transcript.querySelectorAll("img").length, 0);
    assert.equal(transcript.querySelectorAll("iframe").length, 0);
    assert.equal(transcript.querySelectorAll("svg").length, 0);
    const claim = transcript.querySelector(".panel-finding-claim") as HTMLElement;
    assert.equal(claim.textContent, payload);
  });

  test(`a lens name containing ${payload.slice(0, 24)} is text, not markup`, () => {
    const { view, transcript } = page();
    view.start(payload);
    assert.equal(transcript.querySelectorAll("script").length, 0);
    assert.equal(transcript.querySelectorAll("img").length, 0);
    const name = transcript.querySelector(".panel-reviewer-name") as HTMLElement;
    assert.equal(name.textContent, payload);
  });

  test(`a failure reason containing ${payload.slice(0, 24)} is text, not markup`, () => {
    const { view, transcript } = page();
    view.fail("evidence", payload);
    assert.equal(transcript.querySelectorAll("script").length, 0);
    assert.equal(transcript.querySelectorAll("img").length, 0);
  });

  test(`an agreement locus containing ${payload.slice(0, 24)} is text, not markup`, () => {
    const { view, transcript } = page();
    view.done(
      doneEvent({
        agreements: [{ locus: payload, lenses: ["a", "b"], highest: "note" }],
      }),
    );
    assert.equal(transcript.querySelectorAll("script").length, 0);
    assert.equal(transcript.querySelectorAll("img").length, 0);
    const locus = transcript.querySelector(".panel-agreement-locus") as HTMLElement;
    assert.equal(locus.textContent, payload);
  });

  test(`a target name containing ${payload.slice(0, 24)} is text, not markup`, () => {
    const { view, transcript } = page();
    view.done(doneEvent({ target: payload }));
    assert.equal(transcript.querySelectorAll("script").length, 0);
    assert.equal(transcript.querySelectorAll("img").length, 0);
  });
}
