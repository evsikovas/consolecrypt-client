//! Private original-scope AWX facade. This staged file is not a public Bridge.
use super::{
    catalog::{self, AwxScope},
    *,
};
use crate::{
    AppCore, AppError, AppResult,
    session::{Session, Unlocked},
    working_set::VaultCredentialMetadata,
};
use cc_monitoring_core::{AuthRef, SourceConfig};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Mutex, Weak},
};
use tokio::sync::{Semaphore, watch};
use uuid::Uuid;

mod operations;

const MAX_REVIEWS: usize = 4;

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("automation_awx", reason)
}
fn fixed_error(error: AwxError) -> AppError {
    invalid(match error {
        AwxError::Configuration => "awx_configuration",
        AwxError::InvalidInput => "awx_invalid_input",
        AwxError::Cancelled => "awx_canceled",
        AwxError::Expired => "awx_expired",
        AwxError::Transport => "awx_transport",
        AwxError::Response => "awx_response",
        AwxError::Budget => "awx_budget",
        AwxError::Http(401) => "awx_unauthorized",
        AwxError::Http(403) => "awx_forbidden",
        AwxError::Http(_) => "awx_http",
        AwxError::ChangedTemplate => "awx_changed",
        AwxError::NeedsAwxInput => "awx_needs_awx_input",
        AwxError::UnsupportedPrompts => "awx_unsupported_prompts",
    })
}

struct MetadataLifetime(CancellationToken);
impl Drop for MetadataLifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Only Weak owners and bounded non-secret metadata survive an await/review.
/// Old receivers are never updated/rebased to a replacement catalog or unlock.
struct Captured {
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    scope: AwxScope,
    configuration: SourceConfig,
    revision: u64,
    credential: VaultCredentialMetadata,
    catalog_changes: watch::Receiver<u64>,
    shutdown: watch::Receiver<bool>,
    metadata: MetadataLifetime,
}
impl Captured {
    fn unchanged(&self) -> AppResult<()> {
        if self.metadata.0.is_cancelled()
            || self.catalog_changes.has_changed().unwrap_or(true)
            || self.shutdown.has_changed().unwrap_or(true)
            || *self.shutdown.borrow()
        {
            return Err(invalid("awx_context_changed"));
        }
        Ok(())
    }
    async fn check(&self, app: &AppCore) -> AppResult<()> {
        self.unchanged()?;
        let owner = self
            .owner
            .upgrade()
            .ok_or_else(|| invalid("awx_context_changed"))?;
        let u = self
            .unlock
            .upgrade()
            .ok_or_else(|| invalid("awx_context_changed"))?;
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        self.scope.check(&owner, &u)?;
        let current = catalog::selected_connection(
            owner.storage()?.setting_get(catalog::KEY).await?,
            &self.configuration.id,
        )?;
        if current.revision != self.revision
            || current.configuration != self.configuration
            || catalog::credential_metadata(&u, &self.configuration)? != self.credential
        {
            return Err(invalid("awx_connection_changed"));
        }
        self.unchanged()?;
        Ok(())
    }
    async fn authorization(&self, app: &AppCore) -> AppResult<AwxAuthorization> {
        self.check(app).await?;
        let material = {
            let u = self
                .unlock
                .upgrade()
                .ok_or_else(|| invalid("awx_context_changed"))?;
            let secret = u.writer.read_secret(self.credential.secret()).await?;
            if secret.kind != cc_models::secret::SecretKind::Password {
                return Err(invalid("awx_credential_unavailable"));
            }
            Zeroizing::new(secret.value.expose_secret().to_owned())
        };
        self.check(app).await?;
        match &self.configuration.auth {
            AuthRef::BearerRef { .. } => Ok(AwxAuthorization::Bearer(material)),
            AuthRef::BasicRef { .. } => Ok(AwxAuthorization::Basic {
                username: Zeroizing::new(
                    self.credential
                        .username()
                        .ok_or_else(|| invalid("awx_credential_unavailable"))?
                        .to_owned(),
                ),
                password: material,
            }),
            AuthRef::None => Err(invalid("awx_credential_unavailable")),
        }
    }
}

struct Review {
    captured: Arc<Captured>,
    review: AwxLaunchReview,
}

