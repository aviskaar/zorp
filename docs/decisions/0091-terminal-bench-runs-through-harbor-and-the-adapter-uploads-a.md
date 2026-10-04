---
status: accepted
date: 2026-09-01
---

# Terminal-Bench runs through Harbor, and the adapter uploads a binary built from the tree

**Decision:** benchmark runs go through `harbor run --agent
evals.harbor.zorp_agent:ZorpAgent`, and the adapter uploads a Linux
`zorp-agent` built from the working tree by `evals/harbor/build-agent.sh`.
The recipe is `docs/benchmarks.md`.

**Why:** Terminal-Bench's own `tb` CLI stopped at 0.2.18 and no longer
carries the current datasets, so `tb run --dataset name==version` is a dead
end. The scientific suite is `terminal-bench-science/terminal-bench-science@latest`,
70 tasks, and there is no `0.1` of it. The adapter that was in
`evals/harbor/` had never run against real Harbor: it imported a
`TaskEnvironment` that does not exist, and its test stubbed the whole SDK
out with an invented interface, so it passed anyway. Both are rewritten
against the installed SDK, and the test now imports it for real.

**What it ruled out:** installing a published release inside the task
container. That is what the root `Dockerfile` does, and it would have saved
a cross-build, but it benchmarks the last tag instead of the checkout under
review.

**Two things the runs surfaced.** `--yes` is required, because a container
has no terminal and `ApprovalMode::terminal` otherwise resolves to
`NonInteractive`, which refuses every edit and command and reads like an
agent that did nothing. And the denylist binds shell redirects to the
process working directory, so a task that wants output written outside the
image's `WORKDIR` needs a tool call rather than a redirect. Neither is
changed here; both are written down.
