# Emoji: shortcodes, a picker, autocomplete, and custom emoji

Issue #243. Full emoji support in both surfaces, plus custom emoji a person
adds. This records the design decisions; the issue holds the motivation.

## The two kinds, and the one rule that separates them

**Standard emoji** are Unicode characters. A shortcode like `:rocket:` is a
name for a character (🚀). Everything about them is a string-to-string
substitution, and it happens in two places: in the composer at send time (so
the message carries the character, which is what the model, recall, memory,
title and branch all read), and in the renderer for model output (so a model
that writes `:rocket:` gets the character back).

**Custom emoji** are a file a person dropped in a directory. There is no
character to expand to, so `:name:` stays in the message verbatim; the model
sees `:name:`, recall and memory carry `:name:`, and the renderer draws the
file on the way back. Drawing it is the one `<img>` this renderer ever makes,
and its `src` is built in code from a name that is in the listing the page
already fetched, never from text. A `:name:` not in the listing stays literal.

## Source and generation (items 1, 2, 3)

One source: gemoji's `db/emoji.json` (`aliases` are the shortcodes, `category`
and file order give the picker its grouping and ordering). Chosen over raw CLDR
because it is a single JSON that already carries aliases, category and a sane
order; skin-tone and ZWJ sequences are entries in it, not logic here.

One script, `web/scripts/gen-emoji.mjs`, run by hand and its output committed,
never run in CI. It fetches the gemoji JSON (or reads a local copy passed as an
argument) and writes two generated files:

- `web/src/emoji-table.ts`: `EMOJI_SHORTCODES` (alias → character) and
  `EMOJI_CATALOG` (ordered `{char, name, category, keywords}` for the picker).
- `zorp-agent/src/emoji_table.rs`: `SHORTCODES: &[(&str, &str)]`, sorted by
  alias, for `emoji::expand`. The Rust side needs alias → char only; the
  catalog is the browser's.

The two tables cannot drift: one Rust test
(`zorp-agent/tests/emoji_drift.rs`) reads both generated files, extracts the
alias → char pairs from each, and asserts the maps are equal, in the shape of
`there_is_one_chunker_and_one_fingerprint`.

`expand()` (TS `web/src/emoji.ts`, Rust `zorp-agent/src/emoji.rs`) replaces
`:alias:` with its character, **leaving a code span or fence alone** (`:smile:`
in a shell snippet is a shell snippet), and leaving an unknown or custom name
literal. It runs once, in the composer / on the REPL line, before send. It
never rewrites the store, so recall's fingerprint is untouched and an old
conversation reads as it was written.

The renderer (`markdown.ts`) expands standard shortcodes through the same table
in the `math` text-node code path (text nodes only, unknown left literal, code
spans already out of reach). This is the one place standard emoji touches the
renderer.

## Autocomplete (item 4)

`web/src/emoji-complete.ts`: a pure match function (`:` plus two or more
characters → up to eight prefix matches from the catalog) tested on its own,
and a popover under the caret wired in `main.ts`. Arrow keys move, Enter or Tab
pick, Escape dismisses, Enter with the list closed still sends. The REPL gets
the same names on Tab beside the slash/capsule completion `chat::COMMANDS`
drives.

## Picker (item 5)

A button beside the microphone, a popover in the shape of the skills popover: a
search box, a grid, categories along the top, recently used first (recents in
`localStorage`, per the browser-storage rule). Clicking inserts the character
at the caret. Every string lands through `textContent`. A "Custom" category is
built from the `/api/emoji` listing and inserts `:name:`.

## Custom emoji (item 6)

Directory `$XDG_CONFIG_HOME/zorp/emoji/` holding `<name>.{png,gif,webp,svg}`.
A name is one path component and matches `^[a-z0-9_+-]{1,64}$`, the same
"a name is a key, not a path" rule `is_valid_skill_name` applies, checked on
the server before a file is opened and on the page before a `src` is built.
A symlink resolving outside the directory is skipped the way `zorp-skill`
skips one.

`zorp-web/src/emoji.rs`, routes behind the token gate:

- `GET /api/emoji`: names and types, from a scan of the directory.
- `GET /api/emoji/:name`: the file, its own content type,
  `X-Content-Type-Options: nosniff`, a 256 KiB cap, and for an SVG the bare
  `Content-Security-Policy: sandbox` the artifact pane uses, because an SVG can
  carry script.
- `POST /api/emoji?name=<name>`: the raw file as the body (the name in the
  query); refuses a bad name, a file over the cap, or a type it does not serve.
  The type is sniffed from the bytes, not trusted from the client.
- `DELETE /api/emoji/:name`: removes it.

The routes are for a person in the settings pane. No model tool adds, removes
or lists custom emoji; `agent.rs` gets the usual `no_tool_*` test.

In the renderer, a `:name:` in the fetched listing becomes an `<img>` built in
code: `src` is `/api/emoji/<name>` assembled from the listed name, `alt` and
`title` are the name, and a class sizes it to the line. A module-level registry
in `markdown.ts` holds the listed names, set by `main.ts` after it fetches
`/api/emoji`; empty until then, so nothing renders as an image before the
listing is known, and `:notlisted:` never makes a request.

## Font (item 7)

`--sans` in `styles.css` gains `"Apple Color Emoji", "Segoe UI Emoji",
"Noto Color Emoji"` at the end, so emoji draw in colour per platform.

## Not negotiable

- The renderer builds nodes, never an HTML string. The custom `<img>` is
  `createElement` with a `src` assembled in code from a listed name.
- No image from a URL in the text, ever: the `LINK` rule stands, and a test
  says `![x](https://…)` still makes no `img` and `:notlisted:` makes no
  request.
- A custom emoji name is a lookup key, not a path (one component, charset
  checked, symlink-escape skipped).
- Every string lands through `textContent`.
- Expansion never rewrites the store; it happens once in the composer.
- A code span is left alone, in the composer and the renderer.
- No model tool touches custom emoji.
- The two tables cannot drift: one source, one script, two files, one test.

## Out of scope

Reactions, animated-emoji controls, emoji in titles/project names, uploading
custom emoji from the terminal (copy a file in), emoji in the artifact pane,
and teaching the model which custom emoji exist.
