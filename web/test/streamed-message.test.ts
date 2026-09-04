import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { StreamedMessage, endsStreamedMessage } from "../src/streamed-message.ts";
import { renderMarkdown } from "../src/markdown.ts";

/** Render immediately instead of on an animation frame, so tests can assert. */
const now = (fn: () => void): number => {
  fn();
  return 0;
};
const noop = (): void => {};

// The markdown renderer calls document.createElement, so the global has to
// exist before it runs. Same arrangement as markdown.test.ts, for the same
// reason: it lets the real source run unmodified.
const shared = new JSDOM("<!doctype html><body></body>");
(globalThis as Record<string, unknown>).document = shared.window.document;

function fixture() {
  const doc = shared.window.document;
  const transcript = doc.createElement("div");
  doc.body.append(transcript);
  const streamed = new StreamedMessage(
    transcript,
    (body, text) => renderMarkdown(body, text),
    now,
    noop,
  );
  return { doc, transcript, streamed };
}

test("the first fragment opens exactly one message", () => {
  const { transcript, streamed } = fixture();
  streamed.append("he");
  assert.equal(transcript.querySelectorAll("article").length, 1);
  assert.equal(transcript.querySelector(".msg-role")?.textContent, "zorp");
});

test("more fragments extend that message rather than starting new ones", () => {
  const { transcript, streamed } = fixture();
  streamed.append("he");
  streamed.append("ll");
  streamed.append("o");
  assert.equal(
    transcript.querySelectorAll("article").length,
    1,
    "each token became its own message",
  );
  assert.equal(transcript.querySelector(".msg-body")?.textContent, "hello");
});

test("the server's finished text replaces what streamed", () => {
  const { transcript, streamed } = fixture();
  streamed.append("partial ans");
  streamed.finish("partial answer, completed");
  assert.equal(
    transcript.querySelector(".msg-body")?.textContent,
    "partial answer, completed",
  );
});

// A dropped frame must not become a silently truncated answer that looks whole.
test("a finished message is no longer marked as streaming", () => {
  const { transcript, streamed } = fixture();
  streamed.append("x");
  assert.ok(transcript.querySelector(".is-streaming"), "not marked while streaming");
  streamed.finish("x");
  assert.equal(transcript.querySelector(".is-streaming"), null);
});

test("without a finished text, what streamed is kept rather than discarded", () => {
  const { transcript, streamed } = fixture();
  streamed.append("all there is");
  streamed.finish(null);
  assert.equal(transcript.querySelector(".msg-body")?.textContent, "all there is");
});

test("a turn that streamed nothing leaves no empty bubble behind", () => {
  const { transcript, streamed } = fixture();
  streamed.append("   ");
  streamed.finish(null);
  assert.equal(transcript.querySelectorAll("article").length, 0);
});

test("finishing when nothing is open reports that the caller must append", () => {
  const { transcript, streamed } = fixture();
  assert.equal(streamed.finish("the answer"), false);
  assert.equal(transcript.querySelectorAll("article").length, 0);
});

test("finishing an open message reports that it handled the text", () => {
  const { streamed } = fixture();
  streamed.append("x");
  assert.equal(streamed.finish("x"), true);
});

test("a second message can stream after the first is finished", () => {
  const { transcript, streamed } = fixture();
  streamed.append("first");
  streamed.finish(null);
  streamed.append("second");
  streamed.finish(null);
  const bodies = [...transcript.querySelectorAll(".msg-body")].map((n) => n.textContent);
  assert.deepEqual(bodies, ["first", "second"]);
});

// The renderer is the only thing standing between model output and the page.
// Streaming must not become a second path onto it.
test("markup arriving mid-stream is still text, not markup", () => {
  const { transcript, streamed } = fixture();
  streamed.append("<img src=x onerror=");
  streamed.append("alert(1)><script>alert(2)</script>");
  streamed.finish(null);
  assert.equal(transcript.querySelectorAll("img").length, 0);
  assert.equal(transcript.querySelectorAll("script").length, 0);
  assert.ok(transcript.textContent?.includes("onerror"), "the text was dropped entirely");
});

test("a javascript: link arriving in fragments never becomes an anchor", () => {
  const { transcript, streamed } = fixture();
  streamed.append("[click](javas");
  streamed.append("cript:alert(1))");
  streamed.finish(null);
  assert.equal(transcript.querySelectorAll("a").length, 0);
});

