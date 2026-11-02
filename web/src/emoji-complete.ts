/**
 * Emoji autocomplete in the composer: type `:` and two or more characters and
 * a short list opens under the caret.
 *
 * The pure functions (`emojiQuery`, `matchEmoji`, `applyEmojiPick`) decide when
 * the list opens, what it shows, and what a pick inserts, and are tested
 * without a page. The widget draws rows through `textContent` only, in the
 * shape of `SlashMenu`.
 *
 * A standard emoji inserts its character (already expanded, the way the picker
 * inserts a character). A custom emoji has no character, so it inserts
 * `:name:`, which the renderer draws from the file and the store carries
 * honestly. See issue #243.
 */

import { EMOJI_CATALOG, type EmojiEntry } from "./emoji-table.ts";

/**
 * A character the token may contain. Uppercase is allowed so `:Rocket` opens
 * the list even though shortcodes themselves are lowercase; matching lowercases
 * the query, so the case-insensitive behavior the matcher documents holds.
 */
const NAME_CHAR = /[a-zA-Z0-9_+-]/;

/** A match offered in the list: what to show and what a pick inserts. */
export interface EmojiMatch {
  /** The shortcode name, e.g. `rocket`. */
  name: string;
  /** The character for a standard emoji, or `null` for a custom one. */
  char: string | null;
  /** What replaces the `:query` token: the character, or `:name:`. */
  insert: string;
}

/**
 * The `:query` token under the caret when the list should open, or `null`.
 *
 * The token is a `:` followed by one or more shortcode characters, with the
 * caret at the end of that run. The list opens only when the query is two or
 * more characters, so a bare `:` or `:r` does not flood it, and only when the
 * `:` is at the start of input or follows whitespace, so a `:` inside a word
 * (a URL, a time like `12:34`) does not open it. `start` is the index of the
 * `:`, which a pick replaces.
 */
export function emojiQuery(text: string, caret: number): { start: number; query: string } | null {
  if (caret < 0 || caret > text.length) {
    return null;
  }
  let start = caret;
  while (start > 0 && NAME_CHAR.test(text[start - 1])) {
    start -= 1;
  }
  // The character before the run must be the opening colon.
  if (start === 0 || text[start - 1] !== ":") {
    return null;
  }
  const colon = start - 1;
  // The colon must open a token: start of input, or after whitespace. This
  // keeps `http://x` and `12:34` from opening the list.
  if (colon > 0 && !/\s/.test(text[colon - 1])) {
    return null;
  }
  const query = text.slice(start, caret);
  if (query.length < 2) {
    return null;
  }
  return { start: colon, query };
}

/**
 * Up to eight matches for `query`, names that begin with it first and names
 * that merely contain it after, with custom emoji (passed in, since they are
 * not in the generated catalog) matched the same way. Case-insensitive.
 */
export function matchEmoji(query: string, custom: readonly string[] = []): EmojiMatch[] {
  const q = query.toLowerCase();
  const prefix: EmojiMatch[] = [];
  const contains: EmojiMatch[] = [];
  const seen = new Set<string>();

  const consider = (name: string, char: string | null) => {
    if (seen.has(name)) {
      return;
    }
    const lower = name.toLowerCase();
    const match: EmojiMatch = { name, char, insert: char ?? `:${name}:` };
    if (lower.startsWith(q)) {
      seen.add(name);
      prefix.push(match);
    } else if (lower.includes(q)) {
      seen.add(name);
      contains.push(match);
    }
  };

  for (const name of custom) {
    consider(name, null);
  }
  for (const entry of EMOJI_CATALOG as EmojiEntry[]) {
    consider(entry.name, entry.char);
  }
  return [...prefix, ...contains].slice(0, 8);
}

/**
 * Replace the `:query` token the caret sits in with `insert`. `start` is the
 * index of the `:` (from `emojiQuery`); the token runs forward over any more
 * shortcode characters so picking mid-name replaces the whole thing. Returns
 * the new text and where the caret lands.
 */
