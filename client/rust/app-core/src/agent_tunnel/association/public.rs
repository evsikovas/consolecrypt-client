//! Product entrypoints. JSON supplies intent only; issued handles use the SAME

//! captured weak Core/session/frame and never resolve a replacement Core.

use super::*;

use serde::Deserialize;

use serde_json::{Value, json};

use std::sync::Weak;

mod data;

mod operation;

mod revoke;

mod rotation;

#[cfg(test)]

mod tests;

mod tls_review;

mod route;
pub(crate) use route::MonitoringTunnelRoute;

mod collector;
mod collector_service;
pub use collector_service::{AgentCollectorServiceReview,AgentCollectorServiceRevocationReview};

mod outbound_control;

pub use outbound_control::NodeTunnelControlReview;

pub use collector::AgentCollectorOperation;

pub use route::AgentTunnelRouteOperation;

pub use operation::NodeAssociationOperation;

pub use revoke::NodeRevocationReview;

pub use rotation::NodeRotationReview;

pub use tls_review::NodeClientIdentityReview;



#[derive(Clone, Deserialize)]

#[serde(rename_all = "camelCase", deny_unknown_fields)]

struct Frame {

    version: u8,

    profile_id: String,

    notification_scope_id: String,

}

#[derive(Deserialize)]

#[serde(rename_all = "camelCase", deny_unknown_fields)]

struct ProbeRequest {

    version: u8,

    profile_id: String,

    notification_scope_id: String,

    host_id: String,

    expected_host_revision: String,

}

fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {

    if raw.is_empty() || raw.len() > 2048 {

        return Err(unavailable());

    }

    serde_json::from_str(raw).map_err(|_| unavailable())

}

fn revision(raw: &str) -> AppResult<i64> {

    let n = raw.parse::<i64>().map_err(|_| unavailable())?;

    if n <= 0 || n.to_string() != raw {

        return Err(unavailable());

    }

    Ok(n)

}

#[derive(Clone)]

struct Context {

    core: Weak<crate::app::Inner>,

    scope: Arc<OriginalScope>,

    frame: Frame,

    cancel: CancellationToken,

    runtime: tokio::runtime::Handle,

}

impl Context {

    async fn capture_control(app:&AppCore,frame:Frame)->AppResult<Self> {

        if frame.version!=1 {return Err(unavailable())}

        let cancel=CancellationToken::new();

        let scope=Arc::new(OriginalScope::capture_expected(app,&cancel,Duration::from_secs(300),

            &frame.profile_id,&frame.notification_scope_id).await?);

        Ok(Self{core:Arc::downgrade(&app.inner),scope,frame,cancel,

            runtime:tokio::runtime::Handle::try_current().map_err(|_|unavailable())?})

    }

    async fn capture(app: &AppCore, frame: Frame) -> AppResult<Self> {

        if frame.version != 1 {

            return Err(unavailable());

        }

        let cancel = CancellationToken::new();

        let scope = Arc::new(

            OriginalScope::capture_expected(

                app,

                &cancel,

                Duration::from_secs(60),

                &frame.profile_id,

                &frame.notification_scope_id,

            )

            .await?,

        );

        Ok(Self {

            core: Arc::downgrade(&app.inner),

            scope,

            frame,

            cancel,

            runtime: tokio::runtime::Handle::try_current().map_err(|_| unavailable())?,

        })

    }

    /// Collection includes original review/SSH time; no replacement capture.

    async fn capture_collection(app:&AppCore,frame:Frame,ttl:Duration)->AppResult<Self> {

        if frame.version!=1||ttl<Duration::from_secs(30)||ttl>Duration::from_secs(300)||ttl.subsec_nanos()!=0 {

            return Err(unavailable())

        }

        let cancel=CancellationToken::new();

        let scope=Arc::new(OriginalScope::capture_expected(app,&cancel,ttl,&frame.profile_id,&frame.notification_scope_id).await?);

        Ok(Self{core:Arc::downgrade(&app.inner),scope,frame,cancel,

            runtime:tokio::runtime::Handle::try_current().map_err(|_|unavailable())?})

    }

