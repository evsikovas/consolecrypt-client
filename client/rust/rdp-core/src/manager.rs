use crate::permissions::{
    ClipboardAction, DirectoryGrant, GrantedDirectory, RedirectState, SessionCapabilities,
    SessionPermissions, SharedRedirect, MAX_CLIPBOARD_TEXT, MAX_DIRECTORY_GRANTS,
};
use crate::{
    transport, CertificateInfo, ConnectConfig, Frame, Input, PollResult, RdpError,
    SessionDiagnostics, SessionStatus, MAX_SESSIONS,
};
use cap_std::fs::Dir;
use secrecy::SecretString;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Notify;
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use zeroize::Zeroize;
use zeroize::Zeroizing;

#[derive(Debug)]
pub(crate) struct SessionState {
    pub status: SessionStatus,
    pub frame: Option<Frame>,
    pub sequence: u64,
    pub resize_available: bool,
    pub diagnostics: SessionDiagnostics,
}
impl SessionState {
    pub(crate) fn clear_frame(&mut self) {
        if let Some(mut frame) = self.frame.take() {
            frame.rgba.zeroize();
        }
    }
}
#[derive(Debug)]
struct Session {
    state: Arc<Mutex<SessionState>>,
    input: mpsc::Sender<SessionCommand>,
    task: JoinHandle<()>,
    stopped: Arc<AtomicBool>,
    redirects: SharedRedirect,
    notify: Arc<Notify>,
}
pub(crate) enum SessionCommand {
    Inputs(Vec<Input>),
    ConfirmedPaste {
        ticket: String,
        done: oneshot::Sender<Result<(), RdpError>>,
    },
}
impl std::fmt::Debug for SessionCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionCommand(<redacted>)")
    }
}
struct ConfirmationCancellation {
    redirects: SharedRedirect,
    notify: Arc<Notify>,
    id: u64,
    armed: bool,
}
impl Drop for ConfirmationCancellation {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(mut s) = self.redirects.lock() {
                if s.offers.cancel_confirmation(self.id) {
                    s.local_text = None;
                    s.actions.retain(|action| !matches!(action, ClipboardAction::AdvertiseConfirmed(id) if *id == self.id));
                    let _ = s.enqueue(ClipboardAction::Advertise);
                    self.notify.notify_one();
                }
            }
        }
    }
}
struct PasteCancellation {
    redirects: SharedRedirect,
    ticket: String,
}
impl Drop for PasteCancellation {
    fn drop(&mut self) {
        if let Ok(mut s) = self.redirects.lock() {
            s.offers.cancel_ticket(&self.ticket);
        }
    }
}
#[derive(Debug)]
struct Inner {
    closed: bool,
    sessions: HashMap<String, Session>,
    grants: HashMap<String, GrantedDirectory>,
}
#[derive(Debug)]
pub struct RdpManager {
    inner: Mutex<Inner>,
    shutdown_tx: watch::Sender<bool>,
}

