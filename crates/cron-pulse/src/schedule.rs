//! The [`CronSchedule`] type: parsing, matching, next-occurrence search and
//! human-readable description.

use crate::calendar::{civil_from_unix, days_in_month, unix_from_civil};
use crate::error::CronError;
use crate::field::{Field, FieldSpec};

/// A parsed and validated 5-field UTC cron expression.
///
/// The five fields are, in order: `minute hour day_of_month month day_of_week`.
///
/// ```
/// use cron_pulse::CronSchedule;
///
/// let s = CronSchedule::parse("*/15 9-17 * * MON-FRI").unwrap();
/// assert!(s.matches(1_699_963_200)); // 2023-11-14T12:00:00Z (Tuesday)
/// ```
/// Equality is *semantic*: two schedules are equal when they accept exactly
/// the same instants. The original source text is excluded so that `0 0 * * 7`
/// and `0 0 * * 0` (both Sunday) or `jan` and `JAN` compare equal.
#[derive(Debug, Clone)]
pub struct CronSchedule {
    /// Original expression, trimmed.
    source: String,
    minute: Field,
    hour: Field,
    day_of_month: Field,
    month: Field,
    day_of_week: Field,
    /// `true` when the day-of-month field was written as `*`.
    dom_star: bool,
    /// `true` when the day-of-week field was written as `*`.
    dow_star: bool,
}

impl PartialEq for CronSchedule {
    fn eq(&self, other: &Self) -> bool {
        self.minute == other.minute
            && self.hour == other.hour
            && self.day_of_month == other.day_of_month
            && self.month == other.month
            && self.day_of_week == other.day_of_week
            && self.dom_star == other.dom_star
            && self.dow_star == other.dow_star
    }
}

impl Eq for CronSchedule {}

impl CronSchedule {
    /// Parses a 5-field cron expression (or one of the standard macros).
    ///
    /// Macros are case-insensitive: `@yearly`/`@annually`, `@monthly`,
    /// `@weekly`, `@daily`/`@midnight`, `@hourly`.
    ///
    /// # Errors
    ///
    /// Returns a [`CronError`] describing precisely what is wrong with the
    /// expression. This function never panics.
    pub fn parse(expr: &str) -> Result<CronSchedule, CronError> {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return Err(CronError::EmptyExpression);
        }

