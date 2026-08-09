# Telemetry Example

Live OpenTelemetry tracing for `zorp-agent`, viewed in Jaeger.

https://github.com/user-attachments/assets/c6076e3a-ffc3-46d5-9cf5-d0f2189f4425

`zorp-agent` ships real span instrumentation behind the optional `otel`
Cargo feature (`zorp-agent/src/main.rs`, `agent.rs`, `model.rs`). When
enabled, it exports traces over OTLP/HTTP using the standard
`OTEL_EXPORTER_OTLP_ENDPOINT` environment variable — no mocking involved.

## Span hierarchy

```
agent_run                  one call to Agent::run / Agent::resume
└── agent_step              one iteration of the agent loop
    ├── model completion     LLM call span (model.rs)
    └── tool_span             one span per tool invocation
```

Key attributes: `zorp.task` (secret-redacted), `zorp.step_number`,
`zorp.max_steps`, tool name, and step latency.

## Usage

```bash
./run.sh
```

This will:

1. Start Jaeger via `docker-compose.yml` (OTLP HTTP receiver on `:4318`,
   UI on `:16686`).
2. Build `zorp-agent` with `--features otel`.
3. Launch `zorp-agent chat`.

Open http://localhost:16686, select the `zorp-agent` service, and watch
the `agent_run` → `agent_step` → tool/model spans populate as you chat.

## Manual setup

```bash
docker compose up -d

export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_SERVICE_NAME=zorp-agent

cargo build -p zorp-agent --features otel
./target/debug/zorp-agent chat
```

## Teardown

```bash
docker compose down
```
