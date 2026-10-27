// story: e17s01 — SSH transport: russh listener, transport-token password
// auth, git-upload-pack/git-receive-pack over exec channels.
//
// WHY: russh — the stdlib has no SSH server and hand-rolling SSH-2 (kex,
// transport cipher, channel state machine) is a security project of its own.
// Rejected: shelling out to OpenSSH sshd (a separately-operated daemon that
// cannot consult the token table) and git-shell (speaks no SSH protocol).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;

use russh::server::{Auth, Handler, Server, Session};
use russh::ChannelId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};

use crate::token;

/// story: e17s01 — default SSH port when JJHUB_SSH_PORT is unset.
pub const DEFAULT_SSH_PORT: u16 = 2202;

/// story: e17s01 — the read side an SSH session needs (same tables the
/// HTTP transport reads).
#[derive(Clone)]
pub struct SshState {
    pub pool: sqlx::PgPool,
    pub storage_root: String,
    // story: e20s03 — shared limiter (same bucket map as HTTP).
    pub rate_limiter: Option<std::sync::Arc<crate::rate_limit::RateLimiter>>,
}

/// story: e17s01 — SSH bind address/port and host-key directory from the
/// environment; story: e17s02 — `enabled` gates the ssh:// clone line so
/// the template and the listener read one config. Invalid values fail
/// boot loudly (misconfiguration is visible, not silent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfig {
    pub enabled: bool,
    pub bind: SocketAddr,
    pub host_key_dir: std::path::PathBuf,
}

impl SshConfig {
    pub fn from_env() -> SshConfig {
        let enabled = match std::env::var("JJHUB_SSH_ENABLED") {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                other => panic!("invalid JJHUB_SSH_ENABLED value {other:?}"),
            },
            Err(_) => true,
        };
        let host: std::net::IpAddr = std::env::var("JJHUB_SSH_BIND")
            .unwrap_or_else(|_| "0.0.0.0".into())
            .parse()
            .unwrap_or_else(|e| panic!("invalid JJHUB_SSH_BIND: {e}"));
        let port: u16 = std::env::var("JJHUB_SSH_PORT")
            .unwrap_or_else(|_| DEFAULT_SSH_PORT.to_string())
            .parse()
            .unwrap_or_else(|e| panic!("invalid JJHUB_SSH_PORT: {e}"));
        let host_key_dir = std::env::var("JJHUB_SSH_HOST_KEY_DIR")
            .unwrap_or_else(|_| "./data/host-keys".into())
            .into();
        SshConfig {
            enabled,
            bind: SocketAddr::new(host, port),
            host_key_dir,
        }
    }
}

pub struct SshServer {
    state: SshState,
}

impl Server for SshServer {
    type Handler = SshHandler;

    fn new_client(&mut self, peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        // story: e20s03 — the client IP keys the push row.
        let mut handler = SshHandler::new(self.state.clone());
        handler.peer = peer_addr;
        handler
    }
}

/// story: e17s02 — what the SSH pump needs to run the shared
/// post-receive pipeline once `git-receive-pack` exits successfully.
struct PushState {
    channel: ChannelId,
    owner: String,
    name: String,
    pusher: Option<uuid::Uuid>,
    buf: Vec<u8>,
    done: bool,
}

/// story: e17s02 — scan pkt-line frames; `Some(end)` once the first
/// flush-packet (`0000`) is complete (or the stream is malformed),
/// `None` = keep buffering. Mirrors `receive_guard::parse_push_commands`.
fn pkt_flush_end(buf: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    loop {
        if pos + 4 > buf.len() {
            return None;
        }
        let hex = &buf[pos..pos + 4];
        if hex == b"0000" {
            return Some(pos + 4);
        }
        let Ok(text) = std::str::from_utf8(hex) else {
            return Some(pos);
        };
        let Ok(len) = usize::from_str_radix(text, 16) else {
            return Some(pos);
        };
        if len < 4 {
            return Some(pos);
        }
        pos += len;
    }
}

