//! SFTP extension shim.
//!
//! russh-sftp 3 negotiates extensions internally but neither exposes the
//! server's `SSH_FXP_VERSION` extension list nor a way to send arbitrary
//! `SSH_FXP_EXTENDED` requests through [`russh_sftp::client::SftpSession`].
//! We need `posix-rename@openssh.com` for atomic replace (plain SFTP v3
//! `RENAME` on OpenSSH refuses to overwrite an existing file), so the shim
//! sits on the byte stream between russh-sftp and the transport:
//!
//! * every packet is forwarded unchanged, whole (SFTP framing: `u32` length +
//!   payload), in both directions;
//! * the server's `SSH_FXP_VERSION` packet is parsed for its extension list;
//! * [`ExtChannel::request`] injects its own `SSH_FXP_EXTENDED` packets
//!   *between* russh-sftp's packets, using request ids from a reserved high
//!   range (russh-sftp counts up from 1), and diverts the matching replies
//!   before russh-sftp sees them.
//!
//! Packet contents are never logged.

use crate::SftpError;
use russh_sftp::protocol::FileAttributes;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, oneshot};

/// `posix-rename@openssh.com` (version "1"): rename that replaces the target.
pub(crate) const POSIX_RENAME: &str = "posix-rename@openssh.com";

const SSH_FXP_VERSION: u8 = 2;
const SSH_FXP_CLOSE: u8 = 4;
const SSH_FXP_OPENDIR: u8 = 11;
const SSH_FXP_READDIR: u8 = 12;
const SSH_FXP_STATUS: u8 = 101;
const SSH_FXP_HANDLE: u8 = 102;
const SSH_FXP_NAME: u8 = 104;
const SSH_FXP_EXTENDED: u8 = 200;
const SSH_FXP_EXTENDED_REPLY: u8 = 201;

/// Largest packet we forward (russh-sftp itself caps at 256 KiB).
const MAX_FRAME: usize = 4 << 20;
/// First request id used by the shim.
const FIRST_SHIM_ID: u32 = 0xF000_0000;
/// Buffer of the in-process pipe to russh-sftp.
const PIPE_BUF: usize = 512 * 1024;

type Pending = Arc<Mutex<HashMap<u32, oneshot::Sender<Vec<u8>>>>>;

struct Injected {
    id: u32,
    frame: Vec<u8>,
    reply: oneshot::Sender<Vec<u8>>,
}

/// Handle for extension requests on one SFTP connection.
pub(crate) struct ExtChannel {
    extensions: Arc<Mutex<Option<HashMap<String, String>>>>,
    inject: mpsc::Sender<Injected>,
    pending: Pending,
    next_id: AtomicU32,
    timeout: Duration,
}

impl std::fmt::Debug for ExtChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtChannel").finish_non_exhaustive()
    }
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SFTP packet length out of range",
        ));
    }
    let mut frame = vec![0u8; 4 + n];
    frame[..4].copy_from_slice(&len);
    r.read_exact(&mut frame[4..]).await?;
    Ok(Some(frame))
}

/// Minimal SSH wire reader over one packet.
struct Wire<'a>(&'a [u8]);

