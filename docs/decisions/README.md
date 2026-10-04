# Decision records

Architecture decision records in the [MADR](https://adr.github.io/madr/) layout,
one file per decision, numbered oldest first. Each file carries `status` and
`date` front matter and the original entry text, unchanged.

Records are never rewritten. A later decision that reverses an earlier one adds a
**Superseded by** line to the earlier record and is otherwise left as written.

To add one, copy the last file, take the next number, and add a row below.

| # | Date | Decision | Status |
|---|------|----------|--------|
| 0138 | 2026-10-04 | [Decisions are one ADR per file](0138-decisions-are-one-adr-per-file.md) | accepted |
| 0137 | 2026-10-01 | [one loopback guard, in a crate of its own](0137-one-loopback-guard-in-a-crate-of-its-own.md) | accepted |
| 0136 | 2026-10-03 | [the bolt loop stops early on a committed stop width, and only from three attempts up](0136-the-bolt-loop-stops-early-on-a-committed-stop-width-and-only.md) | accepted |
| 0135 | 2026-10-03 | [a state-changing request another page sent is refused before any handler](0135-a-state-changing-request-another-page-sent-is-refused-before.md) | accepted |
| 0134 | 2026-10-03 | [local-weights measurements get a sibling table, and the table refuses a missing value](0134-local-weights-measurements-get-a-sibling-table-and-the-table.md) | accepted |
| 0133 | 2026-09-19 | [a pretraining run trains on a real corpus, or it says it did not](0133-a-pretraining-run-trains-on-a-real-corpus-or-it-says-it-did.md) | accepted |
| 0132 | 2026-09-18 | [a search provider is picked by one environment variable, and a wrong name is an error](0132-a-search-provider-is-picked-by-one-environment-variable-and.md) | accepted |
| 0131 | 2026-09-18 | [`bench` is the third part of `zorp-eval`, and it never gates](0131-bench-is-the-third-part-of-zorp-eval-and-it-never-gates.md) | accepted |
| 0130 | 2026-09-18 | [a small model is offered the plain HTML skill, and the rule says so](0130-a-small-model-is-offered-the-plain-html-skill-and-the-rule-s.md) | accepted |
| 0129 | 2026-09-12 | [native Mac app (`zorp-desktop`) runs `zorp-web` in-process via Tauri v2](0129-native-mac-app-zorp-desktop-runs-zorp-web-in-process-via-tau.md) | accepted |
| 0128 | 2026-09-12 | [a Zorp mode run reports where it is, and a person can answer its checkpoints](0128-a-zorp-mode-run-reports-where-it-is-and-a-person-can-answer.md) | accepted |
| 0127 | 2026-09-13 | [what is loaded and what is still in the window are different questions](0127-what-is-loaded-and-what-is-still-in-the-window-are-different.md) | accepted |
| 0126 | 2026-09-13 | [an approval prompt shows the whole command, and a report asks nothing](0126-an-approval-prompt-shows-the-whole-command-and-a-report-asks.md) | accepted |
| 0125 | 2026-09-13 | [a landing page is a deliverable, so the preview is the constraint and not the page](0125-a-landing-page-is-a-deliverable-so-the-preview-is-the-constr.md) | accepted |
| 0124 | 2026-09-13 | [one list of what zorp keeps, and the browser gets doctor without the wait](0124-one-list-of-what-zorp-keeps-and-the-browser-gets-doctor-with.md) | accepted |
| 0123 | 2026-09-13 | [an agent is a flavor with a description, and a project one is untrusted until somebody clicks](0123-an-agent-is-a-flavor-with-a-description-and-a-project-one-is.md) | accepted |
| 0122 | 2026-09-11 | [the library feature gets its own runner rather than more free disk](0122-the-library-feature-gets-its-own-runner-rather-than-more-fre.md) | accepted |
| 0121 | 2026-09-10 | [the chat REPL grows its own line editor rather than taking one](0121-the-chat-repl-grows-its-own-line-editor-rather-than-taking-o.md) | accepted |
| 0120 | 2026-09-10 | [recall and memory move into zorp-agent, and a turn asks for memory one message at a time](0120-recall-and-memory-move-into-zorp-agent-and-a-turn-asks-for-m.md) | accepted |
| 0119 | 2026-09-10 | [one settings file for both surfaces, and the chain that reads it](0119-one-settings-file-for-both-surfaces-and-the-chain-that-reads.md) | accepted |
| 0118 | 2026-09-10 | [the artifact skills are written to the pane's sandbox, and say which limits are walls](0118-the-artifact-skills-are-written-to-the-pane-s-sandbox-and-sa.md) | accepted |
| 0117 | 2026-09-10 | [three surfaces report what skills are installed, and none of them loads one](0117-three-surfaces-report-what-skills-are-installed-and-none-of.md) | accepted |
| 0116 | 2026-09-10 | [the CLI stays line oriented, and the wins a full screen was for are line oriented too](0116-the-cli-stays-line-oriented-and-the-wins-a-full-screen-was-f.md) | accepted |
| 0115 | 2026-09-09 | [context compaction summarizes with the model, and the summary is never evidence](0115-context-compaction-summarizes-with-the-model-and-the-summary.md) | accepted |
| 0114 | 2026-09-09 | [A project is a label on a conversation, and a scope for what it remembers](0114-a-project-is-a-label-on-a-conversation-and-a-scope-for-what.md) | accepted |
| 0113 | 2026-09-05 | [ensemble is a review loop with a return edge, and every decision in it is code](0113-ensemble-is-a-review-loop-with-a-return-edge-and-every-decis.md) | accepted |
| 0112 | 2026-09-05 | [CI compiles the opt-in features, and refuses a gate an outage can redden](0112-ci-compiles-the-opt-in-features-and-refuses-a-gate-an-outage.md) | accepted |
| 0111 | 2026-09-05 | [the file list is folders, not paths](0111-the-file-list-is-folders-not-paths.md) | accepted |
| 0110 | 2026-09-05 | [a deterministic eval suite gates the harness](0110-a-deterministic-eval-suite-gates-the-harness.md) | accepted |
| 0109 | 2026-09-05 | [a file the answer names opens in the pane](0109-a-file-the-answer-names-opens-in-the-pane.md) | accepted |
| 0108 | 2026-09-05 | [a connection the peer closed is a dropped stream and gets the same treatment](0108-a-connection-the-peer-closed-is-a-dropped-stream-and-gets-th.md) | accepted |
| 0107 | 2026-09-05 | [zorp-web works in a workspace somebody chose, and in none until they have](0107-zorp-web-works-in-a-workspace-somebody-chose-and-in-none-unt.md) | accepted |
| 0106 | 2026-09-05 | [the browser asks for a workspace, first and again when one is missing](0106-the-browser-asks-for-a-workspace-first-and-again-when-one-is.md) | accepted |
| 0105 | 2026-09-05 | [the tool line carries its result as colour, and the browser hears a call start](0105-the-tool-line-carries-its-result-as-colour-and-the-browser-h.md) | accepted |
| 0104 | 2026-09-05 | [the activity group and the settled approval card fold to one line](0104-the-activity-group-and-the-settled-approval-card-fold-to-one.md) | accepted |
| 0103 | 2026-09-05 | [a chat branches at an answer by copying the stored messages up to it into a new session](0103-a-chat-branches-at-an-answer-by-copying-the-stored-messages.md) | accepted |
| 0102 | 2026-09-04 | [a 404 that names an upstream provider is the upstream's error and is retried](0102-a-404-that-names-an-upstream-provider-is-the-upstream-s-erro.md) | accepted |
| 0101 | 2026-09-04 | [the tool line reads as the model's own phrase for its call, clamped in code](0101-the-tool-line-reads-as-the-model-s-own-phrase-for-its-call-c.md) | accepted |
| 0100 | 2026-09-04 | [a stream dropped after delivery is asked again by the loop and never re-sent by the transport](0100-a-stream-dropped-after-delivery-is-asked-again-by-the-loop-a.md) | accepted |
| 0099 | 2026-09-04 | [an error the provider delivers inside a 200 stream is named, and retried only while nothing has reached the caller](0099-an-error-the-provider-delivers-inside-a-200-stream-is-named.md) | accepted |
| 0098 | 2026-09-03 | [a provider that states its context window has said it, and the turn is retried once after compaction](0098-a-provider-that-states-its-context-window-has-said-it-and-th.md) | accepted |
| 0097 | 2026-09-03 | [voice shows that it is listening, and the live transcript is segments through the same loopback endpoint](0097-voice-shows-that-it-is-listening-and-the-live-transcript-is.md) | accepted |
| 0096 | 2026-09-03 | [a reply the provider cut off at its output limit is an error, not an answer](0096-a-reply-the-provider-cut-off-at-its-output-limit-is-an-error.md) | accepted |
| 0095 | 2026-09-03 | [a quoted heredoc body is data, and a denial says which rule fired](0095-a-quoted-heredoc-body-is-data-and-a-denial-says-which-rule-f.md) | accepted |
| 0094 | 2026-09-03 | [compaction elides the model's own tool-call arguments, because that is where the bytes were](0094-compaction-elides-the-model-s-own-tool-call-arguments-becaus.md) | accepted |
| 0093 | 2026-09-02 | [the admission gate is four numbers read from the ledger, and each one was missing from the last reading](0093-the-admission-gate-is-four-numbers-read-from-the-ledger-and.md) | accepted |
| 0092 | 2026-09-01 | [the admission gate is met on the numbers and we are not crossing it](0092-the-admission-gate-is-met-on-the-numbers-and-we-are-not-cros.md) | accepted |
| 0091 | 2026-09-01 | [Terminal-Bench runs through Harbor, and the adapter uploads a binary built from the tree](0091-terminal-bench-runs-through-harbor-and-the-adapter-uploads-a.md) | accepted |
| 0090 | 2026-09-01 | [first run is a guided front door onto the settings that exist, and free means the provider said zero](0090-first-run-is-a-guided-front-door-onto-the-settings-that-exis.md) | accepted |
| 0089 | 2026-09-01 | [the bolt runs several attempts and ends in a write-up, and `deliver` was not the thing to end in](0089-the-bolt-runs-several-attempts-and-ends-in-a-write-up-and-de.md) | accepted |
| 0088 | 2026-09-01 | [the fence is the model's punctuation, not its answer](0088-the-fence-is-the-model-s-punctuation-not-its-answer.md) | accepted |
| 0087 | 2026-09-01 | [the anomaly ledger gets a producer, and it is off by default](0087-the-anomaly-ledger-gets-a-producer-and-it-is-off-by-default.md) | accepted |
| 0086 | 2026-09-01 | [the model proposes a pre-registration, a person still commits it, and an unsure model asks](0086-the-model-proposes-a-pre-registration-a-person-still-commits.md) | accepted |
| 0085 | 2026-09-01 | [the anomaly ledger has no producer, and the gate is not short of data](0085-the-anomaly-ledger-has-no-producer-and-the-gate-is-not-short.md) | accepted |
| 0084 | 2026-08-31 | [auto-approve loses the banner, gains a reviewer](0084-auto-approve-loses-the-banner-gains-a-reviewer.md) | accepted |
| 0083 | 2026-08-31 | [a real run can never cross the hypothesis-search admission gate, so a third condition is recorded](0083-a-real-run-can-never-cross-the-hypothesis-search-admission-g.md) | accepted |
| 0082 | 2026-08-30 | [the model listing carries the key, and a candidate key travels in a body](0082-the-model-listing-carries-the-key-and-a-candidate-key-travel.md) | accepted |
| 0081 | 2026-08-28 | [hypothesis search moves to Gated, and the real ledger stays out of reach](0081-hypothesis-search-moves-to-gated-and-the-real-ledger-stays-o.md) | superseded |
| 0080 | 2026-08-24 | [one compose stack, extended, with an Ollama sidecar](0080-one-compose-stack-extended-with-an-ollama-sidecar.md) | accepted |
| 0079 | 2026-08-24 | [conversation indexing is a quiet background loop, not a button](0079-conversation-indexing-is-a-quiet-background-loop-not-a-butto.md) | accepted |
| 0078 | 2026-08-24 | [voice setup is automatic, resolution-driven, and still local](0078-voice-setup-is-automatic-resolution-driven-and-still-local.md) | accepted |
| 0077 | 2026-08-24 | [recorded voice stays on loopback and Qwen3-ASR writes only into the composer](0077-recorded-voice-stays-on-loopback-and-qwen3-asr-writes-only-i.md) | accepted |
| 0076 | 2026-08-24 | [distribution happens at the capability boundary, over zorp-web's API, with git as the state bus](0076-distribution-happens-at-the-capability-boundary-over-zorp-we.md) | accepted |
| 0075 | 2026-08-24 | [the calibration tolerance is 0.10, set from the first observed curve](0075-the-calibration-tolerance-is-0-10-set-from-the-first-observe.md) | accepted |
| 0074 | 2026-08-23 | [a pooled connection cannot be allowed to forget its read timeout](0074-a-pooled-connection-cannot-be-allowed-to-forget-its-read-tim.md) | accepted |
| 0073 | 2026-08-23 | [the calibration sample is nested, so a bigger run extends the smaller one](0073-the-calibration-sample-is-nested-so-a-bigger-run-extends-the.md) | accepted |
| 0072 | 2026-08-23 | [a provider asking to be asked again is asked again, a bounded number of times and out loud](0072-a-provider-asking-to-be-asked-again-is-asked-again-a-bounded.md) | accepted |
| 0071 | 2026-08-23 | [a cut off stream is an error, and the bound that failed quietly is why](0071-a-cut-off-stream-is-an-error-and-the-bound-that-failed-quiet.md) | accepted |
| 0070 | 2026-08-22 | [one HTTP agent, so the streaming path cannot be the unbounded one](0070-one-http-agent-so-the-streaming-path-cannot-be-the-unbounded.md) | accepted |
| 0069 | 2026-08-22 | [a session title is a model's sentence, so it gets its own column](0069-a-session-title-is-a-model-s-sentence-so-it-gets-its-own-col.md) | accepted |
| 0068 | 2026-08-22 | [the browser is a workspace with draggable halves, and the file list is a picker](0068-the-browser-is-a-workspace-with-draggable-halves-and-the-fil.md) | accepted |
| 0067 | 2026-08-22 | [a PDF in the artifact pane is a PDF, and the isolation is the response header](0067-a-pdf-in-the-artifact-pane-is-a-pdf-and-the-isolation-is-the.md) | accepted |
| 0066 | 2026-08-22 | [the calibration harness counts every attempt it samples, and prints why it dropped each one](0066-the-calibration-harness-counts-every-attempt-it-samples-and.md) | accepted |
| 0065 | 2026-08-22 | [a band too thin to judge is its own no-go, and never a miss](0065-a-band-too-thin-to-judge-is-its-own-no-go-and-never-a-miss.md) | accepted |
| 0064 | 2026-08-22 | [a calibration band is a bin of adjacent confidences, sized by what it can judge](0064-a-calibration-band-is-a-bin-of-adjacent-confidences-sized-by.md) | accepted |
| 0063 | 2026-08-22 | [conversations feed a local memory, and memory is quoted, never summarized](0063-conversations-feed-a-local-memory-and-memory-is-quoted-never.md) | accepted |
| 0062 | 2026-08-22 | [conversations are searchable by meaning, and the vectors never leave the machine](0062-conversations-are-searchable-by-meaning-and-the-vectors-neve.md) | accepted |
| 0061 | 2026-08-21 | [Zorp mode is a browser-driven investigate, not a new capability](0061-zorp-mode-is-a-browser-driven-investigate-not-a-new-capabili.md) | superseded |
| 0060 | 2026-08-21 | [the browser is told what search it has, and is never left to guess](0060-the-browser-is-told-what-search-it-has-and-is-never-left-to.md) | accepted |
| 0059 | 2026-08-21 | [aryabhatta gets a producer, and forecasting is opt-in](0059-aryabhatta-gets-a-producer-and-forecasting-is-opt-in.md) | accepted |
| 0058 | 2026-08-21 | [the go/no-go is computed, the prose list is one list again](0058-the-go-no-go-is-computed-the-prose-list-is-one-list-again.md) | accepted |
| 0057 | 2026-08-20 | [two anomalies with no recorded conditions are not alike](0057-two-anomalies-with-no-recorded-conditions-are-not-alike.md) | accepted |
| 0056 | 2026-08-20 | [nothing reaches the anomaly ledger except through the gate](0056-nothing-reaches-the-anomaly-ledger-except-through-the-gate.md) | accepted |
| 0055 | 2026-08-20 | [the calibration report scores the last forecast, not every draft](0055-the-calibration-report-scores-the-last-forecast-not-every-dr.md) | accepted |
| 0054 | 2026-08-20 | [a review panel is launched by a person, never by a model](0054-a-review-panel-is-launched-by-a-person-never-by-a-model.md) | accepted |
| 0053 | 2026-08-20 | [a command may not call the server it is running under](0053-a-command-may-not-call-the-server-it-is-running-under.md) | accepted |
| 0052 | 2026-08-20 | [the architecture index is deleted, the specs are the index](0052-the-architecture-index-is-deleted-the-specs-are-the-index.md) | accepted |
| 0051 | 2026-08-20 | [the API answers named origins, and the MSRV is checked](0051-the-api-answers-named-origins-and-the-msrv-is-checked.md) | accepted |
| 0050 | 2026-08-19 | [the discovery layer is called aryabhatta, and erbga is wired into it](0050-the-discovery-layer-is-called-aryabhatta-and-erbga-is-wired.md) | accepted |
| 0049 | 2026-08-19 | [the web composer's send button becomes a stop button](0049-the-web-composer-s-send-button-becomes-a-stop-button.md) | superseded |
| 0048 | 2026-08-19 | [the streaming read loop watches the cancel token](0048-the-streaming-read-loop-watches-the-cancel-token.md) | accepted |
| 0047 | 2026-08-19 | [the browser can stand its approvals down, per session, loudly](0047-the-browser-can-stand-its-approvals-down-per-session-loudly.md) | accepted |
| 0046 | 2026-08-19 | [a turn is seeded from the store, and compaction never writes to it](0046-a-turn-is-seeded-from-the-store-and-compaction-never-writes.md) | accepted |
| 0045 | 2026-08-19 | [a PDF is read for its text, not framed for its layout](0045-a-pdf-is-read-for-its-text-not-framed-for-its-layout.md) | accepted |
| 0044 | 2026-08-19 | [a run that wrote a file opens the pane showing it](0044-a-run-that-wrote-a-file-opens-the-pane-showing-it.md) | accepted |
| 0043 | 2026-08-18 | [a draft gets audited against the record, and the auditor is code](0043-a-draft-gets-audited-against-the-record-and-the-auditor-is-c.md) | accepted |
| 0042 | 2026-08-18 | [skills are Claude Code's format, and they grant nothing](0042-skills-are-claude-code-s-format-and-they-grant-nothing.md) | accepted |
| 0041 | 2026-08-18 | [the artifact pane surfaces what a run wrote, and reads office formats](0041-the-artifact-pane-surfaces-what-a-run-wrote-and-reads-office.md) | superseded |
| 0040 | 2026-08-18 | [answers stream, and the streaming path filters reasoning](0040-answers-stream-and-the-streaming-path-filters-reasoning.md) | accepted |
| 0039 | 2026-08-18 | [one system prompt, and it says zorp is a research agent](0039-one-system-prompt-and-it-says-zorp-is-a-research-agent.md) | accepted |
| 0038 | 2026-08-18 | [the markdown renderer is ours, because the alternative is innerHTML](0038-the-markdown-renderer-is-ours-because-the-alternative-is-inn.md) | superseded |
| 0037 | 2026-08-17 | [open-context connects as an MCP server, and searching your own material is not evidence](0037-open-context-connects-as-an-mcp-server-and-searching-your-ow.md) | accepted |
| 0036 | 2026-08-17 | [model settings resolved server-side, UI over env over default](0036-model-settings-resolved-server-side-ui-over-env-over-default.md) | accepted |
| 0035 | 2026-08-17 | [clippy gates CI, on one runner, over all targets](0035-clippy-gates-ci-on-one-runner-over-all-targets.md) | accepted |
| 0034 | 2026-08-17 | [the web event stream belongs to the session, not to the turn](0034-the-web-event-stream-belongs-to-the-session-not-to-the-turn.md) | accepted |
| 0033 | 2026-08-17 | [one version for the product crates, and a release refuses to disagree with it](0033-one-version-for-the-product-crates-and-a-release-refuses-to.md) | accepted |
| 0032 | 2026-08-17 | [--version and --help are answered by the binary, not the model](0032-version-and-help-are-answered-by-the-binary-not-the-model.md) | accepted |
| 0031 | 2026-08-17 | [CI decides what to run with git, not a downloaded action](0031-ci-decides-what-to-run-with-git-not-a-downloaded-action.md) | accepted |
| 0030 | 2026-08-16 | [web search is a capability with a provider behind it, not a Tavily integration](0030-web-search-is-a-capability-with-a-provider-behind-it-not-a-t.md) | accepted |
| 0029 | 2026-08-16 | [everything zorp writes into a project lives under .zorp/](0029-everything-zorp-writes-into-a-project-lives-under-zorp.md) | accepted |
| 0028 | 2026-08-15 | [evolve's search layer is not approved, its measurement discipline is](0028-evolve-s-search-layer-is-not-approved-its-measurement-discip.md) | accepted |
| 0027 | 2026-08-14 | [a fifth capability, evolve, searches question framings and never selects on the metric](0027-a-fifth-capability-evolve-searches-question-framings-and-nev.md) | superseded |
| 0026 | 2026-08-14 | [stdio MCP reads get a deadline, and unadvertised features are not probed](0026-stdio-mcp-reads-get-a-deadline-and-unadvertised-features-are.md) | accepted |
| 0025 | 2026-08-14 | [subcommands win over the bare-task positional](0025-subcommands-win-over-the-bare-task-positional.md) | accepted |
| 0024 | 2026-08-14 | [kill thresholds carry a direction, and are enforced](0024-kill-thresholds-carry-a-direction-and-are-enforced.md) | accepted |
| 0023 | 2026-08-14 | [git is the root of trust for pre-registration integrity](0023-git-is-the-root-of-trust-for-pre-registration-integrity.md) | accepted |
| 0022 | 2026-08-14 | [the vector library is opt-in, not part of research](0022-the-vector-library-is-opt-in-not-part-of-research.md) | accepted |
| 0021 | 2026-08-14 | [measurement code fails loudly instead of guessing](0021-measurement-code-fails-loudly-instead-of-guessing.md) | accepted |
| 0020 | 2026-08-14 | [command policy analyzes substitutions and redirect targets](0020-command-policy-analyzes-substitutions-and-redirect-targets.md) | accepted |
| 0019 | 2026-08-14 | [CI covers the research stack, and the lockfile is committed](0019-ci-covers-the-research-stack-and-the-lockfile-is-committed.md) | accepted |
| 0018 | 2026-08-13 | [paper rebuilt as a real arXiv preprint, with a bibliography](0018-paper-rebuilt-as-a-real-arxiv-preprint-with-a-bibliography.md) | accepted |
| 0017 | 2026-08-13 | [paper corrected to zorp-landing's real branding and arXiv formatting](0017-paper-corrected-to-zorp-landing-s-real-branding-and-arxiv-fo.md) | accepted |
| 0016 | 2026-08-13 | [zorp's own arXiv-style systems paper written, first draft](0016-zorp-s-own-arxiv-style-systems-paper-written-first-draft.md) | accepted |
| 0015 | 2026-08-13 | [README/CONTRIBUTING default to excluding zorp-track, and default-run fixes the ambiguous zorp-agent binary](0015-readme-contributing-default-to-excluding-zorp-track-and-defa.md) | accepted |
| 0014 | 2026-08-13 | [CI excludes zorp-track from the default workspace test run](0014-ci-excludes-zorp-track-from-the-default-workspace-test-run.md) | accepted |
| 0013 | 2026-08-09 | [deliver's design: huiban-only, academic venues only, checkpoint doesn't kill the track](0013-deliver-s-design-huiban-only-academic-venues-only-checkpoint.md) | accepted |
| 0012 | 2026-08-09 | [co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track](0012-co-write-s-design-grounded-drafting-no-post-hoc-claim-check.md) | accepted |
| 0011 | 2026-08-09 | [investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill](0011-investigate-s-design-cli-supplied-prereg-one-attempt-per-cal.md) | accepted |
| 0010 | 2026-08-09 | [validate's design: MCP-only search, two-dimension rubric, new embedding env var](0010-validate-s-design-mcp-only-search-two-dimension-rubric-new-e.md) | accepted |
| 0009 | 2026-08-09 | [research means investigation, not academia, zorp's scope broadens](0009-research-means-investigation-not-academia-zorp-s-scope-broad.md) | accepted |
| 0008 | 2026-08-09 | [eight decisions from an interview round on the open questions](0008-eight-decisions-from-an-interview-round-on-the-open-question.md) | accepted |
| 0007 | 2026-08-09 | [two data stores, split by job, not one general-purpose one](0007-two-data-stores-split-by-job-not-one-general-purpose-one.md) | accepted |
| 0006 | 2026-08-09 | [zorp's own arXiv paper is about the harness, not a discovery it made](0006-zorp-s-own-arxiv-paper-is-about-the-harness-not-a-discovery.md) | accepted |
| 0005 | 2026-08-09 | [zorp's product is four standalone capabilities, human-authored papers only](0005-zorp-s-product-is-four-standalone-capabilities-human-authore.md) | accepted |
| 0004 | 2026-08-08 | [No em dashes or en dashes in repo prose](0004-no-em-dashes-or-en-dashes-in-repo-prose.md) | accepted |
| 0003 | 2026-08-08 | [README rewritten as a full project front page](0003-readme-rewritten-as-a-full-project-front-page.md) | accepted |
| 0002 | 2026-08-08 | [Harness renamed from quecto to zorp](0002-harness-renamed-from-quecto-to-zorp.md) | accepted |
| 0001 | 2026-08-08 | [quecto vendored as the base harness, AI-Scientist-v2 kept local-only](0001-quecto-vendored-as-the-base-harness-ai-scientist-v2-kept-loc.md) | accepted |
