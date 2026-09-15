use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use serde::{Deserialize, Serialize};
use tempfile::Builder;
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
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    let mut script_file = Builder::new()
        .prefix("train_bpe_")
        .suffix(".py")
        .tempfile()
        .map_err(|e| e.to_string())?;
    script_file
        .write_all(TRAIN_BPE_PY.as_bytes())
        .map_err(|e| e.to_string())?;
    let script_path = script_file.into_temp_path();

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
    let mut script_file = Builder::new()
        .prefix("inspect_tokens_")
        .suffix(".py")
        .tempfile()
        .map_err(|e| e.to_string())?;
    script_file
        .write_all(INSPECT_TOKENS_PY.as_bytes())
        .map_err(|e| e.to_string())?;
    let script_path = script_file.into_temp_path();

    let py = env.python_path();
    let mut child = Command::new(py)
        .arg(&script_path)
        .arg("--tokenizer-dir")
        .arg(tokenizer_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("inspect tokens failed: {err}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse json error: {e}"))
}
