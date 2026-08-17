/**
 * The zorp chat UI.
 *
 * A message list, a composer, a session sidebar, and the two things that make
 * this an agent interface rather than a chat toy: tool activity streamed inline
 * as it happens, and an approval card that stops the agent until a human
 * answers. Nothing here approves anything on its own.
 */

import {
  ApiError,
  TurnBusyError,
  approve,
  getSession,
  listSessions,
  newSession,
  sendTurn,
  streamEvents,
  type EventStream,
  type Message,
  type SessionSummary,
  type StreamStatus,
  type ZorpEvent,
} from "./api";

// The spinner frames the CLI uses, so the two surfaces look related.
const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// A small slice of the CLI's verb list. One is chosen per turn.
const WORKING_VERBS = [
  "Cogitating",
  "Percolating",
  "Reticulating",
  "Simmering",
  "Spelunking",
  "Synthesizing",
  "Tinkering",
  "Wrangling",
];

// How long to wait for the event stream to connect before sending the message
// anyway. The server backfills from its buffer, so this is belt and braces.
const STREAM_OPEN_TIMEOUT_MS = 1500;

// How long to collect the server's replay when joining an existing session
// before deciding whether it describes a finished turn or a live one.
const CATCH_UP_MS = 400;

// Autoscroll only when the reader is already at the bottom.
const STICK_TO_BOTTOM_PX = 120;

interface Elements {
  app: HTMLElement;
  scrim: HTMLElement;
  sessionList: HTMLElement;
  newChat: HTMLButtonElement;
  menu: HTMLButtonElement;
  sidebarClose: HTMLButtonElement;
  title: HTMLElement;
  status: HTMLElement;
  statusText: HTMLElement;
  scroller: HTMLElement;
  transcript: HTMLElement;
  working: HTMLElement;
  workingSpinner: HTMLElement;
  workingVerb: HTMLElement;
  jump: HTMLButtonElement;
  composer: HTMLFormElement;
  input: HTMLTextAreaElement;
  send: HTMLButtonElement;
}

type ApprovalOutcome = "allowed" | "denied" | "expired";

interface PendingApproval {
  settle(outcome: ApprovalOutcome): void;
}

const dom = collectElements();

let sessionId: string | null = null;
let stream: EventStream | null = null;
let streamSessionId: string | null = null;
let catchUp: ZorpEvent[] | null = null;
let turnRunning = false;
let workingDepth = 0;
let lastSeq = -1;
let sessions: SessionSummary[] = [];
let activityGroup: HTMLElement | null = null;
let spinnerTimer: number | null = null;
let spinnerFrame = 0;
const pendingApprovals = new Map<string, PendingApproval>();

start();

function start(): void {
  wireComposer();
  wireSidebar();
  wireScroller();
  showEmptyState();
  setStatus("idle", "idle");
  void refreshSessions();
  dom.input.focus();
}

/* ------------------------------------------------------------------ */
/* wiring                                                              */
/* ------------------------------------------------------------------ */

function collectElements(): Elements {
  const byId = <T extends HTMLElement>(id: string): T => {
    const node = document.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node as T;
  };
  return {
    app: byId("app"),
    scrim: byId("scrim"),
    sessionList: byId("session-list"),
    newChat: byId<HTMLButtonElement>("new-chat"),
    menu: byId<HTMLButtonElement>("menu"),
    sidebarClose: byId<HTMLButtonElement>("sidebar-close"),
    title: byId("session-title"),
    status: byId("status"),
    statusText: byId("status-text"),
    scroller: byId("scroller"),
    transcript: byId("transcript"),
    working: byId("working"),
    workingSpinner: byId("working-spinner"),
    workingVerb: byId("working-verb"),
    jump: byId<HTMLButtonElement>("jump"),
    composer: byId<HTMLFormElement>("composer"),
    input: byId<HTMLTextAreaElement>("input"),
    send: byId<HTMLButtonElement>("send"),
  };
}

