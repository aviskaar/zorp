# Running Terminal-Bench against zorp-agent

Terminal-Bench drives an agent inside a container against a task, then runs
the task's own tests against whatever the agent left behind. This page is the
recipe for pointing it at `zorp-agent`.

## The harness is Harbor, not `tb`

Terminal-Bench moved off its own CLI. The `terminal-bench` PyPI package and
its `tb run --dataset name==version` command stopped at 0.2.18 in September
2025 and no longer carry the current datasets. Datasets now live on Harbor
Hub and are run with `harbor run -d org/name@version`.

The dataset for scientific research tasks is:

    terminal-bench-science/terminal-bench-science@latest

70 expert-curated tasks across five scientific domains. There is no `0.1`
version of it and no `terminal-bench-science==0.1` to select. `@latest` is
the only published ref. The dataset needs no Harbor account to download.

Other datasets work the same way, for example
`terminal-bench/terminal-bench@latest` for the general terminal suite and
`harbor/hello-world@latest` for a one file smoke target.

## What you need

    pip install 'harbor>=0.22'
    evals/harbor/build-agent.sh

The adapter is written against Harbor 0.22. Older releases lack
`harbor.agents.model_connection` and the adapter will not import, which
the adapter tests below report as a failure rather than a skip.

The build script compiles `zorp-agent` for Linux inside a `rust:slim-bookworm`
container and leaves it at `target/harbor/<arch>/zorp-agent`. The adapter
uploads that binary into each task container, so a run benchmarks the working
tree rather than a published release. It defaults to the Docker daemon's own
architecture. Pass `linux/amd64` if a dataset ships amd64-only images. The
build caches its registry and object directory in named Docker volumes, so a
rebuild after an edit takes seconds rather than a full compile.

`ZORP_AGENT_BIN` overrides the path if you already have a Linux build.

## Pointing it at a model

Harbor resolves `-m provider/model` into an endpoint and a key, and the
adapter maps that onto zorp's own variables: `ZORP_MODEL`, `ZORP_BASE_URL`,
`ZORP_API_KEY`, and `ZORP_PROVIDER=anthropic` when the provider is Anthropic.
Every other provider gets zorp's default OpenAI wire format.

`-m` is the source of truth for those four. Any other `ZORP_*` knob is passed
straight through with `--ae`, for example `--ae ZORP_MAX_STEPS=40` or
`--ae ZORP_HTTP_TIMEOUT_SECS=600`.

A hosted model:

    export ANTHROPIC_API_KEY=...
    harbor run ... -m anthropic/claude-opus-5

A local Ollama, which the container reaches through the host gateway:

    export OPENAI_BASE_URL=http://host.docker.internal:11434/v1
    harbor run ... -m openai/gemma4:e4b

A local oMLX, same gateway, which wants its key:

    export OPENAI_BASE_URL=http://host.docker.internal:8000/v1
    export OPENAI_API_KEY=$(jq -r .auth.api_key ~/.omlx/settings.json)
    harbor run ... -m openai/Qwen3.6-35B-A3B-MLX-4bit

Tell zorp how big a local model's window is, or it will not compact: zorp
never guesses a context window (`docs/DECISIONS.md`, 2026-08-19), and a
sixty-step run that is never compacted reaches 120k tokens of prompt. Pass
`--ae ZORP_CONTEXT_TOKENS=32768`, or whatever the server is configured to
serve, on every local run.

## Running it

Run from the repo root, with the repo root on `PYTHONPATH` so Harbor can
import the adapter by path.

Inside the container the agent runs from `/`, so the path policy's root is
the whole filesystem and a task may write to `/results` or `/root/results`
as its instruction says. The file tools take absolute paths as a result.

One task:

    PYTHONPATH=$PWD harbor run \
      -d terminal-bench-science/terminal-bench-science@latest \
      -i terminal-bench-science/linked-cell-suppression \
      --agent evals.harbor.zorp_agent:ZorpAgent \
      -m anthropic/claude-opus-5 \
      -o jobs -n 1

The whole dataset, which is 70 tasks and a lot of model calls:

    PYTHONPATH=$PWD harbor run \
      -d terminal-bench-science/terminal-bench-science@latest \
      --agent evals.harbor.zorp_agent:ZorpAgent \
      -m anthropic/claude-opus-5 \
      -o jobs -n 8

`-i` takes the fully qualified task name, `terminal-bench-science/<task>`, and
accepts globs. `-k` sets attempts per task. `--install-only` does setup and
exits, which checks the binary and the image without spending a model call.
These tasks are sized for a full working day of expert effort and their agent
timeout is eight hours, so bound a scouting run with
`--agent-timeout-multiplier` or `--ae ZORP_MAX_STEPS=<n>`.

## What a result looks like

    ┏━━━━━━━━┳━━━━━━━━━━━━┳━━━━━━━┓
    ┃ Trials ┃ Exceptions ┃  Mean ┃
    ┡━━━━━━━━╇━━━━━━━━━━━━╇━━━━━━━┩
    │      1 │          0 │ 0.000 │
    └────────┴────────────┴───────┘

Mean is the mean reward. Exceptions counts trials that broke rather than
scored, and it should be zero: a trial where the agent gave up is a scored
zero, not an exception. Per trial output lands under
`jobs/<job>/<task>__<id>/`, with `result.json` holding the reward and
`agent/zorp-agent.txt` holding what the agent printed. `harbor view jobs`
opens a browser over the lot.

## Approval, and what `--yes` does not wave through

The adapter runs `zorp-agent --yes`. Without it a container has no terminal
to ask, `ApprovalMode::terminal` resolves to `NonInteractive`, and every edit
and command is refused, which looks like an agent that simply did nothing.

`--yes` answers the asks an approval preset produces. It does not touch the
hard denylist in `zorp-agent/src/policy.rs`, and `agent.rs` has a test saying
so. The denylist still refuses `sudo`, backticks, unbalanced command
substitution, `eval`, `git push`, `mkfs`, and recursive force `rm` outside
the working directory.

One denylist rule is worth knowing before reading a low score. Shell
redirects must land inside the policy root, and the policy root is the
process working directory, which in a task container is the image's
`WORKDIR`. So `solve.py > out.json` is allowed and
`solve.py > /root/results/out.json` is denied. A task that asks for output
somewhere else needs the agent to write the file with a tool rather than
redirect into it.

## Checking the adapter

    python -m unittest discover -s evals/harbor -t .

These run against the real Harbor SDK. Instantiating `ZorpAgent` is itself
the main assertion, because `BaseInstalledAgent` is abstract and a drifted
interface fails there.