pub(crate) struct AwxRuntime {
    operations: Arc<operations::Operations>,
    reviews: Mutex<HashMap<Uuid, Review>>,
    permits: Arc<Semaphore>,
}
impl Default for AwxRuntime {
    fn default() -> Self {
        Self {
            operations: Arc::new(operations::Operations::default()),
            reviews: Mutex::new(HashMap::new()),
            permits: Arc::new(Semaphore::new(4)),
        }
    }
}
impl AwxRuntime {
    pub(crate) fn shutdown(&self) {
        self.operations.cancel_all();
        self.reviews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

impl Drop for AwxRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Typed requests contain numeric fixed-route arguments only. Launch parameters
/// are absent from Launch: only the retained opaque review can supply them.
pub(crate) enum ReadAction {
    Templates {
        page: u64,
        search: String,
    },
    Prepare {
        template: u64,
        parameters: AwxLaunchParameters,
    },
    Job {
        job: u64,
    },
    Events {
        job: u64,
        after: u64,
    },
}
enum Action {
    Read(ReadAction),
    Launch(Review),
}

/// Opaque Rust handle: the future gateway must retain this exact registration
/// for cancel/join after UI/profile retirement. No lookup of a latest profile.
pub struct AwxNativeOperation {
    registered: operations::Registered,
    runtime: Arc<AwxRuntime>,
    captured: Arc<Captured>,
    action: Arc<Mutex<Option<Action>>>,
}
impl AwxNativeOperation {
    pub(crate) fn id(&self) -> Uuid {
        self.registered.id()
    }
    pub fn cancel(&self) {
        self.registered.cancel();
    }
    pub async fn joined(&self) {
        self.registered.joined().await;
    }
}

fn register_action(
    runtime: &Arc<AwxRuntime>,
    action: Action,
) -> AppResult<(operations::Registered, Arc<Mutex<Option<Action>>>)> {
    let action = Arc::new(Mutex::new(Some(action)));
    let weak = Arc::downgrade(&action);
    let registered = runtime
        .operations
        .register_with_cleanup(move || {
            if let Some(action) = weak.upgrade() {
                action.lock().unwrap_or_else(|p| p.into_inner()).take();
            }
        })
        .map_err(invalid)?;
    Ok((registered, action))
}

fn envelope(kind: &str, data: Value) -> Value {
    json!({"version":1,"kind":kind,"data":data})
}
fn unknown() -> Value {
    envelope("launch", json!({"status":"unknown","reviewConsumed":true}))
}

impl AppCore {
    async fn capture_awx(
        &self,
        scope: &AwxScope,
        id: &str,
        revision: u64,
    ) -> AppResult<(Arc<AwxRuntime>, Arc<Captured>)> {
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        scope.check(&owner, &u)?;
        let changes = u.awx_catalog.changes();
        let shutdown = u.sharing_shutdown.subscribe();
        let row =
            catalog::selected_connection(owner.storage()?.setting_get(catalog::KEY).await?, id)?;
        if row.revision != revision {
            return Err(invalid("awx_connection_changed"));
        }
        let credential = catalog::credential_metadata(&u, &row.configuration)?;
        let token = CancellationToken::new();
        if !u
            .working()
            .watch_revisions(credential.fences(), token.clone())
        {
            return Err(invalid("awx_credential_changed"));
        }
        let captured = Arc::new(Captured {
            owner: Arc::downgrade(&owner),
            unlock: Arc::downgrade(&u),
            scope: scope.clone(),
            configuration: row.configuration,
            revision,
            credential,
            catalog_changes: changes,
            shutdown,
            metadata: MetadataLifetime(token),
        });
        captured.unchanged()?;
        scope.check(&owner, &u)?;
        Ok((u.awx_runtime.clone(), captured))
    }

    pub(crate) async fn automation_awx_register_read(
        &self,
        scope: &AwxScope,
        connection: &str,
        revision: u64,
        action: ReadAction,
    ) -> AppResult<AwxNativeOperation> {
        // Reject oversized arguments before touching storage or credentials.
        match &action {
            ReadAction::Templates { page, search }
                if *page == 0 || *page > 1000 || search.len() > 256 =>
            {
                return Err(invalid("awx_invalid_input"));
            }
            ReadAction::Prepare {
                template,
                parameters,
            } => {
                valid_id(*template).map_err(fixed_error)?;
                parameters.validate().map_err(fixed_error)?;
            }
            ReadAction::Job { job } | ReadAction::Events { job, .. } => {
                valid_id(*job).map_err(fixed_error)?;
            }
            _ => {}
        }
        let (runtime, captured) = self.capture_awx(scope, connection, revision).await?;
        let (registered, action) = register_action(&runtime, Action::Read(action))?;
        Ok(AwxNativeOperation {
            registered,
            runtime,
            captured,
            action,
        })
    }

    pub(crate) async fn automation_awx_register_launch(
        &self,
        scope: &AwxScope,
        ticket: &str,
    ) -> AppResult<AwxNativeOperation> {
        let ticket = Uuid::parse_str(ticket).map_err(|_| invalid("awx_invalid_input"))?;
        if ticket.is_nil() {
            return Err(invalid("awx_invalid_input"));
        }
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        scope.check(&owner, &u)?;
        // Explicit launch consumes before ANY secret lookup or HTTP await.
        let review = u
            .awx_runtime
            .reviews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&ticket)
            .ok_or_else(|| invalid("awx_review_unavailable"))?;
        if &review.captured.scope != scope || review.review.remaining().is_zero() {
            return Err(invalid("awx_review_unavailable"));
        }
        review.captured.unchanged()?;
        let runtime = u.awx_runtime.clone();
        let captured = review.captured.clone();
        let (registered, action) = register_action(&runtime, Action::Launch(review))?;
        Ok(AwxNativeOperation {
            registered,
            runtime,
            captured,
            action,
        })
    }

    pub(crate) async fn automation_awx_discard_review(
        &self,
        scope: &AwxScope,
        ticket: &str,
    ) -> AppResult<()> {
        let ticket = Uuid::parse_str(ticket).map_err(|_| invalid("awx_invalid_input"))?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        scope.check(&owner, &u)?;
        u.awx_runtime
            .reviews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&ticket);
        Ok(())
    }

