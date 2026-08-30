/**
 * Typed client for the zorp-web HTTP API.
 *
 * The base URL comes from `window.ZORP_API_BASE`. When it is empty the UI
 * talks to its own origin. Setting it lets these static files be served by a
 * CDN or a second container while the server runs somewhere else, which is the
 * whole reason the server and the UI are separate artifacts.
 */

// The one thing this module imports, and only because the fallback it names
// is a rule about the artifact pane rather than about HTTP. `artifact-view`
// imports nothing, so this cannot become a cycle.
import { textUrl } from "./artifact-view";
import {
  readVoiceWaitStream,
  voiceWaitRequest,
  type VoiceWaitEvent,
} from "./voice-readiness";

export type { VoiceWaitEvent } from "./voice-readiness";

declare global {
  interface Window {
    /** Origin of the zorp-web server, for example "http://127.0.0.1:7777". */
    ZORP_API_BASE?: string;
    /** Shared secret, required by the server when it binds a non-loopback interface. */
    ZORP_API_TOKEN?: string;
  }
}

/**
 * One row in the session sidebar.
 *
 * `title` is the short name a model wrote for the conversation when there
 * is one, and the verbatim first message when there is not. The server
 * decides which; the browser never sees the difference and never needs to.
 * Either way it is text nobody here wrote, so it reaches the page through
 * `textContent`.
 */
export interface SessionSummary {
  id: string;
  title: string;
  updated_at: string;
}

/** One persisted turn in a session transcript. */
export interface Message {
  role: "user" | "assistant";
  content: string;
}

export interface SessionTranscript {
  messages: Message[];
}

/**
 * Where an effective settings field's value came from: chosen in the
 * settings panel, read from the matching `ZORP_*` env var, or the
 * hardcoded fallback nobody asked for. Lets the panel say "from ZORP_MODEL"
 * instead of implying the user picked it.
 */
export type SettingsSource = "ui" | "env" | "default";

/**
 * Which wire protocol the configured endpoint speaks. Ollama is not a third
 * value here: it is "openai" pointed at a local base URL, offered in the
 * settings panel as a preset rather than a distinct provider.
 */
export type ModelProvider = "openai" | "anthropic";

/**
 * The effective model configuration, as `GET`/`PUT /api/settings` answer.
 * Settings live server-side; this is a read of what the server currently has,
 * not something the browser ships with every turn. There is no `api_key`
 * field here on purpose: the server never sends the key back out, only
 * `has_api_key`.
 */
export interface Settings {
  provider: ModelProvider;
  provider_source: SettingsSource;
  base_url: string;
  base_url_source: SettingsSource;
  model: string;
  model_source: SettingsSource;
  max_tokens: number | null;
  max_tokens_source: SettingsSource;
  has_api_key: boolean;
  api_key_source: SettingsSource;
  /**
   * False only when nothing at all is configured anywhere: no setting saved
   * through this panel, no `ZORP_*` env var, and no API key. That is exactly
   * the shape that used to fail silently on the first message, so the
   * composer checks this before letting anyone type.
   */
  configured: boolean;
}

/**
 * Body of `PUT /api/settings`. Every field is optional: a PUT only changes
 * what it names, leaving the rest of the stored settings alone. `api_key` is
 * sent once and never read back; an empty string clears the stored key.
 */
export interface SettingsUpdate {
  provider?: string;
  base_url?: string;
  model?: string;
  max_tokens?: number;
  api_key?: string;
}

/**
 * What `GET /api/settings/models` answers with. This endpoint is always 200:
 * an unreachable or non-JSON endpoint comes back as an empty list with
 * `error` explaining why, never a thrown request failure, so the panel can
 * fall back to the free-text model field instead of looking broken.
 */
export interface ModelsList {
  models: string[];
  error: string | null;
}

/** What `POST /api/settings/test` answers with. */
export interface ConnectionTestResult {
  ok: boolean;
  reason?: string;
}

/**
 * Whether one tool is there, and when it is not, why not.
 *
 * `detail` is written for a person: it names the missing feature or the
 * missing environment variable rather than a code.
 */
export interface ToolAvailability {
  available: boolean;
  detail: string;
}

/** What `GET /api/capabilities` answers with. */
export interface Capabilities {
  web_search: ToolAvailability;
  voice: VoiceStatus;
}

/** The server's observed local voice runtime state. */
export interface VoiceStatus {
  available: boolean;
  runtime_reachable: boolean;
  model_present: boolean;
  setup_available: boolean;
  endpoint: string | null;
  model: string | null;
  stage: VoiceWaitEvent["stage"] | null;
  detail: string;
}

/** Qwen3-ASR's editable text and detected language. */
export interface VoiceTranscription {
  text: string;
  language: string;
}

