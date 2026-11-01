/**
 * Slash menu for skill picker.
 *
 * When a person types "/" as the first character of the composer (or right
 * after a newline), this menu appears under the caret. It lists installed
 * skills with name and description, filters by prefix as more is typed, and
 * lets Enter or Tab pick one. Picking inserts `/name ` at the cursor so the
 * person can see and edit it before sending.
 *
 * This is a report, never a switch: the menu only shows names and
 * descriptions, never the skill body.
 *
 * Everything goes through `textContent`. Skill names and descriptions come
 * from `SKILL.md` files on disk, which are untrusted text.
 */

import type { SkillSummary } from "./api.ts";

const ITEM_HEIGHT = 44;
const VISIBLE_ITEMS = 7;

export interface SlashMenuView {
  container: HTMLElement;
  list: HTMLElement;
}

/**
 * Create the view elements from a document.
 *
 * The container and list are `<div>`s created fresh each time this is called,
 * so a test can pass any document and get an isolated DOM.
 */
export function slashMenuView(doc: Document): SlashMenuView {
  const container = doc.createElement("div");
  container.className = "slash-menu";
  container.hidden = true;

  const list = doc.createElement("ul");
  list.className = "slash-menu-list";
  container.append(list);

  return { container, list };
}

/**
 * Whether the cursor sits at position zero or right after a newline.
 *
 * That is the only place the slash menu is allowed to open. A slash in the
 * middle of a sentence is a path or a heading, not a command.
 */
function canOpenSlashMenu(input: HTMLTextAreaElement): boolean {
  const val = input.value;
  const pos = input.selectionStart ?? 0;
  // At the very start.
  if (pos === 0) {
    return true;
  }
  // Right after a newline, so the text before the cursor is only newlines
  // and whitespace on the current line.
  if (pos > 0) {
    const before = val.slice(0, pos);
    const lastNewline = before.lastIndexOf("\n");
    const afterNewline = before.slice(lastNewline + 1);
    if (afterNewline.trimStart() === "") {
      return true;
    }
  }
  return false;
}

/**
 * Handle keydown events in the composer to control the slash menu.
 *
 * Returns true if the event was consumed by the menu (arrow keys, Enter, Tab,
 * Escape), false if the composer should handle it normally.
 */
export function handleSlashKeydown(
  event: KeyboardEvent,
  view: SlashMenuView,
  skills: SkillSummary[],
  onSelect: (name: string) => void,
  input: HTMLTextAreaElement,
): boolean {
  if (!input) {
    return false;
  }

  // Opening: "/" at position 0 or after a newline.
  if (event.key === "/" && !event.ctrlKey && !event.altKey && !event.metaKey) {
    const pos = input.selectionStart ?? 0;
    if (canOpenSlashMenu(input) && pos === input.selectionStart) {
      // Don't open if there's already a slash at the cursor position.
      if (pos >= 1 && input.value[pos - 1] === '/') {
        return false;
      }
      // Show the menu at the cursor position.
      openSlashMenu(view, skills, input);
      // Insert "/" if not already present at cursor.
      const val = input.value;
      const pos2 = input.selectionStart ?? 0;
      const before = val.slice(0, pos2);
      if (!before.endsWith("/")) {
        input.value = before + "/" + val.slice(pos2);
        input.selectionStart = input.selectionEnd = pos2 + 1;
        input.dispatchEvent(
          new input.ownerDocument.defaultView!.Event("input", {
            bubbles: true,
          }),
        );
      }
      return true;
    }
    return false;
  }

  // If menu is open, handle its keys.
  if (!view.container.hidden) {
    const key = event.key;

    switch (key) {
      case "ArrowDown": {
        event.preventDefault();
        const items = view.list.querySelectorAll<HTMLLIElement>("li");
        const current = selectedIndex(view);
        const next = (current + 1) % items.length;
        selectIndex(view, next);
        return true;
      }
      case "ArrowUp": {
        event.preventDefault();
        const items = view.list.querySelectorAll<HTMLLIElement>("li");
        const current = selectedIndex(view);
        const prev = (current - 1 + items.length) % items.length;
        selectIndex(view, prev);
        return true;
      }
      case "Enter": {
        // Shift+Enter inserts a newline, even with the menu open.
        if (event.shiftKey) {
          // Let the textarea handle this key after closing the menu.
          closeSlashMenu(view, input);
          return false;
        }
        event.preventDefault();
        pickSelected(view, onSelect, input);
        return true;
      }
      case "Tab": {
        event.preventDefault();
        pickSelected(view, onSelect, input);
        return true;
      }
      case "Escape": {
        event.preventDefault();
        closeSlashMenu(view, input);
        return true;
      }
      default:
        // Typing key: filter the list in real time but let the key
        // pass through into the composer (no preventDefault).
        handleTypingKey(view, skills, event, input);
        return true;
    }
  }

  return false;
}

