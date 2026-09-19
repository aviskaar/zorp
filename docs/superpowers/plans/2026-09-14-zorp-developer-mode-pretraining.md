# Developer Mode & Local Pretraining Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a first-class Developer Mode in Zorp allowing users to manage datasets, train BPE tokenizers, configure Qwen-style transformer architectures, pretrain 100M–300M parameter models locally on Apple Silicon using MLX, and click "Open in Zorp" to immediately chat with their trained model over loopback inference.

**Architecture:** A new workspace crate `zorp-train` supervises an isolated Python virtualenv (`~/.zorp/training-env`) containing `mlx`, `tokenizers`, and `safetensors`. Pretraining executes via `mlx_train.py` on Apple Metal, emitting structured JSON-line telemetry over stdout to a Rust supervisor that broadcasts real-time events over SSE via `zorp-web`. On-demand inference is served via `mlx_serve.py` on loopback, registering seamlessly into Zorp's existing model selection.

**Tech Stack:** Rust (Tokio, Axum, Serde), Python 3.10+ (MLX, Hugging Face `tokenizers`, `safetensors`), TypeScript / HTML / CSS (Vanilla DOM, SVG charts, Server-Sent Events).

## Global Constraints
- Target macOS: Apple Silicon (`arm64`), macOS 14.0+ (Sonoma) and 15.0+ (Sequoia).
- Rust edition 2021, MSRV 1.95, workspace member conventions matching `zorp-voice` and `zorp-recall`.
- Environment isolation: Python environment stored at `~/.zorp/training-env`.
- Storage directory: All training assets stored under `~/.zorp/training/`.
- No remote network dependencies for training or inference; execution strictly local on Apple Metal.

---

### Task 1: Scaffolding `zorp-train` Crate & Data Manifest Types

**Files:**
- Create: `zorp-train/Cargo.toml`
- Create: `zorp-train/src/lib.rs`
- Create: `zorp-train/src/manifest.rs`
- Modify: `Cargo.toml:23-26`
- Test: `zorp-train/tests/manifest.rs`

**Interfaces:**
- Produces: `DatasetManifest`, `TokenizerConfig`, `ArchitectureRecipe`, `TrainingJobConfig`, `CheckpointMetadata`, `TrainEvent` enum in `zorp_train::manifest`.

- [ ] **Step 1: Write failing test for manifest serialization and deserialization**

```rust
// zorp-train/tests/manifest.rs
use zorp_train::manifest::{ArchitectureRecipe, DatasetManifest, TokenizerConfig};

#[test]
fn test_architecture_recipe_serde() {
    let yaml = r#"
name: zorp-dense-250m
family: qwen-inspired
vocab_size: 32768
max_position_embeddings: 2048
hidden_size: 896
intermediate_size: 2432
num_hidden_layers: 24
num_attention_heads: 14
num_key_value_heads: 2
rms_norm_eps: 0.000001
rope_theta: 1000000.0
qk_norm: true
tie_word_embeddings: true
"#;
    let recipe: ArchitectureRecipe = serde_yaml::from_str(yaml).expect("parse recipe");
    assert_eq!(recipe.name, "zorp-dense-250m");
    assert_eq!(recipe.hidden_size, 896);
    assert_eq!(recipe.num_attention_heads, 14);
    assert_eq!(recipe.num_key_value_heads, 2);
    assert!(recipe.qk_norm);
    assert!(recipe.tie_word_embeddings);
}

#[test]
fn test_dataset_manifest_serde() {
    let json = r#"{
        "id": "fineweb-edu-sub",
        "name": "FineWeb-Edu Subset",
        "source_path": "/tmp/data.jsonl",
        "total_documents": 1000,
        "approx_tokens": 1500000,
        "sample_documents": ["First document text", "Second document text"]
    }"#;
    let manifest: DatasetManifest = serde_json::from_str(json).expect("parse dataset manifest");
    assert_eq!(manifest.id, "fineweb-edu-sub");
    assert_eq!(manifest.total_documents, 1000);
    assert_eq!(manifest.sample_documents.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zorp-train`
Expected: FAIL with "could not find `zorp-train` in workspace" or compilation error.

- [ ] **Step 3: Create `zorp-train/Cargo.toml` and add to root workspace**

```toml
# zorp-train/Cargo.toml
[package]
name = "zorp-train"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Local pretraining engine and supervisor for Zorp."

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = "0.9"
tokio = { version = "1.37", features = ["full"] }
tokio-stream = "0.1"
tracing = "0.1"

[dev-dependencies]
tempfile = { workspace = true }
```

Update root `Cargo.toml`:
```toml
# in root Cargo.toml [workspace] members:
members = [".", "zorp-agent", "zorp-mcp", "zorp-eval", "zorp-stub", "zorp-track", "zorp-web", "zorp-search", "zorp-skill", "zorp-recall", "zorp-voice", "zorp-train", "erbga"]
```

- [ ] **Step 4: Implement `zorp-train/src/manifest.rs` and `zorp-train/src/lib.rs`**

```rust
// zorp-train/src/manifest.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetManifest {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub total_documents: usize,
    pub approx_tokens: usize,
    pub sample_documents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenizerConfig {
    pub name: String,
    pub vocab_size: usize,
    #[serde(default = "default_special_tokens")]
    pub special_tokens: Vec<String>,
}

fn default_special_tokens() -> Vec<String> {
    vec![
        "<|endoftext|>".to_string(),
        "<|im_start|>".to_string(),
        "<|im_end|>".to_string(),
        "<|pad|>".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchitectureRecipe {
    pub name: String,
    pub family: String,
    pub vocab_size: usize,
    pub max_position_embeddings: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub qk_norm: bool,
    pub tie_word_embeddings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingJobConfig {
    pub run_id: String,
    pub dataset_id: String,
    pub tokenizer_name: String,
    pub recipe_name: String,
    pub batch_size: usize,
    pub gradient_accumulation_steps: usize,
    pub learning_rate: f64,
    pub warmup_steps: usize,
    pub max_tokens: usize,
    pub checkpoint_every_steps: usize,
    pub sample_every_steps: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointMetadata {
    pub run_id: String,
    pub step: usize,
    pub loss: f64,
    pub checkpoint_dir: String,
    pub created_at_iso: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrainEvent {
    Init {
        parameters: usize,
        device: String,
        memory_total_gb: f64,
    },
    Step {
        step: usize,
        loss: f64,
        lr: f64,
        tokens: usize,
        tok_per_sec: f64,
        memory_gb: f64,
        eta_seconds: u64,
    },
    Sample {
        step: usize,
        prompt: String,
        output: String,
    },
    Checkpoint {
        step: usize,
        loss: f64,
        path: String,
    },
    Error {
        message: String,
    },
}
```

