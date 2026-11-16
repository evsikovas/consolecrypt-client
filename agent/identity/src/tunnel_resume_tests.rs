// DRAFT Actor/journal tests, never a claim of physical socket drainage.
// The private retirement calls below are the approved CLOSED-DRAIN UNIT FIXTURE;
// future transport must prove real joins/reaping before connecting that seam.
use super::*;

fn enrolled_resume() -> Harness {
    let mut h = Harness::offered(Scopes(vec![Scope::TunnelOpen]), Scopes(vec![Scope::TunnelOpen]));
    h.actor.consume(&h.token, sign(h.claim.clone(), &h.agent), now()).unwrap();
    h.actor.approve(sign(h.cert.clone(), &h.owner), now()).unwrap();
    h
}
fn resume_policy(h: &Harness) -> TunnelRoutePolicyV2 {
    let mut route: TunnelRoutePolicy = serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap();
    route.context = context();
    route.agent_id = h.cert.agent_id;
    route.agent_certificate_hash = h.cert.digest().unwrap();
    route.limits.max_ttl_seconds = 120;
    route.limits.max_frame_bytes = 64;
    route.limits.max_bytes_per_channel = 96;
    route.limits.max_bytes_total = 128;
    TunnelRoutePolicyV2 { format: "cc-agent-tunnel-route-policy/v2".into(), route, resume_original: true }
}
fn original(h: &mut Harness, policy: &TunnelRoutePolicyV2, n: u8) -> (Signed<Lease>, Permit) {
    let lease = sign(h.lease(policy.operation().unwrap(), n), &h.owner);
    let permit = h.actor.admit(lease.clone(), h.proof(&lease.body, n), Some(&h.fresh()),
        &policy.operation().unwrap(), now()).unwrap();
    (lease, permit)
}
fn enroll_disk(root: &DurableTemp, h: &Harness) -> Actor<FileStore> {
    let mut actor = root.create(h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    actor.consume(&h.token, sign(h.claim.clone(), &h.agent), now()).unwrap();
    actor.approve(sign(h.cert.clone(), &h.owner), now()).unwrap();
    actor
}

#[test]
fn tunnel_resume_original_file_journal_restart_preserves_ttl_and_debits() {
    let h = enrolled_resume();
    let p = resume_policy(&h);
    let root = DurableTemp::new();
    let mut a = enroll_disk(&root, &h);
    let lease = sign(h.lease(p.operation().unwrap(), 90), &h.owner);
    let first = a.admit(lease.clone(), h.proof(&lease.body, 90), Some(&h.fresh()), &p.operation().unwrap(), now()).unwrap();
    a.debit_tunnel(&first, &p, &[7;64], now()).unwrap();
    // Private closed-drain UNIT fixture, not an observed network/worker close.
    a.retire_tunnel_after_owned_drain(first, now()).unwrap();
    drop(a);
    let mut restored = root.open(&h); // SAME FileStore JSON/anchor replay.
    let resumed = restored.resume_tunnel(lease.clone(), h.proof(&lease.body, 91), &h.fresh(), &p, Now(time(1001))).unwrap();
    assert_eq!(resumed.expires_at(), lease.body.expires_at);
    let before = restored.snapshot().unwrap().revision();
    assert_eq!(restored.debit_tunnel(&resumed, &p, &[8;33], Now(time(1001))), Err(Error::Quota));
    assert_eq!(restored.snapshot().unwrap().revision(), before);
    restored.debit_tunnel(&resumed, &p, &[8;32], Now(time(1001))).unwrap();
    restored.retire_tunnel_after_owned_drain(resumed, Now(time(1001))).unwrap();
    drop(restored);
    let mut restored = root.open(&h);
    assert!(matches!(restored.resume_tunnel(lease.clone(), h.proof(&lease.body, 92), &h.fresh(), &p, Now(time(1002))), Err(Error::Quota)));
    assert_eq!(restored.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited().0, 96);
    assert_eq!(restored.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited().1, 96);
}

#[test]
fn tunnel_resume_restored_active_or_unknown_generation_is_denied() {
    let h = enrolled_resume(); let p = resume_policy(&h); let root = DurableTemp::new();
    let mut actor = enroll_disk(&root, &h);
    let lease = sign(h.lease(p.operation().unwrap(), 93), &h.owner);
    let permit = actor.admit(lease.clone(), h.proof(&lease.body,93), Some(&h.fresh()), &p.operation().unwrap(), now()).unwrap();
    actor.debit_tunnel(&permit,&p,&[1;16],now()).unwrap();
    drop(permit); drop(actor); // Drop is NOT clean-drain authority or a refund.
    let mut restored = root.open(&h);
    let revision = restored.snapshot().unwrap().revision();
    assert!(matches!(restored.resume_tunnel(lease.clone(), h.proof(&lease.body,94), &h.fresh(), &p, now()),Err(Error::WrongState)));
    assert_eq!(restored.snapshot().unwrap().revision(),revision);
    assert_eq!(restored.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited().0,16);
}

#[test]
fn tunnel_resume_legacy_v1_no_optin_and_same_lease_readmit_never_authorize() {
    let mut h=enrolled_resume(); let p=resume_policy(&h); let v1=p.route.clone();
    let before=serde_json::to_vec(&v1).unwrap();
    let lease=sign(h.lease(v1.operation().unwrap(),95),&h.owner);
    let permit=h.actor.admit(lease.clone(),h.proof(&lease.body,95),Some(&h.fresh()),&v1.operation().unwrap(),now()).unwrap();
    h.actor.retire_tunnel_after_owned_drain(permit,now()).unwrap();
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,96),&h.fresh(),&p,now()),Err(Error::ScopeDenied)));
    assert!(matches!(h.actor.admit(lease.clone(),h.proof(&lease.body,96),Some(&h.fresh()),&v1.operation().unwrap(),now()),Err(Error::Replay)));
    assert_eq!(serde_json::to_vec(&v1).unwrap(),before);
    let opted_digest=p.digest().unwrap();
    let mut no_optin=p;no_optin.resume_original=false;
    assert_ne!(no_optin.digest().unwrap(),opted_digest);
}

