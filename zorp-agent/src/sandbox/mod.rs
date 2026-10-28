// Process supervision needs raw syscalls: setpgid in pre_exec so the
// child leads its own process group, and waitid/killpg to reap it and
// its descendants, or a job object on Windows. There is no safe std
// equivalent, so the workspace unsafe_code lint is turned off for this
// module only.
#![allow(unsafe_code)]

mod capture;
mod group;
pub mod srt;

use crate::tools::ToolError;
use capture::{capture_stream, render_capture};
pub(crate) use group::{shell, ProcessGroup};
pub use srt::{SandboxMode, SrtStatus};
use std::collections::HashSet;
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub type CancelToken = Arc<AtomicBool>;

pub fn cancel_token() -> CancelToken {
    Arc::new(AtomicBool::new(false))
}

pub struct Sandbox {
    repo_root: PathBuf,
    cancel: CancelToken,
    timeout: Duration,
    output_cap: usize,
    reader_finalize_delay: Duration,
    /// Which OS-level sandbox the command runs under. `Off` is today's
    /// behavior and the default; any other mode wraps the command in `srt`.
    /// Set by the surface that resolved it, never by the model.
    mode: SandboxMode,
    /// Secret snapshot, taken lazily on the first `run` and reused for the
    /// sandbox's lifetime. Per instance rather than process-wide so tests
    /// that mutate the environment before building a sandbox stay honest.
    secrets: OnceLock<Arc<Vec<Vec<u8>>>>,
    /// Whether `srt` and its dependencies are present, probed once and
    /// reused for the sandbox's lifetime the way `secrets` is. Only ever
    /// consulted when `mode` is not `Off`.
    srt_status: OnceLock<SrtStatus>,
}

#[derive(Debug)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl CommandOutput {
    pub fn render(&self) -> String {
        format!(
            "exit_status: {}\ntimed_out: {}\ncancelled: {}\nstdout:\n{}\nstderr:\n{}",
            self.status
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".into()),
            self.timed_out,
            self.cancelled,
            self.stdout,
            self.stderr
        )
    }
}

impl Sandbox {
    pub fn new(repo_root: PathBuf, cancel: CancelToken) -> Self {
        let repo_root = shell_dir(repo_root.canonicalize().unwrap_or(repo_root));
        Self {
            repo_root,
            cancel,
            timeout: Duration::from_secs(120),
            output_cap: crate::tools::RUN_COMMAND_OUTPUT_CAP,
            reader_finalize_delay: Duration::ZERO,
            mode: SandboxMode::Off,
            secrets: OnceLock::new(),
            srt_status: OnceLock::new(),
        }
    }

    /// Run every command under `mode`. The surface resolves the mode from the
    /// flag, the environment, the flavor and the saved settings; the model
    /// never sets it. `Off` is the default and leaves `run` exactly as it was.
    pub fn with_mode(mut self, mode: SandboxMode) -> Self {
        self.mode = mode;
        self
    }

    /// The mode this sandbox runs commands under.
    pub fn mode(&self) -> SandboxMode {
        self.mode
    }

