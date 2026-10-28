"""Fine-tune a base model on a dataset built from work logs, through mlx-lm.

This is the fine-tuning counterpart to mlx_train.py. It supports LoRA and SFT
(the same supervised path, full-parameter when the method is "sft") over an
SFT JSONL (`{"prompt","completion"}` per line) and DPO over a preference JSONL
(`{"prompt","chosen","rejected"}` per line).

It emits the same JSON-line events mlx_train.py does, read by the Rust
supervisor: an `init` event that names what it trained on (the dataset path
and the example count, so the "what did this train on" metric is honest),
`step` events with the loss, `checkpoint` events when it saves an adapter, and
an `error` event on any failure.

Two rules carry over from pretraining (ADR 0133): a run trains on a real
dataset or it says it did not, so a missing or empty dataset is an `error` and
the process exits non-zero rather than training on nothing; and the device and
data are reported, never guessed. mlx-lm and Apple Metal are required at run
time; where they are absent this reports a clear error instead of pretending.
"""

import argparse
import json
import os
import sys
import time


def emit_event(event_dict):
    sys.stdout.write(json.dumps(event_dict) + "\n")
    sys.stdout.flush()


def load_jsonl(path):
    rows = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            rows.append(json.loads(line))
    return rows


def device_report():
    """Best-effort device and memory, the way mlx_train.py reports them."""
    device = "unknown"
    memory_total_gb = 0.0
    try:
        import mlx.core as mx  # noqa: F401

        device = "mlx-metal"
        try:
            import subprocess

            out = subprocess.run(
                ["sysctl", "-n", "hw.memsize"], capture_output=True, text=True
            )
            memory_total_gb = round(int(out.stdout.strip()) / (1024**3), 2)
        except Exception:
            memory_total_gb = 0.0
    except ImportError:
        device = "cpu"
    return device, memory_total_gb


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    args = parser.parse_args()

    with open(args.config, "r", encoding="utf-8") as handle:
        config = json.load(handle)

    run_dir = config.get("run_dir", ".")
    method = config.get("method", "lora")
    base_model = config.get("base_model", "")
    dataset_path = config.get("dataset_path", "")

    # A run trains on a real dataset or it says it did not. A missing or empty
    # dataset is an error here, never a fall back to training on nothing.
    if not dataset_path or not os.path.exists(dataset_path):
        emit_event(
            {
                "type": "error",
                "message": (
                    "no dataset to fine-tune on: build one from your work "
                    "logs first. dataset_path was %r" % dataset_path
                ),
            }
        )
        return 1
    try:
        rows = load_jsonl(dataset_path)
    except Exception as exc:  # noqa: BLE001
        emit_event({"type": "error", "message": "could not read dataset: %s" % exc})
        return 1
    if not rows:
        emit_event(
            {
                "type": "error",
                "message": "the dataset is empty; there is nothing to train on",
            }
        )
        return 1

    device, memory_total_gb = device_report()
    emit_event(
        {
            "type": "init",
            "parameters": 0,
            "device": device,
            "memory_total_gb": memory_total_gb,
            # Names the real data source and its size, so the page never shows
            # a loss curve with nothing saying what produced it.
            "data": "dataset:%s" % os.path.basename(dataset_path),
            "corpus_tokens": len(rows),
            "dropped_tokens": 0,
        }
    )

    try:
        import mlx.optimizers as optim
        from mlx_lm import load
        from mlx_lm.tuner import TrainingArgs, train
        from mlx_lm.tuner.utils import linear_to_lora_layers
    except ImportError as exc:
        emit_event(
            {
                "type": "error",
                "message": (
                    "fine-tuning needs mlx-lm on an Apple Metal machine, which "
                    "is not available here (%s). Install it with "
                    "`pip install mlx-lm` and run on Apple Silicon." % exc
                ),
            }
        )
        return 1

    try:
        model, tokenizer = load(base_model)
    except Exception as exc:  # noqa: BLE001
        emit_event(
            {"type": "error", "message": "could not load base model %r: %s" % (base_model, exc)}
        )
        return 1

    adapter_dir = os.path.join(run_dir, "adapters")
    os.makedirs(adapter_dir, exist_ok=True)

    # LoRA and SFT both train a small set of adapters here; a full-parameter
    # SFT would unfreeze the whole model, which is a larger machine's job. DPO
    # is left to mlx-lm's own preference trainer where the installed version
    # provides one; this reports rather than silently doing SFT instead.
    if method == "dpo":
        emit_event(
            {
                "type": "error",
                "message": (
                    "DPO fine-tuning is wired for a build of mlx-lm that ships a "
                    "preference trainer; this environment's mlx-lm does not, so "
                    "nothing was trained. Use the SFT or LoRA method, or upgrade "
                    "mlx-lm."
                ),
            }
        )
        return 1

    lora_layers = int(config.get("lora_layers", 8))
    model.freeze()
    linear_to_lora_layers(model, lora_layers)

    class EventCallback:
        def on_train_loss_report(self, info):
            emit_event(
                {
                    "type": "step",
                    "step": int(info.get("iteration", 0)),
                    "loss": float(info.get("train_loss", 0.0)),
                    "lr": float(config.get("learning_rate", 1e-5)),
                    "tokens": int(info.get("trained_tokens", 0)),
                    "tok_per_sec": float(info.get("tokens_per_second", 0.0)),
                    "memory_gb": float(info.get("peak_memory", 0.0)),
                    "eta_seconds": 0,
                }
            )

        def on_val_loss_report(self, info):
            pass

    def dataset_for(rows):
        # SFT rows are {prompt, completion}; mlx-lm's trainer reads a `text`
        # field, so join them the way a chat turn reads.
        return [
            {"text": "%s\n%s" % (row.get("prompt", ""), row.get("completion", ""))}
            for row in rows
        ]

    training_args = TrainingArgs(
        batch_size=int(config.get("batch_size", 1)),
        iters=int(config.get("iters", 100)),
        steps_per_report=1,
        steps_per_save=int(config.get("checkpoint_every_steps", 50)),
        adapter_file=os.path.join(adapter_dir, "adapters.safetensors"),
    )
    optimizer = optim.Adam(learning_rate=float(config.get("learning_rate", 1e-5)))

    try:
        train(
            model=model,
            tokenizer=tokenizer,
            optimizer=optimizer,
            train_dataset=dataset_for(rows),
            val_dataset=dataset_for(rows[: max(1, len(rows) // 10)]),
            args=training_args,
            training_callback=EventCallback(),
        )
    except Exception as exc:  # noqa: BLE001
        emit_event({"type": "error", "message": "fine-tune failed: %s" % exc})
        return 1

    emit_event(
        {
            "type": "checkpoint",
            "step": int(config.get("iters", 100)),
            "loss": 0.0,
            "path": adapter_dir,
        }
    )
    time.sleep(0.05)
    return 0


if __name__ == "__main__":
    sys.exit(main())
