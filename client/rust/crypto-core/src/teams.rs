//! Teams (protocol 1.9): per-team member identity, user envelopes, the
//! signed membership log, the member code and the key-epoch chain.
//!
//! Trust model: a client accepts a member, role or key only from a log it
//! verified itself from genesis ([`TeamLogView::verify`]); it accepts the
//! team VRK only from a user envelope sealed to its own per-team key, and
//! older VRKs only through chain links opened with a newer one.

use crate::aead;
use crate::device::{random_x25519_secret, x25519, x25519_public, DeviceSecretKeys};
use crate::envelope::device_envelope_key;
use crate::error::{CryptoError, Result};
use crate::keys::Vrk;
use crate::rng::random_array;
use cc_protocol::canonical::envelope_aad;
use cc_protocol::envelopes::{
    EnvelopeAlgorithm, EnvelopeKind, EnvelopeMetadata, NewEnvelope, RecipientType,
};
use cc_protocol::limits::NONCE_LEN;
use cc_protocol::teams::{
    team_key_chain_aad, team_log_hash_input, team_log_message, team_member_code_input,
    SignedTeamLogEntry, TeamKeyChainLink, TeamLogEntry, TeamLogOp, TeamMemberKeys, TEAM_LOG_FORMAT,
};
use cc_protocol::vaults::VaultRole;
use cc_protocol::{Bytes, UserId, VaultId};
use ed25519_dalek::{Signature, VerifyingKey};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use zeroize::Zeroizing;

/// Recipient code of user envelopes in the envelope AAD (ADR-0002).
const RECIPIENT_CODE_USER: u8 = crate::envelope::RECIPIENT_CODE_USER;

/// A member's secret per-team identity (X25519 + Ed25519). Stored only
/// encrypted, inside the member's personal vault.
pub struct TeamIdentity {
    keys: DeviceSecretKeys,
}

impl fmt::Debug for TeamIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TeamIdentity(<redacted>)")
    }
}

impl TeamIdentity {
    /// Fresh keys from the OS CSPRNG.
    pub fn generate() -> Result<Self> {
        Ok(Self {
            keys: DeviceSecretKeys::generate()?,
        })
    }

    /// Serialized secret (zeroizing): same layout as device keys.
    pub fn to_secret_bytes(&self) -> Zeroizing<Vec<u8>> {
        self.keys.to_secret_bytes()
    }

    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            keys: DeviceSecretKeys::from_secret_bytes(bytes)?,
        })
    }

    /// The public keys pinned in the log for `user_id`.
    pub fn member_keys(&self, user_id: UserId) -> TeamMemberKeys {
        let p = self.keys.public_keys();
        TeamMemberKeys {
            user_id,
            encryption_key: p.encryption_bytes(),
            signing_key: p.signing_bytes(),
        }
    }

    /// The Ed25519 public key.
    pub fn signing_key(&self) -> [u8; 32] {
        self.keys.public_keys().signing
    }

    /// Build and sign a log entry for `vault_id` after `head` (seq, hash).
    pub fn sign_entry(
        &self,
        vault_id: VaultId,
        actor: UserId,
        head: Option<(u64, [u8; 32])>,
        issued_at: i64,
        op: TeamLogOp,
    ) -> Result<SignedTeamLogEntry> {
        let (seq, previous_hash) = match head {
            None => (0, [0u8; 32]),
            Some((seq, hash)) => (seq + 1, hash),
        };
        let entry = TeamLogEntry {
            format: TEAM_LOG_FORMAT,
            vault_id,
            seq,
            previous_hash: Bytes::new(previous_hash.to_vec()),
            issued_at,
            actor_user_id: actor,
            actor_signing_key: Bytes::new(self.signing_key().to_vec()),
            op,
        };
        let signed = SignedTeamLogEntry {
            signature: Bytes::new(self.keys.sign_message(&team_log_message(&entry)).to_vec()),
            entry,
        };
        signed
            .validate()
            .map_err(|_| CryptoError::TeamLog("entry is not well-formed"))?;
        Ok(signed)
    }

    /// Open the team VRK from this member's user envelope.
    pub fn open_user_envelope(
        &self,
        envelope: &NewEnvelope,
        vault_id: VaultId,
        user_id: UserId,
    ) -> Result<Vrk> {
        envelope.validate()?;
        if envelope.recipient_type != RecipientType::User {
            return Err(CryptoError::WrongRecipientType {
                expected: RecipientType::User,
                actual: envelope.recipient_type,
            });
        }
        if envelope.recipient_id != Some(user_id.0) {
            return Err(CryptoError::WrongRecipientDevice);
        }
        let nonce: [u8; NONCE_LEN] = envelope
            .nonce
            .to_array()
            .ok_or(CryptoError::InvalidKey { what: "nonce" })?;
        let ephemeral_public: [u8; 32] = envelope
            .metadata
            .ephemeral_public_key
            .as_ref()
            .and_then(|k| k.to_array())
            .ok_or(CryptoError::InvalidKey {
                what: "ephemeral public key",
            })?;
        let shared = self.keys.diffie_hellman(&ephemeral_public)?;
        let own = self.keys.public_keys().encryption;
        let key = device_envelope_key(&shared, &ephemeral_public, &own, vault_id);
        let aad = envelope_aad(vault_id, RECIPIENT_CODE_USER, Some(user_id.as_bytes()));
        aead::open_key32(
            key.expose_secret(),
            &nonce,
            &aad,
            envelope.ciphertext.as_slice(),
        )
        .map(Vrk::from_secret)
    }
}

