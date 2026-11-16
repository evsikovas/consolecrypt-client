use super::*;
fn uid() -> String {
    Uuid::new_v4().to_string()
}
fn policy(_owner: &DeviceSecretKeys) -> ServiceDelegationPolicy {
    let service = DeviceSecretKeys::generate().unwrap();
    let host_id = uid();
    ServiceDelegationPolicy {
        version: 4,
        id: uid(),
        owner_id: uid(),
        owner_epoch: 1,
        tenant: ServicePolicyTenant {
            kind: ServiceTenantKind::LocalProfile,
            id: uid(),
        },
        service_id: uid(),
        signer: ServicePolicyKey {
            kind: "ed25519".into(),
            public_key: hex(&service.public_keys().signing),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        recipient: ServicePolicyKey {
            kind: "x25519".into(),
            public_key: hex(&service.public_keys().encryption),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        project_id: uid(),
        project_revision: 1,
        template_digest: "a".repeat(64),
        playbook: "roles/плейбук.yml".into(),
        schedule_id: uid(),
        schedule_revision: 1,
        definition_digest: "b".repeat(64),
        inventory_digest: "c".repeat(64),
        parameters_digest: "d".repeat(64),
        environment_digest: "e".repeat(64),
        hosts: vec![ServicePolicyHostRef {
            id: host_id.clone(),
            revision: 2,
            target_digest: "f".repeat(64),
        }],
        credentials: vec![],
        route_inputs: ServiceRouteInputs {
            dependencies: vec![ServiceRouteInputRef {
                id: host_id,
                revision: 2,
                kind: cc_models::ObjectKind::Host,
            }],
            key_trust: vec![ServiceRouteInputRef {
                id: uid(),
                revision: 1,
                kind: cc_models::ObjectKind::KnownHost,
            }],
        },
        modes: vec!["check".into()],
        quotas: ServicePolicyQuotas {
            max_jobs: 16,
            max_per_utc_day: 4,
            max_concurrent: 1,
            max_job_seconds: 120,
            max_forks: 1,
        },
        policy_revision: 1,
        revocation_epoch: 1,
        issued_at: 100,
        expires_at: 3700,
        desktop_lock_policy: "independentDelegatedService".into(),
        execution_origin: None,
        access_policy: ServiceAccessPolicy {
            mode: ServiceAccessMode::FreshOwnerHead,
            offline_not_after: None,
        },
    }
}

#[test]
fn service_origin_and_fixed_scheduled_signer_bind_actual_policy_targets_and_deadlines() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let service = DeviceSecretKeys::generate().unwrap();
    let mut p = policy(&owner);
    let public = service.public_keys();
    p.signer.public_key = hex(&public.signing);
    p.recipient.public_key = hex(&public.encryption);
    p.access_policy = ServiceAccessPolicy {
        mode: ServiceAccessMode::BoundedOffline,
        offline_not_after: Some(p.expires_at),
    };
    let target = cc_models::automation::ApprovedTarget {
        host_id: p.hosts[0].id.parse().unwrap(),
        alias: "owned".into(),
        address: "192.0.2.1".into(),
        port: 22,
        username: "runner".into(),
        route_digest: "a".repeat(64),
        credential_refs: vec![],
    };
    p.hosts[0] = ServicePolicyHostRef::from_target(&target, 2).unwrap();
    p.execution_origin = Some(crate::ServiceExecutionOrigin {
        version: 1,
        service_id: p.service_id.clone(),
        runner_revision: 1,
        runner_configuration_digest: "1".repeat(64),
        signer_key_sha256: hex(&Sha256::digest(public.signing)),
        recipient_key_sha256: hex(&Sha256::digest(public.encryption)),
        environment_digest: p.environment_digest.clone(),
        helper_digest: "2".repeat(64),
        known_hosts_digest: "3".repeat(64),
        backend: "linuxNativeSsh".into(),
        network: "serviceLocal".into(),
    });
    let signed = owner
        .sign_service_delegation(p.clone(), &p.owner_id, &p.tenant, 100)
        .unwrap();
    let verified = SignedServiceDelegation::verify_json(
        &signed.to_json().unwrap(),
        &p.owner_id,
        &owner.public_keys(),
        &p.tenant,
        101,
    )
    .unwrap();
    let slot = crate::ServiceScheduledSlot {
        schedule_id: p.schedule_id.parse().unwrap(),
        schedule_revision: p.schedule_revision,
        definition_digest: p.definition_digest.clone(),
        local_slot: "2026-11-13T12:00".into(),
        due_utc: 100,
        grace_seconds: 60,
    };
    let first = service
        .sign_service_scheduled_job(
            &verified,
            &slot,
            vec![target.clone()],
            cc_models::automation::JobMode::Check,
            150,
            101,
        )
        .unwrap();
    let second = service
        .sign_service_scheduled_job(
            &verified,
            &slot,
            vec![target.clone()],
            cc_models::automation::JobMode::Check,
            150,
            102,
        )
        .unwrap();
    assert_eq!(
        first.approval.manifest.job_id,
        second.approval.manifest.job_id
    );
    assert_ne!(
        first.approval.manifest.nonce,
        second.approval.manifest.nonce
    );
    assert_eq!(first.approval.manifest.access_expires_at, 150);
    assert_eq!(first.approval.manifest.policy.timeout_seconds, 49);
    let signature: [u8; 64] = STANDARD
        .decode(&first.approval.signature)
        .unwrap()
        .try_into()
        .unwrap();
    crate::verify_automation_manifest(&public.signing, &first.approval.manifest, &signature)
        .unwrap();
    assert!(
        service
            .sign_service_scheduled_job(
                &verified,
                &slot,
                vec![target.clone()],
                cc_models::automation::JobMode::Apply,
                150,
                101
            )
            .is_err()
    );
    assert!(
        service
            .sign_service_scheduled_job(
                &verified,
                &slot,
                vec![target.clone()],
                cc_models::automation::JobMode::Check,
                101,
                101
            )
            .is_err()
    );
    let mut changed = target.clone();
    changed.address = "192.0.2.99".into();
    assert!(
        service
            .sign_service_scheduled_job(
                &verified,
                &slot,
                vec![changed],
                cc_models::automation::JobMode::Check,
                150,
                101
            )
            .is_err()
    );
    assert!(
        owner
            .sign_service_scheduled_job(
                &verified,
                &slot,
                vec![target],
                cc_models::automation::JobMode::Check,
                150,
                101
            )
            .is_err()
    );
    let mut mutated: Value = serde_json::from_slice(&signed.to_json().unwrap()).unwrap();
    mutated["body"]["execution_origin"]["runner_revision"] = serde_json::json!(2);
    assert!(
        SignedServiceDelegation::verify_json(
            &serde_json::to_vec(&mutated).unwrap(),
            &p.owner_id,
            &owner.public_keys(),
            &p.tenant,
            101
        )
        .is_err()
    );
}
#[test]
fn actual_owner_signature_roundtrip_binds_exact_selected_playbook_and_typed_tenant() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let id = body.owner_id.clone();
    let tenant = body.tenant.clone();
    let signed = owner
        .sign_service_delegation(body, &id, &tenant, 100)
        .unwrap();
    let raw = signed.to_json().unwrap();
    let verified =
        SignedServiceDelegation::verify_json(&raw, &id, &owner.public_keys(), &tenant, 101)
            .unwrap();
    assert_eq!(verified.body().playbook, "roles/плейбук.yml");
    assert_eq!(signed.digest().unwrap(), verified.digest().unwrap());
    let mut changed: Value = serde_json::from_slice(&raw).unwrap();
    changed["body"]["playbook"] = Value::String("roles/other.yml".into());
    assert!(
        SignedServiceDelegation::verify_json(
            &serde_json::to_vec(&changed).unwrap(),
            &id,
            &owner.public_keys(),
            &tenant,
            101
        )
        .is_err()
    );
    let mut other = tenant.clone();
    other.kind = ServiceTenantKind::PersonalAccount;
    assert!(
        SignedServiceDelegation::verify_json(&raw, &id, &owner.public_keys(), &other, 101).is_err()
    );
}
#[test]
fn old_delegation_domain_and_other_owner_key_are_not_reusable() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let id = body.owner_id.clone();
    let tenant = body.tenant.clone();
    let mut old = b"ConsoleCrypt ServiceDelegation v1\0".to_vec();
    old.extend(body.canonical_bytes().unwrap());
    let signed = SignedServiceDelegation {
        body,
        signature: STANDARD.encode(owner.sign_message(&old)),
    };
    assert!(
        SignedServiceDelegation::verify_json(
            &signed.to_json().unwrap(),
            &id,
            &owner.public_keys(),
            &tenant,
            100
        )
        .is_err()
    );
    let other = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    assert!(
        SignedServiceDelegation::verify_json(
            &signed.to_json().unwrap(),
            &body.owner_id,
            &other.public_keys(),
            &body.tenant,
            100
        )
        .is_err()
    );
}
#[test]
fn schema_versions_duplicates_unknown_and_oversized_frames_are_rejected() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    let raw = String::from_utf8(signed.to_json().unwrap()).unwrap();
    for text in [
        raw.replacen("\"version\":4", "\"version\":1", 1),
        raw.replacen("\"version\":4", "\"version\":3", 1),
        raw.replacen("\"version\":4", "\"version\":4,\"version\":4", 1),
        raw.replacen("\"version\":4", "\"version\":4,\"approved\":true", 1),
        format!("{raw}{}", " ".repeat(MAX_WIRE)),
    ] {
        assert!(
            SignedServiceDelegation::verify_json(
                text.as_bytes(),
                &body.owner_id,
                &owner.public_keys(),
                &body.tenant,
                100
            )
            .is_err()
        );
    }
}
#[test]
fn shared_actual_playbook_parser_rejects_noncanonical_and_outside_paths() {
    let owner = DeviceSecretKeys::generate().unwrap();
    for playbook in [
        "./selected.yml",
        "a//selected.yml",
        "../outside.yml",
        ".git/inside.yml",
        "a\\selected.yml",
        "a:no.yml",
        "not-yaml.txt",
    ] {
        let mut body = policy(&owner);
        body.playbook = playbook.into();
        assert!(
            owner
                .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
                .is_err()
        );
    }
}
#[test]
fn private_owner_keys_cannot_become_service_signer_or_encryption_recipient() {
    let owner = DeviceSecretKeys::generate().unwrap();
    for role in 0..3 {
        let mut body = policy(&owner);
        match role {
            0 => body.signer.public_key = hex(&owner.public_keys().signing),
            1 => body.recipient.public_key = hex(&owner.public_keys().signing),
            _ => body.recipient.public_key = hex(&owner.public_keys().encryption),
        }
        assert!(
            owner
                .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
                .is_err()
        );
    }
}
#[test]
fn unsupported_team_unsorted_duplicate_refs_and_expanded_quotas_are_denied() {
    let owner = DeviceSecretKeys::generate().unwrap();
    for bad in 0..5 {
        let mut body = policy(&owner);
        match bad {
            0 => body.tenant.kind = ServiceTenantKind::Team,
            1 => body.hosts.push(body.hosts[0].clone()),
            2 => {
                body.credentials = vec![ServicePolicyCredentialRef {
                    id: uid(),
                    revision: 0,
                }]
            }
            3 => body.quotas.max_concurrent = 2,
            _ => body.quotas.max_per_utc_day = body.quotas.max_jobs + 1,
        }
        assert!(
            owner
                .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
                .is_err()
        );
    }
}
#[test]
fn no_signature_can_extend_twenty_four_hour_or_expired_policy_window() {
    let owner = DeviceSecretKeys::generate().unwrap();
    for (issued, expires, now) in [
        (100, 86501, 100),
        (100, 101, 101),
        (101, 1000, 100),
        (100, MAX_TIME + 1, 100),
    ] {
        let mut body = policy(&owner);
        body.issued_at = issued;
        body.expires_at = expires;
        assert!(
            owner
                .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, now)
                .is_err()
        );
    }
}
#[test]
fn signed_bytes_are_exact_canonical_sorted_json_after_fixed_v4_domain() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let raw = body.canonical_bytes().unwrap();
    let map: std::collections::BTreeMap<String, Value> = serde_json::from_slice(&raw).unwrap();
    assert_eq!(raw, serde_json::to_vec(&map).unwrap());
    let message = body.message().unwrap();
    assert!(message.starts_with(SERVICE_DELEGATION_DOMAIN_V4));
    assert_eq!(&message[SERVICE_DELEGATION_DOMAIN_V4.len()..], raw);
}

