//! Calendar dates, with just enough arithmetic to answer "how old is this
//! profile" and "has this deadline passed".
//!
//! Rolled by hand rather than pulled from a date crate because the whole
//! need is two conversions between a civil date and a day number. A
//! dependency for that would cost more to audit than to write.

use std::fmt;

/// A calendar date in the proleptic Gregorian calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Parse an ISO 8601 calendar date, `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Result<Date, String> {
        let t = text.trim();
        let parts: Vec<&str> = t.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return Err(format!("'{t}' is not an ISO date (expected YYYY-MM-DD)"));
        }
        let year: i32 = parts[0]
            .parse()
            .map_err(|_| format!("'{t}' has a non-numeric year"))?;
        let month: u32 = parts[1]
            .parse()
            .map_err(|_| format!("'{t}' has a non-numeric month"))?;
        let day: u32 = parts[2]
            .parse()
            .map_err(|_| format!("'{t}' has a non-numeric day"))?;
        if !(1..=12).contains(&month) {
            return Err(format!("'{t}' has month {month}, which is not 1..=12"));
        }
        if day < 1 || day > days_in_month(year, month) {
            return Err(format!(
                "'{t}' has day {day}, which is not a real day of that month"
            ));
        }
        Ok(Date { year, month, day })
    }

    /// Today's date in UTC, read from the system clock.
    pub fn today() -> Date {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        from_days(secs.div_euclid(86_400))
    }

    /// Days since 1970-01-01. Negative before it.
    pub fn days_from_epoch(self) -> i64 {
        to_days(self)
    }

    /// How many days `self` is after `other`. Negative if before.
    pub fn days_since(self, other: Date) -> i64 {
        self.days_from_epoch() - other.days_from_epoch()
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01, by the standard shift-the-year-to-March algorithm
/// that makes the leap day the last day of the year.
fn to_days(date: Date) -> i64 {
    let y = date.year as i64 - i64::from(date.month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = date.month as i64;
    let shifted = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * shifted + 2) / 5 + date.day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`to_days`].
fn from_days(days: i64) -> Date {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    Date {
        year: (y + i64::from(month <= 2)) as i32,
        month,
        day,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_iso_dates() {
        let d = Date::parse("2026-08-18").unwrap();
        assert_eq!(
            (d.year, d.month, d.day),
            (2026, 8, 18),
            "should read the parts in ISO order"
        );
        assert_eq!(d.to_string(), "2026-08-18");
    }

    #[test]
    fn rejects_malformed_and_impossible_dates() {
        for bad in [
            "2026-8-18",
            "18-08-2026",
            "2026-13-01",
            "2026-02-30",
            "2026-00-01",
            "not a date",
            "",
        ] {
            assert!(Date::parse(bad).is_err(), "{bad} should not parse");
        }
        // A real leap day parses; the same day in a non-leap year does not.
        assert!(Date::parse("2024-02-29").is_ok());
        assert!(Date::parse("2026-02-29").is_err());
    }

    #[test]
    fn day_arithmetic_spans_months_years_and_leap_days() {
        let epoch = Date::parse("1970-01-01").unwrap();
        assert_eq!(epoch.days_from_epoch(), 0);
        let checked = Date::parse("2026-05-06").unwrap();
        let today = Date::parse("2026-08-18").unwrap();
        // May has 31 days, June 30, July 31: 25 + 30 + 31 + 18 = 104.
        assert_eq!(today.days_since(checked), 104);
        // Across a leap day.
        let before = Date::parse("2024-02-28").unwrap();
        let after = Date::parse("2024-03-01").unwrap();
        assert_eq!(after.days_since(before), 2);
        // And backwards.
        assert_eq!(before.days_since(after), -2);
    }

    #[test]
    fn round_trips_a_wide_span_of_days() {
        for day in [-25_000i64, -1, 0, 1, 19_000, 20_684, 40_000] {
            let d = from_days(day);
            assert_eq!(to_days(d), day, "{d} should round trip");
        }
    }
}
