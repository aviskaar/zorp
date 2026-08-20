/**
 * The context meter: how much of the model's window is left.
 *
 * Two honesty rules run through everything here.
 *
 * The first is that a reported number and an estimated one are different
 * kinds of fact. `reported` is what the provider said the last request cost.
 * `estimated` is zorp dividing byte lengths by four because the provider said
 * nothing, which many local endpoints do. The estimate is marked with a
 * tilde and says so in its own words on hover; it is never dressed up as a
 * measurement.
 *
 * The second is that zorp usually does not know how large the window is. It
 * talks to arbitrary OpenAI-compatible and Anthropic endpoints and none of
 * them can be asked, so there is no default to fall back on that would not be
 * wrong for somebody. With no window configured there is no denominator, so
 * the meter shows the tokens and says plainly that the window is unset,
 * rather than drawing a bar against a number nobody supplied.
 *
 * Everything reaching the page goes through `textContent`. This module builds
 * no HTML strings, for the same reason `markdown.ts` does not.
 */

export type ContextUsageSource = "reported" | "estimated";

export interface ContextReading {
  used_tokens: number;
  limit_tokens?: number;
  source: ContextUsageSource;
}

/** How full the window is, as far as anyone can honestly say. */
export type MeterState = "unknown" | "ok" | "warn" | "full";

export interface MeterView {
  /** The short text on the meter itself. */
  label: string;
  /** The long form, shown on hover and to assistive technology. */
  detail: string;
  /** Share of the window used, or null when the window is unknown. */
  fraction: number | null;
  state: MeterState;
}

/** Above this share of the window, the meter starts warning. */
const WARN_AT = 0.75;
/** Above this, it reads as full. */
const FULL_AT = 0.9;

/** Compact token counts: 812, 12.4k, 1.2M. */
export function formatTokens(tokens: number): string {
  const n = Math.max(0, Math.round(tokens));
  if (n < 1000) {
    return String(n);
  }
  if (n < 1_000_000) {
    return `${trimZero(n / 1000)}k`;
  }
  return `${trimZero(n / 1_000_000)}M`;
}

function trimZero(value: number): string {
  const fixed = value.toFixed(1);
  return fixed.endsWith(".0") ? fixed.slice(0, -2) : fixed;
}

function withThousands(tokens: number): string {
  return Math.max(0, Math.round(tokens)).toLocaleString("en-US");
}

/**
 * What the meter should say for one reading.
 *
 * Pure, so the wording and the thresholds can be tested without a DOM.
 */
export function meterView(reading: ContextReading): MeterView {
  const estimated = reading.source === "estimated";
  const used = Math.max(0, Math.round(reading.used_tokens));
  const limit =
    typeof reading.limit_tokens === "number" && reading.limit_tokens > 0
      ? Math.round(reading.limit_tokens)
      : null;

  const provenance = estimated
    ? "This model reported no token usage, so zorp estimated it from the transcript. It is a rough guide, not a measurement."
    : "Reported by the model for the last request it answered.";

  if (limit === null) {
    return {
      label: `${estimated ? "~" : ""}${formatTokens(used)} sent`,
      detail:
        `${withThousands(used)} tokens in the last request. ` +
        "No context window is configured, so there is no percentage to show. " +
        "Set ZORP_CONTEXT_TOKENS to your model's window and restart zorp-web. " +
        provenance,
      fraction: null,
      state: "unknown",
    };
  }

  const fraction = Math.min(1, used / limit);
  const left = Math.max(0, Math.round((1 - fraction) * 100));
  return {
    label: `${estimated ? "~" : ""}${left}% left`,
    detail:
      `${withThousands(used)} of ${withThousands(limit)} tokens used, ` +
      `${left}% of the window left. ` +
      "The window comes from ZORP_CONTEXT_TOKENS, not from the model. " +
      provenance,
    fraction,
    state: fraction >= FULL_AT ? "full" : fraction >= WARN_AT ? "warn" : "ok",
  };
}

/** The elements the meter writes into. */
export interface MeterElements {
  root: HTMLElement;
  fill: HTMLElement;
  text: HTMLElement;
}

/** Draw one reading. Text only ever arrives through `textContent`. */
export function showMeter(elements: MeterElements, reading: ContextReading): MeterView {
  const view = meterView(reading);
  elements.root.hidden = false;
  elements.root.dataset.state = view.state;
  elements.root.title = view.detail;
  elements.root.setAttribute("aria-label", `Context window: ${view.detail}`);
  elements.text.textContent = view.label;
  elements.fill.style.width = view.fraction === null ? "0%" : `${view.fraction * 100}%`;
  return view;
}

/** Put the meter away, for a session nothing is known about yet. */
export function clearMeter(elements: MeterElements): void {
  elements.root.hidden = true;
  elements.root.removeAttribute("title");
  elements.root.removeAttribute("aria-label");
  elements.root.dataset.state = "unknown";
  elements.text.textContent = "";
  elements.fill.style.width = "0%";
}
