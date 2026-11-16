//! Reads only saved monitoring sources of the current unlock. No caller URL or
//! credential material is accepted by this facade. Metrics never enter storage.
use crate::{AppCore, AppError, AppResult};
use cc_monitoring_core::{AuthRef, Budgets, MonitoringClient, SourceAuthorization, SourceConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;
mod alert_reads;
mod binding_reads;
mod exporter_reads;
#[cfg(all(feature = "test-harness", not(test)))]
pub(crate) use exporter_reads::live_cadvisor_harness::owned_cadvisor_install_signed_job_real_source_binding_and_docker_panel as __focused_live_cadvisor;
mod saved_panel_reads;
mod ssh_route;
mod agent_route;

fn invalid(reason: &str) -> AppError {
    AppError::invalid("monitoring", reason)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadRequest {
    operation_id: Uuid,
    source_id: String,
    source_revision: u64,
    #[serde(default)]
    selected_agent_route_id: Option<String>,
    action: ReadAction,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ReadAction {
    ExporterContext(exporter_reads::ContextRequest),
    ExporterVerification(exporter_reads::VerificationRequest),
    Alerts {},
    AlertRules {},
    SavedPanel {
        #[serde(rename = "panelId")]
        panel_id: String,
        #[serde(rename = "panelRevision")]
        panel_revision: u64,
        range: Option<saved_panel_reads::SavedPanelRange>,
    },
    Probe,
    Query {
        expression: String,
        time: Option<f64>,
    },
    QueryRange {
        expression: String,
        start: f64,
        end: f64,
        #[serde(rename = "stepSeconds")]
        step_seconds: f64,
    },
    LabelValues {
        label: String,
    },
    ClearCache,
    BindingDiscover {
        #[serde(rename = "bindingId")]
        binding_id: String,
        #[serde(rename = "bindingRevision")]
        binding_revision: u64,
        time: f64,
    },
    BindingPanel {
        #[serde(rename = "bindingId")]
        binding_id: String,
        #[serde(rename = "bindingRevision")]
        binding_revision: u64,
        panel: cc_monitoring_core::panels::PanelKind,
        start: f64,
        end: f64,
        #[serde(rename = "stepSeconds")]
        step_seconds: f64,
    },
}

struct Operation {
    cancel: CancellationToken,
    started: bool,
    expires: Instant,
}
struct CachedClient {
    configuration: SourceConfig,
    client: Arc<MonitoringClient>,
}

/// Lifetime is exactly one unlocked vault, never a global bridge singleton.
pub(crate) struct MonitoringRuntime {
    operations: Mutex<HashMap<Uuid, Operation>>,
    clients: Mutex<HashMap<String, CachedClient>>,
    permits: Arc<tokio::sync::Semaphore>,
    source_permits: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}
impl Default for MonitoringRuntime {
    fn default() -> Self {
        Self {
            operations: Mutex::new(HashMap::new()),
            clients: Mutex::new(HashMap::new()),
            permits: Arc::new(tokio::sync::Semaphore::new(8)),
            source_permits: Mutex::new(HashMap::new()),
        }
    }
}

struct Registration<'a> {
    runtime: &'a MonitoringRuntime,
    id: Uuid,
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.runtime
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id)
            .map(|op| op.cancel.cancel());
    }
}