function wireComposer(): void {
  dom.composer.addEventListener("submit", (event) => {
    event.preventDefault();
    void submitMessage();
  });

  dom.input.addEventListener("keydown", (event) => {
    // Enter sends. Shift+Enter is a newline. An open IME composition owns the
    // key and must be left alone.
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) {
      return;
    }
    event.preventDefault();
    void submitMessage();
  });

  dom.input.addEventListener("input", autoGrowInput);
  autoGrowInput();
}

function wireSidebar(): void {
  dom.newChat.addEventListener("click", () => {
    startNewChat();
    closeSidebar();
    dom.input.focus();
  });
  dom.menu.addEventListener("click", openSidebar);
  dom.sidebarClose.addEventListener("click", closeSidebar);
  dom.scrim.addEventListener("click", closeSidebar);
}

function wireScroller(): void {
  dom.scroller.addEventListener("scroll", () => {
    dom.jump.hidden = isNearBottom();
  });
  dom.jump.addEventListener("click", () => {
    dom.scroller.scrollTop = dom.scroller.scrollHeight;
    dom.jump.hidden = true;
  });
}

/* ------------------------------------------------------------------ */
/* sending a turn                                                      */
/* ------------------------------------------------------------------ */

async function submitMessage(): Promise<void> {
  const message = dom.input.value.trim();
  if (!message || turnRunning) {
    return;
  }

  dom.input.value = "";
  autoGrowInput();
  clearEmptyState();
  appendMessage("user", message);
  scrollToBottomIfFollowing(true);

  setTurnRunning(true);

  try {
    if (!sessionId) {
      sessionId = await newSession();
      setTitle("New chat");
      await refreshSessions();
      markActiveSession();
    }
    await ensureStream(sessionId);
    // A server that numbers events per turn rather than per session starts
    // over here, so the replay guard has to start over with it.
    lastSeq = -1;
    await sendTurn(sessionId, message);
  } catch (error) {
    setTurnRunning(false);
    if (error instanceof TurnBusyError) {
      appendError("A turn is already running on this session. Wait for it to finish.");
    } else {
      appendError(describeError(error));
    }
    scrollToBottomIfFollowing(true);
  }
}

/**
 * Open the event stream for a session, reusing the one already open for it.
 *
 * With `joining` set, the server's replay is buffered rather than rendered.
 * That replay may describe a turn that already finished, in which case the
 * stored transcript already covers it and rendering it again would double the
 * conversation. See `applyCatchUp`.
 */
async function ensureStream(id: string, joining = false): Promise<void> {
  if (stream && streamSessionId === id) {
    return;
  }
  closeStream();
  lastSeq = -1;
  catchUp = joining ? [] : null;
  const opened = streamEvents(id, handleEvent, handleStreamStatus);
  stream = opened;
  streamSessionId = id;
  await Promise.race([opened.opened, delay(STREAM_OPEN_TIMEOUT_MS)]);
  if (joining) {
    await delay(CATCH_UP_MS);
    if (streamSessionId === id) {
      applyCatchUp();
    }
  }
}

/**
 * Decide what the buffered replay was. Everything up to and including the last
 * `done` belongs to a turn that already finished, so the stored transcript has
 * it. Anything after that is a turn still in flight and worth joining.
 */
function applyCatchUp(): void {
  const buffered = catchUp ?? [];
  catchUp = null;

  let start = 0;
  for (let index = buffered.length - 1; index >= 0; index -= 1) {
    if (buffered[index].type === "done") {
      start = index + 1;
      break;
    }
  }

  if (start >= buffered.length) {
    return;
  }
  setTurnRunning(true);
  for (const event of buffered.slice(start)) {
    applyEvent(event);
  }
  dom.scroller.scrollTop = dom.scroller.scrollHeight;
}

function handleStreamStatus(status: StreamStatus): void {
  if (status === "open") {
    setStatus("live", turnRunning ? "running" : "connected");
    return;
  }
  if (status === "connecting") {
    setStatus("wait", "connecting");
    return;
  }
  if (status === "reconnecting") {
    setStatus("wait", "reconnecting");
    return;
  }
  setStatus("idle", "disconnected");
}

