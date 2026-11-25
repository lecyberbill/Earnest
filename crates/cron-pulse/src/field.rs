//! Parsing of a single cron field into a compact bitset.
//!
//! A field is a comma-separated list of items. Each item is one of:
//!
//! * `*`            — the whole range,
//! * `N`            — a single value,
//! * `A-B`          — an inclusive range,
//! * `*/S`          — the whole range with a step,
//! * `A-B/S`        — a range with a step,
//! * `N/S`          — a single value with a step (POSIX extension, `N-max/S`).
//!
//! Alphabetic aliases are accepted for the month and day-of-week fields and are
//! matched case-insensitively.

use crate::error::CronError;

/// Maximum number of distinct values a field can hold (0..=59 for minutes).
const MAX_BITS: usize = 64;

/// A parsed cron field: a bitset of accepted values plus the raw text.
///
/// Equality is *semantic*: two fields are equal when they accept the same
/// values and carry the same structural flags. The raw source text is
/// deliberately excluded so that `7` and `0` (both Sunday) or `jan` and `JAN`
/// compare equal.
#[derive(Debug, Clone)]
pub(crate) struct Field {
    /// Bitset of accepted values; bit `i` set means value `i` is accepted.
    bits: u64,
    /// Whether the field was written as a bare `*` (used for POSIX DOM/DOW).
    is_star: bool,
    /// Whether the field was written as `*/S` (a step over the whole range).
    is_step_over_all: bool,
    /// The step value when the field is a single `*/S` item, if any.
    step: Option<u32>,
}

impl PartialEq for Field {
    fn eq(&self, other: &Self) -> bool {
        self.bits == other.bits
            && self.is_star == other.is_star
            && self.is_step_over_all == other.is_step_over_all
            && self.step == other.step
    }
}

impl Eq for Field {}

impl Field {
    /// Returns `true` when `value` is accepted by this field.
    #[inline]
    pub(crate) fn contains(&self, value: u32) -> bool {
        value < MAX_BITS as u32 && (self.bits >> value) & 1 == 1
    }

    /// Returns `true` when the field was written as a bare `*`.
    #[inline]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_star(&self) -> bool {
        self.is_star
    }

    /// Returns `true` when the field is a single `*/S` item.
    #[inline]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_step_over_all(&self) -> bool {
        self.is_step_over_all
    }

    /// Returns the step of a `*/S` field, if the field is exactly that.
    #[inline]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn step(&self) -> Option<u32> {
        self.step
    }

    /// Returns the sorted list of accepted values.
    pub(crate) fn values(&self) -> Vec<u32> {
        (0..MAX_BITS as u32)
            .filter(|v| self.contains(*v))
            .collect()
    }

    /// Iterates over the accepted values in ascending order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0..MAX_BITS as u32).filter(move |v| self.contains(*v))
    }

    /// Returns `true` when the field accepts every value of its domain
    /// (either written as `*` or as an equivalent full range).
    pub(crate) fn is_full(&self) -> bool {
        self.is_star
    }

    /// Returns `true` when the field was written as a bare `*` (POSIX DOM/DOW
    /// semantics rely on this, not on the accepted value set).
    #[inline]
    pub(crate) fn is_wildcard(&self) -> bool {
        self.is_star
    }

    /// Returns the number of accepted values.
    pub(crate) fn count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// Returns the smallest accepted value, if any.
    pub(crate) fn min(&self) -> Option<u32> {
        if self.bits == 0 {
            None
        } else {
            Some(self.bits.trailing_zeros())
        }
    }

    /// Returns the largest accepted value, if any.
    pub(crate) fn max(&self) -> Option<u32> {
        if self.bits == 0 {
            None
        } else {
            Some(63 - self.bits.leading_zeros())
        }
    }

    /// Returns `true` when the accepted values form a contiguous run.
    pub(crate) fn is_contiguous(&self) -> bool {
        match (self.min(), self.max()) {
            (Some(lo), Some(hi)) => {
                let width = hi - lo + 1;
                self.count() == width
            }
            _ => false,
        }
    }

    /// Returns `true` when the accepted values form an arithmetic progression
    /// with a constant step greater than one.
    pub(crate) fn uniform_step(&self) -> Option<u32> {
        let values = self.values();
        if values.len() < 2 {
            return None;
        }
        let step = values[1] - values[0];
        if step <= 1 {
            return None;
        }
        for w in values.windows(2) {
            if w[1] - w[0] != step {
                return None;
            }
        }
        Some(step)
    }
}

/// Describes the accepted value domain of a field.
pub(crate) struct FieldSpec {
    /// Human readable field name used in error messages.
    pub(crate) name: &'static str,
    /// Lowest accepted value.
    pub(crate) min: u32,
    /// Highest accepted value.
    pub(crate) max: u32,
    /// Optional alphabetic aliases (lowercase name, value).
    pub(crate) aliases: &'static [(&'static str, u32)],
    /// When `true`, the value `7` is accepted as an alias of `0` (day of week).
    pub(crate) fold_seven_to_zero: bool,
}

