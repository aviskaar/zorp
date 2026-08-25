//! Schedule syntax and the arithmetic that turns it into instants.

use crate::civil::{Civil, Weekday};
use crate::clock::{local_civil, TimeZone};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleError(pub String);

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ScheduleError {}

/// Appended to every parse error. A schedule is written once and read at
/// 3am by nobody, so the moment it fails is the only moment anyone is
/// looking. The message has to be enough to fix it without opening docs.
const ACCEPTED: &str = "accepted forms: `every <n> minutes`, `every <n> hours`, \
     `hourly at :MM`, `daily at HH:MM`, `weekly on <weekday> at HH:MM`";

/// The longest window `next_after` will scan for each shape. Real
/// transitions are at most a couple of hours wide, so these are generous
/// by more than a day in every case.
const SCAN_DAYS_HOURLY: i64 = 2;
const SCAN_DAYS_DAILY: i64 = 3;
const SCAN_DAYS_WEEKLY: i64 = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Schedule {
    /// Elapsed-time interval, anchored in UTC. Immune to daylight saving
    /// by construction: it names a duration, not a wall-clock reading.
    EveryMinutes(u32),
    Hourly {
        minute: u32,
    },
    Daily {
        minute_of_day: u32,
    },
    Weekly {
        weekday: Weekday,
        minute_of_day: u32,
    },
}

fn err(message: impl Into<String>) -> ScheduleError {
    ScheduleError(format!("{}; {ACCEPTED}", message.into()))
}

/// Cron is precise, and it is also where "it fired at the wrong time and
/// nobody noticed" comes from. Rather than accept it as a second grammar,
/// recognize it well enough to say so.
fn looks_like_cron(text: &str) -> bool {
    if text.starts_with('@') {
        return true;
    }
    let fields: Vec<&str> = text.split_whitespace().collect();
    (5..=6).contains(&fields.len())
        && fields
            .iter()
            .all(|f| f.chars().all(|c| c.is_ascii_digit() || "*/,-".contains(c)))
}

fn parse_hhmm(token: &str) -> Result<u32, ScheduleError> {
    let Some((hours, minutes)) = token.split_once(':') else {
        return Err(err(format!(
            "'{token}' is not a time of day, which must be written HH:MM"
        )));
    };
    let bad = |what: &str| err(format!("'{token}' has {what}"));
    let hours: u32 = hours.parse().map_err(|_| bad("a non-numeric hour"))?;
    let minutes: u32 = minutes.parse().map_err(|_| bad("a non-numeric minute"))?;
    if hours > 23 {
        return Err(bad("an hour above 23"));
    }
    if minutes > 59 {
        return Err(bad("a minute above 59"));
    }
    Ok(hours * 60 + minutes)
}

fn parse_past_the_hour(token: &str) -> Result<u32, ScheduleError> {
    let Some(rest) = token.strip_prefix(':') else {
        return Err(err(format!(
            "'{token}' is not a minute past the hour, which must be written :MM"
        )));
    };
    let minute: u32 = rest
        .parse()
        .map_err(|_| err(format!("'{token}' has a non-numeric minute")))?;
    if minute > 59 {
        return Err(err(format!("'{token}' has a minute above 59")));
    }
    Ok(minute)
}

fn parse_interval(count: &str, unit: &str) -> Result<Schedule, ScheduleError> {
    let count: u32 = count
        .parse()
        .map_err(|_| err(format!("'{count}' is not a whole number of {unit}")))?;
    let minutes = match unit {
        "minute" | "minutes" => count,
        "hour" | "hours" => count.saturating_mul(60),
        other => return Err(err(format!("unknown interval unit '{other}'"))),
    };
    if !(1..=1440).contains(&minutes) {
        return Err(err(format!(
            "an interval of {minutes} minutes is outside the supported range of 1 to 1440"
        )));
    }
    Ok(Schedule::EveryMinutes(minutes))
}

