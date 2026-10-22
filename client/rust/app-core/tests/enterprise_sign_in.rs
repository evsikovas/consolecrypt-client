//! Enterprise sign-in (protocol 1.8) against the sync-core mock server:
//! LDAP new profile + vault + re-authentication, OIDC through the loopback
//! redirect (PKCE + state), cancel, disabled accounts, and password
//! sign-in refused in enterprise mode.
//!
//! Against the real stands (Keycloak `cc-test-keycloak`, Samba AD
//! `cc-test-samba`) see the env-gated `enterprise_live` test at the end.

mod common;

use cc_app_core::enterprise_auth::EnterpriseSignInPurpose;
use cc_app_core::*;
use cc_protocol::meta::{AuthCapability, AuthMode, AuthProvider};
use cc_sync_core::mock::{MockServer, MockServerConfig};
use common::*;

fn enterprise(provider: AuthProvider) -> MockServerConfig {
    MockServerConfig {
        auth: AuthCapability {
            mode: AuthMode::Enterprise,
            provider: Some(provider),
            display_name: Some("Acme SSO".into()),
        },
        ..Default::default()
    }
}

fn protocol_code(e: &AppError) -> Option<String> {
    e.args().get("protocol_code").cloned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ldap_profile_vault_reauth_and_disable() {
    let server = MockServer::start_with(enterprise(AuthProvider::Ldap)).await;
    server.add_directory_user("ACME\\jdoe", "directory-pw", "jdoe@acme.test");
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("a"));

    // Password register / login are refused in enterprise mode.
    let e = app
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "jdoe@acme.test".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap_err();
    assert_eq!(
        protocol_code(&e).as_deref(),
        Some("enterprise_auth_required")
    );
    assert!(app.list_profiles().await.unwrap().is_empty());

    // Wrong directory password: the usual invalid credentials, no profile.
    let e = app
        .create_synced_profile_ldap(None, url.clone(), "acme\\JDOE".into(), "nope".into())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_credentials");
    assert!(app.list_profiles().await.unwrap().is_empty());

    // First sign-in creates the account; the profile is named by its e-mail
    // and has no vault yet (the normal vault onboarding follows).
    let acct = app
        .create_synced_profile_ldap(
            None,
            url.clone(),
            "acme\\JDOE".into(),
            "directory-pw".into(),
        )
        .await
        .unwrap();
    assert_eq!(acct.profile.kind, ProfileKind::Synced);
    assert_eq!(acct.profile.email.as_deref(), Some("jdoe@acme.test"));
    assert_eq!(acct.profile.display_name, "jdoe@acme.test");
    assert_eq!(acct.profile.vault_state, VaultStateDto::NoVault);
    assert!(acct.email_verified);
    app.create_vault(PASSPHRASE.into()).await.unwrap();
    app.sync_now().await.unwrap();

    // Re-authentication with the directory keeps the profile and device.
    let state = app
        .reauthenticate_ldap("acme\\jdoe".into(), "directory-pw".into(), false)
        .await
        .unwrap();
    assert!(!state.new_device_identity);
    assert!(state.trusted);

    // Another directory user cannot re-authenticate this profile.
    server.add_directory_user("other", "other-pw", "other@acme.test");
    let e = app
        .reauthenticate_ldap("other".into(), "other-pw".into(), false)
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("account_mismatch"));

    // Disabled by the administrator: refresh answers account_disabled.
    server.disable_account("jdoe@acme.test");
    let e = app.sync_now().await.unwrap_err();
    assert_eq!(e.code(), "account_disabled", "{e:?}");
    let e = app
        .reauthenticate_ldap("acme\\jdoe".into(), "directory-pw".into(), false)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "account_disabled");
    // Local data stays (encrypted) and the profile can be removed.
    let pid = acct.profile.id.clone();
    app.close_profile().await.unwrap();
    app.remove_profile(pid).await.unwrap();
    assert!(app.list_profiles().await.unwrap().is_empty());
}