/// Opaque review lease in the SAME unlock-owned map and global budget.
/// Weak ownership lets lock dispose the vault while an FFI handle still exists.
pub(crate) struct OwnedMonitoringRegistration {
    unlock: std::sync::Weak<crate::session::Unlocked>,
    id: Uuid,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for OwnedMonitoringRegistration {
    fn drop(&mut self) {
        if let Some(u) = self.unlock.upgrade() {
            u.monitoring
                .operations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.id)
                .map(|op| op.cancel.cancel());
        }
    }
}
impl MonitoringRuntime {
    pub(crate) fn begin_owned(
        unlock: std::sync::Weak<crate::session::Unlocked>,
    ) -> AppResult<(OwnedMonitoringRegistration, CancellationToken)> {
        let u = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let permit = u
            .monitoring
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("monitoring concurrency budget reached"))?;
        let id = Uuid::new_v4();
        let cancel = u.monitoring.register(id)?;
        Ok((
            OwnedMonitoringRegistration {
                unlock,
                id,
                _permit: permit,
            },
            cancel,
        ))
    }
    fn register(&self, id: Uuid) -> AppResult<CancellationToken> {
        if id.is_nil() {
            return Err(invalid("invalid operation identifier"));
        }
        let mut operations = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| op.started || op.expires > Instant::now());
        if let Some(existing) = operations.get(&id) {
            return Err(invalid(if existing.started {
                "operation already active"
            } else {
                "operation cancelled"
            }));
        }
        if operations.len() >= 32 {
            return Err(invalid("monitoring concurrency budget reached"));
        }
        let cancel = CancellationToken::new();
        operations.insert(
            id,
            Operation {
                cancel: cancel.clone(),
                started: true,
                expires: Instant::now() + Duration::from_secs(45),
            },
        );
        Ok(cancel)
    }
    fn begin(&self, id: Uuid) -> AppResult<(Registration<'_>, CancellationToken)> {
        let cancel = self.register(id)?;
        Ok((Registration { runtime: self, id }, cancel))
    }
    fn cancel(&self, id: Uuid) -> AppResult<()> {
        if id.is_nil() {
            return Err(invalid("invalid operation identifier"));
        }
        let mut operations = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| op.started || op.expires > Instant::now());
        if let Some(op) = operations.get(&id) {
            op.cancel.cancel();
            return Ok(());
        }
        if operations.len() >= 32 {
            return Err(invalid("monitoring concurrency budget reached"));
        }
        let cancel = CancellationToken::new();
        cancel.cancel();
        operations.insert(
            id,
            Operation {
                cancel,
                started: false,
                expires: Instant::now() + Duration::from_secs(45),
            },
        );
        Ok(())
    }
    pub(crate) fn close(&self) {
        for op in self
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            op.cancel.cancel();
        }
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        for entry in clients.values() {
            entry.client.clear_cache();
        }
        clients.clear();
    }
    fn permits(
        &self,
        source: &str,
    ) -> AppResult<(
        tokio::sync::OwnedSemaphorePermit,
        tokio::sync::OwnedSemaphorePermit,
    )> {
        let global = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("monitoring concurrency budget reached"))?;
        let mut sources = self
            .source_permits
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !sources.contains_key(source) && sources.len() >= 32 {
            sources.retain(|_, permits| permits.available_permits() < 4);
            if sources.len() >= 32 {
                return Err(invalid("monitoring source budget reached"));
            }
        }
        let source = sources
            .entry(source.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(4)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("monitoring source concurrency budget reached"))?;
        Ok((global, source))
    }
    fn anonymous(&self, source: &SourceConfig) -> AppResult<Arc<MonitoringClient>> {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = clients.get(&source.id) {
            if &cached.configuration == source {
                return Ok(cached.client.clone());
            }
        }
        let client = Arc::new(
            MonitoringClient::new(source.clone(), Budgets::default())
                .map_err(|_| invalid("invalid monitoring transport configuration"))?,
        );
        // Catalog limits plus this independent hard bound prevent an unbounded cache.
        if clients.len() >= 32 {
            clients.clear();
        }
        clients.insert(
            source.id.clone(),
            CachedClient {
                configuration: source.clone(),
                client: client.clone(),
            },
        );
        Ok(client)
    }
}