pub struct SshHandler {
    state: SshState,
    // story: e22s02 — the full authz bundle (identity + scopes/pin),
    // not just the uid: the shared admission gate reads it.
    auth: Option<crate::token::TransportAuth>,
    /// story: e20s03 — client socket for the push-row IP dimension.
    peer: Option<std::net::SocketAddr>,
    stdin: HashMap<ChannelId, ChildStdin>,
    /// story: e17s02 — the open receive-pack push on this connection:
    /// pkt-line capture until flush, consumed by the pump at child exit.
    push: Arc<std::sync::Mutex<Option<PushState>>>,
}

impl SshHandler {
    fn new(state: SshState) -> SshHandler {
        SshHandler {
            state,
            auth: None,
            peer: None,
            stdin: HashMap::new(),
            push: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// story: e17s02 — explicit storage root for tests.
    pub fn with_storage(pool: sqlx::PgPool, storage_root: String) -> SshHandler {
        SshHandler::new(SshState {
            pool,
            storage_root,
            rate_limiter: None,
        })
    }

    /// story: e20s03 — limiter-aware handler for the disconnect tests.
    pub fn with_storage_and_limiter(
        pool: sqlx::PgPool,
        storage_root: String,
        rate_limiter: Option<std::sync::Arc<crate::rate_limit::RateLimiter>>,
    ) -> SshHandler {
        SshHandler::new(SshState {
            pool,
            storage_root,
            rate_limiter,
        })
    }

    /// story: e22s02 — client address for allowlist checks; the real
    /// path fills it in new_client, tests set it directly.
    pub fn with_peer(mut self, peer: std::net::SocketAddr) -> Self {
        self.peer = Some(peer);
        self
    }

    /// story: e22s02 — change the assumed client address between
    /// authentications (tests exercise allowlist pass and fail).
    pub fn set_peer(&mut self, peer: std::net::SocketAddr) {
        self.peer = Some(peer);
    }

    /// story: e17s01 — resolve the repo and run the SAME admission chain
    /// as HTTP (transport::authorize), then spawn the git service with
    /// neutral git config.
    async fn start_service(&mut self, command: &str) -> Result<(Child, ChildStdin), &'static str> {
        let (service, owner, name) = parse_git_exec(command).ok_or("unrecognized git command")?;
        // story: e20s03 — receive-pack charges the SAME push row as
        // smart HTTP (one policy row, two transports) before any
        // admission work or git spawn; upload-pack is exempt, mirroring
        // the HTTP read exemption. Over budget: clean disconnect
        // message (russh has no status channel).
        if service.git_op() == crate::transport::GitOp::ReceivePack {
            if let Some(limiter) = self.state.rate_limiter.as_ref() {
                let who = self
                    .auth
                    .as_ref()
                    .map(|auth| auth.user_id.to_string())
                    .unwrap_or_else(|| "anonymous".to_string());
                let ip = self
                    .peer
                    .map(|p| p.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                if limiter
                    .try_charge(crate::rate_limit::Action::Push, &who, &ip)
                    .is_err()
                {
                    return Err("rate limited, retry later");
                }
            }
        }
        let mut resolved = crate::transport::lookup_repo(&self.state.pool, &owner, &name).await;
        if resolved.is_none() {
            // story: e22s02 — scope and pin gate admission ITSELF: a
            // denied push-to-create never creates (same helper as HTTP).
            if crate::token::transport_gate(self.auth.as_ref(), service.git_op(), None).is_err() {
                return Err("forbidden");
            }
            // story: e19s03 — missing row: ONE admission decision, the
            // same helper as smart HTTP. Receive-pack for the owning
            // token presenter creates the repo PRIVATE; every other
            // request stays not-found. No git process spawns before a
            // row exists.
            resolved = crate::transport::admit_create(
                &self.state.pool,
                &self.state.storage_root,
                &owner,
                &name,
                service.git_op(),
                self.auth.as_ref().map(|auth| auth.user_id),
                // story: e20s03 — the SSH push row is charged at the
                // edge above; no SSH create-row charge (e20s03 scopes
                // the push row only).
                "unknown",
                None,
            )
            .await
            .map_err(|_| "server error")?;
        }
        let (repo_id, repo_owner, visibility) = resolved.ok_or("repository not found")?;
        // story: e17s02 — receive-pack uses the SAME transport::authorize
        // chain as HTTP smart-http receive; no SSH-specific bypass.
        crate::transport::authorize(
            service.git_op(),
            self.auth.as_ref().map(|auth| auth.user_id),
            repo_owner,
            &visibility,
            &self.state.pool,
            repo_id,
            self.auth.as_ref(),
        )
        .await
        .map_err(|(status, _)| match status {
            axum::http::StatusCode::UNAUTHORIZED => "unauthorized",
            _ => "forbidden",
        })?;

        let bare = domain::storage_path(&self.state.storage_root, &owner, &name);
        if !bare.is_dir() {
            return Err("repository data missing");
        }

        let mut cmd = Command::new(service.program());
        if service == GitService::UploadPack {
            cmd.arg("--strict");
        }
        cmd.arg(&bare);
        // story: e17s01 — env discipline: never inherit the operator's
        // git config; the hook `server pre-receive` still fires for push.
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
        cmd.env("GIT_CONFIG_SYSTEM", "/dev/null");
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| {
            tracing::error!(%owner, %name, %e, "failed to spawn git service");
            "spawn failed"
        })?;
        let stdin = child.stdin.take().ok_or("stdin pipe missing")?;
        Ok((child, stdin))
    }

    /// story: e17s02 — public entry for the exec service (used by the
    /// channel callback and by admission tests).
    pub async fn run_git_exec(
        &mut self,
        command: &str,
    ) -> Result<(Child, ChildStdin), &'static str> {
        self.start_service(command).await
    }
}

