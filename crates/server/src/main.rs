// story: e01s01 — main wires one shared rate limiter into both
// transports (app_with_limiter is referenced fully-qualified below).

#[tokio::main]
async fn main() {
    // story: e03s01 — `server pre-receive` runs as the bare repo's
    // pre-receive hook (cwd = the git dir). stdin: `<old> <new> <ref>`
    // lines; exit 1 with reasons refuses the push before any ref moves.
    if std::env::args().nth(1).as_deref() == Some("pre-receive") {
        let status = run_pre_receive().await;
        std::process::exit(status);
    }

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");

    // story: kata scan — the sidecar watches {storage}/.meta; create it
    // before serving so its inotify watch attaches on a fresh volume
    let storage_root =
        std::env::var("JJHUB_STORAGE_ROOT").unwrap_or_else(|_| "./data/repos".into());
    server::ensure_storage_layout(&storage_root).expect("create storage layout");

    // story: e17s01 — SSH transport: one config source (from_env) shared
    // with the template's ssh:// gate; binds before the HTTP listener so a
    // port conflict fails boot loudly. JJHUB_SSH_ENABLED=0 is the
    // HTTP-only opt-out.
    // story: e20s03 — ONE rate limiter for both transports: SSH gets a
    // shared handle (same bucket map), the HTTP app keeps the owner.
    let limiter = server::rate_limit::RateLimiter::limiter_from_env();
    let ssh_limiter = limiter
        .as_ref()
        .map(|l| server::rate_limit::share(l.clone()));
    let ssh_cfg = server::ssh::SshConfig::from_env();
    if ssh_cfg.enabled {
        let ssh = server::ssh::SshListener::bind_addr(
            ssh_cfg.bind,
            &ssh_cfg.host_key_dir,
            pool.clone(),
            storage_root.clone(),
            ssh_limiter,
        )
        .await
        .unwrap_or_else(|e| panic!("failed to start SSH listener: {e}"));
        tokio::spawn(ssh.run());
    }

    // story: e02s01 — bind all interfaces so docker port mapping works.
    let port = std::env::var("JJHUB_PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap_or_else(|e| panic!("failed to bind 0.0.0.0:{port}: {e}"));
    // story: e20s01 — connect_info so the login rate limiter can key
    // on the client IP.
    axum::serve(
        listener,
        server::app_production(pool, storage_root, limiter)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("server error");
}

/// story: e03s01 — hook body: parse ref updates, read each new head's
/// identity from the (quarantined) object store, ask the guard, report.
async fn run_pre_receive() -> i32 {
    use std::io::Read;
    let mut stdin_bytes = Vec::new();
    if let Err(err) = std::io::stdin().read_to_end(&mut stdin_bytes) {
        eprintln!("jjhub: read stdin: {err}");
        return 1;
    }
    let updates = server::receive_guard::parse_ref_updates(&stdin_bytes);
    if updates.is_empty() {
        return 0;
    }

    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("jjhub: DATABASE_URL not set in hook environment");
            return 1;
        }
    };
    let pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
    {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("jjhub: connect: {err}");
            return 1;
        }
    };

    // resolve the repo row for this bare dir (cwd of the hook)
    let bare = match std::env::current_dir() {
        Ok(dir) => std::fs::canonicalize(&dir).unwrap_or(dir),
        Err(err) => {
            eprintln!("jjhub: cwd: {err}");
            return 1;
        }
    };
    let storage_root =
        std::env::var("JJHUB_STORAGE_ROOT").unwrap_or_else(|_| "./data/repos".into());
    let repo_id: Option<uuid::Uuid> = {
        let rows: Vec<(String, String, uuid::Uuid)> = sqlx::query_as(
            "SELECT u.handle, r.name, r.id FROM repositories r JOIN users u ON u.id = r.owner_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        rows.into_iter()
            .find(|(handle, name, _)| {
                // canonicalize both sides: /var vs /private/var on macOS
                // (and any symlinked storage root) would otherwise silently
                // skip the guard
                let candidate =
                    std::fs::canonicalize(domain::storage_path(&storage_root, handle, name))
                        .unwrap_or_else(|_| domain::storage_path(&storage_root, handle, name));
                candidate == bare
            })
            .map(|(_, _, id)| id)
    };
    let Some(repo_id) = repo_id else {
        // unknown repo path: nothing to enforce, never block the push
        return 0;
    };

    // story: e10s01 — admission: protected-ref rule over EVERY update
    // (tags and deletions included), then the open-MR identity rule.
    // Refusals print to stderr before any ref moves.
    // story: e10s03 — every refusal is also persisted with the pushing
    // identity and the ref; a failed insert is logged and the push is
    // STILL refused (exit 1 either way).
    let git_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match server::receive_guard::admit_push(&pool, repo_id, &updates, &git_dir).await {
        Ok(reasons) if reasons.is_empty() => 0,
        Ok(rejections) => {
            for rejection in &rejections {
                eprintln!("jjhub: {}", rejection.reason);
            }
            if let Err(err) =
                server::receive_guard::record_rejections(&pool, repo_id, &rejections).await
            {
                eprintln!("jjhub: record rejection: {err}");
            }
            1
        }
        Err(err) => {
            eprintln!("jjhub: guard query: {err}");
            1
        }
    }
}
