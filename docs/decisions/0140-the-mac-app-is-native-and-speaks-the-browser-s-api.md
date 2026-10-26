---
status: accepted
date: 2026-10-26
---

# The Mac app is native and speaks the browser's API

**Decision:** `Zorp.app` stays a native SwiftUI app rather than a window
around the web UI. It runs `zorp-web` in its own process, as before, and
talks to it over the same HTTP routes and event stream the browser uses.
`ZorpKit/Network/ZorpClient.swift` mirrors `web/src/api.ts` route for route,
and `ZorpKit/Models` mirrors the server's JSON field for field. The tests
decode the server's own frames and check the bodies the app sends.

**Why:** The first native screens were written against a guessed API. A
turn was posted as `prompt` when the server reads `message`, so every
message failed with a bare NSURL error, and the project list, the artifact
list and the bolt all expected shapes the server never sent. A person asked
for a native app with everything the web has, so the fix is to hold the
native client to the real contract, not to replace it with a web view.

**What it costs:** Two clients of one API. A route or field that changes in
`zorp-web` has to change in both, and the contract tests are what catch a
change made in one place only.

**Also decided:** the API key goes in the login keychain and is handed to
the server's memory at launch with `PUT /api/settings`. It is never written
to the settings file, which has no field for it. An app opened from the Dock
does not see a shell's `ZORP_API_KEY`, so without this a Dock-launched app
could never use a hosted model.