/// Plays the system browser: follows `/v1/auth/oidc/start` to the app's
/// redirect URI (the loopback listener of the core).
async fn browse(authorization_url: String) -> u16 {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let start = http.get(&authorization_url).send().await.unwrap();
    assert_eq!(start.status().as_u16(), 302);
    let location = start
        .headers()
        .get(reqwest::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(location.starts_with("http://127.0.0.1:") || location.starts_with("http://[::1]:"));
    http.get(&location).send().await.unwrap().status().as_u16()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oidc_new_profile_via_loopback_and_cancel() {
    let server = MockServer::start_with(enterprise(AuthProvider::Oidc)).await;
    server.set_oidc_user(Some("ann@acme.test"));
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("a"));

    // Cancel: the prepared profile is removed and completion reports it.
    let ch = app
        .enterprise_oidc_begin(
            url.clone(),
            EnterpriseSignInPurpose::NewProfile,
            None,
            None,
            false,
        )
        .await
        .unwrap();
    assert!(ch.loopback);
    assert!(ch.redirect_uri.ends_with("/auth/callback"));
    assert!(ch.authorization_url.contains("code_challenge_method=S256"));
    app.enterprise_oidc_cancel(ch.ticket.clone()).await.unwrap();
    let e = app
        .enterprise_oidc_complete(ch.ticket, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "cancelled");
    assert!(app.list_profiles().await.unwrap().is_empty());

    // Full sign-in: browser → server → loopback → exchange.
    let ch = app
        .enterprise_oidc_begin(
            url.clone(),
            EnterpriseSignInPurpose::NewProfile,
            Some("ann@acme.test".into()),
            Some("Acme".into()),
            false,
        )
        .await
        .unwrap();
    assert!(ch.authorization_url.contains("login_hint=ann%40acme.test"));
    let browser = tokio::spawn(browse(ch.authorization_url.clone()));
    let done = app
        .enterprise_oidc_complete(ch.ticket.clone(), None)
        .await
        .unwrap();
    assert_eq!(browser.await.unwrap(), 200);
    assert_eq!(done.purpose, EnterpriseSignInPurpose::NewProfile);
    let acct = done.account.unwrap();
    assert_eq!(acct.profile.email.as_deref(), Some("ann@acme.test"));
    assert_eq!(acct.profile.display_name, "Acme");
    // The ticket is single use.
    let e = app
        .enterprise_oidc_complete(ch.ticket, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "cancelled");

    // Re-authenticate the same profile through the browser.
    let ch = app
        .enterprise_oidc_begin(
            url.clone(),
            EnterpriseSignInPurpose::Reauthenticate,
            None,
            None,
            false,
        )
        .await
        .unwrap();
    let browser = tokio::spawn(browse(ch.authorization_url.clone()));
    let done = app.enterprise_oidc_complete(ch.ticket, None).await.unwrap();
    assert_eq!(browser.await.unwrap(), 200);
    assert!(done.auth_state.is_some());

    // The IdP refuses a disabled user: the redirect carries the error.
    server.disable_account("ann@acme.test");
    let ch = app
        .enterprise_oidc_begin(
            url,
            EnterpriseSignInPurpose::Reauthenticate,
            None,
            None,
            false,
        )
        .await
        .unwrap();
    let browser = tokio::spawn(browse(ch.authorization_url.clone()));
    let e = app
        .enterprise_oidc_complete(ch.ticket, None)
        .await
        .unwrap_err();
    assert_eq!(browser.await.unwrap(), 200);
    assert_eq!(e.code(), "account_disabled");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oidc_mobile_callback_checks_state() {
    // Mobile hands the callback URL in; a forged state is refused before
    // the code is redeemed (and the code is not spent).
    let server = MockServer::start_with(enterprise(AuthProvider::Oidc)).await;
    server.set_oidc_user(Some("bob@acme.test"));
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("a"));
    let ch = app
        .enterprise_oidc_begin(
            server.url().to_string(),
            EnterpriseSignInPurpose::NewProfile,
            None,
            None,
            false,
        )
        .await
        .unwrap();
    let forged = format!("{}?login_code=ccl_x&state=forged", ch.redirect_uri);
    let e = app
        .enterprise_oidc_complete(ch.ticket, Some(forged))
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("enterprise_callback_invalid"));
    assert!(app.list_profiles().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standard_server_is_unchanged() {
    let server = MockServer::start().await;
    let info = {
        let cfg =
            cc_sync_core::ApiConfig::new(server.url().as_str(), "0.9.0", Platform::Cli).unwrap();
        cc_sync_core::ApiClient::new(
            cfg,
            std::sync::Arc::new(cc_sync_core::MemoryTokenStore::new()),
        )
        .unwrap()
        .meta()
        .await
        .unwrap()
    };
    assert_eq!(info.capabilities.auth.mode, AuthMode::Standard);
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("a"));
    // Directory sign-in on a standard server: enterprise_auth_required.
    let e = app
        .create_synced_profile_ldap(None, server.url().to_string(), "x".into(), "y".into())
        .await
        .unwrap_err();
    assert_eq!(
        protocol_code(&e).as_deref(),
        Some("enterprise_auth_required")
    );
    assert!(app.list_profiles().await.unwrap().is_empty());
}

/// Live stands, env-gated (never started or removed by the tests):
///
/// ```text
/// CC_ENT_LDAP_SERVER=https://… CC_ENT_LDAP_LOGIN='ACME\jdoe' CC_ENT_LDAP_PASSWORD=… \
///   cargo test -p cc-app-core --test enterprise_sign_in enterprise_live -- --nocapture
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enterprise_live_ldap() {
    let (Ok(url), Ok(login), Ok(password)) = (
        std::env::var("CC_ENT_LDAP_SERVER"),
        std::env::var("CC_ENT_LDAP_LOGIN"),
        std::env::var("CC_ENT_LDAP_PASSWORD"),
    ) else {
        eprintln!("skipped: set CC_ENT_LDAP_SERVER/LOGIN/PASSWORD to run against a live stand");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("live"));
    let acct = app
        .create_synced_profile_ldap(None, url, login, password)
        .await
        .unwrap();
    assert_eq!(acct.profile.kind, ProfileKind::Synced);
    assert!(acct.profile.email.is_some());
}
