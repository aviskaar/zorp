/**
 * The zorp chat UI.
 *
 * A message list, a composer, a session sidebar, and the two things that make
 * this an agent interface rather than a chat toy: tool activity streamed inline
 * as it happens, and an approval card that stops the agent until a human
 * answers. Nothing here approves anything on its own.
 */

import { renderMarkdown } from "./markdown";
import { StreamedMessage, endsStreamedMessage } from "./streamed-message";
import {
  needsText,
  producedSince,
  showArtifact as showArtifactIn,
  type ArtifactStamp,
  type Pane,
} from "./artifact-view";
import {
  ApiError,
  TurnBusyError,
  approve,
  getSession,
  getSettings,
  listModels,
  listSessions,
  newSession,
  putSettings,
  sendTurn,
  streamEvents,
  testConnection,
  artifactUrl,
  listArtifacts,
  readArtifact,
  type Artifact,
  type EventStream,
  type Message,
  type Settings,
  type SettingsSource,
  type SettingsUpdate,
  type SessionSummary,
  type StreamStatus,
  type ZorpEvent,
  serverIsReachable,
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
  modelBtn: HTMLButtonElement;
  modelBtnLabel: HTMLElement;
  status: HTMLElement;
  statusText: HTMLElement;
  scroller: HTMLElement;
  transcript: HTMLElement;
  working: HTMLElement;
  workingSpinner: HTMLElement;
  workingVerb: HTMLElement;
  jump: HTMLButtonElement;
  composerWarning: HTMLElement;
  composerWarningSettings: HTMLButtonElement;
  composer: HTMLFormElement;
  input: HTMLTextAreaElement;
  send: HTMLButtonElement;
  settingsOverlay: HTMLElement;
  settingsClose: HTMLButtonElement;
  settingsForm: HTMLFormElement;
  settingsPreset: HTMLSelectElement;
  settingsBaseUrl: HTMLInputElement;
  settingsBaseUrlSource: HTMLElement;
  settingsModelSelect: HTMLSelectElement;
  settingsModelText: HTMLInputElement;
  settingsModelSource: HTMLElement;
  settingsModelHint: HTMLElement;
  settingsRefreshModels: HTMLButtonElement;
  settingsApiKeyField: HTMLElement;
  settingsApiKey: HTMLInputElement;
  settingsApiKeySource: HTMLElement;
  settingsTest: HTMLButtonElement;
  settingsSave: HTMLButtonElement;
  settingsResult: HTMLElement;
  artifactsBtn: HTMLButtonElement;
  artifactsBadge: HTMLElement;
  artifacts: HTMLElement;
  artifactsClose: HTMLButtonElement;
  artifactsRefresh: HTMLButtonElement;
  artifactList: HTMLElement;
  artifactEmpty: HTMLElement;
  artifactDoc: HTMLElement;
  artifactFrame: HTMLIFrameElement;
  artifactImage: HTMLImageElement;
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
/** The last settings the server reported. Null until the first successful
 * `GET /api/settings`, which happens once the server is known reachable. */
let currentSettings: Settings | null = null;

start();

function start(): void {
  wireComposer();
  wireSidebar();
  wireScroller();
  wireSettings();
  wireArtifacts();
  showEmptyState();
  setStatus("idle", "idle");
  void connectOrExplain();
}

/** Check there is a zorp server before offering a composer.
 *
 * Without this the UI looks ready when nothing is behind it: served as static
 * files the base URL is the page's own origin, so a file server answers, the
 * badge reads connected, and the first message comes back as that server's
 * HTML error page. Someone arriving from a link deserves to be told what to
 * run, not to type into a box that goes nowhere.
 */
async function connectOrExplain(): Promise<void> {
  if (await serverIsReachable()) {
    setStatus("idle", "idle");
    void refreshSessions();
    void refreshSettingsBadge();
    dom.input.focus();
    return;
  }
  setStatus("idle", "no server");
  showServerMissing();
}

function showServerMissing(): void {
  dom.transcript.replaceChildren();
  const card = el("div", "card card-error");
  const head = el("div", "card-head");
  head.append(glyph("alert"), textNode("span", "card-title", "No zorp server here"));

  const body = el("div", "card-body");
  const lead = el("p");
  lead.textContent =
    "This page is the interface. The agent itself runs on your machine, so " +
    "that nothing you do here leaves it. Start it and reload.";

  const code = el("pre", "card-code");
  code.textContent =
    "curl -fsSL https://raw.githubusercontent.com/aviskaar/zorp/main/install.sh | bash\n" +
    "zorp-web";

  const tail = el("p");
  tail.textContent =
    "By default the server listens on http://127.0.0.1:7777. If it runs " +
    "somewhere else, set window.ZORP_API_BASE in index.html.";

  body.append(lead, code, tail);
  card.append(head, body);
  dom.transcript.append(card);

  dom.input.disabled = true;
  dom.input.placeholder = "Start zorp-web on your machine, then reload";
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
    modelBtn: byId<HTMLButtonElement>("model-btn"),
    modelBtnLabel: byId("model-btn-label"),
    status: byId("status"),
    statusText: byId("status-text"),
    scroller: byId("scroller"),
    transcript: byId("transcript"),
    working: byId("working"),
    workingSpinner: byId("working-spinner"),
    workingVerb: byId("working-verb"),
    jump: byId<HTMLButtonElement>("jump"),
    composerWarning: byId("composer-warning"),
    composerWarningSettings: byId<HTMLButtonElement>("composer-warning-settings"),
    composer: byId<HTMLFormElement>("composer"),
    input: byId<HTMLTextAreaElement>("input"),
    send: byId<HTMLButtonElement>("send"),
    settingsOverlay: byId("settings-overlay"),
    settingsClose: byId<HTMLButtonElement>("settings-close"),
    settingsForm: byId<HTMLFormElement>("settings-form"),
    settingsPreset: byId<HTMLSelectElement>("settings-preset"),
    settingsBaseUrl: byId<HTMLInputElement>("settings-base-url"),
    settingsBaseUrlSource: byId("settings-base-url-source"),
    settingsModelSelect: byId<HTMLSelectElement>("settings-model"),
    settingsModelText: byId<HTMLInputElement>("settings-model-text"),
    settingsModelSource: byId("settings-model-source"),
    settingsModelHint: byId("settings-model-hint"),
    settingsRefreshModels: byId<HTMLButtonElement>("settings-refresh-models"),
    settingsApiKeyField: byId("settings-api-key-field"),
    settingsApiKey: byId<HTMLInputElement>("settings-api-key"),
    settingsApiKeySource: byId("settings-api-key-source"),
    settingsTest: byId<HTMLButtonElement>("settings-test"),
    settingsSave: byId<HTMLButtonElement>("settings-save"),
    artifactsBtn: byId<HTMLButtonElement>("artifacts-btn"),
    artifactsBadge: byId<HTMLElement>("artifacts-badge"),
    artifacts: byId<HTMLElement>("artifacts"),
    artifactsClose: byId<HTMLButtonElement>("artifacts-close"),
    artifactsRefresh: byId<HTMLButtonElement>("artifacts-refresh"),
    artifactList: byId<HTMLElement>("artifact-list"),
    artifactEmpty: byId<HTMLElement>("artifact-empty"),
    artifactDoc: byId<HTMLElement>("artifact-doc"),
    artifactFrame: byId<HTMLIFrameElement>("artifact-frame"),
    artifactImage: byId<HTMLImageElement>("artifact-image"),
    settingsResult: byId("settings-result"),
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

function wireSettings(): void {
  dom.modelBtn.addEventListener("click", () => void openSettings());
  dom.composerWarningSettings.addEventListener("click", () => void openSettings());
  dom.settingsClose.addEventListener("click", closeSettings);
  dom.settingsOverlay.addEventListener("click", (event) => {
    // Only a click on the backdrop itself closes it; clicks inside the
    // panel bubble up from far more useful targets than "close the dialog".
    if (event.target === dom.settingsOverlay) {
      closeSettings();
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !dom.settingsOverlay.hidden) {
      closeSettings();
    }
  });
  dom.settingsPreset.addEventListener("change", () => applyPreset(dom.settingsPreset.value));
  dom.settingsBaseUrl.addEventListener("change", () => void refreshModelOptions());
  dom.settingsRefreshModels.addEventListener("click", () => void refreshModelOptions());
  dom.settingsTest.addEventListener("click", () => void runConnectionTest());
  dom.settingsForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveSettings();
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
  // Before anything runs, so "what did this turn write" has a baseline. Not
  // awaited: a slow listing must not delay the message.
  void snapshotArtifacts();

  try {
    if (!sessionId) {
      sessionId = await newSession();
      setTitle("New chat");
      await refreshSessions();
      markActiveSession();
    }
    // The stream for this session is already open and stays open, so the
    // turn's events arrive on it. The replay guard deliberately keeps its high
    // water mark: seq climbs for the life of the session, and resetting it
    // here would let a reconnect mid-turn render the backlog twice.
    await ensureStream(sessionId);
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
 * There is one stream per session and it lives as long as the session is on
 * screen, which is what the server's long lived SSE response is for. Every
 * turn arrives on it, so nothing here reopens it between turns.
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
  // transcript. The mark only resets when a stream is opened from scratch,
  // because seq climbs for the whole session and never restarts per turn.
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

  // See endsStreamedMessage for which events close the message being
  // streamed and, more importantly, which ones must not.
  if (endsStreamedMessage(event.type)) {
    finishStream(null);
  }

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
      // A tool ran, so the workspace may have changed. The name and summary
      // are not read for a path: what got written is a question for the
      // directory, not for the tool that claims to have written it.
      void checkForProducedArtifacts();
      break;

    case "verify":
      appendActivity(verifyLine(event.command, event.passed));
      break;

    case "notice":
      appendActivity(noticeLine(event.text));
      break;

    case "assistant_delta":
      appendStreamDelta(event.text);
      break;

    case "assistant":
      finishStream(event.text);
      break;

    case "approval_request":
      appendApproval(event.id, event.tool, event.arguments);
      break;

    case "error":
      appendError(event.message);
      // A turn that failed is still a turn that ran, and the server sends
      // `error` instead of `done` rather than as well as it, so without this
      // a run that wrote three files and then lost the model would leave them
      // unmentioned.
      void checkForProducedArtifacts(true);
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
  // Forced past the poll interval: this is the last chance to notice what the
  // run wrote, and a file written in the final second is the interesting one.
  void checkForProducedArtifacts(true);
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

/* ---- streaming ---- */

/**
 * The assistant message currently being streamed.
 *
 * Fragments are a preview. The server states the finished answer exactly
 * once, in an `assistant` event, and that is what ends up on the page.
 */
const streamed = new StreamedMessage(dom.transcript, renderMarkdown);

function appendStreamDelta(chunk: string): void {
  if (!streamed.open) activityGroup = null;
  streamed.append(chunk);
}

function finishStream(authoritative: string | null): void {
  const handled = streamed.finish(authoritative);
  if (!handled && authoritative !== null) {
    appendMessage("assistant", authoritative);
  }
}

function appendMessage(role: "user" | "assistant", text: string): void {
  activityGroup = null;
  const row = el("article", `msg msg-${role}`);
  const label = el("div", "msg-role");
  label.textContent = role === "user" ? "You" : "zorp";
  const body = el("div", "msg-body");
  // What the user typed is shown as they typed it. Running their own message
  // through a markdown renderer would mean a question containing `#` or `*`
  // came back looking like something they did not write.
  if (role === "user") {
    renderRichText(body, text);
  } else {
    renderMarkdown(body, text);
  }
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
  // Servers that are not zorp answer with HTML. Pasting a whole error page
  // into the transcript tells a reader nothing and looks broken.
  body.textContent = message.trimStart().startsWith("<")
    ? "The server did not answer with JSON. Check that ZORP_API_BASE points at a zorp server."
    : message;
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
  // "This run wrote that" belongs to the conversation it happened in. Carrying
  // the marks into another session would claim a run wrote files it never
  // touched.
  forgetProducedArtifacts();
}

/* ------------------------------------------------------------------ */
/* model settings                                                      */
/*                                                                      */
/* Settings live server-side (docs/DECISIONS.md, 2026-08-17): this      */
/* panel is a client for them, not a place that ships provider config   */
/* with every turn. Every read/write goes through GET/PUT               */
/* /api/settings; nothing here keeps its own copy of the API key.       */
/* ------------------------------------------------------------------ */

/**
 * Preset base URLs and the protocol each one speaks. Ollama is not a
 * separate protocol: it is the OpenAI-compatible wire format pointed at a
 * local server, offered here as a shortcut rather than a distinct provider.
 * "custom" leaves whatever base URL is already in the field alone.
 */
const PRESET_DEFAULTS: Record<string, { baseUrl: string; provider: string; needsKey: boolean }> = {
  ollama: { baseUrl: "http://localhost:11434/v1", provider: "openai", needsKey: false },
  openai: { baseUrl: "https://api.openai.com/v1", provider: "openai", needsKey: true },
  anthropic: { baseUrl: "https://api.anthropic.com/v1", provider: "anthropic", needsKey: true },
  custom: { baseUrl: "", provider: "openai", needsKey: true },
};

const SETTINGS_ENV_VARS: Record<string, string> = {
  provider: "ZORP_PROVIDER",
  base_url: "ZORP_BASE_URL",
  model: "ZORP_MODEL",
  api_key: "ZORP_API_KEY",
  max_tokens: "ZORP_MAX_TOKENS",
};

async function openSettings(): Promise<void> {
  dom.settingsOverlay.hidden = false;
  setSettingsResult("", null);
  await loadSettingsIntoForm();
  void refreshModelOptions();
}

function closeSettings(): void {
  dom.settingsOverlay.hidden = true;
}

/** Refresh just the topbar badge and composer banner, without opening the panel. */
async function refreshSettingsBadge(): Promise<void> {
  try {
    const settings = await getSettings();
    currentSettings = settings;
    updateModelBadge(settings);
    updateComposerWarning(settings);
  } catch {
    // The status pill already reports connectivity problems; a stale model
    // badge on top of that is not worth an error card of its own.
  }
}

async function loadSettingsIntoForm(): Promise<void> {
  try {
    const settings = await getSettings();
    currentSettings = settings;
    applySettingsToForm(settings);
    updateModelBadge(settings);
    updateComposerWarning(settings);
  } catch (error) {
    setSettingsResult(`Could not load settings: ${describeError(error)}`, "fail");
  }
}

function applySettingsToForm(settings: Settings): void {
  const preset = presetFor(settings.provider, settings.base_url);
  dom.settingsPreset.value = preset;
  dom.settingsBaseUrl.value = settings.base_url;
  dom.settingsBaseUrlSource.textContent = sourceLabel("base_url", settings.base_url_source);
  setModelValue(settings.model);
  dom.settingsModelSource.textContent = sourceLabel("model", settings.model_source);
  dom.settingsApiKey.value = "";
  dom.settingsApiKey.placeholder = settings.has_api_key
    ? "leave blank to keep the current key"
    : "leave blank if this endpoint needs no key";
  dom.settingsApiKeySource.textContent = settings.has_api_key
    ? sourceLabel("api_key", settings.api_key_source)
    : "";
  updateApiKeyVisibility(preset);
}

/** Guess which preset a resolved (provider, base_url) pair matches, so
 * reopening the panel shows the right choice instead of always "custom". */
function presetFor(provider: string, baseUrl: string): string {
  if (provider === "anthropic") {
    return "anthropic";
  }
  const trimmed = baseUrl.replace(/\/+$/, "");
  if (trimmed.includes("11434")) {
    return "ollama";
  }
  if (trimmed === "https://api.openai.com/v1") {
    return "openai";
  }
  return "custom";
}

function sourceLabel(field: string, source: SettingsSource): string {
  if (source === "ui") {
    return "saved";
  }
  if (source === "env") {
    return `from ${SETTINGS_ENV_VARS[field] ?? "the environment"}`;
  }
  return "default";
}

function applyPreset(preset: string): void {
  const config = PRESET_DEFAULTS[preset] ?? PRESET_DEFAULTS.custom;
  if (preset !== "custom") {
    dom.settingsBaseUrl.value = config.baseUrl;
  }
  updateApiKeyVisibility(preset);
  void refreshModelOptions();
}

function updateApiKeyVisibility(preset: string): void {
  const needsKey = (PRESET_DEFAULTS[preset] ?? PRESET_DEFAULTS.custom).needsKey;
  dom.settingsApiKeyField.hidden = !needsKey;
}

function setModelValue(model: string): void {
  dom.settingsModelText.value = model;
  const hasOption = Array.from(dom.settingsModelSelect.options).some((option) => option.value === model);
  if (hasOption) {
    dom.settingsModelSelect.value = model;
  }
}

/** Whichever of the select or the free-text fallback is currently showing. */
function currentModelValue(): string {
  return dom.settingsModelSelect.hidden
    ? dom.settingsModelText.value.trim()
    : dom.settingsModelSelect.value;
}

/**
 * Populate the model `<select>` from `GET /api/settings/models`, falling
 * back to the free-text field when listing fails or comes back empty. That
 * endpoint is always 200, so "fails" here means a thrown network error, not
 * the normal "Ollama is not running" case, which comes back as `error` set
 * on an otherwise successful response.
 */
async function refreshModelOptions(): Promise<void> {
  const baseUrl = dom.settingsBaseUrl.value.trim();
  const currentModel = currentModelValue() || currentSettings?.model || "";
  if (!baseUrl) {
    showModelFallback("Enter a base URL to list models.");
    return;
  }
  dom.settingsRefreshModels.disabled = true;
  dom.settingsRefreshModels.textContent = "Listing…";
  try {
    const { models, error } = await listModels(baseUrl);
    if (models.length === 0) {
      showModelFallback(error ?? "No models were returned.");
      return;
    }
    dom.settingsModelSelect.replaceChildren();
    for (const id of models) {
      dom.settingsModelSelect.append(modelOption(id));
    }
    if (currentModel && !models.includes(currentModel)) {
      dom.settingsModelSelect.append(modelOption(currentModel));
    }
    dom.settingsModelSelect.value = currentModel || models[0];
    dom.settingsModelSelect.hidden = false;
    dom.settingsModelText.hidden = true;
    dom.settingsModelHint.hidden = true;
  } catch (error) {
    showModelFallback(describeError(error));
  } finally {
    dom.settingsRefreshModels.disabled = false;
    dom.settingsRefreshModels.textContent = "Refresh models";
  }
}

function modelOption(id: string): HTMLOptionElement {
  const option = document.createElement("option");
  option.value = id;
  option.textContent = id;
  return option;
}

function showModelFallback(reason: string): void {
  dom.settingsModelSelect.hidden = true;
  dom.settingsModelText.hidden = false;
  if (!dom.settingsModelText.value && currentSettings) {
    dom.settingsModelText.value = currentSettings.model;
  }
  dom.settingsModelHint.hidden = false;
  dom.settingsModelHint.textContent = reason;
}

/** What the form currently says, shaped as a `PUT /api/settings` body. */
function formToUpdate(): SettingsUpdate {
  const preset = dom.settingsPreset.value;
  const provider = (PRESET_DEFAULTS[preset] ?? PRESET_DEFAULTS.custom).provider;
  const update: SettingsUpdate = {
    provider,
    base_url: dom.settingsBaseUrl.value.trim(),
    model: currentModelValue(),
  };
  const apiKey = dom.settingsApiKey.value;
  if (apiKey) {
    update.api_key = apiKey;
  }
  return update;
}

async function saveSettings(): Promise<void> {
  dom.settingsSave.disabled = true;
  setSettingsResult("Saving…", null);
  try {
    const settings = await putSettings(formToUpdate());
    currentSettings = settings;
    applySettingsToForm(settings);
    updateModelBadge(settings);
    updateComposerWarning(settings);
    setSettingsResult("Saved.", "ok");
  } catch (error) {
    setSettingsResult(`Could not save: ${describeError(error)}`, "fail");
  } finally {
    dom.settingsSave.disabled = false;
  }
}

/**
 * Test what is on screen, without saving it. The base URL in the form goes
 * along on the request, so an address that turns out to be wrong is never
 * written anywhere and whatever was saved before is still saved after.
 */
async function runConnectionTest(): Promise<void> {
  dom.settingsTest.disabled = true;
  setSettingsResult("Testing…", null);
  try {
    const result = await testConnection(dom.settingsBaseUrl.value.trim());
    if (result.ok) {
      setSettingsResult("Connected.", "ok");
    } else {
      setSettingsResult(result.reason ?? "The endpoint did not answer.", "fail");
    }
  } catch (error) {
    setSettingsResult(describeError(error), "fail");
  } finally {
    dom.settingsTest.disabled = false;
  }
}

function setSettingsResult(text: string, state: "ok" | "fail" | null): void {
  dom.settingsResult.textContent = text;
  if (state) {
    dom.settingsResult.dataset.state = state;
  } else {
    delete dom.settingsResult.dataset.state;
  }
}

function updateModelBadge(settings: Settings): void {
  if (!settings.configured) {
    dom.modelBtn.dataset.state = "unconfigured";
    dom.modelBtnLabel.textContent = "Not configured";
    return;
  }
  dom.modelBtn.dataset.state = "configured";
  dom.modelBtnLabel.textContent = settings.model;
}

/** The whole point of this feature: say so before the first message dies. */
function updateComposerWarning(settings: Settings): void {
  dom.composerWarning.hidden = settings.configured;
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

/* ------------------------------------------------------------------ */
/* artifact pane                                                       */
/* ------------------------------------------------------------------ */

/** The file currently open, so a refresh can put it back. */
let openArtifact: string | null = null;
/**
 * The listing as it was when the turn started, and what has changed since.
 *
 * A run produces files, and until this existed the only way to find out was to
 * open the pane and press Refresh. The snapshot is what makes "produced" a
 * question with an answer: everything new or newer than this is something the
 * run did.
 */
let artifactsAtTurnStart: ArtifactStamp[] | null = null;
/** Paths this turn has produced, so the list can mark them. */
const producedThisTurn = new Set<string>();
/** When the listing was last fetched, to keep tool activity from hammering it. */
let lastArtifactPoll = 0;
/** How often tool activity may trigger a listing refresh. */
const ARTIFACT_POLL_MS = 1500;

const pane: Pane = {
  get doc() {
    return dom.artifactDoc;
  },
  get frame() {
    return dom.artifactFrame;
  },
  get image() {
    return dom.artifactImage;
  },
  get empty() {
    return dom.artifactEmpty;
  },
};

function wireArtifacts(): void {
  dom.artifactsBtn.addEventListener("click", () => {
    const showing = !dom.artifacts.hidden;
    if (showing) {
      closeArtifacts();
    } else {
      openArtifactsPane();
    }
  });
  dom.artifactsClose.addEventListener("click", closeArtifacts);
  dom.artifactsRefresh.addEventListener("click", () => {
    void refreshArtifacts();
  });
}

function openArtifactsPane(): void {
  dom.artifacts.hidden = false;
  dom.artifactsBtn.setAttribute("aria-expanded", "true");
  dom.app.dataset.artifacts = "open";
  // Opening the pane is the user acting on the badge, so the badge has done
  // its job. The rows stay marked; only the count on the button clears.
  clearArtifactBadge();
  const newest = newestProducedPath;
  void refreshArtifacts().then(() => {
    if (newest) {
      void showArtifact(newest);
    }
  });
}

function closeArtifacts(): void {
  dom.artifacts.hidden = true;
  dom.artifactsBtn.setAttribute("aria-expanded", "false");
  delete dom.app.dataset.artifacts;
}

/** The most recently written file this turn produced, if any. */
let newestProducedPath: string | null = null;

/** Back to knowing nothing about what any run wrote. */
function forgetProducedArtifacts(): void {
  producedThisTurn.clear();
  newestProducedPath = null;
  artifactsAtTurnStart = null;
  clearArtifactBadge();
}

/**
 * Take the "before" picture for this turn.
 *
 * Deliberately the listing and not the tool stream. How a file got written is
 * not knowable from a tool summary: a PDF that pandoc produced under
 * `run_command` names no path anywhere, and it is exactly as much a result of
 * the run as one `write_file` wrote. Asking the directory catches both.
 */
async function snapshotArtifacts(): Promise<void> {
  forgetProducedArtifacts();
  try {
    artifactsAtTurnStart = (await listArtifacts()).files;
  } catch {
    // No snapshot means nothing gets claimed as produced this turn. Quietly
    // doing nothing beats badging the button over a failed request.
    artifactsAtTurnStart = null;
  }
}

/**
 * Look for files the run has produced and surface them.
 *
 * With the pane open, the newest one is shown: the user asked to watch the
 * workspace, so showing them what appeared in it is the answer. With the pane
 * closed, the button gets a count and nothing else happens. Opening a pane
 * over what somebody is reading mid-answer is not a feature, it is an
 * interruption, so the closed case stays a dot until they act on it.
 */
async function checkForProducedArtifacts(force = false): Promise<void> {
  const now = Date.now();
  if (!force && now - lastArtifactPoll < ARTIFACT_POLL_MS) {
    return;
  }
  lastArtifactPoll = now;

  let files: Artifact[];
  let truncated: boolean;
  try {
    const listing = await listArtifacts();
    files = listing.files;
    truncated = listing.truncated;
  } catch {
    return;
  }

  const fresh = producedSince(artifactsAtTurnStart, files);
  if (!fresh.length) {
    return;
  }
  for (const file of fresh) {
    producedThisTurn.add(file.path);
  }
  newestProducedPath = fresh[0].path;

  if (dom.artifacts.hidden) {
    showArtifactBadge(producedThisTurn.size);
    return;
  }
  renderArtifactList(files, truncated);
  await showArtifact(newestProducedPath);
}

function showArtifactBadge(count: number): void {
  dom.artifactsBadge.hidden = false;
  dom.artifactsBadge.textContent = count > 9 ? "9+" : String(count);
  dom.artifactsBtn.dataset.produced = "yes";
  dom.artifactsBtn.title =
    count === 1 ? "This run wrote a file" : `This run wrote ${count} files`;
}

function clearArtifactBadge(): void {
  dom.artifactsBadge.hidden = true;
  dom.artifactsBadge.textContent = "";
  delete dom.artifactsBtn.dataset.produced;
  dom.artifactsBtn.title = "Show files this workspace has produced";
}

async function refreshArtifacts(): Promise<void> {
  try {
    const listing = await listArtifacts();
    lastArtifactPoll = Date.now();
    renderArtifactList(listing.files, listing.truncated);
    // Reopening what was already open means a refresh after a run shows the
    // new contents rather than dropping the reader back to an empty pane.
    if (openArtifact && listing.files.some((f) => f.path === openArtifact)) {
      await showArtifact(openArtifact);
    }
  } catch (error) {
    dom.artifactList.replaceChildren();
    setArtifactMessage(`Could not list files: ${describeError(error)}`);
  }
}

function renderArtifactList(files: Artifact[], truncated: boolean): void {
  dom.artifactList.replaceChildren();
  if (!files.length) {
    const empty = el("li", "artifact-none");
    empty.textContent = "Nothing here yet. Files the agent writes show up in this list.";
    dom.artifactList.append(empty);
    return;
  }
  for (const file of files) {
    const row = el("li");
    const button = el("button", "artifact-item") as HTMLButtonElement;
    button.type = "button";
    button.dataset.path = file.path;
    if (file.path === openArtifact) {
      button.dataset.open = "yes";
    }
    button.append(
      textNode("span", "artifact-name", file.path),
      textNode("span", "artifact-size", humanBytes(file.bytes)),
    );
    if (producedThisTurn.has(file.path)) {
      // Which of these the run wrote is worth knowing after the fact, so the
      // mark outlives the badge on the button.
      button.dataset.fresh = "yes";
      button.append(textNode("span", "artifact-fresh", "new"));
    }
    button.addEventListener("click", () => {
      void showArtifact(file.path);
    });
    row.append(button);
    dom.artifactList.append(row);
  }
  if (truncated) {
    const note = el("li", "artifact-none");
    note.textContent = "That is as many as this pane lists. Narrower is better than wrong.";
    dom.artifactList.append(note);
  }
}

/**
 * Show one file.
 *
 * The decision about how is `artifact-view.ts`'s, and the part of it that
 * matters is that anything which can execute (a PDF, an SVG, an HTML file)
 * goes into the sandboxed iframe by URL and is never fetched into this page.
 * Only the types this page renders itself get read as text at all.
 */
async function showArtifact(path: string): Promise<void> {
  openArtifact = path;
  for (const node of dom.artifactList.querySelectorAll<HTMLElement>(".artifact-item")) {
    if (node.dataset.path === path) {
      node.dataset.open = "yes";
    } else {
      delete node.dataset.open;
    }
  }

  if (!needsText(path)) {
    showArtifactIn(pane, path, artifactUrl(path), null, renderMarkdown);
    return;
  }

  try {
    const text = await readArtifact(path);
    showArtifactIn(pane, path, artifactUrl(path), text, renderMarkdown);
  } catch (error) {
    // A file that vanished between listing and opening says so. An empty
    // pane and an empty file look identical, and they are not the same.
    setArtifactMessage(describeError(error));
  }
}

function setArtifactMessage(text: string): void {
  dom.artifactDoc.hidden = true;
  dom.artifactFrame.hidden = true;
  dom.artifactFrame.removeAttribute("src");
  dom.artifactImage.hidden = true;
  dom.artifactImage.removeAttribute("src");
  dom.artifactEmpty.hidden = false;
  dom.artifactEmpty.textContent = text;
}

function humanBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
