import { strict as assert } from "node:assert";
import { test } from "node:test";
import { expandKnown, expandShortcodes, shortcodeChar } from "../src/emoji.ts";

test("shortcodeChar looks up by own property only", () => {
  assert.equal(shortcodeChar("rocket"), "🚀");
  assert.equal(shortcodeChar("+1"), "👍");
  assert.equal(shortcodeChar("notareal_name"), undefined);
  // Not an inherited Object.prototype key mistaken for an emoji.
  assert.equal(shortcodeChar("__proto__"), undefined);
  assert.equal(shortcodeChar("constructor"), undefined);
});

test("expandShortcodes replaces known names and leaves the rest", () => {
  assert.equal(expandShortcodes("ship it :rocket:"), "ship it 🚀");
  assert.equal(expandShortcodes(":+1: :-1:"), "👍 👎");
  assert.equal(expandShortcodes("a :notreal: b"), "a :notreal: b");
  // A custom name has no character, so it stays for the renderer to draw.
  assert.equal(expandShortcodes("my :partyparrot:"), "my :partyparrot:");
});

test("expandShortcodes leaves code spans and fences alone", () => {
  assert.equal(expandShortcodes("use `:rocket:` now"), "use `:rocket:` now");
  assert.equal(expandShortcodes("```\n:rocket:\n```"), "```\n:rocket:\n```");
  assert.equal(expandShortcodes("`:rocket:` then :rocket:"), "`:rocket:` then 🚀");
});

test("expandKnown does not special-case code (the renderer excludes it already)", () => {
  assert.equal(expandKnown(":rocket: :tada:"), "🚀 🎉");
  assert.equal(expandKnown(":notreal:"), ":notreal:");
});