        let expanded = expand_macro(trimmed)?;
        let fields: Vec<&str> = expanded.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronError::InvalidFieldCount {
                found: fields.len(),
            });
        }

        let minute = FieldSpec::MINUTE.parse(fields[0])?;
        let hour = FieldSpec::HOUR.parse(fields[1])?;
        let day_of_month = FieldSpec::DAY_OF_MONTH.parse(fields[2])?;
        let month = FieldSpec::MONTH.parse(fields[3])?;
        let day_of_week = FieldSpec::DAY_OF_WEEK.parse(fields[4])?;

        let dom_star = day_of_month.is_wildcard();
        let dow_star = day_of_week.is_wildcard();

        let schedule = CronSchedule {
            source: expanded.to_string(),
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
            dom_star,
            dow_star,
        };

        schedule.validate_calendar()?;
        Ok(schedule)
    }

    /// Returns the normalized expression this schedule was built from
    /// (macros are expanded, original casing of aliases is preserved).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns `true` if the UTC minute containing `unix_secs` matches.
    ///
    /// Seconds inside the minute are ignored: `matches(t)` and
    /// `matches(t - t % 60)` are equivalent.
    pub fn matches(&self, unix_secs: u64) -> bool {
        let dt = civil_from_unix(unix_secs);
        self.matches_parts(dt.month, dt.day, dt.hour, dt.minute, dt.weekday)
    }

    fn matches_parts(&self, month: u32, day: u32, hour: u32, minute: u32, weekday: u32) -> bool {
        if !self.minute.contains(minute) || !self.hour.contains(hour) || !self.month.contains(month)
        {
            return false;
        }
        let dom_match = self.day_of_month.contains(day);
        let dow_match = self.day_of_week.contains(weekday);
        // POSIX semantics: when both day fields are restricted, the day
        // condition is a logical OR; otherwise it is a logical AND.
        if self.dom_star || self.dow_star {
            dom_match && dow_match
        } else {
            dom_match || dow_match
        }
    }

    /// Returns the next matching UTC timestamp strictly after the minute of
    /// `after_unix_secs`, aligned on the second `:00` of that minute.
    ///
    /// The search jumps over months, days and hours instead of scanning minute
    /// by minute, so it stays fast even across multi-year gaps.
    ///
    /// Returns `None` when no occurrence exists within the representable range
    /// (roughly year 292 277 026 596).
    pub fn next_after(&self, after_unix_secs: u64) -> Option<u64> {
        // Start at the first second of the *next* minute.
        let mut ts = after_unix_secs
            .checked_div(60)?
            .checked_mul(60)?
            .checked_add(60)?;

        // Hard bound: 400 Gregorian years is a full calendar cycle, so if no
        // match is found within it, none will ever be found.
        let limit = ts.checked_add(400 * 366 * 86_400)?;

        while ts <= limit {
            let dt = civil_from_unix(ts);

            if !self.month.contains(dt.month) {
                // Jump to the first second of the next month.
                let (y, m) = if dt.month == 12 {
                    (dt.year + 1, 1)
                } else {
                    (dt.year, dt.month + 1)
                };
                ts = unix_from_civil(y, m, 1, 0, 0, 0)?;
                continue;
            }

            if !self.day_matches(dt.day, dt.weekday) {
                // Jump to the first second of the next day.
                ts = ts.checked_add(86_400 - (ts % 86_400))?;
                continue;
            }

            if !self.hour.contains(dt.hour) {
                // Jump to the first second of the next hour.
                ts = ts.checked_add(3_600 - (ts % 3_600))?;
                continue;
            }

            if !self.minute.contains(dt.minute) {
                // Jump to the first second of the next minute.
                ts = ts.checked_add(60)?;
                continue;
            }

            return Some(ts);
        }
        None
    }

    fn day_matches(&self, day: u32, weekday: u32) -> bool {
        let dom_match = self.day_of_month.contains(day);
        let dow_match = self.day_of_week.contains(weekday);
        if self.dom_star || self.dow_star {
            dom_match && dow_match
        } else {
            dom_match || dow_match
        }
    }

    /// Rejects expressions that can never fire because of the calendar
    /// (e.g. `* * 31 2 *`, `* * 30 2 *`, `* * 31 4 *`).
    ///
    /// The check is exact: it walks the selected months and asks whether at
    /// least one `(month, day)` pair can exist in *some* year. Because the
    /// Gregorian calendar repeats every 400 years and every 400-year cycle
    /// contains leap years, testing a leap year (`2000`) and a non-leap year
    /// (`2001`) is sufficient to decide reachability.
    fn validate_calendar(&self) -> Result<(), CronError> {
        // With POSIX OR semantics, a restricted day-of-week alone is enough
        // to guarantee that the expression fires: every week contains every
        // weekday. Only the `day_of_month`-driven case can be impossible.
        if !self.dow_star {
            return Ok(());
        }

        let months: Vec<u32> = self.month.iter().collect();
        let doms: Vec<u32> = self.day_of_month.iter().collect();

        // A `(month, day)` pair is reachable when some year has that day in
        // that month. Testing a leap year and a common year covers both the
        // `29 February` case and the `31 April` case.
        let reachable = doms.iter().any(|&dom| {
            months.iter().any(|&m| {
                dom <= days_in_month(2000, m) || dom <= days_in_month(2001, m)
            })
        });

        if reachable {
            return Ok(());
        }

        let dom_list = doms
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let month_list = months
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(",");
        Err(CronError::ImpossibleDate {
            reason: format!(
                "day-of-month {} never exists in month(s) {}",
                dom_list, month_list
            ),
        })
    }

    /// Returns a concise English description of the schedule.
    pub fn describe_en(&self) -> String {
        describe(self, Lang::En)
    }

    /// Returns a concise French description of the schedule.
    pub fn describe_fr(&self) -> String {
        describe(self, Lang::Fr)
    }
}

fn expand_macro(expr: &str) -> Result<&str, CronError> {
    if !expr.starts_with('@') {
        return Ok(expr);
    }
    let lower = expr.to_ascii_lowercase();
    let expanded = match lower.as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        _ => {
            return Err(CronError::UnknownMacro {
                name: expr.to_string(),
            })
        }
    };
    Ok(expanded)
}

// ---------------------------------------------------------------------------
// Description
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    En,
    Fr,
}

