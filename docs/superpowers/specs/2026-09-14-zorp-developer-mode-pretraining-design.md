# Developer Mode & Local Pretraining Engine for Zorp

Date: 2026-09-14. Status: validated design.

## 1. Overview & Mental Model

Zorp is an evidence-backed investigation harness and local-first desktop application. To enable building, training, evaluating, and using local language models within a single unified application, Zorp introduces **Developer Mode**.

Instead of treating pretraining as an external toolchain (e.g. scripts or separate web GUIs), pretraining is integrated directly into Zorp as a first-class mode:

```text
Zorp
├── Normal Mode (Agent)
│   └── Chat, investigation, research, artifact creation (uses trained models)
│
└── Developer Mode (Model Development)
    ├── Datasets
    ├── Tokenizer
    ├── Architecture
    ├── Pretraining
    └── Model Registry
```

Crucially, this creates a closed-loop dogfooding environment:
$$\text{Data} \longrightarrow \text{Tokenizer} \longrightarrow \text{Architecture} \longrightarrow \text{Pretrain} \longrightarrow \text{Open in Zorp} \longrightarrow \text{Evaluate \& Notice Weaknesses} \longrightarrow \text{Iterate}$$

---

## 2. Architecture & Process Model

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                               Zorp Desktop / Web                            │
│                                                                             │
│   ┌───────────────────────────┐         ┌───────────────────────────────┐   │
│   │    Normal Mode (Agent)    │         │    Developer Mode (Training)  │   │
│   │  Chat, Research, Artifacts│  ◄───►  │  Datasets, Tokenizer, Pretrain│   │
│   │  (uses trained models)    │         │  Architecture, Model Registry │   │
│   └─────────────┬─────────────┘         └───────────────┬───────────────┘   │
│                 │                                       │                   │
│                 ▼                                       ▼                   │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │ zorp-web Axum Server                                                │   │
│   │  - /api/sessions, /api/settings...                                  │   │
│   │  - /api/dev/datasets, /api/dev/tokenizer, /api/dev/train,           │   │
│   │    /api/dev/models, /api/dev/train/stream (SSE)                     │   │
│   └──────────────────────────────────┬──────────────────────────────────┘   │
└──────────────────────────────────────┼──────────────────────────────────────┘
                                       │
                                       ▼ in-process Rust API
┌─────────────────────────────────────────────────────────────────────────────┐
│ zorp-train (Dedicated Workspace Crate)                                      │
│                                                                             │
│  ┌───────────────────────┐  ┌──────────────────────┐  ┌──────────────────┐  │
│  │ Environment Manager   │  │ Process Supervisor   │  │ Model Registry   │  │
│  │ ~/.zorp/training-env  │  │ Spawns & monitors MLX│  │ Checkpoints &    │  │
│  │ (uv/venv, mlx, etc.)  │  │ Pause, Resume, Stop  │  │ Local Server     │  │
│  └──────────┬────────────┘  └──────────┬───────────┘  └────────┬─────────┘  │
└─────────────┼──────────────────────────┼───────────────────────┼────────────┘
              │                          │ stdout / stdin JSONL  │ loopback HTTP
              ▼                          ▼                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Local Python Subprocesses (Apple Metal Unified Memory)                      │
│                                                                             │
│  ┌────────────────────────────────────────┐  ┌───────────────────────────┐  │
│  │ mlx_train.py                           │  │ mlx_serve.py (On-demand)  │  │
│  │ - BPE training via `tokenizers`        │  │ - Lightweight loopback API│  │
│  │ - Qwen-style transformer in MLX        │  │ - OpenAI-compatible `/v1` │  │
│  │ - Emits structured JSON-line events    │  │ - Serves checkpoint base  │  │
│  └────────────────────────────────────────┘  └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Separation of Responsibilities
* **Rust (`zorp-train` & `zorp-web`) owns**:
  * Desktop UI integration, routing, and SSE event streaming.
  * Python environment provisioning and integrity verification.
  * Subprocess lifecycle (spawning, pausing, resuming, graceful cancellation).
  * Run configuration, logs, checkpoint storage index, and artifact metadata.
* **Python + MLX (`mlx_train.py` & `mlx_serve.py`) owns**:
  * BPE tokenization via Hugging Face `tokenizers`.
  * Model definition, forward loss computation, backpropagation, and AdamW optimizer execution on Apple Metal.
  * Apple Silicon unified memory querying (`mx.metal.get_active_memory()`).
  * Autoregressive evaluation sampling.
  * Checkpoint serialization (`safetensors`).
  * On-demand loopback OpenAI-compatible inference server.