/** The agent started work. Drives the in-progress indicator. */
export interface WorkingEvent {
  seq: number;
  type: "working";
}

/** The agent stopped working. Pairs with `working`. */
export interface WorkingDoneEvent {
  seq: number;
  type: "working_done";
}

/** A tool ran. Rendered as the CLI's compact activity line. */
export interface ToolEvent {
  seq: number;
  type: "tool";
  name: string;
  summary: string;
}

/** A verification command ran and either passed or failed. */
export interface VerifyEvent {
  seq: number;
  type: "verify";
  command: string;
  passed: boolean;
}

/** Out of band commentary from the harness, not from the model. */
export interface NoticeEvent {
  seq: number;
  type: "notice";
  text: string;
}

/**
 * A fragment of the answer, as the model produces it.
 *
 * A preview. The `AssistantEvent` below is the server's one authoritative
 * statement of what the model said, and it replaces whatever these built.
 */
export interface AssistantDeltaEvent {
  seq: number;
  type: "assistant_delta";
  text: string;
}

/** Model output meant for the reader. */
export interface AssistantEvent {
  seq: number;
  type: "assistant";
  text: string;
}

/**
 * The agent is parked on a tool that needs a human decision. Nothing runs
 * until `approve` resolves it, and the server denies it on timeout.
 */
export interface ApprovalRequestEvent {
  seq: number;
  type: "approval_request";
  id: string;
  tool: string;
  arguments: string;
}

/** Where a context token count came from. */
export type ContextUsageSource = "reported" | "estimated";

/**
 * How full the model's context window is.
 *
 * `source` is not decoration. `reported` is what the provider said the last
 * request actually cost. `estimated` is zorp dividing byte lengths by four
 * because the provider said nothing. Showing them the same way would claim a
 * precision one of them does not have, so the meter marks the estimate.
 *
 * `limit_tokens` is absent when nobody has told zorp how large the window is,
 * which is the default: it talks to arbitrary OpenAI-compatible and Anthropic
 * endpoints, local Ollama included, and none of them can be asked. There is
 * then no denominator and the meter says so rather than inventing one.
 */
export interface ContextEvent {
  seq: number;
  type: "context";
  used_tokens: number;
  limit_tokens?: number;
  source: ContextUsageSource;
}

/**
 * This session now has a short, model-written name.
 *
 * Arrives after `done`, because the server asks for it once the turn has
 * an answer to read and never makes the turn wait. It arrives only when a
 * title was actually written: a call that failed, was refused, or was
 * declined sends nothing, and the row keeps showing the first message.
 *
 * `title` is model output. It goes on the page through `textContent`, the
 * same as every other line a model wrote.
 */
export interface SessionTitleEvent {
  seq: number;
  type: "session_title";
  title: string;
}

/** The turn failed. Always shown, never swallowed. */
export interface ErrorEvent {
  seq: number;
  type: "error";
  message: string;
}

/**
 * A human pressed stop and the run ended because of it.
 *
 * Not an `ErrorEvent`, because the reader is the one who caused it. `done`
 * still follows, the same as every other way a turn can end.
 */
export interface StoppedEvent {
  seq: number;
  type: "stopped";
}

/**
 * The turn finished. The session stays open for the next message, and so does
 * the stream. Closing the `EventSource` here would look tidy and would stop
 * the next turn from streaming at all.
 */
export interface DoneEvent {
  seq: number;
  type: "done";
}

/**
 * One message recalled out of an earlier conversation and quoted to the
 * model, with everything needed to say where it came from.
 *
 * `author` is "you" or "the assistant", spelled out by the server rather
 * than left as a role name. The difference between a thing the user wrote
 * and a thing a model wrote is the whole reason this is on the page: an
 * assistant line is a model's earlier output and not a checked fact.
 *
 * `text` is verbatim stored text, never a summary. Nothing in zorp asks a
 * model to write down what it learned from the corpus, so there is no
 * derived claim this could be carrying.
 */
export interface MemoryCitation {
  conversation_id: string;
  title: string;
  seq: number;
  author: string;
  /** `YYYY-MM-DD`, or empty when the store recorded no date. */
  when: string;
  text: string;
  /** Cosine similarity. Ranks this list and means nothing outside it. */
  score: number;
}

/**
 * What this turn recalled, sent before the model is called.
 *
 * Sent even when `used` is empty, because "memory was on and found
 * nothing" and "memory was off" look identical otherwise. `unavailable`
 * carries the reason a recall could not run, most often no local embedder;
 * the turn goes ahead without memory and this is the only thing that says
 * so.
 */
export interface MemoryEvent {
  seq: number;
  type: "memory";
  used: unknown;
  unavailable?: string | null;
}