#[test]
fn tunnel_resume_fresh_nonce_is_consumed_and_old_generation_never_revives() {
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,first)=original(&mut h,&p,97);
    h.actor.retire_tunnel_after_owned_drain(first,now()).unwrap();
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,97),&h.fresh(),&p,now()),Err(Error::Replay)));
    let next=h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,98),&h.fresh(),&p,now()).unwrap();
    h.actor.check_permit(&next,now()).unwrap();
    h.actor.retire_tunnel_after_owned_drain(next,now()).unwrap();
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,98),&h.fresh(),&p,now()),Err(Error::Replay)));
    let last=h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,99),&h.fresh(),&p,now()).unwrap();
    assert_eq!(h.actor.snapshot().unwrap().capacity().consumed_leases,1);
    assert_eq!(h.actor.debit_tunnel(&last,&p,&[],now()),Err(Error::Quota));
    assert_eq!(h.actor.debit_tunnel(&last,&p,&[0;65],now()),Err(Error::Quota));
}

#[test]
fn tunnel_resume_foreign_device_policy_lease_signature_and_node_key_are_denied() {
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,permit)=original(&mut h,&p,100);
    h.actor.retire_tunnel_after_owned_drain(permit,now()).unwrap();
    let revision=h.actor.snapshot().unwrap().revision();
    let mut other=p.clone();other.route.client_device_id=uuid(999);
    assert!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,101),&h.fresh(),&other,now()).is_err());
    let mut other=p.clone();other.route.destination.port+=1;
    assert!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,101),&h.fresh(),&other,now()).is_err());
    let mut changed=lease.clone();changed.signature=SignatureBytes::from_bytes([0;64]);
    assert!(h.actor.resume_tunnel(changed,h.proof(&lease.body,101),&h.fresh(),&p,now()).is_err());
    let wrong=sign(h.proof(&lease.body,101).body,&key(23));
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),wrong,&h.fresh(),&p,now()),Err(Error::InvalidSignature)));
    assert_eq!(h.actor.snapshot().unwrap().revision(),revision);
}

