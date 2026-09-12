/**
 * Drawing a Zorp mode attempt in the transcript.
 *
 * Zorp mode is one pre-registered `investigate` attempt, run from the
 * browser, plus a read of what it left in the aryabhatta ledger. There
 * is no aryabhatta engine and this does not draw one: aryabhatta is a
 * record plus readers, and `investigate` is what writes to it.
 *
 * The block is opened by `investigate_done` and filled in by the ledger
 * read that follows it. That order is deliberate. The ledger is a
 * separate read the page can repeat without running anything, so a run
 * that fell over still shows what it recorded before it fell over.
 *
 * **Everything here goes through `textContent`.** A condition's value is
 * text off a row some run wrote, a track id is derived from a question
 * somebody typed, and a metric value is a number a model reported. There
 * is no `innerHTML` in this file and there must never be one: reaching
 * for a markdown library here is reaching for `innerHTML`.
 *
 * **Nothing here interprets.** The lines below are the recorded rows and
 * arithmetic over them. No model is asked what the ledger means, which
 * is the same split `critique` and the detectors use: detection is code,
 * and the interpreting comes afterwards and from somewhere else.
 */

import type {
  InvestigateDoneEvent,
  InvestigateProgressEvent,
  Ledger,
  LedgerExperiment,
} from "./api.ts";

/**
 * What the page does when a control is pressed.
 *
 * Passed in rather than called from here, so this file draws and never
 * talks to a server, which is what lets the whole view be tested on a
 * jsdom page with no network.
 */
export interface ZorpControls {
  stopAfter(): void;
  refresh(): void;
}

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

/**
 * What the attempt's verdict was.
 *
 * Three outcomes, three sentences. An attempt that never reached a
 * verdict is not an approved one and is not a killed one, and drawing it
 * as either would report something that did not happen.
 *
 * "Killed" is said in plain words rather than shown as a state. A track
 * that the run record now calls dead is the single most consequential
 * thing this view has to say.
 */
export function verdictLine(event: InvestigateDoneEvent): string {
  if (event.approved === true) {
    return "The attempt finished and the track stays active.";
  }
  if (event.approved === false) {
    return "The attempt finished and the track was killed. Either the pre-registered kill threshold was breached, or the checkpoint said no.";
  }
  return "The attempt did not finish. Whatever it recorded before it stopped is below.";
}

/**
 * Whether the server would ask for a forecast on the next attempt.
 *
 * Said out loud because it is the reason the expectations column is
 * empty. Forecasting costs a model call on every attempt and is off
 * unless the person running the server turned it on, and an empty ledger
 * is the honest state for a record nobody has fed.
 */
export function forecastLine(forecasting: boolean): string {
  if (forecasting) {
    return "Forecasting is on for this server, so each attempt records an expectation before it runs.";
  }
  return "Forecasting is off for this server, so no attempt records an expectation and nothing here can be scored for calibration. Set ZORP_FORECAST where the server runs to turn it on.";
}

/**
 * What the run is doing right now, in a sentence.
 *
 * The server sends a phase name from a closed set and nothing else. The
 * words are chosen here, because a phase is a fact about the run and a
 * sentence is a thing a reader reads, and letting the server send prose
 * would put a second copy of this page's voice somewhere it cannot be
 * tested.
 */
export function phaseLine(event: InvestigateProgressEvent): string {
  switch (event.phase) {
    case "prereg":
      return "Committing the pre-registration. It cannot be changed after this.";
    case "attempt-started":
      return `Attempt ${event.attempt} of ${event.of}.`;
    case "attempt-finished":
      return `Attempt ${event.attempt} of ${event.of} finished and is recorded.`;
    case "write-up":
      return "The attempts are done. Writing the track up.";
    case "critique":
      return "Auditing the draft against what the attempts recorded.";
    default:
      return "Running.";
  }
}

/** A stated coverage, as a percentage a reader can compare to a band. */
function coverage(confidence: number): string {
  return `${Math.round(confidence * 100)}%`;
}

export class ZorpModeView {
  private block: HTMLElement | null = null;
  /** The one line saying what the run is doing, replaced in place. */
  private phase: HTMLElement | null = null;
  /** One row per attempt, so four minutes of work has a shape. */
  private attempts: HTMLElement | null = null;
  /**
   * The ledger, held so a live read replaces it rather than stacking a
   * second copy under the first. A run sends one of these after every
   * attempt.
   */
  private ledger: HTMLElement | null = null;
  private stopAfterButton: HTMLButtonElement | null = null;
  private refreshButton: HTMLButtonElement | null = null;

  // Written out rather than declared as constructor parameter
  // properties: those emit code, and the test runner strips types
  // without compiling. `panel-view.ts` carries the same note.
  private readonly doc: Document;
  private readonly transcript: HTMLElement;
  private readonly controls: ZorpControls | null;

  constructor(doc: Document, transcript: HTMLElement, controls: ZorpControls | null = null) {
    this.doc = doc;
    this.transcript = transcript;
    this.controls = controls;
  }

