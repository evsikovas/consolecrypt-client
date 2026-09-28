//! Built-in SSH agent (draft-miller-ssh-agent) serving keys that live only
//! in this process's memory. Read-only: identities are supplied by the app;
//! add/remove/lock requests from clients are refused.
//!
//! Transport: a Unix socket (mode 0600) inside a private 0700 directory,
//! with a peer-uid check, or a Windows named pipe (local clients only).

use crate::AgentError;
use async_trait::async_trait;
use cc_ssh_core::keys::{fingerprint_sha256, key_blob};
use cc_ssh_core::russh::keys::signature::Signer;
use cc_ssh_core::russh::keys::ssh_encoding::Encode;
use cc_ssh_core::russh::keys::ssh_key::private::KeypairData;
use cc_ssh_core::russh::keys::ssh_key::{Certificate, HashAlg, PrivateKey, Signature};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

// Message numbers (draft-miller-ssh-agent §5.1).
const SSH_AGENT_FAILURE: u8 = 5;
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;
const SSH_AGENT_RSA_SHA2_256: u32 = 2;
const SSH_AGENT_RSA_SHA2_512: u32 = 4;
const MAX_FRAME: usize = 256 * 1024;

/// A decrypted key held by the built-in agent (zeroized on drop by ssh-key).
pub struct AgentKey {
    key: PrivateKey,
    certificate: Option<Certificate>,
    comment: String,
}

impl std::fmt::Debug for AgentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentKey")
            .field("fingerprint", &self.fingerprint())
            .field("certificate", &self.certificate.is_some())
            .field("comment", &self.comment)
            .finish_non_exhaustive()
    }
}

impl AgentKey {
    /// `key` must be decrypted (see `cc_ssh_core::keys::load_private_key`).
    pub fn new(
        key: PrivateKey,
        certificate: Option<Certificate>,
        comment: impl Into<String>,
    ) -> Result<Self, AgentError> {
        if key.is_encrypted() {
            return Err(AgentError::Key("the agent needs a decrypted key".into()));
        }
        if let Some(c) = &certificate {
            if c.public_key() != key.public_key().key_data() {
                return Err(AgentError::Key("certificate does not match the key".into()));
            }
        }
        Ok(Self {
            key,
            certificate,
            comment: comment.into(),
        })
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_sha256(&key_blob(self.key.public_key().key_data()))
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    fn sign(&self, data: &[u8], flags: u32) -> Option<Vec<u8>> {
        let sig: Signature = match self.key.key_data() {
            KeypairData::Rsa(rsa) => {
                let hash = if flags & SSH_AGENT_RSA_SHA2_512 != 0 {
                    Some(HashAlg::Sha512)
                } else if flags & SSH_AGENT_RSA_SHA2_256 != 0 {
                    Some(HashAlg::Sha256)
                } else {
                    None
                };
                Signer::try_sign(&(rsa, hash), data).ok()?
            }
            kp => Signer::try_sign(kp, data).ok()?,
        };
        sig.encode_vec().ok()
    }
}

/// Optional user confirmation before each signature (e.g. a UI prompt
/// "Allow OpenSSH to use key X?").
#[async_trait]
pub trait SignConfirm: Send + Sync {
    async fn confirm(&self, fingerprint: &str, comment: &str) -> bool;
}

struct Identity {
    blob: Vec<u8>,
    comment: String,
}

/// Protocol engine (transport independent).
pub struct AgentService {
    identities: Vec<Identity>,
    by_blob: HashMap<Vec<u8>, Arc<AgentKey>>,
    confirm: Option<Arc<dyn SignConfirm>>,
}

impl std::fmt::Debug for AgentService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentService")
            .field("identities", &self.identities.len())
            .field("confirm", &self.confirm.is_some())
            .finish()
    }
}

