/**
 * Standard emoji shortcodes: `:rocket:` -> 🚀.
 *
 * Two entry points, because expansion happens in two places. `expandShortcodes`
 * runs in the composer at send time, so the message that reaches the model,
 * the recall index and a quote a year from now carries the character rather
 * than a name this table might not still have; it leaves code spans and fenced
 * blocks alone, because `:smile:` in a shell snippet is a shell snippet.
 * `expandKnown` runs in the renderer on text the renderer has already split
 * away from code spans and links, so it does no code handling of its own.
 *
 * Both leave an unknown name, and a custom emoji name (which has no character),
 * exactly as it was. The table is generated from the same source as the
 * terminal's (`zorp-agent/src/emoji_table.rs`); a Rust test asserts they agree.
 */

import { EMOJI_SHORTCODES } from "./emoji-table.ts";

const SHORTCODE = /:([a-z0-9_+-]+):/g;

// A fenced block or inline code span (group 1), else a shortcode (group 2).
// Code is matched first so its contents come back verbatim.
const CODE_OR_SHORTCODE = /(```[\s\S]*?```|`[^`\n]*`)|:([a-z0-9_+-]+):/g;

/**
 * The character for a known standard shortcode, or `undefined`. Guards against
 * `:__proto__:` and the like: the lookup is an own-property check, never an
 * inherited one, so a name that happens to be an `Object.prototype` key is not
 * mistaken for an emoji.
 */
export function shortcodeChar(name: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(EMOJI_SHORTCODES, name)
    ? EMOJI_SHORTCODES[name]
    : undefined;
}

/** Expand known shortcodes in text that holds no code spans (renderer use). */
export function expandKnown(text: string): string {
  return text.replace(SHORTCODE, (whole, name: string) => shortcodeChar(name) ?? whole);
}

/**
 * Expand known shortcodes for the composer at send time, leaving code spans,
 * fenced blocks, and unknown or custom names exactly as they were.
 */
export function expandShortcodes(text: string): string {
  return text.replace(CODE_OR_SHORTCODE, (whole, code, name) =>
    code !== undefined ? whole : (shortcodeChar(name) ?? whole),
  );
}
