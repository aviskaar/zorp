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
import { answerActions } from "./copy-response";
import { clearMeter, showMeter, type MeterElements } from "./context-meter";
import { autoApproveView, renderAutoApprove, type AutoApproveView } from "./approval-mode";
import {
  renderSearchIndicator,
  searchIndicatorView,
  type SearchIndicatorView,
} from "./search-indicator";
import { setSendControl } from "./send-control";
import { createVoiceInput } from "./voice-input";
import { PanelView } from "./panel-view";
import { ZorpModeView } from "./zorp-mode";
import { sessionFromSearch, searchForSession } from "./session-url";
import { emptySessionRow, sessionRow, UNTITLED } from "./session-row";
import {
  PaneResizer,
  artifactsBounds,
  layoutStore,
  readLayout,
  saveCollapsed,
  saveWidth,
  setSidebarCollapsed,
  sidebarBounds,
  sidebarIsCollapsed,
} from "./layout";
import { coerceHits, renderNotice, renderResults, summarize } from "./conversation-search";
import { coerceCitations, renderMemoryNote } from "./memory-note";
import {
  needsText,
  producedSince,
  showArtifact as showArtifactIn,
  type ArtifactStamp,
  type Pane,
} from "./artifact-view";
import {
  ApiError,
  NothingRunningError,
  TurnBusyError,
  approve,
  getAutoApprove,
  setAutoApprove,
  getCapabilities,
  getVoiceStatus,
  getSession,
  getSettings,
  listModels,
  listSessions,
  newSession,
  putSettings,
  recallSearch,
  recallStatus,
  sendTurn,
  startPanel,
  startInvestigate,
  getInvestigateStatus,
  getLedger,
  stopTurn,
  streamEvents,
  testConnection,
  artifactUrl,
  listArtifacts,
  readArtifact,
  waitForVoiceModel,
  transcribeVoice,
  type Artifact,
  type EventStream,
  type MemoryEvent,
  type Message,
  type Preregistration,
  type RecallStatus,
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
  sidebar: HTMLElement;
  sidebarResizer: HTMLElement;
  artifactsResizer: HTMLElement;
  sessionList: HTMLElement;
  recall: HTMLElement;
  composerMemory: HTMLElement;
  useMemory: HTMLInputElement;
  recallInput: HTMLInputElement;
  recallStatus: HTMLElement;
  recallResults: HTMLElement;
  newChat: HTMLButtonElement;
  menu: HTMLButtonElement;
  sidebarClose: HTMLButtonElement;
  title: HTMLElement;
  modelBtn: HTMLButtonElement;
  modelBtnLabel: HTMLElement;
  status: HTMLElement;
  statusText: HTMLElement;
  contextMeter: HTMLElement;
  contextBarFill: HTMLElement;
  contextMeterText: HTMLElement;
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
  voiceMicrophone: HTMLButtonElement;
  voiceCancel: HTMLButtonElement;
  voiceStatus: HTMLElement;
  voiceDownload: HTMLButtonElement;
  voiceCommand: HTMLElement;
  reviewPanel: HTMLButtonElement;
  zorpMode: HTMLButtonElement;
  zorpPanel: HTMLElement;
  zorpStatus: HTMLElement;
  zorpForm: HTMLFormElement;
  zorpQuestion: HTMLTextAreaElement;
  zorpMetric: HTMLInputElement;
  zorpThreshold: HTMLInputElement;
  zorpDirection: HTMLSelectElement;
  zorpRun: HTMLButtonElement;
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
  filesMenu: HTMLElement;
  filesPopover: HTMLElement;
  artifacts: HTMLElement;
  artifactTitle: HTMLElement;
  artifactsClose: HTMLButtonElement;
  artifactsRefresh: HTMLButtonElement;
  artifactList: HTMLElement;
  artifactEmpty: HTMLElement;
  artifactDoc: HTMLElement;
  artifactFrame: HTMLIFrameElement;
  artifactPdf: HTMLIFrameElement;
  artifactImage: HTMLImageElement;
}

type ApprovalOutcome = "allowed" | "denied" | "expired" | "stopped";

interface PendingApproval {
  settle(outcome: ApprovalOutcome): void;
}

const dom = collectElements();
/**
 * The open panel block, if any.
 *
 * One per page rather than one per panel: only one panel can run on a
 * session at a time, because a panel occupies the session exactly as a
 * turn does.
 */
const panelView = new PanelView(document, dom.transcript);
/**
 * The open Zorp mode block, if any. One per page, for the same reason
 * the panel's is: an attempt occupies the session exactly as a turn
 * does, so only one can be running.
 */
const zorpView = new ZorpModeView(document, dom.transcript);
const voiceInput = createVoiceInput(
  {
    input: dom.input,
    microphone: dom.voiceMicrophone,
    cancel: dom.voiceCancel,
    status: dom.voiceStatus,
    download: dom.voiceDownload,
    command: dom.voiceCommand,
  },
  {
    status: getVoiceStatus,
    wait: waitForVoiceModel,
    transcribe: transcribeVoice,
  },
);

let sessionId: string | null = null;
let stream: EventStream | null = null;
let streamSessionId: string | null = null;
let catchUp: ZorpEvent[] | null = null;
let turnRunning = false;
/**
 * Whether the turn that is ending was stopped by hand.
 *
 * Set by the `stopped` event and read by `finishTurn`, which is the only
 * place that knows a pending approval card is about to be settled and needs to
 * say why. Cleared when the next turn starts.
 */
let turnStopped = false;
let workingDepth = 0;
let lastSeq = -1;
let sessions: SessionSummary[] = [];
let activityGroup: HTMLElement | null = null;
let spinnerTimer: number | null = null;
let spinnerFrame = 0;
const pendingApprovals = new Map<string, PendingApproval>();
const approvalMode: AutoApproveView = autoApproveView(document);
const searchIndicator: SearchIndicatorView = searchIndicatorView(document);
/**
 * Whether this session has stood its approvals down.
 *
 * A cache of the server's answer, never a decision of its own. It is set from
 * a server response and from nowhere else, so the banner cannot claim a state
 * the server does not hold. The one moment it leads is between the user asking
 * for the mode and a session existing to hold it, and the request is sent the
 * instant one does.
 */