#[test]
fn tunnel_resume_expired_revoked_and_checkpoint_changed_original_is_denied() {
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,permit)=original(&mut h,&p,102);
    h.actor.retire_tunnel_after_owned_drain(permit,now()).unwrap();
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,103),&h.fresh(),&p,Now(time(1120))),Err(Error::Expired)));
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,permit)=original(&mut h,&p,104);
    h.actor.retire_tunnel_after_owned_drain(permit,now()).unwrap();
    h.actor.revoke(sign(Revocation{header:header(3),agent_id:h.cert.agent_id,key_epoch:1,
        certificate_hash:h.cert.digest().unwrap(),issued_at:time(1000)},&h.owner),now()).unwrap();
    assert!(matches!(h.actor.resume_tunnel(lease.clone(),h.proof(&lease.body,105),&h.fresh(),&p,now()),Err(Error::WrongCheckpoint)));
}

#[test]
fn tunnel_debit_uncertain_commit_returns_no_forward_credit_and_never_refunds() {
    #[derive(Clone)] struct Uncertain(Memory);
    impl CompareAndSwap for Uncertain {
        fn load(&self)->Result<State>{self.0.load()}
        fn compare_and_swap(&mut self,r:u64,next:State)->Result<()>{self.0.compare_and_swap(r,next)?;Err(Error::UncertainCommit)}
    }
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,permit)=original(&mut h,&p,106);
    let mut uncertain=Actor::new(Uncertain(h.memory.clone())).unwrap();
    assert_eq!(uncertain.debit_tunnel(&permit,&p,&[4;64],now()),Err(Error::UncertainCommit));
    let restored=Actor::new(h.memory.clone()).unwrap();
    assert_eq!(restored.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited().0,64);
    // No forwarding success/copy is manufactured. Caller has no refund API.
    drop(permit);
}

#[test]
fn tunnel_original_node_purpose_rejects_wrong_v2_binding_before_node_io() {
    let h=enrolled_resume();let p=resume_policy(&h);
    let lease=sign(h.lease(p.operation().unwrap(),107),&h.owner);
    let binding=NodeRequestBinding{certificate:sign(h.cert.clone(),&h.owner),lease,
        freshness:h.fresh(),operation:p.route.operation().unwrap()}; // v1 hash, not v2.
    assert!(matches!(create_original_tunnel_node_request(std::path::Path::new("/must-not-open-node"),
        h.cert.agent_id,h.actor.snapshot().unwrap().owner(),binding,&p,now()),Err(Error::ScopeDenied)));
}