impl AppCore {
    pub async fn monitoring_cancel(&self, operation_id: String) -> AppResult<()> {
        let (owner, unlock) = self.unlocked().await?;
        let id =
            Uuid::parse_str(&operation_id).map_err(|_| invalid("invalid operation identifier"))?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlock).await?;
        unlock.monitoring.cancel(id)
    }

    pub async fn monitoring_read(&self, request_json: String) -> AppResult<Value> {
        if request_json.len() > 24 * 1024 {
            return Err(invalid("monitoring request exceeds budget"));
        }
        let request: ReadRequest = serde_json::from_str(&request_json)
            .map_err(|_| invalid("invalid monitoring read request"))?;
        if let Some(route_id)=&request.selected_agent_route_id {
            cc_agent_node_wire::ResourceId::new(route_id)
                .map_err(|_|invalid("invalid original Agent route identifier"))?;
        }
        if request.selected_agent_route_id.is_some() && !matches!(&request.action,
            ReadAction::Query{..}|ReadAction::QueryRange{..}|ReadAction::LabelValues{..}
            |ReadAction::Alerts{}|ReadAction::AlertRules{}) {
            return Err(invalid("approved Agent route supports one explicit GET; reconnect and approve for another read"));
        }
        let (owner, unlock) = self.unlocked().await?;
        let (_registration, cancel) = unlock.monitoring.begin(request.operation_id)?;
        let mut shutdown = unlock.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let mut ssh_cleanup: Option<Arc<agent_route::SourceRoute>> = None;
        let work = async {
            // Explicit metadata-only initial capture must precede generic
            // route/auth/client construction. It cannot trigger an HTTP read.
            if let Some(result) = exporter_reads::context(
                self,
                &owner,
                &unlock,
                &request.action,
                &request.source_id,
                request.source_revision,
            )
            .await?
            {
                return Ok(
                    json!({"sourceId":request.source_id,"sourceRevision":request.source_revision,"result":result}),
                );
            }
            let exporter_read = exporter_reads::ExporterRead::prepare(
                self,
                &owner,
                &unlock,
                &request.action,
                &request.source_id,
                request.source_revision,
            )
            .await?;
            let (source, snapshot, source_fence, binding_read, saved_panel_read, mutation) = {
                let _profile = self.hold_session(&owner).await?;
                let _unlock = owner.hold_unlocked(&unlock).await?;
                let version = unlock.working().version();
                let source = crate::monitoring_sources::source_for_request(
                    &unlock,
                    &request.source_id,
                    request.source_revision,
                )?;
                let binding_read = binding_reads::BindingRead::prepare(
                    &unlock,
                    &request.action,
                    &request.source_id,
                    request.source_revision,
                )?;
                let saved_panel_read = saved_panel_reads::SavedPanelRead::prepare(
                    &unlock,
                    &request.action,
                    &request.source_id,
                    request.source_revision,
                )?;
                let snapshot = unlock.working().snapshot();
                if unlock.working().version() != version {
                    return Err(invalid("monitoring inputs changed; retry the read"));
                }
                let (settings_id, settings_entry) = snapshot
                    .iter()
                    .filter_map(|(id, entry)| {
                        let crate::working_set::Item::Object(object) = &entry.item else {
                            return None;
                        };
                        let cc_models::VaultObject::VaultSettings(settings) = object.as_ref()
                        else {
                            return None;
                        };
                        Some((id, entry, settings.updated_at))
                    })
                    .max_by_key(|(_, _, updated)| *updated)
                    .map(|(id, entry, _)| (*id, entry))
                    .ok_or_else(|| invalid("monitoring settings unavailable"))?;
                let mut source_fence = BTreeMap::from([(settings_id, settings_entry.revision)]);
                if let Some(binding_read) = &binding_read {
                    let host_id = crate::dto::parse_id("monitoring_host", &binding_read.host_id())?;
                    let host = snapshot
                        .get(&host_id)
                        .ok_or_else(|| invalid("monitoring host unavailable"))?;
                    source_fence.insert(host_id, host.revision);
                }
                if let Some(exporter) = &exporter_read {
                    let host_id = crate::dto::parse_id("monitoring_host", exporter.host_id())?;
                    let host = snapshot
                        .get(&host_id)
                        .ok_or_else(|| invalid("exporter host unavailable"))?;
                    if host.revision != exporter.host_revision() as i64 {
                        return Err(invalid("exporter host changed"));
                    }
                    source_fence.insert(host_id, host.revision);
                }
                (
                    source,
                    snapshot,
                    source_fence,
                    binding_read,
                    saved_panel_read,
                    version,
                )
            };
            // Transport selection is transient: saving a live route ID would
            // itself mutate the original scope. Preserve the entire saved
            // source for both revision checks; never rewrite its URL/auth/TLS.
            let saved_source=source;
            if matches!(&saved_source.route,cc_monitoring_core::Route::AgentTunnel{..})
                && request.selected_agent_route_id.is_none() {
                return Err(invalid("saved live Agent route identifiers require an explicit current route selection"));
            }
            if matches!(&saved_source.route,cc_monitoring_core::Route::AgentDestination{..})
                && !matches!(&request.action,ReadAction::Query{..}|ReadAction::QueryRange{..}
                    |ReadAction::LabelValues{..}|ReadAction::Alerts{}|ReadAction::AlertRules{}|ReadAction::Probe) {
                return Err(invalid("approved Agent route supports one explicit GET or the fixed two-GET probe; reconnect and approve for another read"));
            }
            let mut source=saved_source.clone();
            if let Some(route_id)=&request.selected_agent_route_id {
                source.route=cc_monitoring_core::Route::AgentTunnel{route_id:route_id.clone()};
            }
            // A saved Agent route borrows the already approved original; legacy
            // SSH tunnel identifiers remain unsupported.
            let _permits = unlock.monitoring.permits(&source.id)?;
            let ssh = agent_route::SourceRoute::open(
                self, &owner, &unlock, &source, &saved_source, &snapshot, mutation, &cancel,
            )
            .await?;
            ssh_cleanup = ssh.clone();
            let mut fences = source_fence;
            let credential = match &source.auth {
                AuthRef::None => None,
                AuthRef::BearerRef { credential_id } | AuthRef::BasicRef { credential_id } => {
                    let id = crate::dto::parse_id("credential_id", credential_id)?;
                    let entry = snapshot
                        .get(&id)
                        .ok_or_else(|| invalid("monitoring credential unavailable"))?;
                    let crate::working_set::Item::Object(object) = &entry.item else {
                        return Err(invalid("monitoring credential unavailable"));
                    };
                    let cc_models::VaultObject::Credential(credential) = object.as_ref() else {
                        return Err(invalid("monitoring credential unavailable"));
                    };
                    if credential.kind != cc_models::credential::CredentialKind::Password {
                        return Err(invalid("monitoring requires a password credential"));
                    }
                    let secret_id = credential
                        .secret_id
                        .ok_or_else(|| invalid("monitoring credential has no secret"))?;
                    let secret_entry = snapshot
                        .get(&secret_id)
                        .ok_or_else(|| invalid("monitoring secret unavailable"))?;
                    if !matches!(
                        secret_entry.item,
                        crate::working_set::Item::Secret {
                            kind: cc_models::secret::SecretKind::Password
                        }
                    ) {
                        return Err(invalid("monitoring requires a password secret"));
                    }
                    fences.insert(id, entry.revision);
                    fences.insert(secret_id, secret_entry.revision);
                    Some((credential.clone(), secret_id))
                }
            };
            if !unlock
                .working()
                .watch_revisions(fences.clone(), cancel.clone())
            {
                return Err(invalid("monitoring inputs changed"));
            }
            let client = match credential {
                None => match &ssh {
                    Some(route) => Arc::new(
                        MonitoringClient::with_scoped_connector(
                            source.clone(),
                            Budgets::default(),
                            None,
                            route.clone(),
                        )
                        .map_err(|_| invalid("invalid scoped monitoring transport"))?,
                    ),
                    None => unlock.monitoring.anonymous(&source)?,
                },
                Some((credential, secret_id)) => {
                    let secret = unlock.writer.read_secret(secret_id).await?;
                    if secret.kind != cc_models::secret::SecretKind::Password {
                        return Err(invalid("monitoring requires a password secret"));
                    }
                    let material = Zeroizing::new(secret.value.expose_secret().to_owned());
                    let grant = match &source.auth {
                        AuthRef::BearerRef { .. } => {
                            SourceAuthorization::bearer(&source, material, Duration::from_secs(30))
                        }
                        AuthRef::BasicRef { .. } => {
                            SourceAuthorization::basic(
                                &source,
                                Zeroizing::new(credential.username.ok_or_else(|| {
                                    invalid("basic authentication needs a username")
                                })?),
                                material,
                                Duration::from_secs(30),
                            )
                        }
                        AuthRef::None => unreachable!(),
                    }
                    .map_err(|_| invalid("invalid scoped monitoring credential"))?;
                    Arc::new(
                        if let Some(route) = &ssh {
                            MonitoringClient::with_scoped_connector(
                                source.clone(),
                                Budgets::default(),
                                Some(grant),
                                route.clone(),
                            )
                        } else {
                            MonitoringClient::with_authorization(
                                source.clone(),
                                Budgets::default(),
                                grant,
                            )
                        }
                        .map_err(|_| invalid("invalid monitoring transport configuration"))?,
                    )
                }
            };
            // A changed/deleted credential cannot be sent after asynchronous lookup.
            {
                let _profile = self.hold_session(&owner).await?;
                let _unlock = owner.hold_unlocked(&unlock).await?;
                if let Some(exporter) = &exporter_read {
                    exporter.check(&owner, &unlock).await?;
                }
                let current = crate::monitoring_sources::source_for_request(
                    &unlock,
                    &request.source_id,
                    request.source_revision,
                )?;
                if current != saved_source
                    || fences
                        .iter()
                        .any(|(id, revision)| unlock.working().revision_of(*id) != Some(*revision))
                {
                    return Err(invalid("monitoring source or credential changed"));
                }
                if let Some(route) = &ssh {
                    route.check()?;
                }
            }
            let result = match request.action {
                ReadAction::ExporterContext(_) => unreachable!("metadata context already returned"),
                ReadAction::ExporterVerification(_) => Ok(exporter_read
                    .as_ref()
                    .ok_or_else(|| invalid("missing exporter read plan"))?
                    .execute(&client)
                    .await),
                ReadAction::Alerts {} => alert_reads::result(client.alerts().await),
                ReadAction::AlertRules {} => alert_reads::result(client.alert_rules().await),
                ReadAction::SavedPanel { .. } => Ok(saved_panel_read
                    .ok_or_else(|| invalid("missing saved panel read plan"))?
                    .execute(&client)
                    .await),
                ReadAction::Probe => serde_json::to_value(client.probe().await),
                ReadAction::Query { expression, time } => serde_json::to_value(
                    client
                        .query(&expression, time)
                        .await
                        .map_err(|e| json!({"error":e}))
                        .map_or_else(|e| e, |v| json!(v)),
                ),
                ReadAction::QueryRange {
                    expression,
                    start,
                    end,
                    step_seconds,
                } => serde_json::to_value(
                    client
                        .query_range(&expression, start, end, step_seconds)
                        .await
                        .map_err(|e| json!({"error":e}))
                        .map_or_else(|e| e, |v| json!(v)),
                ),
                ReadAction::LabelValues { label } => serde_json::to_value(
                    client
                        .label_values(&label)
                        .await
                        .map_err(|e| json!({"error":e}))
                        .map_or_else(|e| e, |v| json!(v)),
                ),
                ReadAction::ClearCache => {
                    client.clear_cache();
                    Ok(json!({"cleared":true}))
                }
                ReadAction::BindingDiscover { .. } | ReadAction::BindingPanel { .. } => {
                    let binding_read =
                        binding_read.ok_or_else(|| invalid("missing binding read plan"))?;
                    Ok(binding_read.execute(self, &owner, &unlock, &client).await?)
                }
            }
            .map_err(|_| AppError::internal("monitoring response encoding"))?;
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&unlock).await?;
            if let Some(exporter) = &exporter_read {
                exporter.check(&owner, &unlock).await?;
            }
            let current = crate::monitoring_sources::source_for_request(
                &unlock,
                &request.source_id,
                request.source_revision,
            )?;
            if current != saved_source
                || fences
                    .iter()
                    .any(|(id, revision)| unlock.working().revision_of(*id) != Some(*revision))
            {
                return Err(invalid("monitoring source or credential changed"));
            }
            if let Some(route) = &ssh {
                route.check()?;
            }
            Ok(
                json!({"sourceId":request.source_id,"sourceRevision":request.source_revision,"result":result}),
            )
        };
        let result = tokio::select! {
            biased;
            _ = shutdown.changed() => Err(AppError::VaultLocked),
            _ = cancel.cancelled() => Err(invalid("monitoring operation cancelled")),
            value = tokio::time::timeout(Duration::from_secs(35), work) =>
                value.unwrap_or_else(|_| Err(invalid("monitoring operation timed out"))),
        };
        if let Some(route) = ssh_cleanup {
            route.close().await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn transient_agent_selection_rejects_unknown_ids_and_multi_get_before_profile_lookup() {
        let root=tempfile::tempdir().unwrap();
        let app=AppCore::new(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned())).unwrap();
        for (id,action) in [
            ("not-an-original-route",json!({"kind":"query","expression":"up","time":null})),
            ("00000000-0000-0000-0000-000000000000",json!({"kind":"query","expression":"up","time":null})),
            ("11111111-1111-4111-8111-111111111111",json!({"kind":"probe"})),
        ] {
            let result=app.monitoring_read(json!({"operationId":Uuid::new_v4(),"sourceId":Uuid::new_v4(),
                "sourceRevision":1,"selectedAgentRouteId":id,"action":action}).to_string()).await;
            assert!(matches!(result,Err(AppError::InvalidInput{field,..}) if field=="monitoring"));
        }
    }
    #[test]
    fn alerts_actions_are_parameter_free_and_reject_unknown_authority_or_cursor_fields() {
        for kind in ["alerts", "alertRules"] {
            let base = json!({"operationId":Uuid::new_v4(),"sourceId":Uuid::new_v4(),"sourceRevision":1,"action":{"kind":kind}});
            assert!(serde_json::from_value::<ReadRequest>(base.clone()).is_ok());
            for field in [
                "endpoint",
                "url",
                "expression",
                "filter",
                "cursor",
                "groupNextToken",
                "bindingId",
                "authorization",
            ] {
                let mut input = base.clone();
                input["action"][field] = json!("untrusted");
                assert!(serde_json::from_value::<ReadRequest>(input).is_err());
            }
            let raw = base.to_string();
            assert!(
                serde_json::from_str::<ReadRequest>(&raw.replacen(
                    "\"sourceRevision\":1",
                    "\"sourceRevision\":1,\"sourceRevision\":1",
                    1
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn cancellation_before_admission_and_duplicate_operations_fail_closed() {
        let runtime = MonitoringRuntime::default();
        let cancelled = Uuid::new_v4();
        runtime.cancel(cancelled).unwrap();
        assert!(runtime.begin(cancelled).is_err());
        let id = Uuid::new_v4();
        let (registration, cancel) = runtime.begin(id).unwrap();
        assert!(runtime.begin(id).is_err());
        runtime.cancel(id).unwrap();
        assert!(cancel.is_cancelled());
        drop(registration);
        assert!(runtime.operations.lock().unwrap().get(&id).is_none());
    }
    #[test]
    fn close_cancels_all_and_nil_and_budget_are_rejected() {
        let runtime = MonitoringRuntime::default();
        assert!(runtime.begin(Uuid::nil()).is_err());
        assert!(runtime.cancel(Uuid::nil()).is_err());
        let mut registrations = Vec::new();
        for _ in 0..32 {
            registrations.push(runtime.begin(Uuid::new_v4()).unwrap());
        }
        assert!(runtime.begin(Uuid::new_v4()).is_err());
        assert!(runtime.cancel(Uuid::new_v4()).is_err());
        runtime.close();
        assert!(
            registrations
                .iter()
                .all(|(_, cancel)| cancel.is_cancelled())
        );
    }
    #[test]
    fn public_request_never_accepts_an_endpoint_or_credential_material() {
        let base = json!({"operationId":Uuid::new_v4(),"sourceId":Uuid::new_v4(),
            "sourceRevision":1,"action":{"kind":"probe"}});
        assert!(serde_json::from_value::<ReadRequest>(base.clone()).is_ok());
        for key in ["endpoint", "authorization", "password", "source"] {
            let mut value = base.clone();
            value[key] = json!("untrusted");
            assert!(serde_json::from_value::<ReadRequest>(value).is_err());
        }
    }
}
