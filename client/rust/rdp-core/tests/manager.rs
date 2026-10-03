use cc_rdp_core::{ConnectConfig, Input, RdpError, RdpManager};
use secrecy::SecretString;

fn settings() -> ConnectConfig {
    ConnectConfig {
        address: "127.0.0.1".into(),
        port: 9,
        username: "fixture".into(),
        domain: None,
        width: 800,
        height: 600,
        accepted_certificate_sha256: [1; 32],
    }
}
fn runtime_secret() -> SecretString {
    SecretString::from(uuid::Uuid::new_v4().to_string())
}

#[tokio::test]
async fn permanent_shutdown_rejects_connect_probe_and_input() {
    let manager = RdpManager::new();
    let id = manager.connect(settings(), runtime_secret()).unwrap();
    manager.shutdown();
    assert_eq!(
        manager.connect(settings(), runtime_secret()),
        Err(RdpError::SessionNotFound)
    );
    assert_eq!(
        manager.probe_certificate("127.0.0.1", 9).await,
        Err(RdpError::SessionNotFound)
    );
    assert_eq!(
        manager.send_input(&id, vec![Input::ReleaseAll]),
        Err(RdpError::SessionNotFound)
    );
    assert!(matches!(manager.poll(&id), Err(RdpError::SessionNotFound)));
}
#[tokio::test]
async fn finite_session_limit_and_reusable_disconnect_all() {
    let manager = RdpManager::new();
    for _ in 0..4 {
        manager.connect(settings(), runtime_secret()).unwrap();
    }
    assert_eq!(
        manager.connect(settings(), runtime_secret()),
        Err(RdpError::SessionLimit)
    );
    manager.disconnect_all();
    assert!(manager.connect(settings(), runtime_secret()).is_ok());
    manager.shutdown();
}
#[test]
fn debug_output_redacts_typed_text_and_frame_pixels() {
    let marker = uuid::Uuid::new_v4().to_string();
    let input = Input::UnicodeText(marker.clone());
    assert!(!format!("{input:?}").contains(&marker));
    let frame = cc_rdp_core::Frame {
        sequence: 1,
        width: 200,
        height: 200,
        rgba: marker.as_bytes().to_vec(),
    };
    assert!(!format!("{frame:?}").contains(&marker));
}