let autoApprove = false;
/** The last settings the server reported. Null until the first successful
 * `GET /api/settings`, which happens once the server is known reachable. */
let currentSettings: Settings | null = null;
/**
 * Where the pane widths and the collapsed flag are kept, or null if nowhere,
 * and the two handles once they exist.
 *
 * Up here rather than beside `wireLayout`, and that is not tidiness. `start()`
 * is called partway down this file, so anything declared below it is still
 * uninitialised while the wiring runs: the bundler turns a top-level `let`
 * into a `var`, and a `var` initialiser that runs after the wiring quietly
 * puts the resizers back to null. The first version of this shipped that way
 * and the two handles could not see each other's width, so neither knew how
 * much room was left for the conversation.
 */
const layoutStorage = layoutStore();
let sidebarResizer: PaneResizer | null = null;
let artifactsResizer: PaneResizer | null = null;

start();

function start(): void {
  wireComposer();
  wireLayout();
  wireSidebar();
  wireRecall();
  wireScroller();
  wireSettings();
  wireArtifacts();
  wireApprovalMode();
  wireZorpMode();
  paintApprovalMode();
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
    void refreshSessions().then(restoreSessionFromUrl);
    void refreshSettingsBadge();
    void refreshCapabilities();
    void refreshRecallStatus();
    dom.input.focus();
    return;
  }
  setStatus("idle", "no server");
  showServerMissing();
}

/** Open whatever conversation the URL names, on load and on back/forward.
 *
 * Called after the session list has been read, so a stale link names a
 * session that is simply not in the list and falls through to a new chat
 * rather than to an error card. A link to a deleted conversation is a
 * normal thing to click, not a fault.
 */
function restoreSessionFromUrl(): void {
  const wanted = sessionFromSearch(window.location.search);
  if (wanted === sessionId) {
    return;
  }
  if (wanted === null) {
    startNewChat();
    return;
  }
  const session = sessions.find((candidate) => candidate.id === wanted);
  if (!session) {
    startNewChat();
    return;
  }
  void openSession(session);
}

/** Back and forward move between conversations.
 *
 * Without this the URL changes and the page does not, which is a worse
 * bug than the one the URL was added to fix: the address bar would be
 * telling you about a conversation you are not looking at.
 */
window.addEventListener("popstate", () => {
  restoreSessionFromUrl();
});

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
    sidebar: byId("sidebar"),
    sidebarResizer: byId("sidebar-resizer"),
    artifactsResizer: byId("artifacts-resizer"),
    sessionList: byId("session-list"),
    recall: byId("recall"),
    composerMemory: byId("composer-memory"),
    useMemory: byId<HTMLInputElement>("use-memory"),
    recallInput: byId("recall-input"),
    recallStatus: byId("recall-status"),
    recallResults: byId("recall-results"),
    newChat: byId<HTMLButtonElement>("new-chat"),
    menu: byId<HTMLButtonElement>("menu"),
    sidebarClose: byId<HTMLButtonElement>("sidebar-close"),
    title: byId("session-title"),
    modelBtn: byId<HTMLButtonElement>("model-btn"),
    modelBtnLabel: byId("model-btn-label"),
    status: byId("status"),
    statusText: byId("status-text"),
    contextMeter: byId("context-meter"),
    contextBarFill: byId("context-bar-fill"),
    contextMeterText: byId("context-meter-text"),
    scroller: byId("scroller"),
    transcript: byId("transcript"),
    working: byId("working"),
    workingSpinner: byId("working-spinner"),
    workingVerb: byId("working-verb"),
    jump: byId<HTMLButtonElement>("jump"),
    reviewPanel: byId<HTMLButtonElement>("review-panel"),
    zorpMode: byId<HTMLButtonElement>("zorp-mode"),
    zorpPanel: byId<HTMLElement>("zorp-panel"),
    zorpStatus: byId("zorp-status"),
    zorpForm: byId<HTMLFormElement>("zorp-form"),
    zorpQuestion: byId<HTMLTextAreaElement>("zorp-question"),
    zorpMetric: byId<HTMLInputElement>("zorp-metric"),
    zorpThreshold: byId<HTMLInputElement>("zorp-threshold"),
    zorpDirection: byId<HTMLSelectElement>("zorp-direction"),
    zorpRun: byId<HTMLButtonElement>("zorp-run"),
    composerWarning: byId("composer-warning"),
    composerWarningSettings: byId<HTMLButtonElement>("composer-warning-settings"),
    composer: byId<HTMLFormElement>("composer"),
    input: byId<HTMLTextAreaElement>("input"),
    send: byId<HTMLButtonElement>("send"),
    voiceMicrophone: byId<HTMLButtonElement>("voice-mic"),
    voiceCancel: byId<HTMLButtonElement>("voice-cancel"),
    voiceStatus: byId("voice-status"),
    voiceDownload: byId<HTMLButtonElement>("voice-download"),
    voiceCommand: byId("voice-command"),
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
    filesMenu: byId<HTMLElement>("files-menu"),
    filesPopover: byId<HTMLElement>("files-popover"),
    artifacts: byId<HTMLElement>("artifacts"),
    artifactTitle: byId<HTMLElement>("artifact-title"),
    artifactsClose: byId<HTMLButtonElement>("artifacts-close"),
    artifactsRefresh: byId<HTMLButtonElement>("artifacts-refresh"),
    artifactList: byId<HTMLElement>("artifact-list"),
    artifactEmpty: byId<HTMLElement>("artifact-empty"),
    artifactDoc: byId<HTMLElement>("artifact-doc"),
    artifactFrame: byId<HTMLIFrameElement>("artifact-frame"),
    artifactPdf: byId<HTMLIFrameElement>("artifact-pdf"),
    artifactImage: byId<HTMLImageElement>("artifact-image"),
    settingsResult: byId("settings-result"),
  };
}