/// story: e17s02 — the git service asked for by an SSH exec request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitService {
    UploadPack,
    ReceivePack,
}

impl GitService {
    fn git_op(self) -> crate::transport::GitOp {
        match self {
            GitService::UploadPack => crate::transport::GitOp::UploadPack,
            GitService::ReceivePack => crate::transport::GitOp::ReceivePack,
        }
    }

    fn program(self) -> &'static str {
        match self {
            GitService::UploadPack => "git-upload-pack",
            GitService::ReceivePack => "git-receive-pack",
        }
    }
}

impl Handler for SshHandler {
    type Error = russh::Error;

    // story: e17s01 — one credential model: the transport token. The ssh
    // username is ignored; the token decides identity (GitHub model).
    // story: e22s02 — authentication also enforces expiry and the IP
    // allowlist against this connection's peer, so dead tokens are
    // rejected at the password stage, before any repo context exists.
    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        match token::authenticate(&self.state.pool, password, self.peer.map(|peer| peer.ip())).await
        {
            Some(auth) => {
                self.auth = Some(auth);
                Ok(Auth::Accept)
            }
            None => Ok(Auth::reject()),
        }
    }

    // story: e17s01 — tokens only; decline publickey before any signature
    // work so a pubkey attempt never probes user or repo existence.
    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _public_key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(Auth::reject())
    }

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data);
        match self.start_service(&command).await {
            Ok((child, stdin)) => {
                let handle = session.handle();
                self.stdin.insert(channel, stdin);
                // story: e17s02 — capture the push's pkt-line command list
                // for the shared post-receive pipeline.
                if let Some((GitService::ReceivePack, owner, name)) = parse_git_exec(&command) {
                    *self.push.lock().expect("push mutex poisoned") = Some(PushState {
                        channel,
                        owner,
                        name,
                        pusher: self.auth.as_ref().map(|auth| auth.user_id),
                        buf: Vec::new(),
                        done: false,
                    });
                }
                let push = self.push.clone();
                spawn_git_pump(
                    child,
                    channel,
                    handle,
                    self.state.pool.clone(),
                    self.state.storage_root.clone(),
                    push,
                );
                session.channel_success(channel)?;
            }
            Err(reason) => {
                tracing::warn!(%command, %reason, "ssh exec refused");
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // story: e17s02 — tee client bytes until the push's flush-packet;
        // everything is forwarded to the child unchanged either way.
        {
            let mut guard = self.push.lock().expect("push mutex poisoned");
            if let Some(push) = guard.as_mut() {
                if push.channel == channel && !push.done {
                    push.buf.extend_from_slice(data);
                    if push.buf.len() > 8 * 1024 * 1024 {
                        push.done = true;
                        push.buf.clear();
                        tracing::warn!("ssh push capture exceeded 8 MiB; skipping post-receive");
                    } else if let Some(end) = pkt_flush_end(&push.buf) {
                        push.buf.truncate(end);
                        push.done = true;
                    }
                }
            }
        }
        if let Some(stdin) = self.stdin.get_mut(&channel) {
            // Client data after the child exited is harmless: ignore EPIPE.
            let _ = stdin.write_all(data).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Dropping the pipe closes the child's stdin (half-close).
        self.stdin.remove(&channel);
        Ok(())
    }
}

/// story: e17s01 — pump child stdio over the SSH channel, then report the
/// exit status the way git expects.
///
/// story: e17s02 — a successful receive-pack runs the SAME post-receive
/// pipeline as HTTP (ref sync + MR upsert) with the captured pkt-line body.
#[allow(clippy::too_many_arguments)]
fn spawn_git_pump(
    mut child: Child,
    channel: ChannelId,
    handle: russh::server::Handle,
    pool: sqlx::PgPool,
    storage_root: String,
    push: Arc<std::sync::Mutex<Option<PushState>>>,
) {
    let Some(mut stdout) = child.stdout.take() else {
        return;
    };
    let stderr = child.stderr.take();

    let out_handle = handle.clone();
    let out_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Session gone: keep draining so the child can exit.
                    let _ = out_handle.data(channel, buf[..n].to_vec()).await;
                }
            }
        }
    });

    let err_task = stderr.map(|mut stderr| {
        let handle = handle.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = handle.extended_data(channel, 2, buf[..n].to_vec()).await;
                    }
                }
            }
        })
    });

    tokio::spawn(async move {
        let status = child.wait().await.ok();
        let success = status.as_ref().is_some_and(|s| s.success());
        let _ = out_task.await;
        if let Some(task) = err_task {
            let _ = task.await;
        }
        let code = status.and_then(|s| s.code()).unwrap_or(1) as u32;
        // story: e17s02 — only a clean exit runs the shared pipeline; the
        // pre-receive hook already recorded any refusals itself.
        if success {
            let taken = push
                .lock()
                .expect("push mutex poisoned")
                .take_if(|p| p.channel == channel);
            if let Some(push_state) = taken {
                if push_state.done {
                    crate::transport::post_receive(
                        &pool,
                        &storage_root,
                        &push_state.owner,
                        &push_state.name,
                        push_state.pusher,
                        &push_state.buf,
                    )
                    .await;
                } else {
                    tracing::warn!(
                        owner = push_state.owner,
                        name = push_state.name,
                        "ssh push ended without a flush packet; skipping post-receive"
                    );
                }
            }
        }
        let _ = handle.exit_status_request(channel, code).await;
        let _ = handle.eof(channel).await;
        let _ = handle.close(channel).await;
    });
}

