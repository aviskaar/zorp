//! What happened, kept where someone can look at it.
//!
//! A scheduled job that fails silently every night is worse than no job at
//! all, because it looks like it is working. Every run appends a record
//! here, including the runs that did not happen and why, and the CLI reads
//! it back. Consecutive failures are counted so that "this has been broken
//! for nine nights" is a number rather than something to notice by eye.
//!
//! One append-only JSON-lines file per job. No database: this is written
//! once per run by one process holding that job's lock, and read by a
//! human. Anything more would be machinery around a log.

use crate::civil::from_unix_utc;
use crate::BoxErr;
use std::path::{Path, PathBuf};

/// How a run ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    /// The agent finished and produced an answer.
    Completed,
    /// The agent ran and did not finish: a model error, a step limit, a
    /// failed verification.
    Failed,
    /// The agent stopped because the approval model refused a tool it
    /// wanted. Distinct from `Failed` because the fix is different: the
    /// job is asking for more than a scheduled run is allowed.
    Blocked,
    /// The run never started because a precondition was not met, such as
    /// a missing working directory or an unset required variable.
    Refused,
    /// The previous run of this job was still going.
    SkippedOverlap,
    /// The occurrence was dropped by the job's missed-run policy.
    SkippedMissed,
}

impl RunOutcome {
    pub fn name(&self) -> &'static str {
        match self {
            RunOutcome::Completed => "completed",
            RunOutcome::Failed => "failed",
            RunOutcome::Blocked => "blocked",
            RunOutcome::Refused => "refused",
            RunOutcome::SkippedOverlap => "skipped-overlap",
            RunOutcome::SkippedMissed => "skipped-missed",
        }
    }

    pub fn parse(name: &str) -> Option<RunOutcome> {
        [
            RunOutcome::Completed,
            RunOutcome::Failed,
            RunOutcome::Blocked,
            RunOutcome::Refused,
            RunOutcome::SkippedOverlap,
            RunOutcome::SkippedMissed,
        ]
        .into_iter()
        .find(|outcome| outcome.name() == name)
    }

    /// Whether this outcome should count towards "this job is broken".
    ///
    /// An overlap skip counts. A job whose runs always collide is a job
    /// that never produces anything, which is the silent failure this
    /// module exists to surface. A missed-run skip does not count: that is
    /// the policy the user asked for, working.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            RunOutcome::Failed
                | RunOutcome::Blocked
                | RunOutcome::Refused
                | RunOutcome::SkippedOverlap
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRecord {
    pub run_id: String,
    pub job: String,
    pub scheduled_for: i64,
    pub started_at: i64,
    pub finished_at: i64,
    pub outcome: RunOutcome,
    pub detail: String,
    /// Where the agent's answer was written, when there was one.
    pub answer: Option<String>,
}

/// How many records a trim keeps. Not a cap on what is present: the file
/// is bounded in bytes by `TRIM_ABOVE_BYTES` and cut back to this many
/// records whenever it crosses that line. A daily job takes years to
/// reach either bound, and a one-minute job a few days, by which point
/// the oldest records are long past being useful.
pub const RECORDS_KEPT_ON_TRIM: usize = 1000;

pub struct History {
    dir: PathBuf,
}

const HISTORY_FILE: &str = "history.jsonl";
const WATERMARK_FILE: &str = "watermark";
const ANSWERS_DIR: &str = "answers";

/// Trim when the file passes this. Checked by file size rather than by
/// counting records, so an ordinary append stays a single write and does
/// not read the file back. The file therefore oscillates between
/// `RECORDS_KEPT_ON_TRIM` records and this size, bounded either way.
const TRIM_ABOVE_BYTES: u64 = 512 * 1024;

/// Whether the file's last byte is something other than a newline, which
/// means the last line was never finished.
fn ends_mid_line(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::End(-1)).is_err() {
        return false;
    }
    let mut last = [0u8; 1];
    file.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

fn to_json(record: &RunRecord) -> String {
    // Built through serde_json rather than by hand: `detail` carries model
    // output and error text, which contains quotes and newlines, and a
    // hand-rolled writer is how one bad detail eats the rest of the file.
    serde_json::json!({
        "run_id": record.run_id,
        "job": record.job,
        "scheduled_for": record.scheduled_for,
        "started_at": record.started_at,
        "finished_at": record.finished_at,
        "outcome": record.outcome.name(),
        "detail": record.detail,
        "answer": record.answer,
    })
    .to_string()
}

