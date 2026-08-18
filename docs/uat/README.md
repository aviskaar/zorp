# User acceptance tests

Hand-run acceptance tests of the real `zorp-agent` binary against a live
model. These are black-box runs: no source edits, no test-suite
shortcuts, every verdict backed by observed output. They sit alongside
`cargo test`, not in place of it. The suite proves the mechanisms behave
as specified; these runs prove the shipped binary does.

The method follows quecto's own reports, preserved in
[`../upstream-quecto/uat/`](../upstream-quecto/uat/), and adds Area E for
the four research capabilities, which quecto never had.

## The series

| Run | Report | Build | Scope | Result |
|---|---|---|---|---|
| 001 | [`UAT-report.md`](UAT-report.md) | `zorp-agent 0.2.1`, debug, `--features research` | First zorp UAT. 67 scenarios across five areas: core CLI, tools and safety, persistence, flavors and trust, research capabilities. | ACCEPT with one medium finding. 64 pass, 2 partial, 1 fail. Six findings, F1 to F6. |
| 002 | [`UAT-report-002.md`](UAT-report-002.md) | same, at `f3ea7b1` | Full re-run of all 67 scenarios on merged main, to confirm F1 to F6 are closed and nothing regressed. Fresh sandbox. | ACCEPT. 67 pass. All six findings closed. Three new low-severity findings, G1 to G3. |

| web UI | [`UAT-web-ui.md`](UAT-web-ui.md) | `zorp-web 0.3.2`, debug, at `feat/artifact-pane` | Addition, not a re-run. 24 scenarios over the settings panel and the artifact pane, against a live local Ollama and a real browser, including traversal and injection attempts and four mutation checks. | ACCEPT. 24 pass. Four defects found and fixed during the run, F1 to F4. |

| telemetry | [`UAT-telemetry.md`](UAT-telemetry.md) | same, at `7f8ebaa` | Addition to the baseline, not a re-run. 12 scenarios over the `ZORP_TRACE_FILE` JSONL layer and the `otel` OpenTelemetry export, including the example's Jaeger stack. | ACCEPT. 12 pass. Three low findings, I1 to I3, all documentation or noise. |

| cold install | [`UAT-cold-install.md`](UAT-cold-install.md) | the published `v0.3.1` release, not a local build | 8 scenarios on the README's `curl \| bash` path in a bare `ubuntu:22.04` with no toolchain and nothing cached. | ACCEPT with reservations. 8 pass. Four findings, J1 to J4. J1 fixed here; J2 and J3 fixed on main but live for anyone installing today; J4 needs a maintainer. |

| critique | [`UAT-critique.md`](UAT-critique.md) | `zorp-agent 0.3.2`, debug, at `feat/self-critique` | Addition, not a re-run. 10 scenarios over the `critique` subcommand, driven by a canned completions endpoint rather than a live model so the model-call count is checkable. | ACCEPT. 10 pass. No findings. Live-model behaviour is explicitly out of scope. |

Both runs used a local Ollama endpoint with an isolated `HOME`,
`ZORP_STATE_DB`, and `ZORP_TRUST_FILE`, so nothing touched the tester's
real state.

Findings are numbered per run: `F<n>` in run 001, `G<n>` in run 002,
`H<n>` in a partial sweep that is not written up here, and `I<n>` in the
telemetry pass. A later run should keep going down the alphabet rather
than restarting at F.

## Running the next one

The reproduction runbook is at the end of
[`UAT-report.md`](UAT-report.md#how-to-reproduce): the environment
variables, the core command, and the four research commands with the
in-tree stub MCP server. Run 002 used it unchanged. Start there rather
than reinventing the setup.

Two things run 002 asked for in its own closing section, worth carrying
into run 003:

- Run it as somebody, or something, other than the author of the fixes
  being verified. Run 002 says plainly that it was not an independent
  replication.
- Consider the parallel structure quecto's reports used, one tester per
  area, instead of a single sequential pass.

Open findings carried forward: I1, I2, and I3 from the telemetry pass.
Everything from runs 001 and 002, F1 to F6 and G1 to G3, plus H1 from the
partial sweep, is closed as of `7f8ebaa`.