/// Seal `vrk` to a member's per-team encryption key (`recipient_type =
/// user`). Only for keys taken from a verified log.
pub fn seal_user_envelope(
    vrk: &Vrk,
    vault_id: VaultId,
    member: &TeamMemberKeys,
) -> Result<NewEnvelope> {
    let recipient: [u8; 32] = member
        .encryption_key
        .to_array()
        .ok_or(CryptoError::InvalidKey {
            what: "member encryption key",
        })?;
    let ephemeral = random_x25519_secret()?;
    let nonce = random_array::<NONCE_LEN>()?;
    let ephemeral_public = x25519_public(&ephemeral);
    let shared = x25519(&ephemeral, &recipient)?;
    let key = device_envelope_key(&shared, &ephemeral_public, &recipient, vault_id);
    let aad = envelope_aad(
        vault_id,
        RECIPIENT_CODE_USER,
        Some(member.user_id.as_bytes()),
    );
    let ciphertext = aead::seal(key.expose_secret(), &nonce, &aad, vrk.expose())?;
    Ok(NewEnvelope {
        recipient_type: RecipientType::User,
        recipient_id: Some(member.user_id.0),
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::X25519HkdfSha256Xchacha20poly1305V1,
            kdf: None,
            ephemeral_public_key: Some(Bytes::new(ephemeral_public.to_vec())),
        },
        ciphertext: Bytes::new(ciphertext),
        nonce: Bytes::new(nonce.to_vec()),
    })
}

/// `SHA-256(team_log_hash_input(..))`.
pub fn team_entry_hash(signed: &SignedTeamLogEntry) -> [u8; 32] {
    Sha256::digest(team_log_hash_input(signed)).into()
}

/// The code the inviting admin and the joining member compare (6 groups of
/// 5 digits, like the device code).
pub fn team_member_code(
    vault_id: VaultId,
    admin_signing_key: &[u8; 32],
    member: &TeamMemberKeys,
) -> crate::device::VerificationCode {
    let h: [u8; 32] =
        Sha256::digest(team_member_code_input(vault_id, admin_signing_key, member)).into();
    crate::device::VerificationCode::from_hash(&h)
}

/// Seal the previous epoch's VRK under the new one (chain link).
pub fn seal_key_chain_link(
    new_vrk: &Vrk,
    old_vrk: &Vrk,
    vault_id: VaultId,
    key_epoch: u32,
) -> Result<TeamKeyChainLink> {
    let nonce = random_array::<NONCE_LEN>()?;
    let aad = team_key_chain_aad(vault_id, key_epoch);
    let ciphertext = aead::seal(new_vrk.expose(), &nonce, &aad, old_vrk.expose())?;
    Ok(TeamKeyChainLink {
        key_epoch,
        nonce: Bytes::new(nonce.to_vec()),
        ciphertext: Bytes::new(ciphertext),
    })
}

