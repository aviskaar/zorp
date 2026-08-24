/**
 * Search results for the conversation sidebar.
 *
 * Its own module for the reason `session-url.ts` is: the interesting part
 * is that this data is an *input*. Everything else in the sidebar shows
 * what the server said about itself. A search result carries a slice of a
 * conversation, which is a model writing about files and web pages it has
 * been reading, and it arrives through a text box the user typed into.
 *
 * **Everything here goes through `textContent`.** There is no `innerHTML`
 * in this file and there must never be one, for the same reason
 * `markdown.ts` and `panel-view.ts` say so. A result list that assembles
 * markup would be a cross-site scripting hole with a search box in front
 * of it, which is worse than the same hole without one.
 *
 * The response is checked rather than trusted. A row that is not the shape
 * it claims to be is dropped, not rendered with `undefined` in it.
 */

import type { RecallHit, RecallStatus } from "./api.ts";

export type { RecallHit, RecallStatus };

/**
 * How much of a matching message to show. Long enough to recognize the
 * conversation, short enough that ten results still fit in a sidebar.
 */
export const SNIPPET_CHARS = 160;

function el(doc: Document, tag: string, className = ""): HTMLElement {
  const node = doc.createElement(tag);
  if (className) {
    node.className = className;
  }
  return node;
}

function text(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/**
 * The rows in a response body, with anything unusable dropped.
 *
 * An id is the one field that cannot be defaulted: it is what a click
 * opens, and a row that cannot be opened is not a result. Everything else
 * gets an empty value, because a missing title should read as untitled and
 * not put the word `undefined` on the page.
 */
export function coerceHits(value: unknown): RecallHit[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const hits: RecallHit[] = [];
  for (const row of value) {
    if (typeof row !== "object" || row === null) {
      continue;
    }
    const record = row as Record<string, unknown>;
    const id = asString(record.id).trim();
    if (!id) {
      continue;
    }
    hits.push({
      id,
      title: asString(record.title),
      seq: asNumber(record.seq),
      role: asString(record.role),
      snippet: asString(record.snippet),
      score: asNumber(record.score),
    });
  }
  return hits;
}

/** One line, collapsed and cut, ready to be set as `textContent`. */
function snippet(value: string): string {
  const flat = value.replace(/\s+/g, " ").trim();
  return flat.length > SNIPPET_CHARS ? `${flat.slice(0, SNIPPET_CHARS)}…` : flat;
}

/**
 * Draw the results, replacing whatever the list held.
 *
 * Replacing rather than appending, because a search box fires again on
 * every keystroke and a list that grew would be a list nobody could read.
 */
export function renderResults(
  doc: Document,
  list: HTMLElement,
  hits: RecallHit[],
  onPick: (id: string) => void,
): void {
  list.replaceChildren();
  if (!hits.length) {
    renderNotice(doc, list, "Nothing close enough to show.");
    return;
  }
  for (const hit of hits) {
    const title = hit.title.trim() || "Untitled conversation";
    const item = el(doc, "li", "recall-item");
    const button = el(doc, "button", "recall-button") as HTMLButtonElement;
    button.type = "button";
    button.dataset.id = hit.id;
    // Named for a screen reader, which reads the button and not the
    // paragraph under it. Without this the whole list announces as
    // "button, button, button".
    button.setAttribute("aria-label", `Open the conversation ${title}`);
    button.append(text(doc, "span", "recall-title", title));
    const line = snippet(hit.snippet);
    if (line) {
      button.append(text(doc, "span", "recall-snippet", line));
    }
    button.addEventListener("click", () => onPick(hit.id));
    item.append(button);
    list.append(item);
  }
}

/** A sentence in place of a list. Also `textContent`, also never markup. */
export function renderNotice(doc: Document, list: HTMLElement, message: string): void {
  list.replaceChildren(text(doc, "li", "recall-empty", message));
}

/**
 * What the line under the search box says.
 *
 * An unavailable search repeats the server's own words. They are written
 * to be read by a person and they name the thing to fix, which a generic
 * "search unavailable" does not. Catch-up gets its own sentence because
 * "no results" and "not indexed yet" look identical on screen and mean
 * completely different things.
 */
export function summarize(status: RecallStatus): string {
  if (!status.available) {
    return status.reason?.trim() || "Conversation search is not available on this server.";
  }
  const total = Math.max(0, status.conversations);
  const indexed = Math.max(0, Math.min(status.indexed_conversations, total));
  if (status.running) {
    return `Indexing on this machine. ${indexed} of ${total} conversations indexed.`;
  }
  if (!status.ready) {
    return `${indexed} of ${total} conversations indexed. Automatic indexing is catching up.`;
  }
  const plural = total === 1 ? "conversation" : "conversations";
  const model = status.model?.trim();
  const by = model ? `, embedded on this machine by ${model}` : "";
  return `Ready. ${indexed} ${plural} indexed${by}.`;
}