impl FieldSpec {
    /// The `minute` field: `0..=59`.
    pub(crate) const MINUTE: FieldSpec = FieldSpec {
        name: "minute",
        min: 0,
        max: 59,
        aliases: &[],
        fold_seven_to_zero: false,
    };

    /// The `hour` field: `0..=23`.
    pub(crate) const HOUR: FieldSpec = FieldSpec {
        name: "hour",
        min: 0,
        max: 23,
        aliases: &[],
        fold_seven_to_zero: false,
    };

    /// The `day_of_month` field: `1..=31`.
    pub(crate) const DAY_OF_MONTH: FieldSpec = FieldSpec {
        name: "day_of_month",
        min: 1,
        max: 31,
        aliases: &[],
        fold_seven_to_zero: false,
    };

    /// The `month` field: `1..=12`, with `JAN..DEC` aliases.
    pub(crate) const MONTH: FieldSpec = FieldSpec {
        name: "month",
        min: 1,
        max: 12,
        aliases: &[
            ("jan", 1),
            ("feb", 2),
            ("mar", 3),
            ("apr", 4),
            ("may", 5),
            ("jun", 6),
            ("jul", 7),
            ("aug", 8),
            ("sep", 9),
            ("oct", 10),
            ("nov", 11),
            ("dec", 12),
        ],
        fold_seven_to_zero: false,
    };

    /// The `day_of_week` field: `0..=7` (`7` folds to `0`), with `SUN..SAT` aliases.
    pub(crate) const DAY_OF_WEEK: FieldSpec = FieldSpec {
        name: "day_of_week",
        min: 0,
        max: 7,
        aliases: &[
            ("sun", 0),
            ("mon", 1),
            ("tue", 2),
            ("wed", 3),
            ("thu", 4),
            ("fri", 5),
            ("sat", 6),
        ],
        fold_seven_to_zero: true,
    };

    /// Resolves a single token to a numeric value, honouring aliases.
    fn resolve(&self, token: &str) -> Result<u32, CronError> {
        if token.is_empty() {
            return Err(CronError::EmptyItem {
                field: self.name,
            });
        }
        if token.bytes().all(|b| b.is_ascii_digit()) {
            let value: u32 = token.parse().map_err(|_| CronError::ValueOutOfRange {
                field: self.name,
                value: token.to_string(),
                min: self.min,
                max: self.max,
            })?;
            return self.check_range(value);
        }
        let lowered = token.to_ascii_lowercase();
        for (alias, value) in self.aliases {
            if *alias == lowered {
                return Ok(*value);
            }
        }
        Err(CronError::UnknownAlias {
            field: self.name,
            token: token.to_string(),
        })
    }

    /// Validates that `value` belongs to the field domain.
    fn check_range(&self, value: u32) -> Result<u32, CronError> {
        if value < self.min || value > self.max {
            return Err(CronError::ValueOutOfRange {
                field: self.name,
                value: value.to_string(),
                min: self.min,
                max: self.max,
            });
        }
        if self.fold_seven_to_zero && value == 7 {
            return Ok(0);
        }
        Ok(value)
    }

    /// Parses the whole field text into a [`Field`].
    pub(crate) fn parse(&self, text: &str) -> Result<Field, CronError> {
        let raw = text.trim();
        if raw.is_empty() {
            return Err(CronError::EmptyItem {
                field: self.name,
            });
        }

        let mut bits: u64 = 0;
        let mut is_star = false;
        let mut is_step_over_all = false;
        let mut step: Option<u32> = None;
        let mut item_count = 0usize;

        for item in raw.split(',') {
            item_count += 1;
            let item = item.trim();
            if item.is_empty() {
                return Err(CronError::EmptyItem {
                    field: self.name,
                });
            }

            // Split off an optional `/step` suffix.
            let (base, step_part) = match item.split_once('/') {
                Some((b, s)) => (b.trim(), Some(s.trim())),
                None => (item, None),
            };

            let parsed_step = match step_part {
                Some(s) => {
                    if s.is_empty() {
                        return Err(CronError::EmptyItem {
                            field: self.name,
                        });
                    }
                    if !s.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(CronError::InvalidStep {
                            field: self.name,
                            value: s.to_string(),
                        });
                    }
                    let v: u32 = s.parse().map_err(|_| CronError::InvalidStep {
                        field: self.name,
                        value: s.to_string(),
                    })?;
                    if v == 0 {
                        return Err(CronError::ZeroStep {
                            field: self.name,
                        });
                    }
                    Some(v)
                }
                None => None,
            };

            let (lo, hi) = if base == "*" {
                (self.min, self.max)
            } else if let Some((a, b)) = base.split_once('-') {
                let a = a.trim();
                let b = b.trim();
                if a.is_empty() || b.is_empty() {
                    return Err(CronError::EmptyItem {
                        field: self.name,
                    });
                }
                let lo = self.resolve(a)?;
                let hi = self.resolve(b)?;
                if lo > hi {
                    return Err(CronError::ReversedRange {
                        field: self.name,
                        start: lo,
                        end: hi,
                    });
                }
                (lo, hi)
            } else {
                let single = self.resolve(base)?;
                match parsed_step {
                    // `N/S` is a POSIX extension meaning `N-max/S`.
                    Some(_) => (single, self.max),
                    None => (single, single),
                }
            };

            let stride = parsed_step.unwrap_or(1);
            let mut value = lo;
            while value <= hi {
                if value < MAX_BITS as u32 {
                    bits |= 1u64 << value;
                }
                match value.checked_add(stride) {
                    Some(next) => value = next,
                    None => break,
                }
            }

            if item_count == 1 {
                is_star = base == "*" && parsed_step.is_none();
                is_step_over_all = base == "*" && parsed_step.is_some();
                step = if base == "*" { parsed_step } else { None };
            } else {
                // A list is never a bare `*` nor a pure step field.
                is_star = false;
                is_step_over_all = false;
                step = None;
            }
        }

