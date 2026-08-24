/** One observed state from zorp's local Qwen3-ASR readiness poll. */
export interface VoiceWaitEvent {
  status: "waiting" | "ready" | "error";
  stage:
    | "creating_environment"
    | "installing"
    | "downloading_model"
    | "loading"
    | "ready"
    | "error";
  model: string;
  detail: string;
}

/** A JSON POST is deliberately not a cross-origin simple request. */
export function voiceWaitRequest(token: string): RequestInit {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (token) headers["authorization"] = `Bearer ${token}`;
  return { method: "POST", headers, body: "{}" };
}

/** Read readiness events and reject a connection that ends without a terminal state. */
export async function readVoiceWaitStream(
  body: ReadableStream<Uint8Array>,
  onEvent: (event: VoiceWaitEvent) => void,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  const consume = (finished: boolean): boolean => {
    const frames = buffer.split(/\r?\n\r?\n/);
    buffer = finished ? "" : (frames.pop() ?? "");
    for (const frame of frames) {
      const data = frame
        .split(/\r?\n/)
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (!data) continue;
      let parsed: unknown;
      try {
        parsed = JSON.parse(data);
      } catch {
        throw new Error("the voice readiness stream contained invalid JSON");
      }
      const record = parsed as Partial<VoiceWaitEvent>;
      if (
        (record.status !== "waiting" &&
          record.status !== "ready" &&
          record.status !== "error") ||
        (record.stage !== "creating_environment" &&
          record.stage !== "installing" &&
          record.stage !== "downloading_model" &&
          record.stage !== "loading" &&
          record.stage !== "ready" &&
          record.stage !== "error") ||
        typeof record.model !== "string" ||
        typeof record.detail !== "string"
      ) {
        throw new Error("the voice readiness stream contained an invalid event");
      }
      const event = record as VoiceWaitEvent;
      onEvent(event);
      if (event.status === "ready" || event.status === "error") return true;
    }
    return false;
  };

  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done });
    if (done) {
      buffer += "\n\n";
      if (consume(true)) return;
      throw new Error("the voice readiness stream ended before ready or error");
    }
    if (consume(false)) {
      await reader.cancel();
      return;
    }
  }
}
