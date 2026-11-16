//! Bridges ssh-core's interactive questions (unknown host key, key
//! passphrase) to the UI: a [`PromptRequest`] is broadcast to
//! [`crate::AppCore::subscribe_prompts`] subscribers and the connect waits
//! for `answer_*` (or a timeout). Without subscribers the safe default
//! applies immediately (reject / no passphrase).

use crate::dto::{HostKeyPromptDto, PassphrasePromptDto, PasswordPromptDto, PromptRequest};
use crate::error::{AppError, AppResult};
use cc_ssh_core::{HostKeyDecision, HostKeyInfo};
use secrecy::SecretString;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionDecision {
    Wait,
    CancelJobs,
    CancelJobsAcceptUnknown,
    Abort,
}

pub(crate) struct TransitionQuestion<'a> {
    broker: &'a PromptBroker,
    id: String,
    pub choices: tokio::sync::mpsc::Receiver<TransitionDecision>,
    completed: tokio::sync::watch::Sender<bool>,
}

impl Drop for TransitionQuestion<'_> {
    fn drop(&mut self) {
        self.broker.lock().remove(&self.id);
        let _ = self.completed.send(true);
    }
}

enum Pending {
    HostKey(oneshot::Sender<HostKeyDecision>),
    Secret(oneshot::Sender<Option<SecretString>>),
    Transition(
        tokio::sync::mpsc::Sender<TransitionDecision>,
        tokio::sync::watch::Receiver<bool>,
    ),
}

pub(crate) struct PromptBroker {
    tx: broadcast::Sender<PromptRequest>,
    pending: Mutex<HashMap<String, Pending>>,
    timeout: Duration,
}