function wireComposer(): void {
  dom.composer.addEventListener("submit", (event) => {
    event.preventDefault();
    void submitMessage();
  });

  // The stop lives on the button's own click, not on the form's submit, and
  // the difference matters. Enter in the textarea submits the form, and Enter
  // is a key people lean on: routing the stop through submit would mean a
  // stray Enter during a run killed it. Preventing the default on a submit
  // button's click is what stops the form from submitting underneath this.
  dom.send.addEventListener("click", (event) => {
    if (!turnRunning) {
      return;
    }
    event.preventDefault();
    void stopRunningTurn();
  });

  dom.reviewPanel.addEventListener("click", () => {
    void submitPanel();
  });

  dom.zorpMode.addEventListener("click", () => {
    toggleZorpPanel();
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
  dom.menu.addEventListener("click", showSidebar);
  dom.sidebarClose.addEventListener("click", hideSidebar);
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
/* approval mode                                                       */
/*                                                                      */
/* One switch, two places it shows, and the server is the only thing    */
/* that decides what it currently is. See src/approval-mode.ts.         */
/* ------------------------------------------------------------------ */

function wireApprovalMode(): void {
  approvalMode.button.addEventListener("click", () => void changeApprovalMode(!autoApprove));
  approvalMode.bannerOff.addEventListener("click", () => void changeApprovalMode(false));
}

/** Draw the mode, and keep the empty state from contradicting it. */
function paintApprovalMode(): void {
  renderAutoApprove(approvalMode, autoApprove);
  if (dom.transcript.querySelector(".empty")) {
    showEmptyState();
  }
}

/**
 * Ask the server to change the mode, then show whatever it answers.
 *
 * A failed request leaves the page showing the state the server still holds,
 * which is the only honest thing to draw. Turning it off is the direction that
 * matters most here: if that request fails the banner stays up, because the
 * gate really is still down.
 */
async function changeApprovalMode(on: boolean): Promise<boolean> {
  if (!sessionId) {
    // Nothing to tell yet. `submitMessage` sends this the moment a session
    // exists, before the first turn starts.
    autoApprove = on;
    paintApprovalMode();
    return autoApprove;
  }
  try {
    autoApprove = await setAutoApprove(sessionId, on);
  } catch (error) {
    appendError(`Could not change the approval mode: ${describeError(error)}`);
  }
  paintApprovalMode();
  return autoApprove;
}

/**
 * Read the mode back from the server.
 *
 * A reloaded tab and a switched session both know nothing until they ask, and
 * a gate that is down has to be visible from the first paint rather than from
 * the first approval that does not arrive.
 */
async function refreshApprovalMode(): Promise<void> {
  if (!sessionId) {
    autoApprove = false;
    paintApprovalMode();
    return;
  }
  try {
    autoApprove = await getAutoApprove(sessionId);
  } catch {
    // The status pill already reports a server that cannot be reached. What
    // is on the page is the last thing the server said, which is the best
    // available answer.
  }
  paintApprovalMode();
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
      if (autoApprove) {
        // The switch was thrown before there was a session to hold it. Tell
        // the server now, before the first tool runs, and if that does not
        // land, say so and put the page back to asking rather than leave a
        // banner claiming a mode the server never took.
        autoApprove = await setAutoApprove(sessionId, true).catch(() => false);
        paintApprovalMode();
        if (!autoApprove) {
          appendError("Could not stand approvals down for this chat, so it will keep asking.");
        }
      }
    }
    // The stream for this session is already open and stays open, so the
    // turn's events arrive on it. The replay guard deliberately keeps its high
    // water mark: seq climbs for the life of the session, and resetting it
    // here would let a reconnect mid-turn render the backlog twice.
    await ensureStream(sessionId);
    // The box is read here and cleared below, so recall is a decision
    // about this message and never a state the next one inherits.
    await sendTurn(sessionId, message, dom.useMemory.checked);
    dom.useMemory.checked = false;
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
 * Zorp mode: what the last attempt was about.
 *
 * Held because the ledger is read back by question, not by track id, and
 * because a run that failed still recorded the conditions it started
 * under. Without this the page would have nothing to ask about after an
 * attempt that did not reach its closing frame.
 */
let zorpQuestion: string | null = null;
/** Whether the running turn is a Zorp mode attempt rather than a turn. */
let zorpRunning = false;

function wireZorpMode(): void {
  dom.zorpForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void submitInvestigate();
  });
  // Asked once, on load. Both facts are properties of the server binary
  // and its environment, and neither changes while the page is open.
  void refreshZorpStatus();
}

/**
 * Open or close the Zorp mode form.
 *
 * A toggle rather than a modal, because the form has to sit next to the
 * transcript the attempt will write into.
 */
function toggleZorpPanel(): void {
  const open = dom.zorpPanel.hidden;
  dom.zorpPanel.hidden = !open;
  dom.zorpMode.setAttribute("aria-expanded", String(open));
  if (open) {
    dom.zorpQuestion.focus();
  }
}

/**
 * Say whether Zorp mode can run here, and whether it will forecast.
 *
 * Both come from the server. `available` is what its binary was built
 * with: the research feature is opt-in and an ordinary chat server does
 * not have it, so the honest thing is to say so rather than to offer a
 * button that 501s.
 *
 * Forecasting is reported and never set. It costs a model call on every
 * attempt, it is off by default, and a browser control that turned it on
 * would be one page changing what the server does for everyone using it.
 */
async function refreshZorpStatus(): Promise<void> {
  try {
    const status = await getInvestigateStatus();
    if (!status.available) {
      dom.zorpStatus.textContent =
        "This server was built without the research feature, so it cannot run an attempt. Rebuild zorp-web with --features research.";
      dom.zorpRun.disabled = true;
      return;
    }
    dom.zorpStatus.textContent = status.forecasting
      ? "Forecasting is on, so each attempt records an expectation before it runs."
      : "Forecasting is off, so no expectation is recorded and nothing can be scored for calibration. It is set where the server runs, not here.";
  } catch (error) {
    dom.zorpStatus.textContent = describeError(error);
  }
}

/**
 * Run one pre-registered attempt.
 *
 * A person presses this. There is no tool that reaches it and there must
 * never be one: an attempt writes to a pre-registered evidence record
 * and to the aryabhatta ledger, so a model that could start one could
 * feed the record it is later read against.
 *
 * The pre-registration is all three fields or none. None means reuse
 * what is recorded for this question, which is what a second attempt on
 * the same track does. Half of one is refused here rather than sent, so
 * a typo does not cost a round trip.
 */
async function submitInvestigate(): Promise<void> {
  if (turnRunning) {
    return;
  }
  const question = dom.zorpQuestion.value.trim();
  if (!question) {
    appendError("Zorp mode needs a question. There is nothing to pre-register an attempt against.");
    scrollToBottomIfFollowing(true);
    return;
  }

  const metric = dom.zorpMetric.value.trim();
  const thresholdText = dom.zorpThreshold.value.trim();
  const given = [metric, thresholdText].filter((v) => v !== "").length;
  if (given === 1) {
    appendError(
      "A pre-registration is a metric and a kill threshold together. Give both, or leave both empty to reuse the one already recorded for this question.",
    );
    scrollToBottomIfFollowing(true);
    return;
  }

  let prereg: Preregistration | null = null;
  if (given === 2) {
    const threshold = Number(thresholdText);
    if (!Number.isFinite(threshold)) {
      appendError("The kill threshold has to be a finite number.");
      scrollToBottomIfFollowing(true);
      return;
    }
    prereg = {
      metric_name: metric,
      kill_threshold: threshold,
      threshold_direction: dom.zorpDirection.value as Preregistration["threshold_direction"],
    };
  }

  clearEmptyState();
  appendMessage("user", question);
  scrollToBottomIfFollowing(true);
  setTurnRunning(true);
  zorpRunning = true;
  zorpQuestion = question;

  try {
    if (!sessionId) {
      sessionId = await newSession();
      setTitle("Zorp mode");
      await refreshSessions();
      markActiveSession();
    }
    await ensureStream(sessionId);
    await startInvestigate(sessionId, question, prereg);
  } catch (error) {
    setTurnRunning(false);
    zorpRunning = false;
    if (error instanceof TurnBusyError) {
      appendError("A turn is already running on this session. Wait for it to finish.");
    } else {
      appendError(describeError(error));
    }
    scrollToBottomIfFollowing(true);
  }
}

/**
 * Read back what the attempt left in the ledger.
 *
 * A separate read rather than a payload on the closing frame, on
 * purpose. Conditions are recorded before the work starts, so an attempt
 * that fell over still left something worth showing, and a read the page
 * can repeat is the only shape that covers both endings.
 */
async function showZorpLedger(): Promise<void> {
  if (!zorpQuestion) {
    return;
  }
  try {
    zorpView.showLedger(await getLedger(zorpQuestion));
  } catch (error) {
    appendError(describeError(error));
  }
  scrollToBottomIfFollowing(true);
}

/**
 * Launch a review panel over whatever is in the composer.
 *
 * The composer, not a file picker, and not the transcript. The material
 * has to be something the reader chose and can see, because a panel that
 * quietly reviewed "the last answer" would produce five confident
 * verdicts about a target the reader never confirmed.
 *
 * The whole default panel every time. Choosing lenses is deliberately not
 * offered yet: a reader who can pick the reviewers can pick the ones
 * likely to agree with them, which is the opposite of adversarial. The
 * server accepts a subset, so this is a decision about the page and not a
 * limit of the API.
 */
async function submitPanel(): Promise<void> {
  const body = dom.input.value.trim();
  if (turnRunning) {
    return;
  }
  if (!body) {
    appendError(
      "Put the material to review in the composer first. A panel over an empty target is five agents confidently reviewing nothing.",
    );
    scrollToBottomIfFollowing(true);
    return;
  }

  dom.input.value = "";
  autoGrowInput();
  clearEmptyState();
  appendMessage("user", body);
  scrollToBottomIfFollowing(true);
  setTurnRunning(true);

  try {
    if (!sessionId) {
      sessionId = await newSession();
      setTitle("Review panel");
      await refreshSessions();
      markActiveSession();
    }
    await ensureStream(sessionId);
    await startPanel(sessionId, "the text you sent", body, []);
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
 * Ask the server to end the running turn.
 *
 * This does not end the turn here. The server stops the agent and the agent
 * closes the turn on the stream, the same as every other ending, and the UI
 * goes back to idle when `done` arrives. Ending it locally would put the
 * composer back and leave the agent running, which is worse than no button:
 * the next thing written to the workspace would come from a turn the page says
 * is over.
 *
 * The exception is a server that says nothing was running. Then this page's
 * idea of the session is the stale one, and the honest fix is to go idle.
 */
async function stopRunningTurn(): Promise<void> {
  if (!sessionId || !turnRunning) {
    return;
  }
  setSendControl(dom.send, "stopping");
  try {
    await stopTurn(sessionId);
  } catch (error) {
    if (error instanceof NothingRunningError) {
      finishTurn();
      return;
    }
    // The run is still going and the button has to stay a stop button, or
    // there is no second chance at it.
    setSendControl(dom.send, "stop");
    appendError(`Could not stop the turn: ${describeError(error)}`);
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

    case "context":
      showMeter(meterElements(), {
        used_tokens: event.used_tokens,
        limit_tokens: event.limit_tokens,
        source: event.source,
      });
      break;

    case "memory":
      // Above the answer, because it is the reason for the answer. An
      // activity line would put it in with the tool calls, where it reads
      // as something the agent did rather than as something it was told.
      activityGroup = null;
      appendMemoryNote(event);
      break;

    case "session_title":
      // The server named this conversation. Rename it in place rather than
      // asking for the list again: the event carries the whole answer, and
      // it lands after `done` has already refreshed the sidebar once.
      renameSession(event.title);
      break;

    case "error":
      appendError(event.message);
      // A turn that failed is still a turn that ran, and the server sends
      // `error` instead of `done` rather than as well as it, so without this
      // a run that wrote three files and then lost the model would leave them
      // unmentioned.
      void checkForProducedArtifacts(true);
      break;

    case "stopped":
      turnStopped = true;
      appendStopped();
      // A stopped run wrote whatever it wrote before it was stopped, and
      // those files are exactly the ones worth looking at.
      void checkForProducedArtifacts(true);
      break;

    case "reviewer_started":
      // Activity grouping is for consecutive tool lines and a panel block
      // is not one, so the group is broken here or the next tool line
      // would try to join a group the panel already interrupted.
      activityGroup = null;
      panelView.start(event.lens);
      break;

    case "reviewer_finished":
      activityGroup = null;
      panelView.finish(event.lens, event.findings);
      break;

    case "reviewer_failed":
      activityGroup = null;
      panelView.fail(event.lens, event.why);
      break;

    case "panel_done":
      activityGroup = null;
      panelView.done(event);
      break;

    case "investigate_done":
      activityGroup = null;
      zorpView.done(event);
      break;

    case "done":
      // A panel that ended without a `panel_done`, which is what a stop
      // or an error looks like, leaves its block on the page with its
      // reviewers in whatever state they reached. Forgetting it here is
      // what stops the next panel appending to a stale block.
      panelView.close();
      // The ledger read happens here rather than on `investigate_done`,
      // because an attempt that failed or was stopped never sends one
      // and still recorded the conditions it started under. Reading on
      // every ending is what makes those visible.
      if (zorpRunning) {
        zorpRunning = false;
        void showZorpLedger().then(() => zorpView.close());
      }
      finishTurn();
      break;
  }

  scrollToBottomIfFollowing(following);
}

function finishTurn(): void {
  setTurnRunning(false);
  workingDepth = 0;
  updateWorking();
  // Settled with the reason, so a card left open by a stop does not claim it
  // timed out. The server denied it either way; what differs is who decided.
  expirePendingApprovals(turnStopped ? "stopped" : "expired");
  turnStopped = false;
  void refreshSessions();
  // Forced past the poll interval: this is the last chance to notice what the
  // run wrote, and a file written in the final second is the interesting one.
  void checkForProducedArtifacts(true);
}

function setTurnRunning(running: boolean): void {
  turnRunning = running;
  // One control: an arrow that sends while idle, a square that stops while a
  // turn runs. It used to be disabled for the length of the run, which is why
  // there was no way to stop anything.
  setSendControl(dom.send, running ? "stop" : "send");
  // A second panel while one is running would be refused by the server
  // anyway; disabling it says so before the click rather than after.
  dom.reviewPanel.disabled = running;
  // Same for an attempt, which occupies the session the same way. The
  // toggle stays live so the form can be read while something runs; only
  // the control that would start a second one goes down.
  dom.zorpRun.disabled = running;
  dom.composer.classList.toggle("is-busy", running);
  if (running) {
    turnStopped = false;
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

function meterElements(): MeterElements {
  return {
    root: dom.contextMeter,
    fill: dom.contextBarFill,
    text: dom.contextMeterText,
  };
}

/**
 * Put the meter away.
 *
 * Called whenever the conversation on screen changes. A reading belongs to
 * one session's transcript, and leaving the last one up while a different
 * session loads would state a number about a conversation it was never
 * measured on. The next turn puts it back within a second.
 */
function forgetContextReading(): void {
  clearMeter(meterElements());
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
const streamed = new StreamedMessage(
  dom.transcript,
  renderMarkdown,
  undefined,
  undefined,
  (row, text) => row.append(answerControls(text)),
);

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
  if (role === "assistant") {
    row.append(answerControls(text));
  }
  dom.transcript.append(row);
}

/**
 * The controls under one answer: copy it, or copy it framed for another
 * assistant.
 *
 * `navigator.clipboard` is absent outside a secure context, and a browser can
 * refuse the write even inside one. Both arrive at the button as a rejected
 * promise, which is what makes it say "Copy failed" rather than appear to
 * work. Loopback counts as secure, so the ordinary case is fine.
 */
function answerControls(text: string): HTMLElement {
  return answerActions(document, () => text, (value) =>
    navigator.clipboard
      ? navigator.clipboard.writeText(value)
      : Promise.reject(new Error("this browser offers no clipboard here")),
  );
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

/**
 * Draw what this turn recalled, before the answer that used it.
 *
 * The rendering lives in `src/memory-note.ts` and every string in it goes
 * through `textContent`: a recalled snippet is text out of an old
 * conversation, which can be a tool result or a page the agent fetched.
 */
function appendMemoryNote(event: MemoryEvent): void {
  const card = renderMemoryNote(
    document,
    coerceCitations(event.used),
    event.unavailable ?? null,
    (id) => {
      const known = sessions.find((session) => session.id === id);
      void openSession(known ?? { id, title: "", updated_at: "" });
      closeSidebar();
    },
  );
  dom.transcript.append(card);
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
 * The turn was stopped by hand.
 *
 * A card rather than an activity line, and deliberately not an error card. The
 * run ended without an answer, and the reader needs to know that it ended
 * because they said so and not because something broke. Whatever the model had
 * streamed by then is above this and stays.
 */
function appendStopped(): void {
  activityGroup = null;
  const card = el("div", "card card-stopped");
  const head = el("div", "card-head");
  head.append(glyph("stop"), textNode("span", "card-title", "Stopped"));
  const body = el("p", "card-body");
  body.textContent =
    "You stopped this turn. The agent is not running any more, and anything it had already written is still there.";
  card.append(head, body);
  dom.transcript.append(card);
}

/**
 * The security boundary of the product. The agent is parked until one of these
 * buttons is pressed, and nothing here presses one automatically.
 */
/**
 * How a settled approval card describes itself.
 *
 * Records rather than nested conditionals, so a new outcome is a compile
 * error here instead of quietly falling into whichever branch was last. The
 * distinction they carry is who decided: "expired" means nobody did and the
 * server denied it after five minutes, "stopped" means the reader ended the
 * turn while it was on screen. Both deny the tool. Calling the second one
 * expired would be a small lie told at exactly the moment the reader is
 * checking what their button press did.
 */
const APPROVAL_TITLES: Record<ApprovalOutcome, string> = {
  allowed: "Tool allowed",
  denied: "Tool denied",
  expired: "Approval expired",
  stopped: "Turn stopped",
};

const APPROVAL_NOTES: Record<ApprovalOutcome, string> = {
  allowed: "You allowed this, so the agent carried on.",
  denied: "You denied this. The tool did not run.",
  expired: "The turn ended before this was answered, so the server denied it.",
  stopped: "You stopped the turn while this was waiting, so the tool did not run.",
};

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
  // The third choice, offered here because this is the moment a long run
  // becomes a click per step. It is spelled out rather than abbreviated, it
  // is not the primary button, and taking it puts a banner over the composer
  // that stays there until the mode is turned off.
  const allowAllButton = el("button", "btn btn-allow-all") as HTMLButtonElement;
  allowAllButton.type = "button";
  allowAllButton.textContent = "Allow all for this chat";
  allowAllButton.title =
    "Stop asking for the rest of this chat. The hard denylist still applies.";
  actions.append(allowButton, denyButton, allowAllButton);

  const note = el("p", "card-note");
  card.append(head, lead, toolField, argsField, actions, note);
  dom.transcript.append(card);

  const buttons = [allowButton, denyButton, allowAllButton];
  const enable = (on: boolean): void => {
    for (const button of buttons) {
      button.disabled = !on;
    }
  };

  const settle = (outcome: ApprovalOutcome): void => {
    enable(false);
    actions.remove();
    lead.remove();
    card.classList.add(`is-${outcome}`);
    tag.textContent = outcome;
    title.textContent = APPROVAL_TITLES[outcome];
    note.textContent = APPROVAL_NOTES[outcome];
    pendingApprovals.delete(id);
  };

  const decide = async (allow: boolean): Promise<void> => {
    if (!sessionId) {
      return;
    }
    enable(false);
    note.textContent = "Sending your decision…";
    try {
      await approve(sessionId, id, allow);
      settle(allow ? "allowed" : "denied");
    } catch (error) {
      enable(true);
      note.textContent = `Could not send the decision: ${describeError(error)}`;
    }
  };

  /**
   * Allow this one and stop asking about the rest.
   *
   * Two steps in one click, and the mode goes first on purpose: if the server
   * will not take it, this call stays parked and the card goes back to three
   * buttons rather than approving something under a promise that was not kept.
   */
  const decideAll = async (): Promise<void> => {
    enable(false);
    note.textContent = "Standing approvals down for this chat…";
    if (!(await changeApprovalMode(true))) {
      enable(true);
      note.textContent = "The approval mode did not change, so this is still your decision.";
      return;
    }
    await decide(true);
  };

  allowButton.addEventListener("click", () => void decide(true));
  denyButton.addEventListener("click", () => void decide(false));
  allowAllButton.addEventListener("click", () => void decideAll());

  pendingApprovals.set(id, { settle });
}

/** Settle every card still on screen when a turn ends. */
function expirePendingApprovals(outcome: "expired" | "stopped" = "expired"): void {
  for (const pending of Array.from(pendingApprovals.values())) {
    pending.settle(outcome);
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
/* conversation search                                                 */
/*                                                                     */
/* Searching what you already said, by meaning. The vectors come from  */
/* a model on this machine and go into a file on this machine, and if  */
/* there is no such model the server says so rather than reaching for  */
/* one somewhere else. The rendering is in src/conversation-search.ts. */
/* ------------------------------------------------------------------ */

/** Long enough that typing a word does not fire three searches, short
 * enough that the list keeps up with the box. Each search costs one call
 * to the local model, so this is a real cost and not just a paint. */
const RECALL_DEBOUNCE_MS = 250;
/** Status is local SQLite state, but a sidebar still does not need to ask
 * more than four times a minute. Hidden tabs wait until they are visible. */
const RECALL_STATUS_REFRESH_MS = 15_000;

let recallTimer: number | undefined;
let recallStatusTimer: number | undefined;
/** Which query the last request was for, so a slow answer to an older
 * query cannot overwrite the newer one it arrived after. */
let recallInFlight = "";

function wireRecall(): void {
  dom.recallInput.addEventListener("input", () => {
    window.clearTimeout(recallTimer);
    const query = dom.recallInput.value;
    if (!query.trim()) {
      dom.recallResults.hidden = true;
      dom.recallResults.replaceChildren();
      return;
    }
    recallTimer = window.setTimeout(() => void runRecallSearch(query), RECALL_DEBOUNCE_MS);
  });
  recallStatusTimer ??= window.setInterval(() => {
    if (document.visibilityState === "visible") {
      void refreshRecallStatus();
    }
  }, RECALL_STATUS_REFRESH_MS);
}

/** Show or hide the search box, and say why when it is off. */
async function refreshRecallStatus(): Promise<void> {
  let status: RecallStatus;
  try {
    status = await recallStatus();
  } catch {
    // An older server has no such endpoint. Nothing to show and nothing
    // worth an error card for a feature the page can simply not offer.
    // The composer box goes with it: a tick that the server would answer
    // with a 404 is worse than no tick.
    dom.recall.hidden = true;
    dom.composerMemory.hidden = true;
    dom.useMemory.checked = false;
    return;
  }
  dom.recall.hidden = false;
  dom.recallStatus.textContent = summarize(status);
  dom.recallInput.disabled = !status.available;
  // Two conditions, and both have to hold. `memory` says the server was
  // built able to read the index into a turn, `available` says there is a
  // local embedder to read it with. Offering the box when either is false
  // would be offering a control that answers with an apology.
  const canRecall = status.memory && status.available;
  dom.composerMemory.hidden = !canRecall;
  if (!canRecall) {
    dom.useMemory.checked = false;
  }
}

async function runRecallSearch(query: string): Promise<void> {
  recallInFlight = query;
  try {
    const hits = coerceHits(await recallSearch(query));
    if (recallInFlight !== query) {
      return;
    }
    dom.recallResults.hidden = false;
    renderResults(document, dom.recallResults, hits, (id) => {
      const known = sessions.find((session) => session.id === id);
      void openSession(known ?? { id, title: "", updated_at: "" });
      closeSidebar();
    });
  } catch (error) {
    if (recallInFlight !== query) {
      return;
    }
    dom.recallResults.hidden = false;
    // The server's own words. A 503 here says which endpoint did not
    // answer and what to start, which "search failed" does not.
    renderNotice(document, dom.recallResults, describeError(error));
  }
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
    dom.sessionList.append(emptySessionRow(document));
    return;
  }

  for (const session of sessions) {
    dom.sessionList.append(
      sessionRow(document, session, {
        active: session.id === sessionId,
        when: relativeTime(session.updated_at),
        onOpen: (chosen) => {
          void openSession(chosen);
          closeSidebar();
        },
      }),
    );
  }
}

/**
 * The server named the conversation that is open. Put the name on the page.
 *
 * The stream belongs to one session, so a `session_title` frame is always
 * about the one being shown. The row is rewritten in the local list and the
 * list is redrawn, which is what makes the sidebar catch up without a
 * reload and without a poll.
 *
 * The title is model output. It reaches the DOM through `setTitle` and
 * `renderSessions`, both of which assign `textContent`, and it must keep
 * doing so: this is a string a model wrote after reading the user's message
 * and its own answer.
 */
function renameSession(title: string): void {
  if (!sessionId || !title) {
    return;
  }
  const row = sessions.find((session) => session.id === sessionId);
  if (row) {
    row.title = title;
  }
  renderSessions();
  setTitle(title);
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

/** Put the selected conversation in the address bar, so a reload comes back
 * to it. `replaceState` when the URL already names this session, which is the
 * boot case: restoring what the URL asked for is not a navigation and should
 * not leave an extra entry behind for the back button to walk through.
 */
function rememberSessionInUrl(id: string | null): void {
  const next = searchForSession(window.location.search, id);
  if (next === window.location.search) {
    return;
  }
  const url = `${window.location.pathname}${next}${window.location.hash}`;
  window.history.pushState({ session: id }, "", url);
}

async function openSession(session: SessionSummary): Promise<void> {
  if (session.id === sessionId) {
    return;
  }
  rememberSessionInUrl(session.id);
  closeStream();
  resetTranscript();
  sessionId = session.id;
  setTitle(session.title || UNTITLED);
  markActiveSession();
  // Approval mode belongs to the session being opened, not to the one being
  // left. Until the server answers, the page shows the careful state.
  autoApprove = false;
  paintApprovalMode();
  void refreshApprovalMode();

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
  rememberSessionInUrl(null);
  sessionId = null;
  setTitle("New chat");
  markActiveSession();
  // A new chat asks again. Standing approvals down is something you do to a
  // run, and it does not follow you into the next one.
  autoApprove = false;
  paintApprovalMode();
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
  // Same argument for the context meter: a token count measured on one
  // conversation says nothing about the next one.
  forgetContextReading();
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

/**
 * Ask the server what this build can do, and draw it.
 *
 * Once, on connect. Build features and the search environment do not change
 * while the page is open. Voice readiness can change, so the microphone asks
 * again before recording. A failure leaves the indicators where they started:
 * a capability that could not be confirmed is not one to advertise.
 */
async function refreshCapabilities(): Promise<void> {
  try {
    const capabilities = await getCapabilities();
    renderSearchIndicator(searchIndicator, capabilities.web_search);
    voiceInput.observe(capabilities.voice);
  } catch {
    renderSearchIndicator(searchIndicator, null);
  }
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
    autoApprove
      ? "Auto-approve is on for this chat, so nothing will stop and ask you."
      : "Anything that changes your machine stops for your approval first.",
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

/**
 * Bring the sessions back, whichever way they went away.
 *
 * There are two of those and one button for both. On a phone the sidebar is
 * a drawer held by a class; on a wide window it is a grid column held by a
 * dataset key. Undoing only the one that applies would leave the other
 * waiting to surprise somebody who resized their window.
 */
function showSidebar(): void {
  dom.app.classList.add("sidebar-open");
  setSidebarCollapsed(dom.app, false, dom.menu);
  saveCollapsed(layoutStorage, false);
  describeHandles();
}

/**
 * Put them away.
 *
 * On a narrow window this shuts the drawer and stops there, because down
 * there the drawer being shut is the resting state and remembering it as a
 * collapse would hide the sidebar on the next wide window too.
 */
function hideSidebar(): void {
  closeSidebar();
  if (!isNarrow()) {
    setSidebarCollapsed(dom.app, true, dom.menu);
    saveCollapsed(layoutStorage, true);
    describeHandles();
  }
}

/** Shut the drawer and nothing else. What picking a session should do. */
function closeSidebar(): void {
  dom.app.classList.remove("sidebar-open");
}

/** Whether the stylesheet has the sidebar as a drawer rather than a column. */
function isNarrow(): boolean {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(max-width: 820px)").matches
    : false;
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
function glyph(kind: "shield" | "alert" | "stop"): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "glyph");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");

  // An outline and a mark inside it, for each of the three. The stop glyph is
  // the composer button's square inside a ring, so the card and the control
  // that produced it read as the same idea.
  const outline: Record<typeof kind, string> = {
    shield: "M12 3l7 3v5.5c0 4.2-2.9 7.9-7 9.5-4.1-1.6-7-5.3-7-9.5V6l7-3z",
    alert: "M12 4.5l8.5 15h-17l8.5-15z",
    stop: "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z",
  };
  const inner: Record<typeof kind, string> = {
    shield: "M12 8.5v4m0 3h.01",
    alert: "M12 10v3.5m0 3h.01",
    stop: "M9.75 9.75h4.5v4.5h-4.5z",
  };

  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", outline[kind]);
  svg.append(path);

  const mark = document.createElementNS("http://www.w3.org/2000/svg", "path");
  mark.setAttribute("d", inner[kind]);
  svg.append(mark);

  return svg;
}

/* ------------------------------------------------------------------ */
/* pane sizes and the collapsed sidebar                                */
/* ------------------------------------------------------------------ */

/**
 * Hand both pane edges to the reader.
 *
 * The two handles are aware of each other: how wide either pane may be
 * depends on how wide the other one is, because what they are really
 * dividing up is the room left over once the conversation has kept its
 * minimum. That is why the bounds are functions and not numbers.
 */
function wireLayout(): void {
  const saved = readLayout(layoutStorage);
  const root = dom.app;

  sidebarResizer = new PaneResizer({
    handle: dom.sidebarResizer,
    root,
    property: "--sidebar-w",
    sign: 1,
    bounds: () =>
      sidebarBounds({
        viewport: window.innerWidth,
        other: dom.artifacts.hidden ? 0 : (artifactsResizer?.current ?? 0),
      }),
    measure: () => dom.sidebar.getBoundingClientRect().width,
    onCommit: (px) => {
      saveWidth(layoutStorage, "sidebar", px);
      describeHandles();
    },
  });

  artifactsResizer = new PaneResizer({
    handle: dom.artifactsResizer,
    root,
    property: "--artifacts-w",
    sign: -1,
    bounds: () =>
      artifactsBounds({
        viewport: window.innerWidth,
        other: sidebarIsCollapsed(root) ? 0 : (sidebarResizer?.current ?? 0),
      }),
    measure: () => dom.artifacts.getBoundingClientRect().width,
    onCommit: (px) => {
      saveWidth(layoutStorage, "artifacts", px);
      describeHandles();
    },
  });

  if (saved.sidebar !== null) {
    sidebarResizer.set(saved.sidebar);
  }
  if (saved.artifacts !== null) {
    artifactsResizer.set(saved.artifacts);
  }
  setSidebarCollapsed(root, saved.collapsed, dom.menu);
  describeHandles();

  // A window that got smaller can leave a saved pane sitting over the
  // conversation, so both widths are put back inside their limits whenever
  // the window changes size.
  window.addEventListener("resize", () => {
    sidebarResizer?.reclamp();
    artifactsResizer?.reclamp();
  });
}

/**
 * Both handles, told where they stand.
 *
 * Both and not just the one that moved. Moving either edge changes how far
 * the other may go, and a gesture is a run of events rather than one, so the
 * only way to be sure the pair is right when the run ends is to write both
 * every time. A stale `aria-valuemax` is a limit announced to somebody that
 * is not the limit they will hit.
 */
function describeHandles(): void {
  sidebarResizer?.describe();
  artifactsResizer?.describe();
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
  get pdf() {
    return dom.artifactPdf;
  },
  get image() {
    return dom.artifactImage;
  },
  get empty() {
    return dom.artifactEmpty;
  },
};

function wireArtifacts(): void {
  // The Files button is a picker now, not a pane switch. The listing used to
  // sit on top of the pane and take a third of it, which was the third the
  // document wanted; the pane is a document reader and this is how you
  // choose the document.
  dom.artifactsBtn.addEventListener("click", () => {
    if (dom.filesPopover.hidden) {
      void openFilesMenu();
    } else {
      closeFilesMenu();
    }
  });
  dom.artifactsClose.addEventListener("click", closeArtifacts);
  dom.artifactsRefresh.addEventListener("click", () => {
    void refreshArtifacts();
  });
  document.addEventListener("click", (event) => {
    if (dom.filesPopover.hidden) {
      return;
    }
    // The button is inside the menu, so its own click is handled above and
    // this one sees it as inside and leaves it alone.
    const target = event.target;
    if (target instanceof Node && dom.filesMenu.contains(target)) {
      return;
    }
    closeFilesMenu();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !dom.filesPopover.hidden) {
      closeFilesMenu();
      dom.artifactsBtn.focus();
    }
  });
}

async function openFilesMenu(): Promise<void> {
  dom.filesPopover.hidden = false;
  dom.artifactsBtn.setAttribute("aria-expanded", "true");
  await refreshArtifacts();
}

function closeFilesMenu(): void {
  dom.filesPopover.hidden = true;
  dom.artifactsBtn.setAttribute("aria-expanded", "false");
}

/** Give the preview its column. Says nothing about which file is in it. */
function showArtifactsPane(): void {
  dom.artifacts.hidden = false;
  dom.app.dataset.artifacts = "open";
  describeHandles();
}

function closeArtifacts(): void {
  dom.artifacts.hidden = true;
  delete dom.app.dataset.artifacts;
  describeHandles();
}

/** The most recently written file this turn produced, if any. */
let newestProducedPath: string | null = null;

/** Back to knowing nothing about what any run wrote. */
function forgetProducedArtifacts(): void {
  producedThisTurn.clear();
  newestProducedPath = null;
  artifactsAtTurnStart = null;
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
 * The newest one is shown either way, and a closed pane opens to show it.
 *
 * It used to stay shut and put a count on the Files button instead, on the
 * reasoning that a pane appearing over a half-read answer is an interruption.
 * In use that was wrong in the ordinary case: asking for a document and
 * getting a small dot on a button reads as nothing having happened, and the
 * document sits there unread behind a click nobody knows to make. Somebody who
 * wants the pane out of the way can close it, and closing it is a smaller cost
 * than never finding the file.
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

  renderArtifactList(files, truncated);
  showArtifactsPane();
  await showArtifact(newestProducedPath);
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
      closeFilesMenu();
      showArtifactsPane();
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
 * matters is that anything which can execute (an SVG, an HTML file) goes into
 * the sandboxed iframe by URL and is never fetched into this page. Only the
 * types this page renders itself get read as text at all.
 */
async function showArtifact(path: string): Promise<void> {
  openArtifact = path;
  setArtifactTitle(path);
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

/**
 * Put the open file's name in the pane header.
 *
 * `textContent`, like everything else on this page that came out of the
 * workspace. A path is a name the agent chose, and a name the agent chose is
 * model output by another route.
 */
function setArtifactTitle(path: string | null): void {
  if (path === null) {
    dom.artifactTitle.dataset.empty = "yes";
    dom.artifactTitle.textContent = "Preview";
    dom.artifactTitle.removeAttribute("title");
    return;
  }
  delete dom.artifactTitle.dataset.empty;
  dom.artifactTitle.textContent = path;
  // The header ellipsises a long path, so the whole of it lives here too.
  dom.artifactTitle.title = path;
}

function setArtifactMessage(text: string): void {
  dom.artifactDoc.hidden = true;
  dom.artifactFrame.hidden = true;
  dom.artifactFrame.removeAttribute("src");
  dom.artifactPdf.hidden = true;
  dom.artifactPdf.removeAttribute("src");
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