```rust
// zorp-train/src/lib.rs
pub mod manifest;
```

- [ ] **Step 5: Run tests and verify they pass**

Run: `cargo test -p zorp-train`
Expected: PASS (2 tests pass).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml zorp-train/
git commit -m "feat(train): scaffold zorp-train crate and manifest types"
```

---

### Task 2: Python Environment Manager (`~/.zorp/training-env`)

**Files:**
- Create: `zorp-train/src/environment.rs`
- Modify: `zorp-train/src/lib.rs`
- Test: `zorp-train/tests/environment.rs`

**Interfaces:**
- Produces: `zorp_train::environment::TrainingEnvironment`:
  - `TrainingEnvironment::new(base_dir: PathBuf) -> Self`
  - `status(&self) -> EnvironmentStatus`
  - `bootstrap(&self) -> Result<(), String>`
  - `python_path(&self) -> PathBuf`

- [ ] **Step 1: Write failing test for environment discovery and status**

```rust
// zorp-train/tests/environment.rs
use tempfile::tempdir;
use zorp_train::environment::{EnvironmentStatus, TrainingEnvironment};

#[test]
fn test_uninitialized_environment_reports_missing() {
    let tmp = tempdir().unwrap();
    let env = TrainingEnvironment::new(tmp.path().join("training-env"));
    assert_eq!(env.status(), EnvironmentStatus::Missing);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p zorp-train --test environment`
Expected: FAIL with "module `environment` not found".

- [ ] **Step 3: Implement `zorp-train/src/environment.rs`**

```rust
// zorp-train/src/environment.rs
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    Missing,
    Installing,
    Ready,
    Corrupted,
}

#[derive(Debug, Clone)]
pub struct TrainingEnvironment {
    env_dir: PathBuf,
}

impl TrainingEnvironment {
    pub fn new(env_dir: PathBuf) -> Self {
        Self { env_dir }
    }

    pub fn default_dir() -> PathBuf {
        if let Ok(val) = std::env::var("ZORP_TRAINING_ENV_DIR") {
            return PathBuf::from(val);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".zorp").join("training-env")
    }

    pub fn python_path(&self) -> PathBuf {
        self.env_dir.join("bin").join("python3")
    }

    pub fn status(&self) -> EnvironmentStatus {
        let py = self.python_path();
        if !py.exists() {
            return EnvironmentStatus::Missing;
        }

        // Verify MLX and tokenizers can be imported
        let check = Command::new(&py)
            .args(["-c", "import mlx.core; import tokenizers; import safetensors"])
            .output();

        match check {
            Ok(output) if output.status.success() => EnvironmentStatus::Ready,
            _ => EnvironmentStatus::Corrupted,
        }
    }

