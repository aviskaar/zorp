/**
 * One row in the session sidebar.
 *
 * Its own module because of what goes in it. A session's title used to be
 * the first message the user typed, cut off wherever the column ran out. It
 * is now, when the server managed to write one, a short name a model wrote
 * after reading that message and the reply to it. That makes a sidebar row
 * a place model output reaches the page, which is the category of thing
 * this repo tests.
 *
 * Every string here lands through `textContent`. Nothing in this file
 * assembles HTML and nothing in it may start to: a title comes from a model
 * that has been reading tool results and web pages, and the first
 * `innerHTML` on this path is an injection.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `memory-note` and for the same reason: it is what lets a test render into
 * a jsdom document and read back what actually landed.
 */

import type { SessionSummary } from "./api";

/**
 * Shown when a session has no title at all.
 *
 * That is a session with nothing in it yet, not one whose titling call
 * failed. A failed, refused or declined call writes nothing, and the server
 * then sends the verbatim first message, so the row reads exactly as it did
 * before titles existed.
 */
export const UNTITLED = "Untitled session";

export interface SessionRowOptions {
  /** Draw it as the conversation currently open. */
  active: boolean;
  /** Already formatted, human readable "when". */
  when: string;
  /** Called when the row is clicked. */
  onOpen: (session: SessionSummary) => void;
  /**
   * Called after the person has confirmed, in the browser's own dialog,
   * that they want this conversation gone. Never called for a click that
   * only opens the menu or backs out of the dialog.
   */
  onDelete: (session: SessionSummary) => void;
}

function el(doc: Document, tag: string, className: string): HTMLElement {
  const node = doc.createElement(tag);
  node.className = className;
  return node;
}

function textNode(doc: Document, tag: string, className: string, text: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = text;
  return node;
}

/** Three dots, drawn rather than pulled from an icon font. */
function kebabIcon(doc: Document): SVGSVGElement {
  const svg = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("fill", "currentColor");
  for (const cy of [5, 12, 19]) {
    const dot = doc.createElementNS("http://www.w3.org/2000/svg", "circle");
    dot.setAttribute("cx", "12");
    dot.setAttribute("cy", String(cy));
    dot.setAttribute("r", "1.8");
    svg.append(dot);
  }
  return svg;
}

export function sessionRow(
  doc: Document,
  session: SessionSummary,
  options: SessionRowOptions,
): HTMLElement {
  const item = el(doc, "li", "session-item");
  const button = el(doc, "button", "session-button") as HTMLButtonElement;
  button.type = "button";
  button.dataset.id = session.id;
  button.append(
    textNode(doc, "span", "session-title", session.title || UNTITLED),
    textNode(doc, "span", "session-time", options.when),
  );
  if (options.active) {
    button.classList.add("is-active");
    button.setAttribute("aria-current", "true");
  }
  button.addEventListener("click", () => options.onOpen(session));
  item.append(button);

  const title = session.title || UNTITLED;
  const actions = el(doc, "div", "session-actions");
  const menuBtn = el(doc, "button", "icon-btn session-menu-btn") as HTMLButtonElement;
  menuBtn.type = "button";
  menuBtn.setAttribute("aria-haspopup", "true");
  menuBtn.setAttribute("aria-expanded", "false");
  menuBtn.setAttribute("aria-label", `More actions for ${title}`);
  menuBtn.append(kebabIcon(doc));

  const menu = el(doc, "div", "session-menu");
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  const deleteBtn = el(doc, "button", "session-menu-item session-delete") as HTMLButtonElement;
  deleteBtn.type = "button";
  deleteBtn.setAttribute("role", "menuitem");
  deleteBtn.textContent = "Delete";
  menu.append(deleteBtn);

  const closeMenu = () => {
    menu.hidden = true;
    menuBtn.setAttribute("aria-expanded", "false");
  };

  menuBtn.addEventListener("click", () => {
    const opening = menu.hidden;
    menu.hidden = !opening;
    menuBtn.setAttribute("aria-expanded", String(opening));
  });

  // A dropdown that only closes on its own toggle traps the previous one
  // open forever. `focusout` fires on every path away from the menu, mouse
  // or keyboard, without a document-wide listener this module would have to
  // remember to remove when the row is redrawn.
  actions.addEventListener("focusout", (event) => {
    const next = (event as FocusEvent).relatedTarget as Node | null;
    if (!next || !actions.contains(next)) {
      closeMenu();
    }
  });

  deleteBtn.addEventListener("click", () => {
    closeMenu();
    const sure = doc.defaultView!.confirm(`Delete "${title}"? This can't be undone.`);
    if (sure) {
      options.onDelete(session);
    }
  });

  actions.append(menuBtn, menu);
  item.append(actions);
  return item;
}

/** The row shown in place of the list when there are no sessions. */
export function emptySessionRow(doc: Document): HTMLElement {
  return textNode(doc, "li", "session-empty", "No sessions yet.");
}
