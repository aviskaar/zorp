---
status: accepted
date: 2026-11-16
---

# The Mac app's first run is a guide over the existing routes

**Decision:** When the Mac app starts with no workspace or no configured
model, or after Reset Settings, it shows a full-window welcome guide in place
of the old folder picker and settings popups. The guide has four steps:
welcome, pick a workspace, connect a model, done. Help > Welcome Guide opens
it again. It can be skipped at any step.

The guide is a client of what already exists and nothing more. The workspace
step uses the same picker and route as Settings. The model step lists the
same endpoint presets, loads models from the same listing, tests the
connection with the same probe, and saves through the same settings update.
The API key goes to the login keychain and the server's memory, as it does
from Settings, and never into the settings file.

**Why:** A new install opened two unrelated popups one after the other, with
no word on what zorp needed or why. One guided flow says what each step is
for, and a reset lands somewhere sensible instead of an empty window.

**Not done:** the browser keeps its own onboarding (`web/src/onboarding.ts`).
No new route, setting or stored state was added.