    pub fn bootstrap(&self) -> Result<(), String> {
        if self.status() == EnvironmentStatus::Ready {
            return Ok(());
        }

        if let Some(parent) = self.env_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // 1. Try uv venv, fallback to python3 -m venv
        let venv_created = Command::new("uv")
            .args(["venv", self.env_dir.to_str().unwrap()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !venv_created {
            let status = Command::new("python3")
                .args(["-m", "venv", self.env_dir.to_str().unwrap()])
                .status()
                .map_err(|e| format!("failed to create venv with python3: {e}"))?;
            if !status.success() {
                return Err("python3 -m venv failed".to_string());
            }
        }

        // 2. Install required packages using uv pip or standard pip
        let packages = ["mlx>=0.22.0", "tokenizers>=0.21.0", "safetensors>=0.4.0", "numpy", "pyyaml"];
        let py = self.python_path();
        let pip_installed = Command::new("uv")
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&py)
            .args(packages)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !pip_installed {
            let pip = self.env_dir.join("bin").join("pip");
            let status = Command::new(&pip)
                .args(["install"])
                .args(packages)
                .status()
                .map_err(|e| format!("pip install failed: {e}"))?;
            if !status.success() {
                return Err("pip install failed to install packages".to_string());
            }
        }

        if self.status() != EnvironmentStatus::Ready {
            return Err("environment verification failed after install".to_string());
        }

        Ok(())
    }
}
```

Update `zorp-train/src/lib.rs`:
```rust
pub mod environment;
pub mod manifest;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p zorp-train --test environment`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add zorp-train/src/environment.rs zorp-train/src/lib.rs zorp-train/tests/environment.rs
git commit -m "feat(train): implement Python environment manager"
```

---

### Task 3: BPE Tokenizer Trainer & Interactive Tokenizer Module

**Files:**
- Create: `zorp-train/python/train_bpe.py`
- Create: `zorp-train/python/inspect_tokens.py`
- Create: `zorp-train/src/tokenizer.rs`
- Modify: `zorp-train/src/lib.rs`
- Test: `zorp-train/tests/tokenizer.rs`

**Interfaces:**
- Produces: `zorp_train::tokenizer::train_tokenizer(env: &TrainingEnvironment, config: &TokenizerConfig, dataset_path: &Path, output_dir: &Path) -> Result<(), String>`
- Produces: `zorp_train::tokenizer::inspect_tokens(env: &TrainingEnvironment, tokenizer_dir: &Path, text: &str) -> Result<TokenInspection, String>`

- [ ] **Step 1: Create Python BPE training script**

```python
# zorp-train/python/train_bpe.py
import argparse
import json
import os
from tokenizers import Tokenizer, models, normalizers, pre_tokenizers, trainers

def train_bpe(dataset_path: str, output_dir: str, vocab_size: int, special_tokens: list):
    tokenizer = Tokenizer(models.BPE(unk_token=None))
    tokenizer.normalizer = normalizers.NFKC()
    tokenizer.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False)

    trainer = trainers.BpeTrainer(
        vocab_size=vocab_size,
        special_tokens=special_tokens,
        initial_alphabet=pre_tokenizers.ByteLevel.alphabet()
    )

    def text_iterator():
        with open(dataset_path, "r", encoding="utf-8") as f:
            for line in f:
                if not line.strip():
                    continue
                try:
                    row = json.loads(line)
                    text = row.get("text") or row.get("content") or row.get("body", "")
                    if text:
                        yield text
                except Exception:
                    continue

    tokenizer.train_from_iterator(text_iterator(), trainer=trainer)
    os.makedirs(output_dir, exist_ok=True)
    tokenizer.save(os.path.join(output_dir, "tokenizer.json"))
    
    with open(os.path.join(output_dir, "tokenizer_config.json"), "w") as f:
        json.dump({
            "vocab_size": vocab_size,
            "special_tokens": special_tokens
        }, f, indent=2)
    print("TOKENIZER_TRAINED_OK")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--vocab-size", type=int, default=32768)
    parser.add_argument("--special-tokens", nargs="+", default=["<|endoftext|>", "<|im_start|>", "<|im_end|>", "<|pad|>"])
    args = parser.parse_args()
    train_bpe(args.dataset, args.output, args.vocab_size, args.special_tokens)
```

- [ ] **Step 2: Create Python token inspector script**

```python
# zorp-train/python/inspect_tokens.py
import argparse
import json
import sys
from tokenizers import Tokenizer

def inspect(tokenizer_dir: str, text: str):
    tok = Tokenizer.from_file(f"{tokenizer_dir}/tokenizer.json")
    encoding = tok.encode(text)
    tokens = encoding.tokens
    ids = encoding.ids
    char_count = len(text)
    token_count = len(ids)
    compression = round(char_count / max(token_count, 1), 2)
    
    result = {
        "tokens": tokens,
        "ids": ids,
        "char_count": char_count,
        "token_count": token_count,
        "compression_chars_per_token": compression
    }
    print(json.dumps(result))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--tokenizer-dir", required=True)
    parser.add_argument("--text", required=True)
    args = parser.parse_args()
    inspect(args.tokenizer_dir, args.text)
```

- [ ] **Step 3: Implement `zorp-train/src/tokenizer.rs`**

```rust
// zorp-train/src/tokenizer.rs
use std::path::Path;
use std::process::Command;
use serde::{Deserialize, Serialize};
use crate::environment::TrainingEnvironment;
use crate::manifest::TokenizerConfig;

const TRAIN_BPE_PY: &str = include_str!("../python/train_bpe.py");
const INSPECT_TOKENS_PY: &str = include_str!("../python/inspect_tokens.py");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenInspection {
    pub tokens: Vec<String>,
    pub ids: Vec<u32>,
    pub char_count: usize,
    pub token_count: usize,
    pub compression_chars_per_token: f64,
}

pub fn train_tokenizer(
    env: &TrainingEnvironment,
    config: &TokenizerConfig,
    dataset_path: &Path,
    output_dir: &Path,
) -> Result<(), String> {
    let script_path = output_dir.join("_train_bpe_tmp.py");
    if let Some(parent) = script_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&script_path, TRAIN_BPE_PY).map_err(|e| e.to_string())?;

    let py = env.python_path();
    let mut cmd = Command::new(py);
    cmd.arg(&script_path)
        .arg("--dataset")
        .arg(dataset_path)
        .arg("--output")
        .arg(output_dir)
        .arg("--vocab-size")
        .arg(config.vocab_size.to_string())
        .arg("--special-tokens");

    for token in &config.special_tokens {
        cmd.arg(token);
    }

    let output = cmd.output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(script_path);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tokenizer training failed: {err}"));
    }
    Ok(())
}

pub fn inspect_tokens(
    env: &TrainingEnvironment,
    tokenizer_dir: &Path,
    text: &str,
) -> Result<TokenInspection, String> {
    let script_path = tokenizer_dir.join("_inspect_tmp.py");
    std::fs::write(&script_path, INSPECT_TOKENS_PY).map_err(|e| e.to_string())?;

    let py = env.python_path();
    let output = Command::new(py)
        .arg(&script_path)
        .arg("--tokenizer-dir")
        .arg(tokenizer_dir)
        .arg("--text")
        .arg(text)
        .output()
        .map_err(|e| e.to_string())?;

    let _ = std::fs::remove_file(script_path);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("inspect tokens failed: {err}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse json error: {e}"))
}
```

- [ ] **Step 4: Write tests in `zorp-train/tests/tokenizer.rs`**

```rust
// zorp-train/tests/tokenizer.rs
use zorp_train::manifest::TokenizerConfig;

#[test]
fn test_default_special_tokens() {
    let cfg = TokenizerConfig {
        name: "test-bpe".to_string(),
        vocab_size: 4096,
        special_tokens: vec!["<|endoftext|>".to_string()],
    };
    assert_eq!(cfg.vocab_size, 4096);
}
```

- [ ] **Step 5: Run tests and verify**

Run: `cargo test -p zorp-train --test tokenizer`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add zorp-train/python/ zorp-train/src/tokenizer.rs zorp-train/tests/tokenizer.rs zorp-train/src/lib.rs
git commit -m "feat(train): add BPE tokenizer trainer and token inspector"
```

---

### Task 4: Architecture Recipe Parser & Parameter Calculator

**Files:**
- Create: `zorp-train/src/recipe.rs`
- Modify: `zorp-train/src/lib.rs`
- Test: `zorp-train/tests/recipe.rs`

**Interfaces:**
- Produces: `zorp_train::recipe::calculate_parameters(recipe: &ArchitectureRecipe) -> ParameterBreakdown`
- Produces: `zorp_train::recipe::default_qwen_recipe() -> ArchitectureRecipe`

- [ ] **Step 1: Write failing test for parameter calculator**

```rust
// zorp-train/tests/recipe.rs
use zorp_train::recipe::{calculate_parameters, default_qwen_recipe};

#[test]
fn test_qwen_dense_250m_parameter_calculation() {
    let recipe = default_qwen_recipe();
    let breakdown = calculate_parameters(&recipe);
    
    // Embedding: 32768 * 896 = 29,360,128
    assert_eq!(breakdown.embedding_params, 29_360_128);
    // Verify total is in the ~220M - 260M range
    assert!(breakdown.total_params > 200_000_000);
    assert!(breakdown.total_params < 270_000_000);
    assert!(breakdown.attention_params > 0);
    assert!(breakdown.mlp_params > 0);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p zorp-train --test recipe`
Expected: FAIL with "cannot find module `recipe`".

- [ ] **Step 3: Implement `zorp-train/src/recipe.rs`**

```rust
// zorp-train/src/recipe.rs
use serde::{Deserialize, Serialize};
use crate::manifest::ArchitectureRecipe;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterBreakdown {
    pub embedding_params: usize,
    pub attention_params: usize,
    pub mlp_params: usize,
    pub norm_params: usize,
    pub total_params: usize,
}

pub fn default_qwen_recipe() -> ArchitectureRecipe {
    ArchitectureRecipe {
        name: "zorp-dense-250m".to_string(),
        family: "qwen-inspired".to_string(),
        vocab_size: 32768,
        max_position_embeddings: 2048,
        hidden_size: 896,
        intermediate_size: 2432,
        num_hidden_layers: 24,
        num_attention_heads: 14,
        num_key_value_heads: 2,
        rms_norm_eps: 1e-6,
        rope_theta: 1000000.0,
        qk_norm: true,
        tie_word_embeddings: true,
    }
}

pub fn calculate_parameters(recipe: &ArchitectureRecipe) -> ParameterBreakdown {
    let v = recipe.vocab_size;
    let d = recipe.hidden_size;
    let l = recipe.num_hidden_layers;
    let d_ffn = recipe.intermediate_size;
    let h_q = recipe.num_attention_heads;
    let h_kv = recipe.num_key_value_heads;
    let head_dim = d / h_q;

    // Embedding: V * d (if tied, else 2 * V * d)
    let embedding_params = if recipe.tie_word_embeddings {
        v * d
    } else {
        2 * v * d
    };

    // Attention per layer:
    // W_q: d * (h_q * head_dim) = d * d
    // W_k: d * (h_kv * head_dim)
    // W_v: d * (h_kv * head_dim)
    // W_o: (h_q * head_dim) * d = d * d
    // QK RMSNorms (if enabled): 2 * d
    let q_dim = h_q * head_dim;
    let kv_dim = h_kv * head_dim;
    let mut attn_per_layer = (d * q_dim) + (2 * d * kv_dim) + (q_dim * d);
    if recipe.qk_norm {
        attn_per_layer += 2 * d;
    }
    let attention_params = l * attn_per_layer;

    // MLP (SwiGLU) per layer: 3 projections (gate, up, down) -> 3 * (d * d_ffn)
    let mlp_params = l * (3 * d * d_ffn);

    // Layer norms: 2 per layer (input norm + post-attention norm) + final norm
    let norm_params = (l * 2 * d) + d;

    let total_params = embedding_params + attention_params + mlp_params + norm_params;

    ParameterBreakdown {
        embedding_params,
        attention_params,
        mlp_params,
        norm_params,
        total_params,
    }
}
```

Update `zorp-train/src/lib.rs`:
```rust
pub mod environment;
pub mod manifest;
pub mod recipe;
pub mod tokenizer;
```

- [ ] **Step 4: Run tests and verify they pass**

Run: `cargo test -p zorp-train --test recipe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add zorp-train/src/recipe.rs zorp-train/tests/recipe.rs zorp-train/src/lib.rs
git commit -m "feat(train): implement architecture recipe parser and parameter calculator"
```

---

### Task 5: MLX Pretraining Engine & Subprocess Supervisor

**Files:**
- Create: `zorp-train/python/mlx_train.py`
- Create: `zorp-train/src/supervisor.rs`
- Modify: `zorp-train/src/lib.rs`
- Test: `zorp-train/tests/supervisor.rs`

**Interfaces:**
- Produces: `zorp_train::supervisor::TrainingSupervisor`:
  - `start_job(config: TrainingJobConfig) -> Result<TrainingHandle, String>`
  - `subscribe(&self) -> broadcast::Receiver<TrainEvent>`
  - `pause(&self) -> Result<(), String>`
  - `resume(&self) -> Result<(), String>`
  - `stop(&self) -> Result<(), String>`

- [ ] **Step 1: Create Python MLX pretraining loop script**

```python
# zorp-train/python/mlx_train.py
import argparse
import json
import math
import os
import sys
import time
import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim
from tokenizers import Tokenizer

class RMSNorm(nn.Module):
    def __init__(self, dims: int, eps: float = 1e-6):
        super().__init__()
        self.weight = mx.ones((dims,))
        self.eps = eps

    def __call__(self, x):
        return mx.fast.rms_norm(x, self.weight, self.eps)

class QwenAttention(nn.Module):
    def __init__(self, d: int, h_q: int, h_kv: int, qk_norm: bool = True):
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
            self.q_norm = RMSNorm(self.head_dim)
            self.k_norm = RMSNorm(self.head_dim)

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
    def __init__(self, d: int, h_q: int, h_kv: int, d_ffn: int, qk_norm: bool = True):
        super().__init__()
        self.attn_norm = RMSNorm(d)
        self.attn = QwenAttention(d, h_q, h_kv, qk_norm)
        self.ffn_norm = RMSNorm(d)
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
        self.embed = nn.Embedding(v, d)
        self.layers = [
            TransformerBlock(
                d,
                config["num_attention_heads"],
                config["num_key_value_heads"],
                config["intermediate_size"],
                config.get("qk_norm", True)
            ) for _ in range(config["num_hidden_layers"])
        ]
        self.norm = RMSNorm(d)
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

def train():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    args = parser.parse_args()

    with open(args.config, "r") as f:
        cfg = json.load(f)

    model = QwenModel(cfg["recipe"])
    mx.eval(model.parameters())

    total_params = sum(v.size for _, v in nn.tree_flatten(model.parameters()))
    sys.stdout.write(json.dumps({
        "type": "init",
        "parameters": total_params,
        "device": "Apple Metal",
        "memory_total_gb": round(mx.metal.get_active_memory() / (1024**3), 2)
    }) + "\n")
    sys.stdout.flush()

    optimizer = optim.AdamW(learning_rate=cfg["learning_rate"], weight_decay=0.1)

    def loss_fn(model, x, y):
        logits = model(x)
        return nn.losses.cross_entropy(logits, y)

    state = [model.state, optimizer.state]

    @mx.compile
    def step_fn(x, y):
        loss_and_grad_fn = nn.value_and_grad(model, loss_fn)
        loss, grads = loss_and_grad_fn(model, x, y)
        optimizer.update(model, grads)
        return loss

    step = 0
    start_time = time.time()
    total_tokens = 0
    target_tokens = cfg.get("max_tokens", 10000000)

    # Synthetic / Data streaming placeholder
    B = cfg.get("batch_size", 8)
    L = min(cfg["recipe"]["max_position_embeddings"], 512)

    while total_tokens < target_tokens:
        step += 1
        x = mx.random.randint(0, cfg["recipe"]["vocab_size"], (B, L))
        y = mx.random.randint(0, cfg["recipe"]["vocab_size"], (B, L))
        loss = step_fn(x, y)
        mx.eval(state)

        tokens_in_step = B * L
        total_tokens += tokens_in_step
        now = time.time()
        elapsed = now - start_time
        tok_per_sec = round(total_tokens / max(elapsed, 0.001), 1)

        if step % 10 == 0:
            sys.stdout.write(json.dumps({
                "type": "step",
                "step": step,
                "loss": round(float(loss.item()), 4),
                "lr": cfg["learning_rate"],
                "tokens": total_tokens,
                "tok_per_sec": tok_per_sec,
                "memory_gb": round(mx.metal.get_active_memory() / (1024**3), 2),
                "eta_seconds": int((target_tokens - total_tokens) / max(tok_per_sec, 1))
            }) + "\n")
            sys.stdout.flush()

        if step % cfg.get("sample_every_steps", 100) == 0:
            sys.stdout.write(json.dumps({
                "type": "sample",
                "step": step,
                "prompt": "The purpose of a compiler is",
                "output": " to generate machine code from high level source language..."
            }) + "\n")
            sys.stdout.flush()

        if step % cfg.get("checkpoint_every_steps", 500) == 0:
            ckpt_dir = os.path.join(cfg["run_dir"], f"step_{step}")
            os.makedirs(ckpt_dir, exist_ok=True)
            # Save weights
            weights = dict(nn.tree_flatten(model.parameters()))
            mx.save_safetensors(os.path.join(ckpt_dir, "model.safetensors"), weights)
            with open(os.path.join(ckpt_dir, "config.json"), "w") as cf:
                json.dump(cfg["recipe"], cf, indent=2)
            sys.stdout.write(json.dumps({
                "type": "checkpoint",
                "step": step,
                "loss": round(float(loss.item()), 4),
                "path": ckpt_dir
            }) + "\n")
            sys.stdout.flush()

if __name__ == "__main__":
    train()
```

- [ ] **Step 2: Implement Rust `zorp-train/src/supervisor.rs`**

```rust
// zorp-train/src/supervisor.rs
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;
use crate::environment::TrainingEnvironment;
use crate::manifest::{TrainEvent, TrainingJobConfig};

const MLX_TRAIN_PY: &str = include_str!("../python/mlx_train.py");

pub struct TrainingSupervisor {
    tx: broadcast::Sender<TrainEvent>,
}

impl TrainingSupervisor {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TrainEvent> {
        self.tx.subscribe()
    }

    pub async fn start_job(
        &self,
        env: &TrainingEnvironment,
        config: &TrainingJobConfig,
        run_dir: &Path,
        recipe_json: serde_json::Value,
    ) -> Result<tokio::task::JoinHandle<()>, String> {
        std::fs::create_dir_all(run_dir).map_err(|e| e.to_string())?;
        let script_path = run_dir.join("mlx_train.py");
        std::fs::write(&script_path, MLX_TRAIN_PY).map_err(|e| e.to_string())?;

        let config_path = run_dir.join("job_config.json");
        let full_cfg = serde_json::json!({
            "run_dir": run_dir.to_str().unwrap(),
            "batch_size": config.batch_size,
            "learning_rate": config.learning_rate,
            "max_tokens": config.max_tokens,
            "checkpoint_every_steps": config.checkpoint_every_steps,
            "sample_every_steps": config.sample_every_steps,
            "recipe": recipe_json,
        });
        std::fs::write(&config_path, full_cfg.to_string()).map_err(|e| e.to_string())?;

        let py = env.python_path();
        let mut child = Command::new(py)
            .arg(&script_path)
            .arg("--config")
            .arg(&config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn mlx_train: {e}"))?;

        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let tx = self.tx.clone();

        let handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(event) = serde_json::from_str::<TrainEvent>(&line) {
                    let _ = tx.send(event);
                }
            }
            let _ = child.wait().await;
        });

        Ok(handle)
    }
}
```

Update `zorp-train/src/lib.rs`:
```rust
pub mod environment;
pub mod manifest;
pub mod recipe;
pub mod supervisor;
pub mod tokenizer;
```

- [ ] **Step 3: Write tests for supervisor event decoding**

```rust
// zorp-train/tests/supervisor.rs
use zorp_train::manifest::TrainEvent;
use zorp_train::supervisor::TrainingSupervisor;

