//! Public synthetic wire fixture only. No vault, disk keys, network or executor.
use cc_crypto_core::{DeviceSecretKeys, ServiceDelegationPolicy, SignedServiceDelegation};
use cc_vault_core::DeviceIdentity;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
fn uid() -> String {
    Uuid::new_v4().to_string()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() != 1 {
        return Err("fixed fixture accepts no arguments".into());
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let owner = DeviceIdentity::generate()?;
    let service = DeviceSecretKeys::generate()?;
    let owner_id = owner.device_id().to_string();
    let tenant_id = uid();
    let host_id = uid();
    let body: ServiceDelegationPolicy = serde_json::from_value(json!({
        "version":4,"id":uid(),"owner_id":owner_id,"owner_epoch":1,"tenant":{"kind":"localProfile","id":tenant_id},
        "service_id":uid(),"signer":{"kind":"ed25519","public_key":hex(&service.public_keys().signing),"key_epoch":1,"certificate_epoch":1},
        "recipient":{"kind":"x25519","public_key":hex(&service.public_keys().encryption),"key_epoch":1,"certificate_epoch":1},
        "project_id":uid(),"project_revision":4,"template_digest":"a".repeat(64),"playbook":"roles/РїР»РµР№Р±СѓРє.yml",
        "schedule_id":uid(),"schedule_revision":1,"definition_digest":"b".repeat(64),"inventory_digest":"c".repeat(64),
        "parameters_digest":"d".repeat(64),"environment_digest":"e".repeat(64),
        "hosts":[{"id":host_id,"revision":7,"target_digest":"f".repeat(64)}],"credentials":[],"modes":["check"],
        "route_inputs":{"dependencies":[{"id":host_id,"revision":7,"kind":"host"}],"key_trust":[{"id":uid(),"revision":1,"kind":"known_host"}]},
        "quotas":{"max_jobs":16,"max_per_utc_day":4,"max_concurrent":1,"max_job_seconds":120,"max_forks":1},
        "policy_revision":1,"revocation_epoch":1,"issued_at":now,"expires_at":now+3600,
        "desktop_lock_policy":"independentDelegatedService",
        "access_policy":{"mode":"freshOwnerHead","offline_not_after":null}
    }))?;
    let tenant = body.tenant.clone();
    let signed = owner.sign_service_delegation(body, now)?;
    let raw = signed.to_json()?;
    let verified =
        SignedServiceDelegation::verify_json(&raw, &owner_id, &owner.public_keys(), &tenant, now)?;
    println!(
        "{}",
        json!({"fixtureVersion":1,"syntheticFixtureOnly":true,"nativePreparedReviewExercised":false,"nativeDeviceIdentityExercised":true,
        "ownerId":owner_id,"ownerEpoch":1,"ownerSigning":hex(&owner.public_keys().signing),
        "ownerEncryption":hex(&owner.public_keys().encryption),"ownerNotAfter":now+86400,
        "tenant":tenant,"now":now,"digest":verified.digest()?,"delegation":serde_json::from_slice::<serde_json::Value>(&raw)?})
    );
    Ok(())
}
