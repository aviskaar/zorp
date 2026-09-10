/**
 * The seam in a transcript where the older conversation became a summary.
 *
 * A collapsed native `details` saying how many messages went and what the
 * transcript cost before and after, with the summary itself readable under
 * it. Closed by default, because the summary is not the conversation and a
 * reader scrolling back wants the conversation.
 *
 * **The summary is drawn as plain text through `textContent`, never through
 * the markdown renderer.** That is deliberate and it is not a shortcut: the
 * summary arrives as `##` headings and numbered lists, and drawn as
 * headings and lists it starts to look like part of the conversation. The
 * whole point of this marker is that it is not. The renderer is for
 * answers.
 *
 * It is also model-authored text, which is the category every injection
 * test in this repo is about, and it is labelled as such where a reader can
 * see it. Nothing in this file assembles HTML and nothing in it may start
 * to.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `memory-note` and for the same reason: it is what lets a test render into
 * a jsdom document and read back what actually landed.
 */

import { formatTokens } from "./context-meter.ts";

/** Said where a reader can see it, on every marker. */
export const MODEL_WRITTEN = "model-written summary";

export interface CompactionMarkerData {
  /** How many messages the summary stands in for. */
  messages: number;
  tokens_before: number;
  tokens_after: number;
  /** Model-authored. Goes on the page through `textContent`. */
  summary: string;
}

function el(doc: Document, tag: string, className: string): HTMLElement {
  const node = doc.createElement(tag);
  node.className = className;
  return node;
}

function textNode(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

/**
 * The one line on the summary, in the meter's own number formatting so two
 * places on the page do not spell the same count two ways.
 *
 * "about", because these are byte estimates unless a provider reported
 * usage, and the `context` frame calls that `estimated`. A count that
 * looked exact would be claiming a precision it does not have.
 */
export function markerLabel(data: CompactionMarkerData): string {
  const n = Math.max(0, Math.round(data.messages));
  const plural = n === 1 ? "message" : "messages";
  return `Context compacted: ${n} ${plural} summarized, about ${formatTokens(
    data.tokens_before,
  )} tokens to ${formatTokens(data.tokens_after)}`;
}

/** Draw the marker. */
export function compactionMarker(doc: Document, data: CompactionMarkerData): HTMLElement {
  const root = doc.createElement("details");
  root.className = "compaction";
  // Closed. The summary is not the conversation.
  root.open = false;

  const summary = doc.createElement("summary");
  summary.className = "compaction-summary";
  summary.append(
    textNode(doc, "span", "compaction-label", markerLabel(data)),
    textNode(doc, "span", "compaction-source", MODEL_WRITTEN),
  );

  // `pre` because the summary's own line breaks are what make its sections
  // readable, and this is the way to keep them without turning them into
  // markup.
  const body = textNode(doc, "pre", "compaction-body", data.summary);

  root.append(summary, body);
  return root;
}