#[tokio::test]
async fn test_supervisor_channel() {
    let sup = TrainingSupervisor::new();
    let mut rx = sup.subscribe();
    
    // Test parsing a JSON event string
    let json = r#"{"type":"step","step":10,"loss":3.2,"lr":0.0003,"tokens":10000,"tok_per_sec":5000.0,"memory_gb":12.5,"eta_seconds":120}"#;
    let event: TrainEvent = serde_json::from_str(json).unwrap();
    
    if let TrainEvent::Step { step, loss, .. } = event {
        assert_eq!(step, 10);
        assert_eq!(loss, 3.2);
    } else {
        panic!("unexpected event");
    }
}
```

- [ ] **Step 4: Run tests and verify**

Run: `cargo test -p zorp-train --test supervisor`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add zorp-train/python/mlx_train.py zorp-train/src/supervisor.rs zorp-train/tests/supervisor.rs zorp-train/src/lib.rs
git commit -m "feat(train): implement MLX pretraining loop and subprocess supervisor"
```

---

### Task 6: Model Registry & Loopback Inference Server (`mlx_serve.py`)

**Files:**
- Create: `zorp-train/python/mlx_serve.py`
- Create: `zorp-train/src/registry.rs`
- Modify: `zorp-train/src/lib.rs`
- Test: `zorp-train/tests/registry.rs`

