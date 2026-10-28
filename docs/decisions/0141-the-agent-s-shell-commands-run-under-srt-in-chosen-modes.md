---
status: accepted
date: 2026-10-27
---

# the agent's shell commands run under srt, in modes zorp generates

**Decision:** a run can wrap every shell command in
[anthropics/sandbox-runtime](https://github.com/anthropics/sandbox-runtime)
(`srt`), chosen by a mode. `SandboxMode` is `off`, `workspace`, `build` or
`read-only`. `off` is the default and leaves `Sandbox::run` exactly as it was.
Any other mode generates an `srt` settings file per run, in a temp directory,
and runs the command as `srt --settings <file> -- <shell> -c <command>` inside
the existing `ProcessGroup`. The mode is resolved through the config chain:
`--sandbox`, then `ZORP_SANDBOX`, then the flavor, then the saved setting,
then `off`.

- `workspace`: reads allowed except credential stores (`~/.ssh`, `~/.aws`,
  keychains and the rest in `CREDENTIAL_DIRS`), writes only to the workspace
  and `<workspace>/scratch`, no network.
- `build`: `workspace` plus a network allowlist of the four package
  registries, crates.io, npm, PyPI and GitHub, spelled out as the hosts a
  real build contacts.
- `read-only`: no writes and no network, for panel and ensemble reviewers,
  which already get read-only tools.

**`ProcessGroup` stays the one platform-specific type.** `srt` is wrapped by
the group, it does not replace it: the timeout, the cancel, the output capture
and the descendant kill are unchanged, and `srt` becomes the group leader the
shell was before. This keeps the Windows and Unix supervision (ADR 0139)
untouched; `srt`'s own Windows path is alpha and is not wired in here.

**A mode narrows and never loosens, and the config is zorp's.** The settings
`srt` runs under are generated in code from the mode, never read from a file
the model can write. That is the same reason `base_url` and `provider` are
privileged (ADR 0123): a workspace file that could widen the sandbox would be
a sandbox the sandboxed process controls. A flavor may carry a `sandbox`
field, but it is privileged exactly when it could weaken protection. The
strictest mode is `read-only`, so a flavor that sets it is only tightening and
is never gated; any other value (`off`, `build`, `workspace`) is treated as
privileged and gated behind the content-hash trust an untrusted project flavor
needs for its other privileged fields. The surface has no baseline to compare
against inside `wants_privilege`, so gating every non-maximal mode is the safe
reading: it can cost a trust click when a project agent names a specific
sandbox, but it can never let a project file quietly turn the sandbox down.
The config chain protects the rest: the flavor sits below `--sandbox` and
`ZORP_SANDBOX`, so it cannot override a mode a person set there either.

**Asked for and missing is loud.** When a mode other than `off` is requested
and `srt` (or, on Linux, `bwrap`, or `ripgrep` anywhere) is not present,
`Sandbox::run` refuses with a sentence naming what is missing. It never falls
back to running unsandboxed: a person who asked for a wall, got none, and was
not told is worse off than one who got an error. `zorp-agent doctor` reports
the mode and whether `srt` could run it, and `GET /api/capabilities` reports
the same to the browser. Both observe by running the real binaries, the way
the endpoint probe calls the real endpoint, so neither can report a sandbox
that a command would then refuse.

**The agent still cannot call zorp's own server from the sandbox** (ADR 0053).
The network is denied by default, and the one mode that opens it, `build`,
opens it only to the named registries, so loopback to `zorp-web` stays denied
there too. The in-code own-server deny runs regardless of the mode.

**A denial names the rule.** `srt`'s refusal reads as a filesystem or network
error from the command's point of view, so when a command fails under a mode,
the output is prefixed with the rule that most likely fired (ADR 0095), so the
model corrects the command rather than retrying the same shape.

**The DMG does not bundle `srt`.** `srt` is a Node package, and Node is
already a dependency of the desktop build, but bundling a beta tool into a
signed app and keeping it current is more than this is worth while the whole
feature is opt-in and off by default. The Mac app asks the person to install
`srt`, and the capabilities route already says whether it is there, so the app
can say so too. Bundling can be revisited once the feature has soaked.

**What is verified, and what needs the tool.** The mode machinery, the
settings generation, the loud refusal when `srt` is missing, the error
mapping, the flavor privilege rule, and the config resolution are unit tested
and run everywhere. The enforcement tests (a write inside the workspace
succeeds; reading `~/.ssh` is refused; the network is refused) skip when `srt`
is not installed rather than failing a machine without it, so they assert the
restriction wherever `srt` is present and are a no-op where it is not. Wiring
an `srt` install into CI, and the Mac app's mode picker, are follow-ups.