impl<'a> Wire<'a> {
    fn u8(&mut self) -> Option<u8> {
        let (b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(*b)
    }
    fn u32(&mut self) -> Option<u32> {
        if self.0.len() < 4 {
            return None;
        }
        let (a, rest) = self.0.split_at(4);
        self.0 = rest;
        Some(u32::from_be_bytes([a[0], a[1], a[2], a[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        let hi = u64::from(self.u32()?);
        let lo = u64::from(self.u32()?);
        Some((hi << 32) | lo)
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        if self.0.len() < n {
            return None;
        }
        let (a, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(a)
    }
}

fn put_str(buf: &mut Vec<u8>, s: &[u8]) {
    buf.extend_from_slice(&(s.len() as u32).to_be_bytes());
    buf.extend_from_slice(s);
}

/// Extension map of an `SSH_FXP_VERSION` frame.
fn parse_version(frame: &[u8]) -> Option<HashMap<String, String>> {
    let mut w = Wire(frame.get(4..)?);
    if w.u8()? != SSH_FXP_VERSION {
        return None;
    }
    let _version = w.u32()?;
    let mut map = HashMap::new();
    while !w.0.is_empty() {
        let name = w.bytes()?;
        let data = w.bytes()?;
        map.insert(
            String::from_utf8_lossy(name).into_owned(),
            String::from_utf8_lossy(data).into_owned(),
        );
    }
    Some(map)
}

/// Request id of a server reply frame (all reply types carry one).
fn reply_id(frame: &[u8]) -> Option<u32> {
    let mut w = Wire(frame.get(4..)?);
    match w.u8()? {
        SSH_FXP_STATUS..=105 | SSH_FXP_EXTENDED_REPLY => w.u32(),
        _ => None,
    }
}

/// Put the shim between russh-sftp (gets the returned pipe end) and `stream`.
pub(crate) fn wrap<S>(stream: S) -> (DuplexStream, ExtChannel)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (client_end, shim_end) = tokio::io::duplex(PIPE_BUF);
    let (mut from_client, mut to_client) = tokio::io::split(shim_end);
    let (mut from_server, mut to_server) = tokio::io::split(stream);
    let pending: Pending = Arc::default();
    let extensions: Arc<Mutex<Option<HashMap<String, String>>>> = Arc::default();
    let (up_tx, mut up_rx) = mpsc::channel::<Vec<u8>>(32);
    let (inject_tx, mut inject_rx) = mpsc::channel::<Injected>(8);

    // russh-sftp → shim (a dedicated reader keeps frame reads cancel-safe).
    tokio::spawn(async move {
        while let Ok(Some(frame)) = read_frame(&mut from_client).await {
            if up_tx.send(frame).await.is_err() {
                break;
            }
        }
    });

    // shim → server: russh-sftp's frames interleaved with injected ones.
    let p = pending.clone();
    tokio::spawn(async move {
        let mut inject_open = true;
        loop {
            let frame = tokio::select! {
                f = up_rx.recv() => match f {
                    Some(f) => f,
                    None => break,
                },
                i = inject_rx.recv(), if inject_open => match i {
                    Some(i) => {
                        p.lock().unwrap_or_else(|e| e.into_inner()).insert(i.id, i.reply);
                        i.frame
                    }
                    None => {
                        inject_open = false;
                        continue;
                    }
                },
            };
            if to_server.write_all(&frame).await.is_err() || to_server.flush().await.is_err() {
                break;
            }
        }
        let _ = to_server.shutdown().await;
    });

    // server → russh-sftp, minus the replies to injected requests.
    let p = pending.clone();
    let ext = extensions.clone();
    tokio::spawn(async move {
        while let Ok(Some(frame)) = read_frame(&mut from_server).await {
            if frame.get(4) == Some(&SSH_FXP_VERSION) {
                if let Some(map) = parse_version(&frame) {
                    *ext.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
                }
            } else if let Some(id) = reply_id(&frame).filter(|id| *id >= FIRST_SHIM_ID) {
                let waiter = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                if let Some(tx) = waiter {
                    let _ = tx.send(frame);
                    continue;
                }
            }
            if to_client.write_all(&frame).await.is_err() {
                break;
            }
        }
        // Fail injected requests still waiting for a reply.
        p.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let _ = to_client.shutdown().await;
    });

    (
        client_end,
        ExtChannel {
            extensions,
            inject: inject_tx,
            pending,
            next_id: AtomicU32::new(FIRST_SHIM_ID),
            timeout: Duration::from_secs(60),
        },
    )
}

impl ExtChannel {
    /// Extensions the server announced (empty until the handshake is done).
    pub(crate) fn extensions(&self) -> HashMap<String, String> {
        self.extensions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default()
    }

    pub(crate) fn supports(&self, name: &str, version: &str) -> bool {
        self.extensions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|m| m.get(name))
            .is_some_and(|v| v == version)
    }

    /// Send `SSH_FXP_EXTENDED(name, payload)`; returns the whole reply frame.
    async fn request(&self, name: &str, payload: &[u8]) -> Result<Vec<u8>, SftpError> {
        let mut body = Vec::with_capacity(4 + name.len() + payload.len());
        put_str(&mut body, name.as_bytes());
        body.extend_from_slice(payload);
        self.packet(SSH_FXP_EXTENDED, &body, name).await
    }

    /// Send one request packet (`ptype`, a shim id, `payload`) and return the
    /// whole reply frame. `label` names the request in errors.
    async fn packet(&self, ptype: u8, payload: &[u8], label: &str) -> Result<Vec<u8>, SftpError> {
        let id = self
            .next_id
            .fetch_add(1, Ordering::Relaxed)
            .max(FIRST_SHIM_ID);
        let mut body = Vec::with_capacity(5 + payload.len());
        body.push(ptype);
        body.extend_from_slice(&id.to_be_bytes());
        body.extend_from_slice(payload);
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);

        let closed = || SftpError::Remote {
            path: label.to_string(),
            message: "SFTP connection closed".into(),
        };
        let (tx, rx) = oneshot::channel();
        self.inject
            .send(Injected {
                id,
                frame,
                reply: tx,
            })
            .await
            .map_err(|_| closed())?;
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(closed()),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                Err(SftpError::Remote {
                    path: label.to_string(),
                    message: "timed out waiting for the server".into(),
                })
            }
        }
    }

    /// `OPENDIR` / `READDIR`… / `CLOSE` through the shim, keeping each
    /// entry's `longname` (russh-sftp's `read_dir` drops it; SFTP v3
    /// attributes carry only uid / gid, the names are in the longname).
    pub(crate) async fn read_dir_long(&self, path: &str) -> Result<Vec<NameEntry>, SftpError> {
        let mut p = Vec::with_capacity(4 + path.len());
        put_str(&mut p, path.as_bytes());
        let reply = self.packet(SSH_FXP_OPENDIR, &p, path).await?;
        let handle = parse_handle(&reply, path)?;
        let mut hp = Vec::with_capacity(4 + handle.len());
        put_str(&mut hp, &handle);
        let mut out = Vec::new();
        let result = loop {
            let reply = match self.packet(SSH_FXP_READDIR, &hp, path).await {
                Ok(r) => r,
                Err(e) => break Err(e),
            };
            match reply.get(4) {
                Some(&SSH_FXP_NAME) => match parse_names(&reply) {
                    Some(mut v) => out.append(&mut v),
                    None => {
                        break Err(SftpError::Remote {
                            path: path.to_string(),
                            message: "malformed SFTP name reply".into(),
                        })
                    }
                },
                _ => match status_code(&reply) {
                    // SSH_FX_EOF: listing complete.
                    Some(1) => break Ok(()),
                    _ => break status_result(&reply, path, "readdir"),
                },
            }
        };
        let _ = self.packet(SSH_FXP_CLOSE, &hp, path).await;
        result.map(|()| out)
    }

    /// `posix-rename@openssh.com`: rename `from` to `to`, replacing `to`
    /// atomically if it exists.
    pub(crate) async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        if !self.supports(POSIX_RENAME, "1") {
            return Err(SftpError::Unsupported(POSIX_RENAME.into()));
        }
        let mut payload = Vec::with_capacity(8 + from.len() + to.len());
        put_str(&mut payload, from.as_bytes());
        put_str(&mut payload, to.as_bytes());
        let reply = self.request(POSIX_RENAME, &payload).await?;
        status_result(&reply, from, POSIX_RENAME)
    }
}