    fn app(&self) -> AppResult<AppCore> {

        self.scope.check_current()?;

        Ok(AppCore {

            inner: self.core.upgrade().ok_or_else(unavailable)?,

        })

    }

    async fn check(&self) -> AppResult<()> {

        let app = self.app()?;

        self.scope.check(&app).await

    }

    fn publish(&self, mut value: Value, limit: usize) -> AppResult<String> {

        self.scope.with_io_current(|_| {

            if self.cancel.is_cancelled() || Instant::now() >= self.scope.deadline() {

                return Err(unavailable());

            }

            let object = value.as_object_mut().ok_or_else(unavailable)?;

            object.insert("version".into(), json!(1));

            object.insert("profileId".into(), json!(self.frame.profile_id));

            object.insert(

                "notificationScopeId".into(),

                json!(self.frame.notification_scope_id),

            );

            if object.contains_key("remainingMs") {

                let ms = self

                    .scope

                    .deadline()

                    .saturating_duration_since(Instant::now())

                    .as_millis()

                    .min(60000);

                if ms == 0 {

                    return Err(unavailable());

                }

                object.insert("remainingMs".into(), json!(ms as u64));

            }

            let raw = serde_json::to_string(&value).map_err(|_| unavailable())?;

            if raw.len() > limit

                || self.cancel.is_cancelled()

                || Instant::now() >= self.scope.deadline()

            {

                return Err(unavailable());

            }

            Ok(raw)

        })

    }

    async fn owner(&self) -> AppResult<Value> {

        let app = self.app()?;

        self.scope.with_resources(&app,|_,_,id|{

            let(kind,owner)=match self.scope.owner_context()?{

                super::super::scope::OriginalOwnerContext::LocalProfile{profile_id}=>("localProfile",profile_id.to_string()),

                super::super::scope::OriginalOwnerContext::PersonalAccount{user_id}=>("personalAccount",user_id.to_string()),

            };

            Ok(json!({"kind":kind,"id":owner,"deviceId":id.device_id(),"signingKeySha256":hash(&id.public_keys().signing)}))

        }).await

    }

    async fn validate_host(&self, id: ObjectId, expected: i64) -> AppResult<()> {

        let app = self.app()?;

        self.scope

            .with_resources(&app, |_, u, _| {

                let w = u.working();

                let host = w.host(id).ok_or_else(unavailable)?;

                if expected <= 0

                    || w.revision_of(id) != Some(expected)

                    || w.vault_of(id).is_some()

                    || host.backend != cc_ssh_core::SshBackend::Native

                    || host.host_key_policy != cc_ssh_core::HostKeyPolicy::Strict

                    || crate::sharing_bindings::host_sharing_binding(&host)?.is_some()

                {

                    return Err(unavailable());

                }

                Ok(())

            })

            .await

    }

}

pub(super) async fn bounded_original<T>(

    scope: &OriginalScope,

    work: impl std::future::Future<Output = AppResult<T>>,

) -> AppResult<T> {

    scope.check_current()?;

    tokio::select! {biased;_=scope.cancelled()=>Err(unavailable()),result=work=>{scope.check_current()?;result}}

}

impl AppCore {

    pub async fn agent_node_association_status_json(

        &self,

        request_json: &str,

    ) -> AppResult<String> {

        let frame: Frame = parse(request_json)?;

        let context = Context::capture(self, frame).await?;

        let original = context.scope.clone();

        bounded_original(&original,async move{

        let app = context.app()?;

        let hosts=context.scope.with_resources(&app,|_,u,_|{

            let rows=u.working().native_association_host_choices().ok_or_else(unavailable)?;

            Ok(rows.into_iter().map(|(id,revision,name)|json!({"hostId":id,"hostRevision":revision.to_string(),"name":name})).collect::<Vec<_>>())

        }).await?;

        let custody = ManagedTlsCustody::capture(&app, context.scope.clone()).await?;

        let identity = match custody.stored_metadata(&app).await {

            Ok((None, None)) => {

                json!({"state":"missing","identityId":null,"certificateSha256":null})

            }

            Ok((Some(phase), Some(public))) => {

                json!({"state":if phase==cc_storage_core::TunnelTlsPhase::Stored{"storedMetadata"}else{"pending"},"identityId":public.identity_id,"certificateSha256":public.certificate_sha256})

            }

            _ => json!({"state":"unavailable","identityId":null,"certificateSha256":null}),

        };

        let ledger = ledger::Ledger::capture(&app, context.scope.clone()).await?;

        let snapshot = ledger.snapshot(&app).await?;

        let key = context

            .scope

            .with_loaded(&app, |id| {

                Ok(PublicKey::from_bytes(id.public_keys().signing))

            })

            .await?;

        let associations = snapshot

            .records()

            .iter()

            .map(|r| data::association(r, key))

            .collect::<AppResult<Vec<_>>>()?;

        context.check().await?;

        context.publish(json!({"version":1,"profileId":"","notificationScopeId":"","hosts":hosts,"clientIdentity":identity,"associations":associations}),65536)

        }).await

    }

