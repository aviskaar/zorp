//! Isolated, user-triggered setup for the local Qwen3-ASR runtime.

use crate::QwenAsr;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub const QWEN_ASR_PACKAGE: &str = "qwen-asr==0.0.6";
pub const QWEN_ASR_VLLM_PACKAGE: &str = "qwen-asr[vllm]==0.0.6";
pub const VOICE_AUTOSTART_VAR: &str = "ZORP_VOICE_AUTOSTART";
pub const VOICE_SETUP_DIR_VAR: &str = "ZORP_VOICE_SETUP_DIR";
pub const VOICE_PYTHON_VAR: &str = "ZORP_VOICE_PYTHON";

const OWNER_MARKER: &str = ".zorp-voice-environment";
const BACKEND_MARKER: &str = "backend";
const TRANSFORMERS_SERVER: &str = "zorp-qwen-asr-server.py";
const RUNTIME_LOG: &str = "runtime.log";
const SERVER_SOURCE: &str = include_str!("transformers_server.py");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupBackend {
    Vllm,
    Transformers,
}

impl SetupBackend {
    fn marker(self) -> &'static str {
        match self {
            SetupBackend::Vllm => "vllm",
            SetupBackend::Transformers => "transformers",
        }
    }

    fn from_marker(value: &str) -> Option<SetupBackend> {
        match value.trim() {
            "vllm" => Some(SetupBackend::Vllm),
            "transformers" => Some(SetupBackend::Transformers),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    CreatingEnvironment,
    Installing,
    DownloadingModel,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupProgress {
    pub stage: SetupStage,
}

pub type BootstrapStage = SetupStage;
pub type BootstrapProgress = SetupProgress;

#[derive(Debug)]
pub enum BootstrapOutcome {
    Ready,
    Started(Child),
}

#[non_exhaustive]
#[derive(Debug)]
pub enum SetupError {
    Disabled {
        variable: &'static str,
    },
    Root,
    ProxyRequired,
    NoDataDirectory,
    Filesystem {
        action: &'static str,
        message: String,
    },
    PythonMissing,
    CommandFailed {
        step: &'static str,
        status: String,
    },
    Start {
        message: String,
    },
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::Disabled { variable } => {
                write!(f, "automatic voice setup is disabled by {variable}=0")
            }
            SetupError::Root => write!(f, "automatic voice setup refuses to run as root"),
            SetupError::ProxyRequired => write!(
                f,
                "automatic setup is unavailable for this HTTPS or path endpoint; it needs an operator-managed loopback proxy"
            ),
            SetupError::NoDataDirectory => write!(
                f,
                "automatic voice setup could not find a local data directory"
            ),
            SetupError::Filesystem { action, message } => {
                write!(f, "could not {action} for automatic voice setup: {message}")
            }
            SetupError::PythonMissing => write!(
                f,
                "automatic voice setup needs Python 3, but neither python3 nor python could be started"
            ),
            SetupError::CommandFailed { step, status } => {
                write!(f, "automatic voice setup could not {step} ({status})")
            }
            SetupError::Start { message } => {
                write!(f, "the local Qwen3-ASR runtime could not be started: {message}")
            }
        }
    }
}

impl std::error::Error for SetupError {}

/// One fixed installation and launch plan for a checked direct loopback URL.
///
/// Commands are always executed directly with separate argv entries. No value
/// in this plan is interpreted by a shell.
#[derive(Debug, Clone)]
pub struct VoiceSetup {
    home: PathBuf,
    python: PathBuf,
    python_is_explicit: bool,
    host: String,
    port: u16,
    model: String,
}