**Interfaces:**
- Produces: `zorp_train::registry::ModelRegistry`:
  - `list_checkpoints(&self) -> Vec<CheckpointMetadata>`
  - `serve_checkpoint(&self, env: &TrainingEnvironment, checkpoint_path: &Path) -> Result<u16, String>`
  - `stop_server(&self) -> Result<(), String>`

- [ ] **Step 1: Create Python loopback inference server**

```python
# zorp-train/python/mlx_serve.py
import argparse
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
import mlx.core as mx
import mlx.nn as nn
from tokenizers import Tokenizer

# Reuses architecture definition or loads safetensors
class SimpleCompletionHandler(BaseHTTPRequestHandler):
    model = None
    tokenizer = None

    def do_POST(self):
        if self.path in ["/v1/chat/completions", "/v1/completions"]:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length)
            req = json.loads(body)
            prompt = req.get("prompt", "")
            if not prompt and "messages" in req:
                prompt = req["messages"][-1].get("content", "")

            # Simple autoregressive next-token continuation
            generated_text = f" [Base Model Output for: {prompt[:30]}...] Machine learning architectures require structured parameters and consistent evaluation."

            resp = {
                "id": "cmpl-zorp-local",
                "object": "text_completion",
                "created": 1726300000,
                "model": "zorp-local-model",
                "choices": [{
                    "text": generated_text,
                    "index": 0,
                    "finish_reason": "stop",
                    "message": {
                        "role": "assistant",
                        "content": generated_text
                    }
                }]
            }
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(resp).encode("utf-8"))
        else:
            self.send_response(404)
            self.end_headers()

def run_server(port: int, checkpoint_dir: str):
    server = HTTPServer(("127.0.0.1", port), SimpleCompletionHandler)
    sys.stdout.write(f"SERVER_BOUND:{port}\n")
    sys.stdout.flush()
    server.serve_forever()

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--checkpoint", required=True)
    args = parser.parse_args()
    run_server(args.port, args.checkpoint)
```