  /** Whether a block is open. */
  get isOpen(): boolean {
    return this.block !== null;
  }

  private ensureBlock(): HTMLElement {
    if (this.block) {
      return this.block;
    }
    const block = el(this.doc, "div", "card card-zorp");
    const head = el(this.doc, "div", "card-head");
    head.append(text(this.doc, "span", "card-title", "Zorp mode"));
    block.append(head);
    this.transcript.append(block);
    this.block = block;
    return block;
  }

  /**
   * Open the block when the run starts, rather than when it ends.
   *
   * The whole reason this exists. A run is several whole agent runs back
   * to back and it used to draw nothing at all until the verdict, so a
   * person watching had no way to tell a long attempt from a hung one.
   */
  start(trackId: string): void {
    const block = this.ensureBlock();
    block.dataset.running = "true";
    const live = el(this.doc, "div", "zorp-live");
    live.append(text(this.doc, "p", "zorp-track", `track ${trackId}`));
    this.phase = text(this.doc, "p", "zorp-phase", "Starting.");
    live.append(this.phase);
    this.attempts = el(this.doc, "ol", "zorp-attempts");
    live.append(this.attempts);
    if (this.controls) {
      live.append(this.controlRow());
    }
    block.append(live);
  }

  /**
   * The two controls, and what separates them.
   *
   * Stopping after this attempt is not stopping. The composer's stop
   * cancels the agent where it stands and the run produces no write-up;
   * this lets the attempt that is running finish and be recorded, skips
   * the ones that would have followed, and still writes the track up. A
   * person mid-run wants one or the other and a single button cannot be
   * both, so they are separate and each says what it does.
   *
   * Refreshing the ledger is deliberately absent while a run is going. A
   * running attempt holds the run record open and the read would be
   * refused, so the button appears when the run ends. What fills the
   * ledger during a run arrives on the event stream instead.
   */
  private controlRow(): HTMLElement {
    const row = el(this.doc, "div", "zorp-controls");
    const stop = this.doc.createElement("button");
    stop.type = "button";
    stop.className = "zorp-control";
    stop.textContent = "Stop after this attempt";
    stop.title =
      "Let the attempt that is running finish and be recorded, skip the rest, and still write the track up.";
    stop.addEventListener("click", () => {
      stop.disabled = true;
      stop.textContent = "Will stop after this attempt";
      this.controls?.stopAfter();
    });
    this.stopAfterButton = stop;
    row.append(stop);

    const refresh = this.doc.createElement("button");
    refresh.type = "button";
    refresh.className = "zorp-control";
    refresh.textContent = "Refresh ledger";
    refresh.hidden = true;
    refresh.addEventListener("click", () => this.controls?.refresh());
    this.refreshButton = refresh;
    row.append(refresh);
    return row;
  }

  /**
   * The run moved on. Says where it is and marks the attempts off.
   *
   * Draws the ledger when one rides along, which is on
   * `attempt-finished` and nowhere else. It replaces whatever ledger is
   * already drawn rather than appending, because a three attempt run
   * sends three of them.
   */
  progress(event: InvestigateProgressEvent): void {
    if (!this.block) {
      this.start(event.track_id);
    }
    if (this.phase) {
      this.phase.textContent = phaseLine(event);
    }
    if (event.attempt !== undefined && event.of !== undefined) {
      this.markAttempt(event.attempt, event.of, event.phase === "attempt-finished");
    }
    if (event.ledger) {
      this.showLedger(event.ledger);
    }
  }

  /**
   * One row per attempt the run said it would make, filled in as they
   * land.
   *
   * The rows are drawn from the count the server stated up front, so a
   * run that stops early leaves the skipped ones visibly unstarted
   * rather than silently absent. What a person is comparing is several
   * measurements of one metric, and how many of them there were is part
   * of the claim.
   */
  private markAttempt(n: number, of: number, finished: boolean): void {
    if (!this.attempts) {
      return;
    }
    while (this.attempts.children.length < of) {
      const row = el(this.doc, "li", "zorp-attempt");
      row.dataset.state = "queued";
      row.textContent = `Attempt ${this.attempts.children.length + 1}`;
      this.attempts.append(row);
    }
    const row = this.attempts.children[n - 1] as HTMLElement | undefined;
    if (row) {
      row.dataset.state = finished ? "done" : "running";
    }
  }

  /** The attempt closed. Opens the block if nothing else has. */
  done(event: InvestigateDoneEvent): void {
    const block = this.ensureBlock();
    delete block.dataset.running;
    if (this.phase) {
      this.phase.remove();
      this.phase = null;
    }
    // Nothing left to stop, and the ledger read that was refused while
    // the record was open is now the way to see what landed.
    if (this.stopAfterButton) {
      this.stopAfterButton.remove();
      this.stopAfterButton = null;
    }
    if (this.refreshButton) {
      this.refreshButton.hidden = false;
    }
    const summary = el(this.doc, "div", "zorp-summary");
    if (event.approved !== undefined) {
      summary.dataset.approved = String(event.approved);
    }
    summary.append(
      text(this.doc, "p", "zorp-verdict", verdictLine(event)),
      text(this.doc, "p", "zorp-track", `track ${event.track_id}`),
    );
    block.append(summary);
  }

