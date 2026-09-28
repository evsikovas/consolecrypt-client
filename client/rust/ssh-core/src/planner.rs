//! Connection planner: turns a `Host` id into a fully resolved
//! [`ConnectionPlan`] (CLIENT_SPEC §6.2). The UI never assembles SSH
//! parameters itself.
//!
//! # Precedence rules
//!
//! * **Group inheritance** walks `Host::group_id → Group::parent_id → …`
//!   (nearest group first). A cycle in the parent chain is an error.
//! * **Port**: `host.port` > nearest group `inherited_port` > default (22).
//! * **Credential**: `host.credential_id` > nearest group
//!   `inherited_credential_id` > none (agent / `none` auth).
//! * **Username**: `host.username` > `credential.username` > nearest group
//!   `inherited_username` > [`PlannerDefaults::username`] (e.g. the OS user).
//! * **Jump chain**: non-empty `host.jump_chain` > `host.jump_profile_id` >
//!   nearest group `inherited_jump_profile_id` > direct.
//!   When a *profile* contains the host being planned (typical: a bastion
//!   lives in the same group whose profile names it), the chain is truncated
//!   to the hops *before* that host — a host never jumps through itself.
//!   An explicit `jump_chain` containing the host itself is an error.
//! * **Recursive jump hosts**: the first hop is itself planned; if it has
//!   its own route, that route is prepended (like OpenSSH `ProxyJump`).
//!   Later hops are reached through the previous hop, so their own routes
//!   are not consulted. Any host appearing twice (or the target appearing
//!   in its own route) is a [`PlanError::JumpCycle`].
//! * Every hop is planned with its *own* port / username / credential /
//!   host-key policy / keepalive (group inheritance applies to hops too).
//! * **Proxy**: `host.proxy_id` > first hop's `proxy_id`. It is only used
//!   to reach the first TCP endpoint.
//! * **Forwards**: all valid tunnel profiles of the host
//!   ([`InventoryLookup::tunnels_for_host`]); invalid ones are skipped with a
//!   warning.

use crate::traits::InventoryLookup;
use cc_models::credential::Credential;
use cc_models::group::Group;
use cc_models::host::{Host, HostKeyPolicy, Proxy, SshBackend, DEFAULT_SSH_PORT};
use cc_models::known_host::host_pattern;
use cc_models::tunnel::Tunnel;
use cc_models::{ObjectId, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

/// Upper bound on group nesting (defensive; cycles are detected anyway).
pub const MAX_GROUP_DEPTH: usize = 64;
/// Upper bound on the number of jump hops in a route.
pub const MAX_HOPS: usize = 32;

/// `host:port` pair as dialed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// OpenSSH known-hosts pattern (`host` for 22, else `[host]:port`).
    pub fn known_hosts_pattern(&self) -> String {
        host_pattern(&self.host, self.port)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

/// One SSH hop (a jump host or the target) with all parameters resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hop {
    pub host_id: ObjectId,
    pub name: String,
    pub endpoint: Endpoint,
    pub username: String,
    /// Credential model (no secret material). `None` = try the OS agent.
    pub credential: Option<Credential>,
    pub host_key_policy: HostKeyPolicy,
    /// Keepalive override; `None` = connector default.
    pub keepalive_secs: Option<u32>,
}

/// Fully resolved connection parameters for one host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionPlan {
    pub host_id: ObjectId,
    pub name: String,
    pub target: Endpoint,
    pub username: String,
    pub credential: Option<Credential>,
    pub host_key_policy: HostKeyPolicy,
    /// Jump hosts, first = closest to the client. Empty = direct.
    pub route: Vec<Hop>,
    /// Proxy used to reach the first TCP endpoint.
    pub proxy: Option<Proxy>,
    pub forwards: Vec<Tunnel>,
    pub backend: SshBackend,
    pub keepalive_secs: Option<u32>,
    pub agent_forwarding: bool,
    /// Non-fatal findings (skipped tunnels, public binds…) for the UI.
    pub warnings: Vec<String>,
}