// Re-rendering per token is what makes a fast local model unusable.
test("many fragments coalesce into few renders", () => {
  const transcript = shared.window.document.createElement("div");
  let renders = 0;
  let pending: (() => void) | null = null;
  const streamed = new StreamedMessage(
    transcript,
    (body, text) => {
      renders += 1;
      body.textContent = text;
    },
    (fn) => {
      pending = fn;
      return 1;
    },
    () => {
      pending = null;
    },
  );
  for (let i = 0; i < 50; i += 1) streamed.append("x");
  assert.equal(renders, 0, "rendered before the frame ran");
  pending!();
  assert.equal(renders, 1, "50 fragments caused more than one render");
});

/*
 * Turn shape. A real turn is
 *   working, deltas, working_done, tool, working, deltas, working_done,
 *   assistant, done
 * and both boundaries in it were got wrong before these tests existed.
 */

// The duplicate-answer bug. working_done fires when the model call returns,
// which is before the finished answer is sent.
test("working_done does not end the message, or the answer arrives twice", () => {
  assert.equal(endsStreamedMessage("working_done"), false);
  assert.equal(endsStreamedMessage("working"), false);
});

// The merged-message bug. Text before a tool call and text after it are two
// separate things the model said.
test("a tool call ends the message, or two turns of text merge into one", () => {
  assert.equal(endsStreamedMessage("tool"), true);
});

test("the finished answer does not end the message, it completes it", () => {
  assert.equal(endsStreamedMessage("assistant"), false);
  assert.equal(endsStreamedMessage("assistant_delta"), false);
});

// A session title renames a row in the sidebar and the heading above the
// transcript, and puts nothing in the transcript itself. It usually lands
// after `done`, when nothing is streaming, but the previous turn's title can
// arrive in the middle of this one and must not cut the answer in two.
test("a session title does not end the message, it renames the conversation", () => {
  assert.equal(endsStreamedMessage("session_title"), false);
});

test("visible activity and turn endings all close the message", () => {
  for (const type of ["verify", "notice", "approval_request", "error", "done"] as const) {
    assert.equal(endsStreamedMessage(type), true, `${type} left the message open`);
  }
});

// A stop lands in the middle of an answer more often than not, so whatever
// was streamed has to be closed off and kept before the stopped card goes in
// underneath it.
test("a stop ends the message, so the fragments so far stay above the notice", () => {
  assert.equal(endsStreamedMessage("stopped"), true);
});

// End to end over a real single-step turn: the answer must appear once.
test("a single-step turn renders the answer exactly once", () => {
  const { transcript, streamed } = fixture();
  const events: Array<{ type: string; text?: string }> = [
    { type: "working" },
    { type: "assistant_delta", text: "Hel" },
    { type: "assistant_delta", text: "lo" },
    { type: "working_done" },
    { type: "assistant", text: "Hello" },
    { type: "done" },
  ];
  let appended = 0;
  for (const event of events) {
    if (endsStreamedMessage(event.type as never)) streamed.finish(null);
    if (event.type === "assistant_delta") streamed.append(event.text!);
    if (event.type === "assistant" && !streamed.finish(event.text!)) appended += 1;
  }
  assert.equal(appended, 0, "the answer was appended as a second message");
  assert.equal(transcript.querySelectorAll("article").length, 1);
  assert.equal(transcript.querySelector(".msg-body")?.textContent, "Hello");
});

// End to end over a real multi-step turn: two separate messages, in order.
test("a turn with a tool call renders two messages, not one", () => {
  const { transcript, streamed } = fixture();
  const events: Array<{ type: string; text?: string }> = [
    { type: "working" },
    { type: "assistant_delta", text: "Let me look." },
    { type: "working_done" },
    { type: "tool" },
    { type: "working" },
    { type: "assistant_delta", text: "Found it." },
    { type: "working_done" },
    { type: "assistant", text: "Found it." },
    { type: "done" },
  ];
  for (const event of events) {
    if (endsStreamedMessage(event.type as never)) streamed.finish(null);
    if (event.type === "assistant_delta") streamed.append(event.text!);
    if (event.type === "assistant") streamed.finish(event.text!);
  }
  const bodies = [...transcript.querySelectorAll(".msg-body")].map((n) => n.textContent);
  assert.deepEqual(bodies, ["Let me look.", "Found it."]);
});

/*
 * The default scheduler.
 *
 * Every test above injects its own, which is why a defaulted
 * `requestAnimationFrame` reference could be broken for a full browser
 * session while the suite stayed green. Browsers brand-check the receiver of
 * a window method, so this stub does too.
 */
