//! What to run right now, given when the job last ran and what time it is.
//!
//! This is the whole of the missed-run policy, and it is pure: a watermark
//! in, a list of instants out. The laptop was asleep at 03:00 for six
//! nights, and this decides whether that means six runs, one run, or none.

use crate::clock::TimeZone;
use crate::job::{Job, OnMissed};

/// The decision for one job at one moment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuePlan {
    /// The scheduled occurrences to run now, oldest first. Empty means
    /// nothing is due.
    pub run: Vec<i64>,
    /// Occurrences that went by and are deliberately not being run. Kept
    /// so the outcome can be recorded rather than silently dropped: a
    /// missed run nobody was told about is the failure mode this whole
    /// module exists to avoid.
    pub dropped: Vec<i64>,
    /// The watermark to store once the plan has been carried out.
    pub watermark: i64,
}

impl DuePlan {
    pub fn is_empty(&self) -> bool {
        self.run.is_empty() && self.dropped.is_empty()
    }
}

/// Hard ceiling on how many occurrences are enumerated in one pass. Only
/// reachable by a watermark from the distant past, which `initial_watermark`
/// prevents in normal use. It exists so that a corrupted state file cannot
/// turn `run-due` into an unbounded loop.
const ENUMERATION_CEILING: usize = 100_000;

/// Work out what is due for `job` at `now`, given the watermark left by
/// the previous invocation.
///
/// Occurrences are enumerated through `Schedule::next_after`, so catch-up
/// and ordinary firing cannot disagree about daylight saving: there is one
/// implementation of "when does this fire", not two.
pub fn plan(job: &Job, zone: &dyn TimeZone, watermark: i64, now: i64) -> DuePlan {
    let idle = DuePlan {
        run: Vec::new(),
        dropped: Vec::new(),
        watermark,
    };
    if !job.enabled {
        return idle;
    }
    let mut outstanding: Vec<i64> = Vec::new();
    let mut cursor = watermark;
    while let Some(next) = job.schedule.next_after(zone, cursor) {
        if next > now || outstanding.len() >= ENUMERATION_CEILING {
            break;
        }
        outstanding.push(next);
        cursor = next;
    }
    let Some(newest) = outstanding.last().copied() else {
        return idle;
    };
    // Whatever happens below, the watermark advances past everything that
    // has come due. Leaving it behind would re-report the same backlog on
    // the next tick, and a dropped occurrence would be dropped again and
    // again in the history.
    let watermark = newest;
    let split_at = |keep: usize| {
        let boundary = outstanding.len().saturating_sub(keep);
        (
            outstanding[boundary..].to_vec(),
            outstanding[..boundary].to_vec(),
        )
    };
    let (run, dropped) = match job.on_missed {
        // Newest first in usefulness: a re-review of today's draft beats
        // five re-reviews of the drafts it already superseded.
        OnMissed::RunOnce => split_at(1),
        OnMissed::RunAll => split_at(job.max_catchup.max(1) as usize),
        OnMissed::Skip if now - newest <= job.stale_after_secs => split_at(1),
        OnMissed::Skip => (Vec::new(), outstanding),
    };
    DuePlan {
        run,
        dropped,
        watermark,
    }
}

