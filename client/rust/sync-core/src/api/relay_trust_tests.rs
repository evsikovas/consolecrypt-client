//! Draft maintained-PKI tests; no fake Relay slot/authority or hosttrust edits.
use super::*;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{ServerName, UnixTime};
const ROOT: &[u8] = include_bytes!("relay_trust_fixtures/root1.crt");
const INTERMEDIATE: &[u8] = include_bytes!("relay_trust_fixtures/root1-int1.crt");
const LEAF: &[u8] = include_bytes!("relay_trust_fixtures/root1-int1-ee_example.com-good.crt");
fn trust() -> ApiServerTrust {
    ApiServerTrust::from_public_ca_der("https://example.com/owned/", vec![ROOT.to_vec()]).unwrap()
}
fn verify(verifier: &dyn ServerCertVerifier, name: &str) -> bool {
    verifier
        .verify_server_cert(
            &CertificateDer::from(LEAF),
            &[CertificateDer::from(INTERMEDIATE)],
            &ServerName::try_from(name).unwrap(),
            &[],
            UnixTime::since_unix_epoch(Duration::from_secs(1788954730)),
        )
        .is_ok()
}
#[test]
fn explicit_public_ca_accepts_real_chain_at_fixture_time_and_rejects_wrong_hostname() {
    let t = trust();
    let v = t.verifier().unwrap();
    assert!(verify(&*v, "example.com"));
    assert!(!verify(&*v, "counterfeit.example.invalid"));
}
#[test]
fn untrusted_private_fixture_ca_is_not_implicitly_installed() {
    let v = rustls_platform_verifier::Verifier::new(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .unwrap();
    assert!(!verify(&v, "example.com"));
}
#[test]
fn der_caps_malformed_leaf_and_trailing_data_are_rejected() {
    for roots in [
        vec![],
        vec![ROOT.to_vec(); 5],
        vec![vec![0; 16385]],
        vec![b"not a certificate".to_vec()],
        vec![LEAF.to_vec()],
        vec![[ROOT, b"trailing"].concat()],
        vec![ROOT.to_vec(), ROOT.to_vec()],
    ] {
        assert!(ApiServerTrust::from_public_ca_der("https://example.com/owned/", roots).is_err());
    }
}
#[test]
fn exact_baseurl_rebinding_and_http_are_rejected() {
    let t = trust();
    assert!(
        ApiConfig::new("https://example.com/other/", "test", Platform::Cli)
            .unwrap()
            .with_server_trust(t.clone())
            .is_err()
    );
    assert!(
        ApiConfig::new("https://different.invalid/owned/", "test", Platform::Cli)
            .unwrap()
            .with_server_trust(t)
            .is_err()
    );
    assert!(ApiServerTrust::from_public_ca_der("http://localhost/", vec![ROOT.to_vec()]).is_err());
}
#[test]
fn original_client_captures_immutable_trust_and_late_input_mutation_cannot_rebind_it() {
    let config = ApiConfig::new("https://example.com/owned/", "test", Platform::Cli)
        .unwrap()
        .with_server_trust(trust())
        .unwrap();
    let mut later = config.clone();
    let client = ApiClient::new(config, Arc::new(super::super::MemoryTokenStore::new())).unwrap();
    later.server_url = Url::parse("https://different.invalid/").unwrap();
    assert!(ApiClient::new(later, Arc::new(super::super::MemoryTokenStore::new())).is_err());
    assert_eq!(
        client.config().server_url.as_str(),
        "https://example.com/owned/"
    );
    assert!(client
        .config()
        .extra_server_trust
        .as_ref()
        .unwrap()
        .applies_to(&client.config().server_url));
}
#[test]
fn once_http_and_wss_share_exact_cached_tls_trust_and_default_is_unchanged() {
    let cfg = ApiConfig::new("https://example.com/owned/", "test", Platform::Cli)
        .unwrap()
        .with_server_trust(trust())
        .unwrap();
    let http = cfg.extra_server_tls(true).unwrap().unwrap();
    let wss = cfg.extra_server_tls(true).unwrap().unwrap();
    assert!(Arc::ptr_eq(&http, &wss));
    let default = ApiConfig::new("https://example.com/", "test", Platform::Cli).unwrap();
    assert!(default.extra_server_tls(true).unwrap().is_none());
    assert!(default.extra_server_tls(false).unwrap().is_none());
}