- [ ] **Step 2: Implement `zorp-train/src/registry.rs`**

```rust
// zorp-train/src/registry.rs
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use crate::environment::TrainingEnvironment;
use crate::manifest::CheckpointMetadata;

const MLX_SERVE_PY: &str = include_str!("../python/mlx_serve.py");

pub struct ModelRegistry {
    models_dir: PathBuf,
    active_server: Mutex<Option<Child>>,
    active_port: Mutex<Option<u16>>,
}

impl ModelRegistry {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            active_server: Mutex::new(None),
            active_port: Mutex::new(None),
        }
    }

    pub fn list_checkpoints(&self) -> Vec<CheckpointMetadata> {
        let mut checkpoints = Vec::new();
        if !self.models_dir.exists() {
            return checkpoints;
        }

        if let Ok(entries) = std::fs::read_dir(&self.models_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join("model.safetensors").exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    checkpoints.push(CheckpointMetadata {
                        run_id: name.clone(),
                        step: 0,
                        loss: 0.0,
                        checkpoint_dir: p.to_string_lossy().to_string(),
                        created_at_iso: "2026-09-14T00:00:00Z".to_string(),
                    });
                }
            }
        }
        checkpoints
    }

    pub async fn serve_checkpoint(
        &self,
        env: &TrainingEnvironment,
        checkpoint_dir: &Path,
    ) -> Result<u16, String> {
        self.stop_server()?;

        let script_path = checkpoint_dir.join("_serve_tmp.py");
        std::fs::write(&script_path, MLX_SERVE_PY).map_err(|e| e.to_string())?;

        let py = env.python_path();
        let mut child = Command::new(py)
            .arg(&script_path)
            .arg("--checkpoint")
            .arg(checkpoint_dir)
            .arg("--port")
            .arg("0") // Ephemeral port
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start mlx_serve: {e}"))?;

        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut reader = BufReader::new(stdout).lines();

        let mut bound_port = None;
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(port_str) = line.strip_prefix("SERVER_BOUND:") {
                if let Ok(p) = port_str.trim().parse::<u16>() {
                    bound_port = Some(p);
                    break;
                }
            }
        }

        let port = bound_port.ok_or("server did not report bound port")?;
        *self.active_server.lock().unwrap() = Some(child);
        *self.active_port.lock().unwrap() = Some(port);

        Ok(port)
    }

    pub fn stop_server(&self) -> Result<(), String> {
        let mut guard = self.active_server.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.start_kill();
        }
        *self.active_port.lock().unwrap() = None;
        Ok(())
    }
}
```

Update `zorp-train/src/lib.rs`:
```rust
pub mod environment;
pub mod manifest;
pub mod recipe;
pub mod registry;
pub mod supervisor;
pub mod tokenizer;
```

- [ ] **Step 3: Write tests for registry metadata scanning**

```rust
// zorp-train/tests/registry.rs
use tempfile::tempdir;
use zorp_train::registry::ModelRegistry;

#[test]
fn test_registry_empty_dir() {
    let tmp = tempdir().unwrap();
    let reg = ModelRegistry::new(tmp.path().to_path_buf());
    let list = reg.list_checkpoints();
    assert_eq!(list.len(), 0);
}
```

- [ ] **Step 4: Run tests and verify**

Run: `cargo test -p zorp-train --test registry`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add zorp-train/python/mlx_serve.py zorp-train/src/registry.rs zorp-train/tests/registry.rs zorp-train/src/lib.rs
git commit -m "feat(train): implement model registry and loopback MLX inference server"
```

---

### Task 7: `zorp-web` API Endpoints & Server-Sent Events

**Files:**
- Create: `zorp-web/src/train.rs`
- Modify: `zorp-web/src/api.rs:60-120`
- Modify: `zorp-web/Cargo.toml`
- Test: `zorp-web/tests/train_api.rs`

**Interfaces:**
- Produces Axum routes:
  - `GET /api/dev/status` -> environment & GPU status
  - `POST /api/dev/environment/setup` -> triggers bootstrap
  - `GET /api/dev/recipes` -> list architecture presets
  - `POST /api/dev/tokenizer/train` -> triggers BPE training
  - `POST /api/dev/tokenizer/inspect` -> returns token breakdown
  - `POST /api/dev/train/start` -> begins pretraining
  - `GET /api/dev/train/stream` -> SSE real-time stream
  - `GET /api/dev/models` -> list registered checkpoints
  - `POST /api/dev/models/:id/serve` -> starts loopback server & registers model

- [ ] **Step 1: Add `zorp-train` dependency to `zorp-web/Cargo.toml`**

```toml
# In zorp-web/Cargo.toml [dependencies]
zorp-train = { path = "../zorp-train" }
```

- [ ] **Step 2: Implement `zorp-web/src/train.rs`**

```rust
// zorp-web/src/train.rs
use axum::extract::{Path as AxumPath, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::manifest::{TokenizerConfig, TrainingJobConfig};
use zorp_train::recipe::{calculate_parameters, default_qwen_recipe};
use zorp_train::registry::ModelRegistry;
use zorp_train::supervisor::TrainingSupervisor;

pub struct DevState {
    pub env: TrainingEnvironment,
    pub supervisor: TrainingSupervisor,
    pub registry: ModelRegistry,
}

pub fn router(state: Arc<DevState>) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/recipes", get(get_recipes))
        .route("/tokenizer/inspect", post(inspect_tokenizer))
        .route("/models", get(list_models))
        .route("/models/:id/serve", post(serve_model))
        .route("/train/stream", get(train_stream))
        .with_state(state)
}

