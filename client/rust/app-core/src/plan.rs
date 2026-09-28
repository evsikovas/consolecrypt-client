//! Connection-planner preview with provenance (host editor "effective
//! settings"): every effective value with its source, the expanded route
//! and the planner's findings as **stable codes + args**, so the UI never
//! re-implements inheritance nor matches English messages.
//!
//! Values follow ssh-core's `ConnectionPlanner` exactly: port = host →
//! nearest group → 22; credential = host → nearest group (a password-prompt
//! host stops inheritance); username = host → credential → nearest group →
//! OS user; route = host jump chain → host jump profile → nearest group's
//! jump profile (recursively expanded by the real planner).

use crate::app::AppCore;
use crate::dto::{parse_id, HostDto, SshBackend};
use crate::error::{AppError, AppResult};
use crate::host_auth;
use crate::working_set::WorkingSet;
use async_trait::async_trait;
use cc_models::credential::Credential;
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy};
use cc_models::tunnel::Tunnel;
use cc_models::ObjectId;
use cc_ssh_core::{ConnectionPlanner, InventoryLookup, PlanError, PlannerDefaults};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// Where an effective value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSourceDto {
    /// Set on the host itself.
    Host,
    /// Inherited from a group (`source_id` / `source_name` = the group).
    Group,
    /// Taken from the host's credential (username).
    Credential,
    /// From a jump profile set on the host (route).
    JumpProfile,
    /// Built-in default (port 22, OS user name).
    AppDefault,
    /// Not set anywhere.
    Unset,
}

/// An effective value and its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedValueDto<T> {
    pub value: Option<T>,
    pub source: ValueSourceDto,
    /// Id of the group / credential / jump profile the value comes from.
    pub source_id: Option<String>,
    /// Its display name.
    pub source_name: Option<String>,
}

impl<T> ResolvedValueDto<T> {
    fn unset() -> Self {
        Self {
            value: None,
            source: ValueSourceDto::Unset,
            source_id: None,
            source_name: None,
        }
    }
    fn of(value: Option<T>, source: ValueSourceDto) -> Self {
        Self {
            value,
            source,
            source_id: None,
            source_name: None,
        }
    }
    fn from_group(value: T, g: &Group) -> Self {
        Self {
            value: Some(value),
            source: ValueSourceDto::Group,
            source_id: Some(g.id.to_string()),
            source_name: Some(g.name.clone()),
        }
    }
}

/// Severity of a planner finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSeverityDto {
    /// No connection can be made until fixed.
    Error,
    /// Connects, but probably not as intended.
    Warning,
    /// Explanation (e.g. "password asked when connecting").
    Info,
}

/// A planner finding: stable `code` + non-secret `args` (localize from
/// these; `message` is an English diagnostic only).
///
/// Codes — errors: `missing_host`, `jump_host_deleted` (`host_id`),
/// `missing_group` (`group_id`), `group_cycle` (`group_id`),
/// `group_too_deep`, `credential_missing` (`credential_id`),
/// `missing_jump_profile` (`jump_profile_id`), `missing_proxy`
/// (`proxy_id`), `jump_cycle`, `self_jump`, `too_many_hops`, `no_username`
/// (`host_id`, `name`), `invalid_host` (`field`, `rule`); warnings:
/// `no_credential`, `tunnel_public_bind` (`name`, `bind_host`),
/// `tunnel_skipped` (`name`, `rule`); info: `credential_prompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDiagnosticDto {
    pub code: String,
    pub severity: PlanSeverityDto,
    pub args: BTreeMap<String, String>,
    pub message: String,
}

impl PlanDiagnosticDto {
    fn new(code: &str, severity: PlanSeverityDto, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            severity,
            args: BTreeMap::new(),
            message: message.into(),
        }
    }
    fn arg(mut self, k: &str, v: impl ToString) -> Self {
        self.args.insert(k.to_owned(), v.to_string());
        self
    }
}

/// One hop of the expanded route (first = closest to the client).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteHopDto {
    pub host_id: String,
    pub name: String,
    pub username: Option<String>,
    pub address: String,
    pub port: u16,
    /// `name (user@address[:port])`.
    pub label: String,
}