/** How bad a reviewer thinks something is. */
export type Severity = "note" | "concern" | "blocking";

/** One thing one reviewer objected to. */
export interface PanelFinding {
  severity: Severity;
  claim: string;
  /**
   * Where in the material it applies. Free text, because a panel may be
   * reviewing a document, a diff or a record and a line number does not
   * fit all three. Corroboration is computed by matching these, server
   * side, so a reviewer that leaves it vague makes its own finding
   * harder to corroborate.
   */
  locus: string;
}

/** One locus and every lens that raised something about it. */
export interface Agreement {
  locus: string;
  /**
   * The lenses that raised it. Its length is the corroboration count,
   * and it counts lenses rather than findings so one reviewer listing
   * the same objection three times cannot corroborate itself.
   */
  lenses: string[];
  highest: Severity;
}

/**
 * One reviewer on a panel has started.
 *
 * A panel is several agents at once, so the page needs a per reviewer
 * signal rather than the single `working` a turn emits. Without it a
 * five reviewer panel shows one spinner and no sign that four already
 * finished.
 */
export interface ReviewerStartedEvent {
  seq: number;
  type: "reviewer_started";
  lens: string;
}

/** One reviewer came back with a readable verdict. */
export interface ReviewerFinishedEvent {
  seq: number;
  type: "reviewer_finished";
  lens: string;
  findings: PanelFinding[];
  /** The reviewer's whole answer, so a reader can see the reasoning. */
  answer: string;
}

/**
 * One reviewer did not come back with anything countable.
 *
 * Shown, never swallowed. A panel of five where two failed is not a
 * panel of three, and a page that only draws successes draws it as one.
 */
export interface ReviewerFailedEvent {
  seq: number;
  type: "reviewer_failed";
  lens: string;
  why: string;
}

/**
 * The panel finished.
 *
 * `complete` is the field that must reach the reader. Two of two
 * reviewers agreeing is a weaker claim than two of five, and the
 * corroboration count alone cannot tell them apart.
 */
export interface PanelDoneEvent {
  seq: number;
  type: "panel_done";
  target: string;
  lenses_requested: number;
  verdicts: number;
  complete: boolean;
  agreements: Agreement[];
}

/**
 * One Zorp mode attempt finished, meaning one `investigate` run.
 *
 * `approved` is whether the post-attempt checkpoint kept the track
 * alive. Absent means the attempt did not get that far, and an `error`
 * frame follows saying why. The frame is sent either way on purpose:
 * conditions are recorded before the work starts, so an attempt that
 * fell over still left something in the ledger.
 *
 * The ledger itself is not in here. It is read back through
 * `getLedger`, which the page can call again without running anything.
 */
export interface InvestigateDoneEvent {
  seq: number;
  type: "investigate_done";
  track_id: string;
  approved?: boolean;
}

export type ZorpEvent =
  | WorkingEvent
  | WorkingDoneEvent
  | ToolEvent
  | VerifyEvent
  | NoticeEvent
  | AssistantDeltaEvent
  | AssistantEvent
  | ApprovalRequestEvent
  | ContextEvent
  | MemoryEvent
  | SessionTitleEvent
  | ErrorEvent
  | StoppedEvent
  | ReviewerStartedEvent
  | ReviewerFinishedEvent
  | ReviewerFailedEvent
  | PanelDoneEvent
  | InvestigateDoneEvent
  | DoneEvent;

export type ZorpEventType = ZorpEvent["type"];

/** Anything the server answered with that was not a success. */
export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

/**
 * A turn is already running on this session. The server rejects the second one
 * rather than queueing it, because interleaved turns would corrupt the
 * transcript.
 */
export class TurnBusyError extends ApiError {
  constructor(message: string) {
    super(409, message);
    this.name = "TurnBusyError";
  }
}

/** Base URL with any trailing slashes removed, so path joins stay clean. */
export function apiBase(): string {
  const raw = typeof window === "undefined" ? "" : window.ZORP_API_BASE;
  return (raw ?? "").replace(/\/+$/, "");
}

function apiToken(): string {
  return (typeof window === "undefined" ? "" : window.ZORP_API_TOKEN) ?? "";
}

/**
 * Build a request URL. The token rides as a query parameter because
 * `EventSource` cannot set headers, and one mechanism that works everywhere
 * beats two that each work half the time. Fetches also send it as a bearer
 * header, which is what a server is most likely to check.
 */
function url(path: string): string {
  const full = apiBase() + path;
  const token = apiToken();
  if (!token) {
    return full;
  }
  const separator = full.includes("?") ? "&" : "?";
  return `${full}${separator}token=${encodeURIComponent(token)}`;
}

