//! Default-OFF owned protocol component. Real Actor/Permit/Files/material code;
//! never spawns a collector, unit, adapter or a replacement cleanup observer.
use super::*;

pub(crate) fn run_owned_protocol_component(args: &[String]) -> Result<()> {
    if args != ["owned-lifetime-protocol-owner"] || service_uid()? != 10001 { return Err(Error::CapabilityDenied); }
    if !Path::new("/.dockerenv").is_file()
        || fs::read_link("/proc/self/exe").map_err(|_|Error::Storage)? != Path::new("/opt/cc-enroll-fixture/bin/cc-agent-identity")
        || fs::symlink_metadata("/out").map_err(|_|Error::Storage)?.file_type().is_symlink()
    { return Err(Error::CapabilityDenied); }
    let token = std::env::var("CC_LIFETIME_PAIR_TOKEN").map_err(|_| Error::WrongClaim)?;
    let parsed = uuid::Uuid::parse_str(&token).map_err(|_| Error::WrongClaim)?;
    if parsed.is_nil() || parsed.to_string() != token { return Err(Error::WrongClaim); }
    let root = PathBuf::from(std::env::var_os("CC_LIFETIME_PAIR_ROOT").ok_or(Error::WrongClaim)?);
    if root != Path::new("/out").join(format!("owned-{token}")) { return Err(Error::WrongContext); }
    let metadata = fs::symlink_metadata(&root).map_err(|_| Error::Storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != 10001
        || metadata.mode() & 0o7777 != 0o700 { return Err(Error::Storage); }
    let mut input = control_pipe()?;
    let mut pending = Vec::new();
    let raw = root_file(Path::new(CONFIG))?;
    let config: Config = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
    if config.format != "cc-agent-enroll-config/v1" { return Err(Error::WrongClaim); }
    for path in [&config.node_dir,&config.db_dir,&config.anchor_dir] {
        crate::durable::absolute(path)?;
        crate::durable::secure_path(path)?;
    }
    let pin_raw = root_file(&config.owner_pin)?;
    let genesis: OwnerPin = serde_json::from_slice(&pin_raw).map_err(|_| Error::InvalidWire)?;
    let node = inspect_existing_node_identity(&config.node_dir, config.agent_id)?;
    let store = SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?;
    let mut actor = Actor::new(store)?;
    let catalog = Catalog::load()?;
    let end = Instant::now() + Duration::from_secs(300);
    loop {
        if Instant::now() >= end { return Err(Error::Expired); }
        original_config(&raw,&config.owner_pin,&pin_raw)?;
        catalog.check()?;
        if inspect_existing_node_identity(&config.node_dir,config.agent_id)? != node { return Err(Error::WrongClaim); }
        if !launcher_alive(&mut input,&mut pending)? {
            println!("{}",json!({"format":"cc-owned-lifetime-protocol-owner/v1","token":token,
                "startsReserved":0,"collectorSpawned":false,"protocolComponentOnly":true,
                "noOriginalFilesCreated":true,"actorRevision":actor.snapshot()?.revision()}));
            return Ok(());
        }
        let observed = actor.next_collector_service(config.agent_id,catalog.hash,actual_now()?,
            &mut CollectorServiceClockObservation::observe_local)?;
        let Some(observed) = observed else { std::thread::sleep(Duration::from_millis(20)); continue; };
        let mut nonce = [0u8;16]; getrandom::fill(&mut nonce).map_err(|_| Error::Entropy)?;
        nonce[6] = (nonce[6] & 0x0f) | 0x40;
        nonce[8] = (nonce[8] & 0x3f) | 0x80;
        let permit = actor.reserve_collector_service_start(observed.policy.body.service_id,Nonce::from_bytes(nonce),
            actual_now()?,&mut CollectorServiceClockObservation::observe_local)?;
        let start_state = actor.snapshot()?;
        let starts = match start_state.latest.as_ref().map(|e|&e.transition) {
            Some(crate::durable::Transition::CollectorServiceStart {prior_starts,start,..}) if *start == permit.start_id() =>
                prior_starts.checked_add(1).ok_or(Error::Quota)?,
            _ => return Err(Error::UncertainCommit),
        };
        let mut files = Files::new_service(&permit)?;
        let material = super::super::service_material::prepare(&mut actor,&permit,&mut files,&catalog,
            &config.node_dir,&mut input,&mut pending)?;
        let material_hash = digest(&material.scrape);
        let actual_deadline = material.deadline;
        let actual_expiry = material.expires;
        drop(material);
        // Material writer holds the original pre-child Files capability;
        // thereafter the original owner retains cleanup until STOP/EOF.
        files.cleanup_allowed = false;
        let stored = actor.collector_service_credential_set_original(&permit,actual_now()?,
            &mut CollectorServiceClockObservation::observe_local)?;
        let admission = if stored.is_some() {
            let readback = actor.readback_collector_service_lifetime(permit.service_id(),permit.delegation_hash(),actual_now()?,
                &mut CollectorServiceClockObservation::observe_local)?;
            Some(json!({"authorizationDigest":readback.set.authorization.body.digest()?,
                "orderedCapsuleHashes":readback.set.capsule_hashes()?,"admittedRevision":readback.admitted_revision}))
        } else { None };
        // Hold actual descriptors/Permit through Node receipt delivery and all
        // Native fault assertions. End is committed ONLY on original STOP/EOF.
        while launcher_alive(&mut input,&mut pending)? {
            if Instant::now() >= end { return Err(Error::Expired); }
            actor.check_collector_service(&permit,actual_now()?,&mut CollectorServiceClockObservation::observe_local)?;
            original_config(&raw,&config.owner_pin,&pin_raw)?; catalog.check()?;
            if inspect_existing_node_identity(&config.node_dir,config.agent_id)? != node { return Err(Error::WrongClaim); }
            std::thread::sleep(Duration::from_millis(20));
        }
        let ended = retire_unlaunched(&mut files,&permit)?;
        actor.finish_unlaunched_collector_service(&permit,ended,actual_now()?)?;
        let after = actor.snapshot()?;
        match after.latest.as_ref().map(|e|&e.transition) {
            Some(crate::durable::Transition::CollectorServiceEnd {start,reason:crate::CollectorServiceEndReason::NoChildCreated,..})
                if *start==permit.start_id() => (),
            _ => return Err(Error::UncertainCommit),
        }
        println!("{}",json!({"format":"cc-owned-lifetime-protocol-owner/v1","token":token,"startsReserved":starts,
            "collectorSpawned":false,"protocolComponentOnly":true,"originalFilesRetired":true,
            "endCommitted":true,"endReason":"NoChildCreated","actorRevision":after.revision(),
            "admission":admission,"scrapeConfigHash":material_hash,"originalBootStamp":observed.claim.body.clock,
            "effectiveDeadlineBoottimeNs":actual_deadline,"effectiveExpiresAt":actual_expiry}));
        return Ok(());
    }
}
