//! The original installed supervisor calls this before its actual adapter spawn.
//! SAME Catalog/Files/Access/clock. No service-ID-to-Permit reconstruction,
//! second file owner, generic byte callback or synthetic cleanup observation.
use super::*;
use crate::collector_credentials::OpenedCollectorCredential;
use crate::CollectorCredentialTarget;

/// Private component config and narrowed ORIGINAL ends, not a public receipt.
/// Core must retain its actual Access and use these ends in pre-exec/runtime;
/// metadata alone never starts or authorizes a component.
pub(super) struct CredentialScrape {
    pub(super) raw: Vec<u8>,
    pub(super) scope_hashes: Vec<Hash>,
    pub(super) expires_at: Timestamp,
    pub(super) deadline_boottime_ns: u64,
}

fn config_slot<'a>(config: &'a mut Value, target: &CollectorCredentialTarget) -> Result<&'a mut Value> {
    let rows = match target {
        CollectorCredentialTarget::Receiver {} => config.get_mut("remote_write"),
        CollectorCredentialTarget::Scrape { .. } => config.get_mut("scrape_configs"),
    }.and_then(Value::as_array_mut).ok_or(Error::InvalidWire)?;
    let index = match target {
        CollectorCredentialTarget::Receiver {} => {
            if rows.len() != 1 { return Err(Error::InvalidWire); }
            0
        }
        CollectorCredentialTarget::Scrape { target_id } => {
            let expected = format!("cc_{target_id}");
            let positions: Vec<_> = rows.iter().enumerate().filter_map(|(i, row)|
                (row.get("job_name").and_then(Value::as_str) == Some(expected.as_str())).then_some(i)).collect();
            if positions.len() != 1 { return Err(Error::WrongClaim); }
            positions[0]
        }
    };
    let row = &mut rows[index];
    let object = row.as_object().ok_or(Error::InvalidWire)?;
    if ["basic_auth", "authorization", "bearer_token", "bearer_token_file", "oauth2", "http_headers"]
        .iter().any(|key| object.contains_key(*key))
        || object.get("follow_redirects") != Some(&Value::Bool(false))
    { return Err(Error::CapabilityDenied); }
    Ok(row)
}

/// One first-subset exporter and/or one receiver, each separately owner scoped.
/// The handler calls recipient.open_original for each already bounded capsule;
/// only the actual supervisor can supply these original Files/Catalog/Access.
pub(super) fn build_registered_scrape<S: CompareAndSwap>(
    files: &mut Files, catalog: &Catalog, access: &mut CollectorServiceAccess<'_, S>,
    mut opened: Vec<OpenedCollectorCredential<'_>>, now: Now,
    clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
) -> Result<CredentialScrape> {
    if opened.is_empty() || opened.len() > 2 || catalog.disk.is_none() { return Err(Error::Quota); }
    catalog.check()?;
    // SAME actual Access's signed required intent, never a caller-provided
    // target list. Missing/unexpected/duplicate capsules cannot degrade to
    // unauthenticated config or leave earlier plaintext files behind.
    let actual = opened.iter().map(|item| item.target().clone()).collect::<Vec<_>>();
    crate::check_collector_credential_target_set(access.policy(), &actual, &catalog.manifest)?;
    let mut config: Value = serde_json::from_slice(&catalog.scrape()?).map_err(|_| Error::InvalidWire)?;
    let mut hashes = BTreeSet::new();
    let mut exporter_seen = false;
    let mut receiver_seen = false;
    // Preflight EVERY target before the first mutation. Neither caller URL nor
    // a replacement auth policy can enter the generated installed config.
    for item in &opened {
        if !hashes.insert(item.scope_hash()?) { return Err(Error::Replay); }
        let seen = match item.target() {
            CollectorCredentialTarget::Receiver {} => &mut receiver_seen,
            CollectorCredentialTarget::Scrape { .. } => &mut exporter_seen,
        };
        if *seen { return Err(Error::Quota); }
        *seen = true;
        config_slot(&mut config, item.target())?;
    }
    // Strict Core narrow never increases an existing end. Longest effective
    // end first makes a two-secret batch monotonically narrower throughout.
    opened.sort_by_key(|item| std::cmp::Reverse(item.effective_expiry()));
    let (raw, scope_hashes) = files.with_original_credential_session(catalog, access, now, clock, |writer| {
        let mut scope_hashes = Vec::new();
        for item in opened {
            let written = item.write_registered(writer)?;
            let auth = written.private_auth_projection(writer).map_err(|_| Error::UncertainCommit)?;
            let row = config_slot(&mut config, written.target()).map_err(|_| Error::UncertainCommit)?;
            let basic = auth.get("basic_auth").ok_or(Error::UncertainCommit)?.clone();
            row.as_object_mut().ok_or(Error::UncertainCommit)?.insert("basic_auth".into(), basic);
            scope_hashes.push(written.scope_hash());
        }
        let raw = json_body(&config).map_err(|_| Error::UncertainCommit)?;
        if raw.len() > 65536 { return Err(Error::UncertainCommit); }
        writer.recheck().map_err(|_| Error::UncertainCommit)?;
        Ok((raw, scope_hashes))
    }).map_err(|_| Error::UncertainCommit)?;
    access.recheck(now, clock).map_err(|_| Error::UncertainCommit)?;
    catalog.check().map_err(|_| Error::UncertainCommit)?;
    Ok(CredentialScrape { raw, scope_hashes, expires_at: access.expires_at(),
        deadline_boottime_ns: access.deadline_boottime_ns() })
}

#[path="credential_capsules_lifetime.rs"]
mod lifetime;
pub(super) use lifetime::build_registered_lifetime_scrape;
