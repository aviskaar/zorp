//! Proleptic Gregorian calendar arithmetic, with no dependency on a date
//! library and no notion of a time zone. A `Civil` value is a wall-clock
//! reading: it says "09:00 on Monday" and nothing about which instant that
//! was. Turning one into an instant needs an offset, which is `clock.rs`'s
//! job.

use std::fmt;

/// Seconds in a day. Leap seconds do not exist in Unix time, so this is
/// exact.
pub const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// Parse a weekday name or its common three-letter abbreviation.
    pub fn parse(name: &str) -> Option<Weekday> {
        match name.trim().to_ascii_lowercase().as_str() {
            "monday" | "mon" => Some(Weekday::Monday),
            "tuesday" | "tue" | "tues" => Some(Weekday::Tuesday),
            "wednesday" | "wed" => Some(Weekday::Wednesday),
            "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thursday),
            "friday" | "fri" => Some(Weekday::Friday),
            "saturday" | "sat" => Some(Weekday::Saturday),
            "sunday" | "sun" => Some(Weekday::Sunday),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Weekday::Monday => "monday",
            Weekday::Tuesday => "tuesday",
            Weekday::Wednesday => "wednesday",
            Weekday::Thursday => "thursday",
            Weekday::Friday => "friday",
            Weekday::Saturday => "saturday",
            Weekday::Sunday => "sunday",
        }
    }
}

/// A wall-clock reading with no zone attached. Minute resolution, because
/// no schedule this crate accepts is finer than a minute and seconds would
/// only add ways to be wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Civil {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Civil {
    pub fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Civil {
        Civil {
            year,
            month,
            day,
            hour,
            minute,
        }
    }

    /// The calendar date alone, for asking "is this the same local day".
    pub fn date(&self) -> (i32, u32, u32) {
        (self.year, self.month, self.day)
    }

    /// Minutes since local midnight, for comparing against a schedule's
    /// time of day without comparing dates too.
    pub fn minute_of_day(&self) -> u32 {
        self.hour * 60 + self.minute
    }

    pub fn weekday(&self) -> Weekday {
        weekday_from_days(days_from_civil(self.year, self.month, self.day))
    }
}

impl fmt::Display for Civil {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date. This is Howard
/// Hinnant's `days_from_civil`, which is exact for the whole range we care
/// about and needs no tables.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let d = day as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of `days_from_civil`.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

/// 1970-01-01 was a Thursday, which anchors the whole cycle.
pub fn weekday_from_days(days: i64) -> Weekday {
    match (days + 3).rem_euclid(7) {
        0 => Weekday::Monday,
        1 => Weekday::Tuesday,
        2 => Weekday::Wednesday,
        3 => Weekday::Thursday,
        4 => Weekday::Friday,
        5 => Weekday::Saturday,
        _ => Weekday::Sunday,
    }
}

/// Read a `Civil` as if it were UTC and return the Unix instant. Callers
/// that mean local time subtract the offset afterwards.
pub fn to_unix_utc(c: &Civil) -> i64 {
    days_from_civil(c.year, c.month, c.day) * SECONDS_PER_DAY
        + c.hour as i64 * 3600
        + c.minute as i64 * 60
}

/// Read a Unix instant as UTC. Seconds are truncated, not rounded, so a
/// value mid-minute reads as that minute.
pub fn from_unix_utc(unix: i64) -> Civil {
    let days = unix.div_euclid(SECONDS_PER_DAY);
    let secs = unix.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: (secs % 3600 / 60) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // A leap day, the case naive implementations get wrong.
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        // A century that is not a leap year.
        assert_eq!(civil_from_days(days_from_civil(1900, 3, 1)), (1900, 3, 1));
        // A century that is.
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
    }

    #[test]
    fn every_day_across_a_leap_year_round_trips() {
        let start = days_from_civil(2023, 12, 1);
        let end = days_from_civil(2025, 1, 31);
        for day in start..=end {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn unix_conversion_round_trips_at_minute_resolution() {
        let c = Civil::new(2026, 3, 8, 9, 30);
        let unix = to_unix_utc(&c);
        assert_eq!(from_unix_utc(unix), c);
        assert_eq!(from_unix_utc(0), Civil::new(1970, 1, 1, 0, 0));
        // Pre-epoch instants must not wrap into the wrong day.
        assert_eq!(from_unix_utc(-60), Civil::new(1969, 12, 31, 23, 59));
    }

    #[test]
    fn weekdays_are_anchored_to_a_known_thursday() {
        assert_eq!(weekday_from_days(0), Weekday::Thursday);
        assert_eq!(Civil::new(2026, 8, 17, 0, 0).weekday(), Weekday::Monday);
        assert_eq!(Civil::new(2026, 3, 8, 0, 0).weekday(), Weekday::Sunday);
    }

    #[test]
    fn weekday_names_and_abbreviations_parse_and_garbage_does_not() {
        assert_eq!(Weekday::parse("Monday"), Some(Weekday::Monday));
        assert_eq!(Weekday::parse("mon"), Some(Weekday::Monday));
        assert_eq!(Weekday::parse("  THU "), Some(Weekday::Thursday));
        assert_eq!(Weekday::parse("moonday"), None);
        assert_eq!(Weekday::parse(""), None);
    }

    #[test]
    fn minute_of_day_and_date_split_a_reading_cleanly() {
        let c = Civil::new(2026, 3, 8, 2, 30);
        assert_eq!(c.minute_of_day(), 150);
        assert_eq!(c.date(), (2026, 3, 8));
    }
}
