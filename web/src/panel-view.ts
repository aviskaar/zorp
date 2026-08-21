/**
 * Drawing a review panel in the transcript.
 *
 * A panel is several agents at once, so it needs a different shape from a
 * turn: one block that fills in as reviewers report, rather than a stream
 * of lines. The block is created by the first `reviewer_started` and
 * closed by `panel_done`.
 *
 * **Everything here goes through `textContent`.** Every string this
 * module renders was written by a model that has been reading a document
 * it was handed, and a reviewer's `claim` is the most attacker-shaped
 * text in the product: material under review can contain anything, and a
 * reviewer quoting it back is the shortest path from a file to the page.
 * There is no `innerHTML` in this file and there must never be one.
 */

import type { Agreement, PanelDoneEvent, PanelFinding, Severity } from "./api.ts";

/** Severity ordered worst first, for display. */
const SEVERITY_RANK: Record<Severity, number> = {
  blocking: 0,
  concern: 1,
  note: 2,
};

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
 * One reviewer's row, and the state it can be in.
 *
 * `running` is not a spinner on the whole panel. A five reviewer panel
 * where four have reported and one is still going should look like that,
 * not like a panel that has done nothing.
 */
type RowState = "running" | "finished" | "failed";

const STATE_LABEL: Record<RowState, string> = {
  running: "reviewing…",
  finished: "reported",
  failed: "did not report",
};

export class PanelView {
  private block: HTMLElement | null = null;
  private list: HTMLElement | null = null;
  private readonly rows = new Map<string, HTMLElement>();

  // Written out rather than declared as constructor parameter
  // properties: those emit code, and the test runner strips types
  // without compiling. `streamed-message.ts` carries the same note for
  // the same reason.
  private readonly doc: Document;
  private readonly transcript: HTMLElement;

  constructor(doc: Document, transcript: HTMLElement) {
    this.doc = doc;
    this.transcript = transcript;
  }

  /** Whether a panel block is open. */
  get isOpen(): boolean {
    return this.block !== null;
  }

  private ensureBlock(): HTMLElement {
    if (this.block) {
      return this.block;
    }
    const block = el(this.doc, "div", "card card-panel");
    const head = el(this.doc, "div", "card-head");
    head.append(text(this.doc, "span", "card-title", "Review panel"));
    const list = el(this.doc, "div", "panel-reviewers");
    block.append(head, list);
    this.transcript.append(block);
    this.block = block;
    this.list = list;
    return block;
  }

  private setState(row: HTMLElement, state: RowState): void {
    row.dataset.state = state;
    const status = row.querySelector(".panel-reviewer-status");
    if (status) {
      status.textContent = STATE_LABEL[state];
    }
  }

  /** A reviewer started. Opens the block if this is the first. */
  start(lens: string): void {
    this.ensureBlock();
    if (this.rows.has(lens)) {
      return;
    }
    const row = el(this.doc, "div", "panel-reviewer");
    row.append(
      text(this.doc, "span", "panel-reviewer-name", lens),
      text(this.doc, "span", "panel-reviewer-status", STATE_LABEL.running),
    );
    const findings = el(this.doc, "ul", "panel-findings");
    findings.hidden = true;
    row.append(findings);
    this.setState(row, "running");
    this.rows.set(lens, row);
    this.list?.append(row);
  }

