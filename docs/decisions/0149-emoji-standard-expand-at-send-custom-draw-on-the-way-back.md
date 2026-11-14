---
status: accepted
date: 2026-11-02
---

# emoji: standard ones expand at send, custom ones draw on the way back

**Decision:** emoji support is two mechanisms split by one fact, whether there
is a character to expand to.

A **standard emoji** is a Unicode character, and `:rocket:` is a name for it.
A generated table maps names to characters, produced from gemoji's
`db/emoji.json` by `web/scripts/gen-emoji.mjs` and written to two committed
files: `web/src/emoji-table.ts` (the browser's table and the picker catalog)
and `zorp-agent/src/emoji_table.rs` (the terminal's). They cannot drift: one
source, one script, and `zorp-agent/tests/emoji_drift.rs` reads both and
asserts the alias to character maps agree, in the shape of
`there_is_one_chunker_and_one_fingerprint`. Expansion happens once, in the
composer at send time (`expandShortcodes`) and on the REPL line on Enter
(`emoji::expand`), so the message the model, the recall index and a later quote
all carry the character, never a name this table might not still have. A code
span or fence is left alone, an unknown name stays literal, and the stored
transcript of earlier messages is never rewritten. The renderer expands
standard shortcodes in model output through the same table, in the `math`
text-node path.

A **custom emoji** is a file a person drops in `<config>/emoji/` named
`<name>.{png,gif,webp,svg}`. There is no character to expand to, so `:name:`
stays in the message verbatim: the model sees `:name:`, recall and memory carry
`:name:`, and the renderer draws the file on the way back. Drawing it is the
one `<img>` this renderer makes, and it is made in code: the `src` is
`/api/emoji/<name>` for a name that is in the listing the page fetched, never a
URL from the text. A `:name:` not in the listing stays literal, so the `LINK`
rule still forbids every image the model could point with a URL, which a test
pins (`![x](https://…)` makes no `img`, `:notlisted:` makes no request).

The browser gets autocomplete (a `:` plus two or more characters, custom names
included), a picker beside the microphone (search, categories, recently used in
`localStorage`, a Custom category), and a settings section to add and remove
custom emoji. The terminal gets `:name:` completion on Tab and expansion on
Enter; it cannot draw a custom emoji, so it prints `:name:`.

**Why:** zorp handled emoji only as far as a text field does. Expanding
standard shortcodes at send rather than in the renderer is the load-bearing
choice: the four readers of `messages` (model, recall, memory, title/branch)
then see the character, and a recalled quote does not depend on a name table
years later. Custom emoji cannot expand to a character, so they take the only
other honest path, a name the renderer resolves against a server listing, which
reuses the "a name is a lookup key, not a path" rule skills and file links
already follow.

**Constraints that are not negotiable:**

- The renderer builds nodes, never an HTML string. The custom `<img>` is
  `createElement` with a `src` assembled in code from a listed name.
- No image from a URL in the text, ever: the `LINK` rule stands.
- A custom emoji name is a lookup key, not a path: one component of
  `[a-z0-9_+-]`, at most 64, checked on the server before a file is opened and
  on the page before a `src` is built, and a symlink resolving outside the
  directory is skipped.
- An SVG is served with the bare `Content-Security-Policy: sandbox`, because it
  can carry script; every emoji file is served with `nosniff` and under a size
  cap, and the type is sniffed, not trusted.
- Every string lands through `textContent`.
- Expansion never rewrites the store; it happens once in the composer / on the
  REPL line.
- A code span is left alone, in the composer and the renderer.
- No model tool adds, removes or lists custom emoji;
  `no_tool_manages_custom_emoji` in `agent.rs` says so.
- The two tables cannot drift: one source, one script, two files, one test.

See issue #243 and `docs/superpowers/specs/2026-11-02-emoji-design.md`.
Reactions, animated-emoji controls, emoji in titles or project names, uploading
custom emoji from the terminal, emoji in the artifact pane, and teaching the
model which custom emoji exist are all out of scope.
