use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const STATE: &str = "s7aTe-0123456789abcdefghijklmnopqrstuvwxyzA";
const LOOPBACK: &str = "http://127.0.0.1:5000/auth/callback";

#[test]
fn pkce_s256_matches_rfc7636_vector() {
    // RFC 7636 Appendix B.
    assert_eq!(
        code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn pkce_and_state_are_fresh_and_well_formed() {
    use cc_sync_core::api::enterprise::{is_valid_code_challenge, is_valid_code_verifier};
    let a = Pkce::generate().unwrap();
    let b = Pkce::generate().unwrap();
    assert_eq!(a.verifier.len(), 43);
    assert!(is_valid_code_verifier(&a.verifier));
    assert!(is_valid_code_challenge(&a.challenge));
    assert_eq!(a.challenge, code_challenge(&a.verifier));
    assert_ne!(*a.verifier, *b.verifier);
    let s1 = random_token().unwrap();
    let s2 = random_token().unwrap();
    assert_ne!(*s1, *s2);
    // Android's callback filter expects 32..=256 characters.
    assert!((32..=256).contains(&s1.len()));
}

#[test]
fn redirect_uri_forms_are_allowed_by_the_protocol() {
    let v4: std::net::SocketAddr = "127.0.0.1:49152".parse().unwrap();
    let v6: std::net::SocketAddr = "[::1]:8080".parse().unwrap();
    assert_eq!(
        loopback_redirect_uri(v4),
        "http://127.0.0.1:49152/auth/callback"
    );
    assert_eq!(loopback_redirect_uri(v6), "http://[::1]:8080/auth/callback");
    assert!(is_allowed_app_redirect_uri(&loopback_redirect_uri(v4)));
    assert!(is_allowed_app_redirect_uri(&loopback_redirect_uri(v6)));
    assert!(is_allowed_app_redirect_uri(APP_REDIRECT_URI_MOBILE));
}

#[test]
fn callback_with_code_and_state() {
    let code = parse_app_callback(
        &format!("{LOOPBACK}?login_code=ccl_abc&state={STATE}"),
        LOOPBACK,
        STATE,
    )
    .unwrap();
    assert_eq!(code.expose_secret(), "ccl_abc");
    let mobile = parse_app_callback(
        &format!("consolecrypt://auth/callback?state={STATE}&login_code=x1"),
        APP_REDIRECT_URI_MOBILE,
        STATE,
    )
    .unwrap();
    assert_eq!(mobile.expose_secret(), "x1");
}

#[test]
fn callback_state_is_checked_first() {
    for bad in [
        format!("{LOOPBACK}?login_code=c&state=other"),
        format!("{LOOPBACK}?login_code=c"),
        format!("{LOOPBACK}?login_code=c&state={STATE}&state={STATE}"),
        // A forged error with the wrong state is not trusted either.
        format!("{LOOPBACK}?error=account_disabled&state=other"),
        format!("{LOOPBACK}?login_code=c&state={STATE}x"),
    ] {
        let e = parse_app_callback(&bad, LOOPBACK, STATE).unwrap_err();
        assert_eq!(e.code(), "invalid_input", "{bad}");
        assert_eq!(e.reason(), Some("enterprise_callback_invalid"), "{bad}");
    }
}

#[test]
fn callback_endpoint_must_match_redirect() {
    for bad in [
        format!("http://127.0.0.1:5001/auth/callback?login_code=c&state={STATE}"),
        format!("http://localhost:5000/auth/callback?login_code=c&state={STATE}"),
        format!("http://127.0.0.1:5000/other?login_code=c&state={STATE}"),
        format!("https://127.0.0.1:5000/auth/callback?login_code=c&state={STATE}"),
        format!("{LOOPBACK}?login_code=c&state={STATE}#frag"),
        format!("http://u@127.0.0.1:5000/auth/callback?login_code=c&state={STATE}"),
        format!("consolecrypt://evil/callback?login_code=c&state={STATE}"),
        "not a url".to_owned(),
    ] {
        let redirect = if bad.starts_with("consolecrypt") {
            APP_REDIRECT_URI_MOBILE
        } else {
            LOOPBACK
        };
        assert!(parse_app_callback(&bad, redirect, STATE).is_err(), "{bad}");
    }
}

#[test]
fn callback_needs_exactly_one_login_code() {
    for bad in [
        format!("{LOOPBACK}?state={STATE}"),
        format!("{LOOPBACK}?state={STATE}&login_code="),
        format!("{LOOPBACK}?state={STATE}&login_code=a&login_code=b"),
        format!(
            "{LOOPBACK}?state={STATE}&login_code={}",
            "a".repeat(MAX_LOGIN_CODE_LEN + 1)
        ),
    ] {
        assert!(parse_app_callback(&bad, LOOPBACK, STATE).is_err(), "{bad}");
    }
}

#[test]
fn callback_errors_map_to_app_errors() {
    let err = |code: &str| {
        parse_app_callback(
            &format!("{LOOPBACK}?state={STATE}&error={code}&error_description=Nope%0Ahere"),
            LOOPBACK,
            STATE,
        )
        .unwrap_err()
    };
    assert_eq!(err("account_disabled"), AppError::AccountDisabled);
    assert_eq!(err("invalid_credentials"), AppError::InvalidCredentials);
    assert_eq!(err("access_denied"), AppError::Cancelled);
    assert!(matches!(err("rate_limited"), AppError::RateLimited { .. }));
    for code in [
        "domain_not_allowed",
        "group_required",
        "mfa_required",
        "identity_conflict",
        "idp_unavailable",
        "enterprise_auth_required",
    ] {
        match err(code) {
            AppError::Server {
                code: c, message, ..
            } => {
                assert_eq!(c, code);
                // Control characters from the redirect are dropped.
                assert_eq!(message, "Nopehere");
            }
            other => panic!("{code}: {other:?}"),
        }
    }
    match err("made_up_code") {
        AppError::Server { code, .. } => assert_eq!(code, "bad_request"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn api_error_mapping_for_enterprise_codes() {
    let e: AppError = ApiError::AccountDisabled.into();
    assert_eq!(e.code(), "account_disabled");
    let e: AppError = ApiError::Server {
        status: 503,
        error: Box::new(cc_protocol::ApiError::new(
            ErrorCode::IdpUnavailable,
            "down",
        )),
    }
    .into();
    assert_eq!(e.code(), "server");
    assert_eq!(
        e.args().get("protocol_code").map(String::as_str),
        Some("idp_unavailable")
    );
    let e: AppError = ApiError::RateLimited {
        retry_after: Some(Duration::from_secs(42)),
    }
    .into();
    assert_eq!(
        e.args().get("retry_after_seconds").map(String::as_str),
        Some("42")
    );
    let e: AppError = cc_sync_core::StopReason::AccountDisabled.into();
    assert_eq!(e, AppError::AccountDisabled);
}

#[test]
fn ldap_login_validation() {
    assert_eq!(ldap_login("  ACME\\jdoe ").unwrap(), "ACME\\jdoe");
    assert_eq!(ldap_login("j.doe@acme.com").unwrap(), "j.doe@acme.com");
    assert!(ldap_login("   ").is_err());
    assert!(ldap_login("a\nb").is_err());
    assert!(ldap_login(&"a".repeat(MAX_ENTERPRISE_LOGIN_LEN + 1)).is_err());
    assert!(ldap_credential("jdoe".into(), String::new()).is_err());
    let c = ldap_credential("jdoe".into(), "pw".into()).unwrap();
    // Never prints the password.
    assert!(!format!("{c:?}").contains("pw"));
}

#[test]
fn login_hint_is_optional_and_bounded() {
    assert_eq!(login_hint(Some("  ".into())), None);
    assert_eq!(login_hint(None), None);
    assert_eq!(
        login_hint(Some(" a@acme.com ".into())).as_deref(),
        Some("a@acme.com")
    );
    assert_eq!(login_hint(Some("x".repeat(300))), None);
}

#[test]
fn purpose_wire_names() {
    use std::str::FromStr;
    assert_eq!(
        EnterpriseSignInPurpose::from_str("new_profile").unwrap(),
        EnterpriseSignInPurpose::NewProfile
    );
    assert_eq!(
        EnterpriseSignInPurpose::from_str("reauthenticate").unwrap(),
        EnterpriseSignInPurpose::Reauthenticate
    );
    assert_eq!(
        EnterpriseSignInPurpose::from_str("enable_sync").unwrap(),
        EnterpriseSignInPurpose::EnableSync
    );
    assert!(EnterpriseSignInPurpose::from_str("other").is_err());
}

#[test]
fn start_url_carries_pkce_state_and_device() {
    let pkce = Pkce::generate().unwrap();
    let q = OidcStartQuery {
        redirect_uri: LOOPBACK.into(),
        code_challenge: pkce.challenge.clone(),
        code_challenge_method: Some(CODE_CHALLENGE_METHOD_S256.into()),
        state: STATE.into(),
        device_id: cc_protocol::DeviceId::from(Uuid::nil()),
        login_hint: Some("a@acme.com".into()),
    };
    let url = Url::parse(&q.to_url("https://sync.example.com/base/")).unwrap();
    assert_eq!(url.path(), "/base/v1/auth/oidc/start");
    let p = params(&url);
    assert_eq!(single(&p, "code_challenge"), Some(pkce.challenge.as_str()));
    assert_eq!(single(&p, "state"), Some(STATE));
    assert_eq!(single(&p, "redirect_uri"), Some(LOOPBACK));
    assert_eq!(single(&p, "login_hint"), Some("a@acme.com"));
    // The verifier never appears in the browser URL.
    assert!(!url.as_str().contains(pkce.verifier.as_str()));
}

#[tokio::test]
async fn loopback_listener_accepts_only_the_matching_state() {
    let (listener, redirect_uri) = bind_loopback().await.unwrap();
    let redirect = Url::parse(&redirect_uri).unwrap();
    let addr = listener.local_addr().unwrap();
    let host = redirect.host_str().unwrap().to_owned();
    let port = redirect.port().unwrap();
    let client = tokio::spawn(async move {
        let send = |query: String| {
            let host = host.clone();
            async move {
                let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
                let req =
                    format!("GET /auth/callback?{query} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n");
                s.write_all(req.as_bytes()).await.unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out).await;
                out
            }
        };
        let rejected = send("login_code=c&state=forged".into()).await;
        let accepted = send(format!("login_code=good&state={STATE}")).await;
        (rejected, accepted)
    });
    let url = await_callback(
        &listener,
        &redirect,
        |u| callback_state_matches(u, &redirect, STATE),
        CancellationToken::new(),
        Instant::now() + Duration::from_secs(10),
    )
    .await
    .unwrap();
    let (rejected, accepted) = client.await.unwrap();
    assert!(rejected.starts_with("HTTP/1.1 400"));
    assert!(accepted.starts_with("HTTP/1.1 200"));
    let code = parse_app_callback(url.as_str(), &redirect_uri, STATE).unwrap();
    assert_eq!(code.expose_secret(), "good");
}