fn describe(s: &CronSchedule, lang: Lang) -> String {
    let mut parts: Vec<String> = Vec::new();

    // --- Time of day -------------------------------------------------------
    let time_part = describe_time(s, lang);
    parts.push(time_part);

    // --- Day condition -----------------------------------------------------
    if let Some(day) = describe_day(s, lang) {
        parts.push(day);
    }

    // --- Month condition ---------------------------------------------------
    if let Some(month) = describe_month(s, lang) {
        parts.push(month);
    }

    parts.join(", ")
}

fn describe_time(s: &CronSchedule, lang: Lang) -> String {
    let every_minute = s.minute.is_full();
    let every_hour = s.hour.is_full();

    // "Every N minutes"
    if every_hour {
        if every_minute {
            return match lang {
                Lang::En => "Every minute".to_string(),
                Lang::Fr => "Toutes les minutes".to_string(),
            };
        }
        if let Some(step) = s.minute.uniform_step() {
            if step > 1 && s.minute.min() == Some(0) {
                return match lang {
                    Lang::En => format!("Every {} minutes", step),
                    Lang::Fr => format!("Toutes les {} minutes", step),
                };
            }
        }
        if s.minute.count() == 1 {
            let m = s.minute.min().unwrap_or(0);
            return match lang {
                Lang::En => format!("At minute {} of every hour", m),
                Lang::Fr => format!("À la minute {} de chaque heure", m),
            };
        }
        return match lang {
            Lang::En => format!("At minutes {} of every hour", list(&s.minute)),
            Lang::Fr => format!("Aux minutes {} de chaque heure", list(&s.minute)),
        };
    }

    // Hour is restricted.
    if every_minute {
        if let Some(step) = s.hour.uniform_step() {
            if step > 1 && s.hour.min() == Some(0) {
                return match lang {
                    Lang::En => format!("Every {} hours", step),
                    Lang::Fr => format!("Toutes les {} heures", step),
                };
            }
        }
        return match lang {
            Lang::En => format!("Every minute during hours {}", list(&s.hour)),
            Lang::Fr => format!("Chaque minute pendant les heures {}", list(&s.hour)),
        };
    }

    // Both restricted: list the concrete times when reasonable.
    let times = concrete_times(s);
    if let Some(times) = times {
        return match lang {
            Lang::En => format!("At {}", times),
            Lang::Fr => format!("À {}", times),
        };
    }

    match lang {
        Lang::En => format!(
            "At minutes {} during hours {}",
            list(&s.minute),
            list(&s.hour)
        ),
        Lang::Fr => format!(
            "Aux minutes {} pendant les heures {}",
            list(&s.minute),
            list(&s.hour)
        ),
    }
}

/// Builds a `HH:MM` list when the cartesian product stays small.
fn concrete_times(s: &CronSchedule) -> Option<String> {
    let hours: Vec<u32> = s.hour.iter().collect();
    let minutes: Vec<u32> = s.minute.iter().collect();
    if hours.len() * minutes.len() > 8 {
        return None;
    }
    let mut out = Vec::new();
    for h in hours {
        for &m in &minutes {
            out.push(format!("{:02}:{:02}", h, m));
        }
    }
    Some(out.join(", "))
}

fn describe_day(s: &CronSchedule, lang: Lang) -> Option<String> {
    let dom_restricted = !s.dom_star;
    let dow_restricted = !s.dow_star;

    if !dom_restricted && !dow_restricted {
        return None;
    }

    if dom_restricted && dow_restricted {
        // POSIX OR semantics.
        let dom = describe_dom(s, lang);
        let dow = describe_dow(s, lang);
        return Some(match lang {
            Lang::En => format!("on {} or {}", dom, dow),
            Lang::Fr => format!("le {} ou le {}", dom, dow),
        });
    }

    if dom_restricted {
        Some(describe_dom(s, lang))
    } else {
        Some(describe_dow(s, lang))
    }
}

fn describe_dom(s: &CronSchedule, lang: Lang) -> String {
    let f = &s.day_of_month;
    if let Some(step) = f.uniform_step() {
        if step > 1 && f.min() == Some(1) {
            return match lang {
                Lang::En => format!("every {} days of the month", step),
                Lang::Fr => format!("tous les {} jours du mois", step),
            };
        }
    }
    if f.is_contiguous() {
        let (lo, hi) = (f.min().unwrap_or(1), f.max().unwrap_or(1));
        if lo == hi {
            return match lang {
                Lang::En => format!("day {} of the month", lo),
                Lang::Fr => format!("{} du mois", ordinal_fr(lo)),
            };
        }
        return match lang {
            Lang::En => format!("day {} through {} of the month", lo, hi),
            Lang::Fr => format!("du {} au {} du mois", ordinal_fr(lo), ordinal_fr(hi)),
        };
    }
    match lang {
        Lang::En => format!("days {} of the month", list(f)),
        Lang::Fr => format!("les {} du mois", list(f)),
    }
}

