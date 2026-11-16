//! LOCAL-only fixture activation. No remote ack, production floor or inspector.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalActivationInput {
    pub previous_certificate: Certificate,
    pub committed_certificate: Certificate,
}

/// Local selection observation only; cannot be deserialized as an authority ack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalActivationPublic {
    pub format: u32,
    pub agent_id: AgentId,
    pub rotation_id: String,
    pub selected_key_epoch: u64,
    pub key_fingerprint: Hash,
    pub certificate_hash: Hash,
    pub local_only: bool,
}

/// Native fixture prototype only. Production delivery floor/remote ack absent.
/// The private acknowledgement cannot be constructed by a public DTO:
/// ```compile_fail
/// use cc_agent_identity_policy::LocalRotationAcknowledgement;
/// ```
#[cfg(target_os = "linux")]
pub fn activate_prepared_rotation(
    state_dir: &std::path::Path,
    authority: &mut Actor<SqliteJournalStore>,
    agent_id: AgentId,
    rotation_id: &str,
    input: LocalActivationInput,
) -> Result<LocalActivationPublic> {
    linux::activate(
        state_dir,
        authority,
        agent_id,
        rotation_id,
        input,
        &mut linux::os_now,
    )
}

#[cfg(target_os = "linux")]
pub(crate) use linux::inspection_phase;
/// CLOSED TunnelOnly local selector. The returned local-only facts are NOT a
/// portable ACK; the fixed worker separately observes actual Actor+selection.
#[cfg(target_os = "linux")]
pub fn activate_prepared_tunnel_rotation(
    state_dir:&std::path::Path,authority:&mut Actor<SqliteJournalStore>,agent_id:AgentId,
    rotation_id:&str,input:LocalActivationInput,
)->Result<LocalActivationPublic> {
    linux::activate_tunnel(state_dir,authority,agent_id,rotation_id,input,&mut linux::os_now)
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{deny_presence, selected, Selection};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::node_prepare::{guard_inode, open_checked, read, Guard};
    use serde::de::DeserializeOwned;
    use sha2::{Digest, Sha256};
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;
    use std::time::{Duration, Instant};

    const INTENT: &str = "node-activation.intent.v3";
    const ACTIVE: &str = "node-activation.state.v3";
    const LIMIT: usize = 4096;
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Intent {
        format: u32,
        kind: String,
        prepared: PreparedNodeKeyPublic,
        previous_certificate_hash: Hash,
        certificate_hash: Hash,
        old_record_hash: Hash,
        prepared_intent_hash: Hash,
        prepared_material_hash: Hash,
        prepared_selector_hash: Hash,
    }
    #[derive(PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Active {
        format: u32,
        kind: String,
        selected_key_epoch: u64,
        intent_hash: Hash,
        prepared: PreparedNodeKeyPublic,
    }
    pub(crate) struct Selection {
        pub(crate) prepared: PreparedNodeKeyPublic,
        pub(crate) certificate_hash: Hash,
        pub(crate) previous_certificate_hash: Hash,
    }
    fn file_hash(bytes: &[u8]) -> Hash {
        Hash::from_bytes(Sha256::digest(bytes).into())
    }
    fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        crate::wire::canonical_json(
            &serde_json::to_value(value).map_err(|_| Error::Storage)?,
            &mut bytes,
        )?;
        if bytes.len() > LIMIT {
            return Err(Error::Quota);
        }
        Ok(bytes)
    }
    fn record<T: DeserializeOwned + Serialize>(guard: &Guard, name: &str) -> Result<T> {
        let bytes = read(&guard.directory().join(name), LIMIT)?;
        let value: T = serde_json::from_slice(&bytes).map_err(|_| Error::UncertainCommit)?;
        if canonical(&value)?.as_slice() != bytes.as_slice() {
            return Err(Error::UncertainCommit);
        }
        Ok(value)
    }
    fn inventory(guard: &Guard) -> Result<(bool, bool)> {
        guard_inode(guard)?;
        let mut found = (false, false);
        for entry in fs::read_dir(guard.directory()).map_err(|_| Error::Storage)? {
            let entry = entry.map_err(|_| Error::Storage)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::Storage)?;
            if name.starts_with("node-activation.") {
                match name {
                    INTENT => found.0 = true,
                    ACTIVE => found.1 = true,
                    _ => return Err(Error::UncertainCommit),
                }
                // Bounded reader verifies regular0600/single-link/inode.
                read(&entry.path(), LIMIT)?;
            }
        }
        Ok(found)
    }
    pub(crate) fn deny_presence(guard: &Guard) -> Result<()> {
        if inventory(guard)? != (false, false) {
            return Err(Error::WrongState);
        }
        Ok(())
    }
    fn prepared_binding(
        guard: &Guard,
        old: &PublicNodeIdentity,
        previous: Hash,
        cert: Hash,
    ) -> Result<Intent> {
        let prepared = crate::node_prepare::check(guard, Some(old))?.ok_or(Error::WrongState)?;
        Ok(Intent {
            format: 3,
            kind: "ActivationIntent".into(),
            prepared,
            previous_certificate_hash: previous,
            certificate_hash: cert,
            old_record_hash: file_hash(&read(&guard.directory().join("node-identity.key"), 148)?),
            prepared_intent_hash: file_hash(&read(
                &guard.directory().join("node-rotation.intent.v2"),
                2048,
            )?),
            prepared_material_hash: file_hash(&read(
                &guard.directory().join("node-rotation.pending.v2"),
                212,
            )?),
            prepared_selector_hash: file_hash(&read(
                &guard.directory().join("node-rotation.state.v2"),
                2048,
            )?),
        })
    }
    fn active(intent: &Intent) -> Result<Active> {
        Ok(Active {
            format: 3,
            kind: "ActiveSelection".into(),
            selected_key_epoch: 2,
            intent_hash: file_hash(&canonical(intent)?),
            prepared: intent.prepared.clone(),
        })
    }
    pub(crate) fn selected(guard: &Guard, old: &PublicNodeIdentity) -> Result<Option<Selection>> {
        match inventory(guard)? {
            (false, false) => Ok(None),
            (true, true) => {
                let intent: Intent = record(guard, INTENT)?;
                if intent
                    != prepared_binding(
                        guard,
                        old,
                        intent.previous_certificate_hash,
                        intent.certificate_hash,
                    )?
                    || record::<Active>(guard, ACTIVE)? != active(&intent)?
                {
                    return Err(Error::UncertainCommit);
                }
                Ok(Some(Selection {
                    prepared: intent.prepared,
                    certificate_hash: intent.certificate_hash,
                    previous_certificate_hash: intent.previous_certificate_hash,
                }))
            }
            _ => Err(Error::UncertainCommit),
        }
    }
    /// Pure structural local facts; no authority/durability acknowledgement.
    pub(crate) fn inspection_phase(
        guard: &Guard,
        old: &PublicNodeIdentity,
    ) -> Result<Option<bool>> {
        match inventory(guard)? {
            (false, false) => Ok(None),
            (true, false) => {
                let intent: Intent = record(guard, INTENT)?;
                if intent
                    != prepared_binding(
                        guard,
                        old,
                        intent.previous_certificate_hash,
                        intent.certificate_hash,
                    )?
                {
                    return Err(Error::UncertainCommit);
                }
                Ok(Some(false))
            }
            (true, true) => {
                selected(guard, old)?.ok_or(Error::UncertainCommit)?;
                Ok(Some(true))
            }
            _ => Err(Error::UncertainCommit),
        }
    }
    pub(super) fn os_now() -> Result<Now> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?
            .as_secs();
        Ok(Now(Timestamp::new(seconds)?))
    }
    struct ExclusiveExistingNodeGuard(Guard);
    // Intentionally private/nonserializable/noncloneable; only produced while
    // borrowing a concrete verified SQLite Actor AND the held exclusive guard.
    struct LocalRotationAcknowledgement<'a, 'g> {
        authority: &'a mut Actor<SqliteJournalStore>,
        guard: &'g ExclusiveExistingNodeGuard,
        certificate: &'a Certificate,
        owner: OwnerPin,
    }
    impl LocalRotationAcknowledgement<'_, '_> {
        fn recheck(&mut self, clock: &mut impl FnMut() -> Result<Now>) -> Result<()> {
            guard_inode(&self.guard.0)?;
            if self
                .authority
                .observe_local_rotation(self.certificate, clock)?
                != self.owner
            {
                return Err(Error::WrongCheckpoint);
            }
            Ok(())
        }
    }
    fn ceilings(
        old: &PublicNodeIdentity,
        intent: &Intent,
        input: &LocalActivationInput,
    ) -> Result<()> {
        ceilings_for_scope(old,intent,input,Scope::TelemetryRead)
    }
    fn ceilings_for_scope(old:&PublicNodeIdentity,intent:&Intent,input:&LocalActivationInput,scope:Scope)->Result<()> {
        if !matches!(scope,Scope::TelemetryRead|Scope::TunnelOpen) { return Err(Error::ScopeDenied); }
        let previous = &input.previous_certificate;
        let cert = &input.committed_certificate;
        if previous.agent_id != old.agent_id
            || cert.agent_id != old.agent_id
            || previous.agent_key != old.public_key
            || cert.agent_key != intent.prepared.new_public_key
        {
            return Err(Error::WrongClaim);
        }
        if previous.key_epoch != 1 || cert.key_epoch != 2 {
            return Err(Error::WrongEpoch);
        }
        match previous.origin {
            CertificateOrigin::Enrollment {
                key_fingerprint, ..
            } if key_fingerprint == old.key_fingerprint => (),
            _ => return Err(Error::WrongClaim),
        }
        match cert.origin {
            CertificateOrigin::Rotation {
                previous_certificate,
                previous_key_epoch: 1,
            } if previous_certificate == previous.digest()? => (),
            _ => return Err(Error::WrongClaim),
        }
        if scope==Scope::TunnelOpen && previous.header.owner_epoch!=cert.header.owner_epoch {return Err(Error::WrongEpoch);}
        if previous.header.context != cert.header.context {
            return Err(Error::WrongContext);
        }
        if previous.capabilities.version != 1
            || previous.scopes.0.as_slice() != [scope]
            || previous.capabilities.scopes != previous.scopes
            || cert.capabilities != previous.capabilities
            || cert.scopes != previous.scopes
        {
            return Err(Error::CapabilityDenied);
        }
        if cert.not_before < previous.not_before || cert.expires_at > previous.expires_at {
            return Err(Error::Expired);
        }
        Ok(())
    }
    fn write_new(guard: &Guard, name: &str, bytes: &[u8]) -> Result<()> {
        guard_inode(guard)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(guard.directory().join(name))
            .map_err(|_| Error::UncertainCommit)?;
        fault("after-create", name)?;
        file.write_all(bytes).map_err(|error| {
            #[cfg(test)]
            tests::io_error(error.raw_os_error());
            #[cfg(not(test))]
            let _ = error;
            Error::UncertainCommit
        })?;
        fault("after-write", name)?;
        file.sync_all().map_err(|_| Error::UncertainCommit)?;
        fault("after-file-sync", name)?;
        File::open(guard.directory())
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::UncertainCommit)?;
        fault("after-dir-sync", name)?;
        if read(&guard.directory().join(name), LIMIT)?.as_slice() != bytes {
            return Err(Error::UncertainCommit);
        }
        fault("after-readback", name)?;
        guard_inode(guard)
    }
    fn resync(guard: &Guard, name: &str) -> Result<()> {
        open_checked(&guard.directory().join(name))?
            .sync_all()
            .map_err(|_| Error::UncertainCommit)?;
        File::open(guard.directory())
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::UncertainCommit)?;
        guard_inode(guard)
    }
    #[cfg(not(test))]
    fn fault(_stage: &str, _name: &str) -> Result<()> {
        Ok(())
    }
    #[cfg(test)]
    fn fault(stage: &str, name: &str) -> Result<()> {
        tests::fault(stage, name)
    }
    pub(super) fn activate(
        directory: &Path,
        authority: &mut Actor<SqliteJournalStore>,
        agent_id: AgentId,
        rotation_id: &str,
        input: LocalActivationInput,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<LocalActivationPublic> {
        activate_for_scope(directory,authority,agent_id,rotation_id,input,clock,Scope::TelemetryRead)
    }
    pub(super) fn activate_tunnel(directory:&Path,authority:&mut Actor<SqliteJournalStore>,agent_id:AgentId,
        rotation_id:&str,input:LocalActivationInput,clock:&mut impl FnMut()->Result<Now>)->Result<LocalActivationPublic> {
        activate_for_scope(directory,authority,agent_id,rotation_id,input,clock,Scope::TunnelOpen)
    }
    fn activate_for_scope(directory:&Path,authority:&mut Actor<SqliteJournalStore>,agent_id:AgentId,
        rotation_id:&str,input:LocalActivationInput,clock:&mut impl FnMut()->Result<Now>,scope:Scope)->Result<LocalActivationPublic> {
        if rotation_id.len() != 32
            || !rotation_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidWire);
        }
        let started = Instant::now();
        let guard = ExclusiveExistingNodeGuard(crate::node_prepare::existing_lock(directory)?);
        let old = crate::provision::load_existing_under_guard(&guard.0, agent_id)?;
        let intent = prepared_binding(
            &guard.0,
            &old,
            input.previous_certificate.digest()?,
            input.committed_certificate.digest()?,
        )?;
        if intent.prepared.rotation_id != rotation_id {
            return Err(Error::WrongClaim);
        }
        if scope==Scope::TelemetryRead {ceilings(&old,&intent,&input)?;}
        else {ceilings_for_scope(&old,&intent,&input,scope)?;}
        let found = inventory(&guard.0)?;
        if found.1 && !found.0 {
            return Err(Error::UncertainCommit);
        }
        if found.0 && record::<Intent>(&guard.0, INTENT)? != intent {
            return Err(Error::UncertainCommit);
        }
        let expected_active = active(&intent)?;
        if found.1 && record::<Active>(&guard.0, ACTIVE)? != expected_active {
            return Err(Error::UncertainCommit);
        }
        let owner = authority.observe_local_rotation(&input.committed_certificate, clock)?;
        let mut ack = LocalRotationAcknowledgement {
            authority,
            guard: &guard,
            certificate: &input.committed_certificate,
            owner,
        };
        ack.recheck(clock)?;
        if found.0 {
            resync(&guard.0, INTENT)?;
        } else {
            write_new(&guard.0, INTENT, &canonical(&intent)?)?;
        }
        ack.recheck(clock)?;
        if started.elapsed() > Duration::from_secs(5) {
            return Err(Error::UncertainCommit);
        }
        // Repeat the exact immutable binding after I/O before publishing selection.
        if record::<Intent>(&guard.0, INTENT)? != intent {
            return Err(Error::UncertainCommit);
        }
        if found.1 {
            resync(&guard.0, ACTIVE)?;
        } else {
            write_new(&guard.0, ACTIVE, &canonical(&expected_active)?)?;
        }
        if selected(&guard.0, &old)?.as_ref().map(|s| &s.prepared) != Some(&intent.prepared) {
            return Err(Error::UncertainCommit);
        }
        ack.recheck(clock)?;
        if started.elapsed() > Duration::from_secs(5) {
            return Err(Error::UncertainCommit);
        }
        Ok(LocalActivationPublic {
            format: 3,
            agent_id,
            rotation_id: intent.prepared.rotation_id,
            selected_key_epoch: 2,
            key_fingerprint: intent.prepared.new_key_fingerprint,
            certificate_hash: intent.certificate_hash,
            local_only: true,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        include!("node_activation_tests.rs");
        include!("node_inspect_tests.rs");
    }
}
