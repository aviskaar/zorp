# Sandbox test matrix

This document maps each `srt` sandbox mode against the attack vectors the
sandbox is meant to limit, and states for each one what was observed, what
was not, and what it would take to observe it. It builds on the
[threat model](THREAT_MODEL.md), which describes the operating modes and the
trust boundaries in prose; this document is where the sandbox row of that
model is checked against the tests, including, honestly, the cells no
automated test backs yet.

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
from the mode. The model never writes it. The `build` block below is the
verbatim output of `settings_json` for a workspace at `/home/alice/project`
with `HOME=/home/alice`, printed by running the function directly. The
`workspace` and `read-only` blocks are the same output with the `denyRead`
array abbreviated, since it is byte for byte the nine paths `build` lists;
the function emits those nine concrete paths in every non-`off` mode.

`off` is the exception: `settings_json` returns `None` for it and prints
nothing, because `off` never invokes `srt`. There is no settings block to
show.

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
inside the sandbox (ADR 0053). Under `zorp-web`, which configures the server
port through `Policy::with_own_server`, the in-code own-server deny also
refuses that call regardless of the sandbox mode; a bare `zorp-agent` CLI
run has no such server and does not add that deny, so there the network wall
in the non-off modes is what keeps loopback out.

## Which layer stops which vector

The issue's attack vectors are stopped, where they are stopped at all, by
five different layers. Naming the layer for each one is the honest way to
report this: `srt` does not stop all of them, and saying it does would be
wrong.

- **srt (OS filesystem and network):** writes outside the workspace,
  reading credential directories, and opening the network.
- **In-code policy (denylist, approval gate, own-server deny):** the
  `run_command` denylist and the approval preset decide what the model may
  ask for in the first place, and sit above the sandbox in every mode, `off`
  included. The own-server deny refuses a call to zorp's own loopback server
  (ADR 0053) whatever the sandbox says, but only when a server port is
  configured: `Policy::with_own_server` is called by `zorp-web`, so a bare
  `zorp-agent` CLI run (which has no own server to protect) does not add that
  deny. There, loopback is blocked instead by the `srt` network wall in every
  non-off mode, and in `off` there is no server and no wall.
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

Columns are the modes. A cell is a short phrase for what happens to that
vector under that mode: `full` means no restriction (the `off` baseline),
`blocked` means it is denied outright, and other cells spell out the partial
case (for example `registries only` for the network under `build`, or
`reaped` for a background process that is killed with its group). The last
column names the layer that does the work, from the five layers above.

| Vector | off | build | workspace | read-only | Layer |
| --- | --- | --- | --- | --- | --- |
| Write a file outside the workspace | full | blocked | blocked | blocked (no writes at all) | srt |
| Read a credential directory (`~/.ssh`, `~/.aws`, ...) | full | blocked | blocked | blocked | srt |
| Open the network to an arbitrary host | full | registries only | blocked | blocked | srt |
| Reach zorp's own loopback server | code (zorp-web); not blocked on bare CLI | srt net wall (+ code under zorp-web) | srt net wall (+ code under zorp-web) | srt net wall (+ code under zorp-web) | own-server deny under zorp-web; srt network wall in non-off modes |
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

### Observed without `srt`

These are unit tests over the settings generation and the supervision and
capture layers. They do not need `srt` installed. CI exercises them on every
pull request: the Linux and macOS build-and-test job runs the whole
workspace (`cargo test --workspace --exclude zorp-track`), which includes
these tests, and the Windows job runs the `sandbox::` filter explicitly
(where the `#[cfg(unix)]` enforcement and refuse tests do not compile, so
only the cross-platform ones run there). Each named test below asserted and
passed in this container; use the exact command in the Reproducing section
at the end, which skips the one environment-dependent test. One other member of `sandbox::tests`,
`output_below_cap_is_not_dropped_after_head_fills`, fails here for a reason
that is about output redaction and this container's environment, not about
any `srt` mode; it is not in the list below, and it is analyzed in full at
the end of this section.

- `sandbox::srt::tests` (10 tests, all pass): modes parse and round-trip;
  the strictness ordering `off < build < workspace < read-only`; `off`
  generates no settings; `workspace` writes the workspace and `scratch` and
  opens no network; `read-only` writes nothing and opens no network; `build`
  opens the registries and never loopback; credential stores are in
  `denyRead` when there is a home; the config chain resolves flag then env
  then flavor then saved then off; `wrap` builds `--settings <file> --
  <shell> -c <cmd>`; a sandbox refusal is turned into a rule-naming denial
  and an ordinary error is left alone.
- `sandbox::tests` supervision and capture (pass): a completed shell kills
  its background descendants
  (`completed_shell_kills_background_descendants_before_joining_readers`); a
  timeout kills the descendant process group
  (`timeout_kills_descendant_process_group`); cancellation kills the running
  group (`cancellation_kills_running_group`); output is capped and secrets
  redacted (`caps_and_redacts_output`,
  `redacts_longest_overlapping_secret_before_truncation`); secrets are
  snapshotted before the child runs (`snapshots_secrets_before_the_child_runs`).
- `sandbox::capture::tests` redaction (pass): a secret inside large output is
  still redacted (`secret_inside_large_output_is_still_redacted`), large
  output with no secret bytes passes through unchanged
  (`large_output_without_secret_bytes_passes_through_unchanged`), and
  streaming redaction holds across chunk boundaries
  (`streaming_redaction_retains_bounded_output_and_cross_chunk_secrets`).

### Observed here because `srt` is absent

This container has no `srt` and no `bwrap` installed, which is exactly the
condition the refuse-when-missing rule is about:

- `a_missing_srt_refuses_rather_than_running_unsandboxed`: with a mode other
  than `off` requested and `srt` missing, the command is refused with a
  message naming the mode and `srt`, and is never run unsandboxed. This test
  only asserts when `srt` is absent, which is the case here. Observed passing.

`off_mode_runs_the_shell_directly_even_without_srt` also passes here, but it
is not evidence about `srt`'s absence: `off` short-circuits in `srt_settings`
before any detection, so it runs the shell directly whether or not `srt` is
installed. It confirms `off` is unchanged, nothing more.

### Not observed: OS-level enforcement

Three tests exercise `srt`'s behavior directly, and each begins with
`if !srt_usable() { return; }`, so on a machine with no `srt` it returns
without asserting. It reports as `ok` but checked nothing:

- `workspace_mode_allows_a_write_inside_the_workspace` asserts that an
  allowed write (inside the workspace) succeeds. It is the positive case, not
  a block.
- `workspace_mode_refuses_reading_a_credential_store` asserts a read of
  `~/.ssh` is refused.
- `workspace_mode_refuses_the_network` asserts a `curl` to a remote host is
  refused.

Note what is not here: no test asserts that a write *outside* the workspace
is denied. That cell of the matrix (the top row) rests on `srt`'s
write-confinement contract and the generated `allowWrite` list being
correct, both of which this suite does not exercise end to end. It is listed
as a gap below.

**`srt` is not installed in this container, and no CI runner installs it
either** (no job in `.github/workflows/` has a step that installs `srt`,
`bwrap` or `ripgrep` for the sandbox). So the OS-level enforcement cells in
the matrix above, the ones whose layer is `srt`, are **not exercised by any
automated test today.** They rest on the generated settings being correct,
which is tested, and on `srt` enforcing those settings, which is `srt`'s own
contract and is not re-tested here.

One capture test fails in this environment, and the honest finding is that
it is not about any `srt` mode and not the timing flake it first looks like.
`output_below_cap_is_not_dropped_after_head_fills` runs `printf 12345678`
with an output cap of 10 and expects `12345678` back. It fails here
deterministically (three identical runs), and the cause is secret redaction,
not timing or root. `secret_values` treats any environment variable whose
name contains `KEY`, `TOKEN`, `SECRET` or `PASSWORD` as holding a secret and
redacts its value from command output. This container sets
`CCR_AUTO_MODE_USER_ENV_KEYS_FACT=1`: the name matches `KEY`, so the one byte
`1` is treated as a secret, the `1` in `12345678` is replaced with
`[REDACTED]`, and the expanded output runs past the cap and is truncated.
The test passes in an environment with no such short secret-named variable.
This is a test-isolation and heuristic problem in the capture layer: the
heuristic (`secret_values` and its `KEY`/`TOKEN`/`SECRET`/`PASSWORD` name
match) lives in `sandbox/mod.rs`, and `capture.rs` applies whatever secrets
it is handed. It is unrelated to the sandbox modes this document is about,
is tracked as issue #327, and is left out of this change to keep it to one
subject.

## Gaps and remediation status

| Gap | Status | Remediation |
| --- | --- | --- |
| OS-level filesystem and network enforcement is not exercised by automation | Open | Install `srt`, `ripgrep` and (on Linux) `bwrap`, run as a non-root user, and run `cargo test -p zorp-agent --lib sandbox::`. The three `workspace_mode_*` tests then assert instead of skipping. A CI job that installs these would close this; none exists yet. |
| A write outside the workspace has no denial test | Open | The three `workspace_mode_*` tests cover an allowed write, a credential read and the network, but none asserts that a write outside the workspace is denied. Add a `workspace_mode_refuses_a_write_outside_the_workspace` test, gated the same way on `srt` being present, so the matrix's top row is backed directly rather than resting on the `allowWrite` list alone. |
| Symlink and path-traversal escape from the write set is not independently tested | Open | Covered in principle by `srt` resolving real paths before applying the write set, but zorp has no test that creates a symlink out of the workspace and confirms the write is denied. Add one alongside the `workspace_mode_*` tests, gated the same way on `srt` being present. |
| Windows has no OS-level wall | By design (ADR 0141) | `srt`'s Windows path is alpha and is not shipped. `off` (the default) runs normally; a non-off mode refuses commands because `srt` is missing, so there is no silent fall back to unsandboxed. A Windows user who needs the wall runs zorp under WSL or a Linux container. Supervision, output capture and redaction still apply on Windows. |
| Destructive commands inside the workspace are confined, not forbidden | By design | `srt` confines writes to the workspace; it does not decide that `rm -rf ./build` is unwanted. That judgement is the denylist's and the approval gate's, above the sandbox. `read-only` forbids all writes for a run that wants that floor. |
| Prompt injection is out of the sandbox's scope | By design | Handled by the harness boundary (fenced, nonce-marked, data-labelled recall, memory and skill content), documented in the [threat model](THREAT_MODEL.md). The sandbox is not a defense against it and is not claimed to be. |

## Reproducing the observed evidence

```
# Settings generation, supervision, capture, refuse-when-missing, off.
# Skip the one test that is environment-dependent (issue #327): it fails
# wherever a short secret-named env var matches the test output.
cargo test -p zorp-agent --lib sandbox:: -- \
  --skip output_below_cap_is_not_dropped_after_head_fills

# With srt present (closes the main gap), as a non-root user on Linux:
#   install srt + ripgrep + bwrap on PATH, then run the same command;
#   the three workspace_mode_* tests will assert instead of returning early.
```