test("the default scheduler works when it is a real window method", async () => {
  const win = globalThis as Record<string, unknown>;
  const pending: Array<() => void> = [];
  // Browser semantics: an unqualified call from a module passes `undefined`
  // as the receiver and is allowed; a foreign receiver, such as the object
  // that stored the function as a field, is not.
  const legal = (receiver: unknown): boolean =>
    receiver === undefined || receiver === globalThis;
  win.requestAnimationFrame = function (this: unknown, fn: () => void): number {
    if (!legal(this)) throw new TypeError("Illegal invocation");
    pending.push(fn);
    return pending.length;
  };
  win.cancelAnimationFrame = function (this: unknown): void {
    if (!legal(this)) throw new TypeError("Illegal invocation");
  };

  const transcript = shared.window.document.createElement("div");
  // No scheduler passed: the arrangement main.ts uses.
  const streamed = new StreamedMessage(transcript, (body, text) => {
    body.textContent = text;
  });

  streamed.append("hello");
  assert.equal(pending.length, 1, "nothing was scheduled");
  pending[0]();
  assert.equal(
    transcript.querySelector(".msg-body")?.textContent,
    "hello",
    "the fragment never reached the page",
  );
});

/**
 * A finished message gets handed to the caller so it can decorate the row,
 * which is how the copy button reaches a streamed answer. The hook fires with
 * the text that was actually left on the page, so a copy button built from it
 * offers the server's finished answer rather than the fragments it replaced.
 */
test("finishing a message offers the finished row and text", () => {
  const doc = shared.window.document;
  const transcript = doc.createElement("div");
  const seen: Array<{ text: string; hasBody: boolean }> = [];
  const streamed = new StreamedMessage(
    transcript,
    (body, text) => renderMarkdown(body, text),
    now,
    noop,
    (row, text) => seen.push({ text, hasBody: !!row.querySelector(".msg-body") }),
  );

  streamed.append("partial ans");
  streamed.finish("the whole answer");

  assert.deepEqual(seen, [{ text: "the whole answer", hasBody: true }]);
});

/** An empty turn removes its row, so there is nothing to decorate. */
test("a message with nothing in it is not offered", () => {
  const doc = shared.window.document;
  const transcript = doc.createElement("div");
  const seen: string[] = [];
  const streamed = new StreamedMessage(
    transcript,
    (body, text) => renderMarkdown(body, text),
    now,
    noop,
    (_row, text) => seen.push(text),
  );

  streamed.append("   ");
  streamed.finish("");

  assert.deepEqual(seen, [], "an empty message was decorated anyway");
});

/*
 * A withdrawal. The provider dropped the answer after fragments had reached
 * the page and the agent is asking again, so the fragments come down, a line
 * says why, and the fresh answer streams in under it. The line is the second
 * string this module puts on the page, so it gets the same hostile input the
 * fragments do.
 */
test("a withdrawal takes the fragments down, says why as text, and keeps the row for the re-ask", () => {
  const { transcript, streamed } = fixture();
  streamed.append("the start of a dead ans");
  const hostile = "dropped <img src=x onerror=alert(1)><script>alert(2)</script>; asking again (1 of 2)";
  assert.equal(streamed.withdraw(hostile), true);

  assert.equal(transcript.querySelectorAll("article").length, 1, "the row was closed or duplicated");
  assert.equal(
    transcript.querySelector(".msg-body")?.textContent,
    "",
    "the dead fragments stayed on the page",
  );
  const line = transcript.querySelector(".msg-withdrawn");
  assert.ok(line, "no status line where the fragments were");
  assert.equal(line?.textContent, hostile);
  assert.equal(transcript.querySelectorAll("img").length, 0);
  assert.equal(transcript.querySelectorAll("script").length, 0);
  assert.equal(endsStreamedMessage("assistant_withdrawn"), false);

  streamed.append("the fresh answer");
  streamed.finish("the fresh answer");
  assert.equal(transcript.querySelectorAll("article").length, 1);
  assert.equal(transcript.querySelector(".msg-body")?.textContent, "the fresh answer");
  assert.ok(transcript.querySelector(".msg-withdrawn"), "the status line was lost when the answer landed");
});

test("withdrawing when nothing is open reports that the caller must say it elsewhere", () => {
  const { transcript, streamed } = fixture();
  assert.equal(streamed.withdraw("status"), false);
  assert.equal(transcript.querySelectorAll("article").length, 0);
});