/* ------------------------------------------------------------------ */
/* event handling                                                      */
/* ------------------------------------------------------------------ */

function handleEvent(event: ZorpEvent): void {
  // A reconnect replays from the last id the browser saw. Dropping anything at
  // or below the high water mark keeps a generous replay from doubling the
  // transcript. lastSeq resets when a turn starts, so a server that numbers
  // per turn rather than per session still works.
  if (typeof event.seq === "number" && event.seq <= lastSeq) {
    return;
  }
  if (typeof event.seq === "number") {
    lastSeq = event.seq;
  }

  if (catchUp) {
    catchUp.push(event);
    return;
  }
  applyEvent(event);
}

function applyEvent(event: ZorpEvent): void {
  const following = isNearBottom();

  switch (event.type) {
    case "working":
      workingDepth += 1;
      updateWorking();
      break;

    case "working_done":
      workingDepth = Math.max(0, workingDepth - 1);
      updateWorking();
      break;

    case "tool":
      appendActivity(activityLine(event.name, event.summary));
      break;

    case "verify":
      appendActivity(verifyLine(event.command, event.passed));
      break;

    case "notice":
      appendActivity(noticeLine(event.text));
      break;

    case "assistant":
      appendMessage("assistant", event.text);
      break;

    case "approval_request":
      appendApproval(event.id, event.tool, event.arguments);
      break;

    case "error":
      appendError(event.message);
      break;

    case "done":
      finishTurn();
      break;
  }

  scrollToBottomIfFollowing(following);
}

function finishTurn(): void {
  setTurnRunning(false);
  workingDepth = 0;
  updateWorking();
  expirePendingApprovals();
  void refreshSessions();
}

function setTurnRunning(running: boolean): void {
  turnRunning = running;
  dom.send.disabled = running;
  dom.composer.classList.toggle("is-busy", running);
  if (running) {
    dom.workingVerb.textContent = pick(WORKING_VERBS);
    startSpinner();
    setStatus("live", "running");
  } else {
    stopSpinner();
    if (dom.status.dataset.state === "live") {
      setStatus("live", "connected");
    }
  }
  updateWorking();
}

function updateWorking(): void {
  dom.working.hidden = !(turnRunning || workingDepth > 0);
}

function startSpinner(): void {
  if (spinnerTimer !== null || prefersReducedMotion()) {
    dom.workingSpinner.textContent = "●";
    return;
  }
  spinnerTimer = window.setInterval(() => {
    spinnerFrame = (spinnerFrame + 1) % SPINNER_FRAMES.length;
    dom.workingSpinner.textContent = SPINNER_FRAMES[spinnerFrame];
  }, 90);
}

function stopSpinner(): void {
  if (spinnerTimer !== null) {
    window.clearInterval(spinnerTimer);
    spinnerTimer = null;
  }
}

/* ------------------------------------------------------------------ */
/* transcript rendering                                                */
/* ------------------------------------------------------------------ */

function appendMessage(role: "user" | "assistant", text: string): void {
  activityGroup = null;
  const row = el("article", `msg msg-${role}`);
  const label = el("div", "msg-role");
  label.textContent = role === "user" ? "You" : "zorp";
  const body = el("div", "msg-body");
  renderRichText(body, text);
  row.append(label, body);
  dom.transcript.append(row);
}

/** The CLI's shape: a bullet, the tool name, then the summary. */
function activityLine(name: string, summary: string): HTMLElement {
  const line = el("div", "activity-line");
  line.append(bullet(), mono("activity-name", name), mono("activity-summary", summary));
  return line;
}

function verifyLine(command: string, passed: boolean): HTMLElement {
  const line = el("div", "activity-line");
  const verdict = mono(passed ? "activity-pass" : "activity-fail", passed ? "passed" : "failed");
  line.append(bullet(), mono("activity-name", "verify"), mono("activity-summary", command), verdict);
  return line;
}