impl Schedule {
    pub fn parse(text: &str) -> Result<Schedule, ScheduleError> {
        let lowered = text.trim().to_ascii_lowercase();
        if lowered.is_empty() {
            return Err(err("a schedule cannot be empty"));
        }
        if looks_like_cron(&lowered) {
            return Err(err(format!(
                "'{}' looks like a cron expression, which zorp does not accept",
                text.trim()
            )));
        }
        // `at` and `on` are filler that make a schedule read like English.
        // They carry no meaning, so dropping them early means the shapes
        // below do not have to spell out every combination.
        let words: Vec<&str> = lowered
            .split_whitespace()
            .filter(|w| !matches!(*w, "at" | "on"))
            .collect();
        match words.as_slice() {
            ["every", count, unit] if !unit.is_empty() && unit.starts_with(['m', 'h']) => {
                parse_interval(count, unit)
            }
            ["every", day, time] | ["weekly", day, time] => {
                let Some(weekday) = Weekday::parse(day) else {
                    return Err(err(format!("'{day}' is not a weekday")));
                };
                Ok(Schedule::Weekly {
                    weekday,
                    minute_of_day: parse_hhmm(time)?,
                })
            }
            ["daily", time] => Ok(Schedule::Daily {
                minute_of_day: parse_hhmm(time)?,
            }),
            ["hourly", minute] => Ok(Schedule::Hourly {
                minute: parse_past_the_hour(minute)?,
            }),
            ["daily"] => Err(err("`daily` needs a time of day, as in `daily at 09:00`")),
            ["weekly"] | ["weekly", _] => Err(err(
                "`weekly` needs a weekday and a time of day, as in `weekly on monday at 09:00`",
            )),
            ["hourly"] => Err(err(
                "`hourly` needs a minute past the hour, as in `hourly at :00`",
            )),
            _ => Err(err(format!("'{}' is not a schedule", text.trim()))),
        }
    }