impl Default for RdpManager {
    fn default() -> Self {
        Self::new()
    }
}
impl RdpManager {
    pub fn new() -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            inner: Mutex::new(Inner {
                closed: false,
                sessions: HashMap::new(),
                grants: HashMap::new(),
            }),
            shutdown_tx,
        }
    }
    pub async fn probe_certificate(
        &self,
        address: &str,
        port: u16,
    ) -> Result<CertificateInfo, RdpError> {
        {
            let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
            if inner.closed {
                return Err(RdpError::SessionNotFound);
            }
        }
        let mut stop = self.shutdown_tx.subscribe();
        if *stop.borrow() {
            return Err(RdpError::SessionNotFound);
        }
        tokio::select! {
            biased;
            _ = stop.changed() => Err(RdpError::SessionNotFound),
            result = transport::probe(address, port) => result,
        }
    }
    pub fn connect(
        &self,
        config: ConnectConfig,
        password: SecretString,
    ) -> Result<String, RdpError> {
        self.connect_with_permissions(config, password, SessionPermissions::default())
    }
    pub fn connect_with_permissions(
        &self,
        config: ConnectConfig,
        password: SecretString,
        permissions: SessionPermissions,
    ) -> Result<String, RdpError> {
        config.validate()?;
        let mut inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        if inner.sessions.len() >= MAX_SESSIONS {
            return Err(RdpError::SessionLimit);
        }
        let adopted_grant = permissions.directory_grant_id.clone();
        let folder = resolve_folder(&inner, &permissions)?;
        let redirects = Arc::new(Mutex::new(RedirectState::new(permissions, folder)));
        let notify = Arc::new(Notify::new());
        let task_redirects = redirects.clone();
        let task_notify = notify.clone();
        let handle = tokio::runtime::Handle::try_current().map_err(|_| RdpError::Connection)?;
        let id = uuid::Uuid::new_v4().to_string();
        let (input, receiver) = mpsc::channel(32);
        let state = Arc::new(Mutex::new(SessionState {
            status: SessionStatus::Connecting,
            frame: None,
            sequence: 0,
            resize_available: false,
            diagnostics: SessionDiagnostics::default(),
        }));
        let task_state = state.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let task_stopped = stopped.clone();
        let task = handle.spawn(async move {
            let result = transport::run(
                config,
                password,
                receiver,
                task_state.clone(),
                task_stopped,
                task_redirects.clone(),
                task_notify,
            )
            .await;
            if let Ok(mut redirects) = task_redirects.lock() {
                redirects.close();
            }
            if let Ok(mut state) = task_state.lock() {
                state.clear_frame();
                state.status = match result {
                    Ok(()) => SessionStatus::Disconnected,
                    Err(e) => SessionStatus::Failed(e),
                };
            }
        });
        inner.sessions.insert(
            id.clone(),
            Session {
                state,
                input,
                task,
                stopped,
                redirects,
                notify,
            },
        );
        if let Some(grant) = adopted_grant {
            inner.grants.remove(&grant);
        }
        Ok(id)
    }
    pub fn capabilities(&self) -> SessionCapabilities {
        SessionCapabilities::default()
    }
    pub async fn pick_directory(&self) -> Result<Option<DirectoryGrant>, RdpError> {
        {
            let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
            if inner.closed {
                return Err(RdpError::SessionNotFound);
            }
        }
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            let mut stop = self.shutdown_tx.subscribe();
            if *stop.borrow() {
                return Err(RdpError::SessionNotFound);
            }
            let picked = tokio::select! {biased;_=stop.changed()=>return Err(RdpError::SessionNotFound),picked=rfd::AsyncFileDialog::new().set_title("Choose a folder to share with Remote Desktop").pick_folder()=>picked};
            let Some(picked) = picked else {
                return Ok(None);
            };
            self.register_directory(picked.path()).map(Some)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(RdpError::UnsupportedPlatform)
        }
    }
    fn register_directory(&self, path: &std::path::Path) -> Result<DirectoryGrant, RdpError> {
        if !self.capabilities().folder_supported {
            return Err(RdpError::UnsupportedPlatform);
        }
        let root = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| RdpError::DirectoryGrantUnavailable)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.len() <= 255)
            .unwrap_or("Folder")
            .to_owned();
        let mut inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        if inner.grants.len() >= MAX_DIRECTORY_GRANTS {
            return Err(RdpError::SessionLimit);
        }
        let info = DirectoryGrant {
            id: uuid::Uuid::new_v4().to_string(),
            name,
        };
        inner.grants.insert(
            info.id.clone(),
            GrantedDirectory {
                root: Arc::new(root),
            },
        );
        Ok(info)
    }
    pub fn release_directory_grant(&self, id: &str) -> Result<(), RdpError> {
        let mut inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        inner.grants.remove(id);
        Ok(())
    }
    pub fn set_permissions(
        &self,
        id: &str,
        permissions: SessionPermissions,
    ) -> Result<(), RdpError> {
        let mut inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
        if s.closed {
            return Err(RdpError::SessionNotFound);
        }
        let folder = if permissions.directory_grant_id.is_some()
            && permissions.directory_grant_id == s.permissions.directory_grant_id
        {
            s.folder
                .clone()
                .ok_or(RdpError::DirectoryGrantUnavailable)
                .map(Some)?
        } else {
            resolve_folder(&inner, &permissions)?
        };
        let adopted_grant = permissions.directory_grant_id.clone();
        let generation = s.generation.checked_add(1).ok_or(RdpError::Protocol)?;
        if s.permissions.directory_grant_id != permissions.directory_grant_id
            || s.permissions.directory_writable != permissions.directory_writable
        {
            s.drive_id = s.drive_id.checked_add(1).ok_or(RdpError::Protocol)?;
            s.folder_pending_since = std::time::Instant::now();
            s.folder_status = if folder.is_some() {
                crate::FolderStatus::Pending
            } else {
                crate::FolderStatus::Disabled
            };
        }
        s.generation = generation;
        s.clear_text();
        s.folder = folder;
        s.permissions = permissions;
        // clear_text emptied the queue; this permission transition cannot partially fail.
        s.actions.push_back(ClipboardAction::Advertise);
        session.notify.notify_one();
        drop(s);
        // Picker capability is transferred to this session, not retained until lock.
        // Its exact current Dir can be reused only by this session's permission changes.
        if let Some(grant) = adopted_grant {
            inner.grants.remove(&grant);
        }
        Ok(())
    }
    pub fn permissions(&self, id: &str) -> Result<SessionPermissions, RdpError> {
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
        if s.closed {
            return Err(RdpError::SessionNotFound);
        }
        Ok(s.permissions.clone())
    }
    pub fn offer_clipboard_text(&self, id: &str, text: String) -> Result<(), RdpError> {
        let text = Zeroizing::new(text);
        if text.len() > MAX_CLIPBOARD_TEXT || text.contains('\0') {
            return Err(RdpError::ClipboardLimit);
        }
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
        if !s.enabled() {
            return Err(RdpError::PermissionDenied);
        }
        if !s.ready {
            return Err(RdpError::ClipboardUnavailable);
        }
        s.offers.ordinary_changed()?;
        s.enqueue(ClipboardAction::Advertise)?;
        s.local_text = Some(text);
        session.notify.notify_one();
        Ok(())
    }
    /// Acknowledges exactly this offer; never injects keys on a delayed ACK.
    pub async fn offer_clipboard_text_confirmed(
        &self,
        id: &str,
        text: String,
    ) -> Result<String, RdpError> {
        let text = Zeroizing::new(text);
        if text.len() > MAX_CLIPBOARD_TEXT || text.contains('\0') {
            return Err(RdpError::ClipboardLimit);
        }
        if text.is_empty() {
            return Err(RdpError::ClipboardUnavailable);
        }
        let (receiver, mut cancellation) = {
            let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
            if inner.closed {
                return Err(RdpError::SessionNotFound);
            }
            let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
            let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
            if !s.enabled() {
                return Err(RdpError::PermissionDenied);
            }
            if !s.ready {
                return Err(RdpError::ClipboardUnavailable);
            }
            if s.actions.len() >= 16 {
                return Err(RdpError::InputQueueFull);
            }
            let (done, receiver) = oneshot::channel();
            let generation = s.generation;
            let offer_id = s.offers.begin_confirmation(generation, done)?;
            s.local_text = Some(text);
            s.actions
                .push_back(ClipboardAction::AdvertiseConfirmed(offer_id));
            session.notify.notify_one();
            (
                receiver,
                ConfirmationCancellation {
                    redirects: session.redirects.clone(),
                    notify: session.notify.clone(),
                    id: offer_id,
                    armed: true,
                },
            )
        };
        let result = tokio::time::timeout(crate::clipboard_offers::CONFIRM_TIMEOUT, receiver)
            .await
            .map_err(|_| RdpError::Timeout)?
            .map_err(|_| RdpError::SessionNotFound)??;
        cancellation.armed = false;
        Ok(result)
    }
    /// The caller rechecks focus/profile before this explicit, single-use commit.
    pub async fn commit_clipboard_paste(&self, id: &str, ticket: String) -> Result<(), RdpError> {
        if ticket.len() != 36 || uuid::Uuid::parse_str(&ticket).is_err() {
            return Err(RdpError::ClipboardUnavailable);
        }
        let (receiver, _cancellation) = {
            let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
            if inner.closed {
                return Err(RdpError::SessionNotFound);
            }
            let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
            if session
                .state
                .lock()
                .map_err(|_| RdpError::Connection)?
                .status
                != SessionStatus::Connected
            {
                return Err(RdpError::Connection);
            }
            let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
            let generation = s.generation;
            let enabled = s.enabled();
            s.offers.queue_paste(&ticket, generation, enabled)?;
            let (done, receiver) = oneshot::channel();
            if session
                .input
                .try_send(SessionCommand::ConfirmedPaste {
                    ticket: ticket.clone(),
                    done,
                })
                .is_err()
            {
                s.offers.cancel_ticket(&ticket);
                return Err(RdpError::InputQueueFull);
            }
            (
                receiver,
                PasteCancellation {
                    redirects: session.redirects.clone(),
                    ticket,
                },
            )
        };
        tokio::time::timeout(crate::clipboard_offers::CONFIRM_TIMEOUT, receiver)
            .await
            .map_err(|_| RdpError::Timeout)?
            .map_err(|_| RdpError::SessionNotFound)?
    }
    pub fn request_clipboard_text(&self, id: &str) -> Result<(), RdpError> {
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
        if !s.enabled() {
            return Err(RdpError::PermissionDenied);
        }
        if !s.ready || !s.remote_unicode {
            return Err(RdpError::ClipboardUnavailable);
        }
        if s.pending_request.is_some()
            || s.prepared_request.is_some()
            || s.actions
                .iter()
                .any(|a| matches!(a, ClipboardAction::Request))
        {
            return Err(RdpError::InputQueueFull);
        }
        s.received_text = None;
        s.enqueue(ClipboardAction::Request)?;
        session.notify.notify_one();
        Ok(())
    }
    pub fn take_clipboard_text(&self, id: &str) -> Result<Option<Zeroizing<String>>, RdpError> {
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let mut s = session.redirects.lock().map_err(|_| RdpError::Connection)?;
        if !s.enabled() {
            return Err(RdpError::PermissionDenied);
        }
        Ok(s.received_text.take())
    }

    pub fn poll(&self, id: &str) -> Result<PollResult, RdpError> {
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let mut state = session.state.lock().map_err(|_| RdpError::Connection)?;
        let folder_status = session
            .redirects
            .lock()
            .map_err(|_| RdpError::Connection)?
            .current_folder_status();
        Ok(PollResult {
            status: state.status.clone(),
            folder_status,
            frame: state.frame.take(),
        })
    }
    pub fn send_input(&self, id: &str, inputs: Vec<Input>) -> Result<(), RdpError> {
        transport::validate_inputs(&inputs)?;
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        {
            let state = session.state.lock().map_err(|_| RdpError::Connection)?;
            if state.status != SessionStatus::Connected {
                return Err(RdpError::Connection);
            }
            if inputs
                .iter()
                .any(|input| matches!(input, Input::Resize { .. }))
                && !state.resize_available
            {
                return Err(RdpError::ResizeUnavailable);
            }
        }
        if inputs
            .iter()
            .any(|input| matches!(input, Input::ReleaseAll))
        {
            session
                .redirects
                .lock()
                .map_err(|_| RdpError::Connection)?
                .offers
                .cancel_interaction();
        }
        session
            .input
            .try_send(SessionCommand::Inputs(inputs))
            .map_err(|_| RdpError::InputQueueFull)
    }
    /// Aggregate transport counters only; never typed text or packet bodies.
    pub fn diagnostics(&self, id: &str) -> Result<SessionDiagnostics, RdpError> {
        let inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        if inner.closed {
            return Err(RdpError::SessionNotFound);
        }
        let session = inner.sessions.get(id).ok_or(RdpError::SessionNotFound)?;
        let state = session.state.lock().map_err(|_| RdpError::Connection)?;
        Ok(state.diagnostics.clone())
    }
    pub fn disconnect(&self, id: &str) -> Result<(), RdpError> {
        let mut inner = self.inner.lock().map_err(|_| RdpError::Connection)?;
        let session = inner.sessions.remove(id).ok_or(RdpError::SessionNotFound)?;
        session.stopped.store(true, Ordering::Release);
        if let Ok(mut redirects) = session.redirects.lock() {
            redirects.close();
        }
        session.task.abort();
        if let Ok(mut state) = session.state.lock() {
            state.clear_frame();
            state.status = SessionStatus::Disconnected;
        }
        Ok(())
    }
    pub fn disconnect_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            Self::stop_all(&mut inner);
        }
    }
    pub fn shutdown(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.closed = true;
        inner.grants.clear();
        self.shutdown_tx.send_replace(true);
        Self::stop_all(&mut inner);
    }
    fn stop_all(inner: &mut Inner) {
        for (_, session) in inner.sessions.drain() {
            session.stopped.store(true, Ordering::Release);
            if let Ok(mut redirects) = session.redirects.lock() {
                redirects.close();
            }
            session.task.abort();
            if let Ok(mut state) = session.state.lock() {
                state.clear_frame();
                state.status = SessionStatus::Disconnected;
            }
        }
    }
}
impl Drop for RdpManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn resolve_folder(
    inner: &Inner,
    permissions: &SessionPermissions,
) -> Result<Option<Arc<Dir>>, RdpError> {
    if permissions.directory_writable && permissions.directory_grant_id.is_none() {
        return Err(RdpError::InvalidConfig);
    }
    permissions
        .directory_grant_id
        .as_ref()
        .map(|id| {
            if !SessionCapabilities::default().folder_supported {
                return Err(RdpError::UnsupportedPlatform);
            }
            inner
                .grants
                .get(id)
                .map(|grant| grant.root.clone())
                .ok_or(RdpError::DirectoryGrantUnavailable)
        })
        .transpose()
}

