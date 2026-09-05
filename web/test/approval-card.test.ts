/**
 * Tests for the approval card.
 *
 * The tool name and its arguments are the model's request, and the card is
 * the place a person reads most carefully, so the injection cases come
 * first. The rest pins the two shapes of the card: open and not closable
 * while it waits, folded to its head line once settled.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { APPROVAL_NOTES, APPROVAL_TITLES, approvalCard, prettyArguments } from "../src/approval-card.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document as unknown as Document;

function card(tool = "run_command", args = '{"command":"ls"}') {
  const built = approvalCard(doc, tool, args, doc.createElement("svg"));
  doc.body.append(built.root);
  return built;
}

function click(target: HTMLElement): MouseEvent {
  const event = new dom.window.MouseEvent("click", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

test("a tag in the tool name or the arguments is text, not an element", () => {
  const built = card("<img src=x onerror=alert(1)>", '{"command":"</details><script>alert(1)</script>"}');
  assert.equal(built.root.querySelectorAll("img, script").length, 0);
  assert.equal(built.root.querySelector(".tool-name")?.textContent, "<img src=x onerror=alert(1)>");
  assert.ok(built.root.querySelector(".card-args code")?.textContent?.includes("<script>alert(1)</script>"));
});

test("arguments that do not parse are shown verbatim", () => {
  assert.equal(prettyArguments("not json <b>"), "not json <b>");
  assert.equal(prettyArguments("  "), "(no arguments)");
  assert.equal(prettyArguments('{"a":1}'), '{\n  "a": 1\n}');
});

test("a waiting card is an open details whose head is the summary and refuses to close", () => {
  const built = card();
  assert.equal(built.root.tagName, "DETAILS");
  assert.equal(built.root.open, true);
  const head = built.root.firstElementChild as HTMLElement;
  assert.equal(head.tagName, "SUMMARY");
  assert.equal(head.querySelector(".card-title")?.textContent, "Approval required");
  assert.equal(head.querySelector(".tool-name")?.textContent, "run_command");
  assert.equal(click(head).defaultPrevented, true);
  assert.equal(built.root.open, true);
});

test("the three buttons are live and go together", () => {
  const built = card();
  assert.deepEqual(
    [built.allow, built.deny, built.allowAll].map((button) => [button.textContent, button.disabled]),
    [
      ["Allow", false],
      ["Deny", false],
      ["Allow all for this chat", false],
    ],
  );
  built.enable(false);
  assert.ok([built.allow, built.deny, built.allowAll].every((button) => button.disabled));
  built.note("Sending…");
  assert.equal(built.root.querySelector(".card-note")?.textContent, "Sending…");
});

test("a settled card is closed, names the tool and the outcome in its head, and keeps the arguments", () => {
  const built = card("write_file", '{"path":"a.txt"}');
  built.settle("allowed");
  assert.equal(built.root.open, false);
  assert.ok(built.root.classList.contains("is-allowed"));
  assert.ok(built.root.classList.contains("is-settled"));
  const head = built.root.querySelector("summary") as HTMLElement;
  assert.equal(head.querySelector(".card-title")?.textContent, APPROVAL_TITLES.allowed);
  assert.equal(head.querySelector(".card-tag")?.textContent, "allowed");
  assert.equal(head.querySelector(".tool-name")?.textContent, "write_file");
  assert.equal(built.root.querySelector(".card-actions"), null);
  assert.equal(built.root.querySelector(".card-args code")?.textContent, '{\n  "path": "a.txt"\n}');
  assert.equal(built.root.querySelector(".card-note")?.textContent, APPROVAL_NOTES.allowed);
  // Settled, the head is the reader's to toggle.
  assert.equal(click(head).defaultPrevented, false);
});

test("every outcome has its own title and note", () => {
  for (const outcome of ["denied", "expired", "stopped"] as const) {
    const built = card();
    built.settle(outcome);
    assert.equal(built.root.querySelector(".card-title")?.textContent, APPROVAL_TITLES[outcome]);
    assert.equal(built.root.querySelector(".card-note")?.textContent, APPROVAL_NOTES[outcome]);
    assert.equal(built.root.querySelector(".card-tag")?.textContent, outcome);
    assert.equal(built.root.open, false);
  }
});