/// One entry of an `SSH_FXP_NAME` reply.
#[derive(Debug, Clone)]
pub(crate) struct NameEntry {
    pub filename: String,
    pub longname: String,
    pub attrs: FileAttributes,
}

/// Status code of an `SSH_FXP_STATUS` frame.
fn status_code(frame: &[u8]) -> Option<u32> {
    let mut w = Wire(frame.get(4..)?);
    if w.u8()? != SSH_FXP_STATUS {
        return None;
    }
    let _id = w.u32()?;
    w.u32()
}

/// Handle of an `SSH_FXP_HANDLE` reply (or the error of a status reply).
fn parse_handle(frame: &[u8], path: &str) -> Result<Vec<u8>, SftpError> {
    let mut w = Wire(frame.get(4..).unwrap_or_default());
    if w.u8() == Some(SSH_FXP_HANDLE) {
        let _id = w.u32();
        if let Some(h) = w.bytes() {
            return Ok(h.to_vec());
        }
    }
    status_result(frame, path, "opendir")?;
    Err(SftpError::Remote {
        path: path.to_string(),
        message: "malformed SFTP handle reply".into(),
    })
}

/// SFTP v3 attributes (draft-ietf-secsh-filexfer-02 §5).
fn parse_attrs(w: &mut Wire<'_>) -> Option<FileAttributes> {
    let flags = w.u32()?;
    let mut a = FileAttributes::empty();
    if flags & 0x1 != 0 {
        a.size = Some(w.u64()?);
    }
    if flags & 0x2 != 0 {
        a.uid = Some(w.u32()?);
        a.gid = Some(w.u32()?);
    }
    if flags & 0x4 != 0 {
        a.permissions = Some(w.u32()?);
    }
    if flags & 0x8 != 0 {
        a.atime = Some(w.u32()?);
        a.mtime = Some(w.u32()?);
    }
    if flags & 0x8000_0000 != 0 {
        let n = w.u32()?;
        for _ in 0..n {
            w.bytes()?;
            w.bytes()?;
        }
    }
    Some(a)
}

/// Entries of an `SSH_FXP_NAME` frame.
fn parse_names(frame: &[u8]) -> Option<Vec<NameEntry>> {
    let mut w = Wire(frame.get(4..)?);
    if w.u8()? != SSH_FXP_NAME {
        return None;
    }
    let _id = w.u32()?;
    let count = w.u32()? as usize;
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let filename = String::from_utf8_lossy(w.bytes()?).into_owned();
        let longname = String::from_utf8_lossy(w.bytes()?).into_owned();
        let attrs = parse_attrs(&mut w)?;
        out.push(NameEntry {
            filename,
            longname,
            attrs,
        });
    }
    Some(out)
}