function segment(value: string): string {
  return encodeURIComponent(value);
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) {
    headers["content-type"] = "application/json";
  }
  const token = apiToken();
  if (token) {
    headers["authorization"] = `Bearer ${token}`;
  }

  let response: Response;
  try {
    response = await fetch(url(path), {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (cause) {
    const where = apiBase() || "this page's origin";
    throw new ApiError(0, `cannot reach the zorp server at ${where}`);
  }

  if (!response.ok) {
    const detail = (await response.text().catch(() => "")).trim();
    throw new ApiError(response.status, detail || `${method} ${path} failed with ${response.status}`);
  }

  const text = await response.text();
  if (!text) {
    return undefined as T;
  }
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new ApiError(response.status, `${method} ${path} returned a body that is not JSON`);
  }
}

/** Start a session. Returns its id. */
/** Whether the configured base URL is actually a zorp server.
 *
 * Worth checking on load. When the UI is served as static files the base URL
 * defaults to the page's own origin, so a plain file server answers, the UI
 * looks connected, and the first message returns that server's HTML error
 * page. Better to say the server is missing before anyone types.
 */
export async function serverIsReachable(): Promise<boolean> {
  try {
    const response = await fetch(url("/api/health"));
    if (!response.ok) return false;
    const contentType = response.headers.get("content-type") ?? "";
    if (!contentType.includes("application/json")) return false;
    const body = (await response.json()) as { status?: string };
    return body.status === "ok";
  } catch {
    return false;
  }
}

export async function newSession(): Promise<string> {
  const created = await request<{ id: string }>("POST", "/api/sessions", {});
  return created.id;
}

/** List sessions, newest first as the server orders them. */
export async function listSessions(): Promise<SessionSummary[]> {
  const sessions = await request<SessionSummary[]>("GET", "/api/sessions");
  return Array.isArray(sessions) ? sessions : [];
}

/** Replay a stored conversation. */
export async function getSession(id: string): Promise<SessionTranscript> {
  const transcript = await request<SessionTranscript>("GET", `/api/sessions/${segment(id)}`);
  return { messages: Array.isArray(transcript?.messages) ? transcript.messages : [] };
}

/**
 * Send a user message. The server answers 202 and the result arrives on the
 * event stream. A 409 means a turn is already running.
 *
 * `memory` asks the server to read earlier conversations before answering
 * this one. It defaults to off and is passed per message rather than held
 * per session, because a recall spends context and puts text from old
 * conversations, tool results and fetched pages included, in front of the
 * model. That is a thing to choose each time, not a mode to leave on.
 */
export async function sendTurn(id: string, message: string, memory = false): Promise<void> {
  try {
    await request<void>("POST", `/api/sessions/${segment(id)}/turn`, { message, memory });
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      throw new TurnBusyError("a turn is already running on this session");
    }
    throw error;
  }
}

/**
 * Nothing was running, so nothing was stopped.
 *
 * Worth its own type. The browser only offers a stop control while it thinks a
 * turn is live, so being told otherwise means its idea of the session is out of
 * date, and the fix is to go back to idle rather than to show an error about a
 * turn that already ended.
 */
export class NothingRunningError extends ApiError {
  constructor(message: string) {
    super(409, message);
    this.name = "NothingRunningError";
  }
}

/**
 * Ask the server to stop the running turn. It answers 202 and the run ends a
 * moment later, on the event stream, with `stopped` and then `done`. This
 * resolving is not the end of the turn and must not be treated as one.
 */
export async function stopTurn(id: string): Promise<void> {
  try {
    await request<void>("POST", `/api/sessions/${segment(id)}/stop`);
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      throw new NothingRunningError("no turn is running on this session");
    }
    throw error;
  }
}

/** One reviewing angle a panel can be built from. */
export interface PanelLens {
  name: string;
  /** What a reviewer on this lens is told. Shown so the reader can see
   * what they are asking for rather than only its name. */
  instruction: string;
}

/**
 * The lenses a panel can be built from.
 *
 * Read from the server, never composed here. The browser chooses from a
 * code-defined set and cannot define a lens: a page that could send
 * instructions could send one reviewer the answer it wanted, and the
 * reviewers are supposed to be independent of everything except the
 * material.
 */
export async function getLenses(): Promise<PanelLens[]> {
  const body = await request<{ lenses: PanelLens[] }>("GET", "/api/panel/lenses");
  return body.lenses;
}

/**
 * Launch a review panel over `body`.
 *
 * Answers 202 and the reviewers report on the event stream, the same one
 * a turn uses, ending with `panel_done` and then `done`. Like a turn, it
 * occupies the session: a 409 means one is already running.
 *
 * An empty target is a 400 rather than five agents confidently reviewing
 * nothing at the cost of five requests.
 */
