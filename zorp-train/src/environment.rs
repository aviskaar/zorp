use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    Missing,
    Installing,
    Ready,
    Corrupted,
}

impl std::fmt::Display for EnvironmentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "missing"),
            Self::Installing => write!(f, "installing"),
            Self::Ready => write!(f, "ready"),
            Self::Corrupted => write!(f, "corrupted"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrainingEnvironment {
    env_dir: PathBuf,
}

impl Default for TrainingEnvironment {
    fn default() -> Self {
        Self::new(Self::default_dir())
    }
}

impl TrainingEnvironment {
    pub fn new(env_dir: PathBuf) -> Self {
        Self { env_dir }
    }

    pub fn env_dir(&self) -> &Path {
        &self.env_dir
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
            .args([
                "-c",
                "import mlx.core; import tokenizers; import safetensors",
            ])
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
            .arg("venv")
            .arg(&self.env_dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !venv_created {
            let status = Command::new("python3")
                .args(["-m", "venv"])
                .arg(&self.env_dir)
                .status()
                .map_err(|e| format!("failed to create venv with python3: {e}"))?;
            if !status.success() {
                return Err("python3 -m venv failed".to_string());
            }
        }

        // 2. Install required packages using uv pip or standard pip
        let packages = [
            "mlx>=0.22.0",
            "tokenizers>=0.21.0",
            "safetensors>=0.4.0",
            "numpy",
            "pyyaml",
        ];
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
            let status = if pip.exists() {
                Command::new(&pip).arg("install").args(packages).status()
            } else {
                Command::new(&py)
                    .args(["-m", "pip", "install"])
                    .args(packages)
                    .status()
            }
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