    pub async fn agent_node_association_register_json(

        &self,

        request_json: &str,

    ) -> AppResult<NodeAssociationOperation> {

        let request: ProbeRequest = parse(request_json)?;

        let host = crate::dto::parse_id("host", &request.host_id)?;

        let rev = revision(&request.expected_host_revision)?;

        if host.to_string() != request.host_id {

            return Err(unavailable());

        }

        let permit = review_permit()?;

        let context = Context::capture(

            self,

            Frame {

                version: request.version,

                profile_id: request.profile_id,

                notification_scope_id: request.notification_scope_id,

            },

        )

        .await?;

        let original = context.scope.clone();

        bounded_original(&original, async move {

            context.validate_host(host, rev).await?;

            let owner = context.owner().await?;

            NodeAssociationOperation::new(context, host, rev, permit, owner)

        })

        .await

    }

    pub async fn agent_node_client_identity_prepare_json(

        &self,

        request_json: &str,

    ) -> AppResult<NodeClientIdentityReview> {

        let frame: Frame = parse(request_json)?;

        let permit = review_permit()?;

        let context = Context::capture(self, frame).await?;

        let original = context.scope.clone();

        bounded_original(&original, async move {

            let app = context.app()?;

            let custody = ManagedTlsCustody::capture(&app, context.scope.clone()).await?;

            if custody.stored_metadata(&app).await?.0.is_some() {

                return Err(unknown());

            }

            let owner = context.owner().await?;

            NodeClientIdentityReview::new(context, custody, permit, owner)

        })

        .await

    }

}



impl AppCore {

    /// Explicit MetricsOnly enrollment intent. No scope/policy/cert supplied by UI.

    pub async fn agent_metrics_node_association_register_json(self,raw:&str)->AppResult<NodeAssociationOperation> {

        let request:ProbeRequest=parse(raw)?;

        let host=crate::dto::parse_id("host",&request.host_id)?;let rev=revision(&request.expected_host_revision)?;

        if host.to_string()!=request.host_id{return Err(unavailable())}

        let permit=review_permit()?;

        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id,

            notification_scope_id:request.notification_scope_id}).await?;

        drop(self);

        let scope=context.scope.clone();

        bounded_original(&scope,async move{

            context.validate_host(host,rev).await?;let owner=context.owner().await?;

            NodeAssociationOperation::new_metrics(context,host,rev,permit,owner)

        }).await

    }

}


impl AppCore {

    /// Explicit ExporterOnly enrollment intent. No scope/policy/cert supplied by UI.

    pub async fn agent_exporter_node_association_register_json(self,raw:&str)->AppResult<NodeAssociationOperation> {

        let request:ProbeRequest=parse(raw)?;

        let host=crate::dto::parse_id("host",&request.host_id)?;let rev=revision(&request.expected_host_revision)?;

        if host.to_string()!=request.host_id{return Err(unavailable())}

        let permit=review_permit()?;

        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id,

            notification_scope_id:request.notification_scope_id}).await?;

        drop(self);

        let scope=context.scope.clone();

        bounded_original(&scope,async move{

            context.validate_host(host,rev).await?;let owner=context.owner().await?;

            NodeAssociationOperation::new_exporters(context,host,rev,permit,owner)

        }).await

    }

}


mod exporter;
pub use exporter::AgentExporterOperation;