export async function startPanel(
  id: string,
  label: string,
  body: string,
  lenses: string[],
): Promise<void> {
  try {
    await request<void>("POST", `/api/sessions/${segment(id)}/panel`, {
      label,
      body,
      lenses,
    });
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      throw new TurnBusyError("a turn is already running on this session");
    }
    throw error;
  }
}

/**
 * What this server can actually do, as opposed to what it was asked to do.
 *
 * Read from the server, never inferred here. Whether `web_search` exists
 * depends on a compile-time feature, on the policy, and on a key in the
 * server's environment, and a page can see none of the three.
 *
 * A server too old to know the route, or one answering something unexpected,
 * comes back as unavailable rather than as an error: an indicator that
 * cannot be shown is not worth an error card, and claiming a capability on a
 * malformed answer is the one outcome to avoid.
 */
export async function getCapabilities(): Promise<Capabilities> {
  const body = await request<Partial<Record<string, unknown>>>("GET", "/api/capabilities");
  return {
    web_search: toolAvailability(body?.["web_search"]),
    voice: voiceStatus(body?.["voice"]),
  };
}

function toolAvailability(value: unknown): ToolAvailability {
  const record = (value ?? {}) as Record<string, unknown>;
  const available = record["available"] === true;
  const detail = typeof record["detail"] === "string" ? record["detail"] : "";
  return { available, detail };
}

function voiceStatus(value: unknown): VoiceStatus {
  const record = (value ?? {}) as Record<string, unknown>;
  return {
    available: record["available"] === true,
    runtime_reachable: record["runtime_reachable"] === true,
    model_present: record["model_present"] === true,
    setup_available: record["setup_available"] === true,
    endpoint: typeof record["endpoint"] === "string" ? record["endpoint"] : null,
    model: typeof record["model"] === "string" ? record["model"] : null,
    stage: voiceStage(record["stage"]),
    detail: typeof record["detail"] === "string" ? record["detail"] : "Voice input is unavailable.",
  };
}

export async function getVoiceStatus(): Promise<VoiceStatus> {
  const value = await request<unknown>("GET", "/api/voice/status");
  return voiceStatus(value);
}

/** Wait for the local runtime to report that its configured model is ready. */
export async function waitForVoiceModel(onEvent: (event: VoiceWaitEvent) => void): Promise<void> {
  let terminalError = "";
  await readVoiceStream("/api/voice/wait", "wait for the voice model", (event) => {
    onEvent(event);
    if (event.status === "error") terminalError = event.detail;
  });
  if (terminalError) throw new ApiError(0, terminalError);
}

async function readVoiceStream(
  path: string,
  action: string,
  onEvent: (event: VoiceWaitEvent) => void,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(url(path), voiceWaitRequest(apiToken()));
  } catch {
    throw new ApiError(0, `cannot reach the zorp server to ${action}`);
  }
  if (!response.ok) {
    const detail = (await response.text().catch(() => "")).trim();
    throw new ApiError(response.status, detail || `could not ${action}`);
  }
  if (!response.body) throw new ApiError(response.status, "the voice readiness stream was empty");
  await readVoiceWaitStream(response.body, onEvent);
}

function voiceStage(value: unknown): VoiceWaitEvent["stage"] | null {
  return value === "creating_environment" ||
    value === "installing" ||
    value === "downloading_model" ||
    value === "loading" ||
    value === "ready" ||
    value === "error"
    ? value
    : null;
}

export async function transcribeVoice(recording: Blob): Promise<VoiceTranscription> {
  const headers: Record<string, string> = {
    "content-type": recording.type || "audio/webm",
  };
  const token = apiToken();
  if (token) headers["authorization"] = `Bearer ${token}`;
  let response: Response;
  try {
    response = await fetch(url("/api/voice/transcribe"), {
      method: "POST",
      headers,
      body: recording,
    });
  } catch {
    throw new ApiError(0, "cannot reach the zorp server to transcribe the recording");
  }
  const body = await response.text();
  if (!response.ok) throw new ApiError(response.status, body.trim() || "voice transcription failed");
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    throw new ApiError(response.status, "voice transcription returned invalid JSON");
  }
  const record = parsed as Partial<VoiceTranscription>;
  if (typeof record.text !== "string" || typeof record.language !== "string") {
    throw new ApiError(response.status, "voice transcription returned no text or language");
  }
  return { text: record.text, language: record.language };
}

/**
 * Whether Zorp mode can run on this server, and whether it forecasts.
 *
 * `available` is what the server binary was built with: the research
 * feature is opt-in, so an ordinary chat server answers false and the
 * page can say why the control is off.
 *
 * `forecasting` reads the server's `ZORP_FORECAST`. It is reported and
 * never set from here. A forecast costs an extra model call on every
 * attempt, it is off by default, and one browser flipping it would
 * change what the whole server does for everyone using it.
 */
