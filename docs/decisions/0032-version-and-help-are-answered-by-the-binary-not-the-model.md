---
status: accepted
date: 2026-08-17
---

# --version and --help are answered by the binary, not the model

**Decision:** `zorp` intercepts a leading `--version`, `-V`, `--help` or
`-h` and answers locally. Anywhere other than the first argument they are
still part of the prompt.

**Why:** they were joined into the prompt and POSTed to the model, so two
of the first flags anyone types at an unfamiliar binary cost a
completion, and with no key configured the new user's first impression
was a 401 wall of JSON from OpenAI. This is not a departure from "argv is
the prompt": `main` already intercepted `--init`. These were missing from
that list rather than deliberately excluded from it.

**What it rules out:** a prompt whose *first* word is one of those four
flags. `zorp what does --version print` still reaches the model, and
there is a test for it, because that is the behavior the change could
plausibly have broken.