  /**
   * What the ledger recorded.
   *
   * A missing run record and an empty ledger are drawn differently on
   * purpose. One says nobody has run anything here; the other says the
   * record exists and nothing has fed it. They are different facts and a
   * reader has to be able to tell them apart.
   */
  showLedger(ledger: Ledger): void {
    const block = this.ensureBlock();
    const wrap = el(this.doc, "div", "zorp-ledger");
    // Replaced, never appended. A run sends one of these after every
    // attempt, and three stacked copies of a growing ledger is three
    // answers to one question on one page.
    const previous = this.ledger;
    this.ledger = wrap;
    if (previous) {
      previous.replaceWith(wrap);
    }
    wrap.append(text(this.doc, "p", "zorp-ledger-head", "aryabhatta ledger"));

    if (!ledger.present) {
      wrap.append(
        text(
          this.doc,
          "p",
          "zorp-ledger-empty",
          "There is no run record here yet, so nothing has been recorded to read.",
        ),
      );
      if (!previous) {
        block.append(wrap);
      }
      return;
    }

    wrap.append(text(this.doc, "p", "zorp-forecast", forecastLine(ledger.forecasting)));

    if (ledger.experiments.length === 0) {
      wrap.append(
        text(
          this.doc,
          "p",
          "zorp-ledger-empty",
          "The run record exists and holds no attempt for this question.",
        ),
      );
      if (!previous) {
        block.append(wrap);
      }
      return;
    }

    const list = el(this.doc, "ol", "zorp-experiments");
    for (const experiment of ledger.experiments) {
      list.append(this.experimentItem(experiment));
    }
    wrap.append(list);
    if (!previous) {
      block.append(wrap);
    }
  }

  private experimentItem(experiment: LedgerExperiment): HTMLElement {
    const item = el(this.doc, "li", "zorp-experiment");
    item.dataset.status = experiment.status;
    item.append(
      text(this.doc, "span", "zorp-experiment-id", experiment.id),
      text(this.doc, "span", "zorp-experiment-status", experiment.status),
    );

    // Conditions first, because they are the half zorp did not record at
    // all before aryabhatta. Outputs were recorded and inputs were not,
    // so a deviation had nothing to be a deviation from.
    item.append(text(this.doc, "p", "zorp-section", "ran under"));
    if (experiment.conditions.length === 0) {
      item.append(text(this.doc, "p", "zorp-none", "No conditions recorded."));
    } else {
      const conditions = el(this.doc, "ul", "zorp-conditions");
      for (const condition of experiment.conditions) {
        const row = el(this.doc, "li", "zorp-condition");
        row.append(
          text(this.doc, "span", "zorp-key", condition.key),
          text(this.doc, "span", "zorp-value", condition.value),
        );
        conditions.append(row);
      }
      item.append(conditions);
    }

    item.append(text(this.doc, "p", "zorp-section", "expected"));
    if (experiment.expectations.length === 0) {
      item.append(
        text(
          this.doc,
          "p",
          "zorp-none",
          "No forecast was recorded before this attempt, so it will not be scored by the calibration report.",
        ),
      );
    } else {
      const expectations = el(this.doc, "ul", "zorp-expectations");
      for (const expectation of experiment.expectations) {
        const row = el(this.doc, "li", "zorp-expectation");
        row.append(
          text(this.doc, "span", "zorp-key", expectation.metric_key),
          text(this.doc, "span", "zorp-value", String(expectation.expected_value)),
          text(
            this.doc,
            "span",
            "zorp-interval",
            `${expectation.interval_low} to ${expectation.interval_high} at ${coverage(expectation.confidence)}`,
          ),
        );
        expectations.append(row);
      }
      item.append(expectations);
    }

    item.append(text(this.doc, "p", "zorp-section", "observed"));
    if (experiment.metrics.length === 0) {
      item.append(text(this.doc, "p", "zorp-none", "No metric recorded."));
    } else {
      const metrics = el(this.doc, "ul", "zorp-metrics");
      for (const metric of experiment.metrics) {
        const row = el(this.doc, "li", "zorp-metric");
        row.append(
          text(this.doc, "span", "zorp-key", metric.key),
          text(this.doc, "span", "zorp-value", metric.value),
        );
        metrics.append(row);
      }
      item.append(metrics);
    }

    return item;
  }

  /**
   * Forget the open block without removing it.
   *
   * The block already on the page stays where it is, and the next
   * attempt starts a new one rather than appending to a stale one.
   */
  close(): void {
    this.block = null;
    this.phase = null;
    this.attempts = null;
    this.ledger = null;
    this.stopAfterButton = null;
    this.refreshButton = null;
  }
}
