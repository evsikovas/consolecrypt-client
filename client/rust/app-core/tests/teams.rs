//! Teams (protocol 1.9) against the in-process mock server — TZ teams
//! acceptance criteria 1–6: create a team, a second own device opens it,
//! invite + join with a compared member code, a rejected code adds nobody,
//! viewers cannot write, removal rotates the key, and a member the server
//! adds without a signed log entry is refused.

mod common;

use cc_app_core::team_dto::*;
use cc_app_core::*;
use cc_crypto_core::teams::TeamIdentity;
use cc_protocol::{UserId, VaultId};
use cc_sync_core::mock::MockServer;
use common::*;
use std::path::Path;
use std::str::FromStr;

async fn account(dir: &Path, url: &str, email: &str, mode: AccountMode) -> AppCore {
    let app = common::app(dir);
    app.create_synced_profile(
        "Work".into(),
        url.into(),
        email.into(),
        ACCOUNT_PASSWORD.into(),
        mode,
    )
    .await
    .unwrap();
    app
}

async fn team_of(app: &AppCore, vault_id: &str) -> Option<TeamDto> {
    app.teams()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.vault_id == vault_id)
}

async fn host_names(app: &AppCore) -> Vec<String> {
    let mut v: Vec<String> = app
        .list_hosts()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|h| h.name)
        .collect();
    v.sort();
    v
}

