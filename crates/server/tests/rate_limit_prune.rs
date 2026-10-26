// story: e20s03 — idle pruning: a bucket untouched for longer than
// the window is dropped on access, so key cardinality cannot grow
// the map forever. No timer task; O(map) scan where the buckets are
// already consumed.

use server::rate_limit::{Budget, RateLimitConfig, RateLimiter};

fn budget(per_window: u32) -> Budget {
    Budget {
        per_window,
        window_secs: 1,
        burst: per_window,
    }
}

fn cfg() -> RateLimitConfig {
    RateLimitConfig {
        enabled: true,
        window_secs: 1,
        login_ip: budget(20),
        push_user: budget(1),
        push_ip: budget(5),
        create_user: budget(10),
        create_ip: budget(10),
        token_user: budget(10),
    }
}

/// Prune reports how many buckets it dropped: 0 while all buckets are
/// fresh, 1 after the window passes with no access.
#[tokio::test]
async fn idle_buckets_pruned_after_window() {
    let limiter = RateLimiter::new(cfg());

    // Fresh access creates buckets; prune right away drops nothing.
    limiter
        .try_charge(server::rate_limit::Action::Push, "user-1", "10.0.0.1")
        .expect("fresh buckets have budget");
    assert_eq!(limiter.prune_idle(), 0, "fresh buckets survive pruning");

    // One access, so only its buckets exist; the sleep outlasts the
    // window so the entry counts as idle.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(
        limiter.prune_idle(),
        2,
        "the stale bucket pair (user row + ip row) is dropped"
    );
    assert_eq!(limiter.prune_idle(), 0, "second prune is a no-op");

    // A pruned key gets a fresh budget on the next access.
    limiter
        .try_charge(server::rate_limit::Action::Push, "user-1", "10.0.0.1")
        .expect("fresh budget after prune");
}