/// Owner and group names from an `ls -l` style `longname`
/// (`-rw-r--r--  1 alice staff 1234 Jan  1 12:00 name`). `None` when the
/// server sends another format.
pub(crate) fn longname_owner(longname: &str) -> Option<(String, String)> {
    let mut f = longname.split_whitespace();
    let mode = f.next()?;
    let mode_like = mode.len() >= 10
        && mode.starts_with(['-', 'd', 'l', 'c', 'b', 'p', 's'])
        && mode[1..10]
            .chars()
            .all(|c| matches!(c, 'r' | 'w' | 'x' | '-' | 's' | 'S' | 't' | 'T'));
    if !mode_like {
        return None;
    }
    let _links = f.next()?;
    let owner = f.next()?;
    let group = f.next()?;
    f.next()?; // size: the format really is `ls -l`
    Some((owner.to_owned(), group.to_owned()))
}

/// Map an `SSH_FXP_STATUS` reply frame to a result (`op` names the
/// operation for "unsupported").
fn status_result(frame: &[u8], path: &str, op: &str) -> Result<(), SftpError> {
    let bad = || SftpError::Remote {
        path: path.to_string(),
        message: "malformed SFTP status reply".into(),
    };
    let mut w = Wire(frame.get(4..).ok_or_else(bad)?);
    if w.u8().ok_or_else(bad)? != SSH_FXP_STATUS {
        return Err(bad());
    }
    let _id = w.u32().ok_or_else(bad)?;
    let code = w.u32().ok_or_else(bad)?;
    let message = w
        .bytes()
        .map(|m| String::from_utf8_lossy(m).into_owned())
        .unwrap_or_default();
    match code {
        0 => Ok(()),
        2 => Err(SftpError::NotFound(path.to_string())),
        3 => Err(SftpError::PermissionDenied(path.to_string())),
        8 => Err(SftpError::Unsupported(op.into())),
        _ => Err(SftpError::Remote {
            path: path.to_string(),
            message: if message.is_empty() {
                format!("status {code}")
            } else {
                message
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_and_replies() {
        let mut body = vec![SSH_FXP_VERSION];
        body.extend_from_slice(&3u32.to_be_bytes());
        put_str(&mut body, POSIX_RENAME.as_bytes());
        put_str(&mut body, b"1");
        put_str(&mut body, b"fsync@openssh.com");
        put_str(&mut body, b"1");
        let mut frame = (body.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&body);
        let m = parse_version(&frame).unwrap();
        assert_eq!(m.get(POSIX_RENAME).map(String::as_str), Some("1"));
        assert_eq!(m.len(), 2);
        assert_eq!(reply_id(&frame), None);
        // truncated extension list is rejected, not mis-parsed
        assert!(parse_version(&frame[..frame.len() - 2]).is_none());

        let mut st = vec![SSH_FXP_STATUS];
        st.extend_from_slice(&0xF000_0001u32.to_be_bytes());
        st.extend_from_slice(&4u32.to_be_bytes());
        put_str(&mut st, b"Failure");
        put_str(&mut st, b"");
        let mut frame = (st.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&st);
        assert_eq!(reply_id(&frame), Some(0xF000_0001));
        assert!(matches!(
            status_result(&frame, "/x", POSIX_RENAME),
            Err(SftpError::Remote { ref message, .. }) if message == "Failure"
        ));
        assert_eq!(status_code(&frame), Some(4));
    }

    #[test]
    fn parses_name_replies_and_longnames() {
        let mut body = vec![SSH_FXP_NAME];
        body.extend_from_slice(&0xF000_0002u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        put_str(&mut body, b"notes.txt");
        put_str(
            &mut body,
            b"-rw-r--r--    1 alice    staff        12 Jan  1 12:00 notes.txt",
        );
        // flags: size + uidgid + permissions + acmodtime
        body.extend_from_slice(&0xFu32.to_be_bytes());
        body.extend_from_slice(&12u64.to_be_bytes());
        body.extend_from_slice(&501u32.to_be_bytes());
        body.extend_from_slice(&20u32.to_be_bytes());
        body.extend_from_slice(&0o100644u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&2u32.to_be_bytes());
        let mut frame = (body.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&body);
        let names = parse_names(&frame).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].filename, "notes.txt");
        assert_eq!(names[0].attrs.size, Some(12));
        assert_eq!(names[0].attrs.uid, Some(501));
        assert_eq!(names[0].attrs.mtime, Some(2));
        assert_eq!(
            longname_owner(&names[0].longname),
            Some(("alice".into(), "staff".into()))
        );
        assert!(parse_names(&frame[..frame.len() - 3]).is_none());
        assert_eq!(longname_owner("notes.txt"), None);
        assert_eq!(longname_owner("drwxr-xr-x 2 root"), None);
    }
}
