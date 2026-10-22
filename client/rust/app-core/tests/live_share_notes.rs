//! Notes travelling with a share against a LIVE server with
//! `CC_OBJECT_SHARING_ENABLED=true` and `CC_SHARED_NOTES_ENABLED=true`:
//!
//! ```sh
//! CC_E2E_SERVER=http://127.0.0.1:18080 CC_E2E_MAIL_DIR=<the server's CC_MAIL_DIR> \
//!   cargo test -p cc-app-core --test live_share_notes -- --nocapture
//! ```
//!
//! Sharing discovery needs verified e-mail: the test reads the verification
//! message from the server's file mail transport.

mod common;

use cc_app_core::sharing_dto::{SharingGrantDto, SharingIdentityDto, SharingRoleDto};
use cc_app_core::sharing_projection::{SharedHostProjection, SharedProjection};
use cc_app_core::*;
use common::*;

async fn account(url: &str, dir: &std::path::Path) -> (AppCore, String) {
    let app = common::app(dir);
    let email = format!("notes-{}@example.test", uuid::Uuid::new_v4().simple());
    app.create_synced_profile(
        "Work".into(),
        url.into(),
        email.clone(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSPHRASE.into()).await.unwrap();
    verify_email(url, &email).await;
    (app, email)
}

/// Confirms `email` with the token from the server's file mail transport.
async fn verify_email(url: &str, email: &str) {
    use base64::Engine as _;
    let dir = std::env::var("CC_E2E_MAIL_DIR").expect("CC_E2E_MAIL_DIR");
    let mut token = None;
    for _ in 0..50 {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            if !text.contains(&format!("To: {email}")) {
                continue;
            }
            let body: String = text
                .split("\r\n\r\n")
                .nth(1)
                .or_else(|| text.split("\n\n").nth(1))
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(body)
                .unwrap_or_default();
            let decoded = String::from_utf8_lossy(&decoded).into_owned();
            token = decoded
                .split_whitespace()
                .find(|word| word.starts_with("cct_"))
                .map(str::to_owned);
        }
        if token.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let response = reqwest::Client::new()
        .post(format!("{url}/v1/auth/email/verify"))
        .json(&serde_json::json!({ "token": token.expect("verification mail") }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
}

fn grant(identity: SharingIdentityDto) -> SharingGrantDto {
    SharingGrantDto {
        confirmed_code: identity.verification_code.clone(),
        identity,
        role: SharingRoleDto::Reader,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_notes_against_live_server() {
    let Ok(url) = std::env::var("CC_E2E_SERVER") else {
        eprintln!("skipped: set CC_E2E_SERVER to a server with shared notes enabled");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (owner, _) = account(&url, &tmp.path().join("owner")).await;
    let (reader, reader_email) = account(&url, &tmp.path().join("reader")).await;
    let owner_identity = owner.sharing_identity().await.unwrap();
    let reader_identity = reader.sharing_identity().await.unwrap();
    owner.sharing_discover(reader_email).await.unwrap();
    let created = owner
        .sharing_publish(
            SharedProjection::Host(SharedHostProjection {
                name: "db-prod".into(),
                address: "db.example.test".into(),
                port: 22,
                username: None,
                keepalive_secs: None,
                tags: vec![],
                notes: None,
            }),
            vec![grant(reader_identity.clone())],
        )
        .await
        .unwrap();
    owner.sharing_flush().await.unwrap();
    reader.sharing_list(true).await.unwrap();
    reader
        .sharing_accept(
            created.share_id.clone(),
            owner_identity.verification_code.clone(),
        )
        .await
        .unwrap();
    let share = created.share_id.clone();

    // Links are local and validated.
    reader
        .share_note_link(format!("host:{}", uuid::Uuid::new_v4()), share.clone())
        .await
        .unwrap();
    assert_eq!(reader.share_note_links().await.unwrap().len(), 1);
    assert!(reader
        .share_note_link("file:/etc".into(), share.clone())
        .await
        .is_err());

    // Before the owner opens notes, a member waits for the epoch key.
    let waiting = reader.share_notes(share.clone()).await.unwrap();
    assert!(waiting.enabled && waiting.waiting_for_owner && !waiting.owner);
    assert!(reader
        .share_note_save(
            share.clone(),
            SharedNoteInputDto {
                title: "too early".into(),
                ..Default::default()
            }
        )
        .await
        .is_err());

    // The owner issues the key on first use; the reader writes; both read.
    let opened = owner.share_notes(share.clone()).await.unwrap();
    assert!(opened.owner && !opened.waiting_for_owner && opened.notes.is_empty());
    let scope = serde_json::json!({ "docker_container": "postgres" });
    let note = reader
        .share_note_save(
            share.clone(),
            SharedNoteInputDto {
                title: "Restart order".into(),
                body: "replica first, then **primary**".into(),
                tags: vec!["ops".into()],
                scope: Some(scope.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let seen = owner.share_notes(share.clone()).await.unwrap();
    assert_eq!(seen.notes.len(), 1);
    let read = &seen.notes[0];
    assert_eq!(
        (read.title.as_str(), read.scope.as_ref()),
        ("Restart order", Some(&scope))
    );
    assert!(!read.mine && read.can_delete);
    // Only the author edits.
    assert!(owner
        .share_note_save(
            share.clone(),
            SharedNoteInputDto {
                id: note.id.clone(),
                title: "hijack".into(),
                ..Default::default()
            }
        )
        .await
        .is_err());
    let edited = reader
        .share_note_save(
            share.clone(),
            SharedNoteInputDto {
                id: note.id.clone(),
                title: "Restart order (v2)".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (edited.revision, edited.created_at_ms),
        (2, note.created_at_ms)
    );
    let owner_note = owner
        .share_note_save(
            share.clone(),
            SharedNoteInputDto {
                title: "Backups at 02:00".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Removing the reader starts a new epoch: the reader is cut off, the
    // owner's next read issues a new key and moves both notes into it; the
    // former member's note still verifies against the signed history.
    owner.sharing_rotate(share.clone(), vec![]).await.unwrap();
    owner.sharing_flush().await.unwrap();
    assert!(reader.share_notes(share.clone()).await.is_err());
    let after = owner.share_notes(share.clone()).await.unwrap();
    assert_eq!(after.pending, 0, "{after:?}");
    let mut titles: Vec<_> = after.notes.iter().map(|n| n.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Backups at 02:00", "Restart order (v2)"]);

    // The owner removes notes, including the former member's.
    owner
        .share_note_delete(share.clone(), note.id.clone())
        .await
        .unwrap();
    owner
        .share_note_delete(share.clone(), owner_note.id.clone())
        .await
        .unwrap();
    assert!(owner
        .share_notes(share.clone())
        .await
        .unwrap()
        .notes
        .is_empty());

    for app in [&owner, &reader] {
        app.shutdown().await.unwrap();
    }
    println!("shared notes against {url}: OK");
}
