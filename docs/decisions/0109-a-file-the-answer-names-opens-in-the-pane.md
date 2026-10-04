---
status: accepted
date: 2026-09-05
---

# a file the answer names opens in the pane

**Decision:** an inline code span or a relative markdown link in an
answer that names a file the workspace listing reports becomes a button,
and pressing it opens that file in the artifact pane through the same
two calls an investigate run's draft goes through. `web/src/file-links.ts`
does the upgrade and `main.ts` runs it when an answer is drawn, on the
replay path and the live one, and again whenever a listing arrives.

**Why:** the answer already says where the file is. Reading "the PDF is
at `report.pdf`" and then opening the Files pane to find that same name
in a list is a step the page can take for the reader.

**The listing is the only thing that decides what is a file.** A code
span it does not know stays a code span, so `ClusterIP` in an answer
about Kubernetes is still text and a path the model invented points at
nothing. That is the containment as well as the accuracy: a name the
model wrote can only ever reach a file the workspace has already
reported, so no reference in an answer leads out of the workspace. A
link with a scheme is never captured either, or an external URL whose
last segment happened to match a listed name would stop reaching the
site it named.

**Basename matching, and only when it is unambiguous.** An answer says
`report.pdf` where the listing says `scratch/report.pdf`, so an exact
match is tried first and the basename after it. Two listed files with
that basename resolve to nothing and the reference stays plain text:
opening one of them would be a guess, and a guess here opens the wrong
document while looking like it worked.

**What it ruled out:** the pane's own display rules, which are untouched.
This decides which path to open and nothing else, so a `.svg` or a
`.html` still goes into the sandbox frame and a `.pdf` still goes into
the viewer frame, exactly as the note at the top of `artifact-view.ts`
says. Nothing here fetches or inlines a file. And nothing assembles
markup: the name goes onto the button through `textContent` and into
`dataset`, and into a URL only through `artifactUrl`, which encodes it.
Decorating the code span in place was ruled out too. Replacing it with
the button is what makes a second pass a no-op, since there is then no
code span and no anchor left to find.