### 2.2 Environment Management (`~/.zorp/training-env`)
Following the pattern established in [`zorp-voice`](file:///Users/adityakarnam/Projects/aviskaar/zorp/zorp-voice):
* Managed in `zorp_train::environment`.
* Uses `uv` if available, falling back to standard `python3 -m venv`.
* Pinned dependencies: `mlx>=0.22.0`, `tokenizers>=0.21.0`, `safetensors>=0.4.0`, `numpy`.
* Automatic setup and health check with status reported via `GET /api/dev/status`.

### 2.3 File System Layout
All pretraining assets reside in the user's local Zorp state directory:
```text
~/.zorp/
├── training-env/                     # Isolated Python virtual environment
└── training/
    ├── datasets/                     # Dataset manifests & local caches
    │   └── <dataset-id>.json
    ├── tokenizers/                   # Trained BPE tokenizers
    │   └── <tokenizer-id>/
    │       ├── tokenizer.json
    │       └── tokenizer_config.json
    ├── recipes/                      # Model architecture specifications
    │   └── zorp-dense-250m.yaml
    ├── runs/                         # Training experiments
    │   └── <run-id>/
    │       ├── config.yaml
    │       ├── train.log
    │       ├── metrics.jsonl
    │       └── checkpoints/
    │           └── step_<N>/
    │               ├── model.safetensors
    │               ├── config.json
    │               ├── tokenizer.json
    │               └── optimizer.npz
    └── models/                       # Registry of models ready for inference
        └── <model-id>/ -> symlink or manifest pointing to checkpoint
```

---

## 3. Data & Tokenizer Subsystem

### 3.1 Datasets
* **Format**: Standard newline-delimited JSON (`.jsonl`) with a `"text"` field (with auto-detection fallback to `"content"` or `"body"`).
* **Sources**:
  * Local file path or directory of `.jsonl` files (e.g. FineWeb-Edu, local markdown archives, source code repositories).
  * Hugging Face dataset references (e.g. `HuggingFaceFW/fineweb-edu-10bt` shard).
* **Dataset Manifest**:
  Stored at `~/.zorp/training/datasets/<id>.json`:
  ```json
  {
    "id": "fineweb-edu-sample",
    "name": "FineWeb-Edu Sample (100MB)",
    "source_type": "local",
    "path": "/path/to/data.jsonl",
    "total_documents": 25400,
    "approx_tokens": 38200000,
    "sample_documents": [
      "The purpose of a compiler is...",
      "Machine learning systems require..."
    ]
  }
  ```

### 3.2 Tokenizer Engine
* **Algorithm**: Byte-level Byte Pair Encoding (BPE) using Hugging Face `tokenizers` (`ByteLevelBPETrainer`).
* **Byte Fallback**: Full byte-level encoding ensures no out-of-vocabulary (`<unk>`) tokens ever occur.
* **Configurable Vocabulary**: Default `32,768` (adjustable from 8,192 to 65,536).
* **Special Tokens**:
  * `<|endoftext|>` (Sequence end / separator)
  * `<|im_start|>` (Chat / role marker)
  * `<|im_end|>` (Chat / role end)
  * `<|pad|>` (Padding token)
* **Interactive Token Inspector**:
  * `POST /api/dev/tokenizer/tokenize`: Accepts arbitrary input text and returns parsed token IDs, character spans, token pieces, and compression metrics (characters per token).

---

## 4. Model Architecture & Pretraining Engine

### 4.1 Architecture Recipe
Initial default preset: **Qwen-inspired Dense Transformer (~250M parameters)**.

```yaml
name: zorp-dense-250m
family: qwen-inspired
vocab_size: 32768
max_position_embeddings: 2048
hidden_size: 896
intermediate_size: 2432       # SwiGLU intermediate dimension (~2.7x hidden)
num_hidden_layers: 24
num_attention_heads: 14
num_key_value_heads: 2        # Grouped-Query Attention (GQA, 7:1 ratio)
rms_norm_eps: 1e-6
rope_theta: 1000000.0         # Extended RoPE base frequency
qk_norm: true                 # QK-RMSNorm for numerical stability
tie_word_embeddings: true     # Share input embedding and LM head
```

### 4.2 Parameter Calculator
Given configuration parameters, the exact parameter counts are computed analytically:
* $\text{Embeddings} = V \times d$ (or $2 \times V \times d$ if untied)
* $\text{Self-Attention} = L \times [ d \times (h_q + 2 h_{kv}) \times d_{head} + d \times d ]$
* $\text{MLP (SwiGLU)} = L \times [ 3 \times d \times d_{ffn} ]$
* $\text{RMSNorms} = L \times [2 \times d + (\text{if qk\_norm: } 2 \times d)] + d$
* Total parameters displayed dynamically in UI before training starts.

### 4.3 Training Loop (`mlx_train.py`)
* **Framework**: MLX on Apple Silicon Metal with unified memory.
* **Kernel Compilation**: Loss and gradient computations are compiled using `@mx.compile` for fused GPU execution.
* **Precision**: Native `bfloat16` for weights and activations.
* **Optimization**:
  * AdamW optimizer ($\beta_1 = 0.9, \beta_2 = 0.95, \text{weight decay} = 0.1$).
  * Cosine learning rate decay with linear warmup (default 1,000 steps).
  * Gradient accumulation steps (e.g. batch size 8, accumulation 8 $\implies$ 64 sequences per step).
  * Gradient clipping with maximum norm 1.0.
* **Live In-Flight Evaluation**:
  * Every $K$ steps (default 100), training briefly pauses to run autoregressive sampling on standard prompt seeds (`"The purpose of a compiler is"`, `"Machine learning systems"`).
  * Emits `sample` JSON events over stdout.

### 4.4 IPC Protocol (Stdout JSON-Lines)
The training child process emits structured JSON events to `stdout`:
```jsonl
{"type": "init", "parameters": 248234880, "device": "Apple M4 Max", "memory_total_gb": 64.0}
{"type": "step", "step": 1250, "loss": 3.412, "lr": 0.000295, "tokens": 2560000, "tok_per_sec": 6840, "memory_gb": 28.4, "eta_seconds": 38400}
{"type": "sample", "step": 1250, "prompt": "The purpose of a compiler is", "output": " to translate source code into machine executable code..."}
{"type": "checkpoint", "step": 2000, "loss": 3.104, "path": "/path/to/checkpoint/step_2000"}
```

---

## 5. Developer Mode UI & "Open in Zorp" Model Registry

### 5.1 UI Navigation
* A mode toggle in Zorp's top header / sidebar switches between **Normal Mode** (Agent/Chat) and **Developer Mode**.
* Inside Developer Mode, a sub-navigation bar allows switching between:
  1. **Datasets**: File selection, `.jsonl` inspection, sample previews, token counts.
  2. **Tokenizer**: BPE training form, vocabulary size selection, interactive token visualizer chips.
  3. **Architecture**: Recipe selection, parameter calculator, hyperparameter controls.
  4. **Pretrain**: Run controls, live loss chart (SVG), throughput, Metal memory gauge, live generation feed.
  5. **Model Registry**: Saved checkpoints list with metadata and action buttons.

### 5.2 The "Open in Zorp" Loop
1. Under **Model Registry**, each checkpoint displays an `[ Open in Zorp ]` button.
2. Clicking the button triggers `POST /api/dev/models/:id/serve`:
   * `zorp-train` spawns or connects to `mlx_serve.py` on loopback (e.g. `127.0.0.1:8001`).
   * `mlx_serve.py` loads the checkpoint (`model.safetensors`, `config.json`, `tokenizer.json`) and exposes standard OpenAI-compatible `/v1/chat/completions` and `/v1/completions` endpoints.
   * `zorp-web` registers the model under a local provider group (`Zorp Local Pretrained`).
3. The UI automatically transitions to Normal Mode, starts a new conversation with that model active, and allows immediate prompting.
4. For raw base models, `mlx_serve.py` transparently wraps user messages into direct text continuation prompts with streaming response support.

---

## 6. Verification, Testing & Error Handling

### 6.1 Testing Strategy
* **`zorp-train` unit & integration tests**:
  * Environment discovery and Python executable validation.
  * Tokenizer trainer execution and output format validation.
  * Architecture recipe parsing, parameter count calculation, and config validation.
  * IPC protocol serialization and deserialization tests.
  * Checkpoint index scanning and metadata verification.
* **`zorp-web` API tests**:
  * `/api/dev/status`, `/api/dev/datasets`, `/api/dev/tokenizer`, `/api/dev/train`, `/api/dev/models`.
  * SSE event stream connection and message forwarding tests.
* **MLX tests (Python)**:
  * Forward pass tensor shape tests across varying batch sizes and context lengths.
  * Backward pass and gradient step tests verifying loss reduction on synthetic sequences.
  * Checkpoint save and load equivalence test (weights match after save/load cycle).

### 6.2 Error Handling & Safety
* **Metal Memory Limits**: If Metal memory approaches system RAM limits, the training loop logs a warning and performs explicit cache flushes (`mx.metal.clear_cache()`).
* **Process Termination**: If Zorp is closed, the supervisor sends `SIGTERM` to any running MLX subprocess to guarantee no orphan training processes consume GPU resources in the background.
* **Corrupt Data Rows**: The JSONL data loader skips malformed or empty lines and records a counter rather than crashing the training run.