/**
 * Handle a typing key event while the slash menu is open.
 * Filters the skill list by the new prefix.
 */
function handleTypingKey(
  view: SlashMenuView,
  skills: SkillSummary[],
  event: KeyboardEvent,
  input: HTMLTextAreaElement,
): void {
  // Skip modifier-only keys, navigation, and non-character keys.
  if (event.ctrlKey || event.altKey || event.metaKey) return;
  if (event.key.length > 1 && event.key !== "Backspace" && event.key !== "Delete") return;

  const pos = input.selectionStart ?? 0;
  const val = input.value;

  // Find the slash that is controlling this menu.
  let slashIndex = -1;
  for (let i = Math.max(0, pos - 1); i >= 0; i--) {
    if (val[i] === "/") {
      slashIndex = i;
      break;
    }
    if (val[i] === "\n") break;
  }

  if (slashIndex < 0) return;

  // Build the prefix from text between slash and cursor.
  const prefix = val.slice(slashIndex + 1, pos).toLowerCase();

  // Filter skills by the prefix.
  const filtered = skills.filter((s) => {
    const nameMatch = s.name.toLowerCase().includes(prefix);
    const descMatch = s.description.toLowerCase().includes(prefix);
    return nameMatch || descMatch;
  });

  renderSlashMenu(view, filtered, 0);
}

/**
 * Pick the selected item: close the menu and insert `/name ` at the cursor.
 *
 * Replaces the prefix typed after "/" with the full skill name, plus a space.
 */
function pickSelected(
  view: SlashMenuView,
  onSelect: (name: string) => void,
  input: HTMLTextAreaElement,
): void {
  view.container.hidden = true;
  const items = view.list.querySelectorAll<HTMLLIElement>("li");
  const idx = selectedIndex(view);
  const li = Array.from(items)[idx];
  if (!li || li.dataset.name === undefined) {
    return;
  }
  const name = li.dataset.name!;
  const pos = input.selectionStart ?? 0;
  const val = input.value;

  // Find the slash for this menu.
  let slashIndex = -1;
  for (let i = pos - 1; i >= 0; i--) {
    if (val[i] === "/") {
      slashIndex = i;
      break;
    }
    if (val[i] === "\n") {
      break;
    }
  }
  if (slashIndex < 0) {
    return;
  }

  // Replace the prefix typed after "/" with the skill name and add a space.
  const after = val.slice(pos);
  input.value = val.slice(0, slashIndex) + "/" + name + " " + after;
  const newPos = slashIndex + 1 + name.length + 1; // "/name "
  input.selectionStart = input.selectionEnd = newPos;
  input.dispatchEvent(
    new input.ownerDocument.defaultView!.Event("input", {
      bubbles: true,
    }),
  );

  // Notify the caller of the picked name.
  onSelect(name);
}

/**
 * Close the slash menu and restore the composer.
 *
 * Removes the leading "/" and any prefix typed after it, returning the
 * composer to its pre-menu state.
 */
export function closeSlashMenu(
  view: SlashMenuView,
  input: HTMLTextAreaElement,
): void {
  view.container.hidden = true;
  const pos = input.selectionStart ?? 0;
  const val = input.value;

  // Find the slash.
  let slashIndex = -1;
  for (let i = pos - 1; i >= 0; i--) {
    if (val[i] === "/") {
      slashIndex = i;
      break;
    }
    if (val[i] === "\n") {
      break;
    }
  }

  if (slashIndex >= 0) {
    let newVal: string;
    let newPos: number;
    if (slashIndex === 0) {
      // At start of text: remove only the leading slash.
      newVal = val.slice(1);
      newPos = 0;
    } else {
      // After a newline: remove from slash to the next space (or end).
      let end = slashIndex + 1;
      while (end < val.length && val[end] !== " ") {
        end++;
      }
      if (end < val.length && val[end] === " ") {
        end++; // Include the space.
      }
      newVal = val.slice(0, slashIndex) + val.slice(end);
      newPos = slashIndex + 1;
    }
    input.value = newVal;
    input.selectionStart = input.selectionEnd = newPos;
    input.dispatchEvent(
      new input.ownerDocument.defaultView!.Event("input", {
        bubbles: true,
      }),
    );
  }
}

