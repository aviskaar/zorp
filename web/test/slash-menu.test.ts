/**
 * Slash menu for skill picker.
 *
 * Tests the view creation, opening/closing, keyboard navigation, filtering,
 * and selection without a full page.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  slashMenuView,
  handleSlashKeydown,
  closeSlashMenu,
  renderSlashMenu,
  type SlashMenuView,
} from "../src/slash-menu.ts";

const MARKUP =
  "<!doctype html><body>" +
  '<textarea id="input"></textarea>' +
  "</body></html>";

function fixture(): {
  dom: JSDOM;
  doc: Document;
  view: SlashMenuView;
  textArea: HTMLTextAreaElement;
} {
  const dom = new JSDOM(MARKUP);
  const doc = dom.window.document as unknown as Document;
  const textArea = doc.getElementById(
    "input"
  ) as HTMLTextAreaElement;
  return { dom, doc, view: slashMenuView(doc), textArea };
}

const SKILLS = [
  { name: "tidy-notes", description: "Tidy notes", path: "/", scope: "env" },
  { name: "code-docs", description: "Write docs", path: "/", scope: "env" },
  { name: "lint-fix", description: "Fix lint errors", path: "/", scope: "workspace" },
] as const;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type SkillSummary = any;

test("view creates a hidden container with a list", () => {
  const { doc, view } = fixture();
  assert.strictEqual(view.container.hidden, true);
  assert(view.container.classList.contains("slash-menu"));
  assert(view.list.classList.contains("slash-menu-list"));
});

test("renderSlashMenu renders skills as list items", () => {
  const { doc, view } = fixture();
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);
  assert.strictEqual(view.container.hidden, false);
  const items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items.length, 3);
  assert.strictEqual(items[0].dataset.name, "tidy-notes");
  assert(items[0].classList.contains("selected"));
  const spans = items[0].querySelectorAll("span");
  assert.strictEqual(spans[0].textContent, "tidy-notes");
  assert.strictEqual(spans[1].textContent, "Tidy notes");
});

test("renderSlashMenu shows no skills message when empty", () => {
  const { doc, view } = fixture();
  renderSlashMenu(view, [], 0);
  assert.strictEqual(view.container.hidden, false);
  const items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items.length, 1);
  assert(items[0].classList.contains("slash-menu-empty"));
  assert.strictEqual(
    items[0].textContent,
    "No skills match your input."
  );
});

test("arrow keys move selection", () => {
  const { doc, view, textArea } = fixture();
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);

  const downEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowDown" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(downEvent, view, SKILLS as SkillSummary[], () => {}, textArea);
  let items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items[1].classList.contains("selected"), true);

  const downEvent2 = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowDown" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(downEvent2, view, SKILLS as SkillSummary[], () => {}, textArea);
  items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items[2].classList.contains("selected"), true);

  const downEvent3 = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowDown" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(downEvent3, view, SKILLS as SkillSummary[], () => {}, textArea);
  items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items[0].classList.contains("selected"), true);

  const upEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowUp" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(upEvent, view, SKILLS as SkillSummary[], () => {}, textArea);
  items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items[2].classList.contains("selected"), true);
});

test("Enter picks selected item", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "/";
  textArea.selectionStart = 1;
  textArea.selectionEnd = 1;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);
  let picked: string | null = null;
  const pickHandler = (name: string) => {
    picked = name;
  };

  const enterEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "Enter" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(enterEvent, view, SKILLS as SkillSummary[], pickHandler, textArea);

  assert.strictEqual(picked, "tidy-notes");
  assert(view.container.hidden, true);
  assert.strictEqual(textArea.value, "/tidy-notes ");
  assert.strictEqual(textArea.selectionStart, 12);
});

test("Tab picks selected item same as Enter", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "/";
  textArea.selectionStart = 1;
  textArea.selectionEnd = 1;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);
  let picked: string | null = null;
  const pickHandler = (name: string) => {
    picked = name;
  };

  const tabEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "Tab" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(tabEvent, view, SKILLS as SkillSummary[], pickHandler, textArea);

  assert.strictEqual(picked, "tidy-notes");
});

test("Escape closes the menu", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "/ti";
  textArea.selectionStart = 3;
  textArea.selectionEnd = 3;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);
  assert.strictEqual(view.container.hidden, false);

  const escapeEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "Escape" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(escapeEvent, view, SKILLS as SkillSummary[], () => {}, textArea);
  assert(view.container.hidden, true);
});

test("Shift+Enter lets textarea handle it", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "/tidy-notes ";
  textArea.selectionStart = 12;
  textArea.selectionEnd = 12;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);

  const shiftEnterEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "Enter", shiftKey: true }
  ) as unknown as KeyboardEvent;
  const handled = handleSlashKeydown(
    shiftEnterEvent,
    view,
    SKILLS as SkillSummary[],
    () => {},
    textArea
  );
  assert.strictEqual(handled, false);
  assert(view.container.hidden, true);
});

test("slash in middle of text does not open menu", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "hello / there";
  textArea.selectionStart = 12;
  textArea.selectionEnd = 12;
  let opened = false;
  const pickHandler = () => {
    opened = true;
  };

  const slashEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "/" }
  ) as unknown as KeyboardEvent;
  const handled = handleSlashKeydown(
    slashEvent,
    view,
    SKILLS as SkillSummary[],
    pickHandler,
    textArea
  );
  assert.strictEqual(handled, false);
  assert(!opened);
  assert(view.container.hidden, true);
});

test("slash after newline opens menu", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "first line\n";
  textArea.selectionStart = 11;
  textArea.selectionEnd = 11;
  let opened = false;
  const pickHandler = () => {
    opened = true;
  };

  const slashEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "/" }
  ) as unknown as KeyboardEvent;
  const handled = handleSlashKeydown(
    slashEvent,
    view,
    SKILLS as SkillSummary[],
    pickHandler,
    textArea
  );
  assert.strictEqual(handled, true);
  assert(!opened);
  assert(!view.container.hidden);
});

test("slash opening inserts / if not present", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "";
  textArea.selectionStart = 0;
  textArea.selectionEnd = 0;
  let picked: string | null = null;
  const pickHandler = (name: string) => {
    picked = name;
  };

  const slashEvent = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "/" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(slashEvent, view, SKILLS as SkillSummary[], pickHandler, textArea);

  assert.strictEqual(textArea.value, "/");
});

test("selectIndex scrolls selected into view", () => {
  const { doc, view, textArea } = fixture();
  const manySkills = Array.from({ length: 12 }, (_, i) => ({
    name: `skill-${i}`,
    description: `Description ${i}`,
    path: "/",
    scope: "env",
  })) as unknown as SkillSummary[];
  renderSlashMenu(view, manySkills, 0);

  const down1 = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowDown" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(down1, view, manySkills, () => {}, textArea);
  const down2 = new textArea.ownerDocument.defaultView!.KeyboardEvent(
    "keydown",
    { key: "ArrowDown" }
  ) as unknown as KeyboardEvent;
  handleSlashKeydown(down2, view, manySkills, () => {}, textArea);

  const items = view.list.querySelectorAll<HTMLLIElement>("li");
  assert.strictEqual(items[2].classList.contains("selected"), true);
});

test("closeSlashMenu removes slash prefix and closes", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "/hello world";
  textArea.selectionStart = 6;
  textArea.selectionEnd = 6;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);

  closeSlashMenu(view, textArea);

  assert(view.container.hidden, true);
  assert.strictEqual(textArea.value, "hello world");
  assert.strictEqual(textArea.selectionStart, 0);
});

test("closeSlashMenu with no slash does nothing", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "hello world";
  textArea.selectionStart = 5;
  textArea.selectionEnd = 5;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);

  closeSlashMenu(view, textArea);

  assert(view.container.hidden, true);
  assert.strictEqual(textArea.value, "hello world");
});

test("closeSlashMenu removes everything up to newline before slash", () => {
  const { doc, view, textArea } = fixture();
  textArea.value = "first line\n/cool stuff";
  textArea.selectionStart = 14;
  textArea.selectionEnd = 14;
  renderSlashMenu(view, SKILLS as SkillSummary[], 0);

  closeSlashMenu(view, textArea);

  assert(view.container.hidden, true);
  assert.strictEqual(textArea.value, "first line\nstuff");
  assert.strictEqual(textArea.selectionStart, 12);
});
