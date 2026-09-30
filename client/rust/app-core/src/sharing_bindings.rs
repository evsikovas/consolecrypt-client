//! Explicit recipient-side copies. All writes use the captured unlocked profile,
//! never a new active profile acquired after an asynchronous shared-state read.
use crate::dto::{parse_id, EditableDto};
use crate::sharing_projection::SharedProjection;
use crate::{AppCore, AppError, AppResult, HostDto, SnippetDto};
use cc_models::host::Host;
use cc_models::snippet::{RiskLevel, Snippet, SnippetSource, SnippetVariable};
use cc_models::VaultObject;
use cc_protocol::sharing::SharedItemKind;
use std::collections::HashMap;
use std::sync::Arc;

const INSTANCE: &str = "cc.shared.instance";
const SHARE: &str = "cc.shared.share";

/// A partial or malformed binding must not turn a shared endpoint into an
/// ordinary private endpoint. Only an explicit detach removes both fields.
pub(crate) fn host_sharing_binding(host: &Host) -> AppResult<Option<(String, String)>> {
    match (host.metadata.get(INSTANCE), host.metadata.get(SHARE)) {
        (None, None) => Ok(None),
        (Some(instance), Some(share)) => {
            let instance_id = uuid::Uuid::parse_str(instance)
                .map_err(|_| AppError::invalid("shared_host", "binding_metadata_invalid"))?;
            let share_id = uuid::Uuid::parse_str(share)
                .map_err(|_| AppError::invalid("shared_host", "binding_metadata_invalid"))?;
            if instance_id.is_nil()
                || share_id.is_nil()
                || instance_id.to_string() != *instance
                || share_id.to_string() != *share
            {
                return Err(AppError::invalid("shared_host", "binding_metadata_invalid"));
            }
            Ok(Some((instance.clone(), share.clone())))
        }
        _ => Err(AppError::invalid("shared_host", "binding_metadata_invalid")),
    }
}

#[derive(PartialEq, Eq)]
struct VerifiedHostBinding {
    binding: (String, String),
    address: String,
    port: Option<u16>,
    username: Option<String>,
}

/// The exact route verified before any network action. Runtime consumers must
/// use this immutable plan rather than planning again after an async prompt.
pub(crate) struct PreparedSharingConnection {
    pub unlocked: Arc<crate::session::Unlocked>,
    pub plan: cc_ssh_core::ConnectionPlan,
}

impl PreparedSharingConnection {
    pub(crate) async fn run<T>(
        &self,
        operation: impl std::future::Future<Output = AppResult<T>>,
    ) -> AppResult<T> {
        let mut shutdown = self.unlocked.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let result = tokio::select! {
            biased;
            _ = shutdown.changed() => Err(AppError::VaultLocked),
            result = operation => result,
        }?;
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        Ok(result)
    }
}

impl VerifiedHostBinding {
    fn new(host: &Host, binding: (String, String)) -> Self {
        Self {
            binding,
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
        }
    }
}