impl AgentService {
    pub fn new(keys: Vec<AgentKey>) -> Self {
        let mut identities = Vec::new();
        let mut by_blob = HashMap::new();
        for k in keys {
            let k = Arc::new(k);
            let blob = key_blob(k.key.public_key().key_data());
            by_blob.insert(blob.clone(), k.clone());
            identities.push(Identity {
                blob,
                comment: k.comment.clone(),
            });
            if let Some(cert) = &k.certificate {
                if let Ok(cblob) = cert.to_bytes() {
                    by_blob.insert(cblob.clone(), k.clone());
                    identities.push(Identity {
                        blob: cblob,
                        comment: format!("{} (certificate)", k.comment),
                    });
                }
            }
        }
        Self {
            identities,
            by_blob,
            confirm: None,
        }
    }

    pub fn with_confirm(mut self, confirm: Arc<dyn SignConfirm>) -> Self {
        self.confirm = Some(confirm);
        self
    }

    /// Number of identities offered (keys + certificates).
    pub fn identity_count(&self) -> usize {
        self.identities.len()
    }

    /// Handle one request payload; returns the response payload.
    pub async fn handle(&self, req: &[u8]) -> Vec<u8> {
        let Some((&kind, mut rest)) = req.split_first() else {
            return vec![SSH_AGENT_FAILURE];
        };
        match kind {
            SSH_AGENTC_REQUEST_IDENTITIES => {
                let mut out = vec![SSH_AGENT_IDENTITIES_ANSWER];
                put_u32(&mut out, self.identities.len() as u32);
                for id in &self.identities {
                    put_string(&mut out, &id.blob);
                    put_string(&mut out, id.comment.as_bytes());
                }
                out
            }
            SSH_AGENTC_SIGN_REQUEST => {
                let (Some(blob), Some(data), Some(flags)) = (
                    get_string(&mut rest),
                    get_string(&mut rest),
                    get_u32(&mut rest),
                ) else {
                    return vec![SSH_AGENT_FAILURE];
                };
                let Some(key) = self.by_blob.get(blob) else {
                    return vec![SSH_AGENT_FAILURE];
                };
                if let Some(c) = &self.confirm {
                    if !c.confirm(&key.fingerprint(), &key.comment).await {
                        tracing::info!(fingerprint = %key.fingerprint(), "agent signature denied by user");
                        return vec![SSH_AGENT_FAILURE];
                    }
                }
                match key.sign(data, flags) {
                    Some(sig) => {
                        tracing::debug!(fingerprint = %key.fingerprint(), "agent signed a request");
                        let mut out = vec![SSH_AGENT_SIGN_RESPONSE];
                        put_string(&mut out, &sig);
                        out
                    }
                    None => vec![SSH_AGENT_FAILURE],
                }
            }
            // add / remove / lock / unlock / extensions: read-only agent.
            _ => vec![SSH_AGENT_FAILURE],
        }
    }

    /// Serve one client connection until it closes.
    pub async fn serve_connection<S>(&self, mut s: S) -> Result<(), AgentError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        loop {
            let len = match s.read_u32().await {
                Ok(l) => l as usize,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            };
            if len == 0 || len > MAX_FRAME {
                return Err(AgentError::Protocol(format!("bad frame length {len}")));
            }
            let mut req = vec![0u8; len];
            s.read_exact(&mut req).await?;
            let resp = self.handle(&req).await;
            s.write_u32(resp.len() as u32).await?;
            s.write_all(&resp).await?;
            s.flush().await?;
        }
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    put_u32(out, b.len() as u32);
    out.extend_from_slice(b);
}

fn get_u32(r: &mut &[u8]) -> Option<u32> {
    if r.len() < 4 {
        return None;
    }
    let (h, t) = r.split_at(4);
    *r = t;
    Some(u32::from_be_bytes([h[0], h[1], h[2], h[3]]))
}

