//! One lock per job, so a job that takes longer than its own interval
//! cannot start a second copy of itself.
//!
//! A late run skips. It does not queue. Queueing turns a job that is
//! slower than its schedule into an unbounded backlog, and then into as
//! many concurrent agents as the backlog is deep, which is how a slow
//! nightly job becomes a fork bomb. Skipping is bounded, and the skip is
//! written to the run history so that "this job never actually runs" is
//! visible rather than silent.

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockError {
    /// Another run holds the lock. `pid` is 0 when the holder record could
    /// not be read.
    Held { pid: i32, since: i64 },
    Io(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held { pid, since } => {
                write!(f, "already running (pid {pid}, since unix {since})")
            }
            LockError::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for LockError {}

/// Held for as long as the value lives. Dropping it releases the lock, and
/// a process that dies without dropping leaves a lock that the next run
/// recognizes as stale.
#[derive(Debug)]
pub struct JobLock {
    path: PathBuf,
    pid: i32,
    acquired_at: i64,
}

const LOCK_FILE: &str = "lock";

fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILE)
}

fn io(error: impl fmt::Display) -> LockError {
    LockError::Io(error.to_string())
}

/// Two lines: the holder's process id, then the instant it took the lock.
/// Deliberately not a serialized struct. This file is read by a process
/// deciding whether to start work, so the cheapest possible format that
/// cannot half-parse into something plausible is the right one.
fn read_holder(dir: &Path) -> Option<(i32, i64)> {
    let mut text = String::new();
    std::fs::File::open(lock_path(dir))
        .ok()?
        .read_to_string(&mut text)
        .ok()?;
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let acquired_at = lines.next()?.trim().parse().ok()?;
    Some((pid, acquired_at))
}

impl JobLock {
    /// Take the job's lock, or report who has it.
    ///
    /// The holder record is written to a private file first and then hard
    /// linked into place. `link` fails if the destination exists, so
    /// exactly one of two racing processes wins, and the file it wins with
    /// is already complete. That second property is what matters: a lock
    /// created with `create_new` and written afterwards is briefly visible
    /// and empty, and the loser of the race would have to guess what an
    /// empty lock meant. Here there is nothing to guess, so an unreadable
    /// lock can be treated as the corruption it is.
    ///
    /// The rest is about recognizing a lock whose owner is gone, because a
    /// lock that only a healthy process can release turns one crash into a
    /// permanently dead job.
    pub fn acquire(
        dir: &Path,
        pid: i32,
        now: i64,
        timeout_secs: i64,
        alive: &dyn Fn(i32) -> bool,
    ) -> Result<JobLock, LockError> {
        std::fs::create_dir_all(dir).map_err(io)?;
        match Self::create(dir, pid, now) {
            Ok(lock) => return Ok(lock),
            Err(LockError::Held { .. }) => {}
            Err(other) => return Err(other),
        }
        let held = match read_holder(dir) {
            Some((holder, since)) => alive(holder) && now - since <= timeout_secs,
            None => false,
        };
        if held {
            let (holder, since) = read_holder(dir).unwrap_or((0, 0));
            return Err(LockError::Held {
                pid: holder,
                since,
            });
        }
        Self::force_release(dir)?;
        Self::create(dir, pid, now)
    }

    fn create(dir: &Path, pid: i32, now: i64) -> Result<JobLock, LockError> {
        let path = lock_path(dir);
        // Named for the writing process so two processes racing here
        // cannot scribble over each other's staging file.
        let staged = dir.join(format!("lock.staged.{pid}"));
        let mut file = std::fs::File::create(&staged).map_err(io)?;
        write!(file, "{pid}\n{now}\n").map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        let linked = std::fs::hard_link(&staged, &path);
        let _ = std::fs::remove_file(&staged);
        match linked {
            Ok(()) => Ok(JobLock {
                path,
                pid,
                acquired_at: now,
            }),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let (holder, since) = read_holder(dir).unwrap_or((0, 0));
                Err(LockError::Held {
                    pid: holder,
                    since,
                })
            }
            Err(e) => Err(io(e)),
        }
    }

    /// Who holds the lock, if anyone, without trying to take it.
    pub fn holder(dir: &Path) -> Option<(i32, i64)> {
        read_holder(dir)
    }

    /// Break a lock unconditionally. The escape hatch for a lock left by a
    /// process that is somehow both gone and unrecognized as gone.
    pub fn force_release(dir: &Path) -> Result<(), LockError> {
        match std::fs::remove_file(lock_path(dir)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }

    pub fn acquired_at(&self) -> i64 {
        self.acquired_at
    }
}

