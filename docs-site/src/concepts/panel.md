# panel, adversarial review

`panel` is adversarial review: several reviewers read one target at
once, each from a code-defined lens, and none of them sees what the
others said. Agreement is counted in code afterwards. It produces
opinions and changes nothing, which makes it a reader, not a gate.

It is not [critique](critique.md). Critique audits a draft against a
track's evidence record and refuses if the record moved. Panel reads a
target and reports what independent lenses found.

Two rules are not negotiable:

- **A reviewer gets strictly less than the panel that launched it.** A
  read-only allow-list of tools, so an opinion can never edit what it is
  reviewing.
- **A panel is launched by a person, never by a model.** There is no
  tool that spawns one, and the agent has a test saying so.

The web UI exposes it at `POST /api/sessions/:id/panel` on the existing
event stream. A running panel occupies the session exactly as a turn
does.
