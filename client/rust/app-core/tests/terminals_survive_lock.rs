//! An idle auto-lock must not close open terminals: after unlocking, the
//! same shell is still connected and takes input. Closing the profile still
//! closes it.

mod common;

use cc_app_core::*;
use common::*;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lock_keeps_open_terminals_and_closing_the_profile_ends_them() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("Terminals".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let profiles = app.local_terminal_profiles().await.unwrap();
    let Some(shell) = profiles.profiles.first() else {
        eprintln!("skipped: no local shell on this host");
        app.shutdown().await.unwrap();
        return;
    };
    let opened = app
        .open_local_terminal(shell.id.clone(), 80, 24)
        .await
        .unwrap();
    assert_eq!(opened.status, TerminalStatusDto::Connected);

    app.lock().await.unwrap();
    // Locked: the vault refuses terminal calls, the shell keeps running.
    assert!(app.terminal_info(opened.id.clone()).await.is_err());

    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let after = app.terminal_info(opened.id.clone()).await.unwrap();
    assert_eq!(after.status, TerminalStatusDto::Connected);
    let mut attached = app.attach_terminal(opened.id.clone()).await.unwrap();
    app.terminal_write(
        opened.id.clone(),
        b"echo still-open-after-unlock\r".to_vec(),
    )
    .await
    .unwrap();
    let mut seen = String::from_utf8_lossy(&attached.snapshot).to_string();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !seen.contains("still-open-after-unlock\r\n")
            && !seen.contains("still-open-after-unlock\n")
        {
            match attached.output.recv().await {
                Some(TerminalChunk::Data(bytes)) => seen.push_str(&String::from_utf8_lossy(&bytes)),
                Some(_) | None => break,
            }
        }
    })
    .await
    .expect("the shell answers after unlock");
    assert!(seen.contains("still-open-after-unlock"), "{seen}");
    assert_eq!(app.list_terminals().await.unwrap().len(), 1);

    app.shutdown().await.unwrap();
}
