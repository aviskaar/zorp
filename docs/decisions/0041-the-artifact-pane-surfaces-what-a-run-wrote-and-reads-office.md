---
status: superseded
date: 2026-08-18
---

# the artifact pane surfaces what a run wrote, and reads office formats

**Decision, part one:** the pane notices produced files by diffing the
`/api/artifacts` listing, not by reading tool events. Every listing row now
carries `modified_ms`. The browser snapshots the listing when a turn starts
and compares after tool activity and at the end of the turn; anything new or
newer is something the run wrote.

**Why not tool events:** because how a file got written is not knowable from
one. A PDF that pandoc produced under `run_command` names no path anywhere,
and it is exactly as much a result of the run as one `write_file` wrote.
Parsing tool summaries for paths would catch the easy half and quietly miss
the other, which is the worst of both. Asking the directory catches whatever
the next way of writing a file turns out to be.

**Decision, part two:** a run that produced something says so with a count on
the Files button, not by opening the pane. The pane opens itself only when it
is already open. A pane that appears over the answer somebody is reading is an
interruption, and the run has already finished, so there is nothing being
missed by waiting for a click.

**Superseded by** 2026-08-19: a run that wrote a file opens the pane showing
it. Parts one and three stand.

**Decision, part three:** `.docx`, `.odt`, `.xlsx` and `.pptx` are extracted
to markdown on the server, in `zorp-web/src/documents.rs`, and rendered
through the markdown renderer that already exists. Headings, paragraphs,
lists and tables. Images, fonts, colours, page layout, footnotes, comments,
tracked changes and numbered-list numbering are out of scope and stay out.
This is for reading what a run produced, not for rendering a document. The
extractor treats its input as hostile: caps on archive entries, on
decompressed bytes per part and per archive, and no XML entity expansion
beyond the five predefined ones, so neither a zip bomb nor billion laughs is
a case that needs thinking about at the call site.

**Decision, part four, and the one that needed the argument:** `.svg` and
`.html` are served and shown. Both execute. They are safe here for exactly
one reason: they load into the pane's iframe by URL, and the raw endpoint
sends every file with `X-Content-Type-Options: nosniff` and a bare
`Content-Security-Policy: sandbox`, no `allow-` token. That puts the document
in a unique origin with scripting off, so script inside it neither runs nor
gets a handle on the page that framed it. Verified in a browser: a served SVG
containing `parent.document.title = "pwned"` is refused by the engine with
"Blocked script execution ... the document's frame is sandboxed".

**What that rules out:** inlining either one. An `<svg>` element built from a
served file, or an iframe `srcdoc`, would run that script in this origin and
make every precaution in `web/src/markdown.ts` beside the point. The rule is
in `web/src/artifact-view.ts` and there are tests either side of it: the
server's headers, and the pane's refusal to put the bytes on the page even
when handed them.

**Cost paid:** two dependencies, `zip` and `quick-xml`. `zip` is pinned to 4
rather than the 6 already in the lock as a build dependency of
`libduckdb-sys`, because 6 requires Rust 1.83 and this workspace's floor is
1.82. Every codec but deflate is switched off; an office file uses no other.