/// Open the previous epoch's VRK with the VRK of `link.key_epoch`.
pub fn open_key_chain_link(
    new_vrk: &Vrk,
    link: &TeamKeyChainLink,
    vault_id: VaultId,
) -> Result<Vrk> {
    link.validate()
        .map_err(|_| CryptoError::TeamLog("malformed key chain link"))?;
    let nonce: [u8; NONCE_LEN] = link
        .nonce
        .to_array()
        .ok_or(CryptoError::InvalidKey { what: "nonce" })?;
    let aad = team_key_chain_aad(vault_id, link.key_epoch);
    aead::open_key32(new_vrk.expose(), &nonce, &aad, link.ciphertext.as_slice())
        .map(Vrk::from_secret)
}

// ---- log verification -----------------------------------------------------------------

/// A member as the verified log defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMember {
    pub role: VaultRole,
    pub keys: TeamMemberKeys,
}

/// The team state after verifying its log from genesis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamLogView {
    pub vault_id: VaultId,
    pub seq: u64,
    pub head: [u8; 32],
    pub key_epoch: u32,
    pub members: BTreeMap<UserId, VerifiedMember>,
}

fn reject(why: &'static str) -> CryptoError {
    CryptoError::TeamLog(why)
}

fn verify_signature(key: &[u8], signed: &SignedTeamLogEntry) -> Result<()> {
    let key: [u8; 32] = key.try_into().map_err(|_| reject("bad signing key"))?;
    let vk = VerifyingKey::from_bytes(&key).map_err(|_| reject("bad signing key"))?;
    let sig = Signature::from_slice(signed.signature.as_slice())
        .map_err(|_| CryptoError::InvalidSignature)?;
    vk.verify_strict(&team_log_message(&signed.entry), &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

fn admin(role: VaultRole) -> bool {
    matches!(role, VaultRole::Owner | VaultRole::Admin)
}

impl TeamLogView {
    /// Verify a complete log (genesis first) for `vault_id`.
    pub fn verify(vault_id: VaultId, entries: &[SignedTeamLogEntry]) -> Result<Self> {
        let (first, rest) = entries.split_first().ok_or(reject("empty log"))?;
        let mut view = Self::genesis(vault_id, first)?;
        for e in rest {
            view.apply(e)?;
        }
        Ok(view)
    }

    fn genesis(vault_id: VaultId, signed: &SignedTeamLogEntry) -> Result<Self> {
        signed.validate().map_err(|_| reject("malformed genesis"))?;
        let e = &signed.entry;
        let TeamLogOp::Genesis { owner } = &e.op else {
            return Err(reject("the log does not start with genesis"));
        };
        if e.vault_id != vault_id {
            return Err(reject("genesis of another team"));
        }
        verify_signature(e.actor_signing_key.as_slice(), signed)?;
        let mut members = BTreeMap::new();
        members.insert(
            owner.user_id,
            VerifiedMember {
                role: VaultRole::Owner,
                keys: owner.clone(),
            },
        );
        Ok(Self {
            vault_id,
            seq: 0,
            head: team_entry_hash(signed),
            key_epoch: 0,
            members,
        })
    }

    /// Apply the next entry (same rules as the server, enforced locally).
    pub fn apply(&mut self, signed: &SignedTeamLogEntry) -> Result<()> {
        signed.validate().map_err(|_| reject("malformed entry"))?;
        let e = &signed.entry;
        if e.vault_id != self.vault_id {
            return Err(reject("entry of another team"));
        }
        if e.seq != self.seq + 1 || e.previous_hash.as_slice() != self.head {
            return Err(reject("the log is not a chain"));
        }
        let actor = self
            .members
            .get(&e.actor_user_id)
            .ok_or(reject("entry signed by a non-member"))?
            .clone();
        if actor.keys.signing_key != e.actor_signing_key {
            return Err(reject("entry signed with a key the log did not pin"));
        }
        verify_signature(actor.keys.signing_key.as_slice(), signed)?;
        let role_of = |m: &BTreeMap<UserId, VerifiedMember>, u: &UserId| m.get(u).map(|x| x.role);
        match &e.op {
            TeamLogOp::Genesis { .. } => return Err(reject("second genesis")),
            TeamLogOp::AddMember { member, role } => {
                if !admin(actor.role) || self.members.contains_key(&member.user_id) {
                    return Err(reject("unauthorized add_member"));
                }
                if *role == VaultRole::Admin && actor.role != VaultRole::Owner {
                    return Err(reject("only the owner adds admins"));
                }
                self.members.insert(
                    member.user_id,
                    VerifiedMember {
                        role: *role,
                        keys: member.clone(),
                    },
                );
            }
            TeamLogOp::ChangeRole { user_id, role } => {
                let target = role_of(&self.members, user_id).ok_or(reject("unknown member"))?;
                let allowed = admin(actor.role)
                    && target != VaultRole::Owner
                    && (target != VaultRole::Admin || actor.role == VaultRole::Owner)
                    && (*role != VaultRole::Admin || actor.role == VaultRole::Owner);
                if !allowed {
                    return Err(reject("unauthorized change_role"));
                }
                if let Some(m) = self.members.get_mut(user_id) {
                    m.role = *role;
                }
            }
            TeamLogOp::RemoveMember { user_id } => {
                let target = role_of(&self.members, user_id).ok_or(reject("unknown member"))?;
                let allowed = admin(actor.role)
                    && target != VaultRole::Owner
                    && (target != VaultRole::Admin || actor.role == VaultRole::Owner);
                if !allowed {
                    return Err(reject("unauthorized remove_member"));
                }
                self.members.remove(user_id);
            }
            TeamLogOp::Leave => {
                if actor.role == VaultRole::Owner {
                    return Err(reject("the owner cannot leave"));
                }
                self.members.remove(&e.actor_user_id);
            }
            TeamLogOp::TransferOwner { user_id } => {
                if actor.role != VaultRole::Owner
                    || *user_id == e.actor_user_id
                    || !self.members.contains_key(user_id)
                {
                    return Err(reject("unauthorized transfer_owner"));
                }
                if let Some(m) = self.members.get_mut(&e.actor_user_id) {
                    m.role = VaultRole::Admin;
                }
                if let Some(m) = self.members.get_mut(user_id) {
                    m.role = VaultRole::Owner;
                }
            }
            TeamLogOp::RotateKey { key_epoch } => {
                if !admin(actor.role) || *key_epoch != self.key_epoch + 1 {
                    return Err(reject("unauthorized rotate_key"));
                }
                self.key_epoch = *key_epoch;
            }
        }
        self.seq = e.seq;
        self.head = team_entry_hash(signed);
        Ok(())
    }

    /// Whether a previously verified head (`seq`, `hash`) is part of this
    /// log: a shorter or different log means the server rolled it back.
    pub fn extends(&self, entries: &[SignedTeamLogEntry], seq: u64, hash: &[u8; 32]) -> bool {
        seq <= self.seq
            && entries
                .iter()
                .find(|e| e.entry.seq == seq)
                .is_some_and(|e| team_entry_hash(e) == *hash)
    }

    pub fn role_of(&self, user: UserId) -> Option<VaultRole> {
        self.members.get(&user).map(|m| m.role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> UserId {
        UserId::new()
    }

    #[test]
    fn user_envelope_roundtrip_and_binding() {
        let vault = VaultId::new();
        let alice = user();
        let id = TeamIdentity::generate().unwrap();
        let vrk = Vrk::generate().unwrap();
        let env = seal_user_envelope(&vrk, vault, &id.member_keys(alice)).unwrap();
        env.validate().unwrap();
        let back = id.open_user_envelope(&env, vault, alice).unwrap();
        assert_eq!(back.expose(), vrk.expose());
        assert!(id.open_user_envelope(&env, VaultId::new(), alice).is_err());
        assert!(id.open_user_envelope(&env, vault, user()).is_err());
        let other = TeamIdentity::generate().unwrap();
        assert!(other.open_user_envelope(&env, vault, alice).is_err());
        let restored = TeamIdentity::from_secret_bytes(&id.to_secret_bytes()).unwrap();
        assert!(restored.open_user_envelope(&env, vault, alice).is_ok());
    }

    #[test]
    fn log_verification_follows_the_rules() {
        let vault = VaultId::new();
        let (a, b, c) = (user(), user(), user());
        let (ka, kb, kc) = (
            TeamIdentity::generate().unwrap(),
            TeamIdentity::generate().unwrap(),
            TeamIdentity::generate().unwrap(),
        );
        let t = 1_800_000_000;
        let g = ka
            .sign_entry(
                vault,
                a,
                None,
                t,
                TeamLogOp::Genesis {
                    owner: ka.member_keys(a),
                },
            )
            .unwrap();
        let mut log = vec![g];
        let head = |log: &Vec<SignedTeamLogEntry>| {
            let l = log.last().unwrap();
            Some((l.entry.seq, team_entry_hash(l)))
        };
        log.push(
            ka.sign_entry(
                vault,
                a,
                head(&log),
                t,
                TeamLogOp::AddMember {
                    member: kb.member_keys(b),
                    role: VaultRole::Editor,
                },
            )
            .unwrap(),
        );
        let view = TeamLogView::verify(vault, &log).unwrap();
        assert_eq!(view.role_of(b), Some(VaultRole::Editor));

        // An editor cannot add members; a forged signature fails; a server
        // inserting an unsigned member is rejected.
        let mut bad = log.clone();
        bad.push(
            kb.sign_entry(
                vault,
                b,
                head(&log),
                t,
                TeamLogOp::AddMember {
                    member: kc.member_keys(c),
                    role: VaultRole::Viewer,
                },
            )
            .unwrap(),
        );
        assert!(TeamLogView::verify(vault, &bad).is_err());
        let mut forged = log.clone();
        let mut e = ka
            .sign_entry(
                vault,
                a,
                head(&log),
                t,
                TeamLogOp::RotateKey { key_epoch: 1 },
            )
            .unwrap();
        e.entry.op = TeamLogOp::RotateKey { key_epoch: 1 };
        e.entry.actor_user_id = b;
        forged.push(e);
        assert!(TeamLogView::verify(vault, &forged).is_err());
        // Key substitution: a member key not pinned by the log.
        let mut swapped = log.clone();
        let mut add = ka
            .sign_entry(
                vault,
                a,
                head(&log),
                t,
                TeamLogOp::AddMember {
                    member: kc.member_keys(c),
                    role: VaultRole::Viewer,
                },
            )
            .unwrap();
        if let TeamLogOp::AddMember { member, .. } = &mut add.entry.op {
            member.encryption_key = Bytes::new(vec![9; 32]);
        }
        swapped.push(add);
        assert!(
            TeamLogView::verify(vault, &swapped).is_err(),
            "signature covers keys"
        );

        // Rotation, removal and rollback detection.
        let mut full = log.clone();
        full.push(
            ka.sign_entry(
                vault,
                a,
                head(&full),
                t,
                TeamLogOp::RemoveMember { user_id: b },
            )
            .unwrap(),
        );
        full.push(
            ka.sign_entry(
                vault,
                a,
                head(&full),
                t,
                TeamLogOp::RotateKey { key_epoch: 1 },
            )
            .unwrap(),
        );
        let v = TeamLogView::verify(vault, &full).unwrap();
        assert_eq!(v.key_epoch, 1);
        assert!(v.role_of(b).is_none());
        let pinned = (view.seq, view.head);
        assert!(v.extends(&full, pinned.0, &pinned.1));
        // The owner cannot sign a leave.
        assert!(ka
            .sign_entry(vault, a, head(&log), t, TeamLogOp::Leave)
            .is_ok());
        let mut owner_leaves = log.clone();
        owner_leaves.push(
            ka.sign_entry(vault, a, head(&log), t, TeamLogOp::Leave)
                .unwrap(),
        );
        assert!(TeamLogView::verify(vault, &owner_leaves).is_err());
        let short = TeamLogView::verify(vault, &log[..1]).unwrap();
        assert!(
            !short.extends(&log[..1], pinned.0, &pinned.1),
            "rolled back"
        );
    }

    #[test]
    fn member_code_and_key_chain() {
        let vault = VaultId::new();
        let id = TeamIdentity::generate().unwrap();
        let m = id.member_keys(user());
        let c1 = team_member_code(vault, &[1; 32], &m);
        assert_eq!(c1, team_member_code(vault, &[1; 32], &m));
        assert_ne!(c1, team_member_code(vault, &[2; 32], &m));
        let (old, new) = (Vrk::generate().unwrap(), Vrk::generate().unwrap());
        let link = seal_key_chain_link(&new, &old, vault, 1).unwrap();
        link.validate().unwrap();
        assert_eq!(
            open_key_chain_link(&new, &link, vault).unwrap().expose(),
            old.expose()
        );
        assert!(open_key_chain_link(&old, &link, vault).is_err());
        assert!(open_key_chain_link(&new, &link, VaultId::new()).is_err());
    }
}
