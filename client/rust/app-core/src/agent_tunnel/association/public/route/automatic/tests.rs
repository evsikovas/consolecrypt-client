//! Causal source tests. Actual signed-route detach/attach is also exercised by
//! the existing owned public route test; these local tests confer no grant.
use super::*;
use std::sync::atomic::AtomicBool;

#[test]
fn an_old_detached_finalizer_cannot_cancel_a_new_owned_operation() {
    let old=Custody::new();old.begin_detach().unwrap().commit(1).unwrap();
    let attached=Custody::new();
    assert!(!old.release());assert!(!old.release());
    assert!(attached.release());assert!(!attached.release());
    assert!(old.begin_detach().is_err());assert!(old.begin_action().is_err());
    assert!(old.check_read().is_err());
}

#[test]
fn finalization_during_sql_validation_prevents_a_late_detach_commit() {
    let custody=Custody::new();let pending=custody.begin_detach().unwrap();
    assert!(custody.release());assert!(pending.commit(1).is_err());
    assert!(custody.begin_detach().is_err());assert!(custody.begin_action().is_err());
}

#[test]
fn abandoning_a_detach_restores_only_the_same_unreleased_owner() {
    let custody=Custody::new();{let _pending=custody.begin_detach().unwrap();}
    assert!(custody.release());assert!(!custody.release());
}

#[test]
fn an_inflight_mutation_excludes_handoff_and_cannot_restore_released_rights() {
    let custody=Custody::new();let action=custody.begin_action().unwrap();
    assert!(custody.begin_detach().is_err());assert!(custody.begin_action().is_err());
    assert!(custody.release());drop(action);
    assert!(custody.begin_detach().is_err());assert!(custody.begin_action().is_err());
}

#[test]
fn explicit_cancel_after_native_commit_withdraws_only_the_unclaimed_ticket() {
    let tickets=Tickets::new();let old=Custody::new();let ticket=tickets.allocate().unwrap();
    old.begin_detach().unwrap().commit(ticket).unwrap();
    tickets.available.store(ticket,Ordering::SeqCst);
    assert!(old.withdraw_ticket(&tickets));assert!(!tickets.take(ticket));
    assert!(old.check_read().is_ok());assert!(old.begin_action().is_err());
    assert!(!old.release());assert!(!old.withdraw_ticket(&tickets));
}

#[test]
fn a_new_claim_winning_before_old_cancel_preserves_the_new_owner() {
    let tickets=Tickets::new();let old=Custody::new();let ticket=tickets.allocate().unwrap();
    old.begin_detach().unwrap().commit(ticket).unwrap();
    tickets.available.store(ticket,Ordering::SeqCst);
    assert!(tickets.take(ticket));assert!(!old.withdraw_ticket(&tickets));
    assert!(!old.release());assert!(Custody::new().release());
}

#[test]
fn an_old_ticket_cannot_withdraw_or_claim_a_later_handoff_of_the_same_route() {
    let tickets=Tickets::new();let old=Custody::new();let first=tickets.allocate().unwrap();
    old.begin_detach().unwrap().commit(first).unwrap();
    tickets.available.store(first,Ordering::SeqCst);assert!(tickets.take(first));
    let new=Custody::new();let second=tickets.allocate().unwrap();assert_ne!(first,second);
    new.begin_detach().unwrap().commit(second).unwrap();tickets.available.store(second,Ordering::SeqCst);
    assert!(!old.withdraw_ticket(&tickets));assert!(!tickets.take(first));
    assert_eq!(tickets.available.load(Ordering::SeqCst),second);
    assert!(new.withdraw_ticket(&tickets));
}

#[test]
fn cancellation_during_detaching_never_publishes_or_commits_a_ticket() {
    let tickets=Tickets::new();let owner=Custody::new();let ticket=tickets.allocate().unwrap();
    let pending=owner.begin_detach().unwrap();assert!(owner.release());
    assert!(pending.commit(ticket).is_err());
    assert_eq!(tickets.available.load(Ordering::SeqCst),0);
    assert!(!owner.withdraw_ticket(&tickets));
}

fn request()->Request {
    Request{version:1,profile_id:"11111111-1111-4111-8111-111111111111".into(),
        notification_scope_id:"original-notice".into(),
        association_id:"22222222-2222-4222-8222-222222222222".into(),
        listen_ip:"127.0.0.1".into(),listen_port:0,destination_ip:"127.0.0.1".into(),
        destination_port:22,ttl_seconds:30,idle_seconds:10,resume_original:true}
}

