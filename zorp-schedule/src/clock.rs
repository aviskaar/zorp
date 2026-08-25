//! The two things schedule arithmetic needs from the outside world: what
//! time it is, and what the local UTC offset was at a given instant. Both
//! are traits so that every test in this crate pins time instead of waiting
//! for it. A test suite that took a week to prove a weekly job fires would
//! not be a test suite.
//!
//! Only `SystemClock` and `SystemZone` touch the operating system, and they
//! have no logic in them worth testing beyond "does the call work".

use crate::civil::{from_unix_utc, to_unix_utc, Civil};

/// Reads the current instant as Unix seconds.
pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
}

/// Answers what the local UTC offset was at a given instant, in seconds
/// east of UTC. This is the only shape of time-zone knowledge the crate
/// needs: given an instant, what did the wall clock read.
pub trait TimeZone: Send + Sync {
    fn offset_at(&self, unix: i64) -> i32;
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            // A clock set before 1970 is a broken machine, not a case worth
            // a distinct code path. Treating it as the epoch means the
            // schedule is wrong rather than the process is dead.
            .unwrap_or(0)
    }
}

/// A clock pinned to one instant, for tests.
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now(&self) -> i64 {
        self.0
    }
}

/// A zone at a constant offset, for tests that are not about daylight
/// saving. UTC is `FixedOffsetZone(0)`.
pub struct FixedOffsetZone(pub i32);

impl TimeZone for FixedOffsetZone {
    fn offset_at(&self, _unix: i64) -> i32 {
        self.0
    }
}

/// The machine's local zone, read from the operating system.
///
/// `localtime_r` is the portable way to ask "what did the wall clock read
/// at this instant", and it consults the same zone database and `TZ`
/// setting that `date` does. There is no safe-Rust equivalent in std, and
/// the alternative is parsing TZif files ourselves, which is a lot of code
/// to reimplement something libc already gets right. The unsafe block is
/// the smallest it can be: one call, a zeroed output struct that libc
/// fills, and no pointers kept afterwards.
pub struct SystemZone;

#[allow(unsafe_code)]
impl TimeZone for SystemZone {
    fn offset_at(&self, unix: i64) -> i32 {
        let time = unix as libc::time_t;
        let mut out: libc::tm = unsafe { std::mem::zeroed() };
        let filled = unsafe { libc::localtime_r(&time, &mut out) };
        if filled.is_null() {
            // A zone database that cannot answer is a machine problem. UTC
            // is the only answer that cannot be subtly wrong, and the
            // schedule being visibly shifted beats it being silently off
            // by an hour.
            return 0;
        }
        out.tm_gmtoff as i32
    }
}

/// A zone with scripted transitions, for daylight saving tests. Each entry
/// is "from this instant onwards, the offset is this". Entries must be
/// sorted by instant; the first offset applies to everything before the
/// first transition.
pub struct TransitionZone {
    base: i32,
    transitions: Vec<(i64, i32)>,
}

impl TransitionZone {
    pub fn new(base: i32, transitions: Vec<(i64, i32)>) -> TransitionZone {
        TransitionZone { base, transitions }
    }
}

impl TimeZone for TransitionZone {
    fn offset_at(&self, unix: i64) -> i32 {
        let mut offset = self.base;
        for (at, value) in &self.transitions {
            if unix >= *at {
                offset = *value;
            } else {
                break;
            }
        }
        offset
    }
}

/// What the local wall clock read at `unix`.
pub fn local_civil(zone: &dyn TimeZone, unix: i64) -> Civil {
    from_unix_utc(unix + zone.offset_at(unix) as i64)
}