function noticeLine(text: string): HTMLElement {
  const line = el("div", "activity-line activity-notice");
  line.append(mono("activity-summary", text));
  return line;
}

/** Consecutive activity lines share one group so they get a single left rule. */
function appendActivity(line: HTMLElement): void {
  if (!activityGroup) {
    activityGroup = el("div", "activity");
    dom.transcript.append(activityGroup);
  }
  activityGroup.append(line);
}

function appendError(message: string): void {
  activityGroup = null;
  const card = el("div", "card card-error");
  const head = el("div", "card-head");
  head.append(glyph("alert"), textNode("span", "card-title", "Something went wrong"));
  const body = el("p", "card-body");
  body.textContent = message;
  card.append(head, body);
  dom.transcript.append(card);
}

/**
 * The security boundary of the product. The agent is parked until one of these
 * buttons is pressed, and nothing here presses one automatically.
 */
function appendApproval(id: string, tool: string, args: string): void {
  activityGroup = null;

  const card = el("div", "card card-approval");
  const head = el("div", "card-head");
  const title = textNode("span", "card-title", "Approval required");
  const tag = textNode("span", "card-tag", "waiting");
  head.append(glyph("shield"), title, tag);

  const lead = el("p", "card-body");
  lead.textContent =
    "The agent wants to use a tool that can change this machine. It is stopped until you decide.";

  const toolField = el("div", "field");
  toolField.append(textNode("span", "field-label", "tool"), textNode("code", "tool-name", tool));

  const argsField = el("div", "field field-block");
  argsField.append(textNode("span", "field-label", "arguments"));
  const argsBlock = el("pre", "card-args");
  const argsCode = el("code");
  argsCode.textContent = prettyArguments(args);
  argsBlock.append(argsCode);
  argsField.append(argsBlock);

  const actions = el("div", "card-actions");
  const allowButton = el("button", "btn btn-allow") as HTMLButtonElement;
  allowButton.type = "button";
  allowButton.textContent = "Allow";
  const denyButton = el("button", "btn btn-deny") as HTMLButtonElement;
  denyButton.type = "button";
  denyButton.textContent = "Deny";
  actions.append(allowButton, denyButton);

  const note = el("p", "card-note");
  card.append(head, lead, toolField, argsField, actions, note);
  dom.transcript.append(card);

  const settle = (outcome: ApprovalOutcome): void => {
    allowButton.disabled = true;
    denyButton.disabled = true;
    actions.remove();
    lead.remove();
    card.classList.add(`is-${outcome}`);
    tag.textContent = outcome;
    title.textContent =
      outcome === "allowed" ? "Tool allowed" : outcome === "denied" ? "Tool denied" : "Approval expired";
    note.textContent =
      outcome === "allowed"
        ? "You allowed this, so the agent carried on."
        : outcome === "denied"
          ? "You denied this. The tool did not run."
          : "The turn ended before this was answered, so the server denied it.";
    pendingApprovals.delete(id);
  };

  const decide = async (allow: boolean): Promise<void> => {
    if (!sessionId) {
      return;
    }
    allowButton.disabled = true;
    denyButton.disabled = true;
    note.textContent = "Sending your decision…";
    try {
      await approve(sessionId, id, allow);
      settle(allow ? "allowed" : "denied");
    } catch (error) {
      allowButton.disabled = false;
      denyButton.disabled = false;
      note.textContent = `Could not send the decision: ${describeError(error)}`;
    }
  };

  allowButton.addEventListener("click", () => void decide(true));
  denyButton.addEventListener("click", () => void decide(false));

  pendingApprovals.set(id, { settle });
}

function expirePendingApprovals(): void {
  for (const pending of Array.from(pendingApprovals.values())) {
    pending.settle("expired");
  }
  pendingApprovals.clear();
}

