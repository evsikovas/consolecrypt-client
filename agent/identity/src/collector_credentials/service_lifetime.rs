//! New private service-lifetime path. Does not alter SingleStart open_original.
//! Caller must retrieve the exact complete set from Core's admitted journal.
use super::*;
use crate::{CollectorServiceCredentialAuthorization, CollectorServiceCredentialRef,
    RegisteredCredentialWriter, Timestamp};

struct Entry {
    material: cc_crypto_core::OpenedServiceCredential,
    expected: cc_crypto_core::ServiceCredentialBinding,
    reference: CollectorServiceCredentialRef,
    scope_hash: Hash,
}
/// Not serializable/Clone and no material callback. Always the actual new Permit.
pub(crate) struct OpenedLifetimeCollectorCredentials<'a> {
    entries: Vec<Entry>,
    permit: &'a CollectorServicePermit,
    expires_at: Timestamp,
}
pub(crate) struct RegisteredLifetimeCollectorCredential<'a> {
    reference: CollectorServiceCredentialRef,
    scope_hash: Hash,
    leaf: String,
    expires_at: Timestamp,
    permit: &'a CollectorServicePermit,
}

fn native_authorization(
    value: &Signed<CollectorServiceCredentialAuthorization>,
) -> Result<cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization>> {
    use cc_agent_node_wire::Statement as NativeStatement;
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidWire)?;
    if bytes.len() > 12288 { return Err(Error::Quota); }
    let converted: cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization> =
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    if value.body.signing_bytes()? != converted.body.signing_bytes().map_err(|_| Error::InvalidWire)?
        || value.signature.as_bytes() != converted.signature.as_bytes()
    { return Err(Error::WrongClaim); }
    Ok(converted)
}

impl CollectorCredentialRecipient {
    pub(crate) fn open_original_service_lifetime_set<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        authorization: &Signed<CollectorServiceCredentialAuthorization>,
        capsules: &[cc_crypto_core::ServiceCredentialCapsule],
        installed_catalog: &serde_json::Value, clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedLifetimeCollectorCredentials<'a>> {
        self.open_original_service_lifetime_set_read_phase(actor, permit, authorization, capsules, installed_catalog, None, clock)
    }
    pub(crate) fn open_original_service_lifetime_set_waiting<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        authorization: &Signed<CollectorServiceCredentialAuthorization>,
        capsules: &[cc_crypto_core::ServiceCredentialCapsule],
        installed_catalog: &serde_json::Value, end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedLifetimeCollectorCredentials<'a>> {
        self.open_original_service_lifetime_set_read_phase(actor, permit, authorization, capsules, installed_catalog, Some(end), clock)
    }
    fn open_original_service_lifetime_set_read_phase<'a, S: CompareAndSwap>(
        &mut self, actor: &mut Actor<S>, permit: &'a CollectorServicePermit,
        authorization: &Signed<CollectorServiceCredentialAuthorization>,
        capsules: &[cc_crypto_core::ServiceCredentialCapsule],
        installed_catalog: &serde_json::Value, read_end: Option<std::time::Instant>,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OpenedLifetimeCollectorCredentials<'a>> {
        let (policy, _, pin) = checked_original_read_phase(actor, permit, read_end, clock)?;
        authorization.verify(pin.key)?;
        authorization.body.verify_initial_binding(&pin)?;
        let r = &authorization.body.recipient.body;
        self.inner.check_binding(&binding(&policy, &pin, self.inner.id())?)?;
        if r.recipient_id != self.inner.id() || r.recipient_public_key.as_bytes() != &self.inner.public()? {
            return Err(Error::WrongClaim);
        }
        let native = native_authorization(authorization)?;
        let native_pin = native_pin_metadata(&pin)?;
        let native_base = native_policy_metadata(&policy)?;
        let mut access = borrow_original_read(actor, permit, read_end, clock)?;
        access.matches_original_permit(permit)?;
        if access.policy() != &policy { return Err(Error::Conflict); }
        let now = recheck_original_read(&mut access, clock()?, read_end, clock)?;
        authorization.body.check_original(&policy, access.credential_original_clock(), installed_catalog, now)?;
        // Conversion is exact canonical public metadata, never a Permit cast.
        let native_now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(now.0.seconds())
            .map_err(|_| Error::InvalidTime)?);
        let bindings = cc_crypto_core::preflight_collector_service_credential_set(&native, capsules,
            &native_pin, &native_base, &native.body.admission.body.clock, installed_catalog, native_now)
            .map_err(|_| Error::WrongClaim)?;
        let mut expires_at = authorization.body.expires_at;
        for capsule in capsules {
            expires_at = expires_at.min(Timestamp::new(capsule.access_expires_at().map_err(|_| Error::InvalidWire)?)?);
        }
        access.narrow_expiry(expires_at)?;
        let material = self.inner.with_keys(|keys| {
            let now = recheck_original_read(&mut access, clock()?, read_end, clock)?;
            let native_now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(now.0.seconds())
                .map_err(|_| Error::InvalidTime)?);
            cc_crypto_core::open_collector_service_credential_set(&native, capsules, &native_pin,
                &native_base, &native.body.admission.body.clock, installed_catalog,
                native.body.recipient.body.recipient_id, keys, native_now).map_err(|_| Error::WrongClaim)
        })?;
        if material.len() != bindings.len() || material.len() != authorization.body.credentials.len() {
            return Err(Error::WrongClaim);
        }
        let mut entries = Vec::with_capacity(material.len());
        for (slot, (material, expected)) in material.into_iter().zip(bindings).enumerate() {
            entries.push(Entry { material, expected,
                reference: authorization.body.credentials[slot].clone(),
                scope_hash: authorization.body.capsule_delegation_digest(slot)? });
        }
        let after = recheck_original_read(&mut access, clock()?, read_end, clock)?;
        authorization.body.check_original(&policy, access.credential_original_clock(), installed_catalog, after)?;
        Ok(OpenedLifetimeCollectorCredentials { entries, permit, expires_at })
    }
}