#[cfg(test)]
mod permission_tests {
    use super::*;
    fn local_session(
        manager: &RdpManager,
        permissions: SessionPermissions,
    ) -> (String, SharedRedirect) {
        let (id, redirects, _) = local_session_with_receiver(manager, permissions);
        (id, redirects)
    }
    fn local_session_with_receiver(
        manager: &RdpManager,
        permissions: SessionPermissions,
    ) -> (String, SharedRedirect, mpsc::Receiver<SessionCommand>) {
        let mut inner = manager.inner.lock().unwrap();
        let adopted_grant = permissions.directory_grant_id.clone();
        let folder = resolve_folder(&inner, &permissions).unwrap();
        let redirects = Arc::new(Mutex::new(RedirectState::new(permissions, folder)));
        {
            let mut s = redirects.lock().unwrap();
            s.ready = true;
            s.remote_unicode = true;
        }
        let state = Arc::new(Mutex::new(SessionState {
            status: SessionStatus::Connected,
            frame: None,
            sequence: 0,
            resize_available: false,
            diagnostics: Default::default(),
        }));
        let (input, receiver) = mpsc::channel(1);
        let id = uuid::Uuid::new_v4().to_string();
        let task = tokio::spawn(std::future::pending());
        inner.sessions.insert(
            id.clone(),
            Session {
                state,
                input,
                task,
                stopped: Arc::new(AtomicBool::new(false)),
                redirects: redirects.clone(),
                notify: Arc::new(Notify::new()),
            },
        );
        if let Some(grant) = adopted_grant {
            inner.grants.remove(&grant);
        }
        (id, redirects, receiver)
    }

