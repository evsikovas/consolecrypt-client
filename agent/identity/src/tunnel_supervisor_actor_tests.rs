// DRAFT actual owned TCP -> production metering helper -> real signed Actor.
// No TLS peer/Permit is faked; private closed-drain is ONLY the Actor unit seam.
use super::*;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

fn setup() -> (Harness, TunnelRoutePolicyV2, Signed<Lease>, Permit) {
    let mut h = Harness::offered(
        Scopes(vec![Scope::TunnelOpen]),
        Scopes(vec![Scope::TunnelOpen]),
    );
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    let mut route: TunnelRoutePolicy =
        serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap();
    route.context = context();
    route.agent_id = h.cert.agent_id;
    route.agent_certificate_hash = h.cert.digest().unwrap();
    route.limits.max_ttl_seconds = 120;
    route.limits.max_frame_bytes = 64;
    route.limits.max_bytes_per_channel = 64;
    route.limits.max_bytes_total = 128;
    let policy = TunnelRoutePolicyV2 {
        format: "cc-agent-tunnel-route-policy/v2".into(),
        route,
        resume_original: true,
    };
    let lease = sign(h.lease(policy.operation().unwrap(), 120), &h.owner);
    let permit = h
        .actor
        .admit(
            lease.clone(),
            h.proof(&lease.body, 120),
            Some(&h.fresh()),
            &policy.operation().unwrap(),
            now(),
        )
        .unwrap();
    (h, policy, lease, permit)
}
fn sockets() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let producer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (target, _) = listener.accept().unwrap();
    target
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    (producer, target)
}

#[test]
fn supervised_actual_target_read_debits_before_copy_and_restart_never_refunds_quota() {
    let (mut h, p, lease, permit) = setup();
    let (mut producer, mut target) = sockets();
    producer.write_all(&[7; 64]).unwrap();
    let mut copied = Vec::new();
    let mut buf = [0u8; 32];
    while copied.len() < 64 {
        let n = crate::tunnel_supervisor::debit_target(
            &mut h.actor,
            &permit,
            &p,
            &mut target,
            &mut buf,
            now(),
        )
        .unwrap()
        .unwrap();
        copied.extend_from_slice(&buf[..n]); // ONLY after actual durable debit success.
    }
    assert_eq!(copied, vec![7; 64]);
    producer.write_all(&[8]).unwrap();
    assert_eq!(
        crate::tunnel_supervisor::debit_target(
            &mut h.actor,
            &permit,
            &p,
            &mut target,
            &mut buf,
            now()
        ),
        Err(Error::Quota)
    );
    assert_eq!(copied.len(), 64); // no copy/forward after quota denial.
    target.shutdown(Shutdown::Both).unwrap();
    drop(target);
    drop(producer);
    // Actor-private UNIT drain fixture; not physical TLS/Child integration proof.
    h.actor
        .retire_tunnel_after_owned_drain(permit, now())
        .unwrap();
    let mut restarted = Actor::new(h.memory.clone()).unwrap();
    assert!(matches!(
        restarted.resume_tunnel(
            lease.clone(),
            h.proof(&lease.body, 121),
            &h.fresh(),
            &p,
            now()
        ),
        Err(Error::Quota)
    ));
    assert_eq!(
        restarted.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited(),
        (64, 64)
    );
}

#[test]
fn supervised_actual_expired_original_never_reads_target_or_renews_lease() {
    let (mut h, p, lease, permit) = setup();
    let (mut producer, mut target) = sockets();
    producer.write_all(b"expiry").unwrap();
    let mut buf = [0u8; 6];
    assert_eq!(
        crate::tunnel_supervisor::debit_target(
            &mut h.actor,
            &permit,
            &p,
            &mut target,
            &mut buf,
            Now(time(1120))
        ),
        Err(Error::Expired)
    );
    target.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"expiry"); // bytes remained unread by expired helper.
    assert_eq!(permit.expires_at(), lease.body.expires_at);
    assert_eq!(
        h.actor.snapshot().unwrap().tunnel_admissions[&lease.body.id].debited(),
        (0, 0)
    );
}