impl<'a> OpenedLifetimeCollectorCredentials<'a> {
    pub(crate) fn targets(&self)->Vec<crate::CollectorCredentialTarget>{ self.entries.iter().map(|e|e.reference.target.clone()).collect() }
    pub(crate) fn write_registered<S: CompareAndSwap>(
        self, writer: &mut RegisteredCredentialWriter<'_, '_, S>,
    ) -> Result<Vec<RegisteredLifetimeCollectorCredential<'a>>> {
        if !writer.matches_original_permit(self.permit) { return Err(Error::WrongState); }
        writer.narrow_expiry(self.expires_at)?;
        writer.recheck()?;
        let mut registered = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            let now = writer.recheck()?;
            let leaf = entry.material.with_secret(&entry.expected, now.0.seconds(), |raw| {
                if !cc_crypto_core::collector_basic_password_supported(raw) { return Err(Error::InvalidWire); }
                writer.write_scope_password(entry.scope_hash, raw).map_err(|_| Error::UncertainCommit)
            }).map_err(|_| Error::WrongClaim)??;
            writer.recheck().map_err(|_| Error::UncertainCommit)?;
            if leaf != format!("credential-{}.password", hex::encode(entry.scope_hash.as_bytes())) {
                return Err(Error::UncertainCommit);
            }
            registered.push(RegisteredLifetimeCollectorCredential { reference: entry.reference,
                scope_hash: entry.scope_hash, leaf, expires_at: self.expires_at, permit: self.permit });
        }
        Ok(registered)
    }
}
impl RegisteredLifetimeCollectorCredential<'_> {
    pub(crate) fn target(&self) -> &crate::CollectorCredentialTarget { &self.reference.target }
    pub(crate) fn scope_hash(&self) -> Hash { self.scope_hash }
    pub(crate) fn expires_at(&self) -> Timestamp { self.expires_at }
    pub(crate) fn private_auth_projection<S: CompareAndSwap>(
        &self, writer: &mut RegisteredCredentialWriter<'_, '_, S>,
    ) -> Result<serde_json::Value> {
        if !writer.matches_original_permit(self.permit) { return Err(Error::WrongState); }
        writer.narrow_expiry(self.expires_at)?;
        writer.recheck()?;
        let result = serde_json::json!({"basic_auth":{"username":self.reference.username,"password_file":self.leaf}});
        writer.recheck()?;
        Ok(result)
    }
}