    #[tokio::test]
    async fn dropping_confirmed_future_cancels_only_its_offer_and_drains_old_ack() {
        let manager = Arc::new(RdpManager::new());
        let (id, state) = local_session(
            &manager,
            SessionPermissions {
                clipboard_enabled: true,
                ..Default::default()
            },
        );
        let cloned = manager.clone();
        let cloned_id = id.clone();
        let pending = tokio::spawn(async move {
            cloned
                .offer_clipboard_text_confirmed(&cloned_id, "old synthetic".into())
                .await
        });
        tokio::task::yield_now().await;
        {
            let mut state = state.lock().unwrap();
            let offer_id = state
                .actions
                .iter()
                .find_map(|action| match action {
                    ClipboardAction::AdvertiseConfirmed(id) => Some(*id),
                    _ => None,
                })
                .unwrap();
            let text = state.local_text.clone();
            assert!(state
                .offers
                .begin_advertisement(1, true, text.as_ref(), Some(offer_id)));
        }
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        assert!(state.lock().unwrap().local_text.is_none());
        assert!(state.lock().unwrap().offers.in_flight());

        let cloned = manager.clone();
        let cloned_id = id.clone();
        let next = tokio::spawn(async move {
            cloned
                .offer_clipboard_text_confirmed(&cloned_id, "new synthetic".into())
                .await
        });
        tokio::task::yield_now().await;
        {
            let mut state = state.lock().unwrap();
            state.offers.acknowledge(true, 1, true);
        }
        tokio::task::yield_now().await;
        assert!(!next.is_finished());
        {
            let mut state = state.lock().unwrap();
            let offer_id = state
                .actions
                .iter()
                .find_map(|action| match action {
                    ClipboardAction::AdvertiseConfirmed(id) => Some(*id),
                    _ => None,
                })
                .unwrap();
            let text = state.local_text.clone();
            assert!(state
                .offers
                .begin_advertisement(1, true, text.as_ref(), Some(offer_id)));
            state.offers.acknowledge(true, 1, true);
        }
        assert!(next.await.unwrap().is_ok());
        manager.shutdown();
    }

