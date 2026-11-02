# Sandbox test matrix

This document maps each `srt` sandbox mode against the attack vectors the
sandbox is meant to limit, and states for each one what was observed, what
was not, and what it would take to observe it. It builds on the
[threat model](THREAT_MODEL.md), which describes the operating modes and the
trust boundaries in prose; this document is the test-by-test backing for the
sandbox row of that model.

The sandbox is `srt`
([anthropics/sandbox-runtime](https://github.com/anthropics/sandbox-runtime)),
which wraps each shell command the agent runs and enforces filesystem and
network rules on the process itself. It is a second wall behind the
`run_command` denylist and the approval gate, not a replacement for either:
the denylist and the approval preset decide what the model may ask for, and
the sandbox decides what a command that was allowed can actually touch. See
`docs/decisions/0141-the-agent-s-shell-commands-run-under-srt-in-chosen-modes.md`.

## Two facts to state plainly first

- **`off` is the default** (ADR 0141). A fresh install runs every shell
  command with no `srt` wrapper, exactly as it did before the sandbox
  existed. A mode is something a person turns on, through `--sandbox`,
  `ZORP_SANDBOX`, a trusted flavor, or the saved setting. Nothing a model
  writes can turn it on or change it, and a project flavor that tries to
  loosen the sandbox is dropped by the trust gate before it is read.

- **`srt` is not wired in on Windows** (ADR 0141). zorp does not ship `srt`
  on Windows and does not support its alpha Windows path, so in the shipped
  configuration there are no OS-level filesystem and network walls there.
  The code is not OS-gated, so the mode still decides what happens: `off`,
  the default, runs commands normally; a non-off mode finds `srt` absent and
  hits the loud refuse-when-missing, so the command does not run rather than
  running unsandboxed. (If someone put `srt` and `ripgrep` on a Windows
  `PATH` themselves, the code would invoke `srt`'s alpha Windows path; that
  is unsupported and is not what "not wired in" promises.) A Windows user who
  needs the walls runs zorp under WSL or a Linux container. The supervision,
  output capture and secret redaction described below apply on Windows
  regardless.

## The modes and what each one generates

`srt` is driven by a settings file that zorp generates in code, per run,
from the mode. The model never writes it. The settings below are the real
output of `settings_json` for a workspace at `/home/alice/project` with
`HOME=/home/alice`, printed by running the function directly.

`off` generates no settings and invokes no `srt`:

```
### off
(no settings; srt is never invoked)
```

`build`: writes confined to the workspace and its `scratch`, credential
stores unreadable, network open only to the named package registries.

```json
{
  "filesystem": {
    "allowWrite": ["/home/alice/project", "/home/alice/project/scratch"],
    "denyRead": [
      "/home/alice/.ssh", "/home/alice/.aws", "/home/alice/.gnupg",
      "/home/alice/.kube", "/home/alice/.config/gcloud",
      "/home/alice/.config/gh", "/home/alice/.docker", "/home/alice/.netrc",
      "/home/alice/Library/Keychains"
    ]
  },
  "network": {
    "allowedDomains": [
      "crates.io", "index.crates.io", "static.crates.io",
      "registry.npmjs.org", "pypi.org", "files.pythonhosted.org",
      "github.com", "api.github.com", "codeload.github.com",
      "objects.githubusercontent.com", "raw.githubusercontent.com"
    ]
  }
}
```

`workspace`: the same filesystem rules as `build`, network denied entirely.

```json
{
  "filesystem": {
    "allowWrite": ["/home/alice/project", "/home/alice/project/scratch"],
    "denyRead": [ "...the same nine credential paths as build..." ]
  },
  "network": { "allowedDomains": [] }
}
```

`read-only`: no writes anywhere, network denied entirely, credential stores
still unreadable.

```json
{
  "filesystem": {
    "allowWrite": [],
    "denyRead": [ "...the same nine credential paths..." ]
  },
  "network": { "allowedDomains": [] }
}
```

The loopback address is deliberately absent from every `allowedDomains`
list, including `build`'s, so the agent cannot reach zorp's own server from
inside the sandbox (ADR 0053). The in-code own-server deny runs regardless
of the sandbox.

## Which layer stops which vector

The issue's attack vectors are stopped, where they are stopped at all, by
five different layers. Naming the layer for each one is the honest way to
report this: `srt` does not stop all of them, and saying it does would be
wrong.

- **srt (OS filesystem and network):** writes outside the workspace,
  reading credential directories, and opening the network.
- **In-code policy (denylist, approval gate, own-server deny):** the
  `run_command` denylist and the approval preset decide what the model may
  ask for in the first place, and the own-server deny refuses a call to
  zorp's own loopback server (ADR 0053) whatever the sandbox says. This
  layer sits above the sandbox and runs in every mode, `off` included.
- **Process supervision (`ProcessGroup`, every build including Windows):**
  background and descendant processes, which are reaped with the group when
  the command finishes, is cancelled, or times out.
- **Output capture (`capture.rs`, every build):** malicious or oversized
  tool output, which is capped and has known secrets redacted before it
  reaches the model.
- **Harness boundary (not the sandbox):** prompt injection in retrieved and
  recalled content. Recalled text, skill bodies and memory blocks are fenced
  under a per-turn nonce marker in a `user` message, labelled as data, and
  grant no tool and loosen no approval. This is a property of the recall,
  memory and skill design, documented in the threat model and CLAUDE.md, and
  the sandbox neither adds to it nor takes from it.

## The matrix

Columns are the modes. `blocked` means the layer denies it, `confined` means
it is limited but not forbidden, `full` means no restriction (the `off`
baseline), and `n/a` means the vector is not this layer's job. The enforcing
layer is named in the last column.

| Vector | off | build | workspace | read-only | Layer |
| --- | --- | --- | --- | --- | --- |
| Write a file outside the workspace | full | blocked | blocked | blocked (no writes at all) | srt |
| Read a credential directory (`~/.ssh`, `~/.aws`, ...) | full | blocked | blocked | blocked | srt |
| Open the network to an arbitrary host | full | registries only | blocked | blocked | srt |
| Reach zorp's own loopback server | denied in code | denied in code | denied in code | denied in code | in-code own-server deny (ADR 0053) |
| Destructive command (e.g. `rm -rf`) outside the workspace | full | blocked (write-confined) | blocked (write-confined) | blocked (no writes) | srt |
| Destructive command inside the workspace | full | confined to workspace | confined to workspace | blocked (no writes) | srt write-confinement; denylist and approvals above it |
| Background / descendant process outlives the command | reaped | reaped | reaped | reaped | process supervision |
| Symlink or path traversal out of the write set | full | relies on srt path resolution | relies on srt path resolution | blocked (no writes) | srt |
| Malicious or oversized tool output | capped, secrets redacted | capped, secrets redacted | capped, secrets redacted | capped, secrets redacted | output capture |
| Prompt injection in retrieved / recalled content | fenced as data | fenced as data | fenced as data | fenced as data | harness boundary, not the sandbox |

## What was observed, and what was not

The evidence splits into three groups: what runs and asserts everywhere,
what runs here because `srt` is absent, and what is gated on `srt` being
installed and so was not exercised.

### Observed without `srt`, here and in CI

These are unit tests over the settings generation and the supervision and
capture layers. They do not need `srt` installed. Run with
`cargo test -p zorp-agent --lib sandbox::`. Each named test below asserted
and passed in this container. One other member of `sandbox::tests`,
`output_below_cap_is_not_dropped_after_head_fills`, fails here for an
unrelated root-only reason noted at the end of this section; it is not in
the list below and is not about any `srt` mode.

- `sandbox::srt::tests` (10 tests, all pass): modes parse and round-trip;
  the strictness ordering `off < build < workspace < read-only`; `off`
  generates no settings; `workspace` writes the workspace and `scratch` and
  opens no network; `read-only` writes nothing and opens no network; `build`
  opens the registries and never loopback; credential stores are in
  `denyRead` when there is a home; the config chain resolves flag then env
  then flavor then saved then off; `wrap` builds `--settings <file> --
  <shell> -c <cmd>`; a sandbox refusal is turned into a rule-naming denial
  and an ordinary error is left alone.
- `sandbox::tests` supervision and capture (pass): a completed shell reaps
  its background descendants; a timeout kills the descendant process group;
  cancellation kills the running group; output is capped and secrets
  redacted; secrets are snapshotted before the child runs and redacted even
  inside large output.

### Observed here because `srt` is absent

This container has no `srt` and no `bwrap` installed, which is exactly the
condition the refuse-when-missing rule is about. These assert here:

- `a_missing_srt_refuses_rather_than_running_unsandboxed`: with a mode other
  than `off` requested and `srt` missing, the command is refused with a
  message naming the mode and `srt`, and is never run unsandboxed. Observed
  passing in this environment.
- `off_mode_runs_the_shell_directly_even_without_srt`: `off` is unchanged
  with no `srt` present. Observed passing.

### Not observed: OS-level enforcement

The tests that would prove `srt` actually blocks a write, a credential read
or a network call are `workspace_mode_allows_a_write_inside_the_workspace`,
`workspace_mode_refuses_reading_a_credential_store`, and
`workspace_mode_refuses_the_network`. Each one begins with
`if !srt_usable() { return; }`, so on a machine with no `srt` it returns
without asserting. It reports as `ok` but checked nothing.

**`srt` is not installed in this container, and no CI runner installs it
either** (no job in `.github/workflows/` has a step that installs `srt`,
`bwrap` or `ripgrep` for the sandbox). So the OS-level enforcement cells in
the matrix above, the ones whose layer is `srt`, are **not exercised by any
automated test today.** They rest on the generated settings being correct,
which is tested, and on `srt` enforcing those settings, which is `srt`'s own
contract and is not re-tested here.

The one environmental note: in this root-only container,
`output_below_cap_is_not_dropped_after_head_fills` fails on a pipe-buffer
timing assumption unrelated to the sandbox modes. It passes in CI's normal
user environment. It is recorded here for completeness and is not a finding
about any `srt` mode.

## Gaps and remediation status

| Gap | Status | Remediation |
| --- | --- | --- |
| OS-level filesystem and network enforcement is not exercised by automation | Open | Install `srt`, `ripgrep` and (on Linux) `bwrap`, run as a non-root user, and run `cargo test -p zorp-agent --lib sandbox::`. The three `workspace_mode_*` tests then assert instead of skipping. A CI job that installs these would close this; none exists yet. |
| Symlink and path-traversal escape from the write set is not independently tested | Open | Covered in principle by `srt` resolving real paths before applying the write set, but zorp has no test that creates a symlink out of the workspace and confirms the write is denied. Add one alongside the `workspace_mode_*` tests, gated the same way on `srt` being present. |
| Windows has no OS-level wall | By design (ADR 0141) | `srt`'s Windows path is alpha and is not shipped. `off` (the default) runs normally; a non-off mode refuses commands because `srt` is missing, so there is no silent fall back to unsandboxed. A Windows user who needs the wall runs zorp under WSL or a Linux container. Supervision, output capture and redaction still apply on Windows. |
| Destructive commands inside the workspace are confined, not forbidden | By design | `srt` confines writes to the workspace; it does not decide that `rm -rf ./build` is unwanted. That judgement is the denylist's and the approval gate's, above the sandbox. `read-only` forbids all writes for a run that wants that floor. |
| Prompt injection is out of the sandbox's scope | By design | Handled by the harness boundary (fenced, nonce-marked, data-labelled recall, memory and skill content), documented in the [threat model](THREAT_MODEL.md). The sandbox is not a defense against it and is not claimed to be. |

## Reproducing the observed evidence

```
# Settings generation, supervision, capture, refuse-when-missing, off:
cargo test -p zorp-agent --lib sandbox::

# With srt present (closes the main gap), as a non-root user on Linux:
#   install srt + ripgrep + bwrap on PATH, then run the same command;
#   the three workspace_mode_* tests will assert instead of returning early.
```
