/**
 * The copy button that sits under a finished answer.
 *
 * Its own module for the same reason `artifact-view.ts` is: the interesting
 * parts are testable without a browser, and the browser is where they would
 * otherwise go untested. The clipboard write is injected rather than reached
 * for through `navigator`, which keeps the refusal path reachable in a test.
 * A browser can and does refuse the write, and a button that silently does
 * nothing is worse than one that admits it failed.
 */

/** Puts text on the system clipboard. `navigator.clipboard.writeText` in practice. */
export type WriteText = (text: string) => Promise<void>;

/** How long the button stays on its result before offering again. */
export const RESET_MS = 1600;

const OFFER = "Copy";
const DONE = "Copied";
const FAILED = "Copy failed";

/**
 * @param doc    document the button belongs to
 * @param answer the text to copy, read when the button is clicked
 * @param write  puts it on the clipboard
 * @param after  defers the reset; `setTimeout` in practice
 */
export function copyButton(
  doc: Document,
  answer: () => string,
  write: WriteText,
  after: (fn: () => void, ms: number) => void = (fn, ms) => {
    setTimeout(fn, ms);
  },
): HTMLButtonElement {
  const button = doc.createElement("button");
  button.type = "button";
  button.className = "copy-btn";
  button.textContent = OFFER;
  // The visible label is one word and the message it belongs to is above it,
  // so a screen reader gets the longer form.
  button.setAttribute("aria-label", "Copy this answer");

  const settle = (label: string, state: string | null): void => {
    // Assigned as text, never as markup. The label is ours either way, but
    // this is the one place an answer and a DOM node meet, and the rule in
    // this UI is that model output reaches the page as text or not at all.
    button.textContent = label;
    if (state === null) {
      delete button.dataset.state;
    } else {
      button.dataset.state = state;
    }
  };

  button.addEventListener("click", () => {
    // Read now, not when the button was built. A streamed answer is not final
    // until the turn closes, and the server's finished text replaces the
    // fragments that were on the page a moment earlier.
    void write(answer()).then(
      () => {
        settle(DONE, "done");
        after(() => settle(OFFER, null), RESET_MS);
      },
      () => {
        settle(FAILED, "failed");
        after(() => settle(OFFER, null), RESET_MS);
      },
    );
  });

  return button;
}