    #[tokio::test]
    async fn lock_and_blur_cancel_confirmed_offer_without_waiting_for_network() {
        for lock in [false, true] {
            let manager = Arc::new(RdpManager::new());
            let (id, _, _receiver) = local_session_with_receiver(
                &manager,
                SessionPermissions {
                    clipboard_enabled: true,
                    ..Default::default()
                },
            );
            let cloned = manager.clone();
            let cloned_id = id.clone();
            let pending = tokio::spawn(async move {
                cloned
                    .offer_clipboard_text_confirmed(&cloned_id, "synthetic".into())
                    .await
            });
            tokio::task::yield_now().await;
            if lock {
                manager.shutdown();
            } else {
                manager.send_input(&id, vec![Input::ReleaseAll]).unwrap();
            }
            assert_eq!(pending.await.unwrap(), Err(RdpError::ClipboardUnavailable));
            if lock {
                assert_eq!(
                    manager
                        .offer_clipboard_text_confirmed(&id, "synthetic".into())
                        .await,
                    Err(RdpError::SessionNotFound)
                );
            }
        }
    }

    #[tokio::test]
    async fn queued_confirmed_commit_is_denied_after_revoke_before_dispatch() {
        let manager = Arc::new(RdpManager::new());
        let (id, state, mut receiver) = local_session_with_receiver(
            &manager,
            SessionPermissions {
                clipboard_enabled: true,
                ..Default::default()
            },
        );
        let (done, confirmation) = oneshot::channel();
        {
            let mut state = state.lock().unwrap();
            let offer_id = state.offers.begin_confirmation(1, done).unwrap();
            let text = Zeroizing::new("synthetic".to_owned());
            assert!(state
                .offers
                .begin_advertisement(1, true, Some(&text), Some(offer_id)));
            state.offers.acknowledge(true, 1, true);
        }
        let ticket = confirmation.await.unwrap().unwrap();
        let cloned = manager.clone();
        let cloned_id = id.clone();
        let commit =
            tokio::spawn(async move { cloned.commit_clipboard_paste(&cloned_id, ticket).await });
        let SessionCommand::ConfirmedPaste { ticket, done } = receiver.recv().await.unwrap() else {
            panic!("expected guarded commit");
        };
        manager
            .set_permissions(&id, SessionPermissions::default())
            .unwrap();
        let result = {
            let mut state = state.lock().unwrap();
            let generation = state.generation;
            let enabled = state.enabled();
            state
                .offers
                .consume_paste(&ticket, generation, enabled)
                .map(|_| ())
        };
        assert_eq!(result, Err(RdpError::PermissionDenied));
        done.send(result).unwrap();
        assert_eq!(commit.await.unwrap(), Err(RdpError::PermissionDenied));
        manager.shutdown();
    }

