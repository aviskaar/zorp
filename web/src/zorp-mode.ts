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

import type { InvestigateDoneEvent, Ledger, LedgerExperiment } from "./api.ts";

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

/** A stated coverage, as a percentage a reader can compare to a band. */
function coverage(confidence: number): string {
  return `${Math.round(confidence * 100)}%`;
}

export class ZorpModeView {
  private block: HTMLElement | null = null;

  // Written out rather than declared as constructor parameter
  // properties: those emit code, and the test runner strips types
  // without compiling. `panel-view.ts` carries the same note.
  private readonly doc: Document;
  private readonly transcript: HTMLElement;

  constructor(doc: Document, transcript: HTMLElement) {
    this.doc = doc;
    this.transcript = transcript;
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

  /** The attempt closed. Opens the block if nothing else has. */
  done(event: InvestigateDoneEvent): void {
    const block = this.ensureBlock();
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
      block.append(wrap);
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
      block.append(wrap);
      return;
    }

    const list = el(this.doc, "ol", "zorp-experiments");
    for (const experiment of ledger.experiments) {
      list.append(this.experimentItem(experiment));
    }
    wrap.append(list);
    block.append(wrap);
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
  }
}
