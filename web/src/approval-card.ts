/**
 * The approval card: the security boundary of the product, drawn.
 *
 * The agent is parked until one of the three buttons is pressed, and nothing
 * here presses one. While it waits the card is open and stays open, because
 * a person must see what they are deciding on. Once settled it folds to its
 * head line, the outcome and the tool, and a click on the head shows the
 * arguments and the note again.
 *
 * Every string lands through `textContent`. The tool name and its arguments
 * are the model's request, and a card that turned any of that into markup
 * would be a cross-site scripting hole at exactly the place a person reads
 * most carefully.
 *
 * Its own module, like `approval-mode.ts`, because `main.ts` runs the whole
 * app on import and cannot be loaded from a test.
 */

export type ApprovalOutcome = "allowed" | "denied" | "expired" | "stopped";

/**
 * How a settled card describes itself.
 *
 * Records rather than nested conditionals, so a new outcome is a compile
 * error here instead of quietly falling into whichever branch was last. The
 * distinction they carry is who decided: "expired" means nobody did and the
 * server denied it after five minutes, "stopped" means the reader ended the
 * turn while it was on screen. Both deny the tool. Calling the second one
 * expired would be a small lie told at exactly the moment the reader is
 * checking what their button press did.
 */
export const APPROVAL_TITLES: Record<ApprovalOutcome, string> = {
  allowed: "Tool allowed",
  denied: "Tool denied",
  expired: "Approval expired",
  stopped: "Turn stopped",
};

export const APPROVAL_NOTES: Record<ApprovalOutcome, string> = {
  allowed: "You allowed this, so the agent carried on.",
  denied: "You denied this. The tool did not run.",
  expired: "The turn ended before this was answered, so the server denied it.",
  stopped: "You stopped the turn while this was waiting, so the tool did not run.",
};

export interface ApprovalCard {
  root: HTMLDetailsElement;
  allow: HTMLButtonElement;
  deny: HTMLButtonElement;
  allowAll: HTMLButtonElement;
  /** Turn the three buttons on or off together. */
  enable(on: boolean): void;
  /** The line under the buttons: progress, or why the last click failed. */
  note(text: string): void;
  /** Decided. The buttons go, the head says the outcome, the card folds. */
  settle(outcome: ApprovalOutcome): void;
}

/** Format tool arguments as indented JSON when they parse, verbatim otherwise. */
export function prettyArguments(args: string): string {
  const trimmed = (args ?? "").trim();
  if (!trimmed) {
    return "(no arguments)";
  }
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return trimmed;
  }
}

/** `icon` is the shield drawn by the page, handed in so this needs no SVG of its own. */
export function approvalCard(doc: Document, tool: string, args: string, icon?: Node): ApprovalCard {
  const el = (tag: string, className = ""): HTMLElement => {
    const node = doc.createElement(tag);
    if (className) {
      node.className = className;
    }
    return node;
  };
  const text = (tag: string, className: string, value: string): HTMLElement => {
    const node = el(tag, className);
    node.textContent = value;
    return node;
  };
  const button = (className: string, value: string): HTMLButtonElement => {
    const node = el("button", `btn ${className}`) as HTMLButtonElement;
    node.type = "button";
    node.textContent = value;
    return node;
  };

  const root = el("details", "card card-approval") as HTMLDetailsElement;
  root.open = true;
  let pending = true;

  const head = el("summary", "card-head");
  const title = text("span", "card-title", "Approval required");
  const tag = text("span", "card-tag", "waiting");
  head.append(...(icon ? [icon] : []), title, text("code", "tool-name", tool), tag);
  // Open until decided: a click on the head would fold away the thing being
  // decided on, so it is refused while the buttons are live.
  head.addEventListener("click", (event) => {
    if (pending) {
      event.preventDefault();
    }
  });

  const lead = el("p", "card-body");
  lead.textContent =
    "The agent wants to use a tool that can change this machine. It is stopped until you decide.";

  const argsField = el("div", "field field-block");
  argsField.append(text("span", "field-label", "arguments"));
  const argsBlock = el("pre", "card-args");
  argsBlock.append(text("code", "", prettyArguments(args)));
  argsField.append(argsBlock);

  const allow = button("btn-allow", "Allow");
  const deny = button("btn-deny", "Deny");
  // The third choice, offered here because this is the moment a long run
  // becomes a click per step. It is spelled out rather than abbreviated, it
  // is not the primary button, and taking it turns the toolbar pill red
  // until the mode is turned off. Every call it then lets through is still
  // reviewed before it runs; see `zorp-web/src/tool_safety.rs`.
  const allowAll = button("btn-allow-all", "Allow all for this chat");
  allowAll.title = "Stop asking for the rest of this chat. The hard denylist still applies.";
  const actions = el("div", "card-actions");
  actions.append(allow, deny, allowAll);

  const note = el("p", "card-note");
  root.append(head, lead, argsField, actions, note);

  const enable = (on: boolean): void => {
    for (const each of [allow, deny, allowAll]) {
      each.disabled = !on;
    }
  };

  return {
    root,
    allow,
    deny,
    allowAll,
    enable,
    note(value) {
      note.textContent = value;
    },
    settle(outcome) {
      pending = false;
      enable(false);
      actions.remove();
      lead.remove();
      root.classList.add("is-settled", `is-${outcome}`);
      tag.textContent = outcome;
      title.textContent = APPROVAL_TITLES[outcome];
      note.textContent = APPROVAL_NOTES[outcome];
      head.title = "Show the arguments";
      root.open = false;
    },
  };
}
