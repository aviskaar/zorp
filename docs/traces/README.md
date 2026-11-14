# Published investigation traces

No trace is published yet. This directory is where the first one goes
(issue #306).

**TODO (needs a person):** run one real investigation and commit what it
recorded here. A trace in this directory must come from an actual run.
Nothing here may be written by hand, reconstructed, or generated to look
like a run: a made-up trace is worse than none, because it looks exactly
like a real one.

## What a run needs

- A real, bounded question whose answer is not already known.
- A model endpoint and `ZORP_API_KEY`. Each attempt is a full agent run
  and costs money, which is why nobody has run this unattended.
- A build with the research feature:

  ```bash
  cargo build -p zorp-agent --features research
  ```

## Steps

Run these in an empty directory that will hold the project, and keep the
exact command lines, since they are part of what gets published.

```bash
zorp-agent validate "<question>"
zorp-agent investigate "<question>" \
  --metric-name <name> \
  --kill-threshold <number> \
  --threshold-direction <lower-is-better|higher-is-better>
# repeat investigate for each attempt, including the ones that fail
zorp-agent co-write "<question>"
zorp-agent critique "<question>"
zorp-agent deliver "<question>"
```

Answer checkpoints yourself rather than passing `--yes`, so the human
decisions in the record are real ones. Set `ZORP_FORECAST=1` if the
trace should include forecasts.

## What to publish

Make one subdirectory per trace, for example `docs/traces/<slug>/`, with:

- the `.zorp/tracks/<track_id>/` directory as the run left it, including
  the git-backed pre-registration and kill threshold
- the machine-readable record exported from the project store
- every attempt, failed ones included
- the sources and the supporting and contradicting evidence
- the human checkpoint decisions, kept apart from model suggestions
- the final report and the claim-to-record mapping
- a `RERUN.md` with the zorp commit, the model id, the environment
  variables set (never the key) and the exact commands above

The issue is done when a stranger can inspect that record and rerun it
from the published steps.