impl ConnectionPlan {
    /// The target as a [`Hop`].
    pub fn target_hop(&self) -> Hop {
        Hop {
            host_id: self.host_id,
            name: self.name.clone(),
            endpoint: self.target.clone(),
            username: self.username.clone(),
            credential: self.credential.clone(),
            host_key_policy: self.host_key_policy,
            keepalive_secs: self.keepalive_secs,
        }
    }

    /// Route followed by the target: every SSH session to establish, in order.
    pub fn all_hops(&self) -> Vec<Hop> {
        let mut v = self.route.clone();
        v.push(self.target_hop());
        v
    }

    /// Human readable route, e.g. `alice@b1:22 -> bob@b2:22 -> root@db:22`.
    pub fn describe(&self) -> String {
        self.all_hops()
            .iter()
            .map(|h| format!("{}@{}", h.username, h.endpoint))
            .collect::<Vec<_>>()
            .join(" -> ")
    }
}

/// Defaults applied when neither the host nor its groups specify a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerDefaults {
    pub port: u16,
    /// Fallback username (app-core typically passes the OS user).
    pub username: Option<String>,
}

impl Default for PlannerDefaults {
    fn default() -> Self {
        Self {
            port: DEFAULT_SSH_PORT,
            username: None,
        }
    }
}

/// Planning failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    #[error("host {0} not found")]
    MissingHost(ObjectId),
    #[error("group {0} not found")]
    MissingGroup(ObjectId),
    #[error("credential {0} not found")]
    MissingCredential(ObjectId),
    #[error("jump profile {0} not found")]
    MissingJumpProfile(ObjectId),
    #[error("proxy {0} not found")]
    MissingProxy(ObjectId),
    #[error("group parent chain contains a cycle at group {group_id}")]
    GroupCycle { group_id: ObjectId },
    #[error("group nesting deeper than {MAX_GROUP_DEPTH}")]
    GroupTooDeep,
    #[error("jump chain contains a cycle: {path:?}")]
    JumpCycle { path: Vec<ObjectId> },
    #[error("host {host_id} cannot jump through itself")]
    SelfJump { host_id: ObjectId },
    #[error("route has more than {MAX_HOPS} hops")]
    TooManyHops,
    #[error(
        "no username for host {name} ({host_id}); set one on the host, its credential or group"
    )]
    MissingUsername { host_id: ObjectId, name: String },
    #[error("invalid host {host_id}: {error}")]
    InvalidHost {
        host_id: ObjectId,
        error: ValidationError,
    },
}

/// Effective settings of one host after group inheritance (no route yet).
#[derive(Debug, Clone)]
struct ResolvedHost {
    host: Host,
    groups: Vec<Group>,
    port: u16,
    username: String,
    credential: Option<Credential>,
}

/// Builds [`ConnectionPlan`]s from inventory objects.
#[derive(Clone)]
pub struct ConnectionPlanner {
    inventory: Arc<dyn InventoryLookup>,
    defaults: PlannerDefaults,
}

impl fmt::Debug for ConnectionPlanner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPlanner")
            .field("defaults", &self.defaults)
            .finish_non_exhaustive()
    }
}

impl ConnectionPlanner {
    pub fn new(inventory: Arc<dyn InventoryLookup>) -> Self {
        Self {
            inventory,
            defaults: PlannerDefaults::default(),
        }
    }

    pub fn with_defaults(mut self, defaults: PlannerDefaults) -> Self {
        self.defaults = defaults;
        self
    }

