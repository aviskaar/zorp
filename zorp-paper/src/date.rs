//! Turning epoch milliseconds into a date, without a dependency.

/// A UTC calendar date and time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Epoch milliseconds to a UTC calendar date.
///
/// The days-to-civil conversion is Howard Hinnant's, which is the
/// standard closed form for a proleptic Gregorian calendar and needs no
/// lookup tables or loops. `div_euclid` rather than `/` throughout,
/// because an instant before the epoch is negative and truncating
/// division would round it the wrong way.
pub fn from_millis(millis: i64) -> Utc {
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };

    Utc {
        year,
        month,
        day,
        hour: (seconds_of_day / 3600) as u32,
        minute: ((seconds_of_day % 3600) / 60) as u32,
        second: (seconds_of_day % 60) as u32,
    }
}

/// "August 2026". What a paper puts under the title.
pub fn month_and_year(at: Utc) -> String {
    let name = MONTH_NAMES
        .get(at.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("");
    format!("{name} {}", at.year)
}

/// "D:20260818120000Z", the date format a PDF `/CreationDate` takes.
pub fn pdf_date(at: Utc) -> String {
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}Z",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_the_first_of_january_1970() {
        let at = from_millis(0);
        assert_eq!((at.year, at.month, at.day), (1970, 1, 1));
        assert_eq!((at.hour, at.minute, at.second), (0, 0, 0));
    }

    #[test]
    fn a_known_instant_round_trips() {
        // 2026-08-18T12:34:56Z
        let at = from_millis(1_787_056_496_000);
        assert_eq!((at.year, at.month, at.day), (2026, 8, 18));
        assert_eq!((at.hour, at.minute, at.second), (12, 34, 56));
    }

    #[test]
    fn a_leap_day_is_a_leap_day() {
        // 2024-02-29T00:00:00Z
        let at = from_millis(1_709_164_800_000);
        assert_eq!((at.year, at.month, at.day), (2024, 2, 29));
    }

    #[test]
    fn before_the_epoch_still_gives_a_date() {
        // 1969-12-31T23:59:59Z
        let at = from_millis(-1000);
        assert_eq!((at.year, at.month, at.day), (1969, 12, 31));
        assert_eq!((at.hour, at.minute, at.second), (23, 59, 59));
    }

    #[test]
    fn month_and_year_reads_the_way_a_paper_prints_it() {
        assert_eq!(
            month_and_year(from_millis(1_787_056_496_000)),
            "August 2026"
        );
    }

    #[test]
    fn pdf_date_is_the_format_readers_expect() {
        assert_eq!(
            pdf_date(from_millis(1_787_056_496_000)),
            "D:20260818123456Z"
        );
    }

    #[test]
    fn pdf_date_pads_every_field() {
        // 2003-01-02T03:04:05Z
        assert_eq!(
            pdf_date(from_millis(1_041_476_645_000)),
            "D:20030102030405Z"
        );
    }
}
