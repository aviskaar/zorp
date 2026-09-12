/**
 * Tests for the checkpoint card.
 *
 * Three things matter here and none of them is cosmetic.
 *
 * A checkpoint is not an approval. Declining one kills a track, and the
 * card has to say which checkpoint it is showing, because declining
 * before the first attempt and declining after three are different
 * losses.
 *
 * Nobody answering is not somebody saying no. A run that ended before the
 * question was answered wrote no decision and left the track alone, and a
 * card that settled that as "killed" would report a kill that did not
 * happen.
 *
 * And the prompt is model-authored text. It comes out of an attempt's own
 * summary, so it reaches the page through `textContent` and nothing else.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  CHECKPOINT_NOTES,
  CHECKPOINT_TITLES,
  checkpointCard,
  checkpointStake,
} from "../src/checkpoint-card.ts";

function page(): Document {
  return new JSDOM("<!doctype html><body></body>").window.document;
}

test("the pre-registration checkpoint says it kills a track with no evidence yet", () => {
  const stake = checkpointStake("investigate-prereg");
  assert.match(stake, /before the first attempt/);
  assert.match(stake, /any evidence in it/);
});

test("the post-attempt checkpoint says the attempts already run stay in the record", () => {
  const stake = checkpointStake("investigate");
  assert.match(stake, /already run/);
  assert.notEqual(stake, checkpointStake("investigate-prereg"));
});

test("a checkpoint nobody named still says declining kills the track", () => {
  assert.match(checkpointStake("something-new"), /kills the track/);
});

test("the prompt reaches the page as text and never as markup", () => {
  const doc = page();
  const card = checkpointCard(doc, "investigate", "<img src=x onerror=alert(1)> p95 = 231");
  doc.body.append(card.root);
  assert.equal(doc.querySelectorAll("img").length, 0);
  const prompt = card.root.querySelector(".checkpoint-prompt") as HTMLElement;
  assert.match(prompt.textContent ?? "", /onerror=alert\(1\)/);
});

test("the card waits open and folds once it is settled", () => {
  const doc = page();
  const card = checkpointCard(doc, "investigate", "Keep this track alive?");
  assert.equal(card.root.open, true, "a person has to see what they are deciding on");
  card.settle("kept");
  assert.equal(card.root.open, false);
  assert.equal(card.root.dataset.outcome, "kept");
});

test("settling removes the buttons, so a settled card cannot be pressed again", () => {
  const doc = page();
  const card = checkpointCard(doc, "investigate", "Keep this track alive?");
  assert.ok(card.root.querySelector(".checkpoint-buttons"));
  card.settle("killed");
  assert.equal(card.root.querySelector(".checkpoint-buttons"), null);
});

test("unanswered and stopped do not read as a kill", () => {
  for (const outcome of ["abandoned", "stopped"] as const) {
    assert.ok(!/killed/i.test(CHECKPOINT_TITLES[outcome]), outcome);
    assert.match(CHECKPOINT_NOTES[outcome], /no decision was recorded/, outcome);
    assert.match(CHECKPOINT_NOTES[outcome], /untouched/, outcome);
  }
  assert.match(CHECKPOINT_NOTES.killed, /No write-up is produced/);
});

test("both buttons go down together and the note says why", () => {
  const doc = page();
  const card = checkpointCard(doc, "investigate", "Keep this track alive?");
  card.enable(false);
  assert.equal(card.keep.disabled, true);
  assert.equal(card.kill.disabled, true);
  card.note("Sending your decision…");
  assert.equal(
    (card.root.querySelector(".checkpoint-note") as HTMLElement).textContent,
    "Sending your decision…",
  );
});