// These closed-drain UNIT fixtures exercise Actor/journal time, not physical I/O.
#[test]
fn tunnel_original_idle_resume_and_control_do_not_refresh_after_restart() {
    let h = enrolled_resume(); let p = resume_policy(&h); let root = DurableTemp::new();
    let mut a = enroll_disk(&root, &h);
    let lease = sign(h.lease(p.operation().unwrap(), 110), &h.owner);
    let permit = a.admit(lease.clone(), h.proof(&lease.body,110), Some(&h.fresh()), &p.operation().unwrap(), now()).unwrap();
    assert_eq!(a.original_tunnel_window(&permit,&p,now()).unwrap(),(lease.body.expires_at,time(1015)));
    a.retire_tunnel_after_owned_drain(permit,Now(time(1005))).unwrap(); drop(a);
    let mut a = root.open(&h);
    let permit = a.resume_tunnel(lease.clone(),h.proof(&lease.body,111),&h.fresh(),&p,Now(time(1014))).unwrap();
    assert_eq!(a.original_tunnel_window(&permit,&p,Now(time(1014))).unwrap().1,time(1015));
    a.retire_tunnel_after_owned_drain(permit,Now(time(1014))).unwrap(); drop(a);
    let mut a = root.open(&h); let revision = a.snapshot().unwrap().revision();
    assert!(matches!(a.resume_tunnel(lease.clone(),h.proof(&lease.body,112),&h.fresh(),&p,Now(time(1015))),Err(Error::Expired)));
    assert_eq!(a.snapshot().unwrap().revision(),revision);
}
#[test]
fn tunnel_original_idle_only_committed_data_advances_window_and_backward_denied() {
    let h = enrolled_resume(); let p = resume_policy(&h); let root = DurableTemp::new();
    let mut a = enroll_disk(&root, &h);
    let lease = sign(h.lease(p.operation().unwrap(), 113), &h.owner);
    let permit = a.admit(lease.clone(),h.proof(&lease.body,113),Some(&h.fresh()),&p.operation().unwrap(),now()).unwrap();
    a.debit_tunnel(&permit,&p,&[1],Now(time(1010))).unwrap();
    assert_eq!(a.original_tunnel_window(&permit,&p,Now(time(1010))).unwrap().1,time(1025));
    assert_eq!(a.original_tunnel_window(&permit,&p,Now(time(1009))),Err(Error::InvalidTime));
    a.retire_tunnel_after_owned_drain(permit,Now(time(1011))).unwrap(); drop(a);
    let mut a = root.open(&h);
    let permit = a.resume_tunnel(lease.clone(),h.proof(&lease.body,114),&h.fresh(),&p,Now(time(1024))).unwrap();
    assert_eq!(a.original_tunnel_window(&permit,&p,Now(time(1024))).unwrap().1,time(1025));
    assert_eq!(a.original_tunnel_window(&permit,&p,Now(time(1025))),Err(Error::Expired));
}
#[test]
fn tunnel_original_idle_window_never_extends_original_expiry_or_overflows() {
    let mut h = enrolled_resume(); let mut p = resume_policy(&h);
    p.route.limits.max_ttl_seconds = 120; p.route.limits.idle_timeout_seconds = 30;
    let mut body = h.lease(p.operation().unwrap(),115); body.expires_at = time(1020);
    let lease = sign(body,&h.owner);
    let permit = h.actor.admit(lease.clone(),h.proof(&lease.body,115),Some(&h.fresh()),&p.operation().unwrap(),now()).unwrap();
    h.actor.debit_tunnel(&permit,&p,&[1],Now(time(1001))).unwrap();
    assert_eq!(h.actor.original_tunnel_window(&permit,&p,Now(time(1001))).unwrap().1,lease.body.expires_at);
    assert_eq!(h.actor.original_tunnel_window(&permit,&p,Now(lease.body.expires_at)),Err(Error::Expired));
    // The Timestamp constructor/addition remains the original checked type.
    assert!(Timestamp::new(u64::MAX).and_then(|t|t.checked_add(1)).is_err());
}


