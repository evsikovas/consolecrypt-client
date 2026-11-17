//! SAME actual supervisor/Permit/Files. No idle client, replayed plaintext or cleanup observer.
use super::*;
use crate::collector_credentials::CollectorCredentialRecipient;
use super::service_supervisor::{failure, FailureStage};

pub(super) fn prepare(
    actor: &mut Actor<SqliteJournalStore>, permit: &CollectorServicePermit,
    files: &mut Files, catalog: &Catalog, recipient: &mut CollectorCredentialRecipient,
    stored: crate::StoredCollectorServiceCredentialSet,
    input: &mut File, pending: &mut Vec<u8>,
    checks: &super::service_supervisor::ReadChecks<'_>,
) -> Result<super::service_material::Material> {
    let now = super::service_supervisor::actual_now()?;
    let policy = actor.check_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
    let owner = actor.snapshot_collector_service_waiting(super::service_supervisor::actual_now()?,
            Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?.owner().clone();
    // Actual original Actor policy and clock are checked again by open below.
    stored.check(&owner, &policy, &stored.authorization.body.admission.body.clock, &catalog.manifest, now)
        .map_err(|e| failure(FailureStage::LifetimeValidate, e))?;
    let capsules = stored.decoded().map_err(|e| failure(FailureStage::LifetimeDecode, e))?;
    let opened = recipient.open_original_service_lifetime_set_waiting(actor, permit, &stored.authorization, &capsules,
        &catalog.manifest, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()))
        .map_err(|e| failure(FailureStage::LifetimeOpen, e))?;
    let now = super::service_supervisor::actual_now()?;
    let mut access = actor.borrow_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
    let cfg = super::credential_capsules::build_registered_lifetime_scrape(files, catalog, &mut access, opened, now, &mut || checks.clock(input, pending))
        .map_err(|e| failure(FailureStage::LifetimeProjection, e))?;
    Ok(super::service_material::Material { scrape: cfg.raw, expires: cfg.expires_at, deadline: cfg.deadline_boottime_ns })
}