export function applyEmojiPick(
  text: string,
  start: number,
  caret: number,
  insert: string,
): { text: string; caret: number } {
  let end = caret;
  while (end < text.length && NAME_CHAR.test(text[end])) {
    end += 1;
  }
  // A closing colon right after the name is part of the token the person was
  // typing (`:rocket:`), so absorb it rather than leave it dangling.
  if (text[end] === ":") {
    end += 1;
  }
  const next = text.slice(0, start) + insert + text.slice(end);
  return { text: next, caret: start + insert.length };
}

/**
 * The popover list under the caret. A thin widget like `SlashMenu`: the
 * composer owns the keyboard and calls `move`/`pick`/`close`; this draws the
 * rows and tracks the selection. Every string lands through `textContent`.
 */
export class EmojiComplete {
  private readonly doc: Document;
  private readonly el: HTMLElement;
  private readonly owner: HTMLElement | null;
  private readonly menuId: string;
  private items: EmojiMatch[] = [];
  private index = 0;
  private onpick: (m: EmojiMatch) => void = () => {};

  constructor(doc: Document, owner?: HTMLElement | null) {
    this.doc = doc;
    this.owner = owner ?? null;
    this.menuId = "emoji-complete";
    this.el = doc.createElement("div");
    this.el.className = "slash-menu emoji-complete";
    this.el.id = this.menuId;
    this.el.hidden = true;
    this.el.setAttribute("role", "listbox");
    if (this.owner) {
      this.owner.setAttribute("aria-haspopup", "listbox");
    }
    this.el.addEventListener("mousedown", (event) => event.preventDefault());
  }

  element(): HTMLElement {
    return this.el;
  }

  get open(): boolean {
    return !this.el.hidden;
  }

  show(items: EmojiMatch[], onpick: (m: EmojiMatch) => void): void {
    const sameList =
      this.open &&
      this.items.length === items.length &&
      this.items.every((m, i) => m.name === items[i].name);
    this.items = items;
    this.onpick = onpick;
    if (items.length === 0) {
      this.close();
      return;
    }
    const before = this.index;
    if (!sameList) {
      this.index = 0;
    } else {
      this.index = Math.min(this.index, items.length - 1);
    }
    if (!sameList || this.index !== before || this.el.hidden) {
      this.render();
    }
    this.el.hidden = false;
    this.syncAria();
  }

  close(): void {
    this.el.hidden = true;
    this.items = [];
    this.index = 0;
    this.syncAria();
  }

  move(delta: number): void {
    if (!this.open) {
      return;
    }
    const last = this.items.length - 1;
    this.index = Math.max(0, Math.min(last, this.index + delta));
    this.render();
    this.syncAria();
  }

  pick(): void {
    if (!this.open || this.items.length === 0) {
      return;
    }
    const match = this.items[this.index];
    this.close();
    this.onpick(match);
  }

  private syncAria(): void {
    if (!this.owner) {
      return;
    }
    if (this.open && this.items.length > 0) {
      this.owner.setAttribute("aria-activedescendant", `${this.menuId}-opt-${this.index}`);
    } else {
      this.owner.removeAttribute("aria-activedescendant");
    }
  }

  private render(): void {
    this.el.replaceChildren();
    let selected: HTMLElement | null = null;
    this.items.forEach((match, i) => {
      const row = this.doc.createElement("div");
      row.className = "slash-menu-item";
      row.id = `${this.menuId}-opt-${i}`;
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(i === this.index));
      if (i === this.index) {
        row.classList.add("selected");
        selected = row;
      }
      const glyph = this.doc.createElement("span");
      glyph.className = "emoji-complete-glyph";
      // A standard emoji shows its character; a custom one shows nothing here
      // (the terminal cannot draw it and the popover keeps it text-only).
      glyph.textContent = match.char ?? "🖼";
      const name = this.doc.createElement("span");
      name.className = "slash-menu-name";
      name.textContent = `:${match.name}:`;
      row.append(glyph, name);
      row.addEventListener("mousedown", (event) => {
        event.preventDefault();
        this.index = i;
        this.pick();
      });
      this.el.append(row);
    });
    if (selected && typeof (selected as HTMLElement).scrollIntoView === "function") {
      (selected as HTMLElement).scrollIntoView({ block: "nearest" });
    }
  }
}
