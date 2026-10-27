/**
 * Thumbs and "better answer" controls under an answer, for RL Studio.
 *
 * These are the entry point of the feedback loop, and the one rule they keep
 * (ADR 0063): the signal is the person's, never a model's. A click records a
 * work-log entry and the feedback on it; nothing asks a model to grade the
 * answer, and the stored answer is labelled model-authored wherever it
 * surfaces. The controls appear only when the server reports RL Studio is
 * present, so a click never writes to a store a default build cannot read.
 *
 * Every string on the page goes through `textContent`. This builds no HTML.
 */

export interface FeedbackContext {
  sessionId: string | null;
  model: string;
  /** The user message this answer replied to, verbatim. */
  prompt: string;
  /** The answer, verbatim. */
  answer: string;
}

/** The two API calls this needs, injectable so a test needs no network. */
export interface FeedbackApi {
  record(entry: {
    session_id: string;
    model: string;
    prompt: string;
    answer: string;
    tool_calls: { name: string; status: string }[];
  }): Promise<string>;
  setFeedback(id: string, feedback: { feedback?: string; edited_answer?: string }): Promise<void>;
}

/**
 * Build the control row. `promptForEdit` returns the person's better answer,
 * or null if they cancelled; it is injected so a test does not need a real
 * dialog.
 */
export function feedbackControls(
  doc: Document,
  ctx: FeedbackContext,
  api: FeedbackApi,
  promptForEdit: (current: string) => string | null = (current) =>
    typeof window !== "undefined" ? window.prompt("Better answer:", current) : null,
): HTMLElement {
  const row = doc.createElement("span");
  row.className = "answer-feedback";

  // Recorded lazily, on the first feedback action, so an answer nobody rates
  // writes nothing. The id is reused for later actions on the same answer.
  let recordedId: string | null = null;
  const ensureRecorded = async (): Promise<string> => {
    if (recordedId) return recordedId;
    recordedId = await api.record({
      session_id: ctx.sessionId ?? "",
      model: ctx.model,
      prompt: ctx.prompt,
      answer: ctx.answer,
      tool_calls: [],
    });
    return recordedId;
  };

  const up = doc.createElement("button");
  up.type = "button";
  up.className = "answer-action feedback-up";
  up.textContent = "👍";
  up.setAttribute("aria-label", "Good answer");
  const down = doc.createElement("button");
  down.type = "button";
  down.className = "answer-action feedback-down";
  down.textContent = "👎";
  down.setAttribute("aria-label", "Bad answer");
  const edit = doc.createElement("button");
  edit.type = "button";
  edit.className = "answer-action feedback-edit";
  edit.textContent = "Better answer";

  const mark = (which: "up" | "down" | null) => {
    up.setAttribute("aria-pressed", which === "up" ? "true" : "false");
    down.setAttribute("aria-pressed", which === "down" ? "true" : "false");
  };
  mark(null);

  up.addEventListener("click", () => {
    mark("up");
    void (async () => {
      const id = await ensureRecorded();
      await api.setFeedback(id, { feedback: "up" });
    })();
  });
  down.addEventListener("click", () => {
    mark("down");
    void (async () => {
      const id = await ensureRecorded();
      await api.setFeedback(id, { feedback: "down" });
    })();
  });
  edit.addEventListener("click", () => {
    const better = promptForEdit(ctx.answer);
    if (better === null || better.trim() === "") return;
    edit.setAttribute("data-edited", "true");
    void (async () => {
      const id = await ensureRecorded();
      await api.setFeedback(id, { edited_answer: better });
    })();
  });

  row.append(up, down, edit);
  return row;
}