#[test]
fn tunnel_transport_preview_never_consumes_lease_nonce_or_journal_revision() {
    let mut h=enrolled_resume();let policy=resume_policy(&h);
    let lease=sign(h.lease(policy.operation().unwrap(),121),&h.owner);
    let binding=NodeRequestBinding {certificate:sign(h.cert.clone(),&h.owner),lease:lease.clone(),
        freshness:h.fresh(),operation:policy.operation().unwrap()};
    let before=h.actor.snapshot().unwrap();
    h.actor.preview_original_tunnel_binding(&binding,&policy,now()).unwrap();
    h.actor.preview_original_tunnel_binding(&binding,&policy,now()).unwrap();
    let after=h.actor.snapshot().unwrap();
    assert_eq!(after.revision(),before.revision());
    assert_eq!(after.capacity().consumed_leases,before.capacity().consumed_leases);
    assert_eq!(after.capacity().consumed_nonces,before.capacity().consumed_nonces);
    let proof=h.proof(&lease.body,121);
    h.actor.admit(lease.clone(),proof,Some(&binding.freshness),&binding.operation,now()).unwrap();
    assert_eq!(h.actor.preview_original_tunnel_binding(&binding,&policy,now()),Err(Error::Replay));
}
#[test]
fn tunnel_transport_preview_checks_exact_policy_current_certificate_and_signatures() {
    let mut h=enrolled_resume();let policy=resume_policy(&h);
    let binding=NodeRequestBinding {certificate:sign(h.cert.clone(),&h.owner),
        lease:sign(h.lease(policy.operation().unwrap(),122),&h.owner),freshness:h.fresh(),operation:policy.operation().unwrap()};
    let before=h.actor.snapshot().unwrap().revision();
    let mut changed=policy.clone();changed.route.destination.port+=1;
    assert!(h.actor.preview_original_tunnel_binding(&binding,&changed,now()).is_err());
    let mut bad=binding.clone();bad.lease.signature=SignatureBytes::from_bytes([0;64]);
    assert_eq!(h.actor.preview_original_tunnel_binding(&bad,&policy,now()),Err(Error::InvalidSignature));
    let mut bad=binding.clone();bad.certificate.body.key_epoch=2;
    assert!(h.actor.preview_original_tunnel_binding(&bad,&policy,now()).is_err());
    assert_eq!(h.actor.snapshot().unwrap().revision(),before);
}
#[test]
fn tunnel_transport_preview_does_not_ignore_revocation_checkpoint_or_original_expiry() {
    let mut h=enrolled_resume();let policy=resume_policy(&h);
    let binding=NodeRequestBinding {certificate:sign(h.cert.clone(),&h.owner),
        lease:sign(h.lease(policy.operation().unwrap(),123),&h.owner),freshness:h.fresh(),operation:policy.operation().unwrap()};
    assert!(h.actor.preview_original_tunnel_binding(&binding,&policy,Now(time(1120))).is_err());
    h.actor.revoke(sign(Revocation {header:header(3),agent_id:h.cert.agent_id,key_epoch:1,
        certificate_hash:h.cert.digest().unwrap(),issued_at:time(1000)},&h.owner),now()).unwrap();
    assert_eq!(h.actor.preview_original_tunnel_binding(&binding,&policy,now()),Err(Error::WrongCheckpoint));
}

#[test]
fn tunnel_signed_false_first_admit_debits_and_drain_never_enable_resume() {
    let mut h = enrolled_resume();
    let mut p = resume_policy(&h); p.resume_original = false;
    let (lease, permit) = original(&mut h, &p, 171);
    h.actor.debit_tunnel(&permit, &p, &[3;16], now()).unwrap();
    h.actor.retire_tunnel_after_owned_drain(permit, now()).unwrap();
    let revision = h.actor.snapshot().unwrap().revision();
    let proof = h.proof(&lease.body, 172);
    assert!(matches!(h.actor.resume_tunnel(lease.clone(), proof, &h.fresh(), &p, now()), Err(Error::ScopeDenied)));
    assert_eq!(h.actor.snapshot().unwrap().revision(), revision);
    let mut retargeted = p; retargeted.resume_original = true;
    let proof = h.proof(&lease.body, 173);
    assert!(matches!(h.actor.resume_tunnel(lease.clone(), proof, &h.fresh(), &retargeted, now()), Err(Error::ScopeDenied)));
    assert_eq!(h.actor.snapshot().unwrap().revision(), revision);
    assert_eq!(h.actor.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited(), (16,16));
}

