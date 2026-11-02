/**
 * Typing `/` in the composer to pick an installed skill for one message.
 *
 * This names a skill; it never moves a skill's body. The picker shows the
 * name and description the toolbar pill already fetched from `GET
 * /api/skills`, which carry no body, and picking one puts `/name ` into the
 * composer as plain text the person can see and delete. The request that
 * goes out names the skill in a `skill` field; the server checks the name
 * and asks the model to load it with the `skill` tool. See issue #252 and
 * `docs/DECISIONS.md` (2026-09-10): a control that pasted a body into the
 * composer was refused because it makes untrusted text look like something
 * the person wrote. A name is not a body.
 *
 * The pure functions below decide when the menu opens and what it shows, and
 * are tested without a page. The widget builds DOM through `textContent`
 * only: a name and description come out of a `SKILL.md` zorp did not write.
 *
 * #243's emoji popover is the same shape and should share this widget.
 */

import type { SkillSummary } from "./api.ts";

/**
 * The query under the caret when a slash-skill menu should be open, or
 * `null` when it should not.
 *
 * The menu opens only when the token the caret sits in starts with `/` at
 * the very start of the message or right after a newline, with no space
 * between that `/` and the caret. So `/re` at the start opens with query
 * `re`, `/` on its own opens with an empty query, but `see /etc` opens
 * nothing (the `/` is mid-line) and `/foo bar|` opens nothing (a space
 * closed the token). The returned query is the text between the `/` and the
 * caret, which is what filters the list.
 */
export function slashQuery(text: string, caret: number): string | null {
  if (caret < 0 || caret > text.length) {
    return null;
  }
  // Walk back from the caret to the start of this token. The token is
  // bounded by the start of the message or by any whitespace.
  let start = caret;
  while (start > 0 && !/\s/.test(text[start - 1])) {
    start -= 1;
  }
  // The char before the token must be the start of the message or a
  // newline: a slash after a space mid-line is not a command.
  const atLineStart = start === 0 || text[start - 1] === "\n";
  if (!atLineStart) {
    return null;
  }
  if (text[start] !== "/") {
    return null;
  }
  // Everything between the slash and the caret is the query. If the token
  // extends past the caret that is fine; only the part up to the caret
  // filters. A query that itself contains a slash is still a prefix match
  // against names, which simply will not match, so no special case.
  return text.slice(start + 1, caret);
}

/**
 * The installed skills whose name begins with the query, case-insensitive.
 *
 * An empty query returns every skill, which is what a bare `/` shows. The
 * order is the order the listing came in, so the page decides it, not this.
 */
export function filterSkills(query: string, skills: SkillSummary[]): SkillSummary[] {
  const q = query.toLowerCase();
  return skills.filter((s) => s.name.toLowerCase().startsWith(q));
}

/**
 * The skill named by the leading `/token` of a message, when that token is
 * an installed skill, else `null`.
 *
 * This is what the request's `skill` field is parsed from, in the browser
 * and checked again on the server. The token has to be a real installed
 * skill: `/etc/hosts` at the start of a message is a path, not a command, so
 * it stays plain text and no `skill` field is sent. The message itself is
 * never changed; `/name` stays in what is typed and stored.
 */
export function leadingSkill(message: string, installed: Set<string>): string | null {
  if (!message.startsWith("/")) {
    return null;
  }
  // The token runs to the first whitespace. A message that is only `/name`
  // with no following text still names the skill.
  const match = message.match(/^\/(\S+)(?:\s|$)/);
  if (!match) {
    return null;
  }
  const name = match[1];
  return installed.has(name) ? name : null;
}

/**
 * Replace the `/token` the caret sits in with `/name ` (a trailing space so
 * the next keystroke is the message, not more of the name). Returns the new
 * text and where the caret should land. The caller sets both on the
 * textarea. Mirrors `slashQuery`'s idea of where the token starts.
 */
