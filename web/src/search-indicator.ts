/**
 * How the page shows that this agent can search the web.
 *
 * `web_search` is the only built-in that sends anything off this machine,
 * and it is off in a default build. Whether it is there depends on three
 * things the browser cannot see: whether the server was compiled with the
 * `search` feature, whether the policy permits the tool, and whether the
 * search provider found its key. So this module never decides anything. It
 * draws what `GET /api/capabilities` said and nothing else, and with no
 * answer at all it draws nothing.
 *
 * It is a report, not a switch. There is no control here that turns search
 * on or off, and there should not be: the three conditions above are settled
 * before the process starts, or in its environment, not in a page.
 *
 * Everything reaching the page goes through `textContent` and attribute
 * values. This module builds no HTML strings, for the same reason
 * `markdown.ts` does not.
 */

import type { ToolAvailability } from "./api.ts";

/** The text on the pill. Short: the tooltip carries the detail. */
export const SEARCH_LABEL = "Web search";

/** The elements the indicator writes into. */
export interface SearchIndicatorView {
  root: HTMLElement;
  text: HTMLElement;
}

/** Collect them from a document that already contains the markup. */
export function searchIndicatorView(doc: Document): SearchIndicatorView {
  const byId = (id: string): HTMLElement => {
    const node = doc.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node;
  };
  return { root: byId("search-indicator"), text: byId("search-indicator-text") };
}

/**
 * Draw what the server reported.
 *
 * `null` means nothing has been reported yet, which is drawn the same way an
 * unavailable tool is: as nothing. An indicator that appeared while the
 * answer was still in flight would be a guess, and this pill exists to
 * replace guessing.
 */
export function renderSearchIndicator(
  view: SearchIndicatorView,
  capability: ToolAvailability | null,
): void {
  if (!capability?.available) {
    view.root.hidden = true;
    view.text.textContent = "";
    view.root.removeAttribute("title");
    view.root.removeAttribute("aria-label");
    return;
  }
  view.root.hidden = false;
  view.text.textContent = SEARCH_LABEL;
  view.root.title = capability.detail;
  view.root.setAttribute("aria-label", `Web search: ${capability.detail}`);
}
