---
status: accepted
date: 2026-09-19
---

# a pretraining run trains on a real corpus, or it says it did not

**Decision:** `zorp-train` is developer mode: tokenizer training,
architecture recipes, an MLX pretraining loop on Apple Metal, and a
registry that serves a checkpoint back on loopback. `TrainingJobConfig`
carries `tokenizer_dir` and `dataset_path`, both optional. The supervisor
forwards them exactly as configured and resolves nothing itself. With
either missing, `mlx_train.py` trains on synthetic tokens and reports
`"data": "synthetic"` in its init event alongside the corpus token count.
See issue-less branch `feat/developer-mode-pretraining` and
`docs/superpowers/plans/2026-09-14-zorp-developer-mode-pretraining.md`.

**Why the mode is reported.** The loop trained on `mx.random.randint` for
both inputs and targets, and the tokenizer was imported and never called.
A loss curve over noise descends and looks like learning. Nothing in the
run said which it was, so a reader could not tell a real pretraining run
from a smoke test, and neither could a checkpoint written at the end of
one. Reporting it is cheaper than every consumer having to infer it.

**Why the paths are never guessed.** An earlier draft walked a list of
candidates relative to the process's working directory. That makes the
corpus a run trained on depend on where the server was started, which is
not a property a recorded run can carry. The caller resolves the paths,
the same way `ModelRegistry` is handed its `models_dir` rather than
finding one.

**Why an out of range id is dropped and not clamped.** A tokenizer trained
at one vocabulary against a recipe declaring another is a
misconfiguration. Clamping would train the model on a token the text never
contained, and the count of dropped ids is what says the two disagree.

**Why a sample is generated or absent.** The sample event carried a fixed
sentence no model wrote. On the page that is indistinguishable from a
model that had learned to write it, which is the same failure as a
fabricated measurement. It now decodes what the model produced, and with
no tokenizer there is no sample event at all.

**Why a checkpoint id is looked up and never joined.**
`POST /api/dev/models/:id/serve` starts a Python process pointed at the
directory the id names. The first cut built that directory by joining the
id onto `models_dir`, and took the id as the path outright when it was
absolute, so `../` walked out of the models directory and an absolute id
skipped it entirely. `ModelRegistry::resolve_checkpoint` matches the id
against what `list_checkpoints` actually found and answers with that
directory or with nothing, so a name a request invented can only ever name
a checkpoint the listing already offered. Same rule as every other name
that arrives in a request here.

**What it rules out:** any model-initiated training, tokenizer build or
checkpoint serve, since none of it is a tool; a remote training or
inference endpoint, because the whole point is local weights on this
machine; and reading the corpus from anywhere the caller did not name.
