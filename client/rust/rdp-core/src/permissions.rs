//! Transient, profile-scoped grants; no local path is serialized to RDP or the UI.
use crate::RdpError;
use cap_std::fs::Dir;
use std::time::{Duration, Instant};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

pub const MAX_CLIPBOARD_TEXT: usize = 64 * 1024;
pub(crate) const MAX_DIRECTORY_GRANTS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionPermissions {
    pub clipboard_enabled: bool,
    pub directory_grant_id: Option<String>,
    pub directory_writable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryGrant {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCapabilities {
    pub clipboard_supported: bool,
    pub folder_supported: bool,
}
impl Default for SessionCapabilities {
    fn default() -> Self {
        Self {
            clipboard_supported: true,
            folder_supported: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            )),
        }
    }
}
pub(crate) struct GrantedDirectory {
    pub root: Arc<Dir>,
}
impl std::fmt::Debug for GrantedDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GrantedDirectory(<redacted>)")
    }
}
#[derive(Clone, Copy)]
pub(crate) enum ClipboardAction {
    Advertise,
    Request,
    Respond(u64, bool),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderStatus {
    Disabled,
    Pending,
    Ready,
    Denied,
    Unavailable,
}
impl FolderStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }
}
pub(crate) struct RedirectState {
    pub permissions: SessionPermissions,
    pub folder: Option<Arc<Dir>>,
    pub generation: u64,
    pub closed: bool,
    pub ready: bool,
    pub remote_unicode: bool,
    pub drive_ready: bool,
    pub drive_id: u32,
    pub folder_status: FolderStatus,
    pub folder_pending_since: Instant,
    pub clipboard_counts: [u64; 5],
    pub drive_handshake: u8,
    pub local_text: Option<Zeroizing<String>>,
    pub received_text: Option<Zeroizing<String>>,
    pub pending_request: Option<u64>,
    pub actions: VecDeque<ClipboardAction>,
}
impl std::fmt::Debug for RedirectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedirectState(<redacted>)")
    }
}
impl RedirectState {
    pub fn new(permissions: SessionPermissions, folder: Option<Arc<Dir>>) -> Self {
        let folder_status = if folder.is_some() {
            FolderStatus::Pending
        } else {
            FolderStatus::Disabled
        };
        Self {
            permissions,
            folder,
            generation: 1,
            closed: false,
            ready: false,
            remote_unicode: false,
            drive_ready: false,
            drive_id: crate::directory::DRIVE_ID,
            folder_status,
            folder_pending_since: Instant::now(),
            clipboard_counts: [0; 5],
            drive_handshake: 0,
            local_text: None,
            received_text: None,
            pending_request: None,
            actions: VecDeque::new(),
        }
    }
    pub fn current_folder_status(&self) -> FolderStatus {
        if self.closed || self.folder.is_none() {
            FolderStatus::Disabled
        } else if self.folder_status == FolderStatus::Pending
            && self.folder_pending_since.elapsed() >= Duration::from_secs(20)
        {
            FolderStatus::Unavailable
        } else {
            self.folder_status
        }
    }
    pub fn clear_text(&mut self) {
        self.local_text = None;
        self.received_text = None;
        // CLIPRDR has no request ID. Drain a cancelled wire request before allowing
        // another one, so a late response can never satisfy a newer user action.
        if self.pending_request.is_some() {
            self.pending_request = Some(0);
        }
        self.actions.clear();
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.clear_text();
        self.pending_request = None;
        self.folder = None;
        self.folder_status = FolderStatus::Disabled;
        self.permissions = Default::default();
    }
    pub fn enabled(&self) -> bool {
        !self.closed && self.permissions.clipboard_enabled
    }
    pub fn enqueue(&mut self, action: ClipboardAction) -> Result<(), RdpError> {
        if self.actions.len() >= 16 {
            return Err(RdpError::InputQueueFull);
        }
        self.actions.push_back(action);
        Ok(())
    }
}
pub(crate) type SharedRedirect = Arc<Mutex<RedirectState>>;
