/**
 * The assistant message currently being streamed.
 *
 * The server sends fragments as the model produces them and then states the
 * finished answer exactly once. This holds the fragments on the page in the
 * meantime and gets out of the way when the real answer arrives.
 *
 * Its own module because the lifecycle has edges worth testing: a fragment
 * that opens a message, a hundred more that must not open a hundred more, a
 * finished text that replaces rather than appends, and a turn that ends with
 * nothing to show.
 */
import type { ZorpEventType } from "./api";

/**
 * Whether this event ends the message currently being streamed.
 *
 * The agent runs the model more than once per turn, so a turn looks like
 * `working, deltas, working_done, tool, working, deltas, working_done,
 * assistant, done`. Both boundaries in that sequence are easy to get wrong,
 * and each one was:
 *
 * - Ending on `tool` is required. Text before a tool call and text after it
 *   are two different things the model said, and merging them puts the tool
 *   activity underneath both.
 * - Ending on `working_done` is wrong, and looks right. It fires the instant
 *   the model call returns, which is *before* the finished answer is sent, so
 *   it closed the message and the answer then arrived a second time as a
 *   duplicate.
 */
export function endsStreamedMessage(type: ZorpEventType): boolean {
  switch (type) {
    // More of the message, or the finished text for it.
    case "assistant_delta":
    case "assistant":
      return false;
    // Status, not content.
    case "working":
    case "working_done":
      return false;
    // Anything a reader can see.
    case "tool":
    case "verify":
    case "notice":
    case "approval_request":
    case "error":
    case "done":
      return true;
    default: {
      // A new event type has to make this decision on purpose.
      const unreachable: never = type;
      return unreachable;
    }
  }
}

export class StreamedMessage {
  private row: HTMLElement | null = null;
  private body: HTMLElement | null = null;
  private text = "";
  private frame: number | null = null;
  /// Whether a render is queued. Separate from `frame` because the handle is
  /// only known after `schedule` returns, and a scheduler that runs its
  /// callback immediately would otherwise have that assignment land after the
  /// callback cleared it, leaving a queue that never drains again.
  private pending = false;

  // Written out rather than declared as constructor parameter properties:
  // those emit code, and the test runner strips types without compiling.
  private readonly transcript: HTMLElement;
  private readonly render: (body: HTMLElement, text: string, authoritative: boolean) => void;
  private readonly schedule: (fn: () => void) => number;
  private readonly cancel: (handle: number) => void;

  /**
   * @param transcript where messages are appended
   * @param render     puts text on the page; the markdown renderer in
   *                   practice. Its third argument says whether this text is
   *                   the server's finished answer rather than a preview,
   *                   which is what decides whether anything in it can be
   *                   marked as a finding. A preview is half a sentence and
   *                   half a sentence has not found anything.
   * @param schedule   defers a render; one animation frame in practice
   * @param cancel     cancels a deferred render
   */
  constructor(
    transcript: HTMLElement,
    render: (body: HTMLElement, text: string, authoritative: boolean) => void,
    // Wrapped, not passed by reference. `requestAnimationFrame` is a method
    // on `window` and browsers brand-check its receiver, so storing the bare
    // function and calling it as `this.schedule(...)` throws "Illegal
    // invocation" and nothing ever paints. Every test injected its own
    // scheduler, so this path had no coverage until it broke in a browser.
    schedule: (fn: () => void) => number = (fn) => requestAnimationFrame(fn),
    cancel: (handle: number) => void = (handle) => cancelAnimationFrame(handle),
  ) {
    this.transcript = transcript;
    this.render = render;
    this.schedule = schedule;
    this.cancel = cancel;
  }

  /** Whether a message is currently open. */
  get open(): boolean {
    return this.row !== null;
  }

  /**
   * Replace the message body with `text`.
   *
   * The clear is load bearing. The renderer appends nodes, which is right for
   * a message rendered once, so re-rendering a growing string into the same
   * node without clearing produces "hehellhello" rather than "hello".
   */
  private paint(text: string, authoritative: boolean): void {
    if (!this.body) return;
    this.body.replaceChildren();
    this.render(this.body, text, authoritative);
  }

  /** Add a fragment, opening a message if this is the first one. */
  append(chunk: string): void {
    if (!this.row) {
      const doc = this.transcript.ownerDocument;
      const row = doc.createElement("article");
      row.className = "msg msg-assistant is-streaming";
      const label = doc.createElement("div");
      label.className = "msg-role";
      label.textContent = "zorp";
      const body = doc.createElement("div");
      body.className = "msg-body";
      row.append(label, body);
      this.transcript.append(row);
      this.row = row;
      this.body = body;
      this.text = "";
    }
    this.text += chunk;
    // A local model can produce tokens faster than the document can be
    // re-parsed. One render per frame keeps a long answer from turning the
    // tab into a markdown benchmark.
    if (!this.pending) {
      this.pending = true;
      this.frame = this.schedule(() => {
        this.pending = false;
        this.paint(this.text, false);
      });
    }
  }

  /**
   * Close the message.
   *
   * `authoritative` is the server's finished text and wins over anything
   * streamed. `null` means the turn ended without one, from an error or a
   * cancel, in which case the fragments are all there is and are kept.
   *
   * Returns false when nothing was open, so the caller knows the finished
   * text still needs appending as an ordinary message.
   *
   * This is also the one moment a finding can be marked, and only in the
   * `authoritative` case. The fragments kept after a cancel are text the
   * server never confirmed, possibly cut off mid-sentence, and badging that
   * would be exactly the unearned confidence the marker exists to avoid. One
   * paint, one chance, so a marker cannot end up on the page twice.
   */
  finish(authoritative: string | null): boolean {
    if (this.pending && this.frame !== null) {
      this.cancel(this.frame);
    }
    this.pending = false;
    this.frame = null;
    if (!this.row || !this.body) return false;

    const text = authoritative ?? this.text;
    if (text.trim() === "") {
      this.row.remove();
    } else {
      this.paint(text, authoritative !== null);
      this.row.classList.remove("is-streaming");
    }
    this.row = null;
    this.body = null;
    this.text = "";
    return true;
  }
}
