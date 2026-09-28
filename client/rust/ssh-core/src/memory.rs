//! In-memory implementations of the ssh-core traits for tests, the CLI and
//! early integration. They hold everything in process memory only.

use crate::known_hosts::pattern_matches;
use crate::traits::{
    CredentialResolver, HostKeyDecision, HostKeyInfo, HostKeyPrompt, InventoryLookup,
    KnownHostsError, KnownHostsStore, ResolveError, ResolvedCredential,
};
use async_trait::async_trait;
use cc_models::credential::Credential;
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy};
use cc_models::known_host::KnownHost;
use cc_models::tunnel::Tunnel;
use cc_models::ObjectId;
use secrecy::SecretString;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

fn read<T>(l: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}
fn write<T>(l: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}

/// In-memory [`InventoryLookup`].
#[derive(Debug, Default)]
pub struct MemoryInventory {
    hosts: RwLock<HashMap<ObjectId, Host>>,
    groups: RwLock<HashMap<ObjectId, Group>>,
    jump_profiles: RwLock<HashMap<ObjectId, JumpProfile>>,
    proxies: RwLock<HashMap<ObjectId, Proxy>>,
    credentials: RwLock<HashMap<ObjectId, Credential>>,
    tunnels: RwLock<HashMap<ObjectId, Tunnel>>,
}

impl MemoryInventory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_host(&self, h: Host) -> ObjectId {
        let id = h.id;
        write(&self.hosts).insert(id, h);
        id
    }
    pub fn add_group(&self, g: Group) -> ObjectId {
        let id = g.id;
        write(&self.groups).insert(id, g);
        id
    }
    pub fn add_jump_profile(&self, p: JumpProfile) -> ObjectId {
        let id = p.id;
        write(&self.jump_profiles).insert(id, p);
        id
    }
    pub fn add_proxy(&self, p: Proxy) -> ObjectId {
        let id = p.id;
        write(&self.proxies).insert(id, p);
        id
    }
    pub fn add_credential(&self, c: Credential) -> ObjectId {
        let id = c.id;
        write(&self.credentials).insert(id, c);
        id
    }
    pub fn add_tunnel(&self, t: Tunnel) -> ObjectId {
        let id = t.id;
        write(&self.tunnels).insert(id, t);
        id
    }
    /// Mutate a stored host in place (tests).
    pub fn update_host(&self, id: ObjectId, f: impl FnOnce(&mut Host)) {
        if let Some(h) = write(&self.hosts).get_mut(&id) {
            f(h);
        }
    }
    /// Mutate a stored group in place (tests).
    pub fn update_group(&self, id: ObjectId, f: impl FnOnce(&mut Group)) {
        if let Some(g) = write(&self.groups).get_mut(&id) {
            f(g);
        }
    }
}

