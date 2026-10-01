//! Real OpenSSH/Readline Unicode regression. Only an owned Docker fixture and
//! runtime-generated test keys are used. Run with CC_SSH_IT=1; no user host,
//! SSH agent, keychain or vault is involved.

use cc_ssh_core::testing::{self, SshTestbed, TestInventory, connector};
use cc_ssh_core::{
    ConnectionPlanner, HostKeyDecision, MemoryKnownHosts, PlannerDefaults, PtyRequest, ShellReader,
    ShellWriter, SshSession,
};
use std::{sync::Arc, time::Duration};

async fn drain(reader: &mut ShellReader) -> Vec<u8> {
    let mut bytes = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let idle = (tokio::time::Instant::now() + Duration::from_millis(250)).min(deadline);
        match tokio::time::timeout_at(idle, reader.recv()).await {
            Ok(Some(cc_ssh_core::ShellEvent::Data(data)))
            | Ok(Some(cc_ssh_core::ShellEvent::Stderr(data))) => bytes.extend_from_slice(&data),
            Ok(Some(cc_ssh_core::ShellEvent::Closed)) | Ok(None) | Err(_) => break,
            Ok(Some(_)) => {}
        }
        assert!(bytes.len() < 1024 * 1024, "test output exceeded its bound");
        assert!(
            tokio::time::Instant::now() < deadline,
            "test output did not become idle"
        );
    }
    bytes
}

async fn open(
    inventory: &TestInventory,
    pty: PtyRequest,
) -> (SshSession, ShellWriter, ShellReader, Vec<u8>) {
    let (client, _) = connector(
        inventory.resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        HostKeyDecision::Reject,
    );
    let plan = ConnectionPlanner::new(inventory.inventory.clone())
        .with_defaults(PlannerDefaults::default())
        .plan(inventory.bastion1)
        .await
        .unwrap();
    let session = client.connect(&plan).await.unwrap();
    let shell = session.open_shell(pty).await.unwrap();
    let (writer, mut reader) = shell.split();
    let initial = drain(&mut reader).await;
    (session, writer, reader, initial)
}