export interface InvestigateStatus {
  available: boolean;
  forecasting: boolean;
}

export async function getInvestigateStatus(): Promise<InvestigateStatus> {
  return request<InvestigateStatus>("GET", "/api/investigate/status");
}

/** The pre-registration a first attempt on a track has to commit to. */
export interface Preregistration {
  metric_name: string;
  kill_threshold: number;
  threshold_direction: "lower-is-better" | "higher-is-better";
}

/**
 * Run one pre-registered `investigate` attempt on this session.
 *
 * There is no aryabhatta engine to call: aryabhatta is a record plus
 * readers and ships no command. `investigate` is what writes to it, so
 * this is what Zorp mode runs, and `getLedger` is what reads back what
 * landed.
 *
 * Answers 202 and reports on the event stream, the same one a turn uses,
 * ending with `investigate_done` and then `done`. Like a turn it
 * occupies the session: a 409 means one is already running.
 *
 * `prereg` is required on the first attempt for a question and must
 * match the record on every later one. Leaving it out means "use what is
 * already recorded".
 */
export async function startInvestigate(
  id: string,
  question: string,
  prereg: Preregistration | null,
): Promise<void> {
  try {
    await request<void>("POST", `/api/sessions/${segment(id)}/investigate`, {
      question,
      metric_name: prereg?.metric_name ?? null,
      kill_threshold: prereg?.kill_threshold ?? null,
      threshold_direction: prereg?.threshold_direction ?? null,
    });
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      throw new TurnBusyError("a turn is already running on this session");
    }
    throw error;
  }
}

/** One input an attempt was recorded as having run under. */
export interface LedgerCondition {
  key: string;
  value: string;
}

/**
 * One forecast, as recorded before the attempt ran.
 *
 * There is no `assumptions` field and there must not be one. It is the
 * one model-authored text column on that table, and no read path names
 * it, which is what keeps the subsystem's integrity rules cheap to
 * check.
 */
export interface LedgerExpectation {
  metric_key: string;
  expected_value: number;
  interval_low: number;
  interval_high: number;
  confidence: number;
}

/** One recorded outcome. */
export interface LedgerMetric {
  key: string;
  value: string;
}

/** One attempt, with what went in and what came out. */
export interface LedgerExperiment {
  id: string;
  status: string;
  conditions: LedgerCondition[];
  expectations: LedgerExpectation[];
  metrics: LedgerMetric[];
}

/**
 * A track's whole recorded ledger.
 *
 * `present` separates two things that must not look the same on the
 * page: an empty ledger, which is the honest state for a record nobody
 * has fed, and no run record at all.
 */
export interface Ledger {
  track_id: string;
  present: boolean;
  forecasting: boolean;
  experiments: LedgerExperiment[];
}

/**
 * Read back what a question's attempts recorded.
 *
 * A read and nothing else. It runs no attempt, asks no model anything,
 * and creates no run record that is not already there.
 */
export async function getLedger(question: string): Promise<Ledger> {
  return request<Ledger>("GET", `/api/investigate/ledger?question=${segment(question)}`);
}

/** Read the effective model settings and where each field came from. */
export async function getSettings(): Promise<Settings> {
  return request<Settings>("GET", "/api/settings");
}

/**
 * Save a settings change. Rejects (via `ApiError`, status 400) when the
 * server does not recognize the provider string, so the panel can show that
 * inline instead of the save silently doing nothing.
 */
export async function putSettings(update: SettingsUpdate): Promise<Settings> {
  return request<Settings>("PUT", "/api/settings", update);
}

/**
 * List model ids the given base URL serves, OpenAI's `/models` shape. Never
 * throws on an unreachable or misbehaving endpoint: the server always
 * answers 200, with `error` set and `models` empty, so the panel can fall
 * back to its free-text model field instead of showing a request failure for
 * what is usually just "Ollama is not running yet."
 */
export async function listModels(baseUrl: string, apiKey?: string): Promise<ModelsList> {
  // POST, with the key (when there is one) in the body: a secret in a query
  // string is a secret in URLs. The server falls back to its stored key
  // when none is sent, so a reopened panel with a blank key field still
  // lists from a protected endpoint.
  const body: { base_url: string; api_key?: string } = { base_url: baseUrl };
  if (apiKey) {
    body.api_key = apiKey;
  }
  const result = await request<Partial<ModelsList>>("POST", "/api/settings/models", body);
  return {
    models: Array.isArray(result?.models) ? result.models : [],
    error: typeof result?.error === "string" ? result.error : null,
  };
}

