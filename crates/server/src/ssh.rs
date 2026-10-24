// story: e17s01 — SSH transport: russh listener, transport-token password
// auth, git-upload-pack over exec channels.
//
// WHY: russh — the stdlib has no SSH server and hand-rolling SSH-2 (kex,
// transport cipher, channel state machine) is a security project of its own.
// Rejected: shelling out to OpenSSH sshd (a separately-operated daemon that
// cannot consult the token table) and git-shell (speaks no SSH protocol).

use std::process::Stdio;

use russh::server::{Auth, Handler, Server, Session};
use russh::ChannelId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::token;

/// story: e17s01 — the read side an SSH session needs (same tables the
/// HTTP transport reads).
#[derive(Clone)]
pub struct SshState {
    pub pool: sqlx::PgPool,
    pub storage_root: String,
}

pub struct SshServer {
    state: SshState,
}

impl Server for SshServer {
    type Handler = SshHandler;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        SshHandler {
            state: self.state.clone(),
            user: None,
            stdin: None,
        }
    }
}

pub struct SshHandler {
    state: SshState,
    user: Option<uuid::Uuid>,
    stdin: Option<tokio::process::ChildStdin>,
}

impl Handler for SshHandler {
    type Error = russh::Error;

    // story: e17s01 — one credential model: the transport token. The ssh
    // username is ignored; the token decides identity (GitHub model).
    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        match token::authenticate(&self.state.pool, password).await {
            Some(uid) => {
                self.user = Some(uid);
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

    // story: e17s01 — the only exec admitted is `git-upload-pack` on a
    // resolvable repo the token may read; failures are a bare channel
    // failure (never whether the repo or user exists).
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).to_string();
        let target = match (self.user, parse_upload_pack(&command)) {
            (Some(user), Some((owner, name))) => {
                let repo: Option<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
                    "SELECT r.id, r.owner_id, r.visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = $1 AND r.name = $2",
                )
                .bind(&owner)
                .bind(&name)
                .fetch_optional(&self.state.pool)
                .await
                .ok()
                .flatten();
                match repo {
                    // story: e17s01 — same decision as the HTTP smart
                    // transport's UploadPack arm (transport.rs authorize).
                    Some((repo_id, repo_owner, visibility))
                        if visibility == "public"
                            || crate::transport::push_or_read_allowed(
                                &self.state.pool,
                                repo_id,
                                user,
                                repo_owner,
                            )
                            .await =>
                    {
                        Some((owner, name))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((owner, name)) = target else {
            let _ = session.channel_failure(channel);
            return Ok(());
        };
        let bare = domain::storage_path(&self.state.storage_root, &owner, &name);
        let mut child = match tokio::process::Command::new("git")
            .arg("upload-pack")
            .arg("--strict")
            .arg(&bare)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(%err, "ssh: spawn git upload-pack failed");
                let _ = session.channel_failure(channel);
                return Ok(());
            }
        };
        let Some(mut stdout) = child.stdout.take() else {
            let _ = session.channel_failure(channel);
            return Ok(());
        };
        let mut stderr = child.stderr.take();
        self.stdin = child.stdin.take();

        let _ = session.channel_success(channel);
        let handle = session.handle();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 32 * 1024];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if handle.data(channel, buf[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                }
            }
            if let Some(mut stderr) = stderr.take() {
                let err_handle = handle.clone();
                tokio::spawn(async move {
                    let mut ebuf = vec![0u8; 4096];
                    loop {
                        match stderr.read(&mut ebuf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                // ext 2 = SSH extended data (stderr)
                                if err_handle
                                    .extended_data(channel, 2, ebuf[..n].to_vec())
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
            let status = child.wait().await.map(|s| s.code().unwrap_or(128));
            if let Ok(status) = status {
                let _ = handle.exit_status_request(channel, status as u32).await;
            }
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(data).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // client finished sending; dropping stdin closes the pipe so
        // upload-pack sees EOF and finishes the negotiation.
        self.stdin.take();
        Ok(())
    }
}

/// story: e17s01 — `git-upload-pack '/owner/repo.git'` (git quotes the
/// path) down to a validated (owner, name) pair.
fn parse_upload_pack(command: &str) -> Option<(String, String)> {
    let rest = command.trim().strip_prefix("git-upload-pack")?.trim();
    let path = match rest.strip_prefix('\'') {
        Some(inner) => inner.strip_suffix('\'')?,
        None => rest,
    };
    let path = path.strip_prefix('/').unwrap_or(path);
    let (owner, name) = path.strip_suffix(".git")?.split_once('/')?;
    if !domain::repo_name_valid(owner) || !domain::repo_name_valid(name) {
        return None;
    }
    Some((owner.to_string(), name.to_string()))
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
    /// story: e17s01 — binds before returning so a port conflict fails the
    /// boot loudly, like the HTTP listener's bind.
    pub async fn bind(
        port: &str,
        key_dir: &std::path::Path,
        pool: sqlx::PgPool,
        storage_root: String,
    ) -> Result<Self, String> {
        let (keys, fingerprint) = load_or_generate_keys(key_dir)?;
        let config = russh::server::Config {
            keys,
            ..Default::default()
        };
        let socket = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
            .await
            .map_err(|err| format!("bind 0.0.0.0:{port}: {err}"))?;
        Ok(Self {
            server: SshServer {
                state: SshState { pool, storage_root },
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
        tracing::info!(fingerprint = %self.fingerprint, "ssh listener on 0.0.0.0");
        if let Err(err) = self.server.run_on_socket(self.config, &self.socket).await {
            tracing::error!(%err, "ssh listener exited");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_upload_pack;

    #[test]
    fn parses_git_quoted_upload_pack() {
        assert_eq!(
            parse_upload_pack("git-upload-pack '/alice/demo.git'"),
            Some(("alice".into(), "demo".into()))
        );
    }

    #[test]
    fn parses_unquoted_and_rejects_junk() {
        assert_eq!(
            parse_upload_pack("git-upload-pack /alice/demo.git"),
            Some(("alice".into(), "demo".into()))
        );
        assert_eq!(parse_upload_pack("git-receive-pack '/a/b.git'"), None);
        assert_eq!(parse_upload_pack("git-upload-pack '/a/b'"), None);
        assert_eq!(parse_upload_pack("rm -rf /"), None);
        assert_eq!(
            parse_upload_pack("git-upload-pack '/a/../b.git'"),
            None,
            "path traversal is not a valid name"
        );
    }
}