/// The watermark to record when a job is seen for the very first time.
///
/// Starting from `now` means a newly written job waits for its next
/// occurrence instead of firing immediately. Starting from the epoch would
/// make every new job think it had missed every run since 1970.
pub fn initial_watermark(now: i64) -> i64 {
    now
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::{to_unix_utc, Civil};
    use crate::clock::{local_civil, FixedOffsetZone};
    use crate::job::{JobFile, JobPreset, OnMissed};
    use crate::schedule::Schedule;

    fn utc() -> FixedOffsetZone {
        FixedOffsetZone(0)
    }

    fn at(day: u32, hour: u32, minute: u32) -> i64 {
        to_unix_utc(&Civil::new(2026, 8, day, hour, minute))
    }

    /// A daily 03:00 job with the given missed-run policy.
    fn nightly(on_missed: &str) -> Job {
        let text = format!(
            "[[job]]\nname=\"nightly\"\nschedule=\"daily at 03:00\"\nworkdir=\"/s\"\n\
             prompt=\"p\"\non_missed=\"{on_missed}\"\nmax_catchup=4"
        );
        JobFile::parse(&text).unwrap().jobs.pop().unwrap()
    }

    #[test]
    fn nothing_is_due_before_the_next_occurrence() {
        let job = nightly("run-once");
        // Watermark just after last night's run, and it is now lunchtime.
        let plan = plan(&job, &utc(), at(18, 3, 0), at(18, 12, 0));
        assert!(plan.is_empty());
        assert_eq!(
            plan.watermark,
            at(18, 3, 0),
            "an idle check must not move the watermark past an occurrence it did not run"
        );
    }

    #[test]
    fn a_single_due_occurrence_runs() {
        let job = nightly("run-once");
        let plan = plan(&job, &utc(), at(18, 3, 0), at(19, 3, 5));
        assert_eq!(plan.run, vec![at(19, 3, 0)]);
        assert!(plan.dropped.is_empty());
        assert_eq!(plan.watermark, at(19, 3, 0));
    }

    /// The laptop was shut for six nights. Six agents starting at once on
    /// wake is the wrong answer, and so is pretending nothing happened.
    #[test]
    fn run_once_collapses_a_backlog_to_one_run_and_records_the_rest() {
        let job = nightly("run-once");
        let plan = plan(&job, &utc(), at(12, 3, 0), at(18, 9, 0));
        assert_eq!(
            plan.run,
            vec![at(18, 3, 0)],
            "the most recent missed occurrence is the one worth running"
        );
        assert_eq!(
            plan.dropped,
            vec![at(13, 3, 0), at(14, 3, 0), at(15, 3, 0), at(16, 3, 0), at(17, 3, 0)],
            "the older ones are dropped, but they are reported, not forgotten"
        );
        assert_eq!(plan.watermark, at(18, 3, 0));
    }

    #[test]
    fn run_all_runs_every_missed_occurrence_in_order() {
        let job = nightly("run-all");
        let plan = plan(&job, &utc(), at(15, 3, 0), at(18, 9, 0));
        assert_eq!(plan.run, vec![at(16, 3, 0), at(17, 3, 0), at(18, 3, 0)]);
        assert!(plan.dropped.is_empty());
        assert_eq!(plan.watermark, at(18, 3, 0));
    }

    /// `run-all` without a ceiling is how a month-long holiday turns into
    /// a month of agent runs starting at once.
    #[test]
    fn run_all_is_bounded_by_max_catchup_and_drops_the_oldest() {
        let job = nightly("run-all");
        assert_eq!(job.max_catchup, 4);
        let plan = plan(&job, &utc(), at(10, 3, 0), at(18, 9, 0));
        assert_eq!(plan.run.len(), 4);
        assert_eq!(
            plan.run,
            vec![at(15, 3, 0), at(16, 3, 0), at(17, 3, 0), at(18, 3, 0)],
            "the ceiling keeps the newest, because stale work is the least useful"
        );
        assert_eq!(plan.dropped.len(), 4);
        assert_eq!(plan.watermark, at(18, 3, 0));
    }

    #[test]
    fn skip_runs_nothing_but_still_reports_what_went_by() {
        let job = nightly("skip");
        let plan = plan(&job, &utc(), at(15, 3, 0), at(18, 9, 0));
        assert!(plan.run.is_empty());
        assert_eq!(plan.dropped, vec![at(16, 3, 0), at(17, 3, 0), at(18, 3, 0)]);
        assert_eq!(plan.watermark, at(18, 3, 0));
    }

    /// `skip` must not mean "never run again". It skips what was missed
    /// and then behaves normally.
    #[test]
    fn skip_still_runs_the_next_occurrence_once_it_arrives() {
        let job = nightly("skip");
        let after_backlog = plan(&job, &utc(), at(15, 3, 0), at(18, 9, 0));
        let next = plan(&job, &utc(), after_backlog.watermark, at(19, 3, 1));
        assert_eq!(next.run, vec![at(19, 3, 0)]);
    }

    /// An occurrence that has not arrived yet is not missed, whatever the
    /// policy is. An off-by-one here fires every job an interval early.
    #[test]
    fn an_occurrence_in_the_future_is_never_run_early() {
        for policy in ["run-once", "run-all", "skip"] {
            let job = nightly(policy);
            let plan = plan(&job, &utc(), at(18, 3, 0), at(19, 2, 59));
            assert!(plan.is_empty(), "{policy} ran an occurrence early");
        }
    }

    #[test]
    fn an_occurrence_exactly_now_counts_as_due() {
        let job = nightly("run-once");
        let plan = plan(&job, &utc(), at(18, 3, 0), at(19, 3, 0));
        assert_eq!(plan.run, vec![at(19, 3, 0)]);
    }

    /// The backlog of an interval job is measured in elapsed time, so a
    /// machine that slept for an hour owes twelve five-minute runs and,
    /// under the default policy, runs one of them.
    #[test]
    fn an_interval_job_backlog_collapses_the_same_way() {
        let mut job = nightly("run-once");
        job.schedule = Schedule::EveryMinutes(5);
        let just_after = plan(&job, &utc(), at(18, 3, 0), at(18, 3, 6));
        assert_eq!(just_after.run, vec![at(18, 3, 5)]);
        // An hour asleep owes twelve runs. One of them happens.
        let asleep = plan(&job, &utc(), at(18, 3, 0), at(18, 4, 0));
        assert_eq!(asleep.run, vec![at(18, 4, 0)]);
        assert_eq!(asleep.dropped.len(), 11);
    }

    /// The catch-up path has to agree with the schedule about daylight
    /// saving, not reimplement it. A machine off across the spring-forward
    /// weekend owes exactly one 02:30 run, at 03:00 local.
    #[test]
    fn catch_up_across_a_dst_transition_agrees_with_the_schedule() {
        use crate::clock::TransitionZone;
        let zone = TransitionZone::new(
            -5 * 3600,
            vec![(to_unix_utc(&Civil::new(2026, 3, 8, 7, 0)), -4 * 3600)],
        );
        let mut job = nightly("run-all");
        job.schedule = Schedule::Daily { minute_of_day: 150 };
        let last = to_unix_utc(&Civil::new(2026, 3, 7, 7, 30));
        let now = to_unix_utc(&Civil::new(2026, 3, 9, 12, 0));
        let plan = plan(&job, &zone, last, now);
        let readings: Vec<String> = plan
            .run
            .iter()
            .map(|u| local_civil(&zone, *u).to_string())
            .collect();
        assert_eq!(readings, vec!["2026-03-08 03:00", "2026-03-09 02:30"]);
    }

    #[test]
    fn a_disabled_job_is_never_due() {
        let mut job = nightly("run-all");
        job.enabled = false;
        assert!(plan(&job, &utc(), at(10, 3, 0), at(18, 9, 0)).is_empty());
    }

    #[test]
    fn a_new_job_waits_for_its_next_occurrence_rather_than_firing_at_once() {
        let job = nightly("run-once");
        let now = at(18, 12, 0);
        let immediately = plan(&job, &utc(), initial_watermark(now), now);
        assert!(
            immediately.is_empty(),
            "a job written at lunchtime must not immediately run last night's occurrence"
        );
        let tomorrow = plan(&job, &utc(), initial_watermark(now), at(19, 3, 1));
        assert_eq!(tomorrow.run.len(), 1);
    }

    #[test]
    fn the_defaults_a_job_gets_are_the_narrow_ones() {
        let job = JobFile::parse(
            "[[job]]\nname=\"d\"\nschedule=\"daily at 03:00\"\nworkdir=\"/s\"\nprompt=\"p\"",
        )
        .unwrap()
        .jobs
        .pop()
        .unwrap();
        assert_eq!(job.on_missed, OnMissed::RunOnce);
        assert_eq!(job.preset, JobPreset::ReadOnly);
    }
}
