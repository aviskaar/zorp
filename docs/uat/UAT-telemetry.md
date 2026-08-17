# zorp-agent: UAT, the telemetry surface

**Date:** 2026-08-17
**Build under test:** `zorp-agent 0.2.1` at `7f8ebaa`, debug binary
**Backends:** local Ollama, chat model `qwen3:4b`; Jaeger all-in-one 1.60
via `examples/telemetry/docker-compose.yml`
**Scope:** telemetry only. Runs 001 and 002 covered 67 scenarios and none
of them touched this surface, so this is an addition to that baseline
rather than a re-run.

Two layers are in scope. The JSONL trace file behind `ZORP_TRACE_FILE`,
which `zorp-eval` consumes, and the OpenTelemetry export behind the
`otel` Cargo feature, which the example wires to Jaeger.

---

## Executive summary

| Metric | Result |
|---|---|
| Total scenarios | **12** |
| Pass | **12** |
| Fail | **0** |
| Not runnable | **0** |
| Blocking defects | **0** |

**Verdict: ACCEPT.** Both layers work. The trace file is well-formed and
records failures as clearly as successes. The OpenTelemetry export
produces one connected trace per run with the documented hierarchy and
attributes, and secret redaction holds where it claims to.

Three low-severity findings, all documentation or noise, none affecting
correctness.

---

## Findings

| ID | Severity | Finding |
|---|---|---|
| I1 | 🟡 Low | `examples/telemetry/README.md` documents the span hierarchy as `agent_run / agent_step / model completion / tool_span`. The emitted names are `agent_run`, `agent_step`, `model_complete`, `tool_execute`. Someone searching Jaeger for `tool_span` finds nothing. |
| I2 | 🟡 Low | The HTTP client's internal spans (`encode_headers`, `parse_headers`) are exported as their own root traces, so one agent run shows up in Jaeger as the real trace plus two single-span traces. Noise rather than error, but it makes the trace list confusing on a busy service. |
| I3 | 🟡 Low | The example README calls `zorp.task` "secret-redacted". True, and worth stating more precisely: what is redacted is the value of secret-bearing environment variables. A credential typed directly into a prompt that is not also in the environment is exported verbatim, which is correct behavior for `redact_secrets` and not what a hurried reader will assume. |

---

## Layer 1: the JSONL trace file

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | `ZORP_TRACE_FILE` creates a file | 8 lines written for a one-tool run | ✅ |
| 2 | Every line is valid JSON | 0 invalid lines | ✅ |
| 3 | Event types are meaningful | `run.start`, `turn`, `tool.call`, `tool.result`, `turn`, `assistant.claim`, `termination`, `run.end` | ✅ |
| 4 | `seq` is present and monotonic | `[0,1,2,3,4,5,6,7]` | ✅ |
| 5 | A run has both a start and an end | `run.start` first, `run.end` last | ✅ |
| 6 | A denied tool call is distinguishable | `tool.result` carries `success: false` | ✅ |
| 7 | A failure terminates properly | `ZORP_MAX_STEPS=1` on a multi-step task gives `termination` with `reason: step_limit` | ✅ |

Scenario 6 is the one that matters most for anything consuming these
traces. A denial and a success are not the same event, and the format
says so without the reader having to infer it from context.

## Layer 2: OpenTelemetry

| # | Test | Observed | Verdict |
|---|---|---|---|
| 8 | `--features otel` compiles | clean, 15s incremental | ✅ |
| 9 | `--features research,otel` compiles | clean. A feature combination nothing in CI builds | ✅ |
| 10 | The binary survives no collector listening | `OTEL_EXPORTER_OTLP_ENDPOINT` pointed at a dead port: answered normally, exit 0, 3s, no hang | ✅ |
| 11 | Spans reach Jaeger | service `zorp-agent` registered; one connected trace rooted at `agent_run`, containing `agent_step` twice, `model_complete` twice, `tool_execute` once | ✅ |
| 12 | Documented attributes are present | `zorp.task`, `zorp.step_number`, `zorp.max_steps` all present, alongside `zorp.model`, `zorp.tool_name`, `zorp.tool_arguments`, `zorp.tool_summary`, `zorp.tools_provided`, `zorp.messages_sent` | ✅ |

Scenario 10 is worth calling out. Telemetry that hangs the agent when its
collector is down would be worse than no telemetry, and it does not.

### Secret redaction, checked twice

The claim in the example README is that `zorp.task` is redacted. Tested
both readings, because the first result looked like a defect and was not.

A token that appears only in the prompt text is exported verbatim:

```
zorp.task = 'here is a token sk-LEAKCHECK-4471 please reply with exactly ok'
```

A secret that is actually in the environment is replaced:

```
ZORP_API_KEY=sk-ENVSECRET-7788
zorp.task = 'echo back this key [REDACTED] and reply ok'
env secret leaked into spans: 0        REDACTED markers: 14
```

That is `redact_secrets` working as designed: it scrubs the values of
secret-bearing environment variables from everything that leaves the
process. It is not, and does not claim in code to be, a general
credential detector. Finding I3 is about the wording, not the behavior.

---

## Methodology notes

- One tester, sequential. Isolated `ZORP_STATE_DB` and `ZORP_TRUST_FILE`
  under `/tmp` for every run, as in runs 001 and 002.
- The model was `qwen3:4b`, deliberately. Telemetry should not depend on
  model size, and using the small model keeps this consistent with the
  small-model work done alongside it.
- Docker was available, so the example's Jaeger stack ran for real and
  spans were verified through Jaeger's HTTP API (`/api/services`,
  `/api/traces`) rather than by looking at the UI. Containers were
  stopped with `docker compose down` afterward.
- Scenario 11 was initially recorded as a failure because the first trace
  returned by the API contained only `parse_headers`. That was the
  tester reading one trace instead of all of them. The correct result is
  above, and the mistake is what produced finding I2.
- Not covered: `examples/telemetry/run.sh` end to end, because it launches
  an interactive chat session. Its constituent parts, the compose file,
  the otel build, and the export path, were each exercised separately.