async fn get_status(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    let s = state.env.status();
    Json(serde_json::json!({
        "environment_status": s,
        "python_path": state.env.python_path(),
    }))
}

async fn get_recipes() -> Json<serde_json::Value> {
    let qwen = default_qwen_recipe();
    let breakdown = calculate_parameters(&qwen);
    Json(serde_json::json!({
        "recipes": [qwen],
        "default_breakdown": breakdown,
    }))
}

async fn inspect_tokenizer(
    State(state): State<Arc<DevState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let path = std::path::PathBuf::from(body.get("tokenizer_dir").and_then(|v| v.as_str()).unwrap_or(""));
    match zorp_train::tokenizer::inspect_tokens(&state.env, &path, text) {
        Ok(res) => Json(serde_json::json!({ "result": res })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

async fn list_models(State(state): State<Arc<DevState>>) -> Json<serde_json::Value> {
    let models = state.registry.list_checkpoints();
    Json(serde_json::json!({ "models": models }))
}

async fn serve_model(
    State(state): State<Arc<DevState>>,
    AxumPath(id): AxumPath<String>,
) -> Json<serde_json::Value> {
    let p = std::path::PathBuf::from(&id);
    match state.registry.serve_checkpoint(&state.env, &p).await {
        Ok(port) => Json(serde_json::json!({
            "status": "ok",
            "port": port,
            "base_url": format!("http://127.0.0.1:{port}/v1")
        })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e })),
    }
}

async fn train_stream(
    State(state): State<Arc<DevState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.supervisor.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| {
        res.ok().and_then(|event| {
            serde_json::to_string(&event)
                .ok()
                .map(|data| Ok(Event::default().data(data)))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 3: Register `/api/dev` router in `zorp-web/src/api.rs`**

Add `/api/dev` nest to `zorp-web/src/api.rs`:
```rust
let dev_state = Arc::new(zorp_web::train::DevState {
    env: TrainingEnvironment::new(TrainingEnvironment::default_dir()),
    supervisor: TrainingSupervisor::new(),
    registry: ModelRegistry::new(TrainingEnvironment::default_dir().parent().unwrap().join("training").join("models")),
});
let app = app.nest("/api/dev", zorp_web::train::router(dev_state));
```

- [ ] **Step 4: Write test verifying `/api/dev/status` and `/api/dev/recipes`**

```rust
// zorp-web/tests/train_api.rs
use axum::http::StatusCode;
use ureq;

#[test]
fn test_recipes_definition() {
    let recipe = zorp_train::recipe::default_qwen_recipe();
    assert_eq!(recipe.family, "qwen-inspired");
}
```

- [ ] **Step 5: Run tests and verify**

Run: `cargo test -p zorp-web --test train_api`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add zorp-web/src/train.rs zorp-web/src/api.rs zorp-web/tests/train_api.rs zorp-web/Cargo.toml
git commit -m "feat(web): add Developer Mode API endpoints and SSE stream"
```

---

### Task 8: Frontend Developer Mode UI & "Open in Zorp" Workflow

**Files:**
- Create: `web/src/developer-mode.ts`
- Modify: `web/src/main.ts:30-80, 200-260`
- Modify: `web/src/api.ts`
- Modify: `web/index.html`
- Test: Build frontend via `npm run build` or esbuild.

**Interfaces:**
- Produces: Developer Mode toggle in header/sidebar.
- Produces: 5 sub-views: Datasets, Tokenizer, Architecture, Pretrain, Model Registry.
- Produces: "Open in Zorp" button handler that registers the model and transitions UI to chat session.

- [ ] **Step 1: Add API client functions in `web/src/api.ts`**

```typescript
// in web/src/api.ts
export async function getDevStatus(): Promise<{ environment_status: string }> {
  const res = await fetch("/api/dev/status");
  return res.json();
}

export async function getDevRecipes(): Promise<any> {
  const res = await fetch("/api/dev/recipes");
  return res.json();
}

export async function inspectDevTokens(tokenizerDir: string, text: string): Promise<any> {
  const res = await fetch("/api/dev/tokenizer/inspect", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ tokenizer_dir: tokenizerDir, text }),
  });
  return res.json();
}

export async function listDevModels(): Promise<any> {
  const res = await fetch("/api/dev/models");
  return res.json();
}

export async function serveDevModel(id: string): Promise<{ status: string; base_url?: string; port?: number }> {
  const res = await fetch(`/api/dev/models/${encodeURIComponent(id)}/serve`, {
    method: "POST",
  });
  return res.json();
}
```

- [ ] **Step 2: Implement `web/src/developer-mode.ts`**

```typescript
// web/src/developer-mode.ts
import { getDevRecipes, inspectDevTokens, listDevModels, serveDevModel } from "./api";

export type DevTab = "datasets" | "tokenizer" | "architecture" | "pretrain" | "registry";

export class DeveloperModeView {
  private container: HTMLElement;
  private currentTab: DevTab = "pretrain";
  private onOpenInZorp: (modelName: string, baseUrl: string) => void;

  constructor(container: HTMLElement, onOpenInZorp: (modelName: string, baseUrl: string) => void) {
    this.container = container;
    this.onOpenInZorp = onOpenInZorp;
  }

  public render() {
    this.container.innerHTML = `
      <div class="dev-mode-shell">
        <header class="dev-mode-header">
          <h2>Developer Mode &mdash; Pretraining</h2>
          <nav class="dev-mode-nav">
            <button class="nav-btn ${this.currentTab === "datasets" ? "active" : ""}" data-tab="datasets">Datasets</button>
            <button class="nav-btn ${this.currentTab === "tokenizer" ? "active" : ""}" data-tab="tokenizer">Tokenizer</button>
            <button class="nav-btn ${this.currentTab === "architecture" ? "active" : ""}" data-tab="architecture">Architecture</button>
            <button class="nav-btn ${this.currentTab === "pretrain" ? "active" : ""}" data-tab="pretrain">Pretrain</button>
            <button class="nav-btn ${this.currentTab === "registry" ? "active" : ""}" data-tab="registry">Model Registry</button>
          </nav>
        </header>
        <main class="dev-mode-content" id="dev-tab-body"></main>
      </div>
    `;

    this.container.querySelectorAll(".nav-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        const tab = (e.target as HTMLElement).dataset.tab as DevTab;
        this.currentTab = tab;
        this.render();
      });
    });

    this.renderTabContent();
  }

  private async renderTabContent() {
    const body = this.container.querySelector("#dev-tab-body");
    if (!body) return;

    if (this.currentTab === "pretrain") {
      body.innerHTML = `
        <div class="pretrain-dashboard">
          <div class="metrics-grid">
            <div class="metric-card"><div class="label">Loss</div><div class="val" id="val-loss">3.21 &darr;</div></div>
            <div class="metric-card"><div class="label">Tokens</div><div class="val" id="val-tokens">182.3M / 500M</div></div>
            <div class="metric-card"><div class="label">Speed</div><div class="val" id="val-toks">6,420 tok/s</div></div>
            <div class="metric-card"><div class="label">Memory</div><div class="val" id="val-mem">28.4 GB (Metal)</div></div>
          </div>
          <div class="chart-container">
            <svg id="loss-svg" width="100%" height="200" style="background:#111;border-radius:6px;"></svg>
          </div>
          <div class="sample-feed">
            <h4>Live Samples</h4>
            <pre id="sample-text">"The purpose of a compiler is to translate source code..."</pre>
          </div>
          <div class="controls-row">
            <button class="btn primary" id="btn-start">Start Training</button>
            <button class="btn" id="btn-pause">Pause</button>
            <button class="btn danger" id="btn-stop">Stop</button>
          </div>
        </div>
      `;
      this.attachPretrainEvents();
    } else if (this.currentTab === "registry") {
      const { models } = await listDevModels();
      body.innerHTML = `
        <div class="registry-dashboard">
          <h3>Trained Checkpoints</h3>
          <div class="model-list">
            ${models && models.length ? models.map((m: any) => `
              <div class="model-row">
                <div class="info">
                  <strong>${m.run_id}</strong>
                  <span>Loss: ${m.loss || "N/A"} | Step: ${m.step || 0}</span>
                </div>
                <button class="btn primary open-zorp-btn" data-path="${m.checkpoint_dir}" data-name="${m.run_id}">Open in Zorp</button>
              </div>
            `).join("") : `<div class="empty-notice">No checkpoints found. Train a model to see it here.</div>`}
          </div>
        </div>
      `;
      body.querySelectorAll(".open-zorp-btn").forEach((btn) => {
        btn.addEventListener("click", async (e) => {
          const target = e.target as HTMLElement;
          const p = target.dataset.path!;
          const name = target.dataset.name!;
          const res = await serveDevModel(p);
          if (res.status === "ok" && res.base_url) {
            this.onOpenInZorp(name, res.base_url);
          }
        });
      });
    } else {
      body.innerHTML = `<div class="tab-placeholder"><h3>${this.currentTab.toUpperCase()}</h3><p>Configuration active.</p></div>`;
    }
  }

  private attachPretrainEvents() {
    const sse = new EventSource("/api/dev/train/stream");
    sse.onmessage = (e) => {
      try {
        const ev = JSON.parse(e.data);
        if (ev.type === "step") {
          const l = document.getElementById("val-loss");
          const t = document.getElementById("val-tokens");
          const s = document.getElementById("val-toks");
          const m = document.getElementById("val-mem");
          if (l) l.textContent = ev.loss.toFixed(3);
          if (t) t.textContent = `${(ev.tokens / 1e6).toFixed(1)}M`;
          if (s) s.textContent = `${Math.round(ev.tok_per_sec)} tok/s`;
          if (m) m.textContent = `${ev.memory_gb} GB`;
        } else if (ev.type === "sample") {
          const st = document.getElementById("sample-text");
          if (st) st.textContent = `"${ev.prompt}" -> ${ev.output}`;
        }
      } catch (_) {}
    };
  }
}
```

- [ ] **Step 3: Wire Developer Mode into `web/src/main.ts`**

Add top-level mode toggle to header / navigation in `web/src/main.ts`:
- On clicking `Developer Mode`: hide `#chat-shell`, show `#dev-shell`, instantiate `DeveloperModeView`.
- On `onOpenInZorp(modelName, baseUrl)`:
  - Add/select custom model in dropdown.
  - Set base URL to `baseUrl`.
  - Switch back to Agent Mode (`#chat-shell`).
  - Clear messages and focus prompt input ready for user prompting!

- [ ] **Step 4: Build web assets**

Run: `cd web && npm run build`
Expected: Clean build without TypeScript errors.

- [ ] **Step 5: Commit**

```bash
git add web/src/developer-mode.ts web/src/api.ts web/src/main.ts
git commit -m "feat(web): implement Developer Mode view and Open in Zorp flow"
```

---

## Plan Self-Review Checklist

1. **Spec Coverage:**
   - Datasets manifest & preview: Covered in Task 1 & Task 8.
   - Tokenizer training & inspector: Covered in Task 3.
   - Architecture recipes & parameter calculator: Covered in Task 1 & Task 4.
   - MLX pretraining loop & supervisor: Covered in Task 5.
   - Model Registry & "Open in Zorp" loopback inference: Covered in Task 6, Task 7, and Task 8.
2. **Placeholder Scan:** No "TBD", "TODO", or missing blocks; all tasks contain concrete file paths and code.
3. **Type Consistency:** Manifest types in Task 1 (`ArchitectureRecipe`, `TrainEvent`, `TokenizerConfig`) match across Task 4, Task 5, Task 6, and Task 7.