/// Effective connection settings of a (possibly unsaved) host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPreviewDto {
    pub host_id: String,
    pub port: ResolvedValueDto<u16>,
    pub username: ResolvedValueDto<String>,
    /// `value` = credential id (`None` for password-prompt hosts).
    pub credential_id: ResolvedValueDto<String>,
    pub credential_name: Option<String>,
    /// Auth mode `PasswordPrompt`: the password is asked when connecting.
    pub prompts_for_password: bool,
    /// Expanded jump route (empty = direct).
    pub route: Vec<RouteHopDto>,
    /// Where the route comes from (`value` = jump profile name, if any).
    pub route_source: ResolvedValueDto<String>,
    /// Group names from the root to the host's group.
    pub group_path: Vec<String>,
    pub proxy_id: Option<String>,
    pub backend: SshBackend,
    /// `user@hop:port -> … -> user@target:port` when a plan could be built.
    pub route_description: Option<String>,
    pub diagnostics: Vec<PlanDiagnosticDto>,
    /// A plan could be built (no `error` diagnostic).
    pub ok: bool,
}

/// Diagnostic of a planning failure.
pub(crate) fn plan_error_diagnostic(e: &PlanError) -> PlanDiagnosticDto {
    use PlanSeverityDto::Error;
    let msg = e.to_string();
    match e {
        PlanError::MissingHost(id) => {
            PlanDiagnosticDto::new("missing_host", Error, msg).arg("host_id", id)
        }
        PlanError::MissingGroup(id) => {
            PlanDiagnosticDto::new("missing_group", Error, msg).arg("group_id", id)
        }
        PlanError::MissingCredential(id) => {
            PlanDiagnosticDto::new("credential_missing", Error, msg).arg("credential_id", id)
        }
        PlanError::MissingJumpProfile(id) => {
            PlanDiagnosticDto::new("missing_jump_profile", Error, msg).arg("jump_profile_id", id)
        }
        PlanError::MissingProxy(id) => {
            PlanDiagnosticDto::new("missing_proxy", Error, msg).arg("proxy_id", id)
        }
        PlanError::GroupCycle { group_id } => {
            PlanDiagnosticDto::new("group_cycle", Error, msg).arg("group_id", group_id)
        }
        PlanError::GroupTooDeep => PlanDiagnosticDto::new("group_too_deep", Error, msg),
        PlanError::JumpCycle { .. } => PlanDiagnosticDto::new("jump_cycle", Error, msg),
        PlanError::SelfJump { host_id } => {
            PlanDiagnosticDto::new("self_jump", Error, msg).arg("host_id", host_id)
        }
        PlanError::TooManyHops => PlanDiagnosticDto::new("too_many_hops", Error, msg),
        PlanError::MissingUsername { host_id, name } => {
            PlanDiagnosticDto::new("no_username", Error, msg)
                .arg("host_id", host_id)
                .arg("name", name)
        }
        PlanError::InvalidHost { host_id, error } => {
            PlanDiagnosticDto::new("invalid_host", Error, msg)
                .arg("host_id", host_id)
                .arg("field", error.field)
                .arg("rule", error.reason)
        }
        _ => PlanDiagnosticDto::new("plan_failed", Error, msg),
    }
}

/// The working set with one host replaced by a draft (planner input).
struct PreviewInventory {
    working: Arc<WorkingSet>,
    draft: Host,
}

impl PreviewInventory {
    fn raw_host(&self, id: ObjectId) -> Option<Host> {
        if id == self.draft.id {
            Some(self.draft.clone())
        } else {
            self.working.host(id)
        }
    }
}