    #[tokio::test]
    async fn cancelled_queued_commit_never_becomes_a_later_paste() {
        let manager = Arc::new(RdpManager::new());
        let (id, state, mut receiver) = local_session_with_receiver(
            &manager,
            SessionPermissions {
                clipboard_enabled: true,
                ..Default::default()
            },
        );
        let (done, confirmation) = oneshot::channel();
        {
            let mut state = state.lock().unwrap();
            let offer_id = state.offers.begin_confirmation(1, done).unwrap();
            let text = Zeroizing::new("synthetic".to_owned());
            assert!(state
                .offers
                .begin_advertisement(1, true, Some(&text), Some(offer_id)));
            state.offers.acknowledge(true, 1, true);
        }
        let ticket = confirmation.await.unwrap().unwrap();
        let cloned = manager.clone();
        let cloned_id = id.clone();
        let commit =
            tokio::spawn(async move { cloned.commit_clipboard_paste(&cloned_id, ticket).await });
        let SessionCommand::ConfirmedPaste { ticket, done } = receiver.recv().await.unwrap() else {
            panic!("expected guarded commit");
        };
        commit.abort();
        assert!(commit.await.unwrap_err().is_cancelled());
        assert!(done.is_closed());
        assert!(state
            .lock()
            .unwrap()
            .offers
            .consume_paste(&ticket, 1, true)
            .is_err());
        manager.shutdown();
    }

