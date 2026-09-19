use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use tempfile::NamedTempFile;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::environment::TrainingEnvironment;
use crate::manifest::CheckpointMetadata;

const MLX_SERVE_PY: &str = include_str!("../python/mlx_serve.py");

#[derive(serde::Deserialize)]
struct CheckpointJson {
    run_id: Option<String>,
    step: Option<usize>,
    loss: Option<f64>,
    created_at_iso: Option<String>,
}

pub struct ModelRegistry {
    models_dir: PathBuf,
    active_server: Mutex<Option<Child>>,
    active_port: Mutex<Option<u16>>,
    active_script: Mutex<Option<NamedTempFile>>,
}

impl Drop for ModelRegistry {
    fn drop(&mut self) {
        let _ = self.stop_server();
    }
}

impl ModelRegistry {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            active_server: Mutex::new(None),
            active_port: Mutex::new(None),
            active_script: Mutex::new(None),
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn is_serving(&self) -> bool {
        self.active_server
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    pub fn active_port(&self) -> Option<u16> {
        self.active_port.lock().ok().and_then(|g| *g)
    }

    pub fn list_checkpoints(&self) -> Vec<CheckpointMetadata> {
        let mut checkpoints = Vec::new();
        if !self.models_dir.exists() {
            return checkpoints;
        }

        scan_dir_for_checkpoints(&self.models_dir, 0, &self.models_dir, &mut checkpoints);
        checkpoints.sort_by(|a, b| a.checkpoint_dir.cmp(&b.checkpoint_dir));
        checkpoints
    }

    pub async fn serve_checkpoint(
        &self,
        env: &TrainingEnvironment,
        checkpoint_dir: &Path,
    ) -> Result<u16, String> {
        self.stop_server()?;

        let mut temp_script = tempfile::Builder::new()
            .prefix("zorp_mlx_serve_")
            .suffix(".py")
            .tempfile()
            .map_err(|e| format!("failed to create tempfile for mlx_serve: {e}"))?;

        temp_script
            .write_all(MLX_SERVE_PY.as_bytes())
            .map_err(|e| format!("failed to write mlx_serve: {e}"))?;
        temp_script
            .flush()
            .map_err(|e| format!("failed to flush mlx_serve tempfile: {e}"))?;

        let script_path = temp_script.path().to_path_buf();
        let py = env.python_path();

        let mut child = Command::new(py)
            .arg(&script_path)
            .arg("--checkpoint")
            .arg(checkpoint_dir)
            .arg("--port")
            .arg("0")
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start mlx_serve: {e}"))?;

        // Drain stderr in the background
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut err_reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = err_reader.next_line().await {
                    tracing::debug!(target: "zorp_train::registry::mlx_serve", "{line}");
                }
            });
        }

        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut reader = BufReader::new(stdout).lines();

        let timeout_duration = std::time::Duration::from_secs(10);
        let wait_result = tokio::time::timeout(timeout_duration, async {
            while let Ok(Some(line)) = reader.next_line().await {
                if let Some(port_str) = line.strip_prefix("SERVER_BOUND:") {
                    if let Ok(p) = port_str.trim().parse::<u16>() {
                        return Ok(p);
                    }
                }
            }
            Err("server exited without reporting SERVER_BOUND:<port>".to_string())
        })
        .await;

        let port = match wait_result {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                let _ = child.start_kill();
                return Err(e);
            }
            Err(_) => {
                let _ = child.start_kill();
                return Err("timeout waiting for mlx_serve to bind".to_string());
            }
        };

        // Drain remaining stdout in background
        tokio::spawn(async move { while let Ok(Some(_)) = reader.next_line().await {} });

        *self.active_server.lock().map_err(|e| e.to_string())? = Some(child);
        *self.active_port.lock().map_err(|e| e.to_string())? = Some(port);
        *self.active_script.lock().map_err(|e| e.to_string())? = Some(temp_script);

        Ok(port)
    }

    pub fn stop_server(&self) -> Result<(), String> {
        let mut guard = self.active_server.lock().map_err(|e| e.to_string())?;
        if let Some(mut child) = guard.take() {
            let _ = child.start_kill();
        }
        if let Ok(mut port_guard) = self.active_port.lock() {
            *port_guard = None;
        }
        if let Ok(mut script_guard) = self.active_script.lock() {
            *script_guard = None;
        }
        Ok(())
    }
}

fn scan_dir_for_checkpoints(
    dir: &Path,
    depth: usize,
    root_models_dir: &Path,
    out: &mut Vec<CheckpointMetadata>,
) {
    if depth > 5 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if p.join("model.safetensors").exists() {
                out.push(extract_checkpoint_metadata(&p, root_models_dir));
            } else {
                scan_dir_for_checkpoints(&p, depth + 1, root_models_dir, out);
            }
        }
    }
}

fn extract_checkpoint_metadata(path: &Path, root_models_dir: &Path) -> CheckpointMetadata {
    let mut run_id = None;
    let mut step = None;
    let mut loss = None;
    let mut created_at_iso = None;

    // Try reading checkpoint.json if present
    if let Ok(content) = std::fs::read_to_string(path.join("checkpoint.json")) {
        if let Ok(cfg) = serde_json::from_str::<CheckpointJson>(&content) {
            run_id = cfg.run_id;
            step = cfg.step;
            loss = cfg.loss;
            created_at_iso = cfg.created_at_iso;
        }
    }

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // Infer step if not in checkpoint.json
    let resolved_step = step.unwrap_or_else(|| {
        if let Some(suffix) = file_name.strip_prefix("step_") {
            suffix.parse::<usize>().unwrap_or(0)
        } else if let Some(suffix) = file_name.strip_prefix("step-") {
            suffix.parse::<usize>().unwrap_or(0)
        } else if let Some(suffix) = file_name.strip_prefix("checkpoint_") {
            suffix.parse::<usize>().unwrap_or(0)
        } else if let Some(suffix) = file_name.strip_prefix("checkpoint-") {
            suffix.parse::<usize>().unwrap_or(0)
        } else {
            file_name.parse::<usize>().unwrap_or(0)
        }
    });

    // Infer run_id if not in checkpoint.json
    let resolved_run_id = run_id.unwrap_or_else(|| {
        let is_step_or_ckpt_dir = file_name.starts_with("step_")
            || file_name.starts_with("step-")
            || file_name.starts_with("checkpoint_")
            || file_name.starts_with("checkpoint-")
            || file_name.chars().all(|c| c.is_ascii_digit());

        if is_step_or_ckpt_dir {
            if let Some(parent) = path.parent() {
                let p_name = parent
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if p_name == "checkpoints" {
                    if let Some(grandparent) = parent.parent() {
                        return grandparent
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or(file_name.clone());
                    }
                } else if parent != root_models_dir && !p_name.is_empty() {
                    return p_name;
                }
            }
        }
        file_name
    });

    CheckpointMetadata {
        run_id: resolved_run_id,
        step: resolved_step,
        loss: loss.unwrap_or(0.0),
        checkpoint_dir: path.to_string_lossy().to_string(),
        created_at_iso: created_at_iso.unwrap_or_else(|| "2026-09-14T00:00:00Z".to_string()),
    }
}