#[async_trait]
impl InventoryLookup for PreviewInventory {
    async fn host(&self, id: ObjectId) -> Option<Host> {
        let mut h = self.raw_host(id)?;
        if host_auth::prompts_for_password(&h) {
            h.credential_id = Some(host_auth::prompt_credential_id(h.id));
        }
        Some(h)
    }
    async fn group(&self, id: ObjectId) -> Option<Group> {
        self.working.group(id)
    }
    async fn jump_profile(&self, id: ObjectId) -> Option<JumpProfile> {
        self.working.jump_profile(id)
    }
    async fn proxy(&self, id: ObjectId) -> Option<Proxy> {
        self.working.proxy(id)
    }
    async fn credential(&self, id: ObjectId) -> Option<Credential> {
        if let Some(c) = self.working.credential(id) {
            return Some(c);
        }
        let host = self.raw_host(host_auth::prompt_host_of(id))?;
        host_auth::prompts_for_password(&host).then(|| host_auth::prompt_credential(&host))
    }
    async fn tunnels_for_host(&self, host_id: ObjectId) -> Vec<Tunnel> {
        self.working
            .tunnels()
            .into_iter()
            .filter(|t| t.host_id == host_id)
            .collect()
    }
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref().filter(|v| !v.trim().is_empty()).cloned()
}

fn hop_label(name: &str, user: Option<&str>, address: &str, port: u16) -> String {
    let user = user.map(|u| format!("{u}@")).unwrap_or_default();
    let port = if port == cc_models::host::DEFAULT_SSH_PORT {
        String::new()
    } else {
        format!(":{port}")
    };
    format!("{name} ({user}{address}{port})")
}

/// Group chain of `host` (nearest first) and the finding that stopped it.
fn group_chain(ws: &WorkingSet, host: &Host) -> (Vec<Group>, Option<PlanDiagnosticDto>) {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut next = host.group_id;
    while let Some(gid) = next {
        if !seen.insert(gid) {
            return (
                chain,
                Some(plan_error_diagnostic(&PlanError::GroupCycle {
                    group_id: gid,
                })),
            );
        }
        if chain.len() >= cc_ssh_core::planner::MAX_GROUP_DEPTH {
            return (chain, Some(plan_error_diagnostic(&PlanError::GroupTooDeep)));
        }
        let Some(g) = ws.group(gid) else {
            return (
                chain,
                Some(plan_error_diagnostic(&PlanError::MissingGroup(gid))),
            );
        };
        next = g.parent_id;
        chain.push(g);
    }
    (chain, None)
}