/// Parse one line, returning `None` for anything unreadable. A half
/// written final line is what a machine losing power mid-append leaves,
/// and it must not hide the records before it.
fn from_json(line: &str) -> Option<RunRecord> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(RunRecord {
        run_id: value.get("run_id")?.as_str()?.to_string(),
        job: value.get("job")?.as_str()?.to_string(),
        scheduled_for: value.get("scheduled_for")?.as_i64()?,
        started_at: value.get("started_at")?.as_i64()?,
        finished_at: value.get("finished_at")?.as_i64()?,
        outcome: RunOutcome::parse(value.get("outcome")?.as_str()?)?,
        detail: value.get("detail")?.as_str()?.to_string(),
        answer: value
            .get("answer")
            .and_then(|a| a.as_str())
            .map(str::to_string),
    })
}

impl History {
    /// Open (creating) the state directory for one job.
    pub fn open(state_root: &Path, job: &str) -> Result<History, BoxErr> {
        let dir = state_root.join(job);
        std::fs::create_dir_all(&dir)?;
        Ok(History { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn append(&self, record: &RunRecord) -> Result<(), BoxErr> {
        use std::io::Write;
        let path = self.dir.join(HISTORY_FILE);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        // A machine that lost power mid-append leaves a line with no
        // newline on it. Appending straight onto that fuses the damaged
        // line to this one and loses both, so one crash would cost two
        // records instead of one.
        if ends_mid_line(&path) {
            writeln!(file)?;
        }
        writeln!(file, "{}", to_json(record))?;
        drop(file);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > TRIM_ABOVE_BYTES {
            self.trim()?;
        }
        Ok(())
    }

    /// Rewrite the file keeping only the newest `RECORDS_KEPT_ON_TRIM`
    /// records. Rare by construction: appends are cheap and this happens
    /// once every few thousand of them.
    fn trim(&self) -> Result<(), BoxErr> {
        let kept = self.recent(RECORDS_KEPT_ON_TRIM);
        let mut text = String::new();
        for entry in &kept {
            text.push_str(&to_json(entry));
            text.push('\n');
        }
        std::fs::write(self.dir.join(HISTORY_FILE), text)?;
        Ok(())
    }

    fn all(&self) -> Vec<RunRecord> {
        let Ok(text) = std::fs::read_to_string(self.dir.join(HISTORY_FILE)) else {
            return Vec::new();
        };
        text.lines().filter_map(from_json).collect()
    }

    /// The newest `limit` records, oldest first.
    pub fn recent(&self, limit: usize) -> Vec<RunRecord> {
        let mut all = self.all();
        if all.len() > limit {
            all.drain(..all.len() - limit);
        }
        all
    }

    pub fn last(&self) -> Option<RunRecord> {
        self.all().pop()
    }

    /// How many runs in a row have gone wrong, counting back from the
    /// most recent.
    pub fn consecutive_failures(&self) -> u32 {
        self.all()
            .iter()
            .rev()
            .take_while(|record| record.outcome.is_failure())
            .count() as u32
    }

    pub fn watermark(&self) -> Option<i64> {
        std::fs::read_to_string(self.dir.join(WATERMARK_FILE))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    pub fn set_watermark(&self, unix: i64) -> Result<(), BoxErr> {
        Ok(std::fs::write(
            self.dir.join(WATERMARK_FILE),
            format!("{unix}\n"),
        )?)
    }

    /// Write a run's answer where its record can point at it. This is the
    /// path a `read-only` job's output takes: zorp writes the file, the
    /// agent never gets a write tool to do it with.
    pub fn write_answer(&self, run_id: &str, text: &str) -> Result<PathBuf, BoxErr> {
        let dir = self.dir.join(ANSWERS_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{run_id}.md"));
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

/// Where scheduled-job state lives. Under the XDG state directory rather
/// than the config directory, because this is written by the machine and
/// nobody would want it in the file they keep in version control.
pub fn state_root(home: &Path) -> PathBuf {
    home.join(".local")
        .join("state")
        .join("zorp")
        .join("schedule")
}

/// A readable, stable identifier for one occurrence of one job. Derived
/// from the scheduled instant in UTC, so the same occurrence always gets
/// the same id and a rerun overwrites rather than accumulates.
pub fn run_id(scheduled_for: i64) -> String {
    let c = from_unix_utc(scheduled_for);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(outcome: RunOutcome, at: i64) -> RunRecord {
        RunRecord {
            run_id: run_id(at),
            job: "nightly".into(),
            scheduled_for: at,
            started_at: at,
            finished_at: at + 5,
            outcome,
            detail: String::new(),
            answer: None,
        }
    }

    fn history() -> (tempfile::TempDir, History) {
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(dir.path(), "nightly").unwrap();
        (dir, history)
    }

    #[test]
    fn records_round_trip_through_the_history_file() {
        let (_d, h) = history();
        let mut first = record(RunOutcome::Completed, 1000);
        first.detail = "answered in 4 steps".into();
        first.answer = Some("/state/nightly/answers/x.md".into());
        h.append(&first).unwrap();
        h.append(&record(RunOutcome::Failed, 2000)).unwrap();
        let all = h.recent(10);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], first);
        assert_eq!(all[1].outcome, RunOutcome::Failed);
        assert_eq!(h.last().unwrap().scheduled_for, 2000);
    }

    /// Details come from model output and error strings, so they contain
    /// quotes, newlines and worse. A history file that a bad detail can
    /// corrupt is a history file that stops recording exactly when things
    /// are going wrong.
    #[test]
    fn a_detail_full_of_punctuation_does_not_corrupt_the_file() {
        let (_d, h) = history();
        let mut nasty = record(RunOutcome::Failed, 1000);
        nasty.detail = "line one\nline \"two\"\ttab \\ backslash {\"json\": true}".into();
        h.append(&nasty).unwrap();
        h.append(&record(RunOutcome::Completed, 2000)).unwrap();
        let all = h.recent(10);
        assert_eq!(all.len(), 2, "the nasty detail swallowed a record");
        assert_eq!(all[0].detail, nasty.detail);
    }

    #[test]
    fn a_truncated_last_line_does_not_hide_the_records_before_it() {
        let (_d, h) = history();
        h.append(&record(RunOutcome::Completed, 1000)).unwrap();
        let path = h.dir().join("history.jsonl");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{\"run_id\": \"half-writ");
        std::fs::write(&path, text).unwrap();
        assert_eq!(h.recent(10).len(), 1);
        // And appending after the damage still works.
        h.append(&record(RunOutcome::Failed, 2000)).unwrap();
        assert_eq!(h.last().unwrap().outcome, RunOutcome::Failed);
    }

    #[test]
    fn recent_returns_the_newest_records_up_to_the_limit() {
        let (_d, h) = history();
        for n in 0..10 {
            h.append(&record(RunOutcome::Completed, 1000 + n)).unwrap();
        }
        let recent = h.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].scheduled_for, 1007);
        assert_eq!(recent[2].scheduled_for, 1009);
    }

    /// A job that runs every minute would otherwise grow an unbounded
    /// file on a machine nobody is watching. The file is allowed to grow
    /// to the trim threshold and is then cut back, so what is asserted is
    /// that it is bounded and that trimming keeps the newest records.
    #[test]
    fn the_history_file_does_not_grow_without_bound() {
        let (_d, h) = history();
        let mut trimmed_at_least_once = false;
        let last = 6_000i64;
        for n in 0..=last {
            h.append(&record(RunOutcome::Completed, 1000 + n)).unwrap();
            let size = std::fs::metadata(h.dir().join("history.jsonl"))
                .unwrap()
                .len();
            assert!(size <= TRIM_ABOVE_BYTES + 1024, "grew to {size} bytes");
            trimmed_at_least_once |= size < TRIM_ABOVE_BYTES / 2 && n > 100;
        }
        assert!(trimmed_at_least_once, "the file was never trimmed");
        assert_eq!(
            h.last().unwrap().scheduled_for,
            1000 + last,
            "trimming must drop the oldest, never the newest"
        );
    }

    /// The number that makes a job's breakage noticeable without anyone
    /// going looking for it.
    #[test]
    fn consecutive_failures_counts_back_to_the_last_success() {
        let (_d, h) = history();
        assert_eq!(h.consecutive_failures(), 0);
        h.append(&record(RunOutcome::Failed, 1000)).unwrap();
        h.append(&record(RunOutcome::Failed, 2000)).unwrap();
        assert_eq!(h.consecutive_failures(), 2);
        h.append(&record(RunOutcome::Completed, 3000)).unwrap();
        assert_eq!(h.consecutive_failures(), 0);
        h.append(&record(RunOutcome::Blocked, 4000)).unwrap();
        h.append(&record(RunOutcome::Refused, 5000)).unwrap();
        h.append(&record(RunOutcome::SkippedOverlap, 6000)).unwrap();
        assert_eq!(
            h.consecutive_failures(),
            3,
            "blocked, refused and overlap all mean the job is not working"
        );
    }

    /// A deliberately dropped occurrence is the policy working, not the
    /// job failing, so it must not raise the alarm on its own.
    #[test]
    fn a_dropped_occurrence_does_not_count_as_a_failure() {
        let (_d, h) = history();
        h.append(&record(RunOutcome::Completed, 1000)).unwrap();
        h.append(&record(RunOutcome::SkippedMissed, 2000)).unwrap();
        h.append(&record(RunOutcome::SkippedMissed, 3000)).unwrap();
        assert_eq!(h.consecutive_failures(), 0);
        assert!(!RunOutcome::SkippedMissed.is_failure());
        assert!(RunOutcome::SkippedOverlap.is_failure());
    }

    #[test]
    fn the_watermark_persists_and_starts_unset() {
        let (dir, h) = history();
        assert_eq!(h.watermark(), None);
        h.set_watermark(1234).unwrap();
        assert_eq!(h.watermark(), Some(1234));
        let reopened = History::open(dir.path(), "nightly").unwrap();
        assert_eq!(reopened.watermark(), Some(1234));
        h.set_watermark(5678).unwrap();
        assert_eq!(h.watermark(), Some(5678));
    }

    #[test]
    fn a_corrupt_watermark_reads_as_unset_rather_than_as_zero() {
        let (_d, h) = history();
        std::fs::write(h.dir().join("watermark"), "not-a-number").unwrap();
        assert_eq!(
            h.watermark(),
            None,
            "reading zero here would replay every occurrence since 1970"
        );
    }

    #[test]
    fn answers_are_written_where_the_record_can_point_at_them() {
        let (_d, h) = history();
        let path = h.write_answer("20260818T030000Z", "# Weekly review\n").unwrap();
        assert!(path.is_absolute() || path.starts_with(h.dir()));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# Weekly review\n"
        );
    }

    #[test]
    fn two_jobs_keep_separate_state() {
        let dir = tempfile::tempdir().unwrap();
        let a = History::open(dir.path(), "alpha").unwrap();
        let b = History::open(dir.path(), "beta").unwrap();
        a.set_watermark(111).unwrap();
        b.set_watermark(222).unwrap();
        a.append(&record(RunOutcome::Failed, 1000)).unwrap();
        assert_eq!(a.watermark(), Some(111));
        assert_eq!(b.watermark(), Some(222));
        assert_eq!(b.consecutive_failures(), 0);
    }

    #[test]
    fn run_ids_are_stable_readable_and_unique_per_occurrence() {
        // 2026-08-18 03:00:00 UTC.
        let at = crate::civil::to_unix_utc(&crate::Civil::new(2026, 8, 18, 3, 0));
        assert_eq!(run_id(at), "20260818T0300Z");
        assert_eq!(run_id(at), run_id(at));
        assert_ne!(run_id(at), run_id(at + 60));
    }

    #[test]
    fn outcome_names_round_trip() {
        for outcome in [
            RunOutcome::Completed,
            RunOutcome::Failed,
            RunOutcome::Blocked,
            RunOutcome::Refused,
            RunOutcome::SkippedOverlap,
            RunOutcome::SkippedMissed,
        ] {
            assert_eq!(RunOutcome::parse(outcome.name()), Some(outcome));
        }
        assert_eq!(RunOutcome::parse("invented"), None);
    }

    #[test]
    fn state_lives_under_the_users_state_directory() {
        assert_eq!(
            state_root(Path::new("/home/u")),
            PathBuf::from("/home/u/.local/state/zorp/schedule")
        );
    }
}