#[test]
fn route_inputs_are_mandatory_and_old_v2_policy_never_becomes_v4() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    let mut value: Value = serde_json::from_slice(&signed.to_json().unwrap()).unwrap();
    value["body"]
        .as_object_mut()
        .unwrap()
        .remove("route_inputs");
    assert!(
        SignedServiceDelegation::verify_json(
            &serde_json::to_vec(&value).unwrap(),
            &body.owner_id,
            &owner.public_keys(),
            &body.tenant,
            100
        )
        .is_err()
    );
    let mut old = body.clone();
    old.version = 2;
    assert!(
        owner
            .sign_service_delegation(old, &body.owner_id, &body.tenant, 100)
            .is_err()
    );
}

#[test]
fn route_refs_reject_duplicates_wrong_kind_missing_target_and_mixed_numeric_types() {
    let owner = DeviceSecretKeys::generate().unwrap();
    for case in 0..5 {
        let mut body = policy(&owner);
        match case {
            0 => body
                .route_inputs
                .dependencies
                .push(body.route_inputs.dependencies[0].clone()),
            1 => body.route_inputs.key_trust[0].kind = cc_models::ObjectKind::Credential,
            2 => body.route_inputs.dependencies[0].id = uid(),
            3 => body.route_inputs.dependencies[0].revision = MAX_REV + 1,
            _ => body.route_inputs.key_trust.clear(),
        }
        assert!(
            owner
                .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
                .is_err()
        );
    }
    let body = policy(&owner);
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    for bad in [
        json_value(1.0),
        Value::Bool(true),
        Value::String("1".into()),
    ] {
        let mut value: Value = serde_json::from_slice(&signed.to_json().unwrap()).unwrap();
        value["body"]["route_inputs"]["key_trust"][0]["revision"] = bad;
        assert!(
            SignedServiceDelegation::verify_json(
                &serde_json::to_vec(&value).unwrap(),
                &body.owner_id,
                &owner.public_keys(),
                &body.tenant,
                100
            )
            .is_err()
        );
    }
}
fn json_value(value: f64) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn altered_known_host_proxy_or_jump_revision_cannot_reuse_owner_signature() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let mut body = policy(&owner);
    body.route_inputs.dependencies.push(ServiceRouteInputRef {
        id: uid(),
        revision: 2,
        kind: cc_models::ObjectKind::Proxy,
    });
    body.route_inputs.dependencies.push(ServiceRouteInputRef {
        id: uid(),
        revision: 3,
        kind: cc_models::ObjectKind::JumpProfile,
    });
    body.route_inputs
        .dependencies
        .sort_by(|a, b| a.id.cmp(&b.id));
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    for part in ["dependencies", "key_trust"] {
        let mut value: Value = serde_json::from_slice(&signed.to_json().unwrap()).unwrap();
        value["body"]["route_inputs"][part][0]["revision"] = Value::from(9);
        assert!(
            SignedServiceDelegation::verify_json(
                &serde_json::to_vec(&value).unwrap(),
                &body.owner_id,
                &owner.public_keys(),
                &body.tenant,
                100
            )
            .is_err()
        );
    }
}

