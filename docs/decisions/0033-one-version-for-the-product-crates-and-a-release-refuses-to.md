---
status: accepted
date: 2026-08-17
---

# one version for the product crates, and a release refuses to disagree with it

**Decision:** `zorp`, `zorp-agent`, `zorp-mcp`, `zorp-track`, `zorp-eval`
and `zorp-web` inherit a single `[workspace.package] version`, bumped
with each release tag. `erbga` keeps its own version. A tag push whose
tag, workspace version and Dockerfile `ARG VERSION` default disagree
fails the release.

**Why:** the versions had drifted from the tags without anyone noticing.
`zorp` sat at 0.1.0 and `zorp-agent` at 0.2.1 across both v0.3.0 and
v0.3.1, so the published v0.3.1 binary answered `--version` with 0.2.1.
That is the release whose first message times out on a cold model, and
v0.3.1 is its fix, so every user who installed the fix and checked was
told they had not got it. The Dockerfile default had rotted the same
way, which meant a bare `docker build` fetched the broken release on
purpose.

**What it rules out:** independent per-crate versioning for the product
crates. If one of them ever needs to be published to crates.io on its
own cadence, this has to be revisited. Nothing needs that today, and the
cost of the current scheme was a public release that lied about which
release it was.

`erbga` is excluded because it is standalone published prior work that
does not ship with zorp, and giving it zorp's release number would claim
a relationship that does not exist.