/** Check that the currently saved settings actually reach a server. */
export async function testConnection(
  baseUrl?: string,
  apiKey?: string,
): Promise<ConnectionTestResult> {
  // With a base URL, the server probes that candidate and stores nothing.
  // Without one, it probes whatever is already saved. A key typed but not
  // yet saved rides along the same way, so Test checks what is on screen.
  const body: { base_url?: string; api_key?: string } = {};
  if (baseUrl) {
    body.base_url = baseUrl;
  }
  if (apiKey) {
    body.api_key = apiKey;
  }
  return request<ConnectionTestResult>(
    "POST",
    "/api/settings/test",
    Object.keys(body).length > 0 ? body : undefined,
  );
}

/** Resolve a pending approval. Nothing here ever decides on the user's behalf. */
export async function approve(
  sessionId: string,
  approvalId: string,
  allow: boolean,
): Promise<void> {
  await request<void>("POST", `/api/sessions/${segment(sessionId)}/approve`, {
    id: approvalId,
    allow,
  });
}

/**
 * Stand this session's approvals down, or put them back up.
 *
 * The server owns this: it answers with the state it now holds, and that
 * answer is what the page draws. Nothing here assumes the request worked.
 *
 * It is per session and it is not stored anywhere, so a new chat asks again
 * and so does a restarted server. What it never does is widen what the agent
 * is allowed to attempt: the hard denylist refuses the same commands either
 * way, because the policy decides before anyone is asked anything.
 */
export async function setAutoApprove(sessionId: string, on: boolean): Promise<boolean> {
  const result = await request<{ auto_approve?: unknown }>(
    "POST",
    `/api/sessions/${segment(sessionId)}/auto-approve`,
    { on },
  );
  return result?.auto_approve === true;
}

/** What the server says this session is currently doing about approvals. */
export async function getAutoApprove(sessionId: string): Promise<boolean> {
  const result = await request<{ auto_approve?: unknown }>(
    "GET",
    `/api/sessions/${segment(sessionId)}/auto-approve`,
  );
  return result?.auto_approve === true;
}

export type StreamStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface EventStream {
  /** Resolves the first time the stream connects. */
  readonly opened: Promise<void>;
  /** Stop listening. Safe to call more than once. */
  close(): void;
}

/**
 * Subscribe to a session's event stream.
 *
 * The server holds the response open for the life of the session, so this
 * connects once and stays connected across turns. `EventSource` reconnects on
 * its own and replays the last event id it saw, so a connection that really
 * drops resumes rather than losing the middle of a turn. That automatic
 * reconnect is also why the server must not end the response when a turn
 * finishes: the browser would simply open it again, forever.
 *
 * Frames that do not parse are surfaced as error events instead of being
 * dropped, because a chat UI that stalls without saying why is the worst
 * failure here.
 */
export function streamEvents(
  sessionId: string,
  onEvent: (event: ZorpEvent) => void,
  onStatus?: (status: StreamStatus) => void,
): EventStream {
  const source = new EventSource(url(`/api/sessions/${segment(sessionId)}/events`));
  let closed = false;

  let markOpened!: () => void;
  const opened = new Promise<void>((resolve) => {
    markOpened = resolve;
  });

  onStatus?.("connecting");

  source.onopen = () => {
    markOpened();
    if (!closed) {
      onStatus?.("open");
    }
  };

  source.onmessage = (message: MessageEvent<string>) => {
    if (closed) {
      return;
    }
    const seq = Number(message.lastEventId);
    let parsed: unknown;
    try {
      parsed = JSON.parse(message.data);
    } catch {
      onEvent({
        seq: Number.isFinite(seq) ? seq : 0,
        type: "error",
        message: "the server sent an event that could not be read",
      });
      return;
    }
    if (!isZorpEvent(parsed)) {
      onEvent({
        seq: Number.isFinite(seq) ? seq : 0,
        type: "error",
        message: "the server sent an event with no recognisable type",
      });
      return;
    }
    onEvent(parsed);
  };

  source.onerror = () => {
    if (closed) {
      return;
    }
    // A browser that gives up sets CLOSED. Anything else is a retry in flight.
    onStatus?.(source.readyState === EventSource.CLOSED ? "closed" : "reconnecting");
  };

  return {
    opened,
    close() {
      if (closed) {
        return;
      }
      closed = true;
      source.close();
      onStatus?.("closed");
    },
  };
}

/**
 * Every event type the browser will accept.
 *
 * A record keyed by the union rather than `new Set<ZorpEventType>([...])`,
 * because that form happily accepts a *subset*. Adding `assistant_delta` to
 * the union above and forgetting it here typechecked cleanly and then
 * rejected every streamed fragment at runtime as an unrecognised event. A
 * record makes a missing variant a compile error instead.
 */