/// Build the preview (pure over the working set; runs the real planner).
pub(crate) async fn preview(
    ws: Arc<WorkingSet>,
    draft: Host,
    stored_id: Option<ObjectId>,
) -> PlanPreviewDto {
    let mut diags: Vec<PlanDiagnosticDto> = Vec::new();
    if let Err(e) = draft.validate() {
        diags.push(plan_error_diagnostic(&PlanError::InvalidHost {
            host_id: draft.id,
            error: e,
        }));
    }
    let (groups, group_problem) = group_chain(&ws, &draft);
    diags.extend(group_problem);
    let prompts = host_auth::prompts_for_password(&draft);

    // Port.
    let port = match draft.port.filter(|p| *p != 0) {
        Some(p) => ResolvedValueDto::of(Some(p), ValueSourceDto::Host),
        None => groups
            .iter()
            .find_map(|g| g.inherited_port.filter(|p| *p != 0).map(|p| (p, g)))
            .map(|(p, g)| ResolvedValueDto::from_group(p, g))
            .unwrap_or_else(|| {
                ResolvedValueDto::of(
                    Some(cc_models::host::DEFAULT_SSH_PORT),
                    ValueSourceDto::AppDefault,
                )
            }),
    };

    // Credential.
    let credential_ref: ResolvedValueDto<String> = if prompts {
        ResolvedValueDto::of(None, ValueSourceDto::Host)
    } else if let Some(c) = draft.credential_id {
        ResolvedValueDto::of(Some(c.to_string()), ValueSourceDto::Host)
    } else {
        groups
            .iter()
            .find_map(|g| g.inherited_credential_id.map(|c| (c, g)))
            .map(|(c, g)| ResolvedValueDto::from_group(c.to_string(), g))
            .unwrap_or_else(ResolvedValueDto::unset)
    };
    let credential = credential_ref
        .value
        .as_deref()
        .and_then(|id| id.parse::<ObjectId>().ok())
        .and_then(|id| ws.credential(id));
    if prompts {
        diags.push(PlanDiagnosticDto::new(
            "credential_prompt",
            PlanSeverityDto::Info,
            "the password is asked when connecting",
        ));
    } else if let Some(id) = &credential_ref.value {
        if credential.is_none() {
            diags.push(
                PlanDiagnosticDto::new(
                    "credential_missing",
                    PlanSeverityDto::Error,
                    "the selected credential no longer exists",
                )
                .arg("credential_id", id),
            );
        }
    } else {
        diags.push(PlanDiagnosticDto::new(
            "no_credential",
            PlanSeverityDto::Warning,
            "no credential: the OS SSH agent / interactive authentication is tried",
        ));
    }

    // Username.
    let username = if let Some(u) = non_empty(&draft.username) {
        ResolvedValueDto::of(Some(u), ValueSourceDto::Host)
    } else if let Some((u, c)) = credential
        .as_ref()
        .and_then(|c| non_empty(&c.username).map(|u| (u, c)))
    {
        ResolvedValueDto {
            value: Some(u),
            source: ValueSourceDto::Credential,
            source_id: Some(c.id.to_string()),
            source_name: Some(c.name.clone()),
        }
    } else if let Some((u, g)) = groups
        .iter()
        .find_map(|g| non_empty(&g.inherited_username).map(|u| (u, g)))
    {
        ResolvedValueDto::from_group(u, g)
    } else if let Some(u) = crate::ssh::os_user() {
        ResolvedValueDto::of(Some(u), ValueSourceDto::AppDefault)
    } else {
        diags.push(
            PlanDiagnosticDto::new(
                "no_username",
                PlanSeverityDto::Error,
                "no username: set one on the host, its credential or a group",
            )
            .arg("host_id", draft.id)
            .arg("name", &draft.name),
        );
        ResolvedValueDto::unset()
    };

    // Route source (direct chain of the host).
    let (direct, route_source): (Vec<ObjectId>, ResolvedValueDto<String>) =
        if !draft.jump_chain.is_empty() {
            (
                draft.jump_chain.clone(),
                ResolvedValueDto::of(None, ValueSourceDto::Host),
            )
        } else {
            let from_host = draft.jump_profile_id.map(|p| (p, None));
            let from_group = || {
                groups
                    .iter()
                    .find_map(|g| g.inherited_jump_profile_id.map(|p| (p, Some(g))))
            };
            match from_host.or_else(from_group) {
                None => (Vec::new(), ResolvedValueDto::unset()),
                Some((pid, group)) => match ws.jump_profile(pid) {
                    None => {
                        diags.push(plan_error_diagnostic(&PlanError::MissingJumpProfile(pid)));
                        (Vec::new(), ResolvedValueDto::unset())
                    }
                    Some(p) => {
                        let src = match group {
                            Some(g) => ResolvedValueDto {
                                value: Some(p.name.clone()),
                                source: ValueSourceDto::Group,
                                source_id: Some(g.id.to_string()),
                                source_name: Some(g.name.clone()),
                            },
                            None => ResolvedValueDto {
                                value: Some(p.name.clone()),
                                source: ValueSourceDto::JumpProfile,
                                source_id: Some(p.id.to_string()),
                                source_name: Some(p.name.clone()),
                            },
                        };
                        (p.chain.clone(), src)
                    }
                },
            }
        };
    for id in &direct {
        if *id == draft.id {
            if !draft.jump_chain.is_empty() {
                diags.push(plan_error_diagnostic(&PlanError::SelfJump {
                    host_id: draft.id,
                }));
            }
        } else if ws.host(*id).is_none() {
            diags.push(
                PlanDiagnosticDto::new(
                    "jump_host_deleted",
                    PlanSeverityDto::Error,
                    "a jump host in the chain was deleted",
                )
                .arg("host_id", id),
            );
        }
    }

    // Tunnels (as the planner reports them).
    if let Some(id) = stored_id {
        for t in ws.tunnels().into_iter().filter(|t| t.host_id == id) {
            match t.validate() {
                Ok(()) if t.binds_publicly() => diags.push(
                    PlanDiagnosticDto::new(
                        "tunnel_public_bind",
                        PlanSeverityDto::Warning,
                        format!("tunnel '{}' is reachable from the network", t.name),
                    )
                    .arg("name", &t.name)
                    .arg("bind_host", &t.bind_host),
                ),
                Ok(()) => {}
                Err(e) => diags.push(
                    PlanDiagnosticDto::new(
                        "tunnel_skipped",
                        PlanSeverityDto::Warning,
                        format!("tunnel '{}' skipped: {e}", t.name),
                    )
                    .arg("name", &t.name)
                    .arg("rule", e.reason),
                ),
            }
        }
    }

    // The real planner: expanded route + any error the checks above missed.
    let inv = Arc::new(PreviewInventory {
        working: ws.clone(),
        draft: draft.clone(),
    });
    let planner = ConnectionPlanner::new(inv).with_defaults(PlannerDefaults {
        username: crate::ssh::os_user(),
        ..PlannerDefaults::default()
    });
    let (route, route_description) = match planner.plan(draft.id).await {
        Ok(plan) => {
            let route = plan
                .route
                .iter()
                .map(|h| RouteHopDto {
                    host_id: h.host_id.to_string(),
                    name: h.name.clone(),
                    username: Some(h.username.clone()),
                    address: h.endpoint.host.clone(),
                    port: h.endpoint.port,
                    label: hop_label(
                        &h.name,
                        Some(&h.username),
                        &h.endpoint.host,
                        h.endpoint.port,
                    ),
                })
                .collect::<Vec<_>>();
            (route, Some(plan.describe()))
        }
        Err(e) => {
            let mut d = plan_error_diagnostic(&e);
            if let PlanError::MissingHost(id) = &e {
                if *id != draft.id {
                    d.code = "jump_host_deleted".into();
                }
            }
            let dup = diags.iter().any(|x| x.code == d.code && x.args == d.args);
            if !dup && !diags.iter().any(|x| x.severity == PlanSeverityDto::Error) {
                diags.push(d);
            }
            // Best effort: the host's direct hops.
            let route = direct
                .iter()
                .filter(|id| **id != draft.id)
                .filter_map(|id| ws.host(*id))
                .map(|h| {
                    let p = h.port.unwrap_or(cc_models::host::DEFAULT_SSH_PORT);
                    RouteHopDto {
                        host_id: h.id.to_string(),
                        name: h.name.clone(),
                        username: non_empty(&h.username),
                        address: h.address.clone(),
                        port: p,
                        label: hop_label(&h.name, h.username.as_deref(), &h.address, p),
                    }
                })
                .collect();
            (route, None)
        }
    };
    let ok =
        route_description.is_some() && !diags.iter().any(|d| d.severity == PlanSeverityDto::Error);
    PlanPreviewDto {
        host_id: draft.id.to_string(),
        port,
        username,
        credential_name: if prompts {
            None
        } else {
            credential.as_ref().map(|c| c.name.clone())
        },
        credential_id: credential_ref,
        prompts_for_password: prompts,
        route,
        route_source,
        group_path: groups.iter().rev().map(|g| g.name.clone()).collect(),
        proxy_id: draft.proxy_id.map(|p| p.to_string()),
        backend: draft.backend,
        route_description,
        diagnostics: diags,
        ok,
    }
}

