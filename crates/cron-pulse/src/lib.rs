//! # cron_pulse
//!
//! A pure-Rust, `std`-only library to **parse**, **validate**, **schedule** and
//! **describe** 5-field cron expressions evaluated in **UTC**.
//!
//! * No external dependency, no `unsafe` (`#![forbid(unsafe_code)]`).
//! * Full proleptic Gregorian calendar (correct leap-year rule).
//! * Fast `next_after` search: it jumps by month / day / hour instead of
//!   scanning minute by minute.
//! * POSIX day-of-month / day-of-week semantics (logical OR when both are
//!   restricted).
//! * Human-readable descriptions in English and French.
//!
//! ## Quick start
//!
//! ```
//! use cron_pulse::CronSchedule;
//!
//! let schedule = CronSchedule::parse("*/15 9-17 * * MON-FRI").unwrap();
//!
//! // 2023-11-14T12:00:00Z (1_699_963_200) is a Tuesday, inside 09:00-17:59.
//! assert!(schedule.matches(1_699_963_200));
//!
//! // Next occurrence strictly after that minute.
//! let next = schedule.next_after(1_699_963_200).unwrap();
//! assert_eq!(next % 60, 0);
//!
//! println!("{}", schedule.describe_en()); // "Every 15 minutes, 09:00 through 17:59, Monday through Friday"
//! println!("{}", schedule.describe_fr());
//! ```
//!
//! ## Supported syntax
//!
//! | Field | Range | Aliases |
//! |-------|-------|---------|
//! | minute | `0-59` | — |
//! | hour | `0-23` | — |
//! | day of month | `1-31` | — |
//! | month | `1-12` | `JAN`..`DEC` |
//! | day of week | `0-6` (`7` = Sunday) | `SUN`..`SAT` |
//!
//! Operators: `*`, `N`, `A-B`, `A,B,C`, `*/S`, `A-B/S`, `N/S`.
//!
//! Macros (case-insensitive): `@yearly` / `@annually`, `@monthly`, `@weekly`,
//! `@daily` / `@midnight`, `@hourly`.
//!
//! ## Errors
//!
//! Every fallible operation returns a detailed [`CronError`]. The library never
//! panics on malformed input, empty strings, Unicode or extreme timestamps.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod calendar;
mod error;
mod field;
mod schedule;

pub use crate::calendar::{civil_from_unix, unix_from_civil, CivilDateTime};
pub use crate::error::CronError;
pub use crate::schedule::CronSchedule;

/// Crate version, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