impl std::fmt::Debug for PromptBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptBroker")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl PromptBroker {
    pub(crate) fn transition(
        &self,
        job_count: u32,
        unknown_count: u32,
        operation: &str,
    ) -> AppResult<TransitionQuestion<'_>> {
        if self.tx.receiver_count() == 0 {
            return Err(AppError::invalid(
                "automation_transition",
                "active jobs require an explicit transition choice",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, choices) = tokio::sync::mpsc::channel(4);
        let (completed, completion) = tokio::sync::watch::channel(false);
        self.lock()
            .insert(id.clone(), Pending::Transition(sender, completion));
        let question = TransitionQuestion {
            broker: self,
            id: id.clone(),
            choices,
            completed,
        };
        self.tx
            .send(PromptRequest::AutomationTransition(
                crate::dto::AutomationTransitionPromptDto {
                    request_id: id,
                    job_count,
                    unknown_count,
                    operation: operation.into(),
                },
            ))
            .map_err(|_| {
                AppError::invalid("automation_transition", "transition presenter unavailable")
            })?;
        Ok(question)
    }

    pub(crate) async fn answer_transition(
        &self,
        id: &str,
        decision: TransitionDecision,
    ) -> AppResult<()> {
        let (sender, mut completion) = {
            let pending = self.lock();
            match pending.get(id) {
                Some(Pending::Transition(sender, completed)) => (sender.clone(), completed.clone()),
                Some(_) => {
                    return Err(AppError::invalid(
                        "request_id",
                        "not an automation transition prompt",
                    ))
                }
                None => return Err(AppError::not_found("prompt", id)),
            }
        };
        sender
            .send(decision)
            .await
            .map_err(|_| AppError::not_found("prompt", id))?;
        // The UI can render cancellable progress while this specific reply is
        // pending; a second Abort/CancelJobs reply remains possible.
        if decision != TransitionDecision::Abort {
            loop {
                if *completion.borrow_and_update() {
                    break;
                }
                if completion.changed().await.is_err() {
                    break;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            tx: broadcast::channel(64).0,
            pending: Mutex::new(HashMap::new()),
            timeout,
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<PromptRequest> {
        self.tx.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) async fn host_key(&self, info: HostKeyInfo) -> HostKeyDecision {
        if self.tx.receiver_count() == 0 {
            return HostKeyDecision::Reject;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.lock().insert(id.clone(), Pending::HostKey(tx));
        let req = PromptRequest::HostKey(HostKeyPromptDto {
            request_id: id.clone(),
            host: info.host,
            port: info.port,
            host_pattern: info.host_pattern,
            host_id: info.host_id.map(|i| i.to_string()),
            host_name: info.host_name,
            hop_index: info.hop_index as u32,
            hop_count: info.hop_count as u32,
            key_type: info.key_type,
            fingerprint_sha256: info.fingerprint_sha256,
            other_known_key_types: info.other_known_key_types,
        });
        if self.tx.send(req).is_err() {
            self.lock().remove(&id);
            return HostKeyDecision::Reject;
        }
        let r = tokio::time::timeout(self.timeout, rx).await;
        self.lock().remove(&id);
        match r {
            Ok(Ok(d)) => d,
            _ => HostKeyDecision::Reject,
        }
    }

    pub(crate) async fn passphrase(
        &self,
        credential_id: String,
        credential_name: String,
        attempt: u32,
    ) -> Option<SecretString> {
        if self.tx.receiver_count() == 0 {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.lock().insert(id.clone(), Pending::Secret(tx));
        let req = PromptRequest::Passphrase(PassphrasePromptDto {
            request_id: id.clone(),
            credential_id,
            credential_name,
            attempt,
        });
        self.ask_secret(id, req, rx).await
    }

    /// Ask for a host password (auth mode `PasswordPrompt`).
    pub(crate) async fn password(
        &self,
        host_id: Option<String>,
        host_name: String,
    ) -> Option<SecretString> {
        if self.tx.receiver_count() == 0 {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.lock().insert(id.clone(), Pending::Secret(tx));
        let req = PromptRequest::Password(PasswordPromptDto {
            request_id: id.clone(),
            host_id,
            host_name,
        });
        self.ask_secret(id, req, rx).await
    }

    async fn ask_secret(
        &self,
        id: String,
        req: PromptRequest,
        rx: oneshot::Receiver<Option<SecretString>>,
    ) -> Option<SecretString> {
        if self.tx.send(req).is_err() {
            self.lock().remove(&id);
            return None;
        }
        let r = tokio::time::timeout(self.timeout, rx).await;
        self.lock().remove(&id);
        match r {
            Ok(Ok(p)) => p,
            _ => None,
        }
    }

    pub(crate) fn answer_host_key(&self, request_id: &str, d: HostKeyDecision) -> AppResult<()> {
        let removed = self.lock().remove(request_id);
        match removed {
            Some(Pending::HostKey(tx)) => {
                let _ = tx.send(d);
                Ok(())
            }
            Some(other) => {
                self.lock().insert(request_id.to_owned(), other);
                Err(AppError::invalid("request_id", "not a host key prompt"))
            }
            None => Err(AppError::not_found("prompt", request_id)),
        }
    }

    /// Answer a passphrase or password prompt.
    pub(crate) fn answer_secret(
        &self,
        request_id: &str,
        secret: Option<SecretString>,
    ) -> AppResult<()> {
        let removed = self.lock().remove(request_id);
        match removed {
            Some(Pending::Secret(tx)) => {
                let _ = tx.send(secret);
                Ok(())
            }
            Some(other) => {
                self.lock().insert(request_id.to_owned(), other);
                Err(AppError::invalid(
                    "request_id",
                    "not a passphrase/password prompt",
                ))
            }
            None => Err(AppError::not_found("prompt", request_id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn info() -> HostKeyInfo {
        HostKeyInfo {
            host: "h".into(),
            port: 22,
            host_pattern: "h".into(),
            host_id: None,
            host_name: None,
            hop_index: 0,
            hop_count: 1,
            key_type: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:x".into(),
            public_key: "AAAA".into(),
            other_known_key_types: vec![],
        }
    }

    #[tokio::test]
    async fn no_subscriber_rejects_and_answers_route() {
        let b = Arc::new(PromptBroker::new(Duration::from_secs(5)));
        assert_eq!(b.host_key(info()).await, HostKeyDecision::Reject);
        let mut rx = b.subscribe();
        let b2 = b.clone();
        let t = tokio::spawn(async move { b2.host_key(info()).await });
        let PromptRequest::HostKey(p) = rx.recv().await.unwrap() else {
            panic!()
        };
        assert!(b.answer_secret(&p.request_id, None).is_err());
        b.answer_host_key(&p.request_id, HostKeyDecision::AcceptOnce)
            .unwrap();
        assert_eq!(t.await.unwrap(), HostKeyDecision::AcceptOnce);
    }
}