    /// Set the mode on an already-built sandbox, for callers that hold one by
    /// `&mut` (the tool `Context` does).
    pub fn set_mode(&mut self, mode: SandboxMode) {
        self.mode = mode;
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[cfg(test)]
    fn with_output_cap(mut self, cap: usize) -> Self {
        self.output_cap = cap;
        self
    }

    #[cfg(test)]
    fn with_reader_finalize_delay(mut self, delay: Duration) -> Self {
        self.reader_finalize_delay = delay;
        self
    }

    pub fn run(&self, command: &str) -> Result<CommandOutput, ToolError> {
        let secrets = self
            .secrets
            .get_or_init(|| Arc::new(secret_values()))
            .clone();
        let shell = shell().map_err(|e| ToolError::new(format!("spawn: {e}")))?;
        // Hold the generated `srt` settings file for the whole run; it is
        // removed when this guard drops. `None` when the mode is `Off`, which
        // runs the shell directly exactly as before. A mode that is on but
        // whose tools are missing refuses here, loudly, and never falls back
        // to running unsandboxed.
        let settings = self.srt_settings()?;
        let mut cmd = match &settings {
            Some(settings) => {
                let mut cmd = Command::new(srt::SRT_BIN);
                cmd.args(srt::wrap(settings.path(), &shell, command));
                cmd
            }
            None => {
                let mut cmd = Command::new(&shell);
                cmd.arg("-c").arg(command);
                cmd
            }
        };
        cmd.current_dir(&self.repo_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        ProcessGroup::prepare(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::new(format!("spawn: {e}")))?;
        let group = match ProcessGroup::adopt(&child) {
            Ok(group) => group,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::new(format!("adopt process group: {e}")));
            }
        };
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::new("stdout pipe unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolError::new("stderr pipe unavailable"))?;
        let stdout_probe = probe(&stdout)?;
        let stderr_probe = probe(&stderr)?;
        let output_cap = self.output_cap;
        let finalize_delay = self.reader_finalize_delay;
        let stdout_eof = Arc::new(AtomicBool::new(false));
        let stderr_eof = Arc::new(AtomicBool::new(false));
        let stdout_secrets = secrets.clone();
        let stdout_eof_reader = stdout_eof.clone();
        let out_reader = thread::spawn(move || {
            capture_stream(
                &mut stdout,
                output_cap,
                stdout_secrets,
                stdout_eof_reader,
                finalize_delay,
            )
        });
        let stderr_eof_reader = stderr_eof.clone();
        let err_reader = thread::spawn(move || {
            capture_stream(
                &mut stderr,
                output_cap,
                secrets,
                stderr_eof_reader,
                finalize_delay,
            )
        });
        let started = Instant::now();
        let (status, timed_out, cancelled) = loop {
            if child_exited(&mut child)? {
                let pipes_closed = stream_closed(&stdout_eof, &stdout_probe)?
                    && stream_closed(&stderr_eof, &stderr_probe)?;
                let kill_result = if pipes_closed { Ok(()) } else { group.kill() };
                let status = child
                    .wait()
                    .map_err(|e| ToolError::new(format!("reap: {e}")))?;
                kill_result.map_err(|e| ToolError::new(format!("kill process group: {e}")))?;
                break (status.code(), false, false);
            }
            let cancelled = self.cancel.load(Ordering::SeqCst);
            let timed_out = started.elapsed() >= self.timeout;
            if cancelled || timed_out {
                let kill_result = group.kill();
                if let Err(error) = kill_result {
                    child.try_wait().map_err(|e| {
                        ToolError::new(format!("check child after failed kill: {e}"))
                    })?;
                    return Err(ToolError::new(format!("kill process group: {error}")));
                }
                let status = child
                    .wait()
                    .map_err(|e| ToolError::new(format!("reap: {e}")))?;
                break (status.code(), timed_out, cancelled);
            }
            thread::sleep(Duration::from_millis(20));
        };
        let stdout = out_reader
            .join()
            .map_err(|_| ToolError::new("stdout reader panicked"))?
            .map_err(|e| ToolError::new(format!("read stdout: {e}")))?;
        let stderr = err_reader
            .join()
            .map_err(|_| ToolError::new("stderr reader panicked"))?
            .map_err(|e| ToolError::new(format!("read stderr: {e}")))?;
        // Drop the settings file now that the command is reaped. Named so it
        // is obvious the lifetime was deliberate and not an unused binding.
        drop(settings);
        let mut stderr = render_capture(&stderr, self.output_cap);
        // A sandbox refusal reads as a filesystem or network error from the
        // command's point of view. When the command failed under a mode,
        // name the rule that most likely fired so the model can correct the
        // command rather than retry the same shape (ADR 0095).
        if !self.mode.is_off() && status != Some(0) && !timed_out && !cancelled {
            if let Some(denial) = srt::denial_from_output(self.mode, &stderr) {
                stderr = format!("{denial}\n{stderr}");
            }
        }
        Ok(CommandOutput {
            status,
            stdout: render_capture(&stdout, self.output_cap),
            stderr,
            timed_out,
            cancelled,
        })
    }

    /// The `srt` settings file for this run, or `None` when the mode is
    /// `Off`. Refuses loudly when a mode is on but `srt` or a dependency is
    /// missing: the one thing this must never do is run a command that asked
    /// for a sandbox without one.
    fn srt_settings(&self) -> Result<Option<SettingsFile>, ToolError> {
        if self.mode.is_off() {
            return Ok(None);
        }
        let status = self.srt_status.get_or_init(srt::detect);
        if !status.usable() {
            return Err(ToolError::new(format!(
                "sandbox mode {} was requested but cannot run: {}. Install the missing \
                 tools, or set the sandbox to off.",
                self.mode,
                status.detail()
            )));
        }
        let json = srt::settings_json(self.mode, &self.repo_root)
            .ok_or_else(|| ToolError::new("sandbox: no settings for an active mode"))?;
        SettingsFile::write(&json).map(Some)
    }
}

/// A generated `srt` settings file, removed when dropped. Written under the
/// system temp directory with a unique name per run, so two commands running
/// at once never share one.
struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    fn write(json: &serde_json::Value) -> Result<SettingsFile, ToolError> {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "zorp-srt-{}-{nanos}-{seq}.json",
            std::process::id()
        ));
        let body = serde_json::to_vec(json)
            .map_err(|e| ToolError::new(format!("sandbox settings: {e}")))?;
        std::fs::write(&path, body)
            .map_err(|e| ToolError::new(format!("sandbox settings: {e}")))?;
        Ok(SettingsFile { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for SettingsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The directory a shell is started in. On Windows a canonical path is a
/// verbatim `\\?\C:\...` path, which Git Bash cannot use as a working
/// directory, so a drive path loses the prefix here and only here: the
/// containment checks elsewhere compare canonical paths with canonical
/// paths and keep it.
fn shell_dir(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(rest) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        if rest.as_bytes().get(1) == Some(&b':') {
            return PathBuf::from(rest);
        }
    }
    path
}

/// A second handle on an output pipe, polled for hangup while a reader
/// thread owns the first. Windows has no poll on a pipe, so it has nothing
/// to hold and reads the reader's own EOF flag instead.
#[cfg(unix)]
type Probe = OwnedFd;
#[cfg(windows)]
struct Probe;

#[cfg(unix)]
fn probe(stream: &impl AsRawFd) -> Result<Probe, ToolError> {
    duplicate_fd(stream.as_raw_fd())
}

#[cfg(windows)]
fn probe<T>(_stream: &T) -> Result<Probe, ToolError> {
    Ok(Probe)
}

#[cfg(windows)]
fn stream_closed(eof: &AtomicBool, _probe: &Probe) -> Result<bool, ToolError> {
    Ok(eof.load(Ordering::SeqCst))
}

/// Whether the shell has exited. Unix asks without reaping, so the pid
/// cannot be reused while the group is still being killed. A Windows
/// process handle stays valid until it is closed, so `try_wait` is enough.
#[cfg(unix)]
fn child_exited(child: &mut Child) -> Result<bool, ToolError> {
    child_exited_unreaped(child.id() as i32)
}

#[cfg(windows)]
fn child_exited(child: &mut Child) -> Result<bool, ToolError> {
    child
        .try_wait()
        .map(|status| status.is_some())
        .map_err(|e| ToolError::new(format!("observe child: {e}")))
}

#[cfg(unix)]
fn duplicate_fd(fd: i32) -> Result<OwnedFd, ToolError> {
    let duplicated = unsafe { libc::dup(fd) };
    if duplicated == -1 {
        return Err(ToolError::new(format!(
            "duplicate output pipe: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

#[cfg(unix)]
fn stream_closed(eof: &AtomicBool, probe: &OwnedFd) -> Result<bool, ToolError> {
    if eof.load(Ordering::SeqCst) {
        return Ok(true);
    }
    let mut descriptor = libc::pollfd {
        fd: probe.as_raw_fd(),
        events: libc::POLLHUP,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result == -1 {
        return Err(ToolError::new(format!(
            "poll output pipe: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(descriptor.revents & libc::POLLHUP != 0)
}

#[cfg(unix)]
fn child_exited_unreaped(pid: i32) -> Result<bool, ToolError> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == -1 {
        return Err(ToolError::new(format!(
            "observe child: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { info.si_pid() } != 0)
}

/// Whether a failed group kill means the group had nothing left to kill.
///
/// ESRCH is the obvious one: no such process group. EPERM is the same
/// situation wearing a different errno. Every member of this group is a
/// process we spawned, so we are always allowed to signal it while it is
/// alive; the kernel only refuses when there is nothing signalable there.
/// On macOS a group whose last member is an exited-but-unreaped child
/// answers EPERM rather than ESRCH, and both callers below reach this
/// after the child is known to have exited. The third caller,
/// `tools::Context::kill_background_process`, may reach it while the
/// group is still running; there the same reading holds, because that
/// group was also created by us with setpgid.
///
/// Anything else is a real error and still fails the run. Getting this
/// wrong in the forgiving direction would hide a kill that did not happen;
/// getting it wrong in the strict direction is what produced a flaky test
/// suite, because a command that had already succeeded failed while
/// cleaning up something that was no longer there.
#[cfg(unix)]
fn kill_error_is_already_gone(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ESRCH) | Some(libc::EPERM))
}

#[cfg(unix)]
pub(crate) fn kill_process_group(pgid: i32) -> io::Result<()> {
    if unsafe { libc::kill(-pgid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if kill_error_is_already_gone(&error) {
        Ok(())
    } else {
        Err(error)
    }
}

fn secret_values() -> Vec<Vec<u8>> {
    let mut seen = HashSet::new();
    let mut values = Vec::new();
    for (name, value) in std::env::vars_os() {
        let name = name.as_encoded_bytes();
        let value = value.as_encoded_bytes().to_vec();
        if !value.is_empty()
            && ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                .iter()
                .any(|pattern| contains_ascii_case_insensitive(name, pattern.as_bytes()))
            && seen.insert(value.clone())
        {
            values.push(value);
        }
    }
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values
}

#[allow(dead_code)]
pub(crate) fn redact_secrets(input: &str) -> String {
    let secrets = secret_values();
    let mut output = input.to_string();
    for secret_bytes in &secrets {
        if let Ok(secret_str) = std::str::from_utf8(secret_bytes) {
            if !secret_str.is_empty() {
                output = output.replace(secret_str, "[REDACTED]");
            }
        }
    }
    output
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The three errnos that decide whether a failed group kill is a real
    // failure. ESRCH and EPERM both mean the group had nothing left in it
    // that we could signal, which is the outcome the kill wanted. EINVAL
    // means the call itself was wrong and must still be reported.
    //
    // EPERM is the one that used to be missing, and it was not theoretical:
    // `cargo test -p zorp-agent --lib sandbox::` failed roughly once in
    // twenty-five runs on macOS with "kill process group: Operation not
    // permitted (os error 1)", taking whichever sandbox test happened to
    // lose the race. The child is already confirmed dead by
    // `child_exited_unreaped` before this kill is attempted, so on a group
    // holding only an unreaped zombie macOS answers EPERM where Linux
    // answers ESRCH. Same fact, different errno, and only one of them was
    // being forgiven.
    #[cfg(unix)]
    #[test]
    fn a_group_that_is_already_gone_is_not_a_failed_kill() {
        assert!(kill_error_is_already_gone(&io::Error::from_raw_os_error(
            libc::ESRCH
        )));
        assert!(kill_error_is_already_gone(&io::Error::from_raw_os_error(
            libc::EPERM
        )));
        assert!(!kill_error_is_already_gone(&io::Error::from_raw_os_error(
            libc::EINVAL
        )));
    }
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    use std::sync::Mutex;
    use std::thread;

    // Environment mutation is process-global, so every test that changes it
    // holds this lock for the mutation's full lifetime.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn observes_child_exit_without_reaping_it() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 7")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !child_exited_unreaped(child.id() as i32).unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(child.wait().unwrap().code(), Some(7));
    }

    struct EnvVarGuard(&'static str, Option<std::ffi::OsString>);

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            Self::set_os(name, value)
        }

        fn set_os(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self(name, previous)
        }
    }

    #[cfg(unix)]
    #[test]
    fn snapshots_non_unicode_environment_best_effort() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let invalid = std::ffi::OsString::from_vec(vec![b'a', 0xff, b'b']);
        let _env = EnvVarGuard::set_os("ZORP_TEST_SECRET", &invalid);
        let dir = tempfile::tempdir().unwrap();

        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .run("printf '%s' \"$ZORP_TEST_SECRET\"")
            .unwrap();

        assert_eq!(out.stdout, "[REDACTED]");
        assert!(!out.stdout.contains('�'));
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => std::env::set_var(self.0, value),
                None => std::env::remove_var(self.0),
            }
        }
    }

    #[test]
    fn output_below_cap_is_not_dropped_after_head_fills() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_output_cap(10)
            .run("printf 12345678")
            .unwrap();

        assert_eq!(out.stdout, "12345678");
    }

    #[test]
    fn eof_signal_precedes_delayed_reader_finalization() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(1))
            .with_reader_finalize_delay(Duration::from_millis(100))
            .run("printf done")
            .unwrap();

        assert_eq!(out.stdout, "done");
        assert!(!out.timed_out);
    }

    #[test]
    fn truncation_preserves_utf8_boundaries_and_uses_one_marker() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_output_cap(40)
            .run("printf 'éééééééééééééééééééééééééééé'")
            .unwrap();

        assert!(!out.stdout.contains('\u{fffd}'));
        assert_eq!(out.stdout.matches("truncated").count(), 1);
        assert!(out.stdout.starts_with("éé"));
        assert!(out.stdout.ends_with("éé"));
        assert!(out.stdout.len() <= 40);
    }

    #[test]
    fn completed_shell_kills_background_descendants_before_joining_readers() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("background-survived");
        let command = format!("(sleep 1; touch '{}') &", marker.display());
        let started = Instant::now();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_millis(150))
            .run(&command)
            .unwrap();

        assert!(!out.timed_out);
        assert!(started.elapsed() < Duration::from_millis(500));
        thread::sleep(Duration::from_millis(1100));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn runs_at_repo_root_and_captures_both_streams() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(2))
            .run("pwd; printf err >&2; exit 7")
            .unwrap();
        assert_eq!(out.status, Some(7));
        assert_eq!(
            out.stdout.trim(),
            dir.path().canonicalize().unwrap().display().to_string()
        );
        assert_eq!(out.stderr, "err");
    }

    // Git Bash prints `/c/Users/...` for `pwd`; `pwd -W` is the same
    // directory in the form Windows writes it, with forward slashes.
    #[cfg(windows)]
    #[test]
    fn runs_at_repo_root_and_captures_both_streams() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(5))
            .run("pwd -W; printf err >&2; exit 7")
            .unwrap();
        assert_eq!(out.status, Some(7));
        assert_eq!(
            PathBuf::from(out.stdout.trim().replace('/', "\\")),
            shell_dir(dir.path().canonicalize().unwrap())
        );
        assert_eq!(out.stderr, "err");
    }

    #[test]
    fn caps_and_redacts_output() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZORP_TEST_SECRET_TOKEN", "m4-secret-value");
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_output_cap(128)
            .run("printf '%s' \"$ZORP_TEST_SECRET_TOKEN\"; yes x | head -c 256")
            .unwrap();
        assert!(!out.stdout.contains("m4-secret-value"));
        assert!(out.stdout.contains("[REDACTED]"));
        assert!(out.stdout.contains("truncated"));
        assert!(out.stdout.contains("head or tail"));
    }

    #[test]
    fn reuses_the_secret_snapshot_across_runs_on_one_sandbox() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZORP_TEST_SECRET_TOKEN", "reused-secret");
        let sandbox = Sandbox::new(dir.path().to_path_buf(), cancel_token());

        let first = sandbox.run("printf reused-secret").unwrap();
        assert_eq!(first.stdout, "[REDACTED]");

        // The variable is gone from the environment now, but the sandbox
        // keeps its first snapshot and still redacts the value.
        std::env::remove_var("ZORP_TEST_SECRET_TOKEN");
        let second = sandbox.run("printf reused-secret").unwrap();
        assert_eq!(second.stdout, "[REDACTED]");
    }

    #[test]
    fn snapshots_secrets_before_the_child_runs() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZORP_TEST_SECRET_TOKEN", "snapshot-secret");
        let remover = thread::spawn(|| {
            thread::sleep(Duration::from_millis(40));
            std::env::remove_var("ZORP_TEST_SECRET_TOKEN");
        });
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(1))
            .run("sleep 0.1; printf snapshot-secret")
            .unwrap();
        remover.join().unwrap();

        assert_eq!(out.stdout, "[REDACTED]");
    }

    #[test]
    fn redacts_longest_overlapping_secret_before_truncation() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _short = EnvVarGuard::set("ZORP_TEST_TOKEN", "abc");
        let _long = EnvVarGuard::set("ZORP_TEST_SECRET", "abcdef");
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_output_cap(40)
            .run("printf xxabcdef0123456789")
            .unwrap();

        assert!(!out.stdout.contains("abc"));
        assert!(!out.stdout.contains("def"));
        assert!(out.stdout.contains("[REDACTED]"));
    }

    #[test]
    fn timeout_kills_descendant_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("late.txt");
        let command = format!("(sleep 1; touch '{}') & wait", marker.display());
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_millis(100))
            .run(&command)
            .unwrap();
        assert!(out.timed_out);
        thread::sleep(Duration::from_millis(1200));
        assert!(!marker.exists());
    }

    #[test]
    fn cancellation_kills_running_group() {
        let dir = tempfile::tempdir().unwrap();
        let token = cancel_token();
        let setter = token.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            setter.store(true, Ordering::SeqCst);
        });
        let out = Sandbox::new(dir.path().to_path_buf(), token)
            .with_timeout(Duration::from_secs(2))
            .run("sleep 10")
            .unwrap();
        assert!(out.cancelled);
    }

    /// Whether a sandboxed command could actually run here. The enforcement
    /// tests below only mean anything when it can, so they skip when it
    /// cannot rather than failing on a machine with no `srt` installed.
    fn srt_usable() -> bool {
        srt::detect().usable()
    }

    /// The one thing a sandbox must never do: silently run a command that
    /// asked for a wall without one. With `srt` missing, a mode is a loud
    /// refusal naming what is missing, and never a fall back to running
    /// unsandboxed. Only asserts when `srt` is actually absent.
    #[cfg(unix)]
    #[test]
    fn a_missing_srt_refuses_rather_than_running_unsandboxed() {
        if srt_usable() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let err = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_mode(SandboxMode::Workspace)
            .run("printf hi")
            .unwrap_err();
        let text = err.message;
        assert!(text.contains("sandbox mode workspace"), "{text}");
        assert!(text.contains("srt"), "{text}");
    }

    /// `off` is unchanged: no `srt`, no refusal, the shell runs directly.
    #[cfg(unix)]
    #[test]
    fn off_mode_runs_the_shell_directly_even_without_srt() {
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_mode(SandboxMode::Off)
            .run("printf ok")
            .unwrap();
        assert_eq!(out.stdout, "ok");
    }

    /// A write inside the workspace succeeds under `workspace`. Skipped when
    /// `srt` is not installed.
    #[cfg(unix)]
    #[test]
    fn workspace_mode_allows_a_write_inside_the_workspace() {
        if !srt_usable() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(10))
            .with_mode(SandboxMode::Workspace)
            .run("printf data > inside.txt && cat inside.txt")
            .unwrap();
        assert_eq!(out.status, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stdout.trim(), "data");
    }

    /// Reading a credential store is refused under `workspace`, and the
    /// refusal names the rule. Skipped when `srt` is not installed.
    #[cfg(unix)]
    #[test]
    fn workspace_mode_refuses_reading_a_credential_store() {
        if !srt_usable() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let home = std::env::var("HOME").unwrap_or_default();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(10))
            .with_mode(SandboxMode::Workspace)
            .run(&format!("cat '{home}/.ssh/id_rsa'"))
            .unwrap();
        assert_ne!(out.status, Some(0), "reading ~/.ssh should fail");
    }

    /// The network is denied under `workspace`, so a curl to any host fails.
    /// Skipped when `srt` is not installed.
    #[cfg(unix)]
    #[test]
    fn workspace_mode_refuses_the_network() {
        if !srt_usable() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_timeout(Duration::from_secs(10))
            .with_mode(SandboxMode::Workspace)
            .run("curl -sS --max-time 5 https://example.com")
            .unwrap();
        assert_ne!(out.status, Some(0), "the network should be denied");
    }
}
