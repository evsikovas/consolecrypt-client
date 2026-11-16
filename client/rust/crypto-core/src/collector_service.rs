//! Only two closed collector-service purposes. No caller byte signing API.
use crate::device::DeviceSecretKeys;
use cc_agent_node_wire::{
    CollectorServiceOwnerSigned, Error, Now, PreparedCollectorServiceOwner, Result,
};

impl DeviceSecretKeys {
    pub fn sign_collector_service_owner_v1(
        &self,
        prepared: PreparedCollectorServiceOwner,
        now: Now,
    ) -> Result<CollectorServiceOwnerSigned> {
        prepared.recheck(now)?;
        if prepared.expected_key().as_bytes() != &self.public_keys().signing {
            return Err(Error::WrongClaim);
        }
        let signature = self.sign_message(&prepared.signing_transcript()?);
        prepared.finish(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_agent_node_wire::*;

    fn time(n: u64) -> Timestamp {
        Timestamp::new(n).unwrap()
    }
    fn signed<T: Statement>(key: &DeviceSecretKeys, body: T) -> Signed<T> {
        Signed {
            signature: SignatureBytes::from_bytes(key.sign_message(&body.signing_bytes().unwrap())),
            body,
        }
    }
    fn fixture() -> (
        DeviceSecretKeys,
        OwnerPin,
        Signed<Certificate>,
        CollectorServiceDelegation,
    ) {
        let owner = DeviceSecretKeys::from_test_seeds(&[11; 32], &[12; 32]);
        let node = DeviceSecretKeys::from_test_seeds(&[21; 32], &[22; 32]);
        let node_key = PublicKey::from_bytes(node.public_keys().signing);
        let context: Context = serde_json::from_value(serde_json::json!({
            "tenant":{"kind":"localProfile","id":"11111111-1111-4111-8111-111111111111"},
            "deploymentId":"22222222-2222-4222-8222-222222222222"
        }))
        .unwrap();
        let pin = OwnerPin {
            context: context.clone(),
            key: PublicKey::from_bytes(owner.public_keys().signing),
            epoch: 1,
            checkpoint: 2,
            expires_at: time(100000),
        };
        let header = Header {
            version: 1,
            context,
            owner_epoch: 1,
            checkpoint: 2,
        };
        let cert = signed(
            &owner,
            Certificate {
                header: header.clone(),
                agent_id: AgentId::new("33333333-3333-4333-8333-333333333333").unwrap(),
                key_epoch: 1,
                agent_key: node_key,
                scopes: Scopes(vec![Scope::MetricsCollect]),
                capabilities: Capabilities {
                    version: 1,
                    scopes: Scopes(vec![Scope::MetricsCollect]),
                },
                origin: CertificateOrigin::Enrollment {
                    invitation_id: InvitationId::new("44444444-4444-4444-8444-444444444444")
                        .unwrap(),
                    claim_hash: Hash::from_bytes([8; 32]),
                    key_fingerprint: node_key.fingerprint(),
                },
                not_before: time(1000),
                expires_at: time(100000),
            },
        );
        let manifest = Hash::from_bytes([9; 32]);
        let policy = CollectorServiceDelegation {
            header,
            service_id: ResourceId::new("55555555-5555-4555-8555-555555555555").unwrap(),
            policy_revision: 1,
            revocation_epoch: 1,
            agent_id: cert.body.agent_id,
            key_epoch: 1,
            recipient: node_key,
            certificate_hash: cert.body.digest().unwrap(),
            collector_id: collector_service_resource(manifest).unwrap(),
            manifest_sha256: manifest,
            profile: CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,
            quota: CollectorServiceQuota::fixed(),
            credential_targets:Vec::new(),
            issued_at: time(1000),
            expires_at: time(87400),
        };
        (owner, pin, cert, policy)
    }
    #[test]
    fn actual_device_signer_supports_only_finite_typed_metrics_service() {
        let (owner, pin, cert, policy) = fixture();
        let prepared =
            PreparedCollectorServiceOwner::delegation(&pin, &cert, policy.clone(), Now(time(1000)))
                .unwrap();
        let CollectorServiceOwnerSigned::Delegation(signed) = owner
            .sign_collector_service_owner_v1(prepared, Now(time(1000)))
            .unwrap()
        else {
            panic!("wrong closed purpose")
        };
        verify_statement(&signed, pin.key).unwrap();
        assert_eq!(signed.body, policy);
        let domain = b"metrics-collector-service-delegation-v1";
        assert!(signed
            .body
            .signing_bytes()
            .unwrap()
            .windows(domain.len())
            .any(|w| w == domain));
        assert_eq!(LEASE_TTL, 300);
    }
    #[test]
    fn original_prepared_deadline_and_actual_device_key_are_required_at_sign() {
        let (owner, pin, cert, policy) = fixture();
        let prepared =
            PreparedCollectorServiceOwner::delegation(&pin, &cert, policy.clone(), Now(time(1000)))
                .unwrap();
        assert!(owner
            .sign_collector_service_owner_v1(prepared, Now(time(1060)))
            .is_err());
        let prepared =
            PreparedCollectorServiceOwner::delegation(&pin, &cert, policy, Now(time(1000)))
                .unwrap();
        let foreign = DeviceSecretKeys::from_test_seeds(&[31; 32], &[32; 32]);
        assert!(foreign
            .sign_collector_service_owner_v1(prepared, Now(time(1000)))
            .is_err());
    }
    #[test]
    fn actual_device_revoke_signature_has_separate_original_policy_binding() {
        let (owner, pin, cert, policy) = fixture();
        let prepared =
            PreparedCollectorServiceOwner::delegation(&pin, &cert, policy, Now(time(1000)))
                .unwrap();
        let CollectorServiceOwnerSigned::Delegation(original) = owner
            .sign_collector_service_owner_v1(prepared, Now(time(1000)))
            .unwrap()
        else {
            panic!("wrong closed purpose")
        };
        let revoke = CollectorServiceRevocation {
            header: original.body.header.clone(),
            service_id: original.body.service_id,
            delegation_hash: original.body.digest().unwrap(),
            policy_revision: 1,
            revocation_epoch: 2,
            issued_at: time(1001),
        };
        let prepared =
            PreparedCollectorServiceOwner::revocation(&pin, &original, revoke, Now(time(1001)))
                .unwrap();
        let CollectorServiceOwnerSigned::Revocation(signed) = owner
            .sign_collector_service_owner_v1(prepared, Now(time(1001)))
            .unwrap()
        else {
            panic!("wrong closed purpose")
        };
        verify_statement(&signed, pin.key).unwrap();
        let forged = Signed {
            body: signed.body,
            signature: original.signature,
        };
        assert!(verify_statement(&forged, pin.key).is_err());
    }
}
