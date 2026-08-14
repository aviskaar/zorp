#[cfg(not(unix))]
compile_error!("zorp-agent M4 requires a Unix target");

mod capture;

use crate::tools::ToolError;
use capture::{capture_stream, render_capture};
use std::collections::HashSet;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
    /// Secret snapshot, taken lazily on the first `run` and reused for the
    /// sandbox's lifetime. Per instance rather than process-wide so tests
    /// that mutate the environment before building a sandbox stay honest.
    secrets: OnceLock<Arc<Vec<Vec<u8>>>>,
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
        let repo_root = repo_root.canonicalize().unwrap_or(repo_root);
        Self {
            repo_root,
            cancel,
            timeout: Duration::from_secs(120),
            output_cap: 32 * 1024,
            reader_finalize_delay: Duration::ZERO,
            secrets: OnceLock::new(),
        }
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
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&self.repo_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::new(format!("spawn: {e}")))?;
        let pgid = child.id() as i32;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::new("stdout pipe unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolError::new("stderr pipe unavailable"))?;
        let stdout_probe = duplicate_fd(stdout.as_raw_fd())?;
        let stderr_probe = duplicate_fd(stderr.as_raw_fd())?;
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
            if child_exited_unreaped(pgid)? {
                let pipes_closed = stream_closed(&stdout_eof, &stdout_probe)?
                    && stream_closed(&stderr_eof, &stderr_probe)?;
                let kill_result = if pipes_closed {
                    Ok(())
                } else {
                    kill_process_group(pgid)
                };
                let status = child
                    .wait()
                    .map_err(|e| ToolError::new(format!("reap: {e}")))?;
                kill_result.map_err(|e| ToolError::new(format!("kill process group: {e}")))?;
                break (status.code(), false, false);
            }
            let cancelled = self.cancel.load(Ordering::SeqCst);
            let timed_out = started.elapsed() >= self.timeout;
            if cancelled || timed_out {
                let kill_result = kill_process_group(pgid);
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
        Ok(CommandOutput {
            status,
            stdout: render_capture(&stdout, self.output_cap),
            stderr: render_capture(&stderr, self.output_cap),
            timed_out,
            cancelled,
        })
    }
}

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

fn kill_process_group(pgid: i32) -> io::Result<()> {
    if unsafe { libc::kill(-pgid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

fn secret_values() -> Vec<Vec<u8>> {
    let mut seen = HashSet::new();
    let mut values = Vec::new();
    for (name, value) in std::env::vars_os() {
        let name = name.as_os_str().as_bytes();
        let value = value.as_os_str().as_bytes().to_vec();
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
    use std::os::unix::ffi::OsStringExt;
    use std::sync::Mutex;
    use std::thread;

    // Environment mutation is process-global, so every test that changes it
    // holds this lock for the mutation's full lifetime.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn caps_and_redacts_output() {
        let _env_lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("ZORP_TEST_SECRET_TOKEN", "m4-secret-value");
        let out = Sandbox::new(dir.path().to_path_buf(), cancel_token())
            .with_output_cap(64)
            .run("printf '%s' \"$ZORP_TEST_SECRET_TOKEN\"; yes x | head -c 256")
            .unwrap();
        assert!(!out.stdout.contains("m4-secret-value"));
        assert!(out.stdout.contains("[REDACTED]"));
        assert!(out.stdout.contains("truncated"));
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
}
