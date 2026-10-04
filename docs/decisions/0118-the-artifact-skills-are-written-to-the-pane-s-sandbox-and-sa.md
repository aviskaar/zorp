---
status: accepted
date: 2026-09-10
---

# the artifact skills are written to the pane's sandbox, and say which limits are walls

**Decision:** `.claude/skills/artifact-design` and
`.claude/skills/artifact-diagramming` are the first skills zorp ships with
itself, and they are written around one fact about the pane rather than
around general web taste: `zorp-web` serves `.html` and `.svg` under a bare
`Content-Security-Policy: sandbox`, so the document sits in a unique origin
with scripting off. `artifacts.rs` gives `allow-scripts` to a PDF and to
nothing else, because the browser's own PDF viewer is a scripted document.
`only_the_formats_that_execute_are_sandboxed` already pins that set.

That is why the issue's suggestion of pinned UMD builds from a CDN is not
taken. Chart.js, D3 and Plotly are scripts, so a page built that way renders
as an empty box and nothing says why. Drawing the chart as SVG by hand is
the only option, which is what makes `artifact-diagramming` load-bearing
rather than a nice-to-have.

**A limit that is enforced and a limit that is a rule are said differently.**
Scripts really do not run, and the skill says so flatly. External
stylesheets, fonts and images are a different case: `sandbox` is a document
directive and does not restrict which URLs a page may load, so ruling them
out is a house rule and the skill says that too. Its reasons are the honest
ones: the page is read with no network, it is copied out of the workspace,
and a file a model wrote should not tell a third party that somebody opened
it. A skill that dressed a preference up as a mechanism would be believed
about the mechanism, and the next person to check would find the opposite.

**Where they live is also what they cover.** They sit in this repository's
own `.claude/skills`, so they are discoverable for work in this checkout.
Somebody who wants them in another workspace copies them to
`~/.claude/skills` or points `ZORP_SKILLS_DIR` at them; zorp has no built-in
scope and this does not add one.