#[async_trait]
impl InventoryLookup for MemoryInventory {
    async fn host(&self, id: ObjectId) -> Option<Host> {
        read(&self.hosts).get(&id).cloned()
    }
    async fn group(&self, id: ObjectId) -> Option<Group> {
        read(&self.groups).get(&id).cloned()
    }
    async fn jump_profile(&self, id: ObjectId) -> Option<JumpProfile> {
        read(&self.jump_profiles).get(&id).cloned()
    }
    async fn proxy(&self, id: ObjectId) -> Option<Proxy> {
        read(&self.proxies).get(&id).cloned()
    }
    async fn credential(&self, id: ObjectId) -> Option<Credential> {
        read(&self.credentials).get(&id).cloned()
    }
    async fn tunnels_for_host(&self, host_id: ObjectId) -> Vec<Tunnel> {
        let mut v: Vec<Tunnel> = read(&self.tunnels)
            .values()
            .filter(|t| t.host_id == host_id)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

/// In-memory [`CredentialResolver`]: credential id → secret material.
#[derive(Debug, Default)]
pub struct MemoryCredentialResolver {
    secrets: RwLock<HashMap<ObjectId, ResolvedCredential>>,
    passphrases: RwLock<HashMap<ObjectId, SecretString>>,
    proxy_passwords: RwLock<HashMap<ObjectId, SecretString>>,
}

impl MemoryCredentialResolver {
    pub fn new() -> Self {
        Self::default()
    }
    /// Register secret material for a credential id.
    pub fn insert(&self, credential_id: ObjectId, secret: ResolvedCredential) {
        write(&self.secrets).insert(credential_id, secret);
    }
    /// Passphrase returned by [`CredentialResolver::prompt_passphrase`]
    /// (simulates the user typing it).
    pub fn set_prompt_passphrase(&self, credential_id: ObjectId, passphrase: SecretString) {
        write(&self.passphrases).insert(credential_id, passphrase);
    }
    pub fn set_proxy_password(&self, proxy_id: ObjectId, password: SecretString) {
        write(&self.proxy_passwords).insert(proxy_id, password);
    }
}

#[async_trait]
impl CredentialResolver for MemoryCredentialResolver {
    async fn resolve(&self, credential: &Credential) -> Result<ResolvedCredential, ResolveError> {
        if let Some(s) = read(&self.secrets).get(&credential.id) {
            return Ok(s.clone());
        }
        ResolvedCredential::agent_for(credential).ok_or(ResolveError::NoSecret(credential.id))
    }

    async fn prompt_passphrase(
        &self,
        credential: &Credential,
        attempt: u32,
    ) -> Result<Option<SecretString>, ResolveError> {
        if attempt > 0 {
            return Ok(None);
        }
        Ok(read(&self.passphrases).get(&credential.id).cloned())
    }

    async fn resolve_proxy_password(
        &self,
        proxy: &Proxy,
    ) -> Result<Option<SecretString>, ResolveError> {
        Ok(read(&self.proxy_passwords).get(&proxy.id).cloned())
    }
}

/// In-memory [`KnownHostsStore`].
#[derive(Debug, Default)]
pub struct MemoryKnownHosts {
    entries: RwLock<Vec<KnownHost>>,
}

impl MemoryKnownHosts {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_entries(entries: Vec<KnownHost>) -> Self {
        Self {
            entries: RwLock::new(entries),
        }
    }
    /// Snapshot of all entries.
    pub fn entries(&self) -> Vec<KnownHost> {
        read(&self.entries).clone()
    }
}

#[async_trait]
impl KnownHostsStore for MemoryKnownHosts {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<KnownHost>, KnownHostsError> {
        Ok(read(&self.entries)
            .iter()
            .filter(|e| pattern_matches(&e.host_pattern, host, port))
            .cloned()
            .collect())
    }
    async fn add(&self, entry: KnownHost) -> Result<(), KnownHostsError> {
        let mut entries = write(&self.entries);
        let dup = entries.iter().any(|e| {
            e.host_pattern == entry.host_pattern
                && e.public_key == entry.public_key
                && e.source == entry.source
                && e.revoked == entry.revoked
        });
        if !dup {
            entries.push(entry);
        }
        Ok(())
    }
    async fn revoke(&self, fingerprint_sha256: &str) -> Result<(), KnownHostsError> {
        for e in write(&self.entries).iter_mut() {
            if e.fingerprint_sha256 == fingerprint_sha256 {
                e.revoked = true;
            }
        }
        Ok(())
    }
    async fn remove(&self, id: ObjectId) -> Result<(), KnownHostsError> {
        write(&self.entries).retain(|e| e.id != id);
        Ok(())
    }
}

/// [`HostKeyPrompt`] that always returns the same decision and records
/// every prompt it received.
#[derive(Debug, Clone)]
pub struct FixedHostKeyPrompt {
    decision: HostKeyDecision,
    seen: Arc<Mutex<Vec<HostKeyInfo>>>,
}

impl FixedHostKeyPrompt {
    pub fn new(decision: HostKeyDecision) -> Self {
        Self {
            decision,
            seen: Arc::default(),
        }
    }
    /// Prompts received so far.
    pub fn prompts(&self) -> Vec<HostKeyInfo> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl HostKeyPrompt for FixedHostKeyPrompt {
    async fn confirm_unknown(&self, info: HostKeyInfo) -> HostKeyDecision {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(info);
        self.decision
    }
}