    #[tokio::test]
    async fn real_confirmation_timeout_keeps_old_ack_as_a_draining_tombstone() {
        let manager = Arc::new(RdpManager::new());
        let (id, state) = local_session(
            &manager,
            SessionPermissions {
                clipboard_enabled: true,
                ..Default::default()
            },
        );
        let cloned = manager.clone();
        let cloned_id = id.clone();
        let pending = tokio::spawn(async move {
            cloned
                .offer_clipboard_text_confirmed(&cloned_id, "synthetic".into())
                .await
        });
        tokio::task::yield_now().await;
        {
            let mut state = state.lock().unwrap();
            let offer_id = state
                .actions
                .iter()
                .find_map(|action| match action {
                    ClipboardAction::AdvertiseConfirmed(id) => Some(*id),
                    _ => None,
                })
                .unwrap();
            let text = state.local_text.clone();
            assert!(state
                .offers
                .begin_advertisement(1, true, text.as_ref(), Some(offer_id)));
        }
        assert_eq!(pending.await.unwrap(), Err(RdpError::Timeout));
        assert!(state.lock().unwrap().offers.in_flight());
        assert!(state.lock().unwrap().local_text.is_none());
        state.lock().unwrap().offers.acknowledge(true, 1, true);
        assert!(!state.lock().unwrap().offers.in_flight());
        manager.shutdown();
    }
    #[tokio::test]
    async fn clipboard_requires_opt_in_and_bounded_explicit_request() {
        let manager = RdpManager::new();
        let (id, state) = local_session(&manager, Default::default());
        assert_eq!(
            manager.offer_clipboard_text(&id, "synthetic".into()),
            Err(RdpError::PermissionDenied)
        );
        assert_eq!(
            manager.request_clipboard_text(&id),
            Err(RdpError::PermissionDenied)
        );
        manager
            .set_permissions(
                &id,
                SessionPermissions {
                    clipboard_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        manager
            .offer_clipboard_text(&id, "synthetic / Привет".into())
            .unwrap();
        manager.request_clipboard_text(&id).unwrap();
        assert_eq!(
            manager.request_clipboard_text(&id),
            Err(RdpError::InputQueueFull)
        );
        assert_eq!(
            manager.offer_clipboard_text(&id, "x".repeat(MAX_CLIPBOARD_TEXT + 1)),
            Err(RdpError::ClipboardLimit)
        );
        assert_eq!(
            manager.offer_clipboard_text(&id, "nul\0value".into()),
            Err(RdpError::ClipboardLimit)
        );
        {
            let mut s = state.lock().unwrap();
            s.pending_request = Some(s.generation);
            s.received_text = Some(Zeroizing::new("synthetic".into()));
        }
        manager.set_permissions(&id, Default::default()).unwrap();
        let s = state.lock().unwrap();
        assert!(s.local_text.is_none() && s.received_text.is_none());
        assert_eq!(s.pending_request, Some(0)); // stale wire response must first drain
        drop(s);
        assert_eq!(
            manager.take_clipboard_text(&id).unwrap_err(),
            RdpError::PermissionDenied
        );
    }
    #[tokio::test]
    async fn native_grants_are_opaque_bounded_and_release_does_not_extend_active_access() {
        let manager = RdpManager::new();
        let folder = tempfile::tempdir().unwrap();
        let grant = manager.register_directory(folder.path()).unwrap();
        assert!(uuid::Uuid::parse_str(&grant.id).is_ok());
        assert!(!grant.name.contains(std::path::MAIN_SEPARATOR));
        let permissions = SessionPermissions {
            directory_grant_id: Some(grant.id.clone()),
            ..Default::default()
        };
        let (id, state) = local_session(&manager, permissions.clone());
        assert!(!manager.permissions(&id).unwrap().directory_writable);
        manager.release_directory_grant(&grant.id).unwrap();
        assert!(state.lock().unwrap().folder.is_some());
        manager.set_permissions(&id, permissions.clone()).unwrap();
        assert!(resolve_folder(&manager.inner.lock().unwrap(), &permissions).is_err());
        assert_eq!(
            manager.set_permissions(
                &id,
                SessionPermissions {
                    directory_writable: true,
                    ..Default::default()
                }
            ),
            Err(RdpError::InvalidConfig)
        );
        assert_eq!(
            manager.set_permissions(
                &id,
                SessionPermissions {
                    directory_grant_id: Some(folder.path().to_string_lossy().into_owned()),
                    ..Default::default()
                }
            ),
            Err(RdpError::DirectoryGrantUnavailable)
        );
        manager.set_permissions(&id, Default::default()).unwrap();
        assert!(state.lock().unwrap().folder.is_none());
        for _ in 0..MAX_DIRECTORY_GRANTS {
            manager.register_directory(folder.path()).unwrap();
        }
        assert_eq!(
            manager.register_directory(folder.path()),
            Err(RdpError::SessionLimit)
        );
    }
    #[tokio::test]
    async fn many_adopted_folder_cycles_do_not_exhaust_picker_quota() {
        let manager = RdpManager::new();
        let folder = tempfile::tempdir().unwrap();
        for _ in 0..MAX_DIRECTORY_GRANTS * 3 {
            let grant = manager.register_directory(folder.path()).unwrap();
            let permissions = SessionPermissions {
                directory_grant_id: Some(grant.id.clone()),
                ..Default::default()
            };
            let (id, state) = local_session(&manager, permissions.clone());
            assert!(manager.inner.lock().unwrap().grants.is_empty());
            manager.release_directory_grant(&grant.id).unwrap(); // controller cleanup stays idempotent
            manager
                .set_permissions(
                    &id,
                    SessionPermissions {
                        clipboard_enabled: true,
                        ..permissions.clone()
                    },
                )
                .unwrap();
            manager
                .set_permissions(
                    &id,
                    SessionPermissions {
                        directory_writable: true,
                        ..permissions
                    },
                )
                .unwrap();
            assert!(state.lock().unwrap().folder.is_some());
            manager.disconnect(&id).unwrap();
            assert!(state.lock().unwrap().folder.is_none());
        }
    }
    #[tokio::test]
    async fn failed_connect_releases_adopted_folder_even_with_failed_tab_retained() {
        use sha2::Digest;
        let fingerprint: [u8; 32] = sha2::Sha256::digest(uuid::Uuid::new_v4().as_bytes()).into();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });
        let manager = RdpManager::new();
        let folder = tempfile::tempdir().unwrap();
        let grant = manager.register_directory(folder.path()).unwrap();
        let id = manager
            .connect_with_permissions(
                ConnectConfig {
                    address: "127.0.0.1".into(),
                    port,
                    username: uuid::Uuid::new_v4().to_string(),
                    domain: None,
                    width: 1280,
                    height: 720,
                    accepted_certificate_sha256: fingerprint,
                },
                SecretString::from(uuid::Uuid::new_v4().to_string()),
                SessionPermissions {
                    directory_grant_id: Some(grant.id),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(manager.inner.lock().unwrap().grants.is_empty());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while matches!(manager.poll(&id).unwrap().status, SessionStatus::Connecting)
            && std::time::Instant::now() < until
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(matches!(
            manager.poll(&id).unwrap().status,
            SessionStatus::Failed(_)
        ));
        assert_eq!(
            manager.poll(&id).unwrap().folder_status,
            crate::FolderStatus::Disabled
        );
        {
            let inner = manager.inner.lock().unwrap();
            let redirects = inner.sessions.get(&id).unwrap().redirects.lock().unwrap();
            assert!(redirects.closed && redirects.folder.is_none());
        }
        server.await.unwrap();
    }
    #[tokio::test]
    async fn permanent_shutdown_clears_all_transient_grants_and_rejects_late_use() {
        let manager = RdpManager::new();
        let folder = tempfile::tempdir().unwrap();
        let grant = manager.register_directory(folder.path()).unwrap();
        let (id, state) = local_session(
            &manager,
            SessionPermissions {
                clipboard_enabled: true,
                directory_grant_id: Some(grant.id.clone()),
                directory_writable: true,
            },
        );
        manager
            .offer_clipboard_text(&id, "synthetic".into())
            .unwrap();
        manager.shutdown();
        assert!(manager.inner.lock().unwrap().grants.is_empty());
        {
            let s = state.lock().unwrap();
            assert!(
                s.closed
                    && s.folder.is_none()
                    && s.local_text.is_none()
                    && s.received_text.is_none()
            );
        }
        assert_eq!(
            manager.set_permissions(&id, Default::default()),
            Err(RdpError::SessionNotFound)
        );
        assert_eq!(
            manager.register_directory(folder.path()),
            Err(RdpError::SessionNotFound)
        );
        assert_eq!(
            manager.pick_directory().await,
            Err(RdpError::SessionNotFound)
        );
        assert_eq!(
            manager.request_clipboard_text(&id),
            Err(RdpError::SessionNotFound)
        );
    }
}

#[cfg(test)]
#[path = "live_redirect_tests.rs"]
mod live_redirect_tests;

#[cfg(test)]
#[path = "clipboard_live_tests.rs"]
mod clipboard_live_tests;
