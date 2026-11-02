/**
 * The emoji picker: a button beside the microphone opens a popover with a
 * search box, categories along the top, a grid, and recently used first.
 * Clicking a cell inserts the character (or `:name:` for a custom emoji) at
 * the caller's caret.
 *
 * Every string lands through `textContent`, including a category name and a
 * custom emoji's `:name:`. A custom emoji cell draws the one `<img>` this
 * module makes, with a `src` built in code from a listed name, never from
 * text, the same rule the renderer's custom emoji follows.
 *
 * Recently used emoji live in `localStorage` as a per-viewer convenience, read
 * and written through try/catch so a private window or blocked storage leaves
 * the picker working without a Recent row. See issue #243.
 */

import { EMOJI_CATALOG, type EmojiEntry } from "./emoji-table.ts";
import type { CustomEmoji } from "./api.ts";

const RECENT_KEY = "zorp-emoji-recent";
const RECENT_MAX = 24;

/** The category order along the top. "Recent" and "Custom" are synthesized. */
const CATEGORIES = [
  "Recent",
  "Smileys & Emotion",
  "People & Body",
  "Animals & Nature",
  "Food & Drink",
  "Travel & Places",
  "Activities",
  "Objects",
  "Symbols",
  "Flags",
  "Custom",
];

/** One thing the grid can show: a standard character or a custom emoji. */
interface Cell {
  /** `:name:` for a custom emoji, else the character. What a click inserts. */
  insert: string;
  /** The shortcode name, for the title and for custom lookup. */
  name: string;
  /** The character for a standard emoji, or `null` for a custom one. */
  char: string | null;
}

function readRecent(): string[] {
  try {
    const raw = window.localStorage.getItem(RECENT_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((x) => typeof x === "string").slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}

function writeRecent(list: string[]): void {
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(list.slice(0, RECENT_MAX)));
  } catch {
    // A viewer with storage off simply gets no Recent row.
  }
}

export class EmojiPicker {
  private readonly doc: Document;
  private readonly el: HTMLElement;
  private readonly search: HTMLInputElement;
  private readonly tabs: HTMLElement;
  private readonly grid: HTMLElement;
  private custom: CustomEmoji[] = [];
  private category = "Smileys & Emotion";
  private query = "";
  private oninsert: (text: string) => void = () => {};

  constructor(doc: Document) {
    this.doc = doc;
    this.el = doc.createElement("div");
    this.el.className = "emoji-picker";
    this.el.hidden = true;
    this.el.setAttribute("role", "dialog");
    this.el.setAttribute("aria-label", "Emoji");

    this.search = doc.createElement("input");
    this.search.type = "text";
    this.search.className = "emoji-picker-search";
    this.search.placeholder = "Search emoji";
    this.search.setAttribute("aria-label", "Search emoji");
    this.search.addEventListener("input", () => {
      this.query = this.search.value.trim().toLowerCase();
      this.renderGrid();
    });

    this.tabs = doc.createElement("div");
    this.tabs.className = "emoji-picker-tabs";
    this.tabs.setAttribute("role", "tablist");

    this.grid = doc.createElement("div");
    this.grid.className = "emoji-picker-grid";
    this.grid.setAttribute("role", "listbox");

    this.el.append(this.search, this.tabs, this.grid);
    // Clicking inside the popover must not blur the composer.
    this.el.addEventListener("mousedown", (event) => event.preventDefault());
  }

  element(): HTMLElement {
    return this.el;
  }

  get isOpen(): boolean {
    return !this.el.hidden;
  }

  setCustom(custom: CustomEmoji[]): void {
    this.custom = custom;
    if (this.isOpen) {
      this.renderTabs();
      this.renderGrid();
    }
  }

  open(oninsert: (text: string) => void): void {
    this.oninsert = oninsert;
    this.el.hidden = false;
    this.renderTabs();
    this.renderGrid();
  }

  close(): void {
    this.el.hidden = true;
  }

  toggle(oninsert: (text: string) => void): void {
    if (this.isOpen) {
      this.close();
    } else {
      this.open(oninsert);
    }
  }