#[test]
fn unknown_route_fields_and_future_kind_do_not_normalize_into_known_authority() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let body = policy(&owner);
    let signed = owner
        .sign_service_delegation(body.clone(), &body.owner_id, &body.tenant, 100)
        .unwrap();
    for extra in [false, true] {
        let mut value: Value = serde_json::from_slice(&signed.to_json().unwrap()).unwrap();
        if extra {
            value["body"]["route_inputs"]["key_trust"][0]["approved"] = Value::Bool(true);
        } else {
            value["body"]["route_inputs"]["key_trust"][0]["kind"] =
                Value::String("future_trust_kind".into());
        }
        assert!(
            SignedServiceDelegation::verify_json(
                &serde_json::to_vec(&value).unwrap(),
                &body.owner_id,
                &owner.public_keys(),
                &body.tenant,
                100
            )
            .is_err()
        );
    }
}

#[test]
fn bounded_offline_owner_signature_binds_exact_hard_deadline_without_renewal() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let mut body = policy(&owner);
    body.access_policy = ServiceAccessPolicy {
        mode: ServiceAccessMode::BoundedOffline,
        offline_not_after: Some(body.expires_at),
    };
    let id = body.owner_id.clone();
    let tenant = body.tenant.clone();
    let signed = owner
        .sign_service_delegation(body, &id, &tenant, 100)
        .unwrap();
    let raw = signed.to_json().unwrap();
    assert_eq!(
        SignedServiceDelegation::verify_json(&raw, &id, &owner.public_keys(), &tenant, 3699)
            .unwrap()
            .body()
            .access_policy
            .offline_not_after,
        Some(3700)
    );
    assert!(
        SignedServiceDelegation::verify_json(&raw, &id, &owner.public_keys(), &tenant, 3700)
            .is_err()
    );
    for change in [
        serde_json::json!({"mode":"freshOwnerHead","offline_not_after":null}),
        serde_json::json!({"mode":"boundedOffline","offline_not_after":3701}),
    ] {
        let mut value: Value = serde_json::from_slice(&raw).unwrap();
        value["body"]["access_policy"] = change;
        assert!(
            SignedServiceDelegation::verify_json(
                &serde_json::to_vec(&value).unwrap(),
                &id,
                &owner.public_keys(),
                &tenant,
                101
            )
            .is_err()
        );
    }
}
#[test]
fn access_policy_requires_explicit_mode_and_required_nullable_deadline() {
    for value in [
        serde_json::json!({"mode":"freshOwnerHead"}),
        serde_json::json!({"mode":"boundedOffline","offline_not_after":true}),
        serde_json::json!({"mode":"future","offline_not_after":null}),
        serde_json::json!({"mode":"freshOwnerHead","offline_not_after":null,"approved":true}),
    ] {
        assert!(serde_json::from_value::<ServiceAccessPolicy>(value).is_err());
    }
    let owner = DeviceSecretKeys::generate().unwrap();
    let mut body = policy(&owner);
    body.access_policy = ServiceAccessPolicy {
        mode: ServiceAccessMode::BoundedOffline,
        offline_not_after: None,
    };
    assert!(
        body.validate(&body.owner_id, &owner.public_keys(), &body.tenant, 100)
            .is_err()
    );
    body.access_policy = ServiceAccessPolicy {
        mode: ServiceAccessMode::FreshOwnerHead,
        offline_not_after: Some(body.expires_at),
    };
    assert!(
        body.validate(&body.owner_id, &owner.public_keys(), &body.tenant, 100)
            .is_err()
    );
}
#[test]
fn bounded_offline_cannot_exceed_existing_twenty_four_hour_bound() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let mut body = policy(&owner);
    body.expires_at = body.issued_at + 86401;
    body.access_policy = ServiceAccessPolicy {
        mode: ServiceAccessMode::BoundedOffline,
        offline_not_after: Some(body.expires_at),
    };
    assert!(
        body.validate(&body.owner_id, &owner.public_keys(), &body.tenant, 100)
            .is_err()
    );
}

