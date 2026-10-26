// A shell and everything it starts, held as one unit so a timeout, a
// cancel or a finished command can take all of it down. On Unix that is a
// process group the child leads (setpgid in pre_exec, killpg to end it).
// On Windows it is a job object the child is assigned to right after it
// is spawned, and TerminateJobObject ends every process in it.
//
// The Windows path has one gap Unix does not: the child runs for a moment
// before it is assigned, so a grandchild started in that window would sit
// outside the job. Git Bash takes far longer than that to start anything,
// and the alternative, spawning suspended and resuming the main thread,
// needs a thread handle std does not hand back.

use std::io;
use std::path::PathBuf;
use std::process::{Child, Command};

pub(crate) struct ProcessGroup {
    #[cfg(unix)]
    pgid: i32,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}

impl ProcessGroup {
    /// Set up `cmd` so the child it spawns can be adopted as a group.
    pub(crate) fn prepare(cmd: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setpgid(0, 0) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        let _ = cmd;
    }

    /// The group led by `child`, which was spawned from a prepared command.
    pub(crate) fn adopt(child: &Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                pgid: child.id() as i32,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW,
            };
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
            if unsafe {
                AssignProcessToJobObject(job.as_raw_handle() as _, child.as_raw_handle() as _)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { job })
        }
    }

    /// Kill every process still in the group. A group with nothing left in
    /// it is not an error.
    pub(crate) fn kill(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            super::kill_process_group(self.pgid)
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            if unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

/// The shell every command runs under. Unix has `/bin/sh`. Windows has no
/// POSIX shell of its own, so this is Git for Windows' `sh.exe`, which keeps
/// the command language the same on every platform, and with it every rule
/// the `run_command` denylist reads out of a command line.
pub(crate) fn shell() -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        Ok(PathBuf::from("/bin/sh"))
    }
    #[cfg(windows)]
    {
        windows_shell(
            std::env::var_os("ZORP_SH"),
            std::env::var_os("PATH"),
            &[
                std::env::var_os("ProgramFiles"),
                std::env::var_os("ProgramW6432"),
                std::env::var_os("ProgramFiles(x86)"),
            ],
            std::env::var_os("LOCALAPPDATA"),
        )
    }
}

/// Where Git Bash is, in this order: `ZORP_SH` when it is set, an `sh.exe`
/// on `PATH`, the `bin\sh.exe` beside a `git.exe` on `PATH`, then the
/// places the Git for Windows installer puts it. Never WSL's `bash.exe`,
/// which runs a different machine's filesystem.
#[cfg(any(windows, test))]
fn windows_shell(
    explicit: Option<std::ffi::OsString>,
    path: Option<std::ffi::OsString>,
    program_files: &[Option<std::ffi::OsString>],
    local_app_data: Option<std::ffi::OsString>,
) -> io::Result<PathBuf> {
    if let Some(explicit) = explicit.filter(|v| !v.is_empty()) {
        let explicit = PathBuf::from(explicit);
        return if explicit.is_file() {
            Ok(explicit)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("ZORP_SH is {}, which is not a file", explicit.display()),
            ))
        };
    }
    let mut candidates = Vec::new();
    for dir in path
        .as_deref()
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        candidates.push(dir.join("sh.exe"));
        if dir.join("git.exe").is_file() {
            if let Some(root) = dir.parent() {
                candidates.push(root.join("bin").join("sh.exe"));
            }
        }
    }
    for root in program_files.iter().flatten() {
        candidates.push(PathBuf::from(root).join("Git").join("bin").join("sh.exe"));
    }
    if let Some(local) = local_app_data {
        candidates.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("sh.exe"),
        );
    }
    candidates.into_iter().find(|c| c.is_file()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no POSIX shell found. zorp runs commands under Git Bash on Windows: \
             install Git for Windows (https://git-scm.com/download/win), or set \
             ZORP_SH to its bin\\sh.exe",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn an_explicit_shell_wins_and_a_missing_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let sh = dir.path().join("custom").join("sh.exe");
        touch(&sh);
        let found = windows_shell(Some(sh.clone().into()), None, &[], None).unwrap();
        assert_eq!(found, sh);

        let missing = dir.path().join("nowhere.exe");
        let err = windows_shell(Some(missing.into()), None, &[], None).unwrap_err();
        assert!(err.to_string().contains("ZORP_SH"));
    }

    #[test]
    fn git_on_path_finds_the_bash_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Git").join("cmd").join("git.exe"));
        let sh = dir.path().join("Git").join("bin").join("sh.exe");
        touch(&sh);
        let path = std::env::join_paths([dir.path().join("Git").join("cmd")]).unwrap();
        assert_eq!(windows_shell(None, Some(path), &[], None).unwrap(), sh);
    }

    #[test]
    fn the_installer_location_is_the_fallback_and_nothing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let sh = dir.path().join("Git").join("bin").join("sh.exe");
        touch(&sh);
        let root = Some(dir.path().as_os_str().to_owned());
        assert_eq!(windows_shell(None, None, &[None, root], None).unwrap(), sh);

        let empty = tempfile::tempdir().unwrap();
        let err = windows_shell(
            None,
            None,
            &[Some(empty.path().as_os_str().to_owned())],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Git for Windows"));
    }
}
