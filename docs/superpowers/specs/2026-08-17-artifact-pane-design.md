# Rendering what zorp produces: markdown in the chat, artifacts in a pane

Design, 2026-08-17.

## The problem

zorp's whole point is that a run ends in an artifact. `deliver` writes
`draft.md` and `venues.md` into the track directory. `co-write` produces
prose. The agent writes files into the workspace all day.

The browser shows none of it. Two separate gaps:

**The chat cannot render markdown.** `renderRichText` in `web/src/main.ts`
handles exactly two things: fenced code blocks and inline backticks.
Everything else arrives as flat text. A model that answers with a heading, a
numbered list and a table renders as `## Heading`, `1. thing`, and a row of
pipes. The one product whose output is a written document displays writing
worse than a terminal does.

**There is no way to look at a produced file.** `draft.md` gets written and
the only way to read it is to leave the browser. A research paper the run
produced, or a PDF it was given to work from, has nowhere to appear.

## What this builds

Two pieces that share one renderer.

1. Real markdown rendering for assistant messages in the chat.
2. A pane on the right that opens a file from the workspace: `.md` rendered,
   `.pdf` shown inline, anything else as plain text.

## The constraint that shapes everything

`renderRichText`'s comment says it: "Everything lands on the page through
textContent, so nothing the model or a tool emits can become markup." That
is not a style preference. The text being rendered is attacker-adjacent by
construction: it is model output, and the model has been reading tool
results, web pages and files. `innerHTML` anywhere in this path is a
cross-site scripting hole with extra steps.

So: **the markdown renderer builds DOM nodes and never assembles an HTML
string.** That rules out every markdown library worth using, because they
all return HTML strings and hand you the `innerHTML` call. `marked`,
`markdown-it`, `snarkdown`: all of them produce a string.

Writing our own is therefore not "not invented here", it is the only option
that keeps the property. It also keeps `web/package.json` at zero runtime
dependencies, which is where it is today.

### Links are the sharp edge

A markdown link is the one construct where model output chooses a URL that
the page then makes clickable. `javascript:alert(1)` in an `href` executes
on click. So:

- Only `http:`, `https:` and `mailto:` produce an `<a>`. Anything else
  renders as plain text, link syntax and all, so nothing is silently
  swallowed.
- Every link gets `rel="noopener noreferrer"` and `target="_blank"`.

Images are deliberately not supported. `![](url)` would make the page fetch
an attacker-chosen URL on render, which is a beacon. It renders as text.

## Markdown subset

Enough for a research document, and nothing whose parsing is subtle:

| Construct | Notes |
|---|---|
| `#` through `######` | `h1`-`h6`. Existing code blocks keep working. |
| `-`, `*`, `+` lists | Nested by indent. |
| `1.` ordered lists | |
| `> ` blockquote | |
| `---` horizontal rule | |
| `**bold**`, `*italic*`, `` `code` `` | Inline, composable. |
| `[text](url)` | Scheme-restricted as above. |
| Tables | Pipe tables with a `---` separator row. |
| Fenced code | Already works. Keeps working, unchanged. |

Not supported, on purpose: images, raw HTML blocks, footnotes, reference
links, setext headings. Raw HTML is the important one. It renders as visible
text, which is the safe direction and also the honest one.

## The artifact pane

A right-hand pane, toggled from the top bar, closed by default. It has a
file list and a viewer.

### Serving files is the risky part

`zorp-web` already runs commands and edits files on the machine it was
started on. Adding "and serves file contents over HTTP" is a smaller
increment than it sounds, but only if it is scoped.

`GET /api/artifacts` lists candidate files. `GET /api/artifacts/raw?path=`
returns one. Both sit behind the existing token gate, because both live
under `/api/`.

The rules, each of which gets a test:

- The path is resolved against the workspace root, the directory the server
  was started in, then canonicalized. If the canonical result is not inside
  the canonical root, it is a 403. This is checked after canonicalization,
  so `..` and symlinks pointing outward are both caught by the same check.
- Only files, never directories.
- Listing is depth-limited and count-capped, skips dot-directories except
  `.zorp`, and skips `target/`, `node_modules/` and `.git/`. A repository
  checkout should not produce a ten thousand entry list.
- The response carries `X-Content-Type-Options: nosniff` and a
  `Content-Security-Policy: sandbox` header, so a served file cannot become
  an active document in this origin.

### PDFs

Shown in an `<iframe>` pointed at the raw endpoint. The browser's own PDF
viewer does the work; nothing is bundled and no PDF is parsed by us. The
`sandbox` CSP header on the response is what keeps a hostile PDF from
reaching the rest of the origin.

Nothing here *generates* a PDF. zorp has no PDF writer today, and adding one
means LaTeX or typst, which is its own project. This displays a PDF that
already exists, which is what "render the research paper inline" needs when
the paper came from `deliver` or from the user.

### Content type

Decided from the extension, from a small allowlist, not sniffed:
`.md`/`.markdown`/`.txt` as text, `.pdf` as `application/pdf`, `.json` as
text, everything else refused. An allowlist rather than a denylist because
the failure direction matters: an unknown type served as
`application/octet-stream` is a download prompt, but an unknown type served
as `text/html` is an XSS hole.

## Errors

A file that has vanished between listing and opening gives a 404 that the
pane displays as "that file is not there any more", not an empty pane. A
file too large to render (over 2 MB for text) is refused with a message
saying so, rather than freezing the tab. Both follow the repo's existing
fail-loudly line: a blank pane and a pane showing an empty file look the
same, and they are not the same.

## What this does not do

- No editing. The pane is read-only.
- No live reload. Opening a file reads it once; there is a refresh button.
- No PDF generation.
- No syntax highlighting in code blocks. Separate concern, needs a
  dependency, and the current `data-lang` attribute is already enough to add
  it later.