#[test]
fn initial_owner_status_is_fixed_to_original_policy_and_cannot_be_renewed() {
    let owner = DeviceSecretKeys::generate().unwrap();
    let policy = policy(&owner);
    let id = policy.owner_id.clone();
    let tenant = policy.tenant.clone();
    let signed = owner
        .sign_service_delegation(policy, &id, &tenant, 100)
        .unwrap();
    let first = owner
        .sign_initial_service_status(&signed, &id, &tenant, 100)
        .unwrap()
        .to_json()
        .unwrap();
    let later = owner
        .sign_initial_service_status(&signed, &id, &tenant, 200)
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(first, later); // No now+300 renewal, same deterministic statement.
    let value: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(value["body"]["issued_at"], 100);
    assert_eq!(value["body"]["expires_at"], 400);
    assert_eq!(value["body"]["revoked"], false);
    assert_eq!(value["body"]["delegation_digest"], signed.digest().unwrap());
    let signature = STANDARD
        .decode(value["signature"].as_str().unwrap())
        .unwrap();
    let signature = Signature::from_slice(&signature).unwrap();
    let mut message = b"ConsoleCrypt ServiceTrustHead v1\0".to_vec();
    message.extend(serde_json::to_vec(&canonical(value["body"].clone())).unwrap());
    VerifyingKey::from_bytes(&owner.public_keys().signing)
        .unwrap()
        .verify_strict(&message, &signature)
        .unwrap();
    assert!(
        owner
            .sign_initial_service_status(&signed, &id, &tenant, 400)
            .is_err()
    );
    assert!(
        DeviceSecretKeys::generate()
            .unwrap()
            .sign_initial_service_status(&signed, &id, &tenant, 101)
            .is_err()
    );
}