fn describe_dow(s: &CronSchedule, lang: Lang) -> String {
    let f = &s.day_of_week;
    if f.is_contiguous() {
        let (lo, hi) = (f.min().unwrap_or(0), f.max().unwrap_or(0));
        if lo == hi {
            return match lang {
                Lang::En => format!("on {}", dow_name(lo, lang)),
                Lang::Fr => format!("le {}", dow_name(lo, lang)),
            };
        }
        return match lang {
            Lang::En => format!("{} through {}", dow_name(lo, lang), dow_name(hi, lang)),
            Lang::Fr => format!("du {} au {}", dow_name(lo, lang), dow_name(hi, lang)),
        };
    }
    let names: Vec<&str> = f.iter().map(|d| dow_name(d, lang)).collect();
    match lang {
        Lang::En => format!("on {}", names.join(", ")),
        Lang::Fr => format!("le {}", names.join(", ")),
    }
}

fn describe_month(s: &CronSchedule, lang: Lang) -> Option<String> {
    let f = &s.month;
    if f.is_full() {
        return None;
    }
    if f.is_contiguous() {
        let (lo, hi) = (f.min().unwrap_or(1), f.max().unwrap_or(1));
        if lo == hi {
            return Some(match lang {
                Lang::En => format!("in {}", month_name(lo, lang)),
                Lang::Fr => format!("en {}", month_name(lo, lang)),
            });
        }
        return Some(match lang {
            Lang::En => format!("from {} through {}", month_name(lo, lang), month_name(hi, lang)),
            Lang::Fr => format!("de {} à {}", month_name(lo, lang), month_name(hi, lang)),
        });
    }
    let names: Vec<&str> = f.iter().map(|m| month_name(m, lang)).collect();
    Some(match lang {
        Lang::En => format!("in {}", names.join(", ")),
        Lang::Fr => format!("en {}", names.join(", ")),
    })
}

fn list(f: &Field) -> String {
    let items: Vec<String> = f.iter().map(|v| v.to_string()).collect();
    items.join(", ")
}

fn dow_name(d: u32, lang: Lang) -> &'static str {
    const EN: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const FR: [&str; 7] = [
        "dimanche",
        "lundi",
        "mardi",
        "mercredi",
        "jeudi",
        "vendredi",
        "samedi",
    ];
    let idx = (d % 7) as usize;
    match lang {
        Lang::En => EN[idx],
        Lang::Fr => FR[idx],
    }
}

fn month_name(m: u32, lang: Lang) -> &'static str {
    const EN: [&str; 12] = [
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
    const FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    let idx = (m.saturating_sub(1) as usize).min(11);
    match lang {
        Lang::En => EN[idx],
        Lang::Fr => FR[idx],
    }
}

