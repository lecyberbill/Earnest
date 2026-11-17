//! Integration tests: exercise the public API of `retry-budget` the way a
//! consumer would, covering the invariants sealed in `.ide/invariants.json`.

use retry_budget::{Outcome, RetryBudget, Stats};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn public_types_are_send_sync() {
    assert_send_sync::<RetryBudget>();
    assert_send_sync::<Stats>();
    assert_send_sync::<Outcome>();
}

#[test]
fn documented_example_holds() {
    let mut budget = RetryBudget::new(10, 3);
    assert!(budget.is_retry_allowed());
    for _ in 0..3 {
        budget.record_failure();
    }
    assert!(!budget.is_retry_allowed());
}

#[test]
fn invariant_len_never_exceeds_window() {
    for window in [1usize, 2, 3, 7] {
        let mut budget = RetryBudget::new(window, 2);
        for i in 0..50 {
            if i % 3 == 0 {
                budget.record_success();
            } else {
                budget.record_failure();
            }
            assert!(
                budget.len() <= window,
                "window {window} exceeded: len = {}",
                budget.len()
            );
            let stats = budget.stats();
            assert_eq!(stats.len(), budget.len());
            assert_eq!(stats.successes + stats.failures, stats.len());
        }
    }
}

#[test]
fn invariant_retries_allowed_mirrors_is_open() {
    let configs = [(0usize, 0usize), (0, 5), (1, 1), (4, 2), (16, 8)];
    for (window, max_failures) in configs {
        let mut budget = RetryBudget::new(window, max_failures);
        for step in 0..12 {
            let stats = budget.stats();
            assert_eq!(stats.retries_allowed, !stats.is_open);
            assert_eq!(budget.is_retry_allowed(), !budget.is_open());
            assert_eq!(stats.is_open, budget.is_open());
            assert_eq!(stats.window_size, budget.window_size());
            assert_eq!(stats.max_failures, budget.max_failures());
            assert!(stats.max_failures >= 1);

            if step % 4 == 0 {
                budget.reset();
            } else if step % 2 == 0 {
                budget.record_failure();
            } else {
                budget.record_success();
            }
        }
    }
}

#[test]
fn scenario_failure_storm_opens_then_auto_recovers() {
    let mut budget = RetryBudget::new(6, 3);

    for _ in 0..3 {
        budget.record_failure();
    }
    assert!(!budget.is_retry_allowed(), "failure storm must open circuit");

    // Recovery without any administrative reset: successes evict failures.
    for _ in 0..4 {
        budget.record_success();
    }

    let stats = budget.stats();
    assert_eq!(stats.failures, 2);
    assert!(stats.retries_allowed);
    assert!(!stats.is_open);
}

#[test]
fn scenario_disable_and_reset() {
    let mut budget = RetryBudget::new(0, 3);
    assert!(!budget.is_retry_allowed());
    budget.record_success();
    budget.record_failure();
    budget.reset();
    assert!(!budget.is_retry_allowed(), "disabled budget stays denied");
    assert!(budget.stats().is_empty());
}

#[test]
fn extreme_configuration_does_not_panic() {
    let mut budget = RetryBudget::new(usize::MAX, usize::MAX);
    for _ in 0..1000 {
        budget.record_failure();
    }
    assert_eq!(budget.len(), 1000);
    assert!(budget.is_retry_allowed());
    budget.reset();
    assert_eq!(budget.len(), 0);
}