/// story: e17s02 — `git-upload-pack|git-receive-pack '/owner/name.git'`
/// (git quotes the path) down to (service, owner, name); anything else
/// is refused.
fn parse_git_exec(command: &str) -> Option<(GitService, String, String)> {
    let mut parts = command.split_whitespace();
    let service = parts.next()?;
    let service = match service {
        "git-upload-pack" => GitService::UploadPack,
        "git-receive-pack" => GitService::ReceivePack,
        _ => return None,
    };
    let raw = parts.next()?;
    let path = raw.trim_matches(|c| c == '\'' || c == '"');
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix(".git")?;
    let (owner, name) = path.split_once('/')?;
    if name.contains('/') {
        return None;
    }
    if !domain::repo_name_valid(owner) || !domain::repo_name_valid(name) {
        return None;
    }
    Some((service, owner.to_string(), name.to_string()))
}

/// story: e17s01 — load host keys from `dir`, generating an ed25519 pair
/// on first boot. Regeneration is an operator action (delete the files),
/// never automatic. Returns (keys, sha256 fingerprint).
pub fn load_or_generate_keys(
    dir: &std::path::Path,
) -> Result<(Vec<russh::keys::PrivateKey>, String), String> {
    use russh::keys::ssh_key::LineEnding;
    use russh::keys::{Algorithm, HashAlg, PrivateKey};

    let path = dir.join("ssh_host_ed25519_key");
    if path.exists() {
        let key = russh::keys::load_secret_key(&path, None)
            .map_err(|err| format!("load host key {}: {err}", path.display()))?;
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        return Ok((vec![key], fingerprint));
    }
    let mut rng = rand::rng();
    let key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|err| format!("generate host key: {err}"))?;
    let pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|err| format!("serialize host key: {err}"))?;
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("create host key dir {}: {err}", dir.display()))?;
    std::fs::write(&path, pem.as_bytes())
        .map_err(|err| format!("write host key {}: {err}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
    Ok((vec![key], fingerprint))
}

/// story: e17s01 — the bound listener; `run` serves until the process
/// exits (same lifecycle as the axum server in main).
pub struct SshListener {
    server: SshServer,
    config: std::sync::Arc<russh::server::Config>,
    socket: tokio::net::TcpListener,
    fingerprint: String,
}

impl SshListener {
    /// story: e17s01 — port-string entry (tests bind "0"); binds
    /// 0.0.0.0:<port>.
    pub async fn bind(
        port: &str,
        key_dir: &std::path::Path,
        pool: sqlx::PgPool,
        storage_root: String,
        rate_limiter: Option<std::sync::Arc<crate::rate_limit::RateLimiter>>,
    ) -> Result<Self, String> {
        let addr: SocketAddr = format!("0.0.0.0:{port}")
            .parse()
            .map_err(|err| format!("invalid SSH bind 0.0.0.0:{port}: {err}"))?;
        Self::bind_addr(addr, key_dir, pool, storage_root, rate_limiter).await
    }

    /// story: e17s01 — binds before returning so a port conflict fails the
    /// boot loudly, like the HTTP listener's bind. story: e17s01 config —
    /// the address honors JJHUB_SSH_BIND.
    pub async fn bind_addr(
        addr: SocketAddr,
        key_dir: &std::path::Path,
        pool: sqlx::PgPool,
        storage_root: String,
        rate_limiter: Option<std::sync::Arc<crate::rate_limit::RateLimiter>>,
    ) -> Result<Self, String> {
        let (keys, fingerprint) = load_or_generate_keys(key_dir)?;
        let config = russh::server::Config {
            keys,
            ..Default::default()
        };
        let socket = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|err| format!("bind {addr}: {err}"))?;
        Ok(Self {
            server: SshServer {
                state: SshState {
                    pool,
                    storage_root,
                    rate_limiter,
                },
            },
            config: std::sync::Arc::new(config),
            socket,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// story: e17s01 — actual bound address; tests bind port "0" and read it.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.socket.local_addr()
    }

    pub async fn run(mut self) {
        tracing::info!(fingerprint = %self.fingerprint, "ssh listener");
        if let Err(err) = self.server.run_on_socket(self.config, &self.socket).await {
            tracing::error!(%err, "ssh listener exited");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_git_exec, GitService};

    #[test]
    fn parses_git_quoted_exec() {
        assert_eq!(
            parse_git_exec("git-upload-pack '/alice/demo.git'"),
            Some((GitService::UploadPack, "alice".into(), "demo".into()))
        );
        assert_eq!(
            parse_git_exec("git-receive-pack '/alice/demo.git'"),
            Some((GitService::ReceivePack, "alice".into(), "demo".into()))
        );
    }

    #[test]
    fn parses_unquoted_and_rejects_junk() {
        assert_eq!(
            parse_git_exec("git-upload-pack /alice/demo.git"),
            Some((GitService::UploadPack, "alice".into(), "demo".into()))
        );
        assert_eq!(parse_git_exec("rm -rf /"), None);
        assert_eq!(parse_git_exec("git-upload-pack '/a/b'"), None);
        assert_eq!(parse_git_exec("git-upload-pack '/a/../b.git'"), None);
        assert_eq!(parse_git_exec("git-upload-pack '/a/b/c.git'"), None);
    }
}
