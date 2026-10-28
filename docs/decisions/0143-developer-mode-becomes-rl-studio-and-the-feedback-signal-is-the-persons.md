---
status: accepted
date: 2026-10-27
---

# Developer Mode becomes RL Studio, and the feedback signal is the person's

**Decision:** Developer Mode is renamed **RL Studio** in the browser, the Mac
app and the docs. The `/api/dev/*` route prefix is kept, so nothing breaks.
RL Studio gains three things beyond pretraining: a work log of answers and the
feedback a person gives them, a dataset built from that log, and a fine-tune
round (LoRA, SFT or DPO through `mlx-lm`) that trains on it and whose
checkpoint can then be picked in chat.

**The feedback signal comes from the person or from code, never from the model
grading itself** (ADR 0063). An assistant message is model-authored text;
training a reward on the model's own opinion of its work is exactly how its
guesses become its evidence, and the memory design is built to stop that. So
the only signals RL Studio records are a person's thumbs up or down, a "better
answer" they typed, and tool calls with their status derived in code from the
stored result. Nothing asks a model which answer was better, and there is no
stored sentence a model composed about the work.

**Work logs are conversation history, kept on the machine.** A new
`rl_work_log` table in the session store holds the prompt, the verbatim
answer, the tool calls with code-derived status, and the person's feedback.
It is **never read as evidence**: not by `recall::index_one` (which reads
`messages` and `task`), not by the memory block, not by titling, not by
branching. An answer stored here is labelled model-authored wherever it
surfaces, the way `sessions.display_title` is kept apart from `task` (ADR
0069). A test (`work_log_records_feedback_and_keeps_it_out_of_messages`)
pins that the recorder writes no `messages` rows. It is listed in
`zorp-agent/src/state.rs` so `zorp-agent data` and the data pane can show and
clear it.

**The dataset is built from labels, in code, and reviewable before training.**
`zorp_agent::rl_dataset::build` turns the log into an SFT set and a preference
set: a thumbs-up or an edit becomes an SFT completion (the edit when there is
one); an edit becomes a preference pair, the edit `chosen` over the original
`rejected`. A thumbs-down with no edit produces nothing, because it says the
answer was wrong, not what the right one was, and inventing one would be the
model-authored signal this forbids. The Fine-tune tab shows the counts and the
rows before anything trains, and a build with no examples is refused rather
than run on nothing (the same rule as pretraining, ADR 0133).

**No model starts a run, trains, or serves a checkpoint.**
`no_tool_trains_a_model_or_serves_a_checkpoint` in `zorp-agent/src/agent.rs`
still passes; nothing added here is a tool. Recording a work-log entry is a
person's feedback click, fine-tuning is the Fine-tune tab's button, and both
are browser actions.

**A run says what it trained on.** The fine-tune supervisor writes the dataset
to disk and hands its path over, never guessing it, and `mlx_finetune.py`
reports the dataset and its example count in its `init` event, the same "what
did this train on" metric pretraining added. A missing or empty dataset is an
`error` event and a non-zero exit, never a silent run on nothing.

**What needs hardware, and what is a follow-up.** Fine-tuning runs `mlx-lm`
on Apple Silicon, which CI and a Linux box cannot exercise, so the Rust
plumbing (the manifest, the supervisor command, the dataset builder, the
routes) is unit-tested at the level the rest of `zorp-train` is, and the
actual training is verified on a Metal machine. The Mac app's RL Studio
routes and tests are a follow-up; the rename reaches its menu and window
title.