    /// Plan a connection to `host_id`.
    pub async fn plan(&self, host_id: ObjectId) -> Result<ConnectionPlan, PlanError> {
        let mut cache: HashMap<ObjectId, ResolvedHost> = HashMap::new();
        let target = self.resolve_cached(host_id, &mut cache).await?;
        let mut warnings = Vec::new();

        // Route of the target, then recursively prepend the first hop's route.
        let mut route = self.route_ids(&target).await?;
        let mut seen: HashSet<ObjectId> = HashSet::from([host_id]);
        for id in &route {
            if !seen.insert(*id) {
                let mut path = vec![host_id];
                path.extend(route.iter().copied());
                return Err(PlanError::JumpCycle { path });
            }
        }
        while let Some(&first) = route.first() {
            let first_resolved = self.resolve_cached(first, &mut cache).await?;
            let prefix = self.route_ids(&first_resolved).await?;
            if prefix.is_empty() {
                break;
            }
            for id in &prefix {
                if !seen.insert(*id) {
                    let mut path = prefix.clone();
                    path.extend(route.iter().copied());
                    path.push(host_id);
                    return Err(PlanError::JumpCycle { path });
                }
            }
            let mut new_route = prefix;
            new_route.extend(route);
            route = new_route;
            if route.len() > MAX_HOPS {
                return Err(PlanError::TooManyHops);
            }
        }
        if route.len() > MAX_HOPS {
            return Err(PlanError::TooManyHops);
        }

        let mut hops = Vec::with_capacity(route.len());
        for id in &route {
            let r = self.resolve_cached(*id, &mut cache).await?;
            hops.push(Hop {
                host_id: r.host.id,
                name: r.host.name.clone(),
                endpoint: Endpoint::new(r.host.address.trim(), r.port),
                username: r.username.clone(),
                credential: r.credential.clone(),
                host_key_policy: r.host.host_key_policy,
                keepalive_secs: r.host.keepalive_secs,
            });
        }

        // Proxy: target's, else the first hop's own proxy.
        let proxy_id = target.host.proxy_id.or_else(|| {
            route
                .first()
                .and_then(|id| cache.get(id))
                .and_then(|r| r.host.proxy_id)
        });
        let proxy = match proxy_id {
            Some(id) => Some(
                self.inventory
                    .proxy(id)
                    .await
                    .ok_or(PlanError::MissingProxy(id))?,
            ),
            None => None,
        };

        let mut forwards = Vec::new();
        for t in self.inventory.tunnels_for_host(host_id).await {
            match t.validate() {
                Ok(()) => {
                    if t.binds_publicly() {
                        warnings.push(format!(
                            "tunnel '{}' binds to {} (not loopback): it is reachable from the network",
                            t.name, t.bind_host
                        ));
                    }
                    forwards.push(t);
                }
                Err(e) => warnings.push(format!("tunnel '{}' skipped: {e}", t.name)),
            }
        }

        let target_host = &target.host;
        Ok(ConnectionPlan {
            host_id,
            name: target_host.name.clone(),
            target: Endpoint::new(target_host.address.trim(), target.port),
            username: target.username.clone(),
            credential: target.credential.clone(),
            host_key_policy: target_host.host_key_policy,
            route: hops,
            proxy,
            forwards,
            backend: target_host.backend,
            keepalive_secs: target_host.keepalive_secs,
            agent_forwarding: target_host.agent_forwarding,
            warnings,
        })
    }

    async fn resolve_cached(
        &self,
        id: ObjectId,
        cache: &mut HashMap<ObjectId, ResolvedHost>,
    ) -> Result<ResolvedHost, PlanError> {
        if let Some(r) = cache.get(&id) {
            return Ok(r.clone());
        }
        let host = self
            .inventory
            .host(id)
            .await
            .ok_or(PlanError::MissingHost(id))?;
        let r = self.resolve_host(host).await?;
        cache.insert(id, r.clone());
        Ok(r)
    }

