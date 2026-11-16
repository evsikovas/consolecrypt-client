//! Original-owner LocalOnly metadata. No signer/grant can be reconstructed here.
use super::*;
use cc_storage_core::{
    NodeAssociationCertificate, NodeAssociationOwner, NodeAssociationPersonalOwner,
    NodeAssociationPublic, NodeAssociationSnapshot, ProfileKind, Storage,
};
pub(super) struct Ledger {
    scope: Arc<OriginalScope>,
    owner: NodeAssociationOwner,
    until: Option<Instant>,
}
impl Ledger {
    pub(super) fn audit_owner(&self) -> AppResult<NodeAssociationOwner> {
        self.scope.check_current()?;
        Ok(self.owner.clone())
    }
    pub(super) async fn capture(app: &AppCore, scope: Arc<OriginalScope>) -> AppResult<Self> {
        scope.check(app).await?;
        let value = Self::capture_original(scope).await?;
        value.scope.check(app).await?;
        Ok(value)
    }
    /// Original weak provenance only; no AppCore across SQL queue/commit.
    pub(super) async fn capture_original(scope: Arc<OriginalScope>) -> AppResult<Self> {
        let context = scope.owner_context()?; // Before the non-reentrant I/O guard.
        let (
            storage,
            profile_id,
            device_id,
            vault_id,
            signing_fingerprint,
            context,
            commitment,
            origin,
        ) = scope
            .with_current_resources(|s, u, id| {
                let storage = s.storage()?;
                if storage.path().is_none() {
                    return Err(unavailable());
                }
                Ok((
                    storage,
                    s.profile_id,
                    id.device_id(),
                    u.vault_id,
                    hash(&id.public_keys().signing),
                    context,
                    hex::encode(u.vault()?.key_commitment()),
                    s.api_opt().map(|a| a.config().server_url.to_string()),
                ))
            })?;
        let original = scope.clone();
        let owner = storage
            .read(move |tx| {
                original.with_io_current(|_| {
                    let profile = tx.get_profile()?.ok_or_else(unavailable)?;
                    if profile.profile_id != profile_id || profile.device_id != device_id {
                        return Err(unavailable());
                    }
                    let personal = match (&context, profile.kind) {
                        (
                            super::super::scope::OriginalOwnerContext::LocalProfile {
                                profile_id: actual,
                            },
                            ProfileKind::Local,
                        ) if *actual == profile_id
                            && profile.user_id.is_none()
                            && origin.is_none() =>
                        {
                            None
                        }
                        (
                            super::super::scope::OriginalOwnerContext::PersonalAccount { user_id },
                            ProfileKind::Synced,
                        ) if profile.user_id == Some(*user_id) => {
                            let raw = profile.server_url.as_ref().ok_or_else(unavailable)?;
                            let parsed = url::Url::parse(raw.trim()).map_err(|_| unavailable())?;
                            if origin.as_deref() != Some(parsed.as_str()) {
                                return Err(unavailable());
                            }
                            let mut bytes = b"ConsoleCrypt NodeAssociationOrigin v1\0".to_vec();
                            bytes.extend_from_slice(parsed.as_str().as_bytes());
                            Some(NodeAssociationPersonalOwner {
                                user_id: *user_id,
                                server_url: raw.clone(),
                                origin_fingerprint: hash(&bytes),
                                vault_commitment: commitment,
                            })
                        }
                        _ => return Err(unavailable()),
                    };
                    let owner = NodeAssociationOwner {
                        profile_id,
                        device_id,
                        vault_id,
                        signing_fingerprint,
                        personal,
                    };
                    tx.node_association_snapshot(&owner)?; // Same SQL personal OWNER/Active/trusted pin.
                    Ok::<_, AppError>(owner)
                })
            })
            .await?;
        scope.check_current()?;
        Ok(Self {
            scope,
            owner,
            until: None,
        })
    }
    pub(super) fn cap_review(&mut self, end: Instant) -> AppResult<()> {
        consent_live(end)?;
        self.until = Some(self.until.map_or(end, |old| old.min(end)));
        Ok(())
    }
    fn deadline(&self) -> AppResult<Instant> {
        let end = self.until.ok_or_else(unknown)?;
        consent_live(end)?;
        Ok(end)
    }
    async fn current(&self, app: &AppCore) -> AppResult<()> {
        match self.until {
            Some(end) => under_review(end, self.scope.check(app)).await,
            None => self.scope.check(app).await,
        }
    }
    async fn storage(&self, app: &AppCore) -> AppResult<Storage> {
        let acquire = self.scope.with_resources(app, |session, _, _| {
            let storage = session.storage()?;
            if storage.path().is_none() {
                return Err(unavailable());
            }
            Ok(storage)
        });
        match self.until {
            Some(end) => under_review(end, acquire).await,
            None => acquire.await,
        }
    }
    pub(super) async fn snapshot(&self, app: &AppCore) -> AppResult<NodeAssociationSnapshot> {
        self.current(app).await?;
        let snapshot = self.snapshot_original().await?;
        self.current(app).await?;
        Ok(snapshot)
    }
    pub(super) async fn snapshot_original(&self) -> AppResult<NodeAssociationSnapshot> {
        self.scope.check_current()?;
        if let Some(end) = self.until { consent_live(end)?; }
        let scope = self.scope.clone();
        let until = self.until;
        let owner = self.owner.clone();
        let storage = self.scope.with_current_resources(|session, _, _| {
            let storage = session.storage()?;
            if storage.path().is_none() { return Err(unavailable()); }
            Ok(storage)
        })?;
        let snapshot = storage.read(move |tx| {
            scope.with_io_current(|_| {
                if let Some(end) = until { consent_live(end)?; }
                Ok::<_, AppError>(tx.node_association_snapshot(&owner)?)
            })
        }).await?;
        self.scope.check_current()?;
        if let Some(end) = self.until { consent_live(end)?; }
        Ok(snapshot)
    }
    pub(super) async fn require_unassociated(
        &self,
        app: &AppCore,
        host: ObjectId,
        agent: AgentId,
    ) -> AppResult<()> {
        let agent = serde_json::to_value(agent).map_err(|_| unavailable())?;
        let agent = agent.as_str().ok_or_else(unavailable)?;
        if self
            .snapshot(app)
            .await?
            .records()
            .iter()
            .any(|r| r.public.saved_host_id == host && r.public.agent_id == agent)
        {
            return Err(unknown());
        }
        Ok(())
    }
    pub(super) async fn reserve(
        &self,
        app: &AppCore,
        public: NodeAssociationPublic,
    ) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_reserve(&snapshot, public)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current(app).await
    }
    pub(super) async fn mark_unknown(&self, app: &AppCore, id: Uuid) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_mark_unknown(&snapshot, id)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current(app).await
    }
    pub(super) async fn attach(
        &self,
        app: &AppCore,
        id: Uuid,
        certificate: NodeAssociationCertificate,
    ) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_attach_certificate(&snapshot, id, certificate)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current(app).await
    }
    // Private caller has actual verified apply AND observe on original peer.
    pub(super) async fn complete(&self, app: &AppCore, id: Uuid) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_complete(&snapshot, id)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current(app).await
    }
    pub(super) async fn begin_revocation(&self, app: &AppCore, expected: cc_storage_core::NodeAssociationRecord,
        intent: cc_storage_core::NodeAssociationRevocation) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(until)?;
            tx.node_association_begin_revocation(&snapshot, &expected, intent).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_| unknown())?;
        self.current(app).await
    }
    pub(super) async fn complete_revocation(&self, app: &AppCore, id: Uuid,
        intent: cc_storage_core::NodeAssociationRevocation) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot(app).await?;
        let scope = self.scope.clone();
        let storage = self.storage(app).await?;
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(until)?;
            tx.node_association_complete_revocation(&snapshot, id, &intent).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_| unknown())?;
        self.current(app).await
    }

    pub(super) async fn begin_rotation(&self,app:&AppCore,expected:cc_storage_core::NodeAssociationRecord,
        intent:cc_storage_core::NodeAssociationRotation)->AppResult<()> {
        let until=self.deadline()?;let snapshot=self.snapshot(app).await?;let scope=self.scope.clone();
        let storage=self.storage(app).await?;
        storage.write_node_association(move|tx|scope.with_io_current(|_|{
            consent_live(until)?;tx.node_association_begin_rotation(&snapshot,&expected,intent).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_|unknown())?;self.current(app).await
    }
    pub(super) async fn update_rotation(&self,app:&AppCore,id:Uuid,expected:cc_storage_core::NodeAssociationRotation,
        next:cc_storage_core::NodeAssociationRotation)->AppResult<()> {
        let until=self.deadline()?;let snapshot=self.snapshot(app).await?;let scope=self.scope.clone();
        let storage=self.storage(app).await?;
        storage.write_node_association(move|tx|scope.with_io_current(|_|{
            consent_live(until)?;tx.node_association_update_rotation(&snapshot,id,&expected,next).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_|unknown())?;self.current(app).await
    }

}

fn decode<T:serde::de::DeserializeOwned>(raw:&[u8])->AppResult<T> {
    if raw.is_empty()||raw.len()>16384 {return Err(unavailable())}
    serde_json::from_slice(raw).map_err(|_|unavailable())
}
pub(super) fn initial_certificate(row:&cc_storage_core::NodeAssociationRecord,key:PublicKey)->AppResult<Signed<Certificate>> {
    let cert=decode::<Signed<Certificate>>(&row.public.certificate.as_ref().ok_or_else(unavailable)?.signed_certificate_json)?;
    verify_statement(&cert,key).map_err(|_|unavailable())?;Ok(cert)
}
pub(super) fn rotation_parts(row:&cc_storage_core::NodeAssociationRecord,key:PublicKey)
    ->AppResult<(protocol::RotationCandidate,protocol::RotationProofs,OwnerPin)> {
    let rotation=row.rotation.as_ref().ok_or_else(unavailable)?;
    let candidate=decode::<protocol::RotationCandidate>(rotation.candidate_json.as_ref().ok_or_else(unavailable)?)?;
    let proofs=protocol::RotationProofs {
        certificate:decode(rotation.signed_certificate_json.as_ref().ok_or_else(unavailable)?)?,
        old:decode(rotation.old_proof_json.as_ref().ok_or_else(unavailable)?)?,
        new:decode(rotation.new_proof_json.as_ref().ok_or_else(unavailable)?)?,
    };
    let previous=initial_certificate(row,key)?;protocol::historical_rotation(&previous,&candidate,&proofs,key)?;
    let pin=decode::<OwnerPin>(rotation.committed_owner_pin_json.as_ref().ok_or_else(unavailable)?)?;
    let cert_row=row.public.certificate.as_ref().ok_or_else(unavailable)?;
    let mut expected=decode::<OwnerPin>(&cert_row.committed_owner_pin_json)?;
    expected.checkpoint=proofs.certificate.body.header.checkpoint;
    if pin!=expected || pin.key!=key || rotation.expected_actor_revision<=cert_row.expected_actor_revision {
        return Err(unavailable())
    }
    Ok((candidate,proofs,pin))
}
/// Current certificate is selected ONLY from original enrollment or a completed
/// exact historical rotation chain; pending signed proposals are not current.
pub(super) fn current_certificate(row:&cc_storage_core::NodeAssociationRecord,key:PublicKey)->AppResult<Signed<Certificate>> {
    if row.rotation.as_ref().is_some_and(|r|r.selected_observed) {
        let (_,proofs,commit)=rotation_parts(row,key)?;
        let rotation=row.rotation.as_ref().ok_or_else(unavailable)?;
        let observed=decode::<OwnerPin>(rotation.observed_owner_pin_json.as_ref().ok_or_else(unavailable)?)?;
        let mut expected=commit.clone();expected.checkpoint=observed.checkpoint;
        if observed!=expected || observed.checkpoint<commit.checkpoint
            || rotation.observed_actor_revision.is_none_or(|n|n<rotation.expected_actor_revision) {
            return Err(unavailable())
        }
        Ok(proofs.certificate)
    }else{initial_certificate(row,key)}
}
pub(super) fn current_pin(row:&cc_storage_core::NodeAssociationRecord,key:PublicKey)->AppResult<OwnerPin> {
    current_certificate(row,key)?;
    if let Some(rotation)=row.rotation.as_ref().filter(|r|r.selected_observed) {
        decode(rotation.observed_owner_pin_json.as_ref().ok_or_else(unavailable)?)
    }else{decode(&row.public.certificate.as_ref().ok_or_else(unavailable)?.committed_owner_pin_json)}
}

// Metrics enrollment uses these original weak-owner SQL siblings. Legacy
// Tunnel entrypoints above stay byte-for-byte intact; neither path grants a
// network operation or reconstructs a signer from stored metadata.
impl Ledger {
    fn current_original(&self) -> AppResult<()> {
        self.scope.check_current()?;
        if let Some(end) = self.until { consent_live(end)?; }
        Ok(())
    }
    fn storage_original(&self) -> AppResult<Storage> {
        self.current_original()?;
        self.scope.with_current_resources(|session, _, _| {
            let storage = session.storage()?;
            if storage.path().is_none() { return Err(unavailable()); }
            Ok(storage)
        })
    }
    pub(super) async fn require_unassociated_original(
        &self,
        host: ObjectId,
        agent: AgentId,
    ) -> AppResult<()> {
        let agent = serde_json::to_value(agent).map_err(|_| unavailable())?;
        let agent = agent.as_str().ok_or_else(unavailable)?;
        if self
            .snapshot_original()
            .await?
            .records()
            .iter()
            .any(|r| r.public.saved_host_id == host && r.public.agent_id == agent)
        {
            return Err(unknown());
        }
        Ok(())
    }
    pub(super) async fn reserve_original(
        &self,
        public: NodeAssociationPublic,
    ) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_reserve(&snapshot, public)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
    pub(super) async fn mark_unknown_original(&self, id: Uuid) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_mark_unknown(&snapshot, id)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
    pub(super) async fn attach_original(
        &self,
        id: Uuid,
        certificate: NodeAssociationCertificate,
    ) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_attach_certificate(&snapshot, id, certificate)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
    pub(super) async fn complete_original(&self, id: Uuid) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(until)?;
                    tx.node_association_complete(&snapshot, id)
                        .map_err(AppError::from)?;
                    consent_live(until)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
    pub(super) async fn begin_revocation_original(&self, expected: cc_storage_core::NodeAssociationRecord,
        intent: cc_storage_core::NodeAssociationRevocation) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(until)?;
            tx.node_association_begin_revocation(&snapshot, &expected, intent).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_| unknown())?;
        self.current_original()
    }
    pub(super) async fn complete_revocation_original(&self, id: Uuid,
        intent: cc_storage_core::NodeAssociationRevocation) -> AppResult<()> {
        let until = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(until)?;
            tx.node_association_complete_revocation(&snapshot, id, &intent).map_err(AppError::from)?;
            consent_live(until)
        })).await.map_err(|_| unknown())?;
        self.current_original()
    }
}

mod collector_service;

#[path="ledger/collector_service_lifetime.rs"]
mod collector_service_lifetime;

impl Ledger {
    pub(super) fn require_original_scope(&self,original:&Arc<OriginalScope>)->AppResult<()> {
        if !Arc::ptr_eq(original,&self.scope){return Err(unavailable())}
        self.current_original()
    }
}
