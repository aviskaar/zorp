/**
 * Showing what the model was told to remember.
 *
 * A turn can be asked to read earlier conversations before answering. When
 * it is, the server sends exactly what it found, and this draws it in the
 * transcript above the answer. That card is not decoration. It is the only
 * way a reader can tell why the model "knew" something, and the only way to
 * see that an answer leaned on a conversation from March that has since
 * been corrected.
 *
 * **Everything here goes through `textContent`.** There is no `innerHTML`
 * in this file and there must never be one, for the reason `markdown.ts`
 * and `panel-view.ts` both say so and then some: a recalled snippet is text
 * out of an old conversation, which means it can be a tool result, a page
 * the agent fetched, or a payload somebody planted months ago waiting for
 * exactly this moment. Assembling markup out of it would be a stored
 * cross-site scripting hole with a retrieval engine feeding it.
 *
 * Every entry is labelled with who wrote it. A line the user wrote and a
 * line a model wrote are not the same kind of thing, and a card that drew
 * them identically would be telling the reader that the assistant's old
 * guesses are part of their history in the same way their own words are.
 */

import type { MemoryCitation } from "./api.ts";

export type { MemoryCitation };

/** How much of a recalled message to show before cutting it. */
export const SNIPPET_CHARS = 220;

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
 * The citations in an event body, with anything unusable dropped.
 *
 * A conversation id is the one field that cannot be defaulted, because it
 * is what the "open" control opens. Everything else falls back to empty, so
 * a missing title reads as untitled rather than putting `undefined` on the
 * page.
 */
export function coerceCitations(value: unknown): MemoryCitation[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const out: MemoryCitation[] = [];
  for (const row of value) {
    if (typeof row !== "object" || row === null) {
      continue;
    }
    const record = row as Record<string, unknown>;
    const conversationId = asString(record.conversation_id).trim();
    if (!conversationId) {
      continue;
    }
    out.push({
      conversation_id: conversationId,
      title: asString(record.title),
      seq: asNumber(record.seq),
      author: asString(record.author),
      when: asString(record.when),
      text: asString(record.text),
      score: asNumber(record.score),
    });
  }
  return out;
}

/** One line, collapsed and cut, ready to be set as `textContent`. */
function snippet(value: string): string {
  const flat = value.replace(/\s+/g, " ").trim();
  return flat.length > SNIPPET_CHARS ? `${flat.slice(0, SNIPPET_CHARS)}…` : flat;
}

/**
 * The line under a citation's title: who wrote it, when, and where in the
 * conversation.
 *
 * An assistant line says what it is worth. "the assistant" alone would read
 * as a source; "the assistant, a model's earlier answer" reads as what it
 * is, which is a thing that was said and not a thing that was checked.
 */
export function attribution(citation: MemoryCitation): string {
  const author =
    citation.author === "you" ? "you" : "the assistant, a model's earlier answer";
  const parts = [`written by ${author}`];
  if (citation.when) {
    parts.push(citation.when);
  }
  parts.push(`message ${citation.seq}`);
  return parts.join(" · ");
}

/**
 * Draw the card.
 *
 * `unavailable` is a first class outcome, not an error: memory was asked
 * for and could not be used, the turn went ahead without it, and the reader
 * has to be told or they will read the answer as one that used their
 * history. So is an empty list, which means memory was on and found
 * nothing, and looks identical to memory being off unless it is said.
 */
export function renderMemoryNote(
  doc: Document,
  citations: MemoryCitation[],
  unavailable: string | null,
  onOpen: (conversationId: string) => void,
): HTMLElement {
  const card = el(doc, "div", "card card-memory");
  const head = el(doc, "div", "card-head");
  head.append(text(doc, "span", "card-title", "Recalled from earlier conversations"));
  card.append(head);

  if (unavailable) {
    card.append(
      text(
        doc,
        "p",
        "card-body",
        `Memory was asked for and could not be used, so this answer did not see it. ${unavailable}`,
      ),
    );
    return card;
  }
  if (!citations.length) {
    card.append(
      text(
        doc,
        "p",
        "card-body",
        "Nothing in your earlier conversations was close enough to this message, so this answer used none.",
      ),
    );
    return card;
  }

  card.append(
    text(
      doc,
      "p",
      "card-body",
      "These were quoted to the model as reference data. They are what was said, not what is true.",
    ),
  );

  const list = el(doc, "ul", "memory-list");
  for (const citation of citations) {
    const item = el(doc, "li", "memory-item");
    const title = citation.title.trim() || "Untitled conversation";
    const button = el(doc, "button", "memory-open") as HTMLButtonElement;
    button.type = "button";
    button.dataset.id = citation.conversation_id;
    button.setAttribute("aria-label", `Open the conversation ${title}`);
    button.textContent = title;
    button.addEventListener("click", () => onOpen(citation.conversation_id));
    item.append(button);

    // A class on the attribution, so an assistant line can be told from a
    // user line at a glance and not only by reading it.
    const kind = citation.author === "you" ? "memory-by-you" : "memory-by-model";
    item.append(text(doc, "p", `memory-attribution ${kind}`, attribution(citation)));

    const quoted = snippet(citation.text);
    if (quoted) {
      item.append(text(doc, "blockquote", "memory-quote", quoted));
    }
    list.append(item);
  }
  card.append(list);
  return card;
}