export function applyPick(
  text: string,
  caret: number,
  name: string,
): { text: string; caret: number } {
  let start = caret;
  while (start > 0 && !/\s/.test(text[start - 1])) {
    start -= 1;
  }
  // The token runs forward to the next whitespace, so picking mid-name
  // replaces the whole name.
  let end = caret;
  while (end < text.length && !/\s/.test(text[end])) {
    end += 1;
  }
  // Absorb one run of following whitespace so the `/name ` we insert does
  // not stack a second space on the one that was already there.
  let after = end;
  while (after < text.length && /\s/.test(text[after])) {
    after += 1;
  }
  const insert = `/${name} `;
  const next = text.slice(0, start) + insert + text.slice(after);
  return { text: next, caret: start + insert.length };
}

/**
 * The popover list under the caret. A thin widget: the composer owns the
 * keyboard and calls `move`/`pick`/`close`; this draws the rows and tracks
 * the selection. Every string lands through `textContent`.
 */
export class SlashMenu {
  // Explicit fields and assignment rather than a constructor parameter
  // property: the test runner strips types without transforming, and
  // parameter properties are not valid once the types are gone.
  private readonly doc: Document;
  private readonly el: HTMLElement;
  private items: SkillSummary[] = [];
  private index = 0;
  private onpick: (name: string) => void = () => {};

  constructor(doc: Document) {
    this.doc = doc;
    this.el = doc.createElement("div");
    this.el.className = "slash-menu";
    this.el.hidden = true;
    this.el.setAttribute("role", "listbox");
  }

  /** The element to place in the DOM, once, near the composer. */
  element(): HTMLElement {
    return this.el;
  }

  get open(): boolean {
    return !this.el.hidden;
  }

  /**
   * Show `items`, routing a pick to `onpick`. The selection resets to the
   * first item only when the list actually changed: a re-show with the same
   * names (which happens on every keyup while the menu is open) must not
   * snap the highlight back, or keyboard navigation could never leave the
   * top row.
   */
  show(items: SkillSummary[], onpick: (name: string) => void): void {
    const sameList =
      this.open &&
      this.items.length === items.length &&
      this.items.every((s, i) => s.name === items[i].name);
    this.items = items;
    this.onpick = onpick;
    if (items.length === 0) {
      this.close();
      return;
    }
    if (!sameList) {
      this.index = 0;
    } else {
      this.index = Math.min(this.index, items.length - 1);
    }
    this.render();
    this.el.hidden = false;
  }

  close(): void {
    this.el.hidden = true;
    this.items = [];
    this.index = 0;
  }

  /** Move the selection by `delta`, clamped to the list (no wrap). */
  move(delta: number): void {
    if (!this.open) {
      return;
    }
    const last = this.items.length - 1;
    this.index = Math.max(0, Math.min(last, this.index + delta));
    this.render();
  }

  /** Pick the selected skill. No-op when closed or empty. */
  pick(): void {
    if (!this.open || this.items.length === 0) {
      return;
    }
    const name = this.items[this.index].name;
    this.close();
    this.onpick(name);
  }

  private render(): void {
    this.el.replaceChildren();
    this.items.forEach((skill, i) => {
      const row = this.doc.createElement("div");
      row.className = "slash-menu-item";
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(i === this.index));
      if (i === this.index) {
        row.classList.add("selected");
      }
      const name = this.doc.createElement("span");
      name.className = "slash-menu-name";
      name.textContent = `/${skill.name}`;
      const desc = this.doc.createElement("span");
      desc.className = "slash-menu-desc";
      desc.textContent = skill.description;
      row.append(name, desc);
      // A mouse is a first-class way to pick, the same as the keyboard.
      row.addEventListener("mousedown", (event) => {
        // mousedown, not click: a click would blur the textarea first.
        event.preventDefault();
        this.index = i;
        this.pick();
      });
      this.el.append(row);
    });
  }
}
