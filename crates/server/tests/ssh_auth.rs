//! story: e17s01 (AC t1) — SSH auth: transport tokens accept, wrong token and
//! publickey decline, clone exec runs `git upload-pack --strict`.

use russh::client;
use russh::keys::PublicKeyOrCertificate;
use server::ssh::SshListener;

struct TestClient;
impl client::Handler for TestClient {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn spawn_listener(pool: sqlx::PgPool, storage_root: String) -> String {
    let dir = tempfile::tempdir().expect("key dir");
    let listener = SshListener::bind("0", dir.path(), pool, storage_root, None)
        .await
        .expect("bind ssh");
    let addr = listener.local_addr().expect("local addr");
    // Key dir must outlive the test; keep it alive until the process ends.
    std::mem::forget(dir);
    tokio::spawn(listener.run());
    addr.to_string()
}

/// story: e17s01 — a bare repo with one commit, owned by the `tokentest` user
/// that `token::insert_test_token` creates.
async fn seed_bare_repo(root: &std::path::Path) {
    let storage = root.join("storage");
    let bare = storage.join("tokentest").join("repo.git");
    std::fs::create_dir_all(&bare).expect("bare dir");
    let git = |args: &[&str], cwd: &std::path::Path| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git ran");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "--bare", bare.to_str().unwrap()], root);
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).expect("seed dir");
    git(&["init"], &seed);
    git(
        &[
            "-c",
            "user.email=e2e@example.com",
            "-c",
            "user.name=e2e",
            "commit",
            "--allow-empty",
            "-m",
            "seed",
        ],
        &seed,
    );
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &seed);
    git(&["push", "origin", "HEAD:refs/heads/master"], &seed);
}

async fn connect(addr: &str) -> client::Handle<TestClient> {
    let config = std::sync::Arc::new(client::Config::default());
    client::connect(config, addr, TestClient)
        .await
        .expect("ssh connect")
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_token_is_rejected(pool: sqlx::PgPool) {
    let tmp = tempfile::tempdir().expect("tmp");
    let addr = spawn_listener(pool.clone(), tmp.path().to_string_lossy().into()).await;
    let mut session = connect(&addr).await;
    let ok = session
        .authenticate_password("tokentest", "jjh_definitely_not_a_token")
        .await
        .expect("auth attempt");
    assert!(
        !matches!(ok, client::AuthResult::Success),
        "wrong token must be rejected"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn transport_token_is_accepted(pool: sqlx::PgPool) {
    let (plaintext, _user_id) = server::token::insert_test_token(&pool).await;
    let tmp = tempfile::tempdir().expect("tmp");
    let addr = spawn_listener(pool.clone(), tmp.path().to_string_lossy().into()).await;
    let mut session = connect(&addr).await;
    let ok = session
        .authenticate_password("tokentest", &plaintext)
        .await
        .expect("auth attempt");
    assert!(
        matches!(ok, client::AuthResult::Success),
        "valid transport token must be accepted"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn publickey_authentication_is_declined(pool: sqlx::PgPool) {
    let tmp = tempfile::tempdir().expect("tmp");
    let addr = spawn_listener(pool.clone(), tmp.path().to_string_lossy().into()).await;
    let mut session = connect(&addr).await;
    use russh::keys::PrivateKey;
    let mut rng = rand::rng();
    let key = PrivateKey::random(&mut rng, russh::keys::Algorithm::Ed25519).expect("key");
    let ok = session
        .authenticate_publickey(
            "tokentest",
            russh::keys::PrivateKeyWithHashAlg::new(std::sync::Arc::new(key), None),
        ) // will still be declined
        .await
        .expect("auth attempt");
    assert!(
        !matches!(ok, client::AuthResult::Success),
        "publickey auth must be declined (story: e17s01)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn clone_exec_runs_upload_pack(pool: sqlx::PgPool) {
    let (plaintext, _token_id) = server::token::insert_test_token(&pool).await;
    let user_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'tokentest'")
        .fetch_one(&pool)
        .await
        .expect("fetch test user");
    sqlx::query(
        "INSERT INTO repositories (owner_id, name, visibility) VALUES ($1, 'repo', 'public')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("insert repo row");
    let tmp = tempfile::tempdir().expect("tmp");
    seed_bare_repo(tmp.path()).await;
    let storage = tmp.path().join("storage");
    let addr = spawn_listener(pool.clone(), storage.to_string_lossy().into()).await;
    let mut session = connect(&addr).await;
    let auth = session
        .authenticate_password("tokentest", &plaintext)
        .await
        .expect("auth");
    assert!(
        matches!(auth, client::AuthResult::Success),
        "token must authenticate"
    );
    let mut channel = session.channel_open_session().await.expect("channel");
    channel
        .exec(true, b"git-upload-pack 'tokentest/repo.git'")
        .await
        .expect("exec of upload-pack must be allowed for a token holder");
    // Ref advertisement: upload-pack writes the advertised refs first.
    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while got.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no ref advertisement within 10s"
        );
        match tokio::time::timeout(std::time::Duration::from_secs(5), channel.wait())
            .await
            .expect("wait timed out")
        {
            Some(russh::ChannelMsg::Data { ref data }) => got.extend_from_slice(data),
            Some(russh::ChannelMsg::ExtendedData { ref data, .. }) => {
                panic!("stderr: {}", String::from_utf8_lossy(data))
            }
            Some(russh::ChannelMsg::Eof) | None => break,
            _ => {}
        }
    }
    let text = String::from_utf8_lossy(&got).to_string();
    assert!(
        text.contains("refs/heads/master") || text.contains("HEAD"),
        "expected ref advertisement, got {text:?}"
    );
}
