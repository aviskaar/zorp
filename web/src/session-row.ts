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
  return item;
}

/** The row shown in place of the list when there are no sessions. */
export function emptySessionRow(doc: Document): HTMLElement {
  return textNode(doc, "li", "session-empty", "No sessions yet.");
}
