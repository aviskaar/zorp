/**
 * Tests for the collapsed activity group.
 *
 * The phrase on the summary is the model's own words read back off the
 * latest line, so the injection case comes first, as in
 * `activity-line.test.ts`. The rest pins the summary's two readings, working
 * and count, and that appending never toggles the group.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { WORKING_LABEL, activityGroup } from "../src/activity-group.ts";
import { BRIEF_MAX, toolLine } from "../src/activity-line.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document as unknown as Document;

function group() {
  const built = activityGroup(doc);
  doc.body.append(built.root);
  return built;
}

/** A line the way another renderer might draw it, carrying one state class. */
function bareLine(state = ""): HTMLElement {
  const line = doc.createElement("div");
  line.className = `activity-line ${state}`.trim();
  return line;
}

const label = (root: HTMLElement) => root.querySelector(".activity-summary-label")?.textContent;
const phrase = (root: HTMLElement) => root.querySelector(".activity-summary-phrase")?.textContent;

test("a tag in the latest phrase is text on the summary, not an element", () => {
  const built = group();
  built.append(toolLine(doc, "run_command(ls web)", "exited 0", "<img src=x onerror=alert(1)>"));
  assert.equal(built.root.querySelectorAll("img").length, 0);
  assert.equal(phrase(built.root), "<img src=x onerror=alert(1)>");
});

test("a fresh group is a closed details that reads working", () => {
  const built = group();
  built.append(toolLine(doc, "run_command(ls web)", "exited 0"));
  assert.equal(built.root.tagName, "DETAILS");
  assert.equal(built.root.open, false);
  assert.equal(built.root.querySelector("summary")?.className, "activity-summary-line");
  assert.equal(label(built.root), WORKING_LABEL);
  assert.equal(phrase(built.root), "Listing files in web");
});

test("the summary follows the latest line, clamped, and a line without a phrase clears it", () => {
  const built = group();
  built.append(toolLine(doc, "run_command(ls web)", "exited 0"));
  built.append(toolLine(doc, "run_command(cargo test)", "running", "  *Run* the tests\nand more"));
  assert.equal(phrase(built.root), "Run* the tests");
  built.append(toolLine(doc, "run_command(x)", "running", "y".repeat(BRIEF_MAX + 20)));
  assert.equal(phrase(built.root)?.length, BRIEF_MAX);
  built.append(toolLine(doc, "write_file", "ok"));
  assert.equal(phrase(built.root), "write_file");
  built.append(bareLine());
  assert.equal(phrase(built.root), "");
});

test("the lines are kept exactly as built, under the summary", () => {
  const built = group();
  const first = toolLine(doc, "run_command(ls web)", "exited 0");
  const second = toolLine(doc, "read_file", "ok");
  built.append(first);
  built.append(second);
  assert.deepEqual(Array.from(built.root.children), [built.root.firstElementChild, first, second]);
  assert.equal(first.querySelector(".activity-full code")?.textContent, "ls web");
});

test("appending never toggles the group, in either direction", () => {
  const built = group();
  built.append(bareLine());
  assert.equal(built.root.open, false);
  built.root.open = true;
  built.append(bareLine());
  assert.equal(built.root.open, true);
  built.close();
  assert.equal(built.root.open, true);
});

test("closing turns the summary into a count", () => {
  const one = group();
  one.append(bareLine());
  one.close();
  assert.equal(label(one.root), "1 step");
  assert.equal(phrase(one.root), "");

  const three = group();
  three.append(bareLine("activity-ok"));
  three.append(toolLine(doc, "run_command(ls)", "exited 0"));
  three.append(bareLine("activity-ok"));
  three.close();
  assert.equal(label(three.root), "3 steps");
});

test("failed lines are counted and the group takes the failed state", () => {
  const built = group();
  built.append(bareLine("activity-ok"));
  built.append(bareLine("activity-fail"));
  built.append(bareLine("activity-fail"));
  built.close();
  assert.equal(label(built.root), "3 steps, 2 failed");
  assert.ok(built.root.classList.contains("activity-fail"));
  assert.ok(!built.root.classList.contains("activity-ok"));
});

test("a running line makes the group running, and lines with no state count as ok", () => {
  const built = group();
  built.append(bareLine());
  assert.ok(built.root.classList.contains("activity-ok"));
  built.append(bareLine("activity-fail"));
  assert.ok(built.root.classList.contains("activity-fail"));
  built.append(bareLine("activity-running"));
  assert.ok(built.root.classList.contains("activity-running"));
  const states = ["activity-ok", "activity-fail", "activity-running"];
  assert.equal(states.filter((name) => built.root.classList.contains(name)).length, 1);
});