impl Drop for JobLock {
    /// Release, but only if the file still records this lock.
    ///
    /// A lock broken as stale is replaced by whoever broke it. If the
    /// original holder then woke up and deleted the file on its way out,
    /// it would be deleting the new holder's lock, and the job would be
    /// unlocked while a run was in progress. Checking first makes a
    /// broken lock a one-time event rather than a lasting hole.
    fn drop(&mut self) {
        let dir = match self.path.parent() {
            Some(dir) => dir,
            None => return,
        };
        if read_holder(dir) == Some((self.pid, self.acquired_at)) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Whether a process is still around.
///
/// Signal 0 is the standard "check, do not deliver" probe: it runs the
/// permission and existence checks and sends nothing. `EPERM` means the
/// process exists but belongs to someone else, which still counts as
/// alive. Pid 0 means the caller's own process group to `kill`, never a
/// recorded holder, so it is refused before it can be probed.
#[allow(unsafe_code)]
pub fn process_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn all_alive(_pid: i32) -> bool {
        true
    }

    fn none_alive(_pid: i32) -> bool {
        false
    }

    const HOUR: i64 = 3600;

    #[test]
    fn a_free_lock_is_acquired_and_records_its_holder() {
        let d = dir();
        let lock = JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap();
        assert_eq!(lock.acquired_at(), 1000);
        assert_eq!(JobLock::holder(d.path()), Some((4242, 1000)));
    }

    /// The whole point. A daily job that takes twenty-six hours must not
    /// start a second copy of itself on the second day.
    #[test]
    fn a_lock_held_by_a_live_process_refuses_a_second_run() {
        let d = dir();
        let _held = JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap();
        let second = JobLock::acquire(d.path(), 5151, 1500, HOUR, &all_alive);
        assert_eq!(
            second.unwrap_err(),
            LockError::Held {
                pid: 4242,
                since: 1000
            }
        );
    }

    #[test]
    fn releasing_lets_the_next_run_in() {
        let d = dir();
        let held = JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap();
        drop(held);
        assert_eq!(JobLock::holder(d.path()), None);
        assert!(JobLock::acquire(d.path(), 5151, 1500, HOUR, &all_alive).is_ok());
    }

    /// A machine that lost power mid-run leaves a lock file behind. If
    /// that wedged the job forever, the failure would be permanent and
    /// silent, which is worse than the overlap the lock exists to stop.
    #[test]
    fn a_lock_left_by_a_dead_process_is_broken() {
        let d = dir();
        std::mem::forget(JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap());
        let taken = JobLock::acquire(d.path(), 5151, 1500, HOUR, &none_alive).unwrap();
        assert_eq!(taken.acquired_at(), 1500);
        assert_eq!(JobLock::holder(d.path()), Some((5151, 1500)));
    }

    /// A process that is alive but has held the lock past the job's
    /// timeout is treated as wedged. The timeout is the job's own, so a
    /// legitimately long run raises it rather than being killed by a
    /// default someone else chose.
    #[test]
    fn a_lock_held_past_its_timeout_is_broken() {
        let d = dir();
        std::mem::forget(JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap());
        let within = JobLock::acquire(d.path(), 5151, 1000 + HOUR, HOUR, &all_alive);
        assert!(within.is_err(), "exactly at the timeout is still held");
        let past = JobLock::acquire(d.path(), 5151, 1000 + HOUR + 1, HOUR, &all_alive).unwrap();
        assert_eq!(past.acquired_at(), 1000 + HOUR + 1);
    }

    /// A lock is linked into place with its contents already written, so
    /// a lock that exists is always readable. One that is not readable is
    /// corruption, and corruption must not wedge a job forever.
    #[test]
    fn an_unreadable_lock_is_corruption_and_is_broken() {
        let d = dir();
        std::fs::create_dir_all(d.path()).unwrap();
        std::fs::write(d.path().join("lock"), b"garbage-not-a-lock").unwrap();
        let taken = JobLock::acquire(d.path(), 5151, 1000, HOUR, &all_alive).unwrap();
        assert_eq!(taken.acquired_at(), 1000);
        assert_eq!(JobLock::holder(d.path()), Some((5151, 1000)));
    }

    /// The staging file the holder record is written to must not survive
    /// the acquisition, in either outcome. A leftover would accumulate one
    /// per run forever.
    #[test]
    fn acquiring_leaves_no_staging_files_behind() {
        let d = dir();
        let first = JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap();
        assert!(JobLock::acquire(d.path(), 5151, 1100, HOUR, &all_alive).is_err());
        drop(first);
        let left: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(left.is_empty(), "left behind {left:?}");
    }

    /// If run A's lock is broken as stale and run B takes it, A must not
    /// delete B's lock on the way out. Otherwise breaking one stale lock
    /// quietly disables the lock for the run that replaced it.
    #[test]
    fn releasing_never_removes_a_lock_someone_else_now_holds() {
        let d = dir();
        let first = JobLock::acquire(d.path(), 4242, 1000, HOUR, &all_alive).unwrap();
        JobLock::force_release(d.path()).unwrap();
        let second = JobLock::acquire(d.path(), 5151, 1500, HOUR, &all_alive).unwrap();
        drop(first);
        assert_eq!(
            JobLock::holder(d.path()),
            Some((5151, 1500)),
            "the first lock's drop must not have removed the second lock"
        );
        drop(second);
        assert_eq!(JobLock::holder(d.path()), None);
    }

    #[test]
    fn acquiring_creates_the_state_directory() {
        let d = dir();
        let nested = d.path().join("jobs").join("nightly");
        assert!(!nested.exists());
        let _lock = JobLock::acquire(&nested, 1, 1000, HOUR, &all_alive).unwrap();
        assert!(nested.join("lock").exists());
    }

    #[test]
    fn force_release_on_a_free_lock_is_not_an_error() {
        let d = dir();
        assert!(JobLock::force_release(d.path()).is_ok());
    }

    /// The real liveness check, exercised only enough to know it is wired
    /// to something. Every test above injects its own.
    #[test]
    fn the_real_liveness_check_knows_about_this_process() {
        assert!(process_is_alive(std::process::id() as i32));
        // Pid 0 is "the current process group" to kill(2), never a
        // process this crate could have recorded as a holder.
        assert!(!process_is_alive(0));
    }
}
