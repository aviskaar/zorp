/**
 * Reading answers aloud, through the local speech runtime.
 *
 * The browser does not decide whether speech is possible: it draws what
 * `GET /api/capabilities` reported and nothing else. Speaking is something a
 * person turns on, per answer with the speaker button or for the whole
 * conversation with the read-aloud toggle. No model starts or stops it.
 *
 * Long text is split into chunks by the server (the one chunker in
 * `zorp-voice`), each chunk is synthesized in turn, and playback starts on
 * the first chunk while the next is fetched, so a long answer begins speaking
 * quickly. Everything reaching the page does so through `textContent`; this
 * module builds no HTML strings.
 *
 * The audio itself stays on this machine: the server refuses any speech
 * endpoint that is not on loopback, so the bytes played here came from the
 * local runtime and nowhere else.
 */

import { speak, speechChunks, type SpeechStatus } from "./api.ts";

export const SPEAK_LABEL = "Speak";
export const STOP_LABEL = "Stop";

/** How the player makes audio, swappable so tests need no real `Audio`. */
export type PlayBlob = (blob: Blob, onEnded: () => void) => { stop(): void };

function browserPlayer(blob: Blob, onEnded: () => void): { stop(): void } {
  const objectUrl = URL.createObjectURL(blob);
  const audio = new Audio(objectUrl);
  const cleanup = () => {
    URL.revokeObjectURL(objectUrl);
    onEnded();
  };
  audio.addEventListener("ended", cleanup, { once: true });
  audio.addEventListener("error", cleanup, { once: true });
  void audio.play().catch(cleanup);
  return {
    stop() {
      audio.pause();
      URL.revokeObjectURL(objectUrl);
    },
  };
}

/**
 * Plays an answer as speech, one chunk at a time. One player for the page, so
 * starting a new answer, or pressing stop anywhere, ends whatever was
 * playing.
 */
export class SpeechPlayer {
  private ok = false;
  private model: string | null = null;
  private voice: string | null = null;
  /** Bumped on every `speak` and `stop`, so an in-flight run notices it was
   * superseded and stops fetching or playing. */
  private token = 0;
  private current: { stop(): void } | null = null;
  private resolveChunk: (() => void) | null = null;
  private stateListeners = new Set<(speaking: boolean) => void>();
  private readAloud = false;
  private play: PlayBlob;

  constructor(play: PlayBlob = browserPlayer) {
    this.play = play;
  }

  /** Reflect the server's answer. All three facts must hold to speak. */
  observe(status: SpeechStatus | null | undefined): void {
    this.ok = !!status && status.available && status.runtime_reachable && status.model_present;
    this.model = status?.model ?? null;
    this.voice = status?.voice ?? null;
    if (!this.ok) {
      this.stop();
    }
  }

  available(): boolean {
    return this.ok;
  }

  speaking(): boolean {
    return this.current !== null;
  }

  /** Whether the read-aloud mode is on; set by the composer toggle. */
  isReadAloud(): boolean {
    return this.readAloud && this.ok;
  }

  setReadAloud(on: boolean): void {
    this.readAloud = on;
    if (!on) {
      this.stop();
    }
  }

  onState(listener: (speaking: boolean) => void): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  private notify(speaking: boolean): void {
    for (const listener of this.stateListeners) {
      listener(speaking);
    }
  }

  /** Stop whatever is playing and cancel any in-flight synthesis. */
  stop(): void {
    this.token += 1;
    if (this.current) {
      this.current.stop();
      this.current = null;
    }
    if (this.resolveChunk) {
      const resolve = this.resolveChunk;
      this.resolveChunk = null;
      resolve();
    }
    this.notify(false);
  }

  /** Speak `text`, replacing anything already playing. */
  async speak(text: string): Promise<void> {
    this.stop();
    if (!this.ok) {
      return;
    }
    const mine = (this.token += 1);
    const trimmed = text.trim();
    if (!trimmed) {
      return;
    }
    let chunks: string[];
    try {
      chunks = await speechChunks(trimmed);
    } catch {
      chunks = [trimmed];
    }
    if (chunks.length === 0) {
      chunks = [trimmed];
    }
    this.notify(true);
    for (const chunk of chunks) {
      if (mine !== this.token) {
        return;
      }
      let blob: Blob;
      try {
        blob = await speak(chunk, this.model ?? undefined, this.voice ?? undefined);
      } catch {
        break;
      }
      if (mine !== this.token) {
        return;
      }
      await new Promise<void>((resolve) => {
        this.resolveChunk = resolve;
        this.current = this.play(blob, () => {
          this.current = null;
          if (this.resolveChunk === resolve) {
            this.resolveChunk = null;
          }
          resolve();
        });
      });
    }
    if (mine === this.token) {
      this.current = null;
      this.notify(false);
    }
  }
}

/**
 * A speaker button for the row under an answer. Toggles between speaking this
 * answer and stopping. Hidden when the runtime is not available, so an answer
 * never offers a control that would do nothing.
 */
export function speakerButton(
  doc: Document,
  player: SpeechPlayer,
  text: () => string,
): HTMLButtonElement {
  const button = doc.createElement("button");
  button.type = "button";
  button.className = "answer-action speak-action";
  button.hidden = !player.available();
  const label = (speaking: boolean) => {
    button.textContent = speaking ? STOP_LABEL : SPEAK_LABEL;
    button.setAttribute("aria-pressed", speaking ? "true" : "false");
  };
  label(false);
  // One owned flag so this button only redraws for its own playback, not for
  // another answer's.
  let mine = false;
  player.onState((speaking) => {
    if (!speaking) {
      mine = false;
    }
    label(speaking && mine);
  });
  button.addEventListener("click", () => {
    if (player.speaking() && mine) {
      player.stop();
      return;
    }
    mine = true;
    void player.speak(text());
  });
  return button;
}