#[test]
fn every_destination_listener_and_review_limit_selects_a_distinct_original_operation() {
    let original=Key::from_request(&request());
    for field in 0..7 {
        let mut changed=request();
        match field {
            0=>changed.association_id="33333333-3333-4333-8333-333333333333".into(),
            1=>changed.listen_ip="::1".into(),2=>changed.listen_port=2222,
            3=>changed.destination_ip="::1".into(),4=>changed.destination_port=23,
            5=>changed.ttl_seconds=31,_=>changed.idle_seconds=11,
        }
        changed.validate().unwrap();assert_ne!(original,Key::from_request(&changed));
    }
}

#[test]
fn the_index_cannot_hold_a_parent_alive_or_resurrect_one_after_drop() {
    struct Owner(Arc<AtomicBool>);
    impl Drop for Owner {fn drop(&mut self){self.0.store(true,Ordering::SeqCst)}}
    let dropped=Arc::new(AtomicBool::new(false));
    let owner=Arc::new(Owner(dropped.clone()));let mut registry=Index::new();
    registry.insert(&owner).unwrap();registry.insert(&owner).unwrap();
    assert_eq!(registry.entries.len(),1);assert_eq!(Arc::strong_count(&owner),1);
    let weak=registry.entries[0].clone();drop(owner);
    assert!(dropped.load(Ordering::SeqCst));assert!(weak.upgrade().is_none());
    registry.prune();assert!(registry.entries.is_empty());
}

#[test]
fn the_finite_index_refuses_overflow_and_only_dead_entries_release_capacity() {
    let mut owners:Vec<_>=(0..MAX_RETAINED).map(Arc::new).collect();
    let mut registry=Index::new();for owner in &owners {registry.insert(owner).unwrap()}
    let extra=Arc::new(MAX_RETAINED);
    assert!(registry.insert(&extra).is_err());assert_eq!(registry.entries.len(),MAX_RETAINED);
    owners.pop();registry.insert(&extra).unwrap();assert_eq!(registry.entries.len(),MAX_RETAINED);
}

const PASSWORD:&str="owned retained original tunnel test password 42";
async fn local()->(tempfile::TempDir,AppCore,Frame) {
    let root=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Retained original test".into(),PASSWORD.into()).await.unwrap();
    let(session,unlocked)=app.unlocked().await.unwrap();
    let frame=Frame{version:1,profile_id:session.profile_id.to_string(),
        notification_scope_id:unlocked.notices.scope_id().unwrap()};
    (root,app,frame)
}

#[tokio::test]
async fn a_fresh_ui_capture_matches_actual_parents_without_replacing_the_original_deadline() {
    let(_root,app,frame)=local().await;
    let original=Context::capture(&app,frame.clone()).await.unwrap();
    let deadline=original.scope.deadline();
    let fresh=Context::capture(&app,frame).await.unwrap();
    assert!(original.scope.same_current_capture(&fresh.scope).unwrap());
    assert_eq!(original.scope.deadline(),deadline);
    assert!(!Arc::ptr_eq(&original.scope,&fresh.scope));
    app.shutdown().await.unwrap();
    assert!(original.scope.same_current_capture(&fresh.scope).is_err());
}

#[tokio::test]
async fn re_unlocking_the_same_profile_cannot_restore_an_old_capture() {
    let(_root,app,frame)=local().await;
    let original=Context::capture(&app,frame).await.unwrap();
    app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let(session,unlocked)=app.unlocked().await.unwrap();
    let frame=Frame{version:1,profile_id:session.profile_id.to_string(),
        notification_scope_id:unlocked.notices.scope_id().unwrap()};
    let fresh=Context::capture(&app,frame).await.unwrap();
    assert!(original.scope.same_current_capture(&fresh.scope).is_err());
    drop(unlocked);drop(session);app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_saved_intent_without_an_original_held_operation_never_creates_a_review_or_listener() {
    let(_root,app,frame)=local().await;
    let mut intent=request();intent.profile_id=frame.profile_id.clone();
    intent.notification_scope_id=frame.notification_scope_id.clone();
    for opted in [false,true] {
        intent.resume_original=opted;
        let context=Context::capture(&app,frame.clone()).await.unwrap();
        assert!(attach(context,&intent).await.unwrap().is_none());
    }
    app.shutdown().await.unwrap();
}


#[tokio::test]
async fn blocked_then_transport_eof_waits_for_verified_original_revocation_completion() { super::revocation_completion_cases::blocked_then_transport_eof_waits_for_verified_original_revocation_completion().await; }

#[tokio::test]
async fn failed_or_lost_original_revoke_completion_after_eof_stays_unknown() { super::revocation_completion_cases::failed_or_lost_original_revoke_completion_after_eof_stays_unknown().await; }

#[tokio::test]
async fn original_revocation_cause_cannot_extend_deadline_or_cross_unlock_generation() { super::revocation_completion_cases::original_revocation_cause_cannot_extend_deadline_or_cross_unlock_generation().await; }