    pub(crate) async fn automation_awx_start(
        &self,
        operation: &AwxNativeOperation,
    ) -> AppResult<Value> {
        let running = operation.registered.start().map_err(invalid)?;
        let action = operation
            .action
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .ok_or_else(|| invalid("awx_operation_consumed"))?;
        let cancel = running.cancel_token();
        let mut shutdown = operation.captured.shutdown.clone();
        let mut changes = operation.captured.catalog_changes.clone();
        let launched = std::sync::atomic::AtomicBool::new(false);
        let work = async {
            let _permit = operation
                .runtime
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid("awx_operation_budget"))?;
            operation.captured.check(self).await?;
            let authorization = operation.captured.authorization(self).await?;
            let client = match &action {
                Action::Launch(review) => {
                    AwxClient::resume_review(&review.review, authorization, cancel.clone())
                }
                Action::Read(_) => AwxClient::new(
                    &operation.captured.configuration.endpoint,
                    operation
                        .captured
                        .configuration
                        .tls
                        .custom_ca_pem
                        .as_deref(),
                    authorization,
                    Duration::from_secs(120),
                    cancel.clone(),
                ),
            }
            .map_err(fixed_error)?;
            operation.captured.check(self).await?;
            let result = match action {
                Action::Read(ReadAction::Templates { page, search }) => envelope(
                    "templates",
                    json!(client.templates(page, &search).await.map_err(fixed_error)?),
                ),
                Action::Read(ReadAction::Job { job }) => {
                    envelope("job", json!(client.job(job).await.map_err(fixed_error)?))
                }
                Action::Read(ReadAction::Events { job, after }) => envelope(
                    "events",
                    json!(client.events(job, after).await.map_err(fixed_error)?),
                ),
                Action::Read(ReadAction::Prepare {
                    template,
                    parameters,
                }) => {
                    let review = client
                        .prepare_launch(template, parameters)
                        .await
                        .map_err(fixed_error)?;
                    operation.captured.check(self).await?;
                    let ticket = Uuid::new_v4();
                    let expires = chrono::Utc::now()
                        + chrono::Duration::from_std(review.remaining())
                            .map_err(|_| invalid("awx_expired"))?;
                    let result = envelope(
                        "prepare",
                        json!({"reviewTicket":ticket,"template":review.template(),
                        "parameters":review.parameters(),"expiresUtc":expires.to_rfc3339(),
                        "connectionId":operation.captured.configuration.id,"connectionRevision":operation.captured.revision}),
                    );
                    let mut reviews = operation
                        .runtime
                        .reviews
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    reviews.retain(|_, value| {
                        !value.review.remaining().is_zero() && value.captured.unchanged().is_ok()
                    });
                    if reviews.len() >= MAX_REVIEWS {
                        return Err(invalid("awx_review_budget"));
                    }
                    reviews.insert(
                        ticket,
                        Review {
                            captured: operation.captured.clone(),
                            review,
                        },
                    );
                    result
                }
                Action::Launch(review) => {
                    let outcome = client
                        .launch_observed(review.review, || {
                            launched.store(true, std::sync::atomic::Ordering::Release);
                        })
                        .await;
                    let outcome = match outcome {
                        Err(error @ AwxError::Http(400 | 401 | 403 | 404 | 405)) => {
                            launched.store(false, std::sync::atomic::Ordering::Release);
                            return Err(fixed_error(error));
                        }
                        other => other.map_err(fixed_error)?,
                    };
                    match outcome {
                        AwxLaunchOutcome::Unknown => unknown(),
                        AwxLaunchOutcome::Started {
                            job,
                            parameters_ignored,
                        } => {
                            let url = client.original_job_url(job.id).map_err(fixed_error)?;
                            envelope(
                                "launch",
                                json!({"status":"started","job":job,"parametersIgnored":parameters_ignored,"originalJobUrl":url}),
                            )
                        }
                    }
                }
            };
            operation.captured.check(self).await?;
            Ok(result)
        };
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(invalid("awx_canceled")),
            _ = operation.captured.metadata.0.cancelled() => Err(invalid("awx_credential_changed")),
            _ = shutdown.changed() => Err(invalid("awx_context_changed")),
            _ = changes.changed() => Err(invalid("awx_connection_changed")),
            result = work => result,
        };
        // A POST might already have reached AWX. Never report retryable failure
        // after that boundary; the one-use ticket remains consumed.
        let result = if result.is_err() && launched.load(std::sync::atomic::Ordering::Acquire) {
            Ok(unknown())
        } else {
            result
        };
        drop(running); // joined only after body/response/client actually drop
        result
    }
}

#[cfg(test)]
mod tests;
