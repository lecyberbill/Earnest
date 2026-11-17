//! `retry-budget` — a zero-dependency sliding-window retry budget with
//! circuit-breaker semantics.
//!
//! The budget keeps the last `window_size` outcomes (successes and failures) in
//! a FIFO queue. The circuit is **open** — retries are denied — as soon as the
//! number of failures inside the window reaches `max_failures`. Old outcomes are
//! evicted as new ones arrive, so the budget recovers on its own once enough
//! successes have pushed the failures out of the window.
//!
//! Degenerate configurations are normalised instead of panicking:
//!
//! * `window_size == 0` disables the budget: nothing is recorded, and retries
//!   are permanently denied (fail-safe default).
//! * `max_failures == 0` is clamped to `1`: the circuit opens on the very first
//!   failure.
//!
//! The type never panics, never pre-allocates from user-provided capacities (an
//! `usize::MAX` window is therefore harmless) and is `Send + Sync` so it can be
//! shared behind an `Arc<Mutex<_>>` or kept per-worker.
//!
//! ```
//! use retry_budget::RetryBudget;
//!
//! let mut budget = RetryBudget::new(10, 3);
//! assert!(budget.is_retry_allowed());
//!
//! budget.record_failure();
//! budget.record_failure();
//! budget.record_failure();
//! assert!(!budget.is_retry_allowed(), "3 failures out of 3 = circuit open");
//! ```

#![forbid(unsafe_code)]

use std::collections::VecDeque;

/// A single observed outcome recorded in the sliding window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// The attempt succeeded.
    Success,
    /// The attempt failed.
    Failure,
}

impl Outcome {
    /// `true` when the outcome is [`Outcome::Failure`].
    #[must_use]
    pub const fn is_failure(self) -> bool {
        matches!(self, Outcome::Failure)
    }

    /// `true` when the outcome is [`Outcome::Success`].
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Outcome::Success)
    }
}

/// An immutable snapshot of a [`RetryBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stats {
    /// Configured window capacity (after clamping).
    pub window_size: usize,
    /// Configured failure threshold (after clamping, always `>= 1`).
    pub max_failures: usize,
    /// Number of successes currently inside the window.
    pub successes: usize,
    /// Number of failures currently inside the window.
    pub failures: usize,
    /// `true` when retries are denied (threshold reached, or budget disabled).
    pub is_open: bool,
    /// `true` when a retry may be attempted now.
    pub retries_allowed: bool,
}

impl Stats {
    /// Number of outcomes currently recorded (`successes + failures`).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.successes + self.failures
    }

    /// `true` when the window holds no outcome at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Sliding-window retry budget.
///
/// See the [crate documentation](crate) for the exact semantics.
#[derive(Debug, Clone)]
pub struct RetryBudget {
    window_size: usize,
    max_failures: usize,
    history: VecDeque<Outcome>,
}

impl RetryBudget {
    /// Creates a budget holding at most `window_size` outcomes and opening the
    /// circuit once `max_failures` failures are present in that window.
    ///
    /// * `window_size == 0` normalises to a *disabled* budget (always denies).
    /// * `max_failures == 0` is clamped to `1`.
    ///
    /// The constructor deliberately does not pre-allocate `window_size` slots,
    /// so extreme values such as `usize::MAX` cannot trigger an allocation
    /// failure or a panic.
    #[must_use]
    pub fn new(window_size: usize, max_failures: usize) -> Self {
        Self {
            window_size,
            max_failures: max_failures.max(1),
            history: VecDeque::new(),
        }
    }

    /// Configured window capacity.
    #[must_use]
    pub const fn window_size(&self) -> usize {
        self.window_size
    }

    /// Configured failure threshold (always `>= 1`).
    #[must_use]
    pub const fn max_failures(&self) -> usize {
        self.max_failures
    }