fn get_string<'a>(r: &mut &'a [u8]) -> Option<&'a [u8]> {
    let len = get_u32(r)? as usize;
    if r.len() < len {
        return None;
    }
    let (h, t) = r.split_at(len);
    *r = t;
    Some(h)
}

/// Where the agent listens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentListenOptions {
    /// Parent directory for the private socket directory (Unix). Default:
    /// the OS temp dir (per-user on macOS).
    pub parent_dir: Option<PathBuf>,
}

/// A running built-in agent. Stops and removes its socket on drop.
pub struct AgentHandle {
    path: PathBuf,
    dir: Option<PathBuf>,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHandle")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl AgentHandle {
    /// Socket path (Unix) or pipe name (Windows) for `SSH_AUTH_SOCK` /
    /// `IdentityAgent`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stop serving and remove the socket.
    pub fn stop(self) {
        drop(self)
    }
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Start serving `service`.
pub async fn start_agent(
    service: AgentService,
    opts: &AgentListenOptions,
) -> Result<AgentHandle, AgentError> {
    platform::start(Arc::new(service), opts).await
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use tokio::net::UnixListener;

    pub async fn start(
        service: Arc<AgentService>,
        opts: &AgentListenOptions,
    ) -> Result<AgentHandle, AgentError> {
        let parent = opts.parent_dir.clone().unwrap_or_else(std::env::temp_dir);
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = parent.join(format!("cc-agent-{}", &id[..12]));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let owner_uid = std::fs::metadata(&dir)?.uid();
        let path = dir.join("agent.sock");
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                let _ = std::fs::remove_dir(&dir);
                return Err(e.into());
            }
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let (shutdown, mut rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = rx.changed() => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { continue };
                        // Defense in depth: only our own user may talk to the agent.
                        match stream.peer_cred() {
                            Ok(c) if c.uid() == owner_uid => {}
                            _ => {
                                tracing::warn!("agent: rejected connection from another user");
                                continue;
                            }
                        }
                        let svc = service.clone();
                        tokio::spawn(async move {
                            if let Err(e) = svc.serve_connection(stream).await {
                                tracing::debug!("agent connection ended: {e}");
                            }
                        });
                    }
                }
            }
        });
        Ok(AgentHandle {
            path,
            dir: Some(dir),
            shutdown,
            task,
        })
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use tokio::net::windows::named_pipe::ServerOptions;

    // TODO(ssh): restrict the pipe DACL to the current user explicitly — the
    // default descriptor grants write access only to the creator/admins/
    // SYSTEM, but an explicit DACL needs `windows-sys` + unsafe code (denied
    // workspace-wide); next: add a small audited helper crate. Untested on
    // Windows so far (no Windows CI runner yet); next: run the unit tests on
    // `windows-latest`.
    pub async fn start(
        service: Arc<AgentService>,
        _opts: &AgentListenOptions,
    ) -> Result<AgentHandle, AgentError> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!(r"\\.\pipe\consolecrypt-agent-{}", &id[..16]);
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)?;
        let (shutdown, mut rx) = watch::channel(false);
        let pipe = name.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = rx.changed() => break,
                    connected = server.connect() => {
                        if connected.is_err() {
                            break;
                        }
                        let next = match ServerOptions::new().reject_remote_clients(true).create(&pipe) {
                            Ok(n) => n,
                            Err(_) => break,
                        };
                        let current = std::mem::replace(&mut server, next);
                        let svc = service.clone();
                        tokio::spawn(async move {
                            let _ = svc.serve_connection(current).await;
                        });
                    }
                }
            }
        });
        Ok(AgentHandle {
            path: PathBuf::from(name),
            dir: None,
            shutdown,
            task,
        })
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;
    pub async fn start(
        _s: Arc<AgentService>,
        _o: &AgentListenOptions,
    ) -> Result<AgentHandle, AgentError> {
        Err(AgentError::Unsupported(
            "no agent transport on this platform".into(),
        ))
    }
}
