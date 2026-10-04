---
status: accepted
date: 2026-09-13
---

# a landing page is a deliverable, so the preview is the constraint and not the page

**Decision:** `.claude/skills/landing-page` is the third skill zorp ships
with itself. It covers one self-contained `.html` file written into
`scratch/`, and it is separated from `artifact-design` because the two are
written for different readers. An artifact page is read once in the side
pane and thrown away. A landing page goes on a real web server and is sent
to strangers.

**Why that distinction changes what the skill says.** The artifact skills
say flatly that scripts do not run, which is true of the pane and is the
only thing that matters for a report page. Saying only that to somebody
writing a landing page would be wrong about the artifact they are actually
making, because the shipped page runs in an ordinary browser where scripts
are fine. So this skill states both, and puts the useful rule on top: write
the page so it works with JavaScript off. Then the pane preview *is* the
page, minus the polish, and the no-script view is something worth checking
on purpose rather than a limitation to work around.

**The issue's recommended JSX path is not taken.** Option 1 was a
transpiler from a CDN in a `<script type="text/babel">` block, on the
grounds that it needs no change to zorp and keeps the preview working. It
keeps neither: a bare `sandbox` CSP does not run scripts, so the block
never executes, and the page renders as nothing with nothing to say why.
This is the same finding as the pinned UMD chart builds in the 2026-09-10
entry, arriving from a different direction. The skill names the trap
explicitly and `zorp-skill/tests/first_party.rs` pins that it does, because
it is the first thing anybody tries.

**What replaces it.** The path to JSX is a set of authoring rules that make
the later conversion mechanical rather than a rewrite: one section per
component, repeating blocks written identically so they become one `.map`,
copy in the markup rather than in `::before` content, the `:root` custom
properties as the theme, classes rather than ids for styling, and no script
reaching across sections. That is free to follow, needs no build step, and
is worth having whether or not the page ever becomes an app. A zorp owned
build step is still option 3 in the issue and still needs its own issue.