/** Format tool arguments as indented JSON when they parse, verbatim otherwise. */
function prettyArguments(args: string): string {
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

/**
 * Plain text with fenced code blocks and inline code pulled out. Everything
 * lands on the page through textContent, so nothing the model or a tool emits
 * can become markup.
 */
function renderRichText(target: HTMLElement, text: string): void {
  const parts = (text ?? "").split("```");
  parts.forEach((part, index) => {
    if (index % 2 === 1) {
      const newline = part.indexOf("\n");
      const first = newline === -1 ? part : part.slice(0, newline);
      const isLanguageTag = newline !== -1 && /^[A-Za-z0-9_+.-]*$/.test(first.trim());
      const body = isLanguageTag ? part.slice(newline + 1) : part;
      const block = el("pre", "code-block");
      const code = el("code");
      code.textContent = body.replace(/\n+$/, "");
      if (isLanguageTag && first.trim()) {
        block.dataset.lang = first.trim();
      }
      block.append(code);
      target.append(block);
      return;
    }
    const prose = parts.length > 1 ? part.replace(/^\n+|\n+$/g, "") : part;
    if (prose) {
      target.append(paragraph(prose));
    }
  });
  if (!target.childNodes.length) {
    target.append(paragraph(""));
  }
}

function paragraph(text: string): HTMLElement {
  const node = el("p", "para");
  text.split("`").forEach((segment, index) => {
    if (!segment) {
      return;
    }
    if (index % 2 === 1) {
      node.append(textNode("code", "inline-code", segment));
    } else {
      node.append(document.createTextNode(segment));
    }
  });
  return node;
}

/* ------------------------------------------------------------------ */
/* sessions                                                            */
/* ------------------------------------------------------------------ */

async function refreshSessions(): Promise<void> {
  try {
    sessions = await listSessions();
  } catch {
    // The sidebar is not worth an error card. The status pill already shows a
    // server that cannot be reached.
    sessions = [];
  }
  renderSessions();

  // The server names a session from its first message, so the heading catches
  // up once the list has been read back.
  const active = sessions.find((session) => session.id === sessionId);
  if (active?.title) {
    setTitle(active.title);
  }
}

function renderSessions(): void {
  dom.sessionList.replaceChildren();

  if (!sessions.length) {
    const empty = el("li", "session-empty");
    empty.textContent = "No sessions yet.";
    dom.sessionList.append(empty);
    return;
  }

  for (const session of sessions) {
    const item = el("li", "session-item");
    const button = el("button", "session-button") as HTMLButtonElement;
    button.type = "button";
    button.dataset.id = session.id;
    button.append(
      textNode("span", "session-title", session.title || "Untitled session"),
      textNode("span", "session-time", relativeTime(session.updated_at)),
    );
    if (session.id === sessionId) {
      button.classList.add("is-active");
      button.setAttribute("aria-current", "true");
    }
    button.addEventListener("click", () => {
      void openSession(session);
      closeSidebar();
    });
    item.append(button);
    dom.sessionList.append(item);
  }
}

function markActiveSession(): void {
  for (const button of Array.from(dom.sessionList.querySelectorAll<HTMLButtonElement>(".session-button"))) {
    const active = button.dataset.id === sessionId;
    button.classList.toggle("is-active", active);
    if (active) {
      button.setAttribute("aria-current", "true");
    } else {
      button.removeAttribute("aria-current");
    }
  }
}

async function openSession(session: SessionSummary): Promise<void> {
  if (session.id === sessionId) {
    return;
  }
  closeStream();
  resetTranscript();
  sessionId = session.id;
  setTitle(session.title || "Untitled session");
  markActiveSession();

  try {
    const transcript = await getSession(session.id);
    if (sessionId !== session.id) {
      return;
    }
    if (!transcript.messages.length) {
      showEmptyState();
    } else {
      transcript.messages.forEach((message: Message) => {
        appendMessage(message.role === "user" ? "user" : "assistant", message.content);
      });
    }
  } catch (error) {
    appendError(`Could not load this session: ${describeError(error)}`);
  }

  dom.scroller.scrollTop = dom.scroller.scrollHeight;
  dom.input.focus();
  await ensureStream(session.id, true);
}

function startNewChat(): void {
  closeStream();
  resetTranscript();
  sessionId = null;
  setTitle("New chat");
  markActiveSession();
  showEmptyState();
  setStatus("idle", "idle");
}

function closeStream(): void {
  if (stream) {
    stream.close();
  }
  stream = null;
  streamSessionId = null;
  catchUp = null;
  lastSeq = -1;
}

function resetTranscript(): void {
  expirePendingApprovals();
  pendingApprovals.clear();
  dom.transcript.replaceChildren();
  activityGroup = null;
  setTurnRunning(false);
  workingDepth = 0;
  updateWorking();
  dom.jump.hidden = true;
}

/* ------------------------------------------------------------------ */
/* empty state, status, small helpers                                  */
/* ------------------------------------------------------------------ */

function showEmptyState(): void {
  clearEmptyState();
  const panel = el("div", "empty");
  panel.append(
    textNode("div", "empty-mark", "zorp"),
    textNode("p", "empty-lead", "Ask for something in this workspace and watch it happen."),
  );

  const list = el("ul", "empty-list");
  for (const line of [
    "Every tool the agent runs shows up here as it runs.",
    "Anything that changes your machine stops for your approval first.",
    "The agent works on the directory the server was started in.",
  ]) {
    list.append(textNode("li", "", line));
  }
  panel.append(list);
  dom.transcript.append(panel);
}

function clearEmptyState(): void {
  dom.transcript.querySelector(".empty")?.remove();
}

function setTitle(title: string): void {
  dom.title.textContent = title;
}

function setStatus(state: "idle" | "wait" | "live", text: string): void {
  dom.status.dataset.state = state;
  dom.statusText.textContent = text;
}

function openSidebar(): void {
  dom.app.classList.add("sidebar-open");
}

function closeSidebar(): void {
  dom.app.classList.remove("sidebar-open");
}

function isNearBottom(): boolean {
  const distance = dom.scroller.scrollHeight - dom.scroller.scrollTop - dom.scroller.clientHeight;
  return distance <= STICK_TO_BOTTOM_PX;
}

function scrollToBottomIfFollowing(following: boolean): void {
  if (following) {
    dom.scroller.scrollTop = dom.scroller.scrollHeight;
    dom.jump.hidden = true;
  } else {
    dom.jump.hidden = false;
  }
}

function autoGrowInput(): void {
  dom.input.style.height = "auto";
  dom.input.style.height = `${Math.min(dom.input.scrollHeight, 220)}px`;
}

function describeError(error: unknown): string {
  if (error instanceof ApiError) {
    return error.status ? `${error.message} (HTTP ${error.status})` : error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function relativeTime(value: string): string {
  const then = Date.parse(value);
  if (Number.isNaN(then)) {
    return value ?? "";
  }
  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 60) {
    return "just now";
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.floor(hours / 24);
  if (days < 7) {
    return `${days}d ago`;
  }
  return new Date(then).toLocaleDateString();
}

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function pick<T>(values: readonly T[]): T {
  return values[Math.floor(Math.random() * values.length)];
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function el(tag: string, className = ""): HTMLElement {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  return node;
}

function textNode(tag: string, className: string, text: string): HTMLElement {
  const node = el(tag, className);
  node.textContent = text;
  return node;
}

function mono(className: string, text: string): HTMLElement {
  return textNode("span", className, text);
}

function bullet(): HTMLElement {
  return mono("activity-bullet", "●");
}

/** Two inline icons, drawn rather than pulled from an icon font. */
function glyph(kind: "shield" | "alert"): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "glyph");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");

  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute(
    "d",
    kind === "shield"
      ? "M12 3l7 3v5.5c0 4.2-2.9 7.9-7 9.5-4.1-1.6-7-5.3-7-9.5V6l7-3z"
      : "M12 4.5l8.5 15h-17l8.5-15z",
  );
  svg.append(path);

  const mark = document.createElementNS("http://www.w3.org/2000/svg", "path");
  mark.setAttribute("d", kind === "shield" ? "M12 8.5v4m0 3h.01" : "M12 10v3.5m0 3h.01");
  svg.append(mark);

  return svg;
}