const EVENT_TYPES_BY_NAME: Record<ZorpEventType, true> = {
  working: true,
  working_done: true,
  tool: true,
  verify: true,
  notice: true,
  assistant_delta: true,
  assistant: true,
  approval_request: true,
  context: true,
  memory: true,
  session_title: true,
  error: true,
  stopped: true,
  reviewer_started: true,
  reviewer_finished: true,
  reviewer_failed: true,
  panel_done: true,
  investigate_done: true,
  done: true,
};

const EVENT_TYPES: ReadonlySet<string> = new Set(Object.keys(EVENT_TYPES_BY_NAME));

function isZorpEvent(value: unknown): value is ZorpEvent {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as { type?: unknown; seq?: unknown };
  return typeof candidate.type === "string" && EVENT_TYPES.has(candidate.type);
}

/** One file the artifact pane can open. */
export interface Artifact {
  path: string;
  bytes: number;
  /**
   * Last modified, milliseconds since the epoch, or 0 when the server could
   * not tell. This is what lets the pane notice that a run wrote something:
   * it snapshots the listing when a turn starts and compares afterwards. Size
   * alone would miss a rewrite that landed on the same length.
   */
  modified_ms: number;
}

export interface ArtifactListing {
  files: Artifact[];
  /** The server capped the list. Saying so beats implying it is complete. */
  truncated: boolean;
}

export async function listArtifacts(): Promise<ArtifactListing> {
  return request<ArtifactListing>("GET", "/api/artifacts");
}

/**
 * The address of one artifact. Also what the sandboxed iframe navigates to
 * for the types that only ever load there. The token, if there is one, rides
 * along via `url`.
 */
export function artifactUrl(path: string): string {
  return url(`/api/artifacts/raw?path=${encodeURIComponent(path)}`);
}

/**
 * The text of an artifact, for the markdown renderer.
 *
 * Only ever called for the types the pane renders itself. A `.svg`, a
 * `.html` or a `.pdf` is never fetched into the page: those are addressed by
 * URL from a frame, so the browser loads them somewhere this page cannot be
 * reached from. The office formats are read on the server, so what this
 * fetches for them is text and never the file. See `artifact-view.ts`.
 *
 * `as=text` is what asks the server for the words in a document rather than
 * the document. It is the fallback path for a PDF, on a browser with no
 * viewer to hand the file to, and it changes nothing for any other type.
 */
export async function readArtifact(path: string): Promise<string> {
  const response = await fetch(textUrl(artifactUrl(path)), { headers: authHeaders() });
  if (!response.ok) {
    throw new ApiError(response.status, (await response.text()) || response.statusText);
  }
  return response.text();
}

function authHeaders(): Record<string, string> {
  const token = apiToken();
  return token ? { authorization: `Bearer ${token}` } : {};
}

/* ------------------------------------------------------------------ */
/* conversation search                                                 */
/* ------------------------------------------------------------------ */

/**
 * Whether this server can search the conversations it holds.
 *
 * Answered by every build, including one compiled without the feature, so
 * the page can hide the search box for a reason it can name instead of
 * hiding it because a request 404'd.
 */
export interface RecallStatus {
  available: boolean;
  /** Why not, in the server's own words. Null when it is available. */
  reason: string | null;
  /** The loopback endpoint the vectors would come from. */
  endpoint: string | null;
  model: string | null;
  /** Conversations represented in the derived index. */
  conversations: number;
  /** Conversations in the source store. Missing on older servers. */
  store_conversations?: number;
  chunks: number;
  /** Automatic indexer state. Missing on older servers. */
  running?: boolean;
  ready?: boolean;
  /**
   * Whether a turn can be told to read this index, as opposed to only the
   * sidebar being able to search it. A separate build-time choice, so the
   * page asks rather than assuming that one implies the other.
   */
  memory: boolean;
}

/** One conversation that matched, and the message in it that matched best. */
export interface RecallHit {
  id: string;
  title: string;
  seq: number;
  role: string;
  snippet: string;
  /** Cosine similarity. Ranks this result list and means nothing outside it. */
  score: number;
}

export async function recallStatus(): Promise<RecallStatus> {
  return request<RecallStatus>("GET", "/api/recall/status");
}

/**
 * Search. Returns the raw rows: `conversation-search.ts` is what checks
 * their shape, because it is the thing that puts them on the page.
 */
export async function recallSearch(query: string, limit?: number): Promise<unknown> {
  const cap = limit === undefined ? "" : `&limit=${encodeURIComponent(String(limit))}`;
  const body = await request<{ hits?: unknown }>(
    "GET",
    `/api/recall/search?q=${encodeURIComponent(query)}${cap}`,
  );
  return body?.hits;
}