#[test]
fn owner_status_observation_requires_original_signature_scope_and_never_renews() {
    let owner=DeviceSecretKeys::generate().unwrap();let p=policy(&owner);
    let signed=owner.sign_service_delegation(p.clone(),&p.owner_id,&p.tenant,100).unwrap();
    let verified=SignedServiceDelegation::verify_json(&signed.to_json().unwrap(),&p.owner_id,&owner.public_keys(),&p.tenant,100).unwrap();
    let verifier=verified.owner_status_verifier().unwrap();
    let initial=owner.sign_initial_service_status(&signed,&p.owner_id,&p.tenant,100).unwrap();
    assert!(!verifier.verify_json(&initial.to_json().unwrap(),100).unwrap().invalidates_original());
    let sign=|body:InitialServiceStatusBody,key:&DeviceSecretKeys| {
        let signature=STANDARD.encode(key.sign_message(&owner_status_message(&body).unwrap()));
        serde_json::to_vec(&SignedInitialServiceStatus{body,signature}).unwrap()
    };
    let mut revoked=initial.body.clone();revoked.revoked=true;
    assert!(verifier.verify_json(&sign(revoked.clone(),&owner),100).unwrap().invalidates_original());
    assert!(verifier.verify_json(&sign(revoked.clone(),&DeviceSecretKeys::generate().unwrap()),100).is_err());
    let mut raw:Value=serde_json::from_slice(&initial.to_json().unwrap()).unwrap();raw["body"]["revoked"]=Value::Bool(true);
    assert!(verifier.verify_json(&serde_json::to_vec(&raw).unwrap(),100).is_err());
    let mut advanced=initial.body.clone();advanced.policy_revision+=1;
    assert!(verifier.verify_json(&sign(advanced,&owner),100).unwrap().invalidates_original());
    let mut foreign=revoked.clone();foreign.service_id=uid();
    assert!(verifier.verify_json(&sign(foreign,&owner),100).is_err());
    let mut rollback=revoked.clone();rollback.revocation_epoch=0;
    assert!(verifier.verify_json(&sign(rollback,&owner),100).is_err());
    assert!(verifier.verify_json(&sign(revoked,&owner),400).is_err());
}

