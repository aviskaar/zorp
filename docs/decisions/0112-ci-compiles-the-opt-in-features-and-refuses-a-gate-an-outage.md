---
status: accepted
date: 2026-09-05
---

# CI compiles the opt-in features, and refuses a gate an outage can redden

**Decision:** a new `features` job compiles the cheap non-default features
on every pull request, `zorp-track`'s `library` feature joins the nightly
`research` job only, `zorp-agent`'s `library` feature stays ungated because
it does not fit on a runner, and the `research-pr` path filter is widened
to the files those jobs actually compile. No Harbor job.

**Why:** `cargo test --workspace` resolves default features only. Every
opt-in feature in this workspace was therefore dead to CI: `otel`,
`search`, `clipboard` and `library` on `zorp-agent`, `search`, `memory`
and `voice` on `zorp-web`, and `library` on `zorp-track`. A
`#[cfg(feature = ...)]` block could stop compiling and nothing would go
red until somebody turned the feature on. `recall` and `research` were
already covered and are not duplicated.

**Cheap on the pull request path.** `otel`, `search` and `clipboard` add
no heavy dependency, so they are one `cargo clippy --all-targets --
-D warnings` invocation with no path filter: none of the three has a test
the default build does not already run, so the question is only whether
the code still compiles, and clippy answers it and lints code no lint had
seen. `zorp-web`'s `search`, `memory` and `voice` do have tests of their
own and get `cargo test`. The job runs in about a minute warm.

**`library` is gated nightly, and only half of it, and the reason is
runner disk.** This job compiles the bundled DuckDB amalgamation once per
feature set it resolves, because each one gives `libduckdb-sys` a
different metadata hash and none of them shares a build with the others,
and a debug build of it is enormous. The runner starts with about 87 GB
free and three of those builds is already most of it. Adding
`cargo test -p zorp-track --features library` makes a fourth plus the
whole arrow tree, and it failed with

    ar: .../libduckdb.a: error reading .../ub_src_function_cast_variant.o:
    No space left on device

which reads like a compiler error and is not one. Deleting the android,
CodeQL, dotnet, swift and ghc trees the workspace never touches reclaims
about 22 GB, measured as 87 GB free going to 108 GB, and that is the
difference between the fourth build fitting and not; `df -h /` is printed
on both sides of the deletion so the headroom stays visible rather than
guessed at. A fifth, `cargo check -p zorp-agent --features library`, did
not fit even then, and the run that proved it took 47 minutes.

So `zorp-track`'s `library` runs nightly and on pushes to main, never on a
pull request, and `zorp-agent`'s `library` is not gated at all. What that
leaves uncovered is one `#[cfg]` in validate calling an API the nightly
step already compiles and tests. A gate that goes red for a reason with
nothing to do with the code is the thing this repo refuses, and an ungated
opt-in feature is the smaller problem: CLAUDE.md already says to leave
`library` off unless you are working on retrieval. `research-pr` is back to
the three research builds and finishes in about fourteen minutes.

**The path filter names what the jobs compile, and not documentation.**
It missed `erbga/`, which `zorp-track`'s search layer depends on, all of
`zorp-agent/tests/`, `src/lib.rs` and `src/main.rs` on both crates,
`zorp-web/src/api.rs`, and the workflow file itself, so a change to the
research jobs could not run the research jobs. `docs/` stays out, specs
included: a documentation change cannot break a build, and a twenty
minute job that fires on prose is a job people learn to ignore. The
nightly run is the backstop for whatever the filter cannot name.

**What was refused: a Harbor adapter smoke check.** The adapter exists at
`evals/harbor/zorp_agent.py` and it has tests, but those tests skip
themselves when Harbor is not installed, on purpose, because a stubbed
version of them passed for a year while the adapter could not be
imported. So the only ways to run them in CI are to install Harbor from
PyPI, which puts a package index in the critical path of a gate, or to
run them without it and collect eight skips, which is a green check that
checks nothing. Compiling the file with `py_compile` instead would be the
same false comfort in a smaller package. A gate an upstream outage can
turn red teaches people to ignore red, so this one is not added. Nothing
in the jobs above touches the network: the search tests use a nonsense
key and never spend a search, and the memory and voice tests run their
servers on loopback. The entry below draws the same line between
`zorp-eval`'s two halves, for the same reason.

**Also:** `jobs/` is gitignored. It is per-run local scratch written into
the repository root, and it was showing up untracked in every status.
