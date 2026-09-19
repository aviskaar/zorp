use crate::environment::TrainingEnvironment;
use crate::manifest::{TrainEvent, TrainingJobConfig};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

const MLX_TRAIN_PY: &str = include_str!("../python/mlx_train.py");

pub type TrainingHandle = tokio::task::JoinHandle<()>;

#[derive(Debug, Clone)]
struct ActiveJob {
    pid: u32,
    is_paused: bool,
}

#[derive(Clone)]
pub struct TrainingSupervisor {
    tx: broadcast::Sender<TrainEvent>,
    active_job: Arc<Mutex<Option<ActiveJob>>>,
}

impl Default for TrainingSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl TrainingSupervisor {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            tx,
            active_job: Arc::new(Mutex::new(None)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TrainEvent> {
        self.tx.subscribe()
    }

    pub fn is_running(&self) -> bool {
        self.active_job.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn active_pid(&self) -> Option<u32> {
        self.active_job
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|j| j.pid))
    }

    pub fn pause(&self) -> Result<(), String> {
        let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
        let job = guard.as_mut().ok_or("no active training job")?;
        if job.is_paused {
            return Err("training job is already paused".to_string());
        }

        #[cfg(unix)]
        {
            let raw_pid = job.pid as i32;
            let pid = rustix::process::Pid::from_raw(raw_pid)
                .ok_or_else(|| format!("invalid pid: {raw_pid}"))?;
            rustix::process::kill_process(pid, rustix::process::Signal::STOP)
                .map_err(|e| format!("failed to pause training process: {e}"))?;
            job.is_paused = true;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err("process control not supported on this platform".to_string())
        }
    }

    pub fn resume(&self) -> Result<(), String> {
        let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
        let job = guard.as_mut().ok_or("no active training job")?;
        if !job.is_paused {
            return Err("training job is not paused".to_string());
        }

        #[cfg(unix)]
        {
            let raw_pid = job.pid as i32;
            let pid = rustix::process::Pid::from_raw(raw_pid)
                .ok_or_else(|| format!("invalid pid: {raw_pid}"))?;
            rustix::process::kill_process(pid, rustix::process::Signal::CONT)
                .map_err(|e| format!("failed to resume training process: {e}"))?;
            job.is_paused = false;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err("process control not supported on this platform".to_string())
        }
    }

    pub fn stop(&self) -> Result<(), String> {
        let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
        let job = guard.take().ok_or("no active training job")?;

        #[cfg(unix)]
        {
            let raw_pid = job.pid as i32;
            if let Some(pid) = rustix::process::Pid::from_raw(raw_pid) {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
                if job.is_paused {
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::CONT);
                }
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err("process control not supported on this platform".to_string())
        }
    }

    pub async fn start_job(
        &self,
        env: &TrainingEnvironment,
        config: &TrainingJobConfig,
        run_dir: &Path,
        recipe_json: serde_json::Value,
    ) -> Result<TrainingHandle, String> {
        if self.is_running() {
            return Err("a training job is already running".to_string());
        }

        std::fs::create_dir_all(run_dir).map_err(|e| e.to_string())?;
        let script_path = run_dir.join("mlx_train.py");
        std::fs::write(&script_path, MLX_TRAIN_PY).map_err(|e| e.to_string())?;

        let config_path = run_dir.join("job_config.json");
        let mut full_cfg = serde_json::json!({
            "run_dir": run_dir.to_str().unwrap_or("."),
            "batch_size": config.batch_size,
            "learning_rate": config.learning_rate,
            "max_tokens": config.max_tokens,
            "checkpoint_every_steps": config.checkpoint_every_steps,
            "sample_every_steps": config.sample_every_steps,
            "recipe": recipe_json,
        });
        // Forwarded as given, never guessed. With neither present the
        // Python side trains on synthetic tokens and says so in its init
        // event, which is the honest report for a run with no corpus.
        if let Some(dir) = config.tokenizer_dir.as_deref() {
            full_cfg["tokenizer_dir"] = serde_json::json!(dir);
        }
        if let Some(path) = config.dataset_path.as_deref() {
            full_cfg["dataset_path"] = serde_json::json!(path);
        }
        std::fs::write(&config_path, full_cfg.to_string()).map_err(|e| e.to_string())?;

        let py = env.python_path();
        let mut child = Command::new(py)
            .arg(&script_path)
            .arg("--config")
            .arg(&config_path)
            .env("PYTHONUNBUFFERED", "1")
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn mlx_train: {e}"))?;

        let pid = child.id().ok_or("failed to get child pid")?;
        {
            let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
            *guard = Some(ActiveJob {
                pid,
                is_paused: false,
            });
        }

        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let stderr = child.stderr.take().ok_or("missing stderr")?;
        let tx = self.tx.clone();
        let active_job = Arc::clone(&self.active_job);

        let handle = tokio::spawn(async move {
            let stderr_lines = Arc::new(Mutex::new(VecDeque::with_capacity(30)));
            let stderr_buf_clone = Arc::clone(&stderr_lines);

            let stderr_task = tokio::spawn(async move {
                let mut err_reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = err_reader.next_line().await {
                    tracing::warn!(target: "zorp_train::supervisor", "{line}");
                    if let Ok(mut buf) = stderr_buf_clone.lock() {
                        if buf.len() >= 30 {
                            buf.pop_front();
                        }
                        buf.push_back(line);
                    }
                }
            });

            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(event) = serde_json::from_str::<TrainEvent>(&line) {
                    let _ = tx.send(event);
                }
            }

            let wait_res = child.wait().await;
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), stderr_task).await;

            let collected_stderr: Vec<String> = stderr_lines
                .lock()
                .map(|b| b.iter().cloned().collect())
                .unwrap_or_default();

            match wait_res {
                Ok(status) => {
                    if !status.success() {
                        let status_msg = match status.code() {
                            Some(code) => format!("process exited with code {code}"),
                            None => {
                                #[cfg(unix)]
                                {
                                    use std::os::unix::process::ExitStatusExt;
                                    if let Some(sig) = status.signal() {
                                        format!("process terminated by signal {sig}")
                                    } else {
                                        "process terminated abnormally".to_string()
                                    }
                                }
                                #[cfg(not(unix))]
                                {
                                    "process terminated abnormally".to_string()
                                }
                            }
                        };
                        let message = if collected_stderr.is_empty() {
                            status_msg
                        } else {
                            format!("{status_msg}: {}", collected_stderr.join("\n"))
                        };
                        let _ = tx.send(TrainEvent::Error { message });
                    }
                }
                Err(e) => {
                    let message = if collected_stderr.is_empty() {
                        format!("failed to wait on child process: {e}")
                    } else {
                        format!(
                            "failed to wait on child process: {e}: {}",
                            collected_stderr.join("\n")
                        )
                    };
                    let _ = tx.send(TrainEvent::Error { message });
                }
            }

            if let Ok(mut guard) = active_job.lock() {
                if let Some(ref j) = *guard {
                    if j.pid == pid {
                        *guard = None;
                    }
                }
            }
        });

        Ok(handle)
    }
}
