//! SAME actual supervisor/Permit/Files. No idle client, replayed plaintext or cleanup observer.
use super::*;
use crate::collector_credentials::CollectorCredentialRecipient;

pub(super) fn prepare(
    actor: &mut Actor<SqliteJournalStore>, permit: &CollectorServicePermit,
    files: &mut Files, catalog: &Catalog, recipient: &mut CollectorCredentialRecipient,
    stored: crate::StoredCollectorServiceCredentialSet,
) -> Result<super::service_material::Material> {
    let mut clock = CollectorServiceClockObservation::observe_local;
    let now = super::service_supervisor::actual_now()?;
    let policy = actor.check_collector_service(permit, now, &mut clock)?;
    let owner = actor.snapshot()?.owner().clone();
    // Actual original Actor policy and clock are checked again by open below.
    stored.check(&owner, &policy, &stored.authorization.body.admission.body.clock, &catalog.manifest, now)?;
    let capsules = stored.decoded()?;
    let opened = recipient.open_original_service_lifetime_set(actor, permit, &stored.authorization, &capsules,
        &catalog.manifest, &mut super::service_supervisor::actual_now)?;
    let now = super::service_supervisor::actual_now()?;
    let mut access = actor.borrow_collector_service(permit, now, &mut clock)?;
    let cfg = super::credential_capsules::build_registered_lifetime_scrape(files, catalog, &mut access, opened, now, &mut clock)?;
    Ok(super::service_material::Material { scrape: cfg.raw, expires: cfg.expires_at, deadline: cfg.deadline_boottime_ns })
}
