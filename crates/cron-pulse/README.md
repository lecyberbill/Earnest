# cron_pulse

A pure-Rust, `std`-only library to **parse**, **validate**, **schedule** and
**describe** 5-field cron expressions evaluated in **UTC**.

* Zero external dependency, no `unsafe` (`#![forbid(unsafe_code)]`).
* Full proleptic Gregorian calendar with the correct leap-year rule
  (2024 and 2000 are leap years, 1900 and 2100 are not).
* Fast `next_after` search: it jumps by month / day / hour instead of scanning
  minute by minute, so a 4-year gap between two February 29ths is resolved in
  microseconds.
* POSIX day-of-month / day-of-week semantics (logical OR when both fields are
  restricted, logical AND otherwise).
* Human-readable descriptions in English and French.

## Installation

```toml
[dependencies]
cron_pulse = "0.1"
```

## Quick start

```rust
use cron_pulse::CronSchedule;

let schedule = CronSchedule::parse("*/15 9-17 * * MON-FRI").unwrap();

// 2023-11-14T12:00:00Z (1_699_963_200) is a Tuesday, inside 09:00-17:59.
assert!(schedule.matches(1_699_963_200));

// Next occurrence strictly after that minute, aligned on second :00.
let next = schedule.next_after(1_699_963_200).unwrap();
assert_eq!(next % 60, 0);

println!("{}", schedule.describe_en());
// "Every 15 minutes, 09:00 through 17:59, Monday through Friday"
println!("{}", schedule.describe_fr());
```

## Supported syntax

| Field | Range | Aliases |
|-------|-------|---------|
| minute | `0-59` | — |
| hour | `0-23` | — |
| day of month | `1-31` | — |
| month | `1-12` | `JAN`..`DEC` |
| day of week | `0-6` (`7` = Sunday) | `SUN`..`SAT` |

Operators per field:

* `*` — every value
* `5` — a single value
* `1,15,30` — a list
* `9-17` — an inclusive range
* `*/15` — a step over the whole range
* `9-17/2` — a step over a range
* `5/10` — a step starting at a single value

Aliases are case-insensitive (`jan`, `Jan`, `JAN`). The value `7` in the
day-of-week field is accepted and normalized to `0` (Sunday).

Macros (case-insensitive):

| Macro | Expansion |
|-------|-----------|
| `@yearly`, `@annually` | `0 0 1 1 *` |
| `@monthly` | `0 0 1 * *` |
| `@weekly` | `0 0 * * 0` |
| `@daily`, `@midnight` | `0 0 * * *` |
| `@hourly` | `0 * * * *` |

## Validation

Parsing is strict. The following are rejected with a detailed `CronError`:

* empty expressions and wrong field counts;
* unknown macros;
* empty list items (`1,,2`), non-numeric tokens, unknown aliases;
* out-of-range values (`60` minutes, `0` day of month, `13` month, `8` weekday);
* zero steps (`*/0`);
* reversed ranges (`17-9`);
* calendar dates that can never occur, such as `* * 31 2 *` or `* * 31 4 *`.

`CronError` implements `Debug`, `Clone`, `PartialEq`, `Eq`, `Display` and
`std::error::Error`.

## API

```rust
impl CronSchedule {
    pub fn parse(expr: &str) -> Result<CronSchedule, CronError>;
    pub fn matches(&self, unix_secs: u64) -> bool;
    pub fn next_after(&self, after_unix_secs: u64) -> Option<u64>;
    pub fn describe_en(&self) -> String;
    pub fn describe_fr(&self) -> String;
}
```

* `matches` checks whether the UTC minute containing `unix_secs` matches the
  schedule (seconds are ignored).
* `next_after` returns the next matching timestamp, aligned on second `:00` of
  the minute, strictly after the minute of `after_unix_secs`. It returns `None`
  when no occurrence exists within a full 400-year Gregorian cycle.

The civil calendar helpers are also public:

```rust
pub fn civil_from_unix(unix_secs: u64) -> CivilDateTime;
pub fn unix_from_civil(year: i64, month: u32, day: u32,
                       hour: u32, minute: u32, second: u32) -> Option<u64>;
```

## Robustness

The library never panics on malformed input, empty strings, Unicode or extreme
timestamps (`u64::MAX` included). All arithmetic uses checked operations.

## License

Licensed under either of MIT or Apache-2.0, at your option.