#[test]
fn product_resume_preview_is_readonly_exact_consumed_binding_and_original_idle() {
    let mut h=enrolled_resume();let p=resume_policy(&h);
    let (lease,permit)=original(&mut h,&p,181);
    let binding=NodeRequestBinding {certificate:sign(h.cert.clone(),&h.owner),lease:lease.clone(),
        freshness:h.fresh(),operation:p.operation().unwrap()};
    assert!(matches!(h.actor.preview_original_tunnel_resume_binding(&binding,&p,now()),Err(Error::WrongState)));
    h.actor.debit_tunnel(&permit,&p,&[1;16],now()).unwrap();
    h.actor.retire_tunnel_after_owned_drain(permit,now()).unwrap(); // Closed-drain UNIT fixture, not transport proof.
    let before=h.actor.snapshot().unwrap();
    let window=h.actor.preview_original_tunnel_resume_binding(&binding,&p,Now(time(1001))).unwrap();
    assert_eq!(window.0,lease.body.expires_at);
    assert_eq!(window.1,time(1000+p.route.limits.idle_timeout_seconds));
    let after=h.actor.snapshot().unwrap();assert_eq!(before.revision(),after.revision());
    assert_eq!(after.capacity().consumed_leases,1);
    assert_eq!(after.tunnel_admissions[&lease.body.id].debited(),(16,16));
    let mut changed=binding.clone();changed.freshness.body.issued_at=time(1001);
    changed.freshness=sign(changed.freshness.body,&h.owner);
    assert!(matches!(h.actor.preview_original_tunnel_resume_binding(&changed,&p,Now(time(1001))),Err(Error::WrongClaim)));
    let mut changed=binding.clone();changed.lease.signature=SignatureBytes::from_bytes([0;64]);
    assert!(h.actor.preview_original_tunnel_resume_binding(&changed,&p,Now(time(1001))).is_err());
    let mut changed=p.clone();changed.resume_original=false;
    assert!(matches!(h.actor.preview_original_tunnel_resume_binding(&binding,&changed,now()),Err(Error::ScopeDenied)));
    assert_eq!(h.actor.snapshot().unwrap().revision(),before.revision());
}

#[test]
fn product_resume_preview_rebuilds_original_freshness_from_real_journal_replay() {
    let h=enrolled_resume();let p=resume_policy(&h);let root=DurableTemp::new();
    let mut actor=enroll_disk(&root,&h);let lease=sign(h.lease(p.operation().unwrap(),182),&h.owner);
    let binding=NodeRequestBinding{certificate:sign(h.cert.clone(),&h.owner),lease:lease.clone(),
        freshness:h.fresh(),operation:p.operation().unwrap()};
    let permit=actor.admit(lease.clone(),h.proof(&lease.body,182),Some(&binding.freshness),&binding.operation,now()).unwrap();
    actor.debit_tunnel(&permit,&p,&[2;16],now()).unwrap();
    actor.retire_tunnel_after_owned_drain(permit,now()).unwrap(); // Unit-only owned-drain fixture.
    let revision=actor.snapshot().unwrap().revision();drop(actor);
    let mut restored=root.open(&h);
    restored.preview_original_tunnel_resume_binding(&binding,&p,Now(time(1001))).unwrap();
    assert_eq!(restored.snapshot().unwrap().revision(),revision);
    assert!(matches!(restored.preview_original_tunnel_resume_binding(&binding,&p,
        Now(time(1000+p.route.limits.idle_timeout_seconds))),Err(Error::Expired)));
}

#[test]
fn product_retirement_uncertain_cas_is_not_a_success_observation_or_retry() {
    #[derive(Clone)] struct Uncertain(Memory);
    impl CompareAndSwap for Uncertain {
        fn load(&self)->Result<State>{self.0.load()}
        fn compare_and_swap(&mut self,r:u64,next:State)->Result<()> {
            self.0.compare_and_swap(r,next)?;Err(Error::UncertainCommit)
        }
    }
    let mut h=enrolled_resume();let p=resume_policy(&h);let (lease,permit)=original(&mut h,&p,183);
    let before=h.actor.snapshot().unwrap().revision();
    let mut uncertain=Actor::new(Uncertain(h.memory.clone())).unwrap();
    assert_eq!(uncertain.retire_tunnel_after_owned_drain(permit,now()),Err(Error::UncertainCommit));
    // Commit may actually exist after an unknown acknowledgement. This is not
    // a new uncertainty tombstone: product caller must lack transportRetired
    // and end Unknown, without a retry/reprobe to manufacture that observation.
    let mut restored=Actor::new(h.memory.clone()).unwrap();
    let binding=NodeRequestBinding{certificate:sign(h.cert.clone(),&h.owner),lease,
        freshness:h.fresh(),operation:p.operation().unwrap()};
    assert_eq!(restored.snapshot().unwrap().revision(),before+1);
    restored.preview_original_tunnel_resume_binding(&binding,&p,now()).unwrap();
}
