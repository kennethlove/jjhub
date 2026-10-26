// story: e20s03 — the SSH receive-pack path charges the SAME push
// row as smart HTTP (one policy row, two transports) before any
// admission work or git spawn; over budget the session gets a clean
// rate-limit message. SSH reads are exempt, mirroring HTTP.

use server::rate_limit::{Budget, RateLimitConfig, RateLimiter};
use server::ssh::SshHandler;

fn budget(per_window: u32) -> Budget {
    Budget {
        per_window,
        window_secs: 3600,
        burst: per_window,
    }
}

fn push_limiter(user_budget: u32) -> RateLimiter {
    RateLimiter::new(RateLimitConfig {
        enabled: true,
        window_secs: 3600,
        login_ip: budget(20),
        push_user: budget(user_budget),
        push_ip: budget(5),
        create_user: budget(10),
        create_ip: budget(10),
        token_user: budget(10),
    })
}

/// Gherkin: push budget of 1 — the second SSH push disconnects with
/// the rate-limit message, and the disconnect happens before any
/// admission work (the first call ends at lookup, never spawning).
#[sqlx::test(migrations = "./migrations")]
async fn receive_pack_over_budget_disconnects_before_git(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let (token, _tok) = server::token::insert_test_token(&pool).await;
    let mut handler = SshHandler::with_storage_and_limiter(
        pool,
        root.path().to_str().unwrap().to_string(),
        Some(std::sync::Arc::new(push_limiter(1))),
    );
    let auth = handler
        .auth_password("tokentest", &token)
        .await
        .unwrap();
    assert!(matches!(auth, russh::server::Auth::Accept));

    // 1st push: budget passes; no repo row exists, so it ends at
    // lookup — no git process ever spawns, charge already spent.
    let first = handler
        .run_git_exec("git-receive-pack '/tokentest/fresh.git'")
        .await
        .unwrap_err();
    assert_eq!(first, "repository not found");

    // 2nd push: over budget, clean message, still before admission.
    let second = handler
        .run_git_exec("git-receive-pack '/tokentest/fresh.git'")
        .await
        .unwrap_err();
    assert_eq!(second, "rate limited, retry later");
}

/// Gherkin: SSH clone never throttles — upload-pack is exempt even
/// with a spent push budget.
#[sqlx::test(migrations = "./migrations")]
async fn upload_pack_reads_exempt(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let (token, _tok) = server::token::insert_test_token(&pool).await;
    let mut handler = SshHandler::with_storage_and_limiter(
        pool,
        root.path().to_str().unwrap().to_string(),
        Some(std::sync::Arc::new(push_limiter(1))),
    );
    let auth = handler
        .auth_password("tokentest", &token)
        .await
        .unwrap();
    assert!(matches!(auth, russh::server::Auth::Accept));

    // Spend the push budget.
    let _ = handler
        .run_git_exec("git-receive-pack '/tokentest/fresh.git'")
        .await
        .unwrap_err();

    // Reads are never budget-checked: same lookup error, not rate limit.
    for _ in 0..3 {
        let read = handler
            .run_git_exec("git-upload-pack '/tokentest/fresh.git'")
            .await
            .unwrap_err();
        assert_eq!(read, "repository not found");
    }
}
