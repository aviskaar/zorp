/**
 * Tests for the RL Studio per-answer feedback controls.
 *
 * The one thing that matters: the recorded signal is the person's click, and
 * the answer is recorded lazily so an answer nobody rates writes nothing. No
 * model is consulted here.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { feedbackControls, type FeedbackApi } from "../src/rl-feedback.ts";

function fakeApi(): {
  api: FeedbackApi;
  records: unknown[];
  feedback: { id: string; body: unknown }[];
} {
  const records: unknown[] = [];
  const feedback: { id: string; body: unknown }[] = [];
  let next = 0;
  const api: FeedbackApi = {
    async record(entry) {
      records.push(entry);
      next += 1;
      return `id-${next}`;
    },
    async setFeedback(id, body) {
      feedback.push({ id, body });
    },
  };
  return { api, records, feedback };
}

const ctx = { sessionId: "s1", model: "m", prompt: "q", answer: "a" };

test("nothing is recorded until the person clicks", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const { api, records } = fakeApi();
  feedbackControls(dom.window.document, ctx, api);
  assert.equal(records.length, 0, "an unrated answer writes nothing");
});

test("a thumbs-up records the answer once and sets the feedback", async () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const { api, records, feedback } = fakeApi();
  const row = feedbackControls(dom.window.document, ctx, api);
  const up = row.querySelector(".feedback-up") as HTMLButtonElement;
  up.dispatchEvent(new dom.window.Event("click"));
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(records.length, 1, "recorded once");
  assert.deepEqual(feedback, [{ id: "id-1", body: { feedback: "up" } }]);
  assert.equal(up.getAttribute("aria-pressed"), "true");
});

test("a second action on the same answer reuses the one recorded id", async () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const { api, records, feedback } = fakeApi();
  const row = feedbackControls(dom.window.document, ctx, api);
  (row.querySelector(".feedback-up") as HTMLButtonElement).dispatchEvent(
    new dom.window.Event("click"),
  );
  await new Promise((r) => setTimeout(r, 0));
  (row.querySelector(".feedback-down") as HTMLButtonElement).dispatchEvent(
    new dom.window.Event("click"),
  );
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(records.length, 1, "the answer is recorded only once");
  assert.equal(feedback.length, 2);
  assert.equal(feedback[1].id, "id-1", "the same entry is updated");
});

test("a better answer is recorded as an edit; a cancelled one is not", async () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const { api, records, feedback } = fakeApi();
  // Cancelled edit (returns null): nothing recorded.
  const cancelled = feedbackControls(dom.window.document, ctx, api, () => null);
  (cancelled.querySelector(".feedback-edit") as HTMLButtonElement).dispatchEvent(
    new dom.window.Event("click"),
  );
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(records.length, 0, "a cancelled edit records nothing");

  // A real edit records the answer and sets edited_answer.
  const row = feedbackControls(dom.window.document, ctx, api, () => "the better answer");
  (row.querySelector(".feedback-edit") as HTMLButtonElement).dispatchEvent(
    new dom.window.Event("click"),
  );
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(records.length, 1);
  assert.deepEqual(feedback.at(-1), {
    id: "id-1",
    body: { edited_answer: "the better answer" },
  });
});