impl VoiceSetup {
    /// Resolve setup configuration without creating files or starting code.
    pub fn from_env(client: &QwenAsr) -> Result<VoiceSetup, SetupError> {
        if non_empty_os(VOICE_AUTOSTART_VAR).is_some_and(|value| value == "0") {
            return Err(SetupError::Disabled {
                variable: VOICE_AUTOSTART_VAR,
            });
        }
        refuse_root()?;
        let home = non_empty_os(VOICE_SETUP_DIR_VAR)
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|data| data.join("zorp/voice/qwen-asr-0.0.6")))
            .ok_or(SetupError::NoDataDirectory)?;
        let configured_python = non_empty_os(VOICE_PYTHON_VAR).map(PathBuf::from);
        let python_is_explicit = configured_python.is_some();
        let python = configured_python.unwrap_or_else(|| PathBuf::from("python3"));
        VoiceSetup::build(client, home, python, python_is_explicit)
    }

    pub fn new(
        client: &QwenAsr,
        home: impl Into<PathBuf>,
        python: impl Into<PathBuf>,
    ) -> Result<VoiceSetup, SetupError> {
        VoiceSetup::build(client, home.into(), python.into(), true)
    }

    fn build(
        client: &QwenAsr,
        home: PathBuf,
        python: PathBuf,
        python_is_explicit: bool,
    ) -> Result<VoiceSetup, SetupError> {
        let (host, port) = client
            .direct_runtime_target()
            .ok_or(SetupError::ProxyRequired)?;
        let host = if host.trim_end_matches('.').eq_ignore_ascii_case("localhost") {
            "127.0.0.1".to_string()
        } else {
            host.to_string()
        };
        Ok(VoiceSetup {
            home,
            python,
            python_is_explicit,
            host,
            port,
            model: client.model().to_string(),
        })
    }

    pub fn endpoint(&self) -> String {
        if self.host.contains(':') {
            format!("http://[{}]:{}", self.host, self.port)
        } else {
            format!("http://{}:{}", self.host, self.port)
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Create the private environment and select the first backend pip can
    /// resolve. A failed vLLM resolution is discarded before the plain pinned
    /// package is installed, so the fallback never inherits a partial solve.
    pub fn prepare(
        &self,
        mut progress: impl FnMut(SetupProgress),
    ) -> Result<SetupBackend, SetupError> {
        refuse_root()?;
        if let Some(backend) = self.saved_backend()? {
            if backend == SetupBackend::Transformers {
                self.write_transformers_server()?;
            }
            return Ok(backend);
        }

        self.create_environment(&mut progress)?;
        progress(SetupProgress {
            stage: SetupStage::Installing,
        });
        if self.install(QWEN_ASR_VLLM_PACKAGE)? {
            self.save_backend(SetupBackend::Vllm)?;
            return Ok(SetupBackend::Vllm);
        }

        self.remove_owned_environment()?;
        self.create_environment(&mut progress)?;
        progress(SetupProgress {
            stage: SetupStage::Installing,
        });
        if !self.install(QWEN_ASR_PACKAGE)? {
            return Err(SetupError::CommandFailed {
                step: "install the pinned Qwen3-ASR runtime",
                status: "pip exited unsuccessfully".into(),
            });
        }
        self.write_transformers_server()?;
        self.save_backend(SetupBackend::Transformers)?;
        Ok(SetupBackend::Transformers)
    }

    /// Download the configured model when the selected backend needs a
    /// separate step, then start the runtime at the validated loopback target.
    pub fn start(
        &self,
        backend: SetupBackend,
        mut progress: impl FnMut(SetupProgress),
    ) -> Result<Child, SetupError> {
        refuse_root()?;
        self.ensure_owned_environment()?;
        if backend == SetupBackend::Vllm {
            progress(SetupProgress {
                stage: SetupStage::DownloadingModel,
            });
            let status = Command::new(self.environment_python())
                .args([
                    "-c",
                    "from huggingface_hub import snapshot_download; import sys; snapshot_download(sys.argv[1])",
                ])
                .arg(&self.model)
                .stdin(Stdio::null())
                .stdout(self.log_file()?)
                .stderr(self.log_file()?)
                .status()
                .map_err(|error| SetupError::CommandFailed {
                    step: "download the configured Qwen3-ASR model",
                    status: error.to_string(),
                })?;
            if !status.success() {
                return Err(SetupError::CommandFailed {
                    step: "download the configured Qwen3-ASR model",
                    status: exit_status(status),
                });
            }
        }

        let mut command = match backend {
            SetupBackend::Vllm => {
                progress(SetupProgress {
                    stage: SetupStage::Loading,
                });
                let mut command = Command::new(self.runtime_program());
                command
                    .arg(&self.model)
                    .arg("--host")
                    .arg(&self.host)
                    .arg("--port")
                    .arg(self.port.to_string());
                command
            }
            SetupBackend::Transformers => {
                progress(SetupProgress {
                    stage: SetupStage::DownloadingModel,
                });
                let mut command = Command::new(self.environment_python());
                command
                    .arg(self.transformers_server())
                    .arg("--model")
                    .arg(&self.model)
                    .arg("--host")
                    .arg(&self.host)
                    .arg("--port")
                    .arg(self.port.to_string());
                command
            }
        };
        command
            .stdin(Stdio::null())
            .stdout(self.log_file()?)
            .stderr(self.log_file()?)
            .spawn()
            .map_err(|error| SetupError::Start {
                message: error.to_string(),
            })
    }

    fn saved_backend(&self) -> Result<Option<SetupBackend>, SetupError> {
        if !self.home.exists() {
            return Ok(None);
        }
        self.ensure_owned_environment()?;
        if !self.environment_python().is_file() {
            return Ok(None);
        }
        let marker = self.home.join(BACKEND_MARKER);
        match fs::read_to_string(marker) {
            Ok(value) => Ok(SetupBackend::from_marker(&value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(filesystem("read the backend marker", error)),
        }
    }

    fn create_environment(
        &self,
        progress: &mut impl FnMut(SetupProgress),
    ) -> Result<(), SetupError> {
        if self.home.exists() {
            self.ensure_owned_environment()?;
        }
        fs::create_dir_all(&self.home)
            .map_err(|error| filesystem("create the runtime directory", error))?;
        fs::write(self.home.join(OWNER_MARKER), "zorp voice runtime 0.0.6\n")
            .map_err(|error| filesystem("write the environment ownership marker", error))?;
        progress(SetupProgress {
            stage: SetupStage::CreatingEnvironment,
        });
        let status = self.run_venv(&self.python).or_else(|first| {
            if self.python_is_explicit || first.kind() != std::io::ErrorKind::NotFound {
                return Err(first);
            }
            self.run_venv(Path::new("python"))
        });
        // Only a missing program means Python is missing. Anything else,
        // permissions, a busy file, a full disk, is its own reason and is
        // said as such.
        let status = status.map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => SetupError::PythonMissing,
            _ => filesystem("start Python to create its private environment", error),
        })?;
        if !status.success() {
            return Err(SetupError::CommandFailed {
                step: "create its private Python environment",
                status: exit_status(status),
            });
        }
        Ok(())
    }

    fn ensure_owned_environment(&self) -> Result<(), SetupError> {
        if self.home.join(OWNER_MARKER).is_file() {
            return Ok(());
        }
        Err(SetupError::Filesystem {
            action: "reuse the runtime directory",
            message: format!(
                "{} exists but is not marked as zorp-owned",
                self.home.display()
            ),
        })
    }

    fn run_venv(&self, python: &Path) -> std::io::Result<std::process::ExitStatus> {
        Command::new(python)
            .args([OsStr::new("-m"), OsStr::new("venv")])
            .arg(&self.home)
            .stdin(Stdio::null())
            .status()
    }

    fn install(&self, package: &str) -> Result<bool, SetupError> {
        let status = Command::new(self.environment_python())
            .args([
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--no-input",
                package,
            ])
            .stdin(Stdio::null())
            .status()
            .map_err(|error| SetupError::CommandFailed {
                step: "run pip in the private voice environment",
                status: error.to_string(),
            })?;
        Ok(status.success())
    }

    fn remove_owned_environment(&self) -> Result<(), SetupError> {
        if !self.home.join(OWNER_MARKER).is_file() {
            return Err(SetupError::Filesystem {
                action: "replace the partial runtime environment",
                message: "the directory is not marked as zorp-owned".into(),
            });
        }
        fs::remove_dir_all(&self.home)
            .map_err(|error| filesystem("replace the partial runtime environment", error))
    }

    fn save_backend(&self, backend: SetupBackend) -> Result<(), SetupError> {
        fs::write(self.home.join(BACKEND_MARKER), backend.marker())
            .map_err(|error| filesystem("write the voice backend marker", error))
    }

    fn write_transformers_server(&self) -> Result<(), SetupError> {
        fs::write(self.transformers_server(), SERVER_SOURCE)
            .map_err(|error| filesystem("write the local Transformers server", error))
    }

    fn log_file(&self) -> Result<std::fs::File, SetupError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.home.join(RUNTIME_LOG))
            .map_err(|error| filesystem("open the voice runtime log", error))?;
        writeln!(file, "\n--- zorp voice runtime ---")
            .map_err(|error| filesystem("write the voice runtime log", error))?;
        Ok(file)
    }

    fn environment_python(&self) -> PathBuf {
        self.home.join(scripts_dir()).join(python_name())
    }

    fn runtime_program(&self) -> PathBuf {
        self.home.join(scripts_dir()).join(runtime_name())
    }

    fn transformers_server(&self) -> PathBuf {
        self.home.join(TRANSFORMERS_SERVER)
    }
}