    /// The canonical spelling, which parses back to the same schedule.
    /// `status` prints this rather than echoing what the user typed, so a
    /// tolerated shorthand is visibly resolved to one meaning.
    pub fn describe(&self) -> String {
        match self {
            Schedule::EveryMinutes(n) if *n >= 120 && n % 60 == 0 => {
                format!("every {} hours", n / 60)
            }
            Schedule::EveryMinutes(n) => format!("every {n} minutes"),
            Schedule::Hourly { minute } => format!("hourly at :{minute:02}"),
            Schedule::Daily { minute_of_day } => {
                format!("daily at {:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
            }
            Schedule::Weekly {
                weekday,
                minute_of_day,
            } => format!(
                "weekly on {} at {:02}:{:02}",
                weekday.name(),
                minute_of_day / 60,
                minute_of_day % 60
            ),
        }
    }

    /// The local day, or day and hour, that one occurrence belongs to. Two
    /// readings sharing a key are the same occurrence, which is what stops
    /// a repeated wall-clock hour firing a job twice.
    fn period_key(&self, c: &Civil) -> (i32, u32, u32, u32) {
        let (y, m, d) = c.date();
        match self {
            Schedule::Hourly { .. } => (y, m, d, c.hour),
            _ => (y, m, d, 0),
        }
    }

    /// How far into its period a reading is, in the same units as the
    /// schedule's target.
    fn position(&self, c: &Civil) -> u32 {
        match self {
            Schedule::Hourly { .. } => c.minute,
            _ => c.minute_of_day(),
        }
    }

    fn target(&self) -> u32 {
        match self {
            Schedule::EveryMinutes(_) => 0,
            Schedule::Hourly { minute } => *minute,
            Schedule::Daily { minute_of_day } => *minute_of_day,
            Schedule::Weekly { minute_of_day, .. } => *minute_of_day,
        }
    }

    fn day_matches(&self, c: &Civil) -> bool {
        match self {
            Schedule::Weekly { weekday, .. } => c.weekday() == *weekday,
            _ => true,
        }
    }

    fn scan_days(&self) -> i64 {
        match self {
            Schedule::EveryMinutes(_) => 0,
            Schedule::Hourly { .. } => SCAN_DAYS_HOURLY,
            Schedule::Daily { .. } => SCAN_DAYS_DAILY,
            Schedule::Weekly { .. } => SCAN_DAYS_WEEKLY,
        }
    }

    /// The first instant strictly after `after` at which this schedule
    /// fires.
    ///
    /// One rule covers every wall-clock case: fire at the first minute
    /// whose local reading has reached the target, in a period that has
    /// not already had its occurrence. Ordinary days resolve to the exact
    /// reading. A reading that daylight saving deleted resolves to the
    /// instant the clock jumped past it, so the run happens late rather
    /// than not at all. A reading that daylight saving repeated resolves
    /// to the earlier of the two, because the period key is already spent
    /// by the time the second one comes round.
    pub fn next_after(&self, zone: &dyn TimeZone, after: i64) -> Option<i64> {
        if let Schedule::EveryMinutes(minutes) = self {
            return Some(after + *minutes as i64 * 60);
        }
        let target = self.target();
        let after_local = local_civil(zone, after);
        let spent = self.period_key(&after_local);
        let period_already_fired = self.position(&after_local) >= target;
        let limit = after + self.scan_days() * crate::civil::SECONDS_PER_DAY;
        let mut unix = (after.div_euclid(60) + 1) * 60;
        while unix <= limit {
            let reading = local_civil(zone, unix);
            let fresh = !(period_already_fired && self.period_key(&reading) == spent);
            if fresh && self.day_matches(&reading) && self.position(&reading) >= target {
                return Some(unix);
            }
            unix += 60;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::to_unix_utc;
    use crate::clock::{FixedOffsetZone, TransitionZone};

    /// US Eastern around the 2026 transitions. Spring forward is
    /// 2026-03-08 02:00 local, fall back is 2026-11-01 02:00 local.
    fn us_eastern_2026() -> TransitionZone {
        TransitionZone::new(
            -5 * 3600,
            vec![
                (to_unix_utc(&Civil::new(2026, 3, 8, 7, 0)), -4 * 3600),
                (to_unix_utc(&Civil::new(2026, 11, 1, 6, 0)), -5 * 3600),
            ],
        )
    }

    fn utc() -> FixedOffsetZone {
        FixedOffsetZone(0)
    }

    #[test]
    fn plain_forms_parse() {
        assert_eq!(
            Schedule::parse("every 15 minutes"),
            Ok(Schedule::EveryMinutes(15))
        );
        assert_eq!(
            Schedule::parse("every 2 hours"),
            Ok(Schedule::EveryMinutes(120))
        );
        assert_eq!(
            Schedule::parse("hourly at :20"),
            Ok(Schedule::Hourly { minute: 20 })
        );
        assert_eq!(
            Schedule::parse("daily at 09:00"),
            Ok(Schedule::Daily { minute_of_day: 540 })
        );
        assert_eq!(
            Schedule::parse("weekly on monday at 09:00"),
            Ok(Schedule::Weekly {
                weekday: Weekday::Monday,
                minute_of_day: 540
            })
        );
        assert_eq!(
            Schedule::parse("every monday 09:00"),
            Ok(Schedule::Weekly {
                weekday: Weekday::Monday,
                minute_of_day: 540
            })
        );
    }

    #[test]
    fn filler_words_case_and_spacing_do_not_change_meaning() {
        let canonical = Schedule::parse("weekly on monday at 09:00").unwrap();
        for text in [
            "WEEKLY ON MONDAY AT 09:00",
            "weekly monday 09:00",
            "  weekly   mon   9:00 ",
            "every Monday at 09:00",
        ] {
            assert_eq!(Schedule::parse(text), Ok(canonical), "{text}");
        }
    }

    #[test]
    fn a_schedule_with_no_time_is_an_error_not_a_guess() {
        // Every one of these has more than one plausible reading. A
        // scheduler that picks for you is a scheduler that fires at a time
        // you did not choose, and you find out weeks later.
        for text in ["daily", "weekly", "weekly on monday", "hourly"] {
            let err = Schedule::parse(text).unwrap_err();
            assert!(
                err.0.contains("needs a")
                    && (err.0.contains("time of day") || err.0.contains("minute past the hour")),
                "{text}: {err}"
            );
        }
    }

    #[test]
    fn unparseable_schedules_fail_loudly_and_say_what_is_accepted() {
        for text in [
            "",
            "sometimes",
            "daily at 25:00",
            "daily at 09:60",
            "weekly on moonday at 09:00",
            "every 0 minutes",
            "every 4000 minutes",
            "every -1 minutes",
            "hourly at :99",
            "daily at 9",
        ] {
            let err = Schedule::parse(text).unwrap_err();
            assert!(!err.0.is_empty(), "{text} produced an empty error");
            assert!(
                err.0.contains("daily") || err.0.contains("minutes"),
                "{text}: error should show the accepted forms, got {err}"
            );
        }
    }

    /// Cron is precise and it is also the single most common source of
    /// "it fired at the wrong time and nobody noticed". Accepting it
    /// silently alongside a plain grammar gives two ways to be wrong.
    #[test]
    fn cron_expressions_are_rejected_with_a_pointer_to_the_grammar() {
        for text in ["0 9 * * 1", "*/5 * * * *", "@daily"] {
            let err = Schedule::parse(text).unwrap_err();
            assert!(
                err.0.to_lowercase().contains("cron"),
                "{text}: {err} should mention cron"
            );
        }
    }

    #[test]
    fn describe_round_trips_through_parse() {
        for text in [
            "every 15 minutes",
            "hourly at :20",
            "daily at 09:00",
            "weekly on monday at 09:00",
        ] {
            let parsed = Schedule::parse(text).unwrap();
            assert_eq!(parsed.describe(), text);
            assert_eq!(Schedule::parse(&parsed.describe()), Ok(parsed));
        }
    }

    #[test]
    fn an_interval_schedule_is_pure_utc_arithmetic() {
        let zone = us_eastern_2026();
        let s = Schedule::EveryMinutes(30);
        // Straddling the spring-forward transition. An interval job must
        // not gain or lose a run because the wall clock moved: it is
        // defined in elapsed time, not in local readings.
        let before = to_unix_utc(&Civil::new(2026, 3, 8, 6, 45));
        assert_eq!(s.next_after(&zone, before), Some(before + 1800));
    }

    #[test]
    fn daily_fires_at_the_same_local_time_each_day() {
        let zone = us_eastern_2026();
        let s = Schedule::Daily { minute_of_day: 540 };
        let start = to_unix_utc(&Civil::new(2026, 6, 1, 0, 0));
        let first = s.next_after(&zone, start).unwrap();
        assert_eq!(local_civil(&zone, first), Civil::new(2026, 6, 1, 9, 0));
        let second = s.next_after(&zone, first).unwrap();
        assert_eq!(local_civil(&zone, second), Civil::new(2026, 6, 2, 9, 0));
        assert_eq!(second - first, 86_400);
    }

    #[test]
    fn weekly_fires_only_on_the_named_weekday() {
        let zone = utc();
        let s = Schedule::Weekly {
            weekday: Weekday::Monday,
            minute_of_day: 540,
        };
        // 2026-08-18 is a Tuesday, so the next Monday is 2026-08-24.
        let start = to_unix_utc(&Civil::new(2026, 8, 18, 12, 0));
        let first = s.next_after(&zone, start).unwrap();
        assert_eq!(local_civil(&zone, first), Civil::new(2026, 8, 24, 9, 0));
        let second = s.next_after(&zone, first).unwrap();
        assert_eq!(local_civil(&zone, second), Civil::new(2026, 8, 31, 9, 0));
    }

    #[test]
    fn hourly_fires_once_an_hour_at_the_named_minute() {
        let zone = utc();
        let s = Schedule::Hourly { minute: 20 };
        let start = to_unix_utc(&Civil::new(2026, 8, 18, 9, 25));
        let first = s.next_after(&zone, start).unwrap();
        assert_eq!(local_civil(&zone, first), Civil::new(2026, 8, 18, 10, 20));
        assert_eq!(
            local_civil(&zone, s.next_after(&zone, first).unwrap()),
            Civil::new(2026, 8, 18, 11, 20)
        );
    }

    /// Monday 09:00 spans the spring-forward weekend without drifting: the
    /// wall clock reading is what was asked for, so the elapsed gap is 167
    /// hours, not 168.
    #[test]
    fn weekly_holds_its_local_time_across_spring_forward() {
        let zone = us_eastern_2026();
        let s = Schedule::Weekly {
            weekday: Weekday::Monday,
            minute_of_day: 540,
        };
        // 2026-03-02 is the Monday before the transition.
        let start = to_unix_utc(&Civil::new(2026, 3, 1, 0, 0));
        let before = s.next_after(&zone, start).unwrap();
        assert_eq!(local_civil(&zone, before), Civil::new(2026, 3, 2, 9, 0));
        let after = s.next_after(&zone, before).unwrap();
        assert_eq!(local_civil(&zone, after), Civil::new(2026, 3, 9, 9, 0));
        assert_eq!(after - before, 167 * 3600);
    }

    /// The mirror image. Fall back means 169 hours of elapsed time between
    /// two 09:00 Mondays, and still exactly one run.
    #[test]
    fn weekly_holds_its_local_time_across_fall_back() {
        let zone = us_eastern_2026();
        let s = Schedule::Weekly {
            weekday: Weekday::Monday,
            minute_of_day: 540,
        };
        let start = to_unix_utc(&Civil::new(2026, 10, 25, 0, 0));
        let before = s.next_after(&zone, start).unwrap();
        assert_eq!(local_civil(&zone, before), Civil::new(2026, 10, 26, 9, 0));
        let after = s.next_after(&zone, before).unwrap();
        assert_eq!(local_civil(&zone, after), Civil::new(2026, 11, 2, 9, 0));
        assert_eq!(after - before, 169 * 3600);
    }

    /// 02:30 does not happen on the day the clocks go forward. Skipping
    /// the day would silently drop a run; the first instant at or after
    /// the requested reading is 03:00.
    #[test]
    fn a_daily_time_inside_the_spring_forward_gap_fires_at_the_transition() {
        let zone = us_eastern_2026();
        let s = Schedule::Daily { minute_of_day: 150 };
        let previous = to_unix_utc(&Civil::new(2026, 3, 7, 7, 30));
        assert_eq!(
            local_civil(&zone, previous),
            Civil::new(2026, 3, 7, 2, 30),
            "precondition: previous run was Saturday 02:30 local"
        );
        let fired = s.next_after(&zone, previous).unwrap();
        assert_eq!(local_civil(&zone, fired), Civil::new(2026, 3, 8, 3, 0));
        // And the day after is back to normal.
        let next = s.next_after(&zone, fired).unwrap();
        assert_eq!(local_civil(&zone, next), Civil::new(2026, 3, 9, 2, 30));
    }

    /// 01:30 happens twice on the day the clocks go back. Firing twice is
    /// the classic duplicate-run bug, so only the first one counts.
    #[test]
    fn a_daily_time_inside_the_fall_back_repeat_fires_exactly_once() {
        let zone = us_eastern_2026();
        let s = Schedule::Daily { minute_of_day: 90 };
        let previous = to_unix_utc(&Civil::new(2026, 10, 31, 5, 30));
        assert_eq!(
            local_civil(&zone, previous),
            Civil::new(2026, 10, 31, 1, 30),
            "precondition: previous run was Saturday 01:30 local"
        );
        let fired = s.next_after(&zone, previous).unwrap();
        assert_eq!(local_civil(&zone, fired), Civil::new(2026, 11, 1, 1, 30));
        // The earlier of the two readings, which is EDT, not EST.
        assert_eq!(fired, to_unix_utc(&Civil::new(2026, 11, 1, 5, 30)));
        // The second 01:30 must not produce another run that day.
        let next = s.next_after(&zone, fired).unwrap();
        assert_eq!(local_civil(&zone, next), Civil::new(2026, 11, 2, 1, 30));
    }

    #[test]
    fn next_after_is_strictly_after_and_never_returns_its_input() {
        let zone = us_eastern_2026();
        for s in [
            Schedule::EveryMinutes(5),
            Schedule::Hourly { minute: 0 },
            Schedule::Daily { minute_of_day: 0 },
            Schedule::Weekly {
                weekday: Weekday::Sunday,
                minute_of_day: 0,
            },
        ] {
            let at = to_unix_utc(&Civil::new(2026, 11, 1, 5, 0));
            let next = s.next_after(&zone, at).unwrap();
            assert!(next > at, "{s:?} returned {next} for {at}");
        }
    }
}