/// Invite `email` as `role`, accept on `invitee`, compare codes, confirm.
async fn join(
    admin: &AppCore,
    invitee: &AppCore,
    team: &str,
    email: &str,
    role: TeamRoleDto,
) -> String {
    admin
        .invite_team_member(team.into(), email.into(), role, Some("ops".into()))
        .await
        .unwrap();
    let incoming = invitee.incoming_team_invitations().await.unwrap();
    let inv = incoming.iter().find(|i| i.vault_id == team).unwrap();
    assert_eq!(inv.note.as_deref(), Some("ops"));
    let joined = invitee
        .accept_team_invitation(inv.invitation_id.clone())
        .await
        .unwrap();
    assert!(team_of(invitee, team).await.unwrap().pending);
    let requests = admin.team_join_requests(team.into()).await.unwrap();
    let req = requests
        .iter()
        .find(|r| r.invitation_id == joined.invitation_id)
        .unwrap();
    assert_eq!(req.code, joined.code, "both screens show the same code");
    assert_eq!(req.code.split(' ').count(), 6);
    admin
        .confirm_team_member(team.into(), joined.invitation_id.clone())
        .await
        .unwrap();
    invitee.refresh_teams().await.unwrap();
    let t = team_of(invitee, team).await.unwrap();
    assert!(!t.pending && t.open, "{t:?}");
    assert_eq!(t.role, Some(role));
    joined.invitation_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn teams_lifecycle() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();

    // ── criterion 1: Alice creates a team; her second device opens it ────
    let alice = account(
        &tmp.path().join("a1"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    assert!(alice.teams_supported().await.unwrap());
    let team = alice
        .create_team(
            "Ops".into(),
            Some("#3366ff".into()),
            Some("rocket".into()),
            Some("Production access".into()),
        )
        .await
        .unwrap();
    assert_eq!(team.name, "Ops");
    assert_eq!(team.color.as_deref(), Some("#3366ff"));
    assert_eq!(team.role, Some(TeamRoleDto::Owner));
    assert_eq!(team.members_count, 1);
    let tid = team.vault_id.clone();
    let vid = VaultId::from_str(&tid).unwrap();
    let group = alice
        .create_team_group(tid.clone(), GroupDto::new("prod"))
        .await
        .unwrap();
    let mut web = host("web", "192.0.2.10", None);
    web.group_id = Some(group.id.clone());
    let web = alice.save_host(web).await.unwrap();
    alice
        .save_host(host("personal", "192.0.2.99", None))
        .await
        .unwrap();
    let origins = alice.object_vaults().await.unwrap();
    assert!(origins
        .iter()
        .any(|o| o.object_id == web.id && o.vault_id == tid));
    assert!(origins.iter().any(|o| o.object_id == group.id));
    assert_eq!(origins.len(), 2, "the personal host stays personal");
    alice.sync_now().await.unwrap();
    alice.refresh_teams().await.unwrap();
    wait_until!(
        10,
        "team objects uploaded",
        server.live_objects(vid).len() >= 3
    );

    let alice2 = common::app(&tmp.path().join("a2"));
    let vaults = alice2
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "alice@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap()
        .vaults;
    assert_eq!(vaults.len(), 1, "team vaults are not personal vaults");
    alice2
        .join_vault_with_passphrase(vaults[0].vault_id.clone(), PASSPHRASE.into())
        .await
        .unwrap();
    alice2.sync_now().await.unwrap();
    alice2.refresh_teams().await.unwrap();
    let on2 = team_of(&alice2, &tid)
        .await
        .expect("team on the second device");
    assert_eq!(on2.name, "Ops");
    assert_eq!(on2.role, Some(TeamRoleDto::Owner));
    wait_until!(
        10,
        "team host on device 2",
        host_names(&alice2).await.contains(&"web".to_string())
    );

    // ── criterion 7 (core): a personal colour does not change the team ──
    let t = alice2
        .set_team_preferences(tid.clone(), Some("#00aa00".into()), 3, false)
        .await
        .unwrap();
    assert_eq!(t.color.as_deref(), Some("#00aa00"));
    assert_eq!(t.team_color.as_deref(), Some("#3366ff"));

    // ── criterion 2: Bob joins with the compared code, edits flow ───────
    let bob = account(
        &tmp.path().join("b"),
        &url,
        "bob@example.org",
        AccountMode::Register,
    )
    .await;
    bob.create_vault(PASSPHRASE.into()).await.unwrap();
    join(&alice, &bob, &tid, "bob@example.org", TeamRoleDto::Editor).await;
    wait_until!(
        10,
        "Bob sees the team host",
        host_names(&bob).await.contains(&"web".to_string())
    );
    assert!(!host_names(&bob).await.contains(&"personal".to_string()));
    let mut api = host("api", "192.0.2.11", None);
    api.group_id = Some(group.id.clone());
    bob.save_host(api).await.unwrap();
    bob.sync_now().await.unwrap();
    bob.refresh_teams().await.unwrap();
    alice.refresh_teams().await.unwrap();
    wait_until!(
        10,
        "Alice sees Bob's host",
        host_names(&alice).await.contains(&"api".to_string())
    );
    let members = alice.team_members(tid.clone()).await.unwrap();
    assert_eq!(members.len(), 2);
    assert!(members
        .iter()
        .any(|m| m.email == "bob@example.org" && m.role == TeamRoleDto::Editor));

    // ── criterion 3: codes that do not match add nobody ─────────────────
    let carol = account(
        &tmp.path().join("c"),
        &url,
        "carol@example.org",
        AccountMode::Register,
    )
    .await;
    carol.create_vault(PASSPHRASE.into()).await.unwrap();
    alice
        .invite_team_member(
            tid.clone(),
            "carol@example.org".into(),
            TeamRoleDto::Viewer,
            None,
        )
        .await
        .unwrap();
    let inv = carol.incoming_team_invitations().await.unwrap().remove(0);
    let joined = carol
        .accept_team_invitation(inv.invitation_id.clone())
        .await
        .unwrap();
    // "Codes do not match" → reject.
    alice
        .close_team_invitation(joined.invitation_id.clone())
        .await
        .unwrap();
    carol.refresh_teams().await.unwrap();
    let t = team_of(&carol, &tid).await;
    assert!(t.as_ref().is_none_or(|t| !t.open), "not a member: {t:?}");
    assert_eq!(alice.team_members(tid.clone()).await.unwrap().len(), 2);
    carol.close_team_invitation(joined.invitation_id).await.ok();

    // ── criterion 5: viewers cannot write ───────────────────────────────
    join(
        &alice,
        &carol,
        &tid,
        "carol@example.org",
        TeamRoleDto::Viewer,
    )
    .await;
    wait_until!(
        10,
        "Carol sees the team",
        host_names(&carol).await.contains(&"web".to_string())
    );
    let mut edit = carol.find_host("web".into()).await.unwrap();
    edit.notes = "viewer edit".into();
    assert_eq!(
        code(&carol.save_host(edit).await.unwrap_err()),
        "team_read_only"
    );
    let mut new = host("nope", "192.0.2.12", None);
    new.group_id = Some(group.id.clone());
    assert_eq!(
        code(&carol.save_host(new).await.unwrap_err()),
        "team_read_only"
    );
    assert_eq!(
        code(
            &carol
                .invite_team_member(
                    tid.clone(),
                    "x@example.org".into(),
                    TeamRoleDto::Viewer,
                    None
                )
                .await
                .unwrap_err()
        ),
        "team_read_only"
    );

    // ── roles: promote Carol, demote again ──────────────────────────────
    let carol_id = alice
        .team_members(tid.clone())
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.email == "carol@example.org")
        .unwrap()
        .user_id;
    alice
        .change_team_role(tid.clone(), carol_id.clone(), TeamRoleDto::Editor)
        .await
        .unwrap();
    carol.refresh_teams().await.unwrap();
    assert_eq!(
        team_of(&carol, &tid).await.unwrap().role,
        Some(TeamRoleDto::Editor)
    );

    // ── criterion 6: removing Bob rotates the key; Bob loses the team ────
    let bob_id = alice
        .team_members(tid.clone())
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.email == "bob@example.org")
        .unwrap()
        .user_id;
    let t = alice.remove_team_member(tid.clone(), bob_id).await.unwrap();
    assert_eq!(t.key_epoch, 1);
    assert!(!t.rotation_required);
    assert_eq!(t.members_count, 2);
    let mut db = host("db", "192.0.2.13", None);
    db.group_id = Some(group.id.clone());
    alice.save_host(db).await.unwrap();
    alice.sync_now().await.unwrap();
    bob.refresh_teams().await.unwrap();
    assert!(team_of(&bob, &tid).await.is_none(), "Bob lost the team");
    assert!(!host_names(&bob).await.contains(&"web".to_string()));
    carol.refresh_teams().await.unwrap();
    let ct = team_of(&carol, &tid).await.unwrap();
    assert_eq!(ct.key_epoch, 1);
    wait_until!(
        10,
        "Carol reads the new epoch",
        host_names(&carol).await.contains(&"db".to_string())
    );
    assert!(
        host_names(&carol).await.contains(&"web".to_string()),
        "old objects re-encrypted"
    );
    let log = alice.team_log(tid.clone()).await.unwrap();
    assert_eq!(log[0].op, "rotate_key");
    assert_eq!(log[1].op, "remove_member");
    assert_eq!(log.last().unwrap().op, "genesis");

    // A note about a team host is personal (other members never see it)
    // and outlives the team.
    let note = carol
        .save_note(NoteDto {
            title: "web: reload only".into(),
            target: Some(cc_models::note::NoteTarget::Object {
                object_kind: cc_models::note::NoteObjectKind::Host,
                id: web.id.parse().unwrap(),
            }),
            target_label: Some("web".into()),
            ..NoteDto::default()
        })
        .await
        .unwrap();
    assert!(
        !carol
            .object_vaults()
            .await
            .unwrap()
            .iter()
            .any(|o| o.object_id == note.id),
        "notes never enter a team vault"
    );

    // ── leave ───────────────────────────────────────────────────────────
    carol.leave_team(tid.clone()).await.unwrap();
    assert!(team_of(&carol, &tid).await.is_none());
    assert!(!host_names(&carol).await.contains(&"web".to_string()));
    let kept = carol
        .list_notes()
        .await
        .unwrap()
        .into_iter()
        .find(|n| n.id == note.id)
        .expect("the note outlives the team");
    assert!(
        kept.target.is_some(),
        "kept with its old target, shown as not linked"
    );
    assert_eq!(kept.target_label.as_deref(), Some("web"));

    // ── criterion 4: a member added by the server alone is refused ──────
    let forged = TeamIdentity::generate().unwrap().member_keys(UserId::new());
    assert!(server.inject_team_member(vid, forged));
    alice.refresh_teams().await.unwrap();
    let t = team_of(&alice, &tid).await.unwrap();
    assert_eq!(t.warning.as_deref(), Some("invalid_log"));
    assert_eq!(t.members_count, 1, "the forged member is not trusted");
    let mut more = host("more", "192.0.2.14", None);
    more.group_id = Some(group.id.clone());
    assert_eq!(
        code(&alice.save_host(more).await.unwrap_err()),
        "team_untrusted"
    );
    assert_eq!(
        code(&alice.rotate_team_key(tid.clone()).await.unwrap_err()),
        "team_untrusted"
    );
    // Reads keep working.
    assert!(host_names(&alice).await.contains(&"web".to_string()));

    // Stop sync engines, WebSockets and storage before the runtime ends.
    for app in [&alice, &alice2, &bob, &carol] {
        app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copy_and_move_into_a_team() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let alice = account(
        &tmp.path().join("a"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    let team = alice
        .create_team("Ops".into(), None, None, None)
        .await
        .unwrap();
    let tid = team.vault_id.clone();
    let cred = alice
        .add_password_credential("root".into(), Some("root".into()), "pw".into())
        .await
        .unwrap();
    let h = alice
        .save_host(host("web", "192.0.2.10", Some(&cred.id)))
        .await
        .unwrap();

    // A team host may not point at a personal credential.
    let g = alice
        .create_team_group(tid.clone(), GroupDto::new("prod"))
        .await
        .unwrap();
    let mut cross = host("cross", "192.0.2.20", Some(&cred.id));
    cross.group_id = Some(g.id.clone());
    assert_eq!(
        code(&alice.save_host(cross).await.unwrap_err()),
        "invalid_input"
    );

    // Copy: the credential and its secret come along.
    let copied = alice
        .copy_to_vault(vec![h.id.clone()], Some(tid.clone()))
        .await
        .unwrap();
    assert_ne!(copied[0], h.id);
    let origins = alice.object_vaults().await.unwrap();
    let in_team: Vec<_> = origins.iter().filter(|o| o.vault_id == tid).collect();
    assert_eq!(
        in_team.len(),
        4,
        "group + host + credential + secret: {origins:?}"
    );
    let copy = alice.get_host(copied[0].clone()).await.unwrap();
    assert_ne!(copy.credential_id, h.credential_id);
    assert!(origins
        .iter()
        .any(|o| Some(&o.object_id) == copy.credential_id.as_ref()));

    // Move: the original disappears from the personal vault.
    let moved = alice
        .move_to_vault(vec![h.id.clone()], Some(tid.clone()))
        .await
        .unwrap();
    assert!(alice.get_host(h.id.clone()).await.is_err());
    assert!(alice.get_host(moved[0].clone()).await.is_ok());
    assert_eq!(host_names(&alice).await, vec!["web", "web"]);
    alice.shutdown().await.unwrap();
}

/// Security review SEC-01: the admin's confirmation seals the team key only
/// to the keys (and role) the compared member code was computed from — not
/// to whatever the server reports for the invitation at confirmation time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn confirm_seals_only_to_the_keys_whose_code_was_compared() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let alice = account(
        &tmp.path().join("a"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    let bob = account(
        &tmp.path().join("b"),
        &url,
        "bob@example.org",
        AccountMode::Register,
    )
    .await;
    bob.create_vault(PASSPHRASE.into()).await.unwrap();
    let team = alice
        .create_team("Ops".into(), None, None, None)
        .await
        .unwrap();
    let tid = team.vault_id.clone();

    // Confirming without having been shown a code is refused.
    alice
        .invite_team_member(
            tid.clone(),
            "bob@example.org".into(),
            TeamRoleDto::Viewer,
            None,
        )
        .await
        .unwrap();
    let inv = bob.incoming_team_invitations().await.unwrap().remove(0);
    let joined = bob
        .accept_team_invitation(inv.invitation_id.clone())
        .await
        .unwrap();
    assert_eq!(
        code(
            &alice
                .confirm_team_member(tid.clone(), joined.invitation_id.clone())
                .await
                .unwrap_err()
        ),
        "approval"
    );

    // The admin compares the code over Bob's real keys …
    let requests = alice.team_join_requests(tid.clone()).await.unwrap();
    let shown = requests
        .iter()
        .find(|r| r.invitation_id == joined.invitation_id)
        .unwrap();
    assert_eq!(shown.code, joined.code);

    // … then the server swaps in its own keys and a higher role.
    let bob_id = server.user_id("bob@example.org").unwrap();
    let forged = TeamIdentity::generate().unwrap().member_keys(bob_id);
    let inv_id = joined.invitation_id.parse().unwrap();
    assert!(server.swap_team_invitation_keys(
        inv_id,
        forged,
        Some(cc_protocol::vaults::VaultRole::Admin)
    ));
    // A later listing keeps showing the code that was compared.
    let again = alice.team_join_requests(tid.clone()).await.unwrap();
    let again = again
        .iter()
        .find(|r| r.invitation_id == joined.invitation_id)
        .unwrap();
    assert_eq!(again.code, joined.code);
    assert_eq!(again.role, TeamRoleDto::Viewer);

    let err = alice
        .confirm_team_member(tid.clone(), joined.invitation_id.clone())
        .await
        .unwrap_err();
    assert_eq!(code(&err), "approval", "{err:?}");
    assert_eq!(
        alice.team_members(tid.clone()).await.unwrap().len(),
        1,
        "nobody was added"
    );
    alice.shutdown().await.unwrap();
    bob.shutdown().await.unwrap();
}

/// Security review SEC-02: a joining member holds the team log to the admin
/// whose code it compared. A server that serves a team of its own — its own
/// genesis, an `AddMember` it signed, a key it chose — is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_joiner_refuses_a_team_the_server_made_up() {
    use cc_crypto_core::teams::team_entry_hash;
    use cc_protocol::teams::TeamLogOp;
    use cc_protocol::vaults::VaultRole;

    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let alice = account(
        &tmp.path().join("a"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    let bob = account(
        &tmp.path().join("b"),
        &url,
        "bob@example.org",
        AccountMode::Register,
    )
    .await;
    bob.create_vault(PASSPHRASE.into()).await.unwrap();
    let team = alice
        .create_team("Ops".into(), None, None, None)
        .await
        .unwrap();
    let tid = team.vault_id.clone();
    let vid = VaultId::from_str(&tid).unwrap();
    alice
        .invite_team_member(
            tid.clone(),
            "bob@example.org".into(),
            TeamRoleDto::Viewer,
            None,
        )
        .await
        .unwrap();
    let inv = bob.incoming_team_invitations().await.unwrap().remove(0);
    let joined = bob
        .accept_team_invitation(inv.invitation_id.clone())
        .await
        .unwrap();
    let bob_keys = server
        .team_invitation_keys(joined.invitation_id.parse().unwrap())
        .unwrap();

    // Instead of Alice confirming, the server makes up a team around Bob.
    let attacker = TeamIdentity::generate().unwrap();
    let attacker_user = UserId::new();
    let now = chrono::Utc::now().timestamp();
    let genesis = attacker
        .sign_entry(
            vid,
            attacker_user,
            None,
            now,
            TeamLogOp::Genesis {
                owner: attacker.member_keys(attacker_user),
            },
        )
        .unwrap();
    let add = attacker
        .sign_entry(
            vid,
            attacker_user,
            Some((0, team_entry_hash(&genesis))),
            now,
            TeamLogOp::AddMember {
                member: bob_keys.clone(),
                role: VaultRole::Editor,
            },
        )
        .unwrap();
    let fake_key = cc_vault_core::UnlockedVault::new_team(vid).unwrap();
    let envelope = fake_key.seal_for_member(&bob_keys).unwrap();
    assert!(server.forge_team(vid, vec![genesis, add], bob_keys, envelope));

    let _ = bob.refresh_teams().await;
    let t = team_of(&bob, &tid).await;
    assert!(
        t.as_ref().is_none_or(|t| !t.open),
        "a team the server made up must not open: {t:?}"
    );
    alice.shutdown().await.unwrap();
    bob.shutdown().await.unwrap();
}

/// Security review SEC-03: a member pins the team key it accepted. A server
/// that later hands out a key of its own for the same epoch is refused on
/// the next full open instead of silently taking the member's new writes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_refuses_a_swapped_team_key() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let alice = account(
        &tmp.path().join("a"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    let bob = account(
        &tmp.path().join("b"),
        &url,
        "bob@example.org",
        AccountMode::Register,
    )
    .await;
    bob.create_vault(PASSPHRASE.into()).await.unwrap();
    let team = alice
        .create_team("Ops".into(), None, None, None)
        .await
        .unwrap();
    let tid = team.vault_id.clone();
    let vid = VaultId::from_str(&tid).unwrap();
    let inv = join(&alice, &bob, &tid, "bob@example.org", TeamRoleDto::Editor).await;
    let bob_keys = server.team_invitation_keys(inv.parse().unwrap()).unwrap();
    assert!(team_of(&bob, &tid).await.unwrap().open);

    // Reopening with the same key works.
    bob.lock().await.unwrap();
    bob.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    bob.refresh_teams().await.unwrap();
    assert!(team_of(&bob, &tid).await.unwrap().open, "same key reopens");

    // The server swaps Bob's envelope for a key it chose, same epoch.
    let fake_key = cc_vault_core::UnlockedVault::new_team(vid).unwrap();
    assert!(server.plant_team_user_envelope(vid, fake_key.seal_for_member(&bob_keys).unwrap()));
    bob.lock().await.unwrap();
    bob.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let _ = bob.refresh_teams().await;
    let t = team_of(&bob, &tid).await;
    assert!(
        t.as_ref().is_none_or(|t| !t.open),
        "a swapped team key must not open: {t:?}"
    );
    alice.shutdown().await.unwrap();
    bob.shutdown().await.unwrap();
}

/// Security review SEC-03b (protocol 1.12): an admin vouches for the team
/// key with a signed commitment, so a server cannot hand a member who opens
/// the team for the first time a key of its own — neither next to the
/// admin's commitment nor by dropping it. A pre-1.12 server keeps working.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_joiner_opens_only_a_key_an_admin_vouched_for() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let alice = account(
        &tmp.path().join("a"),
        &url,
        "alice@example.org",
        AccountMode::Register,
    )
    .await;
    alice.create_vault(PASSPHRASE.into()).await.unwrap();
    // Bob keeps his secure store across restarts.
    let bob_dir = tmp.path().join("b");
    let store = memory_store();
    let bob = app_with(&bob_dir, store.clone(), false);
    bob.create_synced_profile(
        "Work".into(),
        url.clone(),
        "bob@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    bob.create_vault(PASSPHRASE.into()).await.unwrap();
    let profile = bob.active_profile().await.unwrap().unwrap().id;
    bob.shutdown().await.unwrap();
    let start_bob = || async {
        let bob = app_with(&bob_dir, store.clone(), false);
        bob.open_profile(profile.clone()).await.unwrap();
        bob.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
        bob
    };

    // Invite + accept with Bob running, confirm while he is away; returns
    // the keys Bob sent.
    let join_while_away = |tid: String| {
        let (server, alice, start_bob) = (&server, &alice, &start_bob);
        async move {
            alice
                .invite_team_member(
                    tid.clone(),
                    "bob@example.org".into(),
                    TeamRoleDto::Editor,
                    None,
                )
                .await
                .unwrap();
            let bob = start_bob().await;
            let inv = bob
                .incoming_team_invitations()
                .await
                .unwrap()
                .into_iter()
                .find(|i| i.vault_id == tid)
                .unwrap();
            let joined = bob.accept_team_invitation(inv.invitation_id).await.unwrap();
            bob.shutdown().await.unwrap();
            alice.team_join_requests(tid.clone()).await.unwrap();
            alice
                .confirm_team_member(tid, joined.invitation_id.clone())
                .await
                .unwrap();
            server
                .team_invitation_keys(joined.invitation_id.parse().unwrap())
                .unwrap()
        }
    };

    // 1. The server swaps the joiner's key; the owner's commitment exposes it.
    let t1 = alice
        .create_team("One".into(), None, None, None)
        .await
        .unwrap()
        .vault_id;
    let v1 = VaultId::from_str(&t1).unwrap();
    let keys = join_while_away(t1.clone()).await;
    let fake = cc_vault_core::UnlockedVault::new_team(v1).unwrap();
    assert!(server.plant_team_user_envelope(v1, fake.seal_for_member(&keys).unwrap()));
    let bob = start_bob().await;
    let _ = bob.refresh_teams().await;
    let t = team_of(&bob, &t1).await;
    assert!(
        t.as_ref().is_none_or(|t| !t.open),
        "swapped key opened: {t:?}"
    );
    bob.shutdown().await.unwrap();

    // 2. … and drops the commitment too: the joiner still refuses.
    let t2 = alice
        .create_team("Two".into(), None, None, None)
        .await
        .unwrap()
        .vault_id;
    let v2 = VaultId::from_str(&t2).unwrap();
    let keys = join_while_away(t2.clone()).await;
    assert!(server.strip_team_key_commitments(v2));
    let fake = cc_vault_core::UnlockedVault::new_team(v2).unwrap();
    assert!(server.plant_team_user_envelope(v2, fake.seal_for_member(&keys).unwrap()));
    let bob = start_bob().await;
    let _ = bob.refresh_teams().await;
    let t = team_of(&bob, &t2).await;
    assert!(
        t.as_ref().is_none_or(|t| !t.open),
        "unvouched key opened: {t:?}"
    );
    bob.shutdown().await.unwrap();

    // 3. An honest join opens; a pre-1.12 server still works.
    let t3 = alice
        .create_team("Three".into(), None, None, None)
        .await
        .unwrap()
        .vault_id;
    join_while_away(t3.clone()).await;
    let bob = start_bob().await;
    bob.refresh_teams().await.unwrap();
    assert!(
        team_of(&bob, &t3).await.unwrap().open,
        "an honest join opens"
    );
    bob.shutdown().await.unwrap();
    server.set_team_key_commitment(false);
    let t4 = alice
        .create_team("Four".into(), None, None, None)
        .await
        .unwrap()
        .vault_id;
    join_while_away(t4.clone()).await;
    let bob = start_bob().await;
    bob.refresh_teams().await.unwrap();
    assert!(
        team_of(&bob, &t4).await.unwrap().open,
        "pre-1.12 server: TOFU as before"
    );
    alice.shutdown().await.unwrap();
    bob.shutdown().await.unwrap();
}
