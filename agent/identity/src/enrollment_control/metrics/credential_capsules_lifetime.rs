//! Child of credential_capsules. Reuses actual fixed config_slot and Files.
use super::*;
use crate::collector_credentials::OpenedLifetimeCollectorCredentials;
pub(in super::super) fn build_registered_lifetime_scrape<S: CompareAndSwap>(
    files: &mut Files, catalog: &Catalog, access: &mut CollectorServiceAccess<'_, S>,
    opened: OpenedLifetimeCollectorCredentials<'_>, now: Now,
    clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
) -> Result<CredentialScrape> {
    catalog.check()?;
    let targets = opened.targets();
    crate::check_collector_credential_target_set(access.policy(), &targets, &catalog.manifest)?;
    let mut config: Value = serde_json::from_slice(&catalog.scrape()?).map_err(|_| Error::InvalidWire)?;
    for target in &targets { config_slot(&mut config, target)?; }
    let (raw, hashes) = files.with_original_credential_session(catalog, access, now, clock, |writer| {
        let written = opened.write_registered(writer)?;
        let mut hashes = BTreeSet::new();
        for item in written {
            if !hashes.insert(item.scope_hash()) { return Err(Error::UncertainCommit); }
            let auth = item.private_auth_projection(writer).map_err(|_| Error::UncertainCommit)?;
            let row = config_slot(&mut config, item.target()).map_err(|_| Error::UncertainCommit)?;
            row.as_object_mut().ok_or(Error::UncertainCommit)?.insert("basic_auth".into(),
                auth.get("basic_auth").ok_or(Error::UncertainCommit)?.clone());
        }
        let raw = json_body(&config).map_err(|_| Error::UncertainCommit)?;
        if raw.len() > 65536 { return Err(Error::UncertainCommit); }
        writer.recheck().map_err(|_| Error::UncertainCommit)?;
        Ok((raw, hashes.into_iter().collect()))
    }).map_err(|_| Error::UncertainCommit)?;
    access.recheck(now, clock).map_err(|_| Error::UncertainCommit)?;
    catalog.check().map_err(|_| Error::UncertainCommit)?;
    Ok(CredentialScrape { raw, scope_hashes: hashes, expires_at: access.expires_at(), deadline_boottime_ns: access.deadline_boottime_ns() })
}
