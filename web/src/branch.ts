/**
 * Branching a chat at one of its answers.
 *
 * The server names the answer by ordinal: the Nth assistant message with
 * text, which is what `GET /api/sessions/:id` replays as an `assistant`
 * entry and what the page draws as a zorp message. The page counts as it
 * draws, on a reopened transcript and on a live turn alike, so no seq has
 * to travel and both sides count the same thing.
 *
 * What the page must not count is a row the store never got. An answer cut
 * off by a stop or an error stays on the page, because it is what the
 * reader saw, but the loop records nothing for a reply that never finished,
 * so it has no ordinal and gets no button.
 */

/** Counts answers the way the server does: text-bearing, in order, from one. */
export class AnswerCount {
  private n = 0;

  /**
   * The ordinal for a message about to go on the page, or null when it is
   * not an answer the store has: empty text, or `kept` false for a row the
   * turn ended without recording.
   */
  next(text: string, kept = true): number | null {
    if (!kept || text.trim() === "") return null;
    this.n += 1;
    return this.n;
  }

  reset(): void {
    this.n = 0;
  }
}

const LABEL = "Branch";
const PURPOSE = "Start a new chat from this answer";

/**
 * The button. It goes down while `branch` runs and comes back when that
 * settles: on success the page has moved to the new chat and this row is
 * gone, and on failure the person may try again. What it says is its own
 * label, never the answer.
 */
export function branchButton(doc: Document, branch: () => Promise<void>): HTMLButtonElement {
  const button = doc.createElement("button");
  button.type = "button";
  button.className = "copy-btn branch-btn";
  button.textContent = LABEL;
  button.title = PURPOSE;
  button.setAttribute("aria-label", PURPOSE);
  button.addEventListener("click", () => {
    button.disabled = true;
    // Both arms, not `finally`: `finally` hands a rejection on, and a
    // rejection nobody is waiting for is an unhandled one.
    const up = (): void => {
      button.disabled = false;
    };
    void branch().then(up, up);
  });
  return button;
}
