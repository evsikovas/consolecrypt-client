//! One-shot original-route history observation. No Helper/Node connection.
use super::*;
use crate::service_credential_issuer::OwnerScope;
use crate::session::{Session, Unlocked};
use std::sync::Weak;

fn unavailable() -> AppError {
    AppError::invalid("service_results", "service_results_unavailable_or_changed")
}
struct ClosingSession(Option<Arc<SshSession>>, tokio::runtime::Handle);
impl Drop for ClosingSession {
    fn drop(&mut self) {
        if let Some(remote) = self.0.take() {
            self.1.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(5), remote.disconnect()).await;
            });
        }
    }
}
impl AppCore {
    pub(crate) async fn automation_service_results_original(
        &self,
        owner: Arc<Session>,
        weak: Weak<Unlocked>,
        runner_id: &str,
        expected_revision: u64,
        configuration: &AutomationRunnerConfig,
        body: Value,
        owner_scope: Arc<OwnerScope>,
        deadline: tokio::time::Instant,
    ) -> AppResult<Value> {
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        let work = async {
            let (host, identity) = configuration.service_remote()?;
            let stamp = ServiceConnectSource::capture(&u)?;
            let generation = owner.automation.unlocked_generation()?;
            let check = || async {
                let saved = owner
                    .storage()?
                    .read({
                        let id = runner_id.to_string();
                        move |tx| stored_runner(tx, &id)
                    })
                    .await?;
                if saved.read_only
                    || saved.revision != Some(expected_revision)
                    || serde_json::to_value(saved.configuration).map_err(AppError::internal)?
                        != serde_json::to_value(configuration).map_err(AppError::internal)?
                {
                    return Err(unavailable());
                }
                let _profile = self.hold_session(&owner).await?;
                let _unlock = owner.hold_unlocked(&u).await?;
                stamp.current(&u)?;
                owner_scope.check(&owner, &u)?;
                if tokio::time::Instant::now() >= deadline {
                    return Err(unavailable());
                }
                if owner.automation.unlocked_generation()? != generation
                    || owner.automation.stopped.is_cancelled()
                {
                    return Err(unavailable());
                }
                Ok::<_, AppError>(())
            };
            check().await?;
            let prepared = service_route_original(&u, host).await?;
            check().await?;
            let remote = tokio::time::timeout(
                Duration::from_secs(30),
                prepared
                    .run(async { Ok(Arc::new(u.ssh.connector.connect(&prepared.plan).await?)) }),
            )
            .await
            .map_err(|_| unavailable())??;
            let mut closing =
                ClosingSession(Some(remote.clone()), tokio::runtime::Handle::current());
            let work = async {
                check().await?;
                let result = prepared
                    .run(async {
                        cc_automation_core::ServiceRemoteConfig {
                            identity_id: identity.into(),
                        }
                        .results(&remote, body)
                        .await
                        .map_err(automation_error)
                    })
                    .await?;
                check().await?;
                Ok::<_, AppError>(result)
            }
            .await;
            let closed = tokio::time::timeout(Duration::from_secs(5), async {
                remote.disconnect().await.map_err(|_| unavailable())?;
                remote.closed().await;
                Ok::<_, AppError>(())
            })
            .await;
            if matches!(closed, Ok(Ok(()))) {
                closing.0.take();
            }
            closed.map_err(|_| unavailable())??;
            let value = work?;
            check().await?;
            Ok(value)
        };
        tokio::select! {biased;
            _=owner_scope.cancelled()=>Err(unavailable()),
            _=owner.automation.stopped.cancelled()=>Err(unavailable()),
            _=shutdown.changed()=>Err(AppError::VaultLocked),
            _=tokio::time::sleep_until(deadline)=>Err(unavailable()),
            result=work=>result,
        }
    }
}