        if bits == 0 {
            return Err(CronError::EmptyItem {
                field: self.name,
            });
        }

        Ok(Field {
            bits,
            is_star,
            is_step_over_all,
            step,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: FieldSpec = FieldSpec {
        name: "minute",
        min: 0,
        max: 59,
        aliases: &[],
        fold_seven_to_zero: false,
    };

    const MONTH: FieldSpec = FieldSpec {
        name: "month",
        min: 1,
        max: 12,
        aliases: &[
            ("jan", 1),
            ("feb", 2),
            ("mar", 3),
            ("apr", 4),
            ("may", 5),
            ("jun", 6),
            ("jul", 7),
            ("aug", 8),
            ("sep", 9),
            ("oct", 10),
            ("nov", 11),
            ("dec", 12),
        ],
        fold_seven_to_zero: false,
    };

    const DOW: FieldSpec = FieldSpec {
        name: "day_of_week",
        min: 0,
        max: 7,
        aliases: &[
            ("sun", 0),
            ("mon", 1),
            ("tue", 2),
            ("wed", 3),
            ("thu", 4),
            ("fri", 5),
            ("sat", 6),
        ],
        fold_seven_to_zero: true,
    };

    #[test]
    fn star_matches_everything() {
        let f = MINUTE.parse("*").unwrap();
        assert!(f.is_star());
        assert_eq!(f.count(), 60);
        assert!(f.contains(0) && f.contains(59));
    }

    #[test]
    fn single_and_list() {
        let f = MINUTE.parse("5").unwrap();
        assert_eq!(f.values(), vec![5]);
        let f = MINUTE.parse("1,15,30").unwrap();
        assert_eq!(f.values(), vec![1, 15, 30]);
    }

    #[test]
    fn ranges_and_steps() {
        let f = MINUTE.parse("9-17").unwrap();
        assert_eq!(f.values(), (9..=17).collect::<Vec<_>>());
        let f = MINUTE.parse("*/15").unwrap();
        assert_eq!(f.values(), vec![0, 15, 30, 45]);
        assert!(f.is_step_over_all());
        assert_eq!(f.step(), Some(15));
        let f = MINUTE.parse("9-17/2").unwrap();
        assert_eq!(f.values(), vec![9, 11, 13, 15, 17]);
    }

    #[test]
    fn aliases_are_case_insensitive() {
        let f = MONTH.parse("jan,MAR,Dec").unwrap();
        assert_eq!(f.values(), vec![1, 3, 12]);
        let f = DOW.parse("sun").unwrap();
        assert_eq!(f.values(), vec![0]);
        let f = DOW.parse("7").unwrap();
        assert_eq!(f.values(), vec![0]);
        let f = DOW.parse("MON-FRI").unwrap();
        assert_eq!(f.values(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn rejects_invalid_syntax() {
        assert_eq!(
            MINUTE.parse("*/0").unwrap_err(),
            CronError::ZeroStep { field: "minute" }
        );
        assert_eq!(
            MINUTE.parse("17-9").unwrap_err(),
            CronError::ReversedRange {
                field: "minute",
                start: 17,
                end: 9
            }
        );
        assert!(matches!(
            MINUTE.parse("60").unwrap_err(),
            CronError::ValueOutOfRange { .. }
        ));
        assert!(matches!(
            MINUTE.parse("1,,2").unwrap_err(),
            CronError::EmptyItem { .. }
        ));
        assert!(matches!(
            MONTH.parse("foo").unwrap_err(),
            CronError::UnknownAlias { .. }
        ));
        assert!(matches!(
            MINUTE.parse("1-2-3").unwrap_err(),
            CronError::UnknownAlias { .. }
        ));
    }

    #[test]
    fn shape_helpers() {
        let f = MINUTE.parse("0-59").unwrap();
        assert!(f.is_contiguous());
        assert_eq!(f.uniform_step(), None);
        let f = MINUTE.parse("*/5").unwrap();
        assert_eq!(f.uniform_step(), Some(5));
        assert!(!f.is_contiguous());
        let f = MINUTE.parse("0").unwrap();
        assert!(f.is_contiguous());
        assert_eq!(f.min(), Some(0));
        assert_eq!(f.max(), Some(0));
    }
}