  private cellsFor(category: string): Cell[] {
    if (category === "Custom") {
      return this.custom.map((c) => ({ insert: `:${c.name}:`, name: c.name, char: null }));
    }
    if (category === "Recent") {
      const recent = readRecent();
      return recent.map((insert) => this.cellForInsert(insert)).filter((c): c is Cell => c !== null);
    }
    return (EMOJI_CATALOG as EmojiEntry[])
      .filter((e) => e.category === category)
      .map((e) => ({ insert: e.char, name: e.name, char: e.char }));
  }

  /** Reconstruct a cell from a stored recent insert (a char or `:name:`). */
  private cellForInsert(insert: string): Cell | null {
    if (insert.startsWith(":") && insert.endsWith(":")) {
      const name = insert.slice(1, -1);
      if (this.custom.some((c) => c.name === name)) {
        return { insert, name, char: null };
      }
      return null; // A custom emoji that was since removed.
    }
    const entry = (EMOJI_CATALOG as EmojiEntry[]).find((e) => e.char === insert);
    return entry ? { insert, name: entry.name, char: entry.char } : null;
  }

  private searchCells(): Cell[] {
    const q = this.query;
    const out: Cell[] = [];
    for (const c of this.custom) {
      if (c.name.toLowerCase().includes(q)) {
        out.push({ insert: `:${c.name}:`, name: c.name, char: null });
      }
    }
    for (const e of EMOJI_CATALOG as EmojiEntry[]) {
      if (e.name.includes(q) || e.keywords.includes(q)) {
        out.push({ insert: e.char, name: e.name, char: e.char });
      }
      if (out.length >= 120) {
        break;
      }
    }
    return out;
  }

  private renderTabs(): void {
    this.tabs.replaceChildren();
    for (const category of CATEGORIES) {
      if (category === "Custom" && this.custom.length === 0) {
        continue;
      }
      const tab = this.doc.createElement("button");
      tab.type = "button";
      tab.className = "emoji-picker-tab";
      tab.setAttribute("role", "tab");
      tab.textContent = tabLabel(category);
      tab.title = category;
      tab.setAttribute("aria-selected", String(category === this.category && this.query === ""));
      if (category === this.category && this.query === "") {
        tab.classList.add("selected");
      }
      tab.addEventListener("click", () => {
        this.category = category;
        this.query = "";
        this.search.value = "";
        this.renderTabs();
        this.renderGrid();
      });
      this.tabs.append(tab);
    }
  }

  private renderGrid(): void {
    this.grid.replaceChildren();
    const cells = this.query ? this.searchCells() : this.cellsFor(this.category);
    if (cells.length === 0) {
      const empty = this.doc.createElement("div");
      empty.className = "emoji-picker-empty";
      empty.textContent = this.query ? "No emoji found" : "Nothing here yet";
      this.grid.append(empty);
      return;
    }
    for (const cell of cells) {
      const button = this.doc.createElement("button");
      button.type = "button";
      button.className = "emoji-picker-cell";
      button.setAttribute("role", "option");
      button.title = `:${cell.name}:`;
      button.setAttribute("aria-label", `:${cell.name}:`);
      if (cell.char !== null) {
        button.textContent = cell.char;
      } else {
        // A custom emoji: the one img, built from the listed name.
        const img = this.doc.createElement("img");
        img.className = "custom-emoji";
        img.src = `/api/emoji/${encodeURIComponent(cell.name)}`;
        img.alt = `:${cell.name}:`;
        button.append(img);
      }
      button.addEventListener("click", () => {
        this.remember(cell.insert);
        this.oninsert(cell.insert);
      });
      this.grid.append(button);
    }
  }

  private remember(insert: string): void {
    const recent = readRecent().filter((x) => x !== insert);
    recent.unshift(insert);
    writeRecent(recent);
  }
}

/** A short glyph for a category tab. Text only. */
function tabLabel(category: string): string {
  const labels: Record<string, string> = {
    Recent: "🕘",
    "Smileys & Emotion": "😀",
    "People & Body": "🧑",
    "Animals & Nature": "🐱",
    "Food & Drink": "🍎",
    "Travel & Places": "✈️",
    Activities: "⚽",
    Objects: "💡",
    Symbols: "❤️",
    Flags: "🏳️",
    Custom: "🖼",
  };
  return labels[category] ?? category;
}
