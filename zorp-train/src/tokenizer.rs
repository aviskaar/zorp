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
        .arg(config.vocab_size.to_string());

    if !config.special_tokens.is_empty() {
        cmd.arg("--special-tokens");
        for token in &config.special_tokens {
            cmd.arg(token);
        }
    }

    let output = cmd.output();
    let _ = std::fs::remove_file(&script_path);
    let output = output.map_err(|e| e.to_string())?;

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
        .output();

    let _ = std::fs::remove_file(&script_path);
    let output = output.map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("inspect tokens failed: {err}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse json error: {e}"))
}