#[test]
fn fixed_revocation_reuses_original_status_domain_and_cannot_be_a_renewal() {
    let owner=DeviceSecretKeys::generate().unwrap();let p=policy(&owner);
    let signed=owner.sign_service_delegation(p.clone(),&p.owner_id,&p.tenant,p.issued_at).unwrap();
    let verified=SignedServiceDelegation::verify_json(&signed.to_json().unwrap(),&p.owner_id,&owner.public_keys(),&p.tenant,p.issued_at).unwrap();
    let clock=p.issued_at+10;
    let revoked=owner.sign_service_revocation(&signed,&p.owner_id,p.policy_revision,p.revocation_epoch+1,clock,clock).unwrap();
    let raw=revoked.to_json().unwrap();let wire:serde_json::Value=serde_json::from_slice(&raw).unwrap();
    assert_eq!(wire["body"]["revoked"],true);assert_eq!(wire["body"]["expires_at"],clock+300);
    assert!(verified.owner_status_verifier().unwrap().verify_json(&raw,clock).unwrap().invalidates_original());
    assert!(verified.owner_status_verifier().unwrap().verify_json(&raw,clock+300).is_err());
    assert!(owner.sign_service_revocation(&signed,&p.owner_id,p.policy_revision,p.revocation_epoch,clock,clock).is_err());
    assert!(owner.sign_service_revocation(&signed,&p.owner_id,p.policy_revision,p.revocation_epoch+1,clock+1,clock).is_err());
    assert!(DeviceSecretKeys::generate().unwrap().sign_service_revocation(&signed,&p.owner_id,p.policy_revision,p.revocation_epoch+1,clock,clock).is_err());
}