impl QwenAsr {
    /// Ensure the checked local runtime exists without weakening the client's
    /// loopback boundary. The caller owns any child this starts.
    pub fn ensure_runtime(
        &self,
        progress: impl FnMut(BootstrapProgress),
    ) -> Result<BootstrapOutcome, SetupError> {
        let status = self.status();
        if status.runtime_reachable && status.model_present {
            return Ok(BootstrapOutcome::Ready);
        }
        if status.runtime_reachable {
            return Err(SetupError::Start {
                message: "a local runtime is answering without the configured model".into(),
            });
        }
        let setup = VoiceSetup::from_env(self)?;
        let mut progress = progress;
        let backend = setup.prepare(&mut progress)?;
        setup
            .start(backend, progress)
            .map(BootstrapOutcome::Started)
    }
}

fn non_empty_os(var: &str) -> Option<OsString> {
    std::env::var_os(var).filter(|value| !value.is_empty())
}

fn filesystem(action: &'static str, error: std::io::Error) -> SetupError {
    SetupError::Filesystem {
        action,
        message: error.to_string(),
    }
}

fn refuse_root() -> Result<(), SetupError> {
    #[cfg(unix)]
    {
        refuse_root_for(rustix::process::geteuid().is_root())
    }
    #[cfg(not(unix))]
    {
        refuse_root_for(false)
    }
}

fn refuse_root_for(is_root: bool) -> Result<(), SetupError> {
    if is_root {
        Err(SetupError::Root)
    } else {
        Ok(())
    }
}

fn exit_status(status: std::process::ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "terminated by a signal".into())
}

#[cfg(windows)]
fn scripts_dir() -> &'static str {
    "Scripts"
}

#[cfg(not(windows))]
fn scripts_dir() -> &'static str {
    "bin"
}

#[cfg(windows)]
fn python_name() -> &'static str {
    "python.exe"
}

#[cfg(not(windows))]
fn python_name() -> &'static str {
    "python"
}

#[cfg(windows)]
fn runtime_name() -> &'static str {
    "qwen-asr-serve.exe"
}

#[cfg(not(windows))]
fn runtime_name() -> &'static str {
    "qwen-asr-serve"
}

#[cfg(test)]
mod tests {
    use super::{refuse_root_for, SetupError};

    #[test]
    fn root_is_refused_before_any_install_or_spawn() {
        assert!(matches!(refuse_root_for(true), Err(SetupError::Root)));
        assert!(refuse_root_for(false).is_ok());
    }
}
