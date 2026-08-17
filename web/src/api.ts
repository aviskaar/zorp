/**
 * Typed client for the zorp-web HTTP API.
 *
 * The base URL comes from `window.ZORP_API_BASE`. When it is empty the UI
 * talks to its own origin. Setting it lets these static files be served by a
 * CDN or a second container while the server runs somewhere else, which is the
 * whole reason the server and the UI are separate artifacts.
 */

declare global {
  interface Window {
    /** Origin of the zorp-web server, for example "http://127.0.0.1:7777". */
    ZORP_API_BASE?: string;
    /** Shared secret, required by the server when it binds a non-loopback interface. */
    ZORP_API_TOKEN?: string;
  }
}

/** One row in the session sidebar. */
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

/** The turn failed. Always shown, never swallowed. */
export interface ErrorEvent {
  seq: number;
  type: "error";
  message: string;
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

export type ZorpEvent =
  | WorkingEvent
  | WorkingDoneEvent
  | ToolEvent
  | VerifyEvent
  | NoticeEvent
  | AssistantEvent
  | ApprovalRequestEvent
  | ErrorEvent
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
 */
export async function sendTurn(id: string, message: string): Promise<void> {
  try {
    await request<void>("POST", `/api/sessions/${segment(id)}/turn`, { message });
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      throw new TurnBusyError("a turn is already running on this session");
    }
    throw error;
  }
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

const EVENT_TYPES: ReadonlySet<string> = new Set<ZorpEventType>([
  "working",
  "working_done",
  "tool",
  "verify",
  "notice",
  "assistant",
  "approval_request",
  "error",
  "done",
]);

function isZorpEvent(value: unknown): value is ZorpEvent {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as { type?: unknown; seq?: unknown };
  return typeof candidate.type === "string" && EVENT_TYPES.has(candidate.type);
}