/**
 * Open the slash menu under the composer, with the given skills filtered
 * by the current composer text after the leading slash.
 */
function openSlashMenu(
  view: SlashMenuView,
  skills: SkillSummary[],
  input: HTMLTextAreaElement,
): void {
  const pos = input.selectionStart ?? 0;
  const val = input.value;

  // Extract the prefix: text between the leading "/" (or "/\n") and the
  // cursor.
  const beforeCursor = val.slice(0, pos);
  // Find the slash that started this.
  let slashStart = beforeCursor.lastIndexOf("/");
  // If there's a newline after that slash, skip past it too.
  const newlineAfterSlash = beforeCursor.indexOf("\n", slashStart);
  if (newlineAfterSlash !== -1 && newlineAfterSlash < pos) {
    slashStart = newlineAfterSlash;
  }
  const prefix = beforeCursor.slice(slashStart + 1).toLowerCase();

  // Filter skills by the prefix.
  const filtered = skills.filter((s) => {
    const nameMatch = s.name.toLowerCase().includes(prefix);
    const descMatch = s.description.toLowerCase().includes(prefix);
    return nameMatch || descMatch;
  });

  // Update aria-label for accessibility.
  view.container.setAttribute(
    "aria-label",
    `Skill picker. ${filtered.length} result${filtered.length === 1 ? "" : "s"}. Use arrow keys to navigate.`,
  );

  renderSlashMenu(view, filtered, 0);
  positionMenu(view, input);
}

/**
 * Render the skill list into the menu container.
 */
export function renderSlashMenu(
  view: SlashMenuView,
  skills: SkillSummary[],
  selected: number,
): void {
  view.list.replaceChildren();

  if (skills.length === 0) {
    const empty = view.list.ownerDocument.createElement("li");
    empty.className = "slash-menu-empty";
    empty.textContent = "No skills match your input.";
    view.list.append(empty);
    view.container.hidden = false;
    return;
  }

  for (let i = 0; i < skills.length; i++) {
    const skill = skills[i];
    const li = view.list.ownerDocument.createElement("li");
    li.className =
      i === selected ? "slash-menu-item selected" : "slash-menu-item";
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", i === selected ? "true" : "false");
    li.dataset.index = String(i);
    li.dataset.name = skill.name;

    const nameSpan = view.list.ownerDocument.createElement("span");
    nameSpan.className = "slash-menu-name";
    nameSpan.textContent = skill.name;

    const descSpan = view.list.ownerDocument.createElement("span");
    descSpan.className = "slash-menu-description";
    descSpan.textContent = skill.description;

    li.append(nameSpan, descSpan);
    view.list.append(li);
  }

  view.container.hidden = false;
}

function selectedIndex(view: SlashMenuView): number {
  const items = view.list.querySelectorAll<HTMLLIElement>("li.selected");
  if (items.length > 0) {
    return parseInt(items[0].dataset.index ?? "0", 10);
  }
  return 0;
}

function selectIndex(view: SlashMenuView, index: number): void {
  const items = view.list.querySelectorAll<HTMLLIElement>("li");
  items.forEach((li, i) => {
    li.classList.toggle("selected", i === index);
    li.setAttribute("aria-selected", i === index ? "true" : "false");
  });
  // Scroll the selected item into view.
  const selected = items[index];
  if (selected) {
    const listRect = view.list.getBoundingClientRect();
    const itemRect = selected.getBoundingClientRect();
    if (itemRect.bottom > listRect.bottom - 5) {
      view.list.scrollTop += itemRect.bottom - listRect.bottom + 5;
    } else if (itemRect.top < listRect.top + 5) {
      view.list.scrollTop -= listRect.top - itemRect.top - 5;
    }
  }
}

/**
 * Position the slash menu under the composer at the caret position.
 */
function positionMenu(
  view: SlashMenuView,
  input: HTMLTextAreaElement,
): void {
  const composerRect = input.getBoundingClientRect();
  view.container.style.position = "absolute";
  view.container.style.left = composerRect.left + "px";
  view.container.style.top = (composerRect.bottom + 2) + "px";
  view.container.style.width = composerRect.width + "px";
  view.container.style.maxHeight = (VISIBLE_ITEMS * ITEM_HEIGHT) + "px";
  view.container.style.overflowY = "auto";
}
