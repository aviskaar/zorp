# Decision log

The decision log is now a set of architecture decision records in
[`docs/decisions/`](decisions/README.md), one file per decision. See
[issue 277](https://github.com/aviskaar/zorp/issues/277).

Code and docs cite entries by date, such as `docs/DECISIONS.md` (2026-09-05).
This table maps each date to its record so those references still resolve.

| Date | Record |
|------|--------|
| 2026-10-04 | [0138](decisions/0138-decisions-are-one-adr-per-file.md) Decisions are one ADR per file |
| 2026-10-01 | [0137](decisions/0137-one-loopback-guard-in-a-crate-of-its-own.md) one loopback guard, in a crate of its own |
| 2026-10-03 | [0136](decisions/0136-the-bolt-loop-stops-early-on-a-committed-stop-width-and-only.md) the bolt loop stops early on a committed stop width, and only from three attempts up |
| 2026-10-03 | [0135](decisions/0135-a-state-changing-request-another-page-sent-is-refused-before.md) a state-changing request another page sent is refused before any handler |
| 2026-10-03 | [0134](decisions/0134-local-weights-measurements-get-a-sibling-table-and-the-table.md) local-weights measurements get a sibling table, and the table refuses a missing value |
| 2026-09-19 | [0133](decisions/0133-a-pretraining-run-trains-on-a-real-corpus-or-it-says-it-did.md) a pretraining run trains on a real corpus, or it says it did not |
| 2026-09-18 | [0132](decisions/0132-a-search-provider-is-picked-by-one-environment-variable-and.md) a search provider is picked by one environment variable, and a wrong name is an error |
| 2026-09-18 | [0131](decisions/0131-bench-is-the-third-part-of-zorp-eval-and-it-never-gates.md) `bench` is the third part of `zorp-eval`, and it never gates |
| 2026-09-18 | [0130](decisions/0130-a-small-model-is-offered-the-plain-html-skill-and-the-rule-s.md) a small model is offered the plain HTML skill, and the rule says so |
| 2026-09-12 | [0129](decisions/0129-native-mac-app-zorp-desktop-runs-zorp-web-in-process-via-tau.md) native Mac app (`zorp-desktop`) runs `zorp-web` in-process via Tauri v2 |
| 2026-09-12 | [0128](decisions/0128-a-zorp-mode-run-reports-where-it-is-and-a-person-can-answer.md) a Zorp mode run reports where it is, and a person can answer its checkpoints |
| 2026-09-13 | [0127](decisions/0127-what-is-loaded-and-what-is-still-in-the-window-are-different.md) what is loaded and what is still in the window are different questions |
| 2026-09-13 | [0126](decisions/0126-an-approval-prompt-shows-the-whole-command-and-a-report-asks.md) an approval prompt shows the whole command, and a report asks nothing |
| 2026-09-13 | [0125](decisions/0125-a-landing-page-is-a-deliverable-so-the-preview-is-the-constr.md) a landing page is a deliverable, so the preview is the constraint and not the page |
| 2026-09-13 | [0124](decisions/0124-one-list-of-what-zorp-keeps-and-the-browser-gets-doctor-with.md) one list of what zorp keeps, and the browser gets doctor without the wait |
| 2026-09-13 | [0123](decisions/0123-an-agent-is-a-flavor-with-a-description-and-a-project-one-is.md) an agent is a flavor with a description, and a project one is untrusted until somebody clicks |
| 2026-09-11 | [0122](decisions/0122-the-library-feature-gets-its-own-runner-rather-than-more-fre.md) the library feature gets its own runner rather than more free disk |
| 2026-09-10 | [0121](decisions/0121-the-chat-repl-grows-its-own-line-editor-rather-than-taking-o.md) the chat REPL grows its own line editor rather than taking one |
| 2026-09-10 | [0120](decisions/0120-recall-and-memory-move-into-zorp-agent-and-a-turn-asks-for-m.md) recall and memory move into zorp-agent, and a turn asks for memory one message at a time |
| 2026-09-10 | [0119](decisions/0119-one-settings-file-for-both-surfaces-and-the-chain-that-reads.md) one settings file for both surfaces, and the chain that reads it |
| 2026-09-10 | [0118](decisions/0118-the-artifact-skills-are-written-to-the-pane-s-sandbox-and-sa.md) the artifact skills are written to the pane's sandbox, and say which limits are walls |
| 2026-09-10 | [0117](decisions/0117-three-surfaces-report-what-skills-are-installed-and-none-of.md) three surfaces report what skills are installed, and none of them loads one |
| 2026-09-10 | [0116](decisions/0116-the-cli-stays-line-oriented-and-the-wins-a-full-screen-was-f.md) the CLI stays line oriented, and the wins a full screen was for are line oriented too |
| 2026-09-09 | [0115](decisions/0115-context-compaction-summarizes-with-the-model-and-the-summary.md) context compaction summarizes with the model, and the summary is never evidence |
| 2026-09-09 | [0114](decisions/0114-a-project-is-a-label-on-a-conversation-and-a-scope-for-what.md) A project is a label on a conversation, and a scope for what it remembers |
| 2026-09-05 | [0113](decisions/0113-ensemble-is-a-review-loop-with-a-return-edge-and-every-decis.md) ensemble is a review loop with a return edge, and every decision in it is code |
| 2026-09-05 | [0112](decisions/0112-ci-compiles-the-opt-in-features-and-refuses-a-gate-an-outage.md) CI compiles the opt-in features, and refuses a gate an outage can redden |
| 2026-09-05 | [0111](decisions/0111-the-file-list-is-folders-not-paths.md) the file list is folders, not paths |
| 2026-09-05 | [0110](decisions/0110-a-deterministic-eval-suite-gates-the-harness.md) a deterministic eval suite gates the harness |
| 2026-09-05 | [0109](decisions/0109-a-file-the-answer-names-opens-in-the-pane.md) a file the answer names opens in the pane |
| 2026-09-05 | [0108](decisions/0108-a-connection-the-peer-closed-is-a-dropped-stream-and-gets-th.md) a connection the peer closed is a dropped stream and gets the same treatment |
| 2026-09-05 | [0107](decisions/0107-zorp-web-works-in-a-workspace-somebody-chose-and-in-none-unt.md) zorp-web works in a workspace somebody chose, and in none until they have |
| 2026-09-05 | [0106](decisions/0106-the-browser-asks-for-a-workspace-first-and-again-when-one-is.md) the browser asks for a workspace, first and again when one is missing |
| 2026-09-05 | [0105](decisions/0105-the-tool-line-carries-its-result-as-colour-and-the-browser-h.md) the tool line carries its result as colour, and the browser hears a call start |
| 2026-09-05 | [0104](decisions/0104-the-activity-group-and-the-settled-approval-card-fold-to-one.md) the activity group and the settled approval card fold to one line |
| 2026-09-05 | [0103](decisions/0103-a-chat-branches-at-an-answer-by-copying-the-stored-messages.md) a chat branches at an answer by copying the stored messages up to it into a new session |
| 2026-09-04 | [0102](decisions/0102-a-404-that-names-an-upstream-provider-is-the-upstream-s-erro.md) a 404 that names an upstream provider is the upstream's error and is retried |
| 2026-09-04 | [0101](decisions/0101-the-tool-line-reads-as-the-model-s-own-phrase-for-its-call-c.md) the tool line reads as the model's own phrase for its call, clamped in code |
| 2026-09-04 | [0100](decisions/0100-a-stream-dropped-after-delivery-is-asked-again-by-the-loop-a.md) a stream dropped after delivery is asked again by the loop and never re-sent by the transport |
| 2026-09-04 | [0099](decisions/0099-an-error-the-provider-delivers-inside-a-200-stream-is-named.md) an error the provider delivers inside a 200 stream is named, and retried only while nothing has reached the caller |
| 2026-09-03 | [0098](decisions/0098-a-provider-that-states-its-context-window-has-said-it-and-th.md) a provider that states its context window has said it, and the turn is retried once after compaction |
| 2026-09-03 | [0097](decisions/0097-voice-shows-that-it-is-listening-and-the-live-transcript-is.md) voice shows that it is listening, and the live transcript is segments through the same loopback endpoint |
| 2026-09-03 | [0096](decisions/0096-a-reply-the-provider-cut-off-at-its-output-limit-is-an-error.md) a reply the provider cut off at its output limit is an error, not an answer |
| 2026-09-03 | [0095](decisions/0095-a-quoted-heredoc-body-is-data-and-a-denial-says-which-rule-f.md) a quoted heredoc body is data, and a denial says which rule fired |
| 2026-09-03 | [0094](decisions/0094-compaction-elides-the-model-s-own-tool-call-arguments-becaus.md) compaction elides the model's own tool-call arguments, because that is where the bytes were |
| 2026-09-02 | [0093](decisions/0093-the-admission-gate-is-four-numbers-read-from-the-ledger-and.md) the admission gate is four numbers read from the ledger, and each one was missing from the last reading |
| 2026-09-01 | [0092](decisions/0092-the-admission-gate-is-met-on-the-numbers-and-we-are-not-cros.md) the admission gate is met on the numbers and we are not crossing it |
| 2026-09-01 | [0091](decisions/0091-terminal-bench-runs-through-harbor-and-the-adapter-uploads-a.md) Terminal-Bench runs through Harbor, and the adapter uploads a binary built from the tree |
| 2026-09-01 | [0090](decisions/0090-first-run-is-a-guided-front-door-onto-the-settings-that-exis.md) first run is a guided front door onto the settings that exist, and free means the provider said zero |
| 2026-09-01 | [0089](decisions/0089-the-bolt-runs-several-attempts-and-ends-in-a-write-up-and-de.md) the bolt runs several attempts and ends in a write-up, and `deliver` was not the thing to end in |
| 2026-09-01 | [0088](decisions/0088-the-fence-is-the-model-s-punctuation-not-its-answer.md) the fence is the model's punctuation, not its answer |
| 2026-09-01 | [0087](decisions/0087-the-anomaly-ledger-gets-a-producer-and-it-is-off-by-default.md) the anomaly ledger gets a producer, and it is off by default |
| 2026-09-01 | [0086](decisions/0086-the-model-proposes-a-pre-registration-a-person-still-commits.md) the model proposes a pre-registration, a person still commits it, and an unsure model asks |
| 2026-09-01 | [0085](decisions/0085-the-anomaly-ledger-has-no-producer-and-the-gate-is-not-short.md) the anomaly ledger has no producer, and the gate is not short of data |
| 2026-08-31 | [0084](decisions/0084-auto-approve-loses-the-banner-gains-a-reviewer.md) auto-approve loses the banner, gains a reviewer |
| 2026-08-31 | [0083](decisions/0083-a-real-run-can-never-cross-the-hypothesis-search-admission-g.md) a real run can never cross the hypothesis-search admission gate, so a third condition is recorded |
| 2026-08-30 | [0082](decisions/0082-the-model-listing-carries-the-key-and-a-candidate-key-travel.md) the model listing carries the key, and a candidate key travels in a body |
| 2026-08-28 | [0081](decisions/0081-hypothesis-search-moves-to-gated-and-the-real-ledger-stays-o.md) hypothesis search moves to Gated, and the real ledger stays out of reach |
| 2026-08-24 | [0080](decisions/0080-one-compose-stack-extended-with-an-ollama-sidecar.md) one compose stack, extended, with an Ollama sidecar |
| 2026-08-24 | [0079](decisions/0079-conversation-indexing-is-a-quiet-background-loop-not-a-butto.md) conversation indexing is a quiet background loop, not a button |
| 2026-08-24 | [0078](decisions/0078-voice-setup-is-automatic-resolution-driven-and-still-local.md) voice setup is automatic, resolution-driven, and still local |
| 2026-08-24 | [0077](decisions/0077-recorded-voice-stays-on-loopback-and-qwen3-asr-writes-only-i.md) recorded voice stays on loopback and Qwen3-ASR writes only into the composer |
| 2026-08-24 | [0076](decisions/0076-distribution-happens-at-the-capability-boundary-over-zorp-we.md) distribution happens at the capability boundary, over zorp-web's API, with git as the state bus |
| 2026-08-24 | [0075](decisions/0075-the-calibration-tolerance-is-0-10-set-from-the-first-observe.md) the calibration tolerance is 0.10, set from the first observed curve |
| 2026-08-23 | [0074](decisions/0074-a-pooled-connection-cannot-be-allowed-to-forget-its-read-tim.md) a pooled connection cannot be allowed to forget its read timeout |
| 2026-08-23 | [0073](decisions/0073-the-calibration-sample-is-nested-so-a-bigger-run-extends-the.md) the calibration sample is nested, so a bigger run extends the smaller one |
| 2026-08-23 | [0072](decisions/0072-a-provider-asking-to-be-asked-again-is-asked-again-a-bounded.md) a provider asking to be asked again is asked again, a bounded number of times and out loud |
| 2026-08-23 | [0071](decisions/0071-a-cut-off-stream-is-an-error-and-the-bound-that-failed-quiet.md) a cut off stream is an error, and the bound that failed quietly is why |
| 2026-08-22 | [0070](decisions/0070-one-http-agent-so-the-streaming-path-cannot-be-the-unbounded.md) one HTTP agent, so the streaming path cannot be the unbounded one |
| 2026-08-22 | [0069](decisions/0069-a-session-title-is-a-model-s-sentence-so-it-gets-its-own-col.md) a session title is a model's sentence, so it gets its own column |
| 2026-08-22 | [0068](decisions/0068-the-browser-is-a-workspace-with-draggable-halves-and-the-fil.md) the browser is a workspace with draggable halves, and the file list is a picker |
| 2026-08-22 | [0067](decisions/0067-a-pdf-in-the-artifact-pane-is-a-pdf-and-the-isolation-is-the.md) a PDF in the artifact pane is a PDF, and the isolation is the response header |
| 2026-08-22 | [0066](decisions/0066-the-calibration-harness-counts-every-attempt-it-samples-and.md) the calibration harness counts every attempt it samples, and prints why it dropped each one |
| 2026-08-22 | [0065](decisions/0065-a-band-too-thin-to-judge-is-its-own-no-go-and-never-a-miss.md) a band too thin to judge is its own no-go, and never a miss |
| 2026-08-22 | [0064](decisions/0064-a-calibration-band-is-a-bin-of-adjacent-confidences-sized-by.md) a calibration band is a bin of adjacent confidences, sized by what it can judge |
| 2026-08-22 | [0063](decisions/0063-conversations-feed-a-local-memory-and-memory-is-quoted-never.md) conversations feed a local memory, and memory is quoted, never summarized |
| 2026-08-22 | [0062](decisions/0062-conversations-are-searchable-by-meaning-and-the-vectors-neve.md) conversations are searchable by meaning, and the vectors never leave the machine |
| 2026-08-21 | [0061](decisions/0061-zorp-mode-is-a-browser-driven-investigate-not-a-new-capabili.md) Zorp mode is a browser-driven investigate, not a new capability |
| 2026-08-21 | [0060](decisions/0060-the-browser-is-told-what-search-it-has-and-is-never-left-to.md) the browser is told what search it has, and is never left to guess |
| 2026-08-21 | [0059](decisions/0059-aryabhatta-gets-a-producer-and-forecasting-is-opt-in.md) aryabhatta gets a producer, and forecasting is opt-in |
| 2026-08-21 | [0058](decisions/0058-the-go-no-go-is-computed-the-prose-list-is-one-list-again.md) the go/no-go is computed, the prose list is one list again |
| 2026-08-20 | [0057](decisions/0057-two-anomalies-with-no-recorded-conditions-are-not-alike.md) two anomalies with no recorded conditions are not alike |
| 2026-08-20 | [0056](decisions/0056-nothing-reaches-the-anomaly-ledger-except-through-the-gate.md) nothing reaches the anomaly ledger except through the gate |
| 2026-08-20 | [0055](decisions/0055-the-calibration-report-scores-the-last-forecast-not-every-dr.md) the calibration report scores the last forecast, not every draft |
| 2026-08-20 | [0054](decisions/0054-a-review-panel-is-launched-by-a-person-never-by-a-model.md) a review panel is launched by a person, never by a model |
| 2026-08-20 | [0053](decisions/0053-a-command-may-not-call-the-server-it-is-running-under.md) a command may not call the server it is running under |
| 2026-08-20 | [0052](decisions/0052-the-architecture-index-is-deleted-the-specs-are-the-index.md) the architecture index is deleted, the specs are the index |
| 2026-08-20 | [0051](decisions/0051-the-api-answers-named-origins-and-the-msrv-is-checked.md) the API answers named origins, and the MSRV is checked |
| 2026-08-19 | [0050](decisions/0050-the-discovery-layer-is-called-aryabhatta-and-erbga-is-wired.md) the discovery layer is called aryabhatta, and erbga is wired into it |
| 2026-08-19 | [0049](decisions/0049-the-web-composer-s-send-button-becomes-a-stop-button.md) the web composer's send button becomes a stop button |
| 2026-08-19 | [0048](decisions/0048-the-streaming-read-loop-watches-the-cancel-token.md) the streaming read loop watches the cancel token |
| 2026-08-19 | [0047](decisions/0047-the-browser-can-stand-its-approvals-down-per-session-loudly.md) the browser can stand its approvals down, per session, loudly |
| 2026-08-19 | [0046](decisions/0046-a-turn-is-seeded-from-the-store-and-compaction-never-writes.md) a turn is seeded from the store, and compaction never writes to it |
| 2026-08-19 | [0045](decisions/0045-a-pdf-is-read-for-its-text-not-framed-for-its-layout.md) a PDF is read for its text, not framed for its layout |
| 2026-08-19 | [0044](decisions/0044-a-run-that-wrote-a-file-opens-the-pane-showing-it.md) a run that wrote a file opens the pane showing it |
| 2026-08-18 | [0043](decisions/0043-a-draft-gets-audited-against-the-record-and-the-auditor-is-c.md) a draft gets audited against the record, and the auditor is code |
| 2026-08-18 | [0042](decisions/0042-skills-are-claude-code-s-format-and-they-grant-nothing.md) skills are Claude Code's format, and they grant nothing |
| 2026-08-18 | [0041](decisions/0041-the-artifact-pane-surfaces-what-a-run-wrote-and-reads-office.md) the artifact pane surfaces what a run wrote, and reads office formats |
| 2026-08-18 | [0040](decisions/0040-answers-stream-and-the-streaming-path-filters-reasoning.md) answers stream, and the streaming path filters reasoning |
| 2026-08-18 | [0039](decisions/0039-one-system-prompt-and-it-says-zorp-is-a-research-agent.md) one system prompt, and it says zorp is a research agent |
| 2026-08-18 | [0038](decisions/0038-the-markdown-renderer-is-ours-because-the-alternative-is-inn.md) the markdown renderer is ours, because the alternative is innerHTML |
| 2026-08-17 | [0037](decisions/0037-open-context-connects-as-an-mcp-server-and-searching-your-ow.md) open-context connects as an MCP server, and searching your own material is not evidence |
| 2026-08-17 | [0036](decisions/0036-model-settings-resolved-server-side-ui-over-env-over-default.md) model settings resolved server-side, UI over env over default |
| 2026-08-17 | [0035](decisions/0035-clippy-gates-ci-on-one-runner-over-all-targets.md) clippy gates CI, on one runner, over all targets |
| 2026-08-17 | [0034](decisions/0034-the-web-event-stream-belongs-to-the-session-not-to-the-turn.md) the web event stream belongs to the session, not to the turn |
| 2026-08-17 | [0033](decisions/0033-one-version-for-the-product-crates-and-a-release-refuses-to.md) one version for the product crates, and a release refuses to disagree with it |
| 2026-08-17 | [0032](decisions/0032-version-and-help-are-answered-by-the-binary-not-the-model.md) --version and --help are answered by the binary, not the model |
| 2026-08-17 | [0031](decisions/0031-ci-decides-what-to-run-with-git-not-a-downloaded-action.md) CI decides what to run with git, not a downloaded action |
| 2026-08-16 | [0030](decisions/0030-web-search-is-a-capability-with-a-provider-behind-it-not-a-t.md) web search is a capability with a provider behind it, not a Tavily integration |
| 2026-08-16 | [0029](decisions/0029-everything-zorp-writes-into-a-project-lives-under-zorp.md) everything zorp writes into a project lives under .zorp/ |
| 2026-08-15 | [0028](decisions/0028-evolve-s-search-layer-is-not-approved-its-measurement-discip.md) evolve's search layer is not approved, its measurement discipline is |
| 2026-08-14 | [0027](decisions/0027-a-fifth-capability-evolve-searches-question-framings-and-nev.md) a fifth capability, evolve, searches question framings and never selects on the metric |
| 2026-08-14 | [0026](decisions/0026-stdio-mcp-reads-get-a-deadline-and-unadvertised-features-are.md) stdio MCP reads get a deadline, and unadvertised features are not probed |
| 2026-08-14 | [0025](decisions/0025-subcommands-win-over-the-bare-task-positional.md) subcommands win over the bare-task positional |
| 2026-08-14 | [0024](decisions/0024-kill-thresholds-carry-a-direction-and-are-enforced.md) kill thresholds carry a direction, and are enforced |
| 2026-08-14 | [0023](decisions/0023-git-is-the-root-of-trust-for-pre-registration-integrity.md) git is the root of trust for pre-registration integrity |
| 2026-08-14 | [0022](decisions/0022-the-vector-library-is-opt-in-not-part-of-research.md) the vector library is opt-in, not part of research |
| 2026-08-14 | [0021](decisions/0021-measurement-code-fails-loudly-instead-of-guessing.md) measurement code fails loudly instead of guessing |
| 2026-08-14 | [0020](decisions/0020-command-policy-analyzes-substitutions-and-redirect-targets.md) command policy analyzes substitutions and redirect targets |
| 2026-08-14 | [0019](decisions/0019-ci-covers-the-research-stack-and-the-lockfile-is-committed.md) CI covers the research stack, and the lockfile is committed |
| 2026-08-13 | [0018](decisions/0018-paper-rebuilt-as-a-real-arxiv-preprint-with-a-bibliography.md) paper rebuilt as a real arXiv preprint, with a bibliography |
| 2026-08-13 | [0017](decisions/0017-paper-corrected-to-zorp-landing-s-real-branding-and-arxiv-fo.md) paper corrected to zorp-landing's real branding and arXiv formatting |
| 2026-08-13 | [0016](decisions/0016-zorp-s-own-arxiv-style-systems-paper-written-first-draft.md) zorp's own arXiv-style systems paper written, first draft |
| 2026-08-13 | [0015](decisions/0015-readme-contributing-default-to-excluding-zorp-track-and-defa.md) README/CONTRIBUTING default to excluding zorp-track, and default-run fixes the ambiguous zorp-agent binary |
| 2026-08-13 | [0014](decisions/0014-ci-excludes-zorp-track-from-the-default-workspace-test-run.md) CI excludes zorp-track from the default workspace test run |
| 2026-08-09 | [0013](decisions/0013-deliver-s-design-huiban-only-academic-venues-only-checkpoint.md) deliver's design: huiban-only, academic venues only, checkpoint doesn't kill the track |
| 2026-08-09 | [0012](decisions/0012-co-write-s-design-grounded-drafting-no-post-hoc-claim-check.md) co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track |
| 2026-08-09 | [0011](decisions/0011-investigate-s-design-cli-supplied-prereg-one-attempt-per-cal.md) investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill |
| 2026-08-09 | [0010](decisions/0010-validate-s-design-mcp-only-search-two-dimension-rubric-new-e.md) validate's design: MCP-only search, two-dimension rubric, new embedding env var |
| 2026-08-09 | [0009](decisions/0009-research-means-investigation-not-academia-zorp-s-scope-broad.md) research means investigation, not academia, zorp's scope broadens |
| 2026-08-09 | [0008](decisions/0008-eight-decisions-from-an-interview-round-on-the-open-question.md) eight decisions from an interview round on the open questions |
| 2026-08-09 | [0007](decisions/0007-two-data-stores-split-by-job-not-one-general-purpose-one.md) two data stores, split by job, not one general-purpose one |
| 2026-08-09 | [0006](decisions/0006-zorp-s-own-arxiv-paper-is-about-the-harness-not-a-discovery.md) zorp's own arXiv paper is about the harness, not a discovery it made |
| 2026-08-09 | [0005](decisions/0005-zorp-s-product-is-four-standalone-capabilities-human-authore.md) zorp's product is four standalone capabilities, human-authored papers only |
| 2026-08-08 | [0004](decisions/0004-no-em-dashes-or-en-dashes-in-repo-prose.md) No em dashes or en dashes in repo prose |
| 2026-08-08 | [0003](decisions/0003-readme-rewritten-as-a-full-project-front-page.md) README rewritten as a full project front page |
| 2026-08-08 | [0002](decisions/0002-harness-renamed-from-quecto-to-zorp.md) Harness renamed from quecto to zorp |
| 2026-08-08 | [0001](decisions/0001-quecto-vendored-as-the-base-harness-ai-scientist-v2-kept-loc.md) quecto vendored as the base harness, AI-Scientist-v2 kept local-only |
