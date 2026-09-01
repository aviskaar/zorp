/**
 * The endpoints zorp knows a shortcut for.
 *
 * One table, read by two places: the settings panel's preset `<select>` and
 * the first-run flow's provider step. It was in `main.ts` and the flow needed
 * the same rows, so it moved here rather than being copied. A second copy of
 * this is a second place for a base URL to drift.
 *
 * A preset is a shortcut and not a provider. `provider` is the wire format,
 * and there are only two of those: Ollama, oMLX and OpenRouter are all the
 * OpenAI-compatible format pointed somewhere else. "custom" leaves whatever
 * base URL is already in the field alone.
 */

export interface ProviderPreset {
  /** What the person picking it sees. */
  label: string;
  /** One line saying what choosing it means. */
  summary: string;
  baseUrl: string;
  /** The wire format: "openai" or "anthropic". */
  provider: string;
  needsKey: boolean;
  /** Where this provider's keys are made, when it has such a page. */
  keyUrl?: string;
}

export const PRESET_DEFAULTS: Record<string, ProviderPreset> = {
  ollama: {
    label: "Ollama",
    summary: "Models running on this machine. No key, no account, nothing leaves the machine.",
    baseUrl: "http://localhost:11434/v1",
    provider: "openai",
    needsKey: false,
  },
  // oMLX is OpenAI-compatible too, but unlike Ollama it can require an API
  // key (`--api-key`), so the key field stays visible for it.
  omlx: {
    label: "oMLX",
    summary: "Apple silicon models running on this machine. A key only if you started it with one.",
    baseUrl: "http://localhost:8000/v1",
    provider: "openai",
    needsKey: true,
  },
  openrouter: {
    label: "OpenRouter",
    summary: "One key, many providers, and a list that says which models cost nothing.",
    baseUrl: "https://openrouter.ai/api/v1",
    provider: "openai",
    keyUrl: "https://openrouter.ai/workspaces/default/keys",
    needsKey: true,
  },
  openai: {
    label: "OpenAI",
    summary: "GPT models, billed to your OpenAI account.",
    baseUrl: "https://api.openai.com/v1",
    provider: "openai",
    keyUrl: "https://platform.openai.com/api-keys",
    needsKey: true,
  },
  anthropic: {
    label: "Anthropic",
    summary: "Claude models, billed to your Anthropic account.",
    baseUrl: "https://api.anthropic.com/v1",
    provider: "anthropic",
    keyUrl: "https://console.anthropic.com/settings/keys",
    needsKey: true,
  },
  custom: {
    label: "Custom (OpenAI-compatible)",
    summary: "Any other server that speaks the OpenAI format.",
    baseUrl: "",
    provider: "openai",
    needsKey: true,
  },
};

/** The preset for a key, or the custom row for a key nobody knows. */
export function preset(name: string): ProviderPreset {
  return PRESET_DEFAULTS[name] ?? PRESET_DEFAULTS.custom;
}

/**
 * Guess which preset a resolved (provider, base_url) pair matches, so
 * reopening the settings panel shows the right choice instead of always
 * "custom".
 */
export function presetFor(provider: string, baseUrl: string): string {
  if (provider === "anthropic") {
    return "anthropic";
  }
  const trimmed = baseUrl.replace(/\/+$/, "");
  if (trimmed.includes("11434")) {
    return "ollama";
  }
  if (trimmed === "http://localhost:8000/v1" || trimmed === "http://127.0.0.1:8000/v1") {
    return "omlx";
  }
  if (trimmed === "https://openrouter.ai/api/v1") {
    return "openrouter";
  }
  if (trimmed === "https://api.openai.com/v1") {
    return "openai";
  }
  return "custom";
}
