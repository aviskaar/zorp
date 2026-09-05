/**
 * One run of consecutive activity lines, collapsed to a single line.
 *
 * A long turn's transcript is the answers, not the plumbing. The lines
 * `activity-line.ts` builds are kept exactly as they are and put under a
 * native `details`, whose summary says "Working on it" and the phrase of
 * the latest line while the turn is still adding to the group, and the
 * count once it is closed. A reader who wants the lines opens it; a group
 * that is being appended to never toggles itself.
 *
 * The phrase on the summary is read off the latest line and written back
 * through `textContent`, after the same clamp the line itself used, since
 * it is the model's own words or text derived from its command. Nothing
 * here assembles markup.
 *
 * Its own module, like `activity-line.ts`, because `main.ts` runs the whole
 * app on import and cannot be loaded from a test.
 */

// With the extension, so `node --test` can load this module without a
// bundler. `tsconfig.json` allows it and esbuild resolves it the same way.
import { clampPhrase } from "./activity-line.ts";

/** The summary while the turn is still adding lines. */
export const WORKING_LABEL = "Working on it";

/** The three states a line can carry, in the order they win. */
const STATES = ["activity-running", "activity-fail", "activity-ok"] as const;

export interface ActivityGroup {
  root: HTMLDetailsElement;
  /** Put a line under the group and say what it is doing. */
  append(line: HTMLElement): void;
  /** Nothing more is coming: the summary becomes the count. */
  close(): void;
}

export function activityGroup(doc: Document): ActivityGroup {
  const root = doc.createElement("details") as HTMLDetailsElement;
  root.className = "activity";
  const summary = doc.createElement("summary");
  summary.className = "activity-summary-line";
  summary.title = "Show what ran";
  const marker = doc.createElement("span");
  marker.className = "activity-summary-marker";
  marker.textContent = "▸";
  const label = doc.createElement("span");
  label.className = "activity-summary-label";
  label.textContent = WORKING_LABEL;
  const phrase = doc.createElement("span");
  phrase.className = "activity-summary-phrase";
  summary.append(marker, label, phrase);
  root.append(summary);

  const lines = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>(".activity-line"));

  // Any running line means running, else any failed line means failed. A
  // line carrying none of the three counts as ok.
  const paintState = (): void => {
    const all = lines();
    const state =
      STATES.find((name) => name !== "activity-ok" && all.some((line) => line.classList.contains(name))) ??
      "activity-ok";
    root.classList.remove(...STATES);
    root.classList.add(state);
  };

  return {
    root,
    append(line) {
      root.append(line);
      const latest = line.querySelector(".activity-brief, .activity-name");
      phrase.textContent = clampPhrase(latest?.textContent) ?? "";
      paintState();
    },
    close() {
      const all = lines();
      const failed = all.filter((line) => line.classList.contains("activity-fail")).length;
      label.textContent = `${all.length} ${all.length === 1 ? "step" : "steps"}${failed ? `, ${failed} failed` : ""}`;
      phrase.textContent = "";
      paintState();
    },
  };
}
