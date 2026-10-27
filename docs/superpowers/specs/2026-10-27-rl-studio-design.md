# RL Studio

Developer Mode becomes RL Studio: the place a person pretrains or fine-tunes
a model, uses it in zorp's chat and work, records how it did, and feeds that
back into the next round of training. The point is a model personalized to
the person using it, trained on a signal that is theirs and not the model's.

See ADR 0143 for the decision and the rules; this spec is the shape.

## What already exists

- `zorp-train/` pretrains a Qwen-style model on Apple Metal through MLX, with
  a tokenizer trainer, a parameter calculator and a model registry that serves
  a checkpoint back on loopback (ADR 0133).
- The session store holds conversations. `zorp-recall` indexes them; `memory`
  quotes them into a turn. Neither may read model-authored text as evidence
  (ADR 0063).

## The loop

1. **Chat on a model.** A registry checkpoint, served on loopback, is picked
   in the model picker as any endpoint is.
2. **Give feedback.** Under each answer, a thumbs up or down and a "better
   answer" control. A click records a work-log entry (prompt, answer, tool
   calls with code-derived status) and the feedback on it. The controls show
   only when the build has RL Studio.
3. **Build a dataset.** The Fine-tune tab turns the work log into an SFT set
   (kept and improved answers) and a preference set (edit over original),
   shows the counts and rows, and lets the person read them before training.
4. **Fine-tune.** A LoRA, SFT or DPO round over that dataset through
   `mlx-lm`, on the same event stream as pretraining, reporting what it
   trained on. The resulting adapter or checkpoint is served and picked in
   chat, and the loop repeats.

## The rules (ADR 0063, 0133)

- The feedback signal is the person's (thumbs, edits) or code's (tool status,
  tests), never the model grading itself. No claim table, no model-authored
  label.
- Work logs are conversation history kept on the machine, in `rl_work_log`,
  never read by recall, memory, titling or branching, and listed in
  `state.rs` so they can be cleared.
- No model starts a run, trains, or serves a checkpoint
  (`no_tool_trains_a_model_or_serves_a_checkpoint`).
- A run trains on a real dataset or says it did not; an empty build is
  refused.

## Surfaces

- `/api/dev/*` keeps its prefix. New routes: `GET/POST /api/dev/worklog`,
  `POST /api/dev/worklog/:id/feedback`, `POST /api/dev/dataset/preview`,
  `POST /api/dev/finetune/start`, all behind the `train` feature.
- Browser: RL Studio's Fine-tune and Feedback tabs, and the per-answer
  feedback controls, gated on the `rl_studio` capability.
- `GET /api/capabilities` reports `rl_studio.available`.

## Out of scope here

- The Mac app's RL Studio routes and tests (follow-up; the rename reaches its
  menu and title).
- Reward models and online RL. The signal stays offline and person-labelled.