impl AppCore {
    /// Copy only verified public fields. Own credentials and known-host policy
    /// stay private; sharing never chooses a credential for the recipient.
    pub async fn sharing_copy_host(
        &self,
        share_id: String,
        credential_id: Option<String>,
    ) -> AppResult<HostDto> {
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Host,
            move |u, snapshot, projection| {
                Box::pin(async move {
                    let SharedProjection::Host(public) = projection else {
                        return Err(AppError::invalid("shared_host", "incorrect shared kind"));
                    };
                    let mut host = Host::new(&public.name, &public.address);
                    host.port = Some(public.port);
                    host.username = public.username.clone();
                    host.keepalive_secs = public.keepalive_secs;
                    host.tags = public.tags.clone();
                    host.notes = public.notes.clone().unwrap_or_default();
                    if let Some(id) = credential_id {
                        let id = parse_id("credential_id", &id)?;
                        if u.working().credential(id).is_none() {
                            return Err(AppError::not_found("credential", id));
                        }
                        if crate::host_auth::owner_of(u, id).is_some() {
                            return Err(AppError::invalid(
                                "credential_id",
                                "credential belongs to another host",
                            ));
                        }
                        host.credential_id = Some(id);
                    } else {
                        host.metadata
                            .insert(crate::host_auth::META_AUTH_PROMPT.into(), "password".into());
                    }
                    host.metadata.insert(
                        INSTANCE.into(),
                        snapshot.binding().server_instance_id.to_string(),
                    );
                    host.metadata
                        .insert(SHARE.into(), snapshot.binding().share_id.to_string());
                    crate::inventory::validate_refs(u.working(), &VaultObject::Host(host.clone()))?;
                    let dto = HostDto::from_model(&host);
                    u.writer.put(VaultObject::Host(host)).await?;
                    Ok(dto)
                })
            },
        )
        .await
    }

    /// Received commands are imported at Unknown risk and never executed here.
    pub async fn sharing_copy_snippet(&self, share_id: String) -> AppResult<SnippetDto> {
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Snippet,
            |u, _, projection| {
                Box::pin(async move {
                    let SharedProjection::Snippet(public) = projection else {
                        return Err(AppError::invalid("shared_snippet", "incorrect shared kind"));
                    };
                    let now = chrono::Utc::now();
                    let snippet = Snippet {
                        id: cc_models::ObjectId::new(),
                        name: public.name.clone(),
                        description: public.description.clone(),
                        package_name: None,
                        catalog_id: None,
                        snippet_type: public.snippet_type,
                        shell: public.shell.clone(),
                        template: public.template.clone(),
                        variables: public
                            .variables
                            .iter()
                            .map(|v| SnippetVariable {
                                name: v.name.clone(),
                                description: v.description.clone(),
                                required: v.required,
                                default: None,
                            })
                            .collect(),
                        tags: public.tags.clone(),
                        risk_level: RiskLevel::Unknown,
                        source: SnippetSource::Imported,
                        created_by: None,
                        created_at: now,
                        updated_at: now,
                        last_used_at: None,
                        usage_count: 0,
                    };
                    let dto = SnippetDto::from_model(&snippet);
                    u.writer.put(VaultObject::Snippet(snippet)).await?;
                    Ok(dto)
                })
            },
        )
        .await
    }

    /// Refresh a bound private host before a new connection. An endpoint change
    /// requires a separate explicit review, rather than sending own credentials
    /// to a newly supplied address. Lock/offline/revoked access fails closed.
    pub async fn sharing_refresh_bound_host(
        &self,
        host_id: String,
        confirm_endpoint_change: bool,
    ) -> AppResult<HostDto> {
        if confirm_endpoint_change {
            return Err(AppError::invalid(
                "shared_host",
                "exact_endpoint_confirmation_required",
            ));
        }
        self.sharing_refresh_bound_host_inner(host_id, None).await
    }

    /// Approve exactly the endpoint shown by the UI. A later signed endpoint
    /// change cannot inherit an earlier generic confirmation.
    pub async fn sharing_refresh_bound_host_expected(
        &self,
        host_id: String,
        expected_address: String,
        expected_port: u16,
    ) -> AppResult<HostDto> {
        if expected_address.is_empty() || expected_port == 0 {
            return Err(AppError::invalid(
                "shared_host",
                "invalid expected endpoint",
            ));
        }
        self.sharing_refresh_bound_host_inner(host_id, Some((expected_address, expected_port)))
            .await
    }

    async fn sharing_refresh_bound_host_inner(
        &self,
        host_id: String,
        expected_endpoint: Option<(String, u16)>,
    ) -> AppResult<HostDto> {
        let (_, expected) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        let host = crate::inventory::host_of(&expected, id)?;
        let Some((instance, share_id)) = host_sharing_binding(&host)? else {
            return Ok(HostDto::from_model(&host));
        };
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Host,
            move |u, snapshot, projection| {
                Box::pin(async move {
                    if !std::ptr::eq(expected.as_ref(), u)
                        || snapshot.binding().server_instance_id.to_string() != instance
                    {
                        return Err(AppError::invalid(
                            "shared_host",
                            "profile or instance changed",
                        ));
                    }
                    let SharedProjection::Host(public) = projection else {
                        return Err(AppError::invalid("shared_host", "incorrect shared kind"));
                    };
                    let mut host = crate::inventory::host_of(u, id)?;
                    if host_sharing_binding(&host)?
                        != Some((instance.clone(), snapshot.binding().share_id.to_string()))
                    {
                        return Err(AppError::invalid("shared_host", "binding changed"));
                    }
                    let accepted_endpoint = expected_endpoint
                        .as_ref()
                        .map(|(address, port)| address == &public.address && *port == public.port)
                        .unwrap_or(
                            host.address == public.address && host.port == Some(public.port),
                        );
                    if !accepted_endpoint {
                        return Err(AppError::invalid("shared_host", "endpoint_changed_review"));
                    }
                    // Preserve local authentication, routing, trust and metadata.
                    host.name = public.name.clone();
                    host.address = public.address.clone();
                    host.port = Some(public.port);
                    host.username = public.username.clone();
                    host.keepalive_secs = public.keepalive_secs;
                    host.tags = public.tags.clone();
                    host.notes = public.notes.clone().unwrap_or_default();
                    host.updated_at = chrono::Utc::now();
                    host.validate()
                        .map_err(|_| AppError::invalid("shared_host", "invalid host"))?;
                    crate::inventory::validate_refs(u.working(), &VaultObject::Host(host.clone()))?;
                    let dto = HostDto::from_model(&host);
                    u.writer.put(VaultObject::Host(host)).await?;
                    Ok(dto)
                })
            },
        )
        .await
    }

    pub(crate) async fn sharing_prepare_connection(
        &self,
        host_id: &str,
    ) -> AppResult<PreparedSharingConnection> {
        let (_, expected) = self.unlocked().await?;
        let id = parse_id("host_id", host_id)?;
        // Validate the target even before planning. An invalid target route
        // must not hide malformed/partial sharing metadata.
        let target = expected
            .working()
            .host(id)
            .ok_or_else(|| AppError::from(cc_ssh_core::PlanError::MissingHost(id)))?;
        host_sharing_binding(&target)?;
        let mut verified = HashMap::new();
        // A local route can change while online history is being checked.
        // Replan until every actual target/jump endpoint has been refreshed;
        // never dial a new hop merely because the target itself is private.
        for _ in 0..=cc_ssh_core::planner::MAX_HOPS {
            let (_, current) = self.unlocked().await?;
            if !Arc::ptr_eq(&expected, &current) {
                return Err(AppError::invalid("shared_host", "profile changed"));
            }
            let plan = expected.ssh.plan(id).await?;
            let mut changed = false;
            for hop in plan.all_hops() {
                let host = crate::inventory::host_of(&expected, hop.host_id)?;
                let Some(binding) = host_sharing_binding(&host)? else {
                    continue;
                };
                let version = VerifiedHostBinding::new(&host, binding);
                if verified.get(&host.id) == Some(&version) {
                    if hop.endpoint.host != version.address.trim()
                        || version.port != Some(hop.endpoint.port)
                        || version
                            .username
                            .as_ref()
                            .is_some_and(|name| name != &hop.username)
                    {
                        return Err(AppError::invalid(
                            "shared_host",
                            "route changed during verification",
                        ));
                    }
                    continue;
                }
                self.sharing_refresh_bound_host(host.id.to_string(), false)
                    .await?;
                let (_, current) = self.unlocked().await?;
                if !Arc::ptr_eq(&expected, &current) {
                    return Err(AppError::invalid("shared_host", "profile changed"));
                }
                let refreshed = crate::inventory::host_of(&expected, hop.host_id)?;
                let binding = host_sharing_binding(&refreshed)?
                    .ok_or_else(|| AppError::invalid("shared_host", "binding changed"))?;
                verified.insert(host.id, VerifiedHostBinding::new(&refreshed, binding));
                changed = true;
            }
            let (_, current) = self.unlocked().await?;
            if !Arc::ptr_eq(&expected, &current) {
                return Err(AppError::invalid("shared_host", "profile changed"));
            }
            if !changed {
                return Ok(PreparedSharingConnection {
                    unlocked: expected,
                    plan,
                });
            }
        }
        Err(AppError::invalid(
            "shared_host",
            "route changed during verification",
        ))
    }

    /// Explicitly make a bound copy independent. This cannot erase a disclosed
    /// copy and does not revoke or alter the original shared object's ACL.
    pub async fn sharing_detach_host(&self, host_id: String) -> AppResult<HostDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        let mut host = crate::inventory::host_of(&u, id)?;
        host.metadata.remove(INSTANCE);
        host.metadata.remove(SHARE);
        host.updated_at = chrono::Utc::now();
        let dto = HostDto::from_model(&host);
        u.writer.put(VaultObject::Host(host)).await?;
        Ok(dto)
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use crate::AppConfig;
    use std::time::Duration;
    use tokio::net::TcpListener;

    async fn local_app(directory: &std::path::Path) -> AppCore {
        let app = AppCore::new(AppConfig::for_tests(
            directory.to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Runtime route test".into(),
            format!("runtime-{}-{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()),
        )
        .await
        .unwrap();
        app
    }

    #[tokio::test]
    async fn immutable_plan_never_uses_a_later_unverified_jump_route() {
        let directory = tempfile::tempdir().unwrap();
        let app = local_app(directory.path()).await;
        let approved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let changed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut target = HostDto::new("Explicit private endpoint", "127.0.0.1");
        target.port = Some(approved.local_addr().unwrap().port());
        target.username = Some("operator".into());
        let mut target = app.save_host(target).await.unwrap();
        let prepared = app.sharing_prepare_connection(&target.id).await.unwrap();
        assert!(prepared.plan.route.is_empty());
        let mut jump = HostDto::new("Later unverified jump", "127.0.0.1");
        jump.port = Some(changed.local_addr().unwrap().port());
        jump.username = Some("operator".into());
        jump.metadata
            .insert(INSTANCE.into(), uuid::Uuid::new_v4().to_string());
        jump.metadata
            .insert(SHARE.into(), uuid::Uuid::new_v4().to_string());
        let jump = app.save_host(jump).await.unwrap();
        target.jump_chain = vec![jump.id];
        app.save_host(target).await.unwrap();
        // A newly planned call now requires sharing, proving the inventory was
        // changed. The already approved call still uses its immutable route.
        assert!(matches!(
            app.sharing_prepare_connection(&prepared.plan.host_id.to_string())
                .await,
            Err(AppError::LocalProfile)
        ));
        let network = prepared.run(
            prepared
                .unlocked
                .ssh
                .exec_plan(&prepared.plan, "printf explicit"),
        );
        let accepted = async {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(3), approved.accept())
                .await
                .expect("the immutable plan must use its approved endpoint")
                .unwrap();
            drop(socket); // The probe deliberately offers no SSH protocol.
        };
        let (result, ()) = tokio::join!(network, accepted);
        assert!(result.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), changed.accept())
                .await
                .is_err(),
            "a route update must never silently select an unverified jump"
        );
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn captured_plan_is_cancelled_on_lock_before_dial() {
        let directory = tempfile::tempdir().unwrap();
        let app = local_app(directory.path()).await;
        let endpoint = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut target = HostDto::new("Explicit private endpoint", "127.0.0.1");
        target.port = Some(endpoint.local_addr().unwrap().port());
        target.username = Some("operator".into());
        let target = app.save_host(target).await.unwrap();
        let prepared = app.sharing_prepare_connection(&target.id).await.unwrap();
        app.lock().await.unwrap();
        assert!(matches!(
            prepared
                .run(
                    prepared
                        .unlocked
                        .ssh
                        .exec_plan(&prepared.plan, "printf explicit")
                )
                .await,
            Err(AppError::VaultLocked)
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), endpoint.accept())
                .await
                .is_err()
        );
        app.shutdown().await.unwrap();
    }
}
