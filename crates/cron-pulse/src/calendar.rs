//! Pure `std` civil (proleptic Gregorian) UTC calendar helpers.
//!
//! No external dependency is used: the conversions below are the classic
//! Howard Hinnant `days_from_civil` / `civil_from_days` algorithms, valid for
//! the whole proleptic Gregorian calendar (including the full leap-year rule:
//! divisible by 4, except centuries not divisible by 400).

/// Number of seconds in one day.
pub const SECS_PER_DAY: u64 = 86_400;
/// Number of seconds in one hour.
pub const SECS_PER_HOUR: u64 = 3_600;
/// Number of seconds in one minute.
pub const SECS_PER_MINUTE: u64 = 60;

/// A broken-down UTC date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDateTime {
    /// Proleptic Gregorian year (e.g. `2024`).
    pub year: i64,
    /// Month in `1..=12`.
    pub month: u32,
    /// Day of month in `1..=31`.
    pub day: u32,
    /// Hour in `0..=23`.
    pub hour: u32,
    /// Minute in `0..=59`.
    pub minute: u32,
    /// Second in `0..=59`.
    pub second: u32,
    /// Day of week, `0` = Sunday … `6` = Saturday.
    pub weekday: u32,
}

/// Returns `true` if `year` is a leap year in the proleptic Gregorian calendar.
///
/// * `2024` → `true`
/// * `2100` → `false` (century not divisible by 400)
/// * `2000` → `true` (century divisible by 400)
#[inline]
pub const fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Returns the number of days in `month` of `year`.
///
/// Returns `0` for an out-of-range month so that callers can treat it as
/// "never matches" instead of panicking.
#[inline]
pub const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Converts a civil date to the number of days since `1970-01-01`.
///
/// The result is negative for dates before the Unix epoch.
#[inline]
pub const fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let d = day as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Converts a number of days since `1970-01-01` back to a civil date.
#[inline]
pub const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// Returns the day of week (`0` = Sunday … `6` = Saturday) for a day count
/// since the Unix epoch. `1970-01-01` was a Thursday (`4`).
#[inline]
pub const fn weekday_from_days(days: i64) -> u32 {
    let rem = days % 7;
    let rem = if rem < 0 { rem + 7 } else { rem };
    ((rem + 4) % 7) as u32
}

/// Converts a Unix timestamp (seconds, UTC) into a broken-down date and time.
///
/// The conversion is exact for every `u64` value representable by the
/// proleptic Gregorian calendar (i.e. up to year ~584 billion).
#[inline]
pub fn civil_from_unix(unix_secs: u64) -> CivilDateTime {
    let days = (unix_secs / SECS_PER_DAY) as i64;
    let secs_of_day = unix_secs % SECS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    CivilDateTime {
        year,
        month,
        day,
        hour: (secs_of_day / SECS_PER_HOUR) as u32,
        minute: ((secs_of_day % SECS_PER_HOUR) / SECS_PER_MINUTE) as u32,
        second: (secs_of_day % SECS_PER_MINUTE) as u32,
        weekday: weekday_from_days(days),
    }
}

/// Converts a broken-down UTC date and time into a Unix timestamp.
///
/// Returns `None` when the timestamp would be negative (before the epoch) or
/// when the date is not a valid civil date.
#[inline]
pub fn unix_from_civil(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<u64> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let secs_of_day = hour as i64 * SECS_PER_HOUR as i64
        + minute as i64 * SECS_PER_MINUTE as i64
        + second as i64;
    let secs = days
        .checked_mul(SECS_PER_DAY as i64)?
        .checked_add(secs_of_day)?;
    if secs < 0 {
        None
    } else {
        Some(secs as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_year_rule() {
        assert!(is_leap_year(2024));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(2100));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(1600));
    }

    #[test]
    fn days_in_month_values() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
        assert_eq!(days_in_month(2024, 13), 0);
    }

    #[test]
    fn epoch_roundtrip() {
        let dt = civil_from_unix(0);
        assert_eq!(
            dt,
            CivilDateTime {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                weekday: 4,
            }
        );
        assert_eq!(unix_from_civil(1970, 1, 1, 0, 0, 0), Some(0));
    }

    #[test]
    fn known_timestamps() {
        // 2024-02-29T12:34:56Z
        let ts = unix_from_civil(2024, 2, 29, 12, 34, 56).unwrap();
        assert_eq!(ts, 1_709_210_096);
        let dt = civil_from_unix(ts);
        assert_eq!((dt.year, dt.month, dt.day), (2024, 2, 29));
        assert_eq!((dt.hour, dt.minute, dt.second), (12, 34, 56));
        assert_eq!(dt.weekday, 4); // Thursday
    }

    #[test]
    fn roundtrip_over_many_days() {
        let mut days = -20_000i64;
        while days < 40_000 {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "day {days}");
            assert!(d >= 1 && d <= days_in_month(y, m));
            days += 1;
        }
    }

    #[test]
    fn weekday_known_dates() {
        // 2000-01-01 was a Saturday.
        assert_eq!(weekday_from_days(days_from_civil(2000, 1, 1)), 6);
        // 2100-01-01 was a Friday.
        assert_eq!(weekday_from_days(days_from_civil(2100, 1, 1)), 5);
        // 1969-12-31 was a Wednesday.
        assert_eq!(weekday_from_days(days_from_civil(1969, 12, 31)), 3);
    }

    #[test]
    fn extreme_timestamps_do_not_panic() {
        let dt = civil_from_unix(u64::MAX);
        assert!(dt.year > 0);
        let dt = civil_from_unix(u64::MAX / 2);
        assert!(dt.year > 0);
    }

    #[test]
    fn invalid_civil_dates_rejected() {
        assert_eq!(unix_from_civil(2023, 2, 29, 0, 0, 0), None);
        assert_eq!(unix_from_civil(2024, 4, 31, 0, 0, 0), None);
        assert_eq!(unix_from_civil(2024, 13, 1, 0, 0, 0), None);
        assert_eq!(unix_from_civil(2024, 1, 1, 24, 0, 0), None);
        assert_eq!(unix_from_civil(1969, 12, 31, 23, 59, 59), None);
    }
}
