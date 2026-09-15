use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;
use crate::environment::TrainingEnvironment;
use crate::manifest::{TrainEvent, TrainingJobConfig};

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
        self.active_job
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
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
        let full_cfg = serde_json::json!({
            "run_dir": run_dir.to_str().unwrap_or("."),
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

        let pid = child.id().ok_or("failed to get child pid")?;
        {
            let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
            *guard = Some(ActiveJob {
                pid,
                is_paused: false,
            });
        }

        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let tx = self.tx.clone();
        let active_job = Arc::clone(&self.active_job);

        let handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(event) = serde_json::from_str::<TrainEvent>(&line) {
                    let _ = tx.send(event);
                }
            }
            let _ = child.wait().await;
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
