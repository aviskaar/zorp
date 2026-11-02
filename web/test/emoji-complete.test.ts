import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  EmojiComplete,
  applyEmojiPick,
  emojiQuery,
  matchEmoji,
} from "../src/emoji-complete.ts";

test("emojiQuery opens on a colon token of two or more characters", () => {
  // "hi :ro" with the caret at the end.
  assert.deepEqual(emojiQuery("hi :ro", 6), { start: 3, query: "ro" });
  assert.deepEqual(emojiQuery(":rock", 5), { start: 0, query: "rock" });
  // One character is too few.
  assert.equal(emojiQuery(":r", 2), null);
  // A bare colon.
  assert.equal(emojiQuery("hi :", 4), null);
});

test("emojiQuery does not open mid-word", () => {
  // A time and a URL both have a colon inside a word.
  assert.equal(emojiQuery("at 12:34", 8), null);
  assert.equal(emojiQuery("http://x", 8), null);
  // After whitespace it opens.
  assert.deepEqual(emojiQuery("see :sm", 7), { start: 4, query: "sm" });
});

test("emojiQuery opens for an uppercase query, since matching is case-insensitive", () => {
  assert.deepEqual(emojiQuery(":Rock", 5), { start: 0, query: "Rock" });
  // And that query matches, lowercased.
  assert.ok(matchEmoji("Rock").some((e) => e.name === "rocket"));
});

test("matchEmoji returns prefix matches first, capped at eight", () => {
  const m = matchEmoji("rock");
  assert.ok(m.length > 0 && m.length <= 8);
  assert.ok(m.some((e) => e.name === "rocket" && e.char === "🚀"));
  // A standard match inserts the character.
  const rocket = m.find((e) => e.name === "rocket")!;
  assert.equal(rocket.insert, "🚀");
});

test("matchEmoji includes custom names, which insert :name:", () => {
  const m = matchEmoji("party", ["partyparrot", "partyblob"]);
  const parrot = m.find((e) => e.name === "partyparrot");
  assert.ok(parrot, "custom name not offered");
  assert.equal(parrot!.char, null);
  assert.equal(parrot!.insert, ":partyparrot:");
});

test("applyEmojiPick replaces the whole :token: with the insert", () => {
  // Caret after "ro" in "ship :ro", pick rocket's character.
  const r = applyEmojiPick("ship :ro", 5, 8, "🚀");
  assert.equal(r.text, "ship 🚀");
  assert.equal(r.caret, "ship 🚀".length);
  // A closing colon already typed is absorbed.
  const r2 = applyEmojiPick("ship :rocket: go", 5, 12, "🚀");
  assert.equal(r2.text, "ship 🚀 go");
});

test("EmojiComplete draws rows through textContent and picks", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const widget = new EmojiComplete(dom.window.document);
  let picked: string | null = null;
  const matches = matchEmoji("rock");
  widget.show(matches, (m) => {
    picked = m.insert;
  });
  assert.equal(widget.open, true);
  const rows = widget.element().querySelectorAll(".slash-menu-item");
  assert.ok(rows.length > 0);
  // Names are shown as :name: and no HTML is parsed.
  assert.match(rows[0].querySelector(".slash-menu-name")!.textContent ?? "", /^:[a-z0-9_+-]+:$/);
  widget.pick();
  assert.equal(widget.open, false);
  assert.ok(picked && picked.length > 0);
});