async fn input(writer: &ShellWriter, reader: &mut ShellReader, data: &[u8]) -> Vec<u8> {
    writer
        .write(bytes::Bytes::copy_from_slice(data))
        .await
        .unwrap();
    drain(reader).await
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("synthetic shell output must retain valid UTF-8")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openssh_utf8_locale_readline_and_canonical_erase() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run the owned OpenSSH Unicode fixture");
        return;
    }
    let fixture = tokio::task::spawn_blocking(|| SshTestbed::start(false))
        .await
        .unwrap()
        .expect("owned SSH testbed");
    // Start the remote shell in C without overriding the requested LC_CTYPE.
    // Readline's meta conversion is what corrupts the leading UTF-8 byte.
    fixture
        .exec_in(
            &fixture.bastion1,
            r#"sed -i 's@/bin/sh$@/bin/bash@' /etc/passwd
cat > /home/tester/.bash_profile <<'EOF'
export LANG=C
HISTFILE=/dev/null
INPUTRC=/dev/null
PS1='cc-test> '
bind 'set input-meta on'
bind 'set output-meta on'
printf 'CC_LANG=%s CC_CTYPE=%s CC_ALL=%s\n' "$LANG" "$LC_CTYPE" "$LC_ALL"
bind -v | grep -E 'convert-meta|input-meta|output-meta'
stty -a
EOF
chown tester:tester /home/tester/.bash_profile
printf 'AcceptEnv LC_*\n' > /etc/ssh/sshd_config.d/unicode-test.conf
kill -HUP 1"#,
        )
        .unwrap();
    // OpenSSH re-execs after SIGHUP; wait before starting key exchange.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let inventory = fixture.inventory();

    // Product defaults must work even when the server's LANG remains C.
    let (session, writer, mut reader, initial) = open(&inventory, PtyRequest::default()).await;
    let first = input(&writer, &mut reader, "п".as_bytes()).await;
    assert_eq!(
        first,
        "п".as_bytes(),
        "first scalar must not become Readline Meta-P"
    );
    assert_eq!(
        input(&writer, &mut reader, "ривет".as_bytes()).await,
        "ривет".as_bytes()
    );
    assert!(text(&initial).contains("CC_LANG=C CC_CTYPE=C.UTF-8 CC_ALL=\r\n"));
    assert!(text(&initial).contains("set convert-meta off"));
    assert!(
        text(&initial)
            .split_ascii_whitespace()
            .any(|mode| mode == "iutf8")
    );
    input(&writer, &mut reader, b"\x03").await;
    assert_eq!(input(&writer, &mut reader, b"ASCII").await, b"ASCII");
    input(&writer, &mut reader, b"\x03").await;

    // A preceding snippet must not change subsequent typed or pasted text.
    input(&writer, &mut reader, b"# synthetic snippet\r").await;
    assert_eq!(
        input(&writer, &mut reader, "привет ёж".as_bytes()).await,
        "привет ёж".as_bytes()
    );
    input(&writer, &mut reader, b"\x03").await;
    let pasted = input(
        &writer,
        &mut reader,
        "\x1b[200~printf 'CC_PASTE:%s\\n' 'Вставка'\x1b[201~".as_bytes(),
    )
    .await;
    assert!(text(&pasted).contains("Вставка"));
    let executed = input(&writer, &mut reader, b"\r").await;
    assert!(text(&executed).contains("CC_PASTE:Вставка\r\n"));

    // Readline must erase the full Cyrillic scalar, rather than one byte.
    input(&writer, &mut reader, "п".as_bytes()).await;
    input(&writer, &mut reader, b"\x7f").await;
    let erased = input(
        &writer,
        &mut reader,
        "printf 'CC_ERASE:%s\\n' 'я'\r".as_bytes(),
    )
    .await;
    // If an incomplete п remains, the command would be a nonexistent пprintf.
    assert!(text(&erased).contains("CC_ERASE:я\r\n"));

    // Canonical kernel erase exercises IUTF8 independently of Readline.
    input(
        &writer,
        &mut reader,
        b"stty icanon -echo; head -n 1 | od -An -tx1; stty echo\r",
    )
    .await;
    let erased = input(&writer, &mut reader, "п\x7fя\n".as_bytes()).await;
    assert!(
        text(&erased).contains("d1 8f 0a"),
        "canonical erase lost UTF-8 boundaries"
    );
    assert!(!text(&erased).contains("d0 d1 8f"));
    writer.close().await.unwrap();
    session.disconnect().await.unwrap();

    // Explicit caller environment is not silently replaced. LC_ALL wins over
    // LC_CTYPE by POSIX rules; a caller/server forcing C remains a known limit.
    let (session, writer, mut reader, initial) = open(
        &inventory,
        PtyRequest {
            env: vec![
                ("LC_CTYPE".into(), "C.UTF-8".into()),
                ("LC_ALL".into(), "C".into()),
            ],
            ..Default::default()
        },
    )
    .await;
    assert!(text(&initial).contains("CC_CTYPE=C.UTF-8 CC_ALL=C\r\n"));
    assert_eq!(
        input(&writer, &mut reader, "п".as_bytes()).await,
        [b':', 0xbf]
    );
    writer.close().await.unwrap();
    session.disconnect().await.unwrap();

    // Refused env requests remain best-effort and must not prevent a shell.
    fixture
        .exec_in(
            &fixture.bastion1,
            "rm /etc/ssh/sshd_config.d/unicode-test.conf; kill -HUP 1",
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let (session, writer, mut reader, initial) = open(&inventory, PtyRequest::default()).await;
    assert!(text(&initial).contains("CC_CTYPE= CC_ALL=\r\n"));
    assert_eq!(input(&writer, &mut reader, b"ASCII").await, b"ASCII");
    writer.close().await.unwrap();
    session.disconnect().await.unwrap();
    // The fixture Drop removes its own container/network, including test keys.
}
