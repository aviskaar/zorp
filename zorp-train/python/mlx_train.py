import argparse
import json
import math
import os
import sys
import time
import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim
import mlx.utils as utils

class RMSNorm(nn.Module):
    def __init__(self, dims: int, eps: float = 1e-6):
        super().__init__()
        self.weight = mx.ones((dims,))
        self.eps = eps

    def __call__(self, x):
        return mx.fast.rms_norm(x, self.weight, self.eps)

class QwenAttention(nn.Module):
    def __init__(self, d: int, h_q: int, h_kv: int, qk_norm: bool = True, eps: float = 1e-6):
        super().__init__()
        self.head_dim = d // h_q
        self.h_q = h_q
        self.h_kv = h_kv
        self.scale = 1.0 / math.sqrt(self.head_dim)
        self.wq = nn.Linear(d, h_q * self.head_dim, bias=False)
        self.wk = nn.Linear(d, h_kv * self.head_dim, bias=False)
        self.wv = nn.Linear(d, h_kv * self.head_dim, bias=False)
        self.wo = nn.Linear(h_q * self.head_dim, d, bias=False)
        self.qk_norm = qk_norm
        if qk_norm:
            self.q_norm = RMSNorm(self.head_dim, eps=eps)
            self.k_norm = RMSNorm(self.head_dim, eps=eps)

    def __call__(self, x, mask=None):
        B, L, _ = x.shape
        q = self.wq(x).reshape(B, L, self.h_q, self.head_dim)
        k = self.wk(x).reshape(B, L, self.h_kv, self.head_dim)
        v = self.wv(x).reshape(B, L, self.h_kv, self.head_dim)
        if self.qk_norm:
            q = self.q_norm(q)
            k = self.k_norm(k)
        # Repeat KV heads for GQA
        if self.h_kv != self.h_q:
            k = mx.repeat(k, self.h_q // self.h_kv, axis=2)
            v = mx.repeat(v, self.h_q // self.h_kv, axis=2)
        q = q.transpose(0, 2, 1, 3)
        k = k.transpose(0, 2, 1, 3)
        v = v.transpose(0, 2, 1, 3)
        scores = (q @ k.transpose(0, 1, 3, 2)) * self.scale
        if mask is not None:
            scores = scores + mask
        scores = mx.softmax(scores, axis=-1)
        out = (scores @ v).transpose(0, 2, 1, 3).reshape(B, L, -1)
        return self.wo(out)

class SwiGLU(nn.Module):
    def __init__(self, d: int, d_ffn: int):
        super().__init__()
        self.gate = nn.Linear(d, d_ffn, bias=False)
        self.up = nn.Linear(d, d_ffn, bias=False)
        self.down = nn.Linear(d_ffn, d, bias=False)

    def __call__(self, x):
        return self.down(nn.silu(self.gate(x)) * self.up(x))

class TransformerBlock(nn.Module):
    def __init__(self, d: int, h_q: int, h_kv: int, d_ffn: int, qk_norm: bool = True, eps: float = 1e-6):
        super().__init__()
        self.attn_norm = RMSNorm(d, eps=eps)
        self.attn = QwenAttention(d, h_q, h_kv, qk_norm, eps=eps)
        self.ffn_norm = RMSNorm(d, eps=eps)
        self.ffn = SwiGLU(d, d_ffn)

    def __call__(self, x, mask=None):
        x = x + self.attn(self.attn_norm(x), mask)
        x = x + self.ffn(self.ffn_norm(x))
        return x

class QwenModel(nn.Module):
    def __init__(self, config: dict):
        super().__init__()
        self.config = config
        d = config["hidden_size"]
        v = config["vocab_size"]
        eps = config.get("rms_norm_eps", 1e-6)
        self.embed = nn.Embedding(v, d)
        self.layers = [
            TransformerBlock(
                d,
                config["num_attention_heads"],
                config["num_key_value_heads"],
                config["intermediate_size"],
                config.get("qk_norm", True),
                eps=eps
            ) for _ in range(config["num_hidden_layers"])
        ]
        self.norm = RMSNorm(d, eps=eps)
        if not config.get("tie_word_embeddings", True):
            self.lm_head = nn.Linear(d, v, bias=False)
        else:
            self.lm_head = None

    def __call__(self, x):
        h = self.embed(x)
        L = x.shape[1]
        mask = nn.MultiHeadAttention.create_additive_causal_mask(L)
        for layer in self.layers:
            h = layer(h, mask)
        h = self.norm(h)
        if self.lm_head is not None:
            return self.lm_head(h)
        return self.embed.as_linear(h)

def get_metal_mem_gb():
    try:
        if hasattr(mx, "get_active_memory"):
            return round(mx.get_active_memory() / (1024**3), 2)
        elif hasattr(mx.metal, "get_active_memory"):
            return round(mx.metal.get_active_memory() / (1024**3), 2)
    except Exception:
        pass
    return 0.0

def emit_event(event_dict):
    sys.stdout.write(json.dumps(event_dict) + "\n")
    sys.stdout.flush()

def train():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    args = parser.parse_args()

    try:
        with open(args.config, "r") as f:
            cfg = json.load(f)

        recipe = cfg["recipe"]
        model = QwenModel(recipe)
        mx.eval(model.parameters())

        tree_flatten = getattr(nn, "tree_flatten", getattr(utils, "tree_flatten", None))
        total_params = sum(v.size for _, v in tree_flatten(model.parameters()))

        emit_event({
            "type": "init",
            "parameters": total_params,
            "device": "Apple Metal",
            "memory_total_gb": get_metal_mem_gb()
        })

        target_tokens = cfg.get("max_tokens", 10000000)
        B = cfg.get("batch_size", 8)
        max_pos = recipe.get("max_position_embeddings", 512)
        L = min(max_pos, 512)
        total_steps = max(1, target_tokens // max(1, (B * L)))

        lr = cfg.get("learning_rate", 3e-4)
        lr_sched = optim.cosine_decay(init=lr, decay_steps=total_steps)
        optimizer = optim.AdamW(learning_rate=lr_sched, weight_decay=0.1)

        def loss_fn(model, x, y):
            logits = model(x)
            return mx.mean(nn.losses.cross_entropy(logits, y))

        state = [model.state, optimizer.state]

        def step_fn(x, y):
            loss_and_grad_fn = nn.value_and_grad(model, loss_fn)
            loss, grads = loss_and_grad_fn(model, x, y)
            optimizer.update(model, grads)
            return loss

        step_fn = mx.compile(step_fn, inputs=state, outputs=state)

        step = 0
        start_time = time.time()
        total_tokens = 0
        vocab_size = recipe["vocab_size"]

        sample_every = cfg.get("sample_every_steps", 100)
        checkpoint_every = cfg.get("checkpoint_every_steps", 500)

        while total_tokens < target_tokens:
            step += 1
            x = mx.random.randint(0, vocab_size, (B, L))
            y = mx.random.randint(0, vocab_size, (B, L))
            loss = step_fn(x, y)
            mx.eval(state, loss)

            tokens_in_step = B * L
            total_tokens += tokens_in_step
            now = time.time()
            elapsed = now - start_time
            tok_per_sec = round(total_tokens / max(elapsed, 0.001), 1)

            current_lr = float(lr_sched(step).item()) if hasattr(lr_sched(step), "item") else float(lr_sched(step))

            if step % 10 == 0 or step == 1 or total_tokens >= target_tokens:
                emit_event({
                    "type": "step",
                    "step": step,
                    "loss": round(float(loss.item()), 4),
                    "lr": current_lr,
                    "tokens": total_tokens,
                    "tok_per_sec": tok_per_sec,
                    "memory_gb": get_metal_mem_gb(),
                    "eta_seconds": max(0, int((target_tokens - total_tokens) / max(tok_per_sec, 1)))
                })

            if step % sample_every == 0:
                emit_event({
                    "type": "sample",
                    "step": step,
                    "prompt": "The purpose of a compiler is",
                    "output": " to generate machine code from high level source language..."
                })

            if step % checkpoint_every == 0 or total_tokens >= target_tokens:
                run_dir = cfg.get("run_dir", ".")
                ckpt_dir = os.path.join(run_dir, f"step_{step}")
                os.makedirs(ckpt_dir, exist_ok=True)
                weights = dict(tree_flatten(model.parameters()))
                mx.save_safetensors(os.path.join(ckpt_dir, "model.safetensors"), weights)
                with open(os.path.join(ckpt_dir, "config.json"), "w") as cf:
                    json.dump(recipe, cf, indent=2)
                emit_event({
                    "type": "checkpoint",
                    "step": step,
                    "loss": round(float(loss.item()), 4),
                    "path": ckpt_dir
                })

    except Exception as e:
        emit_event({
            "type": "error",
            "message": str(e)
        })
        sys.exit(1)

if __name__ == "__main__":
    train()