/// The instant at which the local wall clock read `civil`, assuming that
/// reading exists exactly once. Returns `None` when it does not: during a
/// spring-forward gap the reading never happens, and during a fall-back
/// repeat it happens twice and the caller has to decide which one it meant.
/// `schedule.rs` handles both, so this stays honest about ambiguity rather
/// than picking for the caller.
pub fn unique_instant_for(zone: &dyn TimeZone, civil: &Civil) -> Option<i64> {
    let as_if_utc = to_unix_utc(civil);
    let mut found: Vec<i64> = Vec::new();
    // Offsets in play anywhere near this reading. A day either side covers
    // every real transition, which are at most a couple of hours wide.
    for probe in [-86_400, 0, 86_400] {
        let offset = zone.offset_at(as_if_utc + probe) as i64;
        let candidate = as_if_utc - offset;
        if local_civil(zone, candidate) == *civil && !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    match found.len() {
        1 => Some(found[0]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// US Eastern around the 2026 transitions. Spring forward is
    /// 2026-03-08 02:00 local (07:00 UTC), fall back is 2026-11-01 02:00
    /// local (06:00 UTC).
    pub fn us_eastern_2026() -> TransitionZone {
        TransitionZone::new(
            -5 * 3600,
            vec![
                (to_unix_utc(&Civil::new(2026, 3, 8, 7, 0)), -4 * 3600),
                (to_unix_utc(&Civil::new(2026, 11, 1, 6, 0)), -5 * 3600),
            ],
        )
    }

    #[test]
    fn transition_zone_reports_the_offset_in_force() {
        let zone = us_eastern_2026();
        assert_eq!(
            zone.offset_at(to_unix_utc(&Civil::new(2026, 1, 1, 12, 0))),
            -5 * 3600
        );
        assert_eq!(
            zone.offset_at(to_unix_utc(&Civil::new(2026, 6, 1, 12, 0))),
            -4 * 3600
        );
        assert_eq!(
            zone.offset_at(to_unix_utc(&Civil::new(2026, 12, 1, 12, 0))),
            -5 * 3600
        );
    }

    #[test]
    fn local_civil_reads_the_wall_clock_either_side_of_a_transition() {
        let zone = us_eastern_2026();
        let just_before = to_unix_utc(&Civil::new(2026, 3, 8, 6, 59));
        let just_after = to_unix_utc(&Civil::new(2026, 3, 8, 7, 0));
        assert_eq!(
            local_civil(&zone, just_before),
            Civil::new(2026, 3, 8, 1, 59)
        );
        assert_eq!(local_civil(&zone, just_after), Civil::new(2026, 3, 8, 3, 0));
    }

    #[test]
    fn a_reading_inside_the_spring_forward_gap_has_no_instant() {
        let zone = us_eastern_2026();
        assert_eq!(
            unique_instant_for(&zone, &Civil::new(2026, 3, 8, 2, 30)),
            None
        );
    }

    #[test]
    fn a_repeated_fall_back_reading_is_reported_as_ambiguous() {
        let zone = us_eastern_2026();
        assert_eq!(
            unique_instant_for(&zone, &Civil::new(2026, 11, 1, 1, 30)),
            None
        );
    }

    #[test]
    fn an_ordinary_reading_resolves_to_one_instant() {
        let zone = us_eastern_2026();
        let civil = Civil::new(2026, 3, 9, 9, 0);
        let unix = unique_instant_for(&zone, &civil).unwrap();
        assert_eq!(local_civil(&zone, unix), civil);
        // 09:00 EDT is 13:00 UTC.
        assert_eq!(unix, to_unix_utc(&Civil::new(2026, 3, 9, 13, 0)));
    }

    #[test]
    fn the_system_zone_answers_and_stays_in_range() {
        // No assertion about the value: it depends on where the test runs.
        // What matters is that the call returns a plausible offset rather
        // than garbage, which is the only thing this wrapper can get wrong.
        let offset = SystemZone.offset_at(SystemClock.now());
        assert!(
            (-18 * 3600..=18 * 3600).contains(&offset),
            "implausible offset {offset}"
        );
    }

    #[test]
    fn the_system_clock_is_after_this_code_was_written() {
        assert!(SystemClock.now() > to_unix_utc(&Civil::new(2026, 1, 1, 0, 0)));
    }
}
