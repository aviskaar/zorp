---
status: accepted
date: 2026-09-05
---

# the browser asks for a workspace, first and again when one is missing

**Finding:** `zorp-web` ran the agent in the directory the server was
started in, which for anyone running from source is zorp's own tree, so
every file the agent wrote landed in this repo. The server is being
changed to work in a directory the person picks. That leaves the page
with a question it never had to ask, and two moments to ask it in: the
first run, and the first send on a server that has no answer yet.

**Decision:** a picker, `web/src/workspace.ts`, and three places it
surfaces. It is the first step of the first-run flow, before the model,
because a model with nowhere to work is useless; the step is skipped when
a workspace is already set, exactly as the model steps are skipped when
the settings are already good. It is behind a top bar pill next to the
model and Files buttons, showing the last path segment with the full path
in the `title` and reading "No workspace" in warn colours when there is
none. And it opens by itself, with one sentence saying why, when a start
comes back 409 with `no workspace chosen`. The hook for that is
`appendError` in `main.ts` and not the three start sites, because an
`error` frame on the event stream lands there too. `api.ts` stops turning
every 409 from a start into `TurnBusyError`: both a busy session and a
missing workspace answer 409 and only the body tells them apart, so
`startFailure` reads the body and one function does it for turns, panels
and investigate runs alike.

The picker shows one path, and the header, the text field and the Save
button all read it, so the button can never send something other than
what the person is looking at. Directory names come off a filesystem, so
they are untrusted text and land through `textContent` like a model id in
`onboarding.ts`; the test file lists a directory called
`<img src=x onerror=...>` and checks it is a text node. A refusal shows
the server's own sentence with nothing added, and leaves the listing up
so the next try starts where the last one stopped. The scratch directory
is named from the API's `scratch` field and never joined together in the
page, because where generated files land is the server's rule and a
second copy of it would go quietly wrong the day the server changed.

**What it ruled out:** cutting a long path in code, when
`.model-btn`'s existing `max-width` and ellipsis already truncate a long
model id and the pill reuses that class whole. A picker that lists files;
this chooses a place to work. Any new styling for the top bar button.

**Not changed:** nothing here starts a turn, a panel or an investigate
run, and nothing here decides what a valid workspace is. The server
refuses a path and sends the sentence; the page shows it.