  /**
   * A reviewer reported.
   *
   * Findings are shown worst first. A reviewer that found nothing says so
   * rather than rendering an empty list: "no findings" is a result, and a
   * blank row reads like a reviewer that is still going.
   */
  finish(lens: string, findings: PanelFinding[]): void {
    // A verdict for a reviewer we never saw start still has to appear.
    // Dropping it would be the one failure this view must not have: a
    // reviewer that ran and is not on the page.
    this.start(lens);
    const row = this.rows.get(lens);
    if (!row) {
      return;
    }
    this.setState(row, "finished");
    const list = row.querySelector(".panel-findings");
    if (!(list instanceof this.doc.defaultView!.HTMLElement)) {
      return;
    }
    list.hidden = false;
    if (findings.length === 0) {
      const item = el(this.doc, "li", "panel-finding panel-finding-none");
      item.append(text(this.doc, "span", "panel-finding-claim", "No findings."));
      list.append(item);
      return;
    }
    const sorted = [...findings].sort(
      (a, b) => SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity],
    );
    for (const finding of sorted) {
      const item = el(this.doc, "li", "panel-finding");
      item.dataset.severity = finding.severity;
      item.append(
        text(this.doc, "span", "panel-finding-severity", finding.severity),
        text(this.doc, "span", "panel-finding-claim", finding.claim),
        text(this.doc, "span", "panel-finding-locus", finding.locus),
      );
      list.append(item);
    }
  }

  /**
   * A reviewer did not report.
   *
   * Drawn, never dropped. A panel of five where two failed is not a panel
   * of three, and a view that only draws successes draws it as one.
   */
  fail(lens: string, why: string): void {
    this.start(lens);
    const row = this.rows.get(lens);
    if (!row) {
      return;
    }
    this.setState(row, "failed");
    const list = row.querySelector(".panel-findings");
    if (list instanceof this.doc.defaultView!.HTMLElement) {
      list.hidden = false;
      const item = el(this.doc, "li", "panel-finding panel-finding-failed");
      item.append(text(this.doc, "span", "panel-finding-claim", why));
      list.append(item);
    }
  }

  /**
   * The panel closed.
   *
   * The summary leads with completeness, because that is the number a
   * reader needs before they read any other one. Two of two reviewers
   * agreeing is a weaker claim than two of five, and the corroboration
   * counts underneath cannot tell those apart on their own.
   */
  done(event: PanelDoneEvent): void {
    const block = this.ensureBlock();
    const summary = el(this.doc, "div", "panel-summary");
    summary.dataset.complete = String(event.complete);
    summary.append(
      text(this.doc, "p", "panel-summary-count", completenessLine(event)),
    );

    if (event.agreements.length > 0) {
      summary.append(
        text(
          this.doc,
          "p",
          "panel-summary-heading",
          "Raised independently by more than one reviewer",
        ),
      );
      const list = el(this.doc, "ul", "panel-agreements");
      for (const agreement of event.agreements) {
        list.append(this.agreementItem(agreement));
      }
      summary.append(list);
    }
    block.append(summary);
    this.close();
  }

  private agreementItem(agreement: Agreement): HTMLElement {
    const item = el(this.doc, "li", "panel-agreement");
    item.dataset.severity = agreement.highest;
    item.append(
      text(this.doc, "span", "panel-agreement-locus", agreement.locus),
      text(
        this.doc,
        "span",
        "panel-agreement-count",
        `${agreement.lenses.length} reviewers`,
      ),
      text(this.doc, "span", "panel-agreement-lenses", agreement.lenses.join(", ")),
    );
    return item;
  }

  /**
   * Forget the open block without closing it.
   *
   * Called when a panel ends for a reason other than `panel_done`: an
   * error, or a stop. The block that is already on the page stays where
   * it is, with its reviewers in whatever state they reached, and the
   * next panel starts a new one rather than appending to a stale block.
   */
  close(): void {
    this.block = null;
    this.list = null;
    this.rows.clear();
  }
}

/**
 * The one sentence a reader has to see.
 *
 * Says the shortfall in words rather than leaving it to be worked out
 * from two numbers. "3 of 5 reviewers reported" is arithmetic the reader
 * has to do; "2 did not report" is the fact.
 */
export function completenessLine(event: PanelDoneEvent): string {
  const { verdicts, lenses_requested: requested } = event;
  if (event.complete) {
    return `All ${requested} reviewers reported on ${event.target}.`;
  }
  const missing = Math.max(0, requested - verdicts);
  if (missing === 0) {
    // Requested and reported agree but the panel is still not complete,
    // which is what a stop looks like. Saying "all reported" here would
    // be the one wrong thing to say.
    return `Panel on ${event.target} did not finish. Read the reviewers below as partial.`;
  }
  const plural = missing === 1 ? "reviewer" : "reviewers";
  return `${verdicts} of ${requested} reviewers reported on ${event.target}. ${missing} ${plural} did not, so treat any agreement below as covering ${verdicts} views and not ${requested}.`;
}
