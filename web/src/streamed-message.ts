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
    // Less of the message: the fragments come down and the fresh answer
    // streams into the same row under a line saying why. Closing the row
    // here would leave the dead fragments on the page as a finished answer.
    case "assistant_withdrawn":
      return false;
    // Status, not content. The context meter lives in the topbar and puts
    // nothing in the transcript, so an answer arriving while it updates must
    // not be cut in two.
    case "working":
    case "working_done":
    case "context":
      return false;
    // A title renames a row in the sidebar and the heading above the
    // transcript. It puts nothing in the transcript itself, so it must not
    // cut an answer in two. It normally lands after `done`, when nothing
    // is streaming, but a title from the previous turn can arrive during
    // this one and that is exactly the case this line covers.
    case "session_title":
      return false;
    // Anything a reader can see. A call starting puts its line on the page
    // before the result does, so it is the boundary `tool` used to be.
    case "tool_started":
    case "tool":
    case "verify":
    case "notice":
    case "approval_request":
    case "error":
    case "stopped":
    case "done":
      return true;
    // A recall card is reader visible and lands before the model is
    // called, so nothing is streaming when it arrives. Closing a streamed
    // message here is the right answer anyway: the card is a block in the
    // transcript, and a half written answer must not carry on underneath
    // one.
    case "memory":
      return true;
    // Panel frames are all reader visible, so they close a streamed
    // message the same way a tool line does. A panel does not stream an
    // assistant message today, so nothing is currently cut in two by
    // this; it is the right answer if one ever does.
    case "reviewer_started":
    case "reviewer_finished":
    case "reviewer_failed":
    case "panel_done":
      return true;
    // The same for a Zorp mode attempt's closing frame. It is a reader
    // visible block, so anything that was streaming before it is
    // finished rather than continued underneath it.
    case "investigate_done":
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
  private readonly render: (body: HTMLElement, text: string) => void;
  private readonly schedule: (fn: () => void) => number;
  private readonly cancel: (handle: number) => void;
  private readonly onFinished: (row: HTMLElement, text: string) => void;

  /**
   * @param transcript where messages are appended
   * @param render     puts text on the page; the markdown renderer in practice
   * @param schedule   defers a render; one animation frame in practice
   * @param cancel     cancels a deferred render
   * @param onFinished handed each completed row and the text left on it, so a
   *                   caller can decorate it. This is how the copy button
   *                   reaches a streamed answer without this module knowing
   *                   anything about clipboards.
   */
  constructor(
    transcript: HTMLElement,
    render: (body: HTMLElement, text: string) => void,
    // Wrapped, not passed by reference. `requestAnimationFrame` is a method
    // on `window` and browsers brand-check its receiver, so storing the bare
    // function and calling it as `this.schedule(...)` throws "Illegal
    // invocation" and nothing ever paints. Every test injected its own
    // scheduler, so this path had no coverage until it broke in a browser.
    schedule: (fn: () => void) => number = (fn) => requestAnimationFrame(fn),
    cancel: (handle: number) => void = (handle) => cancelAnimationFrame(handle),
    onFinished: (row: HTMLElement, text: string) => void = () => {},
  ) {
    this.transcript = transcript;
    this.render = render;
    this.schedule = schedule;
    this.cancel = cancel;
    this.onFinished = onFinished;
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
  private paint(text: string): void {
    if (!this.body) return;
    this.body.replaceChildren();
    this.render(this.body, text);
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
        this.paint(this.text);
      });
    }
  }

  /**
   * Take back what streamed and say why, keeping the message open.
   *
   * The provider dropped the answer and the agent is asking again, so the
   * fragments on the page are the start of an answer nobody will finish.
   * They are discarded, `status` goes in their place as one text node, and
   * the next fragment streams in under it. Returns false when nothing was
   * open, so the caller can put the status somewhere else.
   *
   * `status` lands through `textContent`. It is composed by the page from
   * numbers today, but this is the renderer's boundary and nothing crosses
   * it as markup.
   */
  withdraw(status: string): boolean {
    if (this.pending && this.frame !== null) {
      this.cancel(this.frame);
    }
    this.pending = false;
    this.frame = null;
    if (!this.row || !this.body) return false;

    this.text = "";
    this.body.replaceChildren();
    const line = this.row.ownerDocument.createElement("div");
    line.className = "msg-withdrawn";
    line.textContent = status;
    // Before the body, so the fresh answer streams in under the line that
    // explains where the last one went.
    this.row.insertBefore(line, this.body);
    return true;
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
   */
  finish(authoritative: string | null): boolean {
    if (this.pending && this.frame !== null) {
      this.cancel(this.frame);
    }
    this.pending = false;
    this.frame = null;
    if (!this.row || !this.body) return false;

    const final = authoritative ?? this.text;
    if (final.trim() === "") {
      this.row.remove();
    } else {
      this.paint(final);
      this.row.classList.remove("is-streaming");
      // After the paint, so anything added here survives the last repaint,
      // and only for a row that stayed: an empty turn has nothing to offer.
      this.onFinished(this.row, final);
    }
    this.row = null;
    this.body = null;
    this.text = "";
    return true;
  }
}
