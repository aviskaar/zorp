---
status: accepted
date: 2026-10-26
---

# Windows runs commands under Git Bash, in a job object

**Decision:** zorp ships Windows binaries (`x86_64-pc-windows-msvc`, a
`.zip` on every release). The agent's shell tools run each command under Git
for Windows' `sh.exe`, found as `ZORP_SH`, then an `sh.exe` on `PATH`, then
the `bin\sh.exe` beside a `git.exe` on `PATH`, then the installer's own
locations, and never WSL's `bash.exe`. The shell and everything it starts are
held in a job object, which a timeout, a cancel, a finished command or a
background kill ends with `TerminateJobObject`, the way a Unix process group
is ended with `killpg`. `sandbox::ProcessGroup` is the one type both
platforms go through. With no Git Bash the tool says so and runs nothing.

**Why:** The `run_command` denylist and every approval rule read POSIX shell
syntax. Running PowerShell or `cmd` instead would need a second denylist for a
second command language, and a rule missing from one of them is a command
that runs unasked. Git Bash keeps the language the same on every platform.

**What it costs:** A child runs for a moment before it is assigned to its job,
so a grandchild started in that window would escape it. Git Bash takes far
longer than that to start anything. A path the model writes as `/c/...` does
not match a repository root written `C:\...`, so an absolute target inside the
repository is refused rather than allowed, which is the safe direction.

**What it rules out:** A native Windows desktop app is a separate piece of
work. Only the sandbox's tests run on Windows in CI; the rest of the suite
still assumes a Unix host.
