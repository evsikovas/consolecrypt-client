//! Fixed read-only Windows observations. No installer/command selection API.
use super::{begin_setup, *};
use crate::session::{Session, Unlocked};
use std::{sync::Weak, time::Instant};
use tokio_util::sync::CancellationToken;

mod probe;
#[cfg(test)]
mod tests;
const DEADLINE: Duration = Duration::from_secs(15);
fn denied(reason: &'static str) -> AppError {
    AppError::invalid("windows_prerequisites", reason)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    operation_id: String,
    profile_id: String,
    notification_scope_id: String,
}
fn canonical(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|v| !v.is_nil() && v.to_string() == value)
}
fn request(raw: &str) -> AppResult<Request> {
    if raw.len() > 1024 {
        return Err(denied("invalid_request"));
    }
    let r: Request = serde_json::from_str(raw).map_err(|_| denied("invalid_request"))?;
    if r.version != 1
        || !canonical(&r.operation_id)
        || !canonical(&r.profile_id)
        || r.notification_scope_id.is_empty()
        || r.notification_scope_id.len() > 256
        || r.notification_scope_id.chars().any(char::is_control)
    {
        return Err(denied("invalid_request"));
    }
    Ok(r)
}
struct Original {
    core: Weak<crate::app::Inner>,
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    profile: String,
    nonce: String,
    generation: u64,
    expires: Instant,
    dropped: CancellationToken,
}
impl Original {
    fn check_held(&self, u: &Arc<Unlocked>, cancel: &CancellationToken) -> AppResult<()> {
        if cancel.is_cancelled()
            || self.dropped.is_cancelled()
            || Instant::now() >= self.expires
            || self.owner.automation.stopped.is_cancelled()
            || *u.sharing_shutdown.borrow()
            || self.profile != self.owner.profile_id.to_string()
            || !Weak::ptr_eq(&self.unlock, &Arc::downgrade(u))
            || self.owner.automation.unlocked_generation()? != self.generation
            || u.notices.scope_id().as_deref() != Some(self.nonce.as_str())
        {
            return Err(denied("context_changed"));
        }
        Ok(())
    }
    async fn check(&self, cancel: &CancellationToken) -> AppResult<()> {
        let work = async {
            let app = AppCore {
                inner: self
                    .core
                    .upgrade()
                    .ok_or_else(|| denied("context_changed"))?,
            };
            let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&self.owner).await?;
            let _unlock = self.owner.hold_unlocked(&u).await?;
            self.check_held(&u, cancel)
        };
        tokio::select! {biased;
            _=cancel.cancelled()=>Err(AppError::Cancelled),
            _=self.dropped.cancelled()=>Err(AppError::Cancelled),
            _=tokio::time::sleep_until(self.expires.into())=>Err(denied("observation_timeout")),
            result=work=>result,
        }
    }
    async fn publish(
        &self,
        cancel: &CancellationToken,
        value: probe::Observed,
    ) -> AppResult<String> {
        let work = async {
            let app = AppCore {
                inner: self
                    .core
                    .upgrade()
                    .ok_or_else(|| denied("context_changed"))?,
            };
            let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&self.owner).await?;
            let _unlock = self.owner.hold_unlocked(&u).await?;
            self.check_held(&u, cancel)?;
            let encoded = serde_json::to_string(&json!({"version":1,"profileId":self.profile,
            "notificationScopeId":self.nonce,"capturedAtUtc":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),
            "source":"windowsReadOnlyV1","elevation":value.elevation,"features":value.features,"reboot":value.reboot
        })).map_err(AppError::internal)?;
            self.check_held(&u, cancel)?;
            Ok(encoded)
        };
        tokio::select! {biased;
            _=cancel.cancelled()=>Err(AppError::Cancelled),
            _=self.dropped.cancelled()=>Err(AppError::Cancelled),
            _=tokio::time::sleep_until(self.expires.into())=>Err(denied("observation_timeout")),
            result=work=>result,
        }
    }
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl AppCore {
    /// Explicit read-only snapshot. Uses the EXISTING setup map/cancel endpoint.
    /// An aborted API future cancels the detached original worker; it retains
    /// that map entry until its actual child has been killed and joined.
    pub async fn automation_windows_prerequisites_json(&self, raw: &str) -> AppResult<String> {
        let input = request(raw)?;
        #[cfg(not(windows))]
        {
            let _ = input;
            return Err(denied("unsupported_platform"));
        }
        #[cfg(windows)]
        {
            let expires = Instant::now() + DEADLINE;
            let (owner, u) = tokio::time::timeout_at(expires.into(), self.unlocked())
                .await
                .map_err(|_| denied("observation_timeout"))??;
            let dropped = CancellationToken::new();
            let original = Original {
                core: Arc::downgrade(&self.inner),
                owner: owner.clone(),
                unlock: Arc::downgrade(&u),
                profile: input.profile_id,
                nonce: input.notification_scope_id,
                generation: owner.automation.unlocked_generation()?,
                expires,
                dropped: dropped.clone(),
            };
            let mut shutdown = u.sharing_shutdown.subscribe();
            drop(u);
            let _cancel_on_drop = CancelOnDrop(dropped.clone());
            original.check(&dropped).await?;
            let (send, receive) = tokio::sync::oneshot::channel();
            // No strong Core/Unlocked across child I/O; no latest-scope lookup.
            tokio::spawn(async move {
                let result = async {
                    let (_registration, cancel) =
                        begin_setup(&owner.automation, &input.operation_id)?;
                    original.check(&cancel).await?;
                    if cancel.is_cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    let value =
                        probe::windows(&cancel, &dropped, &mut shutdown, original.expires).await?;
                    if cancel.is_cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    original.publish(&cancel, value).await
                }
                .await;
                let _ = send.send(result);
            });
            receive
                .await
                .map_err(|_| denied("observation_unavailable"))?
        }
    }
}