    /// Group chain, nearest first.
    async fn group_chain(&self, host: &Host) -> Result<Vec<Group>, PlanError> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut next = host.group_id;
        while let Some(gid) = next {
            if !seen.insert(gid) {
                return Err(PlanError::GroupCycle { group_id: gid });
            }
            if chain.len() >= MAX_GROUP_DEPTH {
                return Err(PlanError::GroupTooDeep);
            }
            let g = self
                .inventory
                .group(gid)
                .await
                .ok_or(PlanError::MissingGroup(gid))?;
            next = g.parent_id;
            chain.push(g);
        }
        Ok(chain)
    }

    async fn resolve_host(&self, host: Host) -> Result<ResolvedHost, PlanError> {
        host.validate().map_err(|error| PlanError::InvalidHost {
            host_id: host.id,
            error,
        })?;
        let groups = self.group_chain(&host).await?;

        let port = host
            .port
            .or_else(|| groups.iter().find_map(|g| g.inherited_port))
            .filter(|p| *p != 0)
            .unwrap_or(self.defaults.port);

        let credential_id = host
            .credential_id
            .or_else(|| groups.iter().find_map(|g| g.inherited_credential_id));
        let credential = match credential_id {
            Some(cid) => Some(
                self.inventory
                    .credential(cid)
                    .await
                    .ok_or(PlanError::MissingCredential(cid))?,
            ),
            None => None,
        };

        let non_empty = |s: &Option<String>| s.as_ref().filter(|v| !v.trim().is_empty()).cloned();
        let username = non_empty(&host.username)
            .or_else(|| credential.as_ref().and_then(|c| non_empty(&c.username)))
            .or_else(|| groups.iter().find_map(|g| non_empty(&g.inherited_username)))
            .or_else(|| non_empty(&self.defaults.username))
            .ok_or_else(|| PlanError::MissingUsername {
                host_id: host.id,
                name: host.name.clone(),
            })?;

        Ok(ResolvedHost {
            host,
            groups,
            port,
            username,
            credential,
        })
    }

    /// Direct route ids of a resolved host (without recursive expansion).
    async fn route_ids(&self, r: &ResolvedHost) -> Result<Vec<ObjectId>, PlanError> {
        let host = &r.host;
        if !host.jump_chain.is_empty() {
            if host.jump_chain.contains(&host.id) {
                return Err(PlanError::SelfJump { host_id: host.id });
            }
            return Ok(host.jump_chain.clone());
        }
        let profile_id = host
            .jump_profile_id
            .or_else(|| r.groups.iter().find_map(|g| g.inherited_jump_profile_id));
        let Some(pid) = profile_id else {
            return Ok(Vec::new());
        };
        let profile = self
            .inventory
            .jump_profile(pid)
            .await
            .ok_or(PlanError::MissingJumpProfile(pid))?;
        // A host never jumps through itself: keep only the hops before it.
        let chain = match profile.chain.iter().position(|id| *id == host.id) {
            Some(pos) => profile.chain[..pos].to_vec(),
            None => profile.chain,
        };
        Ok(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryInventory;
    use cc_models::credential::CredentialKind;
    use cc_models::host::{JumpProfile, ProxyKind};
    use cc_models::tunnel::TunnelKind;

    fn planner(inv: &Arc<MemoryInventory>) -> ConnectionPlanner {
        ConnectionPlanner::new(inv.clone() as Arc<dyn InventoryLookup>).with_defaults(
            PlannerDefaults {
                port: 22,
                username: Some("osuser".into()),
            },
        )
    }

    fn host(name: &str, addr: &str) -> Host {
        Host::new(name, addr)
    }

    fn profile(chain: Vec<ObjectId>) -> JumpProfile {
        let now = chrono::Utc::now();
        JumpProfile {
            id: ObjectId::new(),
            name: "profile".into(),
            chain,
            created_at: now,
            updated_at: now,
        }
    }

    fn proxy(name: &str) -> Proxy {
        let now = chrono::Utc::now();
        Proxy {
            id: ObjectId::new(),
            name: name.into(),
            kind: ProxyKind::Socks5,
            address: "127.0.0.1".into(),
            port: 1080,
            username: None,
            password_secret_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn tunnel(host_id: ObjectId, name: &str, kind: TunnelKind, bind: &str) -> Tunnel {
        let now = chrono::Utc::now();
        Tunnel {
            id: ObjectId::new(),
            name: name.into(),
            kind,
            host_id,
            bind_host: bind.into(),
            bind_port: 15432,
            target_host: Some("db.internal".into()),
            target_port: Some(5432),
            auto_start: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn direct_host_uses_defaults() {
        let inv = Arc::new(MemoryInventory::new());
        let id = inv.add_host(host("db", "10.10.10.20"));
        let plan = planner(&inv).plan(id).await.unwrap();
        assert_eq!(plan.target, Endpoint::new("10.10.10.20", 22));
        assert_eq!(plan.username, "osuser");
        assert!(plan.route.is_empty());
        assert!(plan.credential.is_none());
        assert!(plan.proxy.is_none());
        assert_eq!(plan.host_key_policy, HostKeyPolicy::Ask);
        assert_eq!(plan.describe(), "osuser@10.10.10.20:22");
    }

    #[tokio::test]
    async fn missing_username_is_an_error() {
        let inv = Arc::new(MemoryInventory::new());
        let id = inv.add_host(host("db", "10.0.0.1"));
        let p = ConnectionPlanner::new(inv.clone());
        assert!(matches!(
            p.plan(id).await,
            Err(PlanError::MissingUsername { .. })
        ));
    }

    #[tokio::test]
    async fn port_precedence_host_group_chain_default() {
        let inv = Arc::new(MemoryInventory::new());
        let mut parent = Group::new("parent");
        parent.inherited_port = Some(2200);
        let parent_id = inv.add_group(parent);
        let mut child = Group::new("child");
        child.parent_id = Some(parent_id);
        let child_id = inv.add_group(child);

        let mut h = host("h", "h.example");
        h.group_id = Some(child_id);
        let hid = inv.add_host(h);
        // inherited from grandparent
        assert_eq!(planner(&inv).plan(hid).await.unwrap().target.port, 2200);
        // nearest group wins
        inv.update_group(child_id, |g| g.inherited_port = Some(2201));
        assert_eq!(planner(&inv).plan(hid).await.unwrap().target.port, 2201);
        // host wins
        inv.update_host(hid, |h| h.port = Some(2222));
        assert_eq!(planner(&inv).plan(hid).await.unwrap().target.port, 2222);
        // no group → default
        inv.update_host(hid, |h| {
            h.port = None;
            h.group_id = None;
        });
        assert_eq!(planner(&inv).plan(hid).await.unwrap().target.port, 22);
    }

    #[tokio::test]
    async fn username_precedence_host_credential_group_default() {
        let inv = Arc::new(MemoryInventory::new());
        let mut g = Group::new("g");
        g.inherited_username = Some("groupuser".into());
        let gid = inv.add_group(g);
        let mut cred = Credential::new("key", CredentialKind::SshPrivateKey);
        cred.username = Some("creduser".into());
        let cid = inv.add_credential(cred);

        let mut h = host("h", "h");
        h.group_id = Some(gid);
        let hid = inv.add_host(h);
        assert_eq!(planner(&inv).plan(hid).await.unwrap().username, "groupuser");

        inv.update_host(hid, |h| h.credential_id = Some(cid));
        assert_eq!(planner(&inv).plan(hid).await.unwrap().username, "creduser");

        inv.update_host(hid, |h| h.username = Some("hostuser".into()));
        assert_eq!(planner(&inv).plan(hid).await.unwrap().username, "hostuser");

        // Blank strings are treated as unset.
        inv.update_host(hid, |h| {
            h.username = Some("  ".into());
            h.credential_id = None;
            h.group_id = None;
        });
        assert_eq!(planner(&inv).plan(hid).await.unwrap().username, "osuser");
    }

    #[tokio::test]
    async fn credential_precedence_host_then_nearest_group() {
        let inv = Arc::new(MemoryInventory::new());
        let c_parent = inv.add_credential(Credential::new("p", CredentialKind::Password));
        let c_child = inv.add_credential(Credential::new("c", CredentialKind::Password));
        let c_host = inv.add_credential(Credential::new("h", CredentialKind::Password));
        let mut parent = Group::new("parent");
        parent.inherited_credential_id = Some(c_parent);
        let pid = inv.add_group(parent);
        let mut child = Group::new("child");
        child.parent_id = Some(pid);
        let cid = inv.add_group(child);
        let mut h = host("h", "h");
        h.group_id = Some(cid);
        let hid = inv.add_host(h);

        let plan = planner(&inv).plan(hid).await.unwrap();
        assert_eq!(plan.credential.unwrap().id, c_parent);
        inv.update_group(cid, |g| g.inherited_credential_id = Some(c_child));
        assert_eq!(
            planner(&inv)
                .plan(hid)
                .await
                .unwrap()
                .credential
                .unwrap()
                .id,
            c_child
        );
        inv.update_host(hid, |h| h.credential_id = Some(c_host));
        assert_eq!(
            planner(&inv)
                .plan(hid)
                .await
                .unwrap()
                .credential
                .unwrap()
                .id,
            c_host
        );
    }

    #[tokio::test]
    async fn group_cycle_is_detected() {
        let inv = Arc::new(MemoryInventory::new());
        let a = inv.add_group(Group::new("a"));
        let mut b = Group::new("b");
        b.parent_id = Some(a);
        let b = inv.add_group(b);
        inv.update_group(a, |g| g.parent_id = Some(b));
        let mut h = host("h", "h");
        h.group_id = Some(b);
        let hid = inv.add_host(h);
        assert!(matches!(
            planner(&inv).plan(hid).await,
            Err(PlanError::GroupCycle { .. })
        ));
    }

    #[tokio::test]
    async fn missing_references_are_errors() {
        let inv = Arc::new(MemoryInventory::new());
        let missing = ObjectId::new();
        let mut h = host("h", "h");
        h.group_id = Some(missing);
        let hid = inv.add_host(h);
        assert_eq!(
            planner(&inv).plan(hid).await,
            Err(PlanError::MissingGroup(missing))
        );
        inv.update_host(hid, |h| {
            h.group_id = None;
            h.credential_id = Some(missing);
        });
        assert_eq!(
            planner(&inv).plan(hid).await,
            Err(PlanError::MissingCredential(missing))
        );
        inv.update_host(hid, |h| {
            h.credential_id = None;
            h.jump_profile_id = Some(missing);
        });
        assert_eq!(
            planner(&inv).plan(hid).await,
            Err(PlanError::MissingJumpProfile(missing))
        );
        inv.update_host(hid, |h| {
            h.jump_profile_id = None;
            h.proxy_id = Some(missing);
        });
        assert_eq!(
            planner(&inv).plan(hid).await,
            Err(PlanError::MissingProxy(missing))
        );
        inv.update_host(hid, |h| {
            h.proxy_id = None;
            h.jump_chain = vec![missing];
        });
        assert_eq!(
            planner(&inv).plan(hid).await,
            Err(PlanError::MissingHost(missing))
        );
        assert_eq!(
            planner(&inv).plan(missing).await,
            Err(PlanError::MissingHost(missing))
        );
    }

    #[tokio::test]
    async fn jump_precedence_chain_over_profile_over_group_profile() {
        let inv = Arc::new(MemoryInventory::new());
        let b_group = inv.add_host(host("b-group", "bg"));
        let b_profile = inv.add_host(host("b-profile", "bp"));
        let b_chain = inv.add_host(host("b-chain", "bc"));
        let group_profile = inv.add_jump_profile(profile(vec![b_group]));
        let host_profile = inv.add_jump_profile(profile(vec![b_profile]));
        let mut g = Group::new("g");
        g.inherited_jump_profile_id = Some(group_profile);
        let gid = inv.add_group(g);
        let mut t = host("t", "t");
        t.group_id = Some(gid);
        let tid = inv.add_host(t);

        let ids = |p: &ConnectionPlan| p.route.iter().map(|h| h.host_id).collect::<Vec<_>>();
        assert_eq!(ids(&planner(&inv).plan(tid).await.unwrap()), vec![b_group]);
        inv.update_host(tid, |h| h.jump_profile_id = Some(host_profile));
        assert_eq!(
            ids(&planner(&inv).plan(tid).await.unwrap()),
            vec![b_profile]
        );
        inv.update_host(tid, |h| h.jump_chain = vec![b_chain]);
        assert_eq!(ids(&planner(&inv).plan(tid).await.unwrap()), vec![b_chain]);
    }

    #[tokio::test]
    async fn each_hop_is_planned_with_its_own_settings() {
        let inv = Arc::new(MemoryInventory::new());
        let bastion_cred = inv.add_credential(Credential::new("bk", CredentialKind::SshPrivateKey));
        let target_cred = inv.add_credential(Credential::new("tk", CredentialKind::Password));
        let mut edge = Group::new("edge");
        edge.inherited_username = Some("jump".into());
        edge.inherited_port = Some(2022);
        edge.inherited_credential_id = Some(bastion_cred);
        let edge = inv.add_group(edge);
        let mut b = host("bastion", "bastion.example");
        b.group_id = Some(edge);
        b.host_key_policy = HostKeyPolicy::Strict;
        b.keepalive_secs = Some(10);
        let bid = inv.add_host(b);
        let mut t = host("db", "10.10.10.20");
        t.username = Some("alex".into());
        t.credential_id = Some(target_cred);
        t.jump_chain = vec![bid];
        t.host_key_policy = HostKeyPolicy::AcceptNew;
        let tid = inv.add_host(t);

        let plan = planner(&inv).plan(tid).await.unwrap();
        assert_eq!(plan.route.len(), 1);
        let hop = &plan.route[0];
        assert_eq!(hop.endpoint, Endpoint::new("bastion.example", 2022));
        assert_eq!(hop.username, "jump");
        assert_eq!(hop.credential.as_ref().unwrap().id, bastion_cred);
        assert_eq!(hop.host_key_policy, HostKeyPolicy::Strict);
        assert_eq!(hop.keepalive_secs, Some(10));
        assert_eq!(plan.username, "alex");
        assert_eq!(plan.credential.as_ref().unwrap().id, target_cred);
        assert_eq!(plan.host_key_policy, HostKeyPolicy::AcceptNew);
        assert_eq!(plan.target.port, 22);
        assert_eq!(
            plan.describe(),
            "jump@bastion.example:2022 -> alex@10.10.10.20:22"
        );
        let all = plan.all_hops();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].host_id, tid);
    }

    #[tokio::test]
    async fn profile_containing_the_host_is_truncated() {
        // Typical setup: bastions live in the same group whose profile names them.
        let inv = Arc::new(MemoryInventory::new());
        let gid = inv.add_group(Group::new("prod"));
        let mut b1 = host("b1", "b1");
        b1.group_id = Some(gid);
        let b1 = inv.add_host(b1);
        let mut b2 = host("b2", "b2");
        b2.group_id = Some(gid);
        let b2 = inv.add_host(b2);
        let pid = inv.add_jump_profile(profile(vec![b1, b2]));
        inv.update_group(gid, |g| g.inherited_jump_profile_id = Some(pid));
        let mut t = host("t", "t");
        t.group_id = Some(gid);
        let tid = inv.add_host(t);

        let ids = |p: &ConnectionPlan| p.route.iter().map(|h| h.host_id).collect::<Vec<_>>();
        assert_eq!(ids(&planner(&inv).plan(tid).await.unwrap()), vec![b1, b2]);
        assert_eq!(ids(&planner(&inv).plan(b2).await.unwrap()), vec![b1]);
        assert!(planner(&inv).plan(b1).await.unwrap().route.is_empty());
    }

    #[tokio::test]
    async fn first_hop_route_is_expanded_recursively() {
        let inv = Arc::new(MemoryInventory::new());
        let outer = inv.add_host(host("outer", "outer"));
        let mut inner = host("inner", "inner");
        inner.jump_chain = vec![outer];
        let inner = inv.add_host(inner);
        let mut mid = host("mid", "mid");
        // mid's own route must NOT be used: it is not the first hop.
        mid.jump_chain = vec![outer];
        let mid = inv.add_host(mid);
        let mut t = host("t", "t");
        t.jump_chain = vec![inner, mid];
        let tid = inv.add_host(t);
        let plan = planner(&inv).plan(tid).await.unwrap();
        let ids: Vec<_> = plan.route.iter().map(|h| h.host_id).collect();
        assert_eq!(ids, vec![outer, inner, mid]);
    }

    #[tokio::test]
    async fn jump_cycles_are_detected() {
        let inv = Arc::new(MemoryInventory::new());
        // a -> [b], b -> [a]
        let a = inv.add_host(host("a", "a"));
        let mut b = host("b", "b");
        b.jump_chain = vec![a];
        let b = inv.add_host(b);
        inv.update_host(a, |h| h.jump_chain = vec![b]);
        assert!(matches!(
            planner(&inv).plan(a).await,
            Err(PlanError::JumpCycle { .. })
        ));

        // duplicate hop in an explicit chain
        let c = inv.add_host(host("c", "c"));
        let mut d = host("d", "d");
        d.jump_chain = vec![c, c];
        let d = inv.add_host(d);
        assert!(matches!(
            planner(&inv).plan(d).await,
            Err(PlanError::JumpCycle { .. })
        ));

        // x -> [y], y -> [z], z -> [y]  (cycle further out)
        let y = inv.add_host(host("y", "y"));
        let mut z = host("z", "z");
        z.jump_chain = vec![y];
        let z = inv.add_host(z);
        inv.update_host(y, |h| h.jump_chain = vec![z]);
        let mut x = host("x", "x");
        x.jump_chain = vec![y];
        let x = inv.add_host(x);
        assert!(matches!(
            planner(&inv).plan(x).await,
            Err(PlanError::JumpCycle { .. })
        ));
    }

    #[tokio::test]
    async fn explicit_self_jump_is_invalid() {
        let inv = Arc::new(MemoryInventory::new());
        let a = inv.add_host(host("a", "a"));
        inv.update_host(a, |h| h.jump_chain = vec![h.id]);
        assert!(matches!(
            planner(&inv).plan(a).await,
            Err(PlanError::InvalidHost { .. })
        ));
    }

    #[tokio::test]
    async fn proxy_precedence_target_then_first_hop() {
        let inv = Arc::new(MemoryInventory::new());
        let p_hop = inv.add_proxy(proxy("hop-proxy"));
        let p_target = inv.add_proxy(proxy("target-proxy"));
        let mut b = host("b", "b");
        b.proxy_id = Some(p_hop);
        let b = inv.add_host(b);
        let mut t = host("t", "t");
        t.jump_chain = vec![b];
        let tid = inv.add_host(t);
        assert_eq!(
            planner(&inv).plan(tid).await.unwrap().proxy.unwrap().id,
            p_hop
        );
        inv.update_host(tid, |h| h.proxy_id = Some(p_target));
        assert_eq!(
            planner(&inv).plan(tid).await.unwrap().proxy.unwrap().id,
            p_target
        );
    }

    #[tokio::test]
    async fn forwards_are_validated_and_public_binds_warned() {
        let inv = Arc::new(MemoryInventory::new());
        let tid = inv.add_host(host("t", "t"));
        inv.add_tunnel(tunnel(tid, "a-local", TunnelKind::Local, "127.0.0.1"));
        inv.add_tunnel(tunnel(tid, "b-public", TunnelKind::Dynamic, "0.0.0.0"));
        let mut bad = tunnel(tid, "c-bad", TunnelKind::Local, "127.0.0.1");
        bad.target_host = None;
        inv.add_tunnel(bad);
        let plan = planner(&inv).plan(tid).await.unwrap();
        assert_eq!(plan.forwards.len(), 2);
        assert_eq!(plan.warnings.len(), 2, "{:?}", plan.warnings);
        assert!(plan.warnings.iter().any(|w| w.contains("b-public")));
        assert!(plan.warnings.iter().any(|w| w.contains("c-bad")));
    }

    #[test]
    fn endpoint_display_and_pattern() {
        assert_eq!(Endpoint::new("h", 22).to_string(), "h:22");
        assert_eq!(Endpoint::new("::1", 2222).to_string(), "[::1]:2222");
        assert_eq!(Endpoint::new("H", 22).known_hosts_pattern(), "h");
        assert_eq!(Endpoint::new("h", 2222).known_hosts_pattern(), "[h]:2222");
    }
}