    /// Number of outcomes currently recorded (never exceeds `window_size`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// `true` when no outcome is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Iterates over the recorded outcomes, oldest first.
    pub fn outcomes(&self) -> impl Iterator<Item = Outcome> + '_ {
        self.history.iter().copied()
    }

    /// Number of failures currently inside the window.
    #[must_use]
    pub fn failures_in_window(&self) -> usize {
        self.history.iter().filter(|o| o.is_failure()).count()
    }

    /// Number of successes currently inside the window.
    #[must_use]
    pub fn successes_in_window(&self) -> usize {
        self.history.iter().filter(|o| o.is_success()).count()
    }

    /// `true` when the circuit is open, i.e. retries are denied.
    ///
    /// A disabled budget (`window_size == 0`) always reports an open circuit.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.window_size == 0 || self.failures_in_window() >= self.max_failures
    }

    /// `true` when a retry may be attempted right now.
    #[must_use]
    pub fn is_retry_allowed(&self) -> bool {
        !self.is_open()
    }

    /// Records a successful attempt. No-op on a disabled budget.
    pub fn record_success(&mut self) {
        self.push(Outcome::Success);
    }

    /// Records a failed attempt. No-op on a disabled budget.
    pub fn record_failure(&mut self) {
        self.push(Outcome::Failure);
    }

    /// Clears the window, closing the circuit again (unless the budget is
    /// disabled, in which case retries stay denied). Configuration is kept.
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Returns a consistent snapshot of the budget.
    ///
    /// The snapshot satisfies `successes + failures == len()`,
    /// `retries_allowed == !is_open` and `len() <= window_size`.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let failures = self.failures_in_window();
        let successes = self.history.len() - failures;
        let is_open = self.window_size == 0 || failures >= self.max_failures;
        Stats {
            window_size: self.window_size,
            max_failures: self.max_failures,
            successes,
            failures,
            is_open,
            retries_allowed: !is_open,
        }
    }

    /// Pushes an outcome and evicts the oldest one when the window is full.
    fn push(&mut self, outcome: Outcome) {
        if self.window_size == 0 {
            return;
        }
        if self.history.len() >= self.window_size {
            self.history.pop_front();
        }
        self.history.push_back(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------- C1 ---

    #[test]
    fn test_new_clamps_zero_window() {
        let budget = RetryBudget::new(0, 3);
        // A zero window is normalised at the lower bound: the budget is
        // disabled rather than panicking.
        assert_eq!(budget.window_size(), 0);
        assert_eq!(budget.max_failures(), 3);
        assert!(budget.is_empty());
        assert_eq!(budget.len(), 0);
        assert!(!budget.is_retry_allowed());
    }

    #[test]
    fn test_new_clamps_zero_max_failures() {
        let budget = RetryBudget::new(10, 0);
        assert_eq!(budget.max_failures(), 1);
        assert_eq!(budget.window_size(), 10);
        assert!(budget.is_retry_allowed());

        let stats = budget.stats();
        assert_eq!(stats.max_failures, 1);
        assert_eq!(stats.window_size, 10);
    }

    #[test]
    fn test_new_no_panic_extreme_values() {
        let budget = RetryBudget::new(usize::MAX, usize::MAX);
        assert_eq!(budget.window_size(), usize::MAX);
        assert_eq!(budget.max_failures(), usize::MAX);
        assert!(budget.is_retry_allowed());
        assert_eq!(budget.len(), 0);

        let clamped = RetryBudget::new(usize::MAX, 0);
        assert_eq!(clamped.max_failures(), 1);

        // Recording must not pre-allocate `usize::MAX` slots.
        let mut budget = RetryBudget::new(usize::MAX, 2);
        budget.record_failure();
        assert_eq!(budget.len(), 1);
        assert!(budget.is_retry_allowed());
        budget.record_failure();
        assert!(!budget.is_retry_allowed());
        assert_eq!(budget.len(), 2);
    }

    // ---------------------------------------------------------------- C2 ---

    #[test]
    fn test_fifo_eviction_order() {
        let mut budget = RetryBudget::new(3, 10);
        budget.record_failure();
        budget.record_success();
        budget.record_success();
        assert_eq!(budget.len(), 3);
        assert_eq!(budget.stats().failures, 1);

        // The 4th outcome evicts the oldest one, which is the failure.
        budget.record_success();
        assert_eq!(budget.len(), 3);
        assert_eq!(budget.stats().failures, 0);
        assert_eq!(budget.stats().successes, 3);
        assert_eq!(
            budget.outcomes().collect::<Vec<_>>(),
            vec![Outcome::Success, Outcome::Success, Outcome::Success]
        );
    }

    #[test]
    fn test_window_capacity_respected() {
        let mut budget = RetryBudget::new(4, 100);
        for _ in 0..50 {
            budget.record_failure();
        }
        assert_eq!(budget.len(), 4);
        assert_eq!(budget.stats().failures, 4);
        assert_eq!(budget.stats().successes, 0);
    }

    #[test]
    fn test_mixed_sequence_eviction() {
        let mut budget = RetryBudget::new(5, 3);

        for outcome in [
            Outcome::Failure,
            Outcome::Failure,
            Outcome::Success,
            Outcome::Failure,
            Outcome::Success,
        ] {
            match outcome {
                Outcome::Success => budget.record_success(),
                Outcome::Failure => budget.record_failure(),
            }
        }

        assert_eq!(budget.len(), 5);
        assert_eq!(budget.stats().failures, 3);
        assert!(!budget.is_retry_allowed(), "3 failures = threshold reached");

        budget.record_success();
        assert_eq!(budget.stats().failures, 2);
        assert!(budget.is_retry_allowed());

        budget.record_success();
        assert_eq!(budget.stats().failures, 1);
        assert!(budget.is_retry_allowed());

        budget.record_success();
        let stats = budget.stats();
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.successes, 4);
        assert_eq!(stats.len(), 5);
    }

    // ---------------------------------------------------------------- C3 ---

    #[test]
    fn test_circuit_opens_at_threshold() {
        let mut budget = RetryBudget::new(10, 3);
        budget.record_failure();
        assert!(budget.is_retry_allowed());
        budget.record_failure();
        assert!(budget.is_retry_allowed());
        budget.record_failure();
        assert!(!budget.is_retry_allowed());
        assert!(budget.is_open());
        assert!(budget.stats().is_open);
        assert!(!budget.stats().retries_allowed);
    }

    #[test]
    fn test_circuit_stays_closed_below_threshold() {
        let mut budget = RetryBudget::new(10, 5);
        for _ in 0..4 {
            budget.record_failure();
        }
        assert!(budget.is_retry_allowed());
        assert!(!budget.is_open());
        assert!(!budget.stats().is_open);

        // Successes keep it closed too.
        budget.record_success();
        assert!(budget.is_retry_allowed());
    }

    #[test]
    fn test_circuit_reopens_after_eviction() {
        let mut budget = RetryBudget::new(3, 2);
        budget.record_failure();
        budget.record_failure();
        assert!(!budget.is_retry_allowed());

        // Three successes evict both failures out of the window: the circuit
        // becomes permissive again without an explicit reset.
        budget.record_success();
        assert!(!budget.is_retry_allowed(), "one failure still in window");
        budget.record_success();
        assert!(budget.is_retry_allowed());
        budget.record_success();

        let stats = budget.stats();
        assert_eq!(stats.failures, 0);
        assert_eq!(stats.successes, 3);
        assert!(!stats.is_open);
        assert!(stats.retries_allowed);
    }

    // ---------------------------------------------------------------- C4 ---

    #[test]
    fn test_retry_allowed_initial_state() {
        let budget = RetryBudget::new(5, 2);
        assert!(budget.is_retry_allowed());
        assert!(!budget.is_open());
        assert_eq!(budget.len(), 0);
        assert_eq!(budget.failures_in_window(), 0);

        let stats = budget.stats();
        assert!(stats.retries_allowed);
        assert!(stats.is_empty());
    }

    #[test]
    fn test_retry_allowed_after_opening() {
        let mut budget = RetryBudget::new(5, 2);
        budget.record_failure();
        budget.record_failure();
        assert!(!budget.is_retry_allowed());

        // Successes recorded while the circuit is open do not close it as long
        // as the failures remain inside the window.
        budget.record_success();
        assert_eq!(budget.stats().failures, 2);
        assert!(!budget.is_retry_allowed());
    }

    // ---------------------------------------------------------------- C5 ---

    #[test]
    fn test_record_success_updates_history() {
        let mut budget = RetryBudget::new(3, 2);
        budget.record_success();

        assert_eq!(budget.len(), 1);
        assert_eq!(budget.successes_in_window(), 1);
        assert_eq!(budget.failures_in_window(), 0);

        let stats = budget.stats();
        assert_eq!(stats.successes, 1);
        assert_eq!(stats.failures, 0);
        assert!(stats.retries_allowed);
        assert_eq!(budget.outcomes().collect::<Vec<_>>(), vec![Outcome::Success]);
    }

    #[test]
    fn test_record_failure_updates_history() {
        let mut budget = RetryBudget::new(3, 2);
        budget.record_failure();

        assert_eq!(budget.len(), 1);
        assert_eq!(budget.failures_in_window(), 1);
        assert_eq!(budget.successes_in_window(), 0);

        let stats = budget.stats();
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.successes, 0);
        assert!(stats.retries_allowed);
        assert_eq!(budget.outcomes().collect::<Vec<_>>(), vec![Outcome::Failure]);
    }

    // ---------------------------------------------------------------- C6 ---

    #[test]
    fn test_reset_clears_state() {
        let mut budget = RetryBudget::new(4, 2);
        budget.record_failure();
        budget.record_success();
        budget.reset();

        assert_eq!(budget.len(), 0);
        assert!(budget.is_empty());

        let stats = budget.stats();
        assert_eq!(stats.successes, 0);
        assert_eq!(stats.failures, 0);
        assert!(stats.retries_allowed);
        assert!(!stats.is_open);

        // Configuration is preserved across a reset.
        assert_eq!(stats.window_size, 4);
        assert_eq!(stats.max_failures, 2);
        assert_eq!(budget.window_size(), 4);
        assert_eq!(budget.max_failures(), 2);
    }

    #[test]
    fn test_reset_after_open_circuit() {
        let mut budget = RetryBudget::new(4, 2);
        budget.record_failure();
        budget.record_failure();
        assert!(!budget.is_retry_allowed());

        budget.reset();

        assert!(budget.is_retry_allowed());
        assert!(!budget.is_open());
        assert_eq!(budget.len(), 0);
    }

    // ---------------------------------------------------------------- C7 ---

    #[test]
    fn test_stats_consistency() {
        let mut budget = RetryBudget::new(6, 4);
        budget.record_failure();
        budget.record_success();
        budget.record_failure();

        let stats = budget.stats();
        assert_eq!(stats.window_size, 6);
        assert_eq!(stats.max_failures, 4);
        assert_eq!(stats.failures, 2);
        assert_eq!(stats.successes, 1);
        assert_eq!(stats.len(), budget.len());
        assert_eq!(stats.failures + stats.successes, stats.len());
        assert_eq!(stats.is_open, stats.failures >= stats.max_failures);
        assert_eq!(stats.retries_allowed, !stats.is_open);
        assert_eq!(budget.is_retry_allowed(), stats.retries_allowed);
        assert_eq!(budget.is_open(), stats.is_open);
    }

    #[test]
    fn test_stats_reflects_window_content() {
        let mut budget = RetryBudget::new(2, 5);
        budget.record_failure();
        budget.record_failure();
        assert_eq!(budget.stats().failures, 2);

        budget.record_success();
        budget.record_success();

        let stats = budget.stats();
        assert_eq!(stats.failures, 0);
        assert_eq!(stats.successes, 2);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats.len(), budget.window_size());
    }

    // ---------------------------------------------------------------- C8 ---

    #[test]
    fn test_zero_window_always_denied() {
        let mut budget = RetryBudget::new(0, 5);
        assert!(!budget.is_retry_allowed());

        budget.record_success();
        budget.record_failure();

        assert_eq!(budget.len(), 0, "a disabled budget records nothing");
        assert!(!budget.is_retry_allowed());
        assert!(budget.is_open());

        let stats = budget.stats();
        assert!(stats.is_open);
        assert!(!stats.retries_allowed);
        assert_eq!(stats.len(), 0);
    }

    #[test]
    fn test_zero_max_failures_opens_on_first_failure() {
        let mut budget = RetryBudget::new(5, 0);
        assert_eq!(budget.max_failures(), 1);
        assert!(budget.is_retry_allowed());

        budget.record_failure();

        assert!(!budget.is_retry_allowed());
        assert!(budget.stats().is_open);

        budget.reset();
        assert!(budget.is_retry_allowed());
    }

    #[test]
    fn test_zero_window_reset_no_panic() {
        let mut budget = RetryBudget::new(0, 0);
        budget.record_failure();
        budget.record_success();
        budget.reset();
        budget.reset();

        assert!(!budget.is_retry_allowed());
        assert_eq!(budget.len(), 0);

        let stats = budget.stats();
        assert_eq!(stats.window_size, 0);
        assert_eq!(stats.max_failures, 1);
        assert_eq!(stats.successes + stats.failures, 0);
        assert!(stats.is_open);
    }
}
