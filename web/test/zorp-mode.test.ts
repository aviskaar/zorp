/**
 * Tests for Zorp mode's display rules.
 *
 * Three things matter here and none of them is cosmetic.
 *
 * An empty ledger and a missing run record must not look the same. An
 * empty ledger is the honest state for a record nobody has fed, and
 * saying "nothing recorded" for a server that has no run record at all
 * would report the wrong fact.
 *
 * A verdict must say what it is. "Approved" and "the track was killed"
 * are the two outcomes an attempt can have, and a page that renders them
 * the same way hides the one that matters.
 *
 * And nothing reaches the page as markup. A condition's value comes off
 * a row a run wrote, a track id comes off a question a person typed, and
 * both land in the DOM through `textContent` and nothing else.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import type { InvestigateDoneEvent, Ledger } from "../src/api.ts";
import { forecastLine, verdictLine, ZorpModeView } from "../src/zorp-mode.ts";

function page(): { view: ZorpModeView; transcript: HTMLElement; doc: Document } {
  const dom = new JSDOM("<!doctype html><div id='transcript'></div>");
  const doc = dom.window.document;
  const transcript = doc.getElementById("transcript") as HTMLElement;
  return { view: new ZorpModeView(doc, transcript), transcript, doc };
}

function done(over: Partial<InvestigateDoneEvent> = {}): InvestigateDoneEvent {
  return {
    seq: 9,
    type: "investigate_done",
    track_id: "2026-08-21-does-caching-help",
    approved: true,
    ...over,
  };
}

function ledger(over: Partial<Ledger> = {}): Ledger {
  return {
    track_id: "2026-08-21-does-caching-help",
    present: true,
    forecasting: false,
    experiments: [],
    ...over,
  };
}

test("the closing frame opens a block naming the track", () => {
  const { view, transcript } = page();
  assert.equal(transcript.querySelectorAll(".card-zorp").length, 0);
  view.done(done());
  const block = transcript.querySelector(".card-zorp");
  assert.ok(block);
  assert.match(block.textContent ?? "", /2026-08-21-does-caching-help/);
});

test("an approved attempt and a killed track do not read the same", () => {
  const approved = verdictLine(done({ approved: true }));
  const killed = verdictLine(done({ approved: false }));
  assert.notEqual(approved, killed);
  assert.match(killed, /killed/);
  // An attempt that never got a verdict must not be drawn as either one.
  const unfinished = verdictLine(done({ approved: undefined }));
  assert.notEqual(unfinished, approved);
  assert.notEqual(unfinished, killed);
});

test("a missing run record and an empty ledger say different things", () => {
  const missing = page();
  missing.view.showLedger(ledger({ present: false }));
  const missingText = missing.transcript.textContent ?? "";

  const empty = page();
  empty.view.showLedger(ledger({ present: true }));
  const emptyText = empty.transcript.textContent ?? "";

  assert.notEqual(missingText, emptyText);
  assert.match(missingText, /no run record/i);
});

test("the ledger shows the conditions an attempt ran under", () => {
  const { view, transcript } = page();
  view.showLedger(
    ledger({
      experiments: [
        {
          id: "exp-1",
          status: "completed",
          conditions: [
            { key: "model", value: "qwen3:8b" },
            { key: "checkpoint_mode", value: "auto-approve" },
          ],
          expectations: [],
          metrics: [{ key: "latency_ms", value: "42" }],
        },
      ],
    }),
  );
  const rows = transcript.querySelectorAll(".zorp-condition");
  assert.equal(rows.length, 2);
  const text = transcript.textContent ?? "";
  assert.match(text, /checkpoint_mode/);
  assert.match(text, /auto-approve/);
  assert.match(text, /latency_ms/);
  assert.match(text, /42/);
});

test("an attempt with no forecast says so rather than showing nothing", () => {
  const { view, transcript } = page();
  view.showLedger(
    ledger({
      forecasting: false,
      experiments: [
        {
          id: "exp-1",
          status: "completed",
          conditions: [],
          expectations: [],
          metrics: [],
        },
      ],
    }),
  );
  assert.match(transcript.textContent ?? "", /no forecast/i);
});

test("a recorded forecast shows its interval and its stated coverage", () => {
  const { view, transcript } = page();
  view.showLedger(
    ledger({
      forecasting: true,
      experiments: [
        {
          id: "exp-1",
          status: "completed",
          conditions: [],
          expectations: [
            {
              metric_key: "latency_ms",
              expected_value: 80,
              interval_low: 60,
              interval_high: 100,
              confidence: 0.8,
            },
          ],
          metrics: [],
        },
      ],
    }),
  );
  const text = transcript.textContent ?? "";
  assert.match(text, /60/);
  assert.match(text, /100/);
  assert.match(text, /80%/);
});

test("forecasting off is said plainly, because it is why the ledger is empty", () => {
  assert.match(forecastLine(false), /off/i);
  assert.match(forecastLine(true), /on/i);
  assert.notEqual(forecastLine(false), forecastLine(true));
});

/**
 * A condition value is text off a recorded row and a track id is
 * derived from a question a person typed. Both reach the page as text
 * and never as markup. This module builds DOM nodes; the moment it
 * assembles an HTML string, a run that recorded a hostile condition
 * value owns the page.
 */
test("nothing rendered becomes markup", () => {
  const { view, transcript } = page();
  const hostile = "<img src=x onerror=alert(1)>";
  view.done(done({ track_id: hostile }));
  view.showLedger(
    ledger({
      track_id: hostile,
      experiments: [
        {
          id: hostile,
          status: hostile,
          conditions: [{ key: hostile, value: hostile }],
          expectations: [],
          metrics: [{ key: hostile, value: hostile }],
        },
      ],
    }),
  );
  assert.equal(transcript.querySelectorAll("img").length, 0);
  assert.ok((transcript.textContent ?? "").includes(hostile));
});

test("closing the view lets the next attempt start a fresh block", () => {
  const { view, transcript } = page();
  view.done(done());
  view.close();
  view.done(done());
  assert.equal(transcript.querySelectorAll(".card-zorp").length, 2);
});

test("a ledger shown after the block closed still reaches the page", () => {
  const { view, transcript } = page();
  view.close();
  view.showLedger(ledger({ present: true }));
  assert.equal(transcript.querySelectorAll(".card-zorp").length, 1);
});
