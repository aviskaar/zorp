/**
 * Tests for the composer's slash-skill picker.
 *
 * The pure functions decide when the menu opens, what it shows, and which
 * skill a message names; the widget draws the rows. The load-bearing claims:
 * the menu opens only for a `/` at the start of a line, a message names a
 * skill only when its leading token is a real installed skill (so a path
 * like `/etc/hosts` is left alone), and every string on the page lands
 * through `textContent` because a skill name and description come from a
 * `SKILL.md` zorp did not write.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  SlashMenu,
  applyPick,
  filterSkills,
  leadingSkill,
  slashQuery,
} from "../src/slash-menu.ts";
import type { SkillSummary } from "../src/api.ts";

function skill(name: string, description = ""): SkillSummary {
  return { name, description, path: `/skills/${name}`, scope: "user", declared_tools: [] };
}

const SKILLS = [skill("react-components"), skill("landing-page"), skill("review")];

test("slashQuery opens on a message-leading slash and reads the query", () => {
  assert.equal(slashQuery("/re", 3), "re");
  assert.equal(slashQuery("/", 1), "");
  // Leading whitespace before the slash still counts as the message start,
  // because the message is trimmed before the skill is parsed.
  assert.equal(slashQuery("  /re", 5), "re");
});

test("slashQuery stays closed when the slash is not the leading token", () => {
  // A slash in the middle of a sentence is not a command.
  assert.equal(slashQuery("see /etc", 8), null);
  // A space closed the token.
  assert.equal(slashQuery("/foo bar", 8), null);
  // Plain text opens nothing.
  assert.equal(slashQuery("hello", 5), null);
  // A slash after a newline is not the message's leading token, and only the
  // leading token is ever sent as the skill, so the picker does not open.
  assert.equal(slashQuery("hello\n/lan", 10), null);
});

test("slashQuery reads only up to the caret", () => {
  // Caret after `/re`, even though the token continues.
  assert.equal(slashQuery("/review", 3), "re");
});

test("filterSkills prefix-matches case-insensitively and empty shows all", () => {
  assert.deepEqual(
    filterSkills("re", SKILLS).map((s) => s.name),
    ["react-components", "review"],
  );
  assert.deepEqual(
    filterSkills("", SKILLS).map((s) => s.name),
    ["react-components", "landing-page", "review"],
  );
  assert.deepEqual(filterSkills("zz", SKILLS), []);
});

test("leadingSkill names a skill only for a real installed leading token", () => {
  const installed = new Set(["review", "landing-page"]);
  assert.equal(leadingSkill("/review look at this", installed), "review");
  assert.equal(leadingSkill("/review", installed), "review");
  // Not installed: left as plain text, no skill named.
  assert.equal(leadingSkill("/nope do a thing", installed), null);
  // A path, not a command.
  assert.equal(leadingSkill("/etc/hosts please", installed), null);
  // No leading slash.
  assert.equal(leadingSkill("review this", installed), null);
});

test("applyPick replaces the token with `/name ` and places the caret", () => {
  const r = applyPick("/re", 3, "review");
  assert.equal(r.text, "/review ");
  assert.equal(r.caret, "/review ".length);
  // Mid-name pick replaces the whole name and absorbs the following space,
  // so there is no double space before the rest.
  const mid = applyPick("/rev ok", 2, "review");
  assert.equal(mid.text, "/review ok");
  assert.equal(mid.caret, "/review ".length);
});

test("SlashMenu shows rows through textContent and never builds HTML", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const menu = new SlashMenu(dom.window.document);
  dom.window.document.body.append(menu.element());
  const injected = skill("<img src=x onerror=alert(1)>", "<b>desc</b>");
  menu.show([injected], () => {});
  assert.equal(menu.open, true);
  // No element was created from the injected string: it is text.
  assert.equal(dom.window.document.querySelector("img"), null);
  assert.equal(dom.window.document.querySelector("b"), null);
  const name = dom.window.document.querySelector(".slash-menu-name");
  assert.equal(name?.textContent, "/<img src=x onerror=alert(1)>");
});

test("SlashMenu move clamps and pick returns the selected name", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const menu = new SlashMenu(dom.window.document);
  dom.window.document.body.append(menu.element());
  let picked = "";
  menu.show(SKILLS, (name) => {
    picked = name;
  });
  menu.move(-1); // clamps at the top
  menu.move(1); // -> landing-page
  menu.pick();
  assert.equal(picked, "landing-page");
  assert.equal(menu.open, false);
});

test("SlashMenu announces the popup and active option on the owner", () => {
  const dom = new JSDOM("<!doctype html><body><textarea></textarea></body>");
  const input = dom.window.document.querySelector("textarea")!;
  const menu = new SlashMenu(dom.window.document, input);
  dom.window.document.body.append(menu.element());
  // A textbox with a popup listbox, not a combobox.
  assert.equal(input.getAttribute("aria-haspopup"), "listbox");
  assert.equal(input.getAttribute("aria-controls"), menu.element().id);
  assert.equal(input.getAttribute("role"), null);
  assert.equal(input.getAttribute("aria-activedescendant"), null);
  menu.show(SKILLS, () => {});
  assert.equal(input.getAttribute("aria-activedescendant"), `${menu.element().id}-opt-0`);
  menu.move(1);
  assert.equal(input.getAttribute("aria-activedescendant"), `${menu.element().id}-opt-1`);
  menu.close();
  assert.equal(input.getAttribute("aria-activedescendant"), null);
});

test("SlashMenu stays closed for an empty list", () => {
  const dom = new JSDOM("<!doctype html><body></body>");
  const menu = new SlashMenu(dom.window.document);
  menu.show([], () => {});
  assert.equal(menu.open, false);
});

test("SlashMenu keeps the selection when re-shown with the same items", () => {
  // The composer re-shows the menu on every keyup, which must not snap the
  // highlight back to the top, or arrow-key navigation could never move.
  const dom = new JSDOM("<!doctype html><body></body>");
  const menu = new SlashMenu(dom.window.document);
  dom.window.document.body.append(menu.element());
  let picked = "";
  const onpick = (name: string) => {
    picked = name;
  };
  menu.show(SKILLS, onpick);
  menu.move(1); // -> landing-page
  menu.show(SKILLS, onpick); // same list, as a keyup would
  menu.pick();
  assert.equal(picked, "landing-page");
});
