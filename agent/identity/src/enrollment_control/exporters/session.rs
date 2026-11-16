#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Probed,
    RevocationProbed,
    Claimed,
    Applied,
    Revoked,
    Closed,
}
struct Session<S> {
    store: Option<S>,
    node_dir: PathBuf,
    agent: AgentId,
    node: PublicNodeIdentity,
    phase: Phase,
    seen: BTreeSet<String>,
    expected_pin: OwnerPin,
    expected_rev: u64,
    claim: Option<Hash>,
    certificate: Option<Hash>,
    offer_expires: Option<Timestamp>,
    effects: bool,
    catalog: Option<artifact::Catalog>,
}
impl<S: CompareAndSwap> Session<S> {
    fn snapshot(&self) -> Result<State> {
        self.store.as_ref().ok_or(Error::WrongState)?.load()
    }
    fn checked(&self, now: Now) -> Result<State> {
        if let Some(catalog) = &self.catalog {
            catalog.check()?;
        }
        let state = self.snapshot()?;
        if state.owner() != &self.expected_pin || state.revision() != self.expected_rev {
            return Err(Error::Conflict);
        }
        if now.0 >= state.owner().expires_at || self.offer_expires.is_some_and(|x| now.0 >= x) {
            return Err(Error::Expired);
        }
        if inspect_existing_node_identity(&self.node_dir, self.agent)? != self.node {
            return Err(Error::WrongClaim);
        }
        Ok(state)
    }
    fn mutate<F>(&mut self, revision: u64, effect: F) -> Result<()>
    where
        F: FnOnce(&mut Actor<RevisionFence<S>>) -> Result<()>,
    {
        self.effects = true; // Any ambiguity after this is terminal UNKNOWN.
        let inner = self.store.take().ok_or(Error::WrongState)?;
        let mut actor = Actor::new(RevisionFence {
            inner,
            expected: revision,
        })?;
        let result = effect(&mut actor);
        self.store = Some(actor.into_store().inner);
        result
    }
    fn expect_request(&self, revision: u64, pin: &OwnerPin) -> Result<()> {
        if revision != self.expected_rev || pin != &self.expected_pin {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    fn readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let certificate = state.certificate(self.agent).ok_or(Error::NotFound)?;
        if state.revoked(self.agent)
            || certificate.agent_key != self.node.public_key
            || certificate.key_epoch != self.node.key_epoch
            || now.0 < certificate.not_before
            || now.0 >= certificate.expires_at
            || Some(certificate.digest()?) != self.certificate
        {
            return Err(Error::WrongClaim);
        }
        Ok(
            json!({"certificate":certificate,"certificateDigest":certificate.digest()?,
            "nodeFingerprint":self.node.key_fingerprint,"ownerPin":state.owner(),
            "actorRevision":state.revision(),"revoked":false}),
        )
    }
    fn revoked_readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let cert = state.certificate(self.agent).ok_or(Error::NotFound)?;
        // No certificate-live check: a CURRENT expired key must be tombstonable.
        if !state.revoked(self.agent)
            || cert.agent_key != self.node.public_key
            || cert.key_epoch != self.node.key_epoch
            || Some(cert.digest()?) != self.certificate
        {
            return Err(Error::WrongClaim);
        }
        Ok(
            json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
            "certificateDigest":cert.digest()?,"revoked":true}),
        )
    }
    fn request(&mut self, frame: Envelope, clock: &mut Clock) -> Result<Value> {
        if self.seen.len() >= 4 || !self.seen.insert(frame.request_id.clone()) {
            return Err(Error::Replay);
        }
        let now = clock.now()?;
        self.checked(now)?;
        let payload = match (self.phase, frame.kind.as_str()) {
            (Phase::New, "probeExporters") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                self.phase = Phase::Probed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent),
                    "advertisedScopes":[Scope::ExportersManage],"exporter":self.catalog.as_ref().map(|c|c.descriptor()).transpose()?.unwrap_or(Value::Null)})
            }
            (Phase::New, "probeExportersRevocation") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                let cert = state.certificate(self.agent).ok_or(Error::NotFound)?;
                if cert.agent_key != self.node.public_key
                    || cert.key_epoch != self.node.key_epoch
                    || cert.scopes.0.as_slice() != [Scope::ExportersManage]
                    || cert.capabilities.scopes != cert.scopes
                    || cert.capabilities.version != 1
                    || cert.header.owner_epoch != state.owner().epoch
                {
                    return Err(Error::WrongClaim);
                }
                self.phase = Phase::RevocationProbed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent),
                    "advertisedScopes":[Scope::ExportersManage],"exporter":self.catalog.as_ref().map(|c|c.descriptor()).transpose()?.unwrap_or(Value::Null)})
            }
            (Phase::RevocationProbed, "revokeCertificate") => {
                let input: RevokeCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                let state = self.checked(clock.now()?)?;
                let current = state.certificate(self.agent).ok_or(Error::NotFound)?;
                let revoke = &input.revocation.body;
                if state.revoked(self.agent)
                    || current.agent_key != self.node.public_key
                    || current.key_epoch != self.node.key_epoch
                    || current.scopes.0.as_slice() != [Scope::ExportersManage]
                    || current.capabilities.scopes != current.scopes
                    || current.header.owner_epoch != state.owner().epoch
                    || revoke.agent_id != self.agent
                    || revoke.key_epoch != current.key_epoch
                    || revoke.certificate_hash != current.digest()?
                {
                    return Err(Error::WrongClaim);
                }
                let digest = current.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self
                    .expected_pin
                    .checkpoint
                    .checked_add(1)
                    .ok_or(Error::Quota)?;
                self.mutate(self.expected_rev, |actor| {
                    actor.revoke(input.revocation, clock.now()?)
                })?;
                self.expected_rev = revision;
                self.expected_pin.checkpoint = checkpoint;
                self.certificate = Some(digest);
                let value = self.revoked_readback(clock.now()?)?;
                self.phase = Phase::Revoked;
                value
            }
            (Phase::Revoked, "observeRevocation") => {
                let input: ObserveRevocation = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                if input.agent_id != self.agent
                    || input.key_epoch != self.node.key_epoch
                    || Some(input.certificate_digest) != self.certificate
                {
                    return Err(Error::WrongClaim);
                }
                let value = self.revoked_readback(clock.now()?)?;
                self.phase = Phase::Closed;
                value
            }
            (Phase::Probed, "stageOffer") => {
                let input: StageOffer = decode(frame.payload)?;
                self.expect_request(input.expected_actor_revision, &input.expected_owner_pin)?;
                let state = self.checked(clock.now()?)?;
                if state.certificate(self.agent).is_some()
                    || self.node.key_epoch != 1
                    || input.offer.body.agent_id != self.agent
                    || input.offer.body.agent_key != self.node.public_key
                    || input.offer.body.scopes.0.as_slice() != [Scope::ExportersManage]
                {
                    return Err(Error::WrongClaim);
                }
                let secret = token(input.token.as_str())?;
                if !bool::from(
                    secret
                        .digest()
                        .as_bytes()
                        .ct_eq(input.offer.body.token_hash.as_bytes()),
                ) {
                    return Err(Error::InvalidToken); // BEFORE first mutation.
                }
                let pre_pin = state.owner().clone();
                let start = state.revision();
                let offered_rev = start.checked_add(1).ok_or(Error::Quota)?;
                let claimed_rev = offered_rev.checked_add(1).ok_or(Error::Quota)?;
                let offered_cp = pre_pin.checkpoint.checked_add(1).ok_or(Error::Quota)?;
                let offer = input.offer;
                let offered = offer.clone();
                self.mutate(start, |a| a.offer(offered, clock.now()?))?;
                self.expected_rev = offered_rev;
                self.expected_pin.checkpoint = offered_cp;
                self.offer_expires = Some(offer.body.expires_at);
                clock.cap(offer.body.expires_at)?;
                self.checked(clock.now()?)?;
                // Custody requires PRE-offer current pin, not genesis/current-post-offer.
                let claim = create_exporter_node_claim(
                    &self.node_dir,
                    self.agent,
                    &pre_pin,
                    offer.clone(),
                    clock.now()?,
                )?;
                let digest = claim.body.digest()?;
                self.checked(clock.now()?)?;
                let consumed = claim.clone();
                self.mutate(offered_rev, |a| {
                    if a.consume(&secret, consumed, clock.now()?)? != digest {
                        return Err(Error::WrongClaim);
                    }
                    Ok(())
                })?;
                self.expected_rev = claimed_rev;
                let state = self.checked(clock.now()?)?;
                if state.invitation_status(offer.body.invitation_id)
                    != Some(InvitationStatus::Claimed)
                {
                    return Err(Error::WrongState);
                }
                self.claim = Some(digest);
                self.phase = Phase::Claimed;
                json!({"claim":claim,"claimDigest":digest,"ownerPin":state.owner(),"actorRevision":state.revision()})
            }
            (Phase::Claimed, "applyCertificate") => {
                let input: ApplyCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                let cert = &input.certificate.body;
                if self.claim != Some(input.claim_digest)
                    || cert.agent_id != self.agent
                    || cert.agent_key != self.node.public_key
                    || cert.key_epoch != 1
                    || !matches!(&cert.origin, CertificateOrigin::Enrollment { claim_hash, .. }
                        if Some(*claim_hash) == self.claim)
                {
                    return Err(Error::WrongClaim);
                }
                let digest = cert.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self
                    .expected_pin
                    .checkpoint
                    .checked_add(1)
                    .ok_or(Error::Quota)?;
                self.checked(clock.now()?)?;
                self.mutate(self.expected_rev, |a| {
                    a.approve(input.certificate, clock.now()?)
                })?;
                self.expected_rev = revision;
                self.expected_pin.checkpoint = checkpoint;
                self.certificate = Some(digest);
                let v = self.readback(clock.now()?)?;
                self.phase = Phase::Applied;
                v
            }
            (Phase::Applied, "observeCertificate") => {
                let input: ObserveCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                if input.agent_id != self.agent
                    || self.certificate != Some(input.certificate_digest)
                {
                    return Err(Error::WrongClaim);
                }
                let v = self.readback(clock.now()?)?;
                self.phase = Phase::Closed;
                v
            }
            _ => return Err(Error::WrongState),
        };
        self.checked(clock.now()?)?;
        Ok(
            json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}),
        )
    }
}

