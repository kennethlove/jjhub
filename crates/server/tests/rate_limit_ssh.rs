// story: e20s03 — the SSH receive-pack path charges the SAME push
// row as smart HTTP (one policy row, two transports) before any
// admission work or git spawn; over budget the session gets a clean
// rate-limit message. SSH reads are exempt, mirroring HTTP.

use russh::server::Handler;
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
/// the rate-limit message. The charge lands before any admission
/// work or git spawn (the disconnect precedes both).
#[sqlx::test(migrations = "./migrations")]
async fn receive_pack_over_budget_disconnects_before_git(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().unwrap();
    let (token, _tok) = server::token::insert_test_token(&pool).await;
    let mut handler = SshHandler::with_storage_and_limiter(
        pool.clone(),
        root.path().to_str().unwrap().to_string(),
        Some(std::sync::Arc::new(push_limiter(1))),
    );
    let auth = handler.auth_password("tokentest", &token).await.unwrap();
    assert!(matches!(auth, russh::server::Auth::Accept));

    // 1st push: budget passes; push-to-create (e19s03) admits and
    // spawns — one token spent.
    let (child, _stdin) = handler
        .run_git_exec("git-receive-pack '/tokentest/fresh.git'")
        .await
        .expect("first push must pass the budget and be admitted");
    drop(child); // kill_on_drop: no pack data needed for the test

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM repositories")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "the first push created the repo");

    // 2nd push: over budget — clean message, before admission (no new
    // spawn) or any further work.
    let second = handler
        .run_git_exec("git-receive-pack '/tokentest/again.git'")
        .await
        .unwrap_err();
    assert_eq!(second, "rate limited, retry later");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM repositories")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        rows, 1,
        "over-budget push never reached admission (no second create)"
    );
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
    let auth = handler.auth_password("tokentest", &token).await.unwrap();
    assert!(matches!(auth, russh::server::Auth::Accept));

    // Spend the push budget (also creates the repo via e19s03).
    let (child, _stdin) = handler
        .run_git_exec("git-receive-pack '/tokentest/seed.git'")
        .await
        .expect("first push must pass");
    drop(child);

    // Reads are never budget-checked: clone of the existing repo
    // spawns every time despite the spent push budget...
    for _ in 0..3 {
        let (child, _stdin) = handler
            .run_git_exec("git-upload-pack '/tokentest/seed.git'")
            .await
            .expect("reads must never hit the rate limiter");
        drop(child);
    }

    // ...and a read of a missing repo stays not-found, never
    // rate-limited — the charge only ever runs for receive-pack.
    let missing = handler
        .run_git_exec("git-upload-pack '/tokentest/missing.git'")
        .await
        .unwrap_err();
    assert_eq!(missing, "repository not found");
}
