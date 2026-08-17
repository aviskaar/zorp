# UAT: cold install from the README

**Date:** 2026-08-17
**Build under test:** the published `v0.3.1` release, installed by the
README's `curl | bash` line. Not a local build.
**Environment:** `ubuntu:22.04` container, no `cargo`, no `rustc`, no
`node`, no `npm`, no `git`, no `curl` until the run adds it. Nothing
cached from any earlier run.

## Why this run exists

`launch/v0.3.1-ready-to-post.md` lists as a blocker: "Have someone who is
not the author install it cold, on their own machine, from the curl line,
and watch where they get stuck without helping them."

This is not that. The author's agent running a container is not a
stranger, and it cannot be surprised in the ways a stranger will be. What
it can do is remove every convenience a development machine provides and
follow only what the README says, which is the part of the exercise that
does not need a stranger. **The blocker stands.**

Method: no source edits, no local binaries, nothing on `PATH` but what
the installer put there. Every verdict below is observed output.

## What passed

| # | Scenario | Observed | |
|---|---|---|---|
| 1 | Install with no toolchain present | `cargo`, `rustc`, `node`, `npm`, `git` all absent; install completed | ✅ |
| 2 | Checksum verified, not skipped | `Checksum verified.` | ✅ |
| 3 | Prebuilt path taken, no source fallback | `Using prebuilt binaries for x86_64-unknown-linux-gnu.` | ✅ |
| 4 | All three binaries land | `zorp`, `zorp-agent`, `zorp-web` in `~/.local/bin` | ✅ |
| 5 | UI static files land | `dist index.html styles.css` in `~/.local/share/zorp/web` | ✅ |
| 6 | Binary starts (the GLIBC class) | `zorp-agent --version` ran, exit 0 | ✅ |
| 7 | `PATH` warning when `~/.local/bin` is not on it | Warning printed with the exact `export` line | ✅ |
| 8 | `zorp-agent --help` | Usage printed locally | ✅ |

Scenario 6 is the one worth naming. Two separate `GLIBC_2.39 not found`
failures were shipped before, so a released binary starting at all on
Ubuntu 22.04 is a regression test, not a formality.

## Findings

### J1 (medium, fixed here): the installer did not mention `zorp-web`

The success line read:

```
Successfully installed 'zorp' and 'zorp-agent' to /root/.local/bin!
```

It installs three binaries. `zorp-web` is the chat UI, the thing that
makes zorp usable by someone who does not live in a terminal, and it
arrived on the machine without being named. A user following the README
has no reason to know it is there.

Fixed: the message now names every binary actually installed, and adds
one line saying how to start the UI. Verified in the same clean
container:

```
Successfully installed 'zorp', 'zorp-agent' and 'zorp-web' to /root/.local/bin!
For the chat UI, run 'zorp-web' and open http://127.0.0.1:7777
```

The port is `zorp-web`'s real default (`default_value_t = 7777`), checked
against the source rather than written from memory.

### J2 (high, fixed in main, not yet released): the version is wrong

```
$ zorp-agent --version
zorp-agent 0.2.1
```

The release is v0.3.1. v0.2.1 is the release whose first message times
out on a cold model, and v0.3.1 is that fix, so a user who installed the
fix and checked was told they had not got it.

Fixed on main by #31, which also added a release-time check that refuses
a tag disagreeing with the manifest or the Dockerfile default. **Not
fixed for anyone downloading today.** The artifacts on the releases page
still report 0.2.1; only a new tag corrects that.

### J3 (medium, fixed in main, not yet released): `zorp --version` bills you

```
$ zorp --version
zorp: https://api.openai.com/v1/chat/completions: status code 401: {
    "error": { "message": "You didn't provide an API key. ..." }
}
```

The flag was joined into the prompt and sent to the model. A new user's
first impression is a wall of OpenAI's JSON, and a user with a key
configured pays for it.

Fixed on main by #33. Same caveat as J2: not fixed for anyone
downloading today.

### J4 (low, not fixed): the container path in the README does not work

`docker pull ghcr.io/aviskaar/zorp:latest` answers `unauthorized`. The
image is published and its workflow is green; the package is private.
Tracked as #30, needs a maintainer with `admin:packages`. The README now
says so and offers `docker build` instead.

## Verdict

**ACCEPT with reservations.** The install path itself is sound: it works
on a machine with nothing on it, verifies what it downloads, and puts
everything where the README says. Every finding is about what the user is
*told* afterwards, not about whether the software arrived.

Two of the four findings are already fixed on main and invisible to users
until a release is cut. That is the argument for cutting one: J2 and J3
are live for every person who installs zorp right now.
