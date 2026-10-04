---
status: superseded
date: 2026-08-18
---

# the markdown renderer is ours, because the alternative is innerHTML

**Decision:** `web/src/markdown.ts` renders markdown by building DOM nodes.
No markdown dependency, and no `innerHTML` anywhere in the path from model
output to the page. Alongside it, `GET /api/artifacts` and
`GET /api/artifacts/raw` serve files from the directory the server was
started in, and a pane renders them: markdown through the same renderer, PDF
in an iframe. Design:
`docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Superseded by** 2026-08-19: a PDF is read for its text, not framed for its
layout, and only as to the PDF. The iframe never rendered one: the sandbox
this entry relies on is exactly what stops a browser's PDF viewer starting.
Everything else here stands, the sandbox included, which is still what makes
`.svg` and `.html` safe to show.

**Why not a library:** every markdown library worth using returns an HTML
string and hands you the `innerHTML` call. The text being rendered here is
model output, and the model has been reading tool results, web pages and
files, so treating it as trusted markup is cross-site scripting with extra
steps. The old code-block-only renderer already had this property and said
so in a comment; keeping it was the requirement, and writing the renderer
was the only way to keep it while rendering more than code blocks.

Two consequences that needed their own care. A markdown link is the one
construct where model output chooses a URL that the page makes clickable, so
only `http`, `https` and `mailto` become anchors and everything else renders
as visible text. Images are not supported at all, because `![](url)` would
make the page fetch an attacker-chosen URL on render, which is a beacon.

**Why serving files is scoped the way it is:** `zorp-web` already runs
commands and edits files in that directory, so reading from it is not new
reach. The new thing is a door that takes a path from a URL. Paths resolve
against the root and are checked for containment *after* canonicalization,
because looking for `..` in a string does not catch a symlink, and a symlink
inside the workspace can point anywhere. Types are an allowlist rather than
a denylist: an unknown type guessed at as `text/html` is a hole, while an
unknown type refused is an inconvenience. Every response carries `nosniff`
and a `sandbox` CSP, which is what makes it safe to put a PDF in an iframe.

**What it rules out:** rendering raw HTML in markdown, which shows as text
instead; generating PDFs, which needs LaTeX or typst and is its own project;
and editing from the pane, which is read-only. Syntax highlighting is
deferred, and the existing `data-lang` attribute is enough to add it later.

`web/` also gained its first test harness, jsdom plus `node:test`, wired
into the `web` CI job. A security-critical renderer with no automated test
was not worth shipping, and a typecheck does not notice a renderer that
starts producing markup.