impl AppCore {
    /// Effective settings of a host with their provenance, the expanded
    /// route and planner findings as codes (host editor). `host` may be an
    /// unsaved draft (empty / unknown id) or edited values of a stored host;
    /// nothing is saved and no connection is made.
    pub async fn plan_preview(&self, host: HostDto) -> AppResult<PlanPreviewDto> {
        let (_, u) = self.unlocked().await?;
        let parsed = match host.id.trim() {
            "" => None,
            id => parse_id("id", id).ok(),
        };
        let stored = parsed.filter(|id| u.working().host(*id).is_some());
        // A draft keeps its (client-minted) id so self-references resolve.
        let id = parsed.unwrap_or_default();
        let existing = stored.and_then(|id| u.working().host(id));
        let draft = host
            .to_model_unchecked(id, existing.as_ref())
            .map_err(|e| match e {
                AppError::InvalidInput { .. } => e,
                other => AppError::internal(other),
            })?;
        Ok(preview(u.working().clone(), draft, stored).await)
    }

    /// [`AppCore::plan_preview`] of a stored host.
    pub async fn plan_preview_host(&self, host_id: String) -> AppResult<PlanPreviewDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        let h = crate::inventory::host_of(&u, id)?;
        Ok(preview(u.working().clone(), h, Some(id)).await)
    }
}
