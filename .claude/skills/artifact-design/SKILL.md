---
name: artifact-design
description: Design guidance for HTML pages zorp writes into the workspace, which the browser shows in its side pane. Use before writing a report, dashboard, summary page, or any .html file meant to be looked at rather than parsed.
---

# Writing an HTML artifact

zorp's browser shows `.html` and `.svg` files from the workspace in a side
pane. Write the file, and the pane picks it up.

## Where to put it

Save it under `scratch/` in the workspace with a descriptive name:
`scratch/latency-by-region.html`, not `scratch/output.html`. The pane lists
what the workspace holds, and a directory of `output-2.html` is a directory
nobody can navigate.

## The one constraint that decides everything else

**The pane serves your file under a bare `Content-Security-Policy: sandbox`.**
The document is in a unique origin with scripting off. That is not a
setting; it is what makes it safe to render a file a model wrote.

So:

- **No `<script>`.** Not inline, not external. It will not run, and nothing
  will say so. A page whose content appears only after JavaScript is a blank
  page.
- **No external anything.** No CDN, no `<link>` to Google Fonts, no remote
  images. `sandbox` alone does not block those loads, so this one is a rule
  and not a wall: the page has to stand on its own. It is read on a laptop
  with no network, it is copied out of the workspace and opened somewhere
  else, and a file the model wrote should not phone anywhere on the reader's
  behalf. A page whose fonts arrive from a third party is also a page that
  told that third party somebody opened it.
- **No chart library.** Chart.js, D3, Plotly and every other one are
  scripts. Draw the chart as SVG yourself; see the `artifact-diagramming`
  skill.
- **No `localStorage`, no `fetch`, no form submission.** Same reason.

Everything the page needs is in the file: styles in one `<style>` block,
images as `data:` URIs if they are small and left out if they are not.

This is a real limit and it is worth saying plainly in the page when it
bites. A table of numbers that says what it is beats an interactive chart
that renders as nothing.

## Theme

The iframe is its own document and inherits nothing from the app around it.
It follows the browser's colour scheme, not zorp's.

Paint your own background explicitly. A page with a transparent body shows
whatever the pane happens to be, which changes under you.

```css
:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #16181d;
  --muted: #5b6478;
  --line: #dde1e9;
  --accent: #1450f5;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d0e11;
    --fg: #e7e9ee;
    --muted: #99a1af;
    --line: #23262d;
    --accent: #5eead4;
  }
}
body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
}
```

Define every colour as a token on bare `:root` first and redefine only what
changes in the dark block. A colour whose only definition is inside a media
query is a colour that is missing half the time.

## Type

System stacks, since there are no web fonts to load:

```css
font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
font-family: ui-monospace, SFMono-Regular, Menlo, monospace; /* code, numbers */
```

- Body text 14 to 16px, line height 1.5 to 1.6.
- Measure capped: `max-width: 70ch` on running text. A paragraph the width
  of a wide pane is unreadable.
- `text-wrap: balance` on headings.
- `font-variant-numeric: tabular-nums` on every column of figures, so digits
  line up down the column.
- One size scale, four or five steps. A page with nine heading sizes has no
  hierarchy.

## Layout

The pane is narrow and resizable. Assume 380px and be pleased when it is
900px.

- Grid or flex with `gap`. Never margins that collapse against each other.
- One column under about 640px: `grid-template-columns: repeat(auto-fit,
  minmax(260px, 1fr))` does this without a media query.
- Side gutters: `padding: 24px clamp(16px, 4vw, 40px)`.
- **Wide content scrolls inside itself, never the page.** Wrap every table,
  code block and diagram in `<div style="overflow-x:auto">`. A page that
  scrolls sideways is a page with a bug in it.
- `img, svg { max-width: 100% }`.

## What to avoid

The default look of a generated page is recognisable and it is not good:

- Purple or blue-to-pink gradient headers.
- Every surface with the same 8px radius and the same drop shadow.
- Three "stat cards" with a big number and no unit and no comparison.
- Emoji as section icons.
- A colour per section with no meaning behind the colours.

Prefer: one accent used sparingly, real borders instead of shadows, numbers
with their units and something to compare against, and whitespace doing the
grouping that boxes would otherwise do.

## Tables

Most of what gets written into this pane is a table. Make it a good one.

- Right-align numbers, left-align text, with `tabular-nums`.
- Header row in the muted colour, one bottom border, no vertical rules.
- Zebra striping only past about fifteen rows.
- Put the unit in the header, not in every cell.
- Sort by whatever the reader came to find out, which is rarely the first
  column alphabetically.

## Before you save

- Does it say what it is, in the first thing on the page, in words?
- Is every number's unit and source stated?
- Does it read at 380px wide?
- Is there a `<script>` in it? Remove it; it does not run.
- Does the body have an explicit background?
