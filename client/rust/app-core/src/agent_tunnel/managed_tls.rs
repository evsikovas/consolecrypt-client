//! Native-only client TLS custody. Stored identity is NEVER Node trust/enrollment.
//! Every operation receives a freshly captured original scope. No Bridge surface.
use super::{scope::OriginalSessionDeviceIdentity, tls::TunnelTls};
use crate::{AppCore, AppError, AppResult};
use cc_platform_core::{MAX_SECRET_LEN, SecureStore};
use cc_storage_core::{Storage, TunnelTlsOwner, TunnelTlsPhase, TunnelTlsPublic};
use chrono::{Datelike, Utc};
use rcgen::{
    CertificateParams, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
    PublicKeyData,
};
use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock, Weak},
};
use uuid::Uuid;
use zeroize::Zeroizing;

fn unavailable() -> AppError {
    AppError::invalid("agent_tunnel", "tls_identity_unavailable")
}
fn unknown() -> AppError {
    AppError::invalid("agent_tunnel", "tls_identity_unknown")
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn os_slot() -> AppResult<tokio::sync::OwnedSemaphorePermit> {
    static JOBS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    JOBS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| unavailable())
}

pub(super) struct ManagedTlsCustody {
    scope: Arc<OriginalSessionDeviceIdentity>,
    secure: Arc<dyn SecureStore>,
    owner: TunnelTlsOwner,
}
impl std::fmt::Debug for ManagedTlsCustody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagedTlsCustody([redacted])")
    }
}
pub(super) struct ManagedTlsIdentity {
    public: TunnelTlsPublic,
    key: Zeroizing<Vec<u8>>,
    scope: Arc<OriginalSessionDeviceIdentity>,
}
impl std::fmt::Debug for ManagedTlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagedTlsIdentity([redacted])")
    }
}
impl ManagedTlsIdentity {
    /// Public metadata from THIS actual loaded key/cert pair; still no Node
    /// trust, enrollment or dispatch approval. Never exposes the secret name.
    pub(super) fn public_identity(&self) -> AppResult<&TunnelTlsPublic> {
        self.scope.check_current()?;
        validate_pair(&self.public, &self.key)?;
        Ok(&self.public)
    }
    /// Only this internal consumer can obtain maintained TLS configuration.
    /// A public certificate/fingerprint is not an enrollment or route approval.
    pub(super) fn into_tls(
        self,
        original: &Arc<OriginalSessionDeviceIdentity>,
        roots: RootCertStore,
        name: ServerName<'static>,
        policy: &cc_agent_node_wire::TunnelRoutePolicy,
    ) -> AppResult<TunnelTls> {
        if !Arc::ptr_eq(&self.scope, original) {
            return Err(unavailable());
        }
        self.scope.check_current()?;
        validate_pair(&self.public, &self.key)?;
        let tls = TunnelTls::provisioned(
            vec![CertificateDer::from(self.public.certificate_der.clone())],
            PrivatePkcs8KeyDer::from(self.key.to_vec()).into(),
            roots,
            name,
            policy,
        )
        .map_err(|_| unavailable())?;
        self.scope.check_current()?;
        Ok(tls)
    }
}
struct Generated {
    public: TunnelTlsPublic,
    key: Zeroizing<Vec<u8>>,
}
fn generate() -> AppResult<Generated> {
    let id = Uuid::new_v4();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(|_| unavailable())?;
    let der = Zeroizing::new(key.serialize_der());
    if der.is_empty() || der.len() > MAX_SECRET_LEN {
        return Err(unavailable());
    }
    let now = Utc::now();
    let end = now
        .checked_add_signed(chrono::Duration::days(30))
        .ok_or_else(unavailable)?;
    let mut params = CertificateParams::new(vec!["cc-tunnel-client.invalid".to_owned()])
        .map_err(|_| unavailable())?;
    params.not_before = rcgen::date_time_ymd(now.year(), now.month() as u8, now.day() as u8);
    params.not_after = rcgen::date_time_ymd(end.year(), end.month() as u8, end.day() as u8);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let cert = params
        .self_signed(&key)
        .map_err(|_| unavailable())?
        .der()
        .to_vec();
    let public = TunnelTlsPublic {
        identity_id: id,
        certificate_sha256: hash(&cert),
        private_key_sha256: hash(&der),
        certificate_der: cert,
    };
    validate_pair(&public, &der)?;
    Ok(Generated { public, key: der })
}
fn validate_pair(public: &TunnelTlsPublic, key: &[u8]) -> AppResult<()> {
    if key.is_empty()
        || key.len() > MAX_SECRET_LEN
        || public.certificate_der.len() > 4096
        || hash(key) != public.private_key_sha256
        || hash(&public.certificate_der) != public.certificate_sha256
    {
        return Err(unknown());
    }
    let pair = KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(key),
        &PKCS_ECDSA_P256_SHA256,
    )
    .map_err(|_| unknown())?;
    let (tail, cert) =
        x509_parser::parse_x509_certificate(&public.certificate_der).map_err(|_| unknown())?;
    if !tail.is_empty() || cert.public_key().raw != pair.subject_public_key_info() {
        return Err(unknown());
    }
    let now = Utc::now().timestamp();
    if cert.validity().not_before.timestamp() > now {
        return Err(unavailable());
    }
    if cert.validity().not_after.timestamp() <= now {
        return Err(AppError::invalid("agent_tunnel", "tls_identity_expired"));
    }
    // Fixed Native generation only; parsing never grants trust to imported PEM.
    Ok(())
}
impl ManagedTlsCustody {
    pub(super) async fn capture(
        app: &AppCore,
        scope: Arc<OriginalSessionDeviceIdentity>,
    ) -> AppResult<Self> {
        let (secure, owner) = scope
            .with_resources(app, |session, u, id| {
                let storage = session.storage()?;
                if storage.path().is_none() {
                    return Err(unavailable());
                }
                let context = serde_json::to_vec(&serde_json::json!({
                    "profile":session.profile_id,"vault":u.vault_id,
                "owner":scope.owner_context()?,
                    "origin":session.api_opt().map(|api|api.config().server_url.to_string()),
                }))
                .map_err(|_| unavailable())?;
                Ok((
                    session.ctx.secure.clone(),
                    TunnelTlsOwner {
                        profile_id: session.profile_id,
                        device_id: id.device_id(),
                        signer_fingerprint: hash(&id.public_keys().signing),
                        context_fingerprint: hash(&context),
                    },
                ))
            })
            .await?;
        Ok(Self {
            scope,
            secure,
            owner,
        })
    }
    /// Acquire only from this original weak scope for one bounded SQL call.
    /// An idle custody handle must not keep the SQLCipher worker/key alive.
    async fn storage(&self, app: &AppCore) -> AppResult<Storage> {
        self.storage_original(&Arc::downgrade(&app.inner)).await
    }
    async fn check_original(&self, core: &Weak<crate::app::Inner>) -> AppResult<()> {
        let app = AppCore {
            inner: core.upgrade().ok_or_else(unavailable)?,
        };
        self.scope.check(&app).await
    }
    async fn storage_original(&self, core: &Weak<crate::app::Inner>) -> AppResult<Storage> {
        let app = AppCore {
            inner: core.upgrade().ok_or_else(unavailable)?,
        };
        self.scope
            .with_resources(&app, |session, _, _| {
                let storage = session.storage()?;
                if storage.path().is_none() {
                    return Err(unavailable());
                }
                Ok(storage)
            })
            .await
    }
    // SQL waits are cancelable; the worker callback still checks the original
    // scope if already queued. OS jobs use their separate owned join below.
    async fn sql<T>(&self, work: impl std::future::Future<Output = AppResult<T>>) -> AppResult<T> {
        self.scope.check_current()?;
        tokio::select! {biased;_=self.scope.cancelled()=>Err(unknown()),result=work=>{self.scope.check_current()?;result}}
    }
    pub(super) async fn stored_metadata(
        &self,
        app: &AppCore,
    ) -> AppResult<(Option<TunnelTlsPhase>, Option<TunnelTlsPublic>)> {
        self.stored_metadata_original(&Arc::downgrade(&app.inner)).await
    }
    /// Public metadata only; no TLS private-key read and no Core across SQL.
    pub(super) async fn stored_metadata_original(
        &self,core:&Weak<crate::app::Inner>,
    )->AppResult<(Option<TunnelTlsPhase>,Option<TunnelTlsPublic>)> {
        self.check_original(core).await?;
        let owner=self.owner.clone();let scope=self.scope.clone();
        let value=self.sql(async {
            self.storage_original(core).await?.read(move |tx|scope.with_io_current(|_| {
                let row=tx.tunnel_tls_snapshot(&owner)?;
                Ok::<_,AppError>((row.phase(),row.public().cloned()))
            })).await.map_err(AppError::from)
        }).await?;
        self.check_original(core).await?;
        Ok(value)
    }
    pub(super) async fn creation_absent(&self, app: &AppCore) -> AppResult<()> {
        if self.stored_metadata(app).await?.0.is_some() {
            return Err(unknown());
        }
        let head = self.witness_name();
        self.os(move |s| {
            if s.get(&head).map_err(|_| unknown())?.is_some() {
                return Err(unknown());
            }
            Ok(())
        })
        .await?;
        self.scope.check(app).await
    }
    fn secret_name(&self, id: Uuid) -> String {
        format!("profiles/{}/tunnel-tls/{id}", self.owner.profile_id)
    }
    fn witness_name(&self) -> String {
        format!("profiles/{}/tunnel-tls-head-v1", self.owner.profile_id)
    }
    fn witness(&self, p: &TunnelTlsPublic) -> AppResult<Vec<u8>> {
        let bytes = serde_json::to_vec(&(&self.owner, p)).map_err(|_| unavailable())?;
        Ok(Sha256::digest(bytes).to_vec())
    }
    async fn os<T: Send + 'static>(
        &self,
        f: impl FnOnce(Arc<dyn SecureStore>) -> AppResult<T> + Send + 'static,
    ) -> AppResult<T> {
        self.os_with_slot(os_slot()?, f).await
    }
    async fn os_with_slot<T: Send + 'static>(
        &self,
        permit: tokio::sync::OwnedSemaphorePermit,
        f: impl FnOnce(Arc<dyn SecureStore>) -> AppResult<T> + Send + 'static,
    ) -> AppResult<T> {
        self.scope.check_current()?;
        let secure = self.secure.clone();
        let scope = self.scope.clone();
        // A dropped caller does not cancel a running OS job. Its owned data
        // stays with that job; Pending SQL prevents replacement/re-generation.
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            scope.check_current()?;
            let out = f(secure)?;
            scope.check_current()?;
            Ok(out)
        })
        .await
        .map_err(|_| unknown())?;
        self.scope.check_current()?;
        result
    }
    /// Explicit first creation only. No automatic recovery, overwrite or reset.
    pub(super) async fn create_explicit(&self, app: &AppCore) -> AppResult<TunnelTlsPublic> {
        self.create_original(&Arc::downgrade(&app.inner)).await
    }
    /// Issued public handles keep only Weak Core even while awaiting an actual
    /// uncancellable OS job. Original checks alone may temporarily upgrade it.
    pub(super) async fn create_original(
        &self,
        core: &Weak<crate::app::Inner>,
    ) -> AppResult<TunnelTlsPublic> {
        self.check_original(core).await?;
        let owner = self.owner.clone();
        let scope = self.scope.clone();
        let snapshot = self
            .sql(async {
                self.storage_original(core)
                    .await?
                    .read(move |tx| {
                        scope.check_current()?;
                        Ok::<_, AppError>(tx.tunnel_tls_snapshot(&owner)?)
                    })
                    .await
                    .map_err(AppError::from)
            })
            .await?;
        if snapshot.phase().is_some() {
            return Err(unavailable());
        }
        let witness_name = self.witness_name();
        self.os(move |s| {
            if s.get(&witness_name).map_err(|_| unknown())?.is_some() {
                return Err(unknown());
            }
            Ok(())
        })
        .await?;
        let generated = generate()?;
        // Quota refusal must occur BEFORE the irreversible SQL reservation.
        let slot = os_slot()?;
        let public = generated.public.clone();
        let p = public.clone();
        let scope = self.scope.clone();
        self.sql(async {
            self.storage_original(core)
                .await?
                .write_tunnel_tls(move |tx| {
                    scope.check_current()?;
                    tx.tunnel_tls_reserve(&snapshot, p).map_err(AppError::from)
                })
                .await
                .map_err(|_| unknown())
        })
        .await?;
        // After reservation EVERY failure/late cancellation is unknown. Never
        // delete/reset the row or regenerate the missing/replaced private key.
        let name = self.secret_name(public.identity_id);
        let head = self.witness_name();
        let witness = self.witness(&public)?;
        let original = self.scope.clone();
        self.os_with_slot(slot, move |s| {
            original.check_current().map_err(|_| unknown())?;
            if s.get(&name).map_err(|_| unknown())?.is_some()
                || s.get(&head).map_err(|_| unknown())?.is_some()
            {
                return Err(unknown());
            }
            original.check_current().map_err(|_| unknown())?;
            s.set(&name, &generated.key).map_err(|_| unknown())?;
            original.check_current().map_err(|_| unknown())?;
            let read = s.get(&name).map_err(|_| unknown())?.ok_or_else(unknown)?;
            if read.expose_secret() != generated.key.as_slice() {
                return Err(unknown());
            }
            original.check_current().map_err(|_| unknown())?;
            s.set(&head, &witness).map_err(|_| unknown())?;
            original.check_current().map_err(|_| unknown())?;
            if s.get(&head)
                .map_err(|_| unknown())?
                .ok_or_else(unknown)?
                .expose_secret()
                != witness.as_slice()
            {
                return Err(unknown());
            }
            Ok(())
        })
        .await
        .map_err(|_| unknown())?;
        self.check_original(core).await.map_err(|_| unknown())?;
        let owner = self.owner.clone();
        let scope = self.scope.clone();
        let expected_public = public.clone();
        self.sql(async {
            self.storage_original(core)
                .await?
                .write_tunnel_tls(move |tx| {
                    scope.check_current()?;
                    let pending = tx.tunnel_tls_snapshot(&owner)?;
                    if pending.public() != Some(&expected_public) {
                        return Err(unknown());
                    }
                    tx.tunnel_tls_complete(&pending).map_err(AppError::from)
                })
                .await
                .map_err(|_| unknown())
        })
        .await?;
        let loaded = self.load_original(core).await.map_err(|_| unknown())?;
        if loaded.public != public {
            return Err(unknown());
        }
        Ok(public)
    }
    pub(super) async fn load(&self, app: &AppCore) -> AppResult<ManagedTlsIdentity> {
        self.load_original(&Arc::downgrade(&app.inner)).await
    }
    pub(super) async fn load_original(
        &self,
        core: &Weak<crate::app::Inner>,
    ) -> AppResult<ManagedTlsIdentity> {
        self.check_original(core).await?;
        let owner = self.owner.clone();
        let scope = self.scope.clone();
        let snapshot = self
            .sql(async {
                self.storage_original(core)
                    .await?
                    .read(move |tx| {
                        scope.check_current()?;
                        Ok::<_, AppError>(tx.tunnel_tls_snapshot(&owner)?)
                    })
                    .await
                    .map_err(AppError::from)
            })
            .await?;
        if snapshot.phase() != Some(TunnelTlsPhase::Stored) {
            return Err(unknown());
        }
        let public = snapshot.public().ok_or_else(unknown)?.clone();
        let name = self.secret_name(public.identity_id);
        let head = self.witness_name();
        let witness = self.witness(&public)?;
        let p = public.clone();
        let key = self
            .os(move |s| {
                let key = s.get(&name).map_err(|_| unknown())?.ok_or_else(unknown)?;
                let h = s.get(&head).map_err(|_| unknown())?.ok_or_else(unknown)?;
                if h.expose_secret() != witness.as_slice() {
                    return Err(unknown());
                }
                validate_pair(&p, key.expose_secret())?;
                Ok(Zeroizing::new(key.expose_secret().to_vec()))
            })
            .await?;
        self.check_original(core).await?;
        Ok(ManagedTlsIdentity {
            public,
            key,
            scope: self.scope.clone(),
        })
    }
    /// Only public certificate bytes, using the existing held-directory
    /// no-clobber editor primitive. Never removes a published destination.
    pub(super) async fn export_public(&self, app: &AppCore, destination: PathBuf) -> AppResult<()> {
        let identity = self.load(app).await?;
        let cert = pem::encode(&pem::Pem::new(
            "CERTIFICATE",
            identity.public.certificate_der.clone(),
        ));
        drop(identity);
        let scope = self.scope.clone();
        self.os(move |_| {
            scope.check_current()?;
            if !destination.is_absolute() {
                return Err(unavailable());
            }
            let name = destination
                .file_name()
                .and_then(|v| v.to_str())
                .filter(|n| n.ends_with(".pem"))
                .ok_or_else(unavailable)?;
            let parent = cc_automation_core::ProjectRoot::open(
                destination.parent().ok_or_else(unavailable)?,
            )
            .map_err(|_| unavailable())?;
            scope.check_current()?;
            parent
                .write_file(name, None, &cert)
                .map_err(|_| unknown())?;
            scope.check_current().map_err(|_| unknown())
        })
        .await
        .map_err(|_| unknown())?;
        self.scope.check(app).await.map_err(|_| unknown())
    }
}

#[cfg(test)]
mod tests;