fn ordinal_fr(d: u32) -> String {
    if d == 1 {
        "1er".to_string()
    } else {
        d.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> u64 {
        unix_from_civil(y, mo, d, h, mi, 0).expect("valid civil date")
    }

    #[test]
    fn parses_all_macros_case_insensitively() {
        for (m, expected) in [
            ("@yearly", "0 0 1 1 *"),
            ("@ANNUALLY", "0 0 1 1 *"),
            ("@monthly", "0 0 1 * *"),
            ("@weekly", "0 0 * * 0"),
            ("@daily", "0 0 * * *"),
            ("@Midnight", "0 0 * * *"),
            ("@hourly", "0 * * * *"),
        ] {
            let s = CronSchedule::parse(m).unwrap();
            assert_eq!(s.source(), expected, "macro {m}");
        }
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        assert_eq!(CronSchedule::parse("").unwrap_err(), CronError::EmptyExpression);
        assert_eq!(
            CronSchedule::parse("   \t\n ").unwrap_err(),
            CronError::EmptyExpression
        );
        assert_eq!(
            CronSchedule::parse("* * * *").unwrap_err(),
            CronError::InvalidFieldCount { found: 4 }
        );
        assert_eq!(
            CronSchedule::parse("* * * * * *").unwrap_err(),
            CronError::InvalidFieldCount { found: 6 }
        );
        assert!(matches!(
            CronSchedule::parse("@fortnightly").unwrap_err(),
            CronError::UnknownMacro { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("*/0 * * * *").unwrap_err(),
            CronError::ZeroStep { field: "minute" }
        ));
        assert!(matches!(
            CronSchedule::parse("0 17-9 * * *").unwrap_err(),
            CronError::ReversedRange { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("60 * * * *").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("0 24 * * *").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("0 0 0 * *").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("0 0 * 13 *").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        assert!(matches!(
            CronSchedule::parse("0 0 * * 8").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        // Unicode / garbage must not panic.
        assert!(CronSchedule::parse("é é é é é").is_err());
        assert!(CronSchedule::parse("0 0 * * 🦀").is_err());
        assert!(CronSchedule::parse("0 0 * * *\u{0}").is_err());
    }

    #[test]
    fn rejects_impossible_calendar_dates() {
        for expr in ["* * 31 2 *", "* * 30 2 *", "* * 31 4 *", "* * 31 6 *", "* * 31 9 *", "* * 31 11 *"] {
            assert!(
                matches!(
                    CronSchedule::parse(expr).unwrap_err(),
                    CronError::ImpossibleDate { .. }
                ),
                "expected ImpossibleDate for {expr}"
            );
        }
        // 29 February is possible (leap years exist).
        assert!(CronSchedule::parse("* * 29 2 *").is_ok());
        // 31 in a 31-day month is fine.
        assert!(CronSchedule::parse("* * 31 1 *").is_ok());
        // 31 in a month list containing a 31-day month is fine.
        assert!(CronSchedule::parse("* * 31 2,3 *").is_ok());
        // A restricted day-of-week makes the expression fireable (POSIX OR).
        assert!(CronSchedule::parse("* * 31 2 MON").is_ok());
    }

    #[test]
    fn matches_uses_utc_minute() {
        let s = CronSchedule::parse("30 12 * * *").unwrap();
        let t = ts(2024, 3, 15, 12, 30);
        assert!(s.matches(t));
        assert!(s.matches(t + 59)); // seconds inside the minute are ignored
        assert!(!s.matches(t + 60));
        assert!(!s.matches(ts(2024, 3, 15, 12, 29)));
    }

    #[test]
    fn next_after_is_strictly_after_the_minute() {
        let s = CronSchedule::parse("0 * * * *").unwrap();
        let t = ts(2024, 1, 1, 10, 0);
        assert_eq!(s.next_after(t), Some(ts(2024, 1, 1, 11, 0)));
        // Inside the matching minute: next occurrence is the following hour.
        assert_eq!(s.next_after(t + 30), Some(ts(2024, 1, 1, 11, 0)));
        // Just before: same minute is skipped, next hour returned.
        assert_eq!(s.next_after(t - 1), Some(t));
    }

    #[test]
    fn next_after_weekday_and_time() {
        // 09:00 Monday through Friday.
        let s = CronSchedule::parse("0 9 * * MON-FRI").unwrap();
        // 2024-03-15 is a Friday.
        let friday = ts(2024, 3, 15, 9, 0);
        assert_eq!(s.next_after(friday), Some(ts(2024, 3, 18, 9, 0))); // Monday
        assert_eq!(s.next_after(ts(2024, 3, 16, 0, 0)), Some(ts(2024, 3, 18, 9, 0)));
    }

    #[test]
    fn next_after_leap_day_across_four_years_is_fast() {
        // 29 February at midnight: next occurrence is 4 years later.
        let s = CronSchedule::parse("0 0 29 2 *").unwrap();
        let start = ts(2024, 2, 29, 0, 0);
        let expected = ts(2028, 2, 29, 0, 0);
        let begin = std::time::Instant::now();
        let got = s.next_after(start);
        let elapsed = begin.elapsed();
        assert_eq!(got, Some(expected));
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "next_after took {elapsed:?}, expected a jump-based search"
        );
    }

    #[test]
    fn next_after_skips_non_leap_century() {
        // 29 February 2096 -> 2104 (2100 is not a leap year).
        let s = CronSchedule::parse("0 0 29 2 *").unwrap();
        let start = ts(2096, 2, 29, 0, 0);
        assert_eq!(s.next_after(start), Some(ts(2104, 2, 29, 0, 0)));
    }

    #[test]
    fn next_after_handles_year_boundary() {
        let s = CronSchedule::parse("0 0 1 1 *").unwrap();
        assert_eq!(
            s.next_after(ts(2024, 6, 1, 0, 0)),
            Some(ts(2025, 1, 1, 0, 0))
        );
    }

    #[test]
    fn next_after_extreme_timestamps_do_not_panic() {
        let s = CronSchedule::parse("0 0 1 1 *").unwrap();
        let _ = s.next_after(u64::MAX);
        let _ = s.next_after(u64::MAX - 1);
        let _ = s.next_after(0);
        let s = CronSchedule::parse("* * * * *").unwrap();
        assert_eq!(s.next_after(0), Some(60));
    }

    #[test]
    fn posix_dom_dow_or_semantics() {
        // Both restricted -> OR.
        let s = CronSchedule::parse("0 0 13 * FRI").unwrap();
        // 2024-09-13 is a Friday (both match).
        assert!(s.matches(ts(2024, 9, 13, 0, 0)));
        // 2024-09-06 is a Friday, not the 13th -> matches via DOW.
        assert!(s.matches(ts(2024, 9, 6, 0, 0)));
        // 2024-10-13 is a Sunday, not a Friday -> matches via DOM.
        assert!(s.matches(ts(2024, 10, 13, 0, 0)));
        // 2024-10-14 is a Monday and not the 13th -> no match.
        assert!(!s.matches(ts(2024, 10, 14, 0, 0)));

        // DOM restricted, DOW wildcard -> AND (i.e. only DOM matters).
        let s = CronSchedule::parse("0 0 13 * *").unwrap();
        assert!(s.matches(ts(2024, 10, 13, 0, 0)));
        assert!(!s.matches(ts(2024, 10, 14, 0, 0)));

        // DOW restricted, DOM wildcard -> AND (i.e. only DOW matters).
        let s = CronSchedule::parse("0 0 * * FRI").unwrap();
        assert!(s.matches(ts(2024, 9, 6, 0, 0)));
        assert!(!s.matches(ts(2024, 9, 7, 0, 0)));
    }

    #[test]
    fn sunday_seven_is_normalized() {
        let a = CronSchedule::parse("0 0 * * 7").unwrap();
        let b = CronSchedule::parse("0 0 * * 0").unwrap();
        assert_eq!(a, b);
        // 2024-03-17 is a Sunday.
        assert!(a.matches(ts(2024, 3, 17, 0, 0)));
    }

    #[test]
    fn aliases_are_case_insensitive() {
        let a = CronSchedule::parse("0 0 * jan-mar mon-fri").unwrap();
        let b = CronSchedule::parse("0 0 * JAN-MAR MON-FRI").unwrap();
        assert_eq!(a, b);
        assert!(a.matches(ts(2024, 2, 5, 0, 0))); // Monday in February
        assert!(!a.matches(ts(2024, 4, 1, 0, 0))); // April
    }

    #[test]
    fn describe_en_examples() {
        assert_eq!(
            CronSchedule::parse("*/15 * * * *").unwrap().describe_en(),
            "Every 15 minutes"
        );
        assert_eq!(
            CronSchedule::parse("0 9 * * MON-FRI").unwrap().describe_en(),
            "At 09:00, Monday through Friday"
        );
        assert_eq!(
            CronSchedule::parse("@daily").unwrap().describe_en(),
            "At 00:00"
        );
        assert_eq!(
            CronSchedule::parse("0 0 1 1 *").unwrap().describe_en(),
            "At 00:00, day 1 of the month, in January"
        );
    }

    #[test]
    fn describe_fr_examples() {
        assert_eq!(
            CronSchedule::parse("*/15 * * * *").unwrap().describe_fr(),
            "Toutes les 15 minutes"
        );
        assert_eq!(
            CronSchedule::parse("0 9 * * MON-FRI").unwrap().describe_fr(),
            "À 09:00, du lundi au vendredi"
        );
        assert_eq!(
            CronSchedule::parse("@daily").unwrap().describe_fr(),
            "À 00:00"
        );
    }

    #[test]
    fn describe_never_panics_on_any_valid_expression() {
        for expr in [
            "* * * * *",
            "0 0 1 1 *",
            "59 23 31 12 6",
            "*/7 */5 */3 */2 */2",
            "1,2,3 4,5 6,7 8,9 0,1,2,3,4,5,6",
        ] {
            let s = CronSchedule::parse(expr).unwrap();
            assert!(!s.describe_en().is_empty());
            assert!(!s.describe_fr().is_empty());
        }
    }
}
