use super::*;
use ring::{rand::SystemRandom, signature::KeyPair};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

struct TestIssuer {
    issuer: Url,
    certificate: String,
    signing_key: Arc<signature::EcdsaKeyPair>,
    overrides: Arc<Mutex<Value>>,
    nonce: Arc<Mutex<Option<String>>>,
    challenge: Arc<Mutex<Option<String>>>,
    token_requests: Arc<Mutex<Vec<String>>>,
    redirect_discovery: Arc<std::sync::atomic::AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestIssuer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn signer() -> signature::EcdsaKeyPair {
    let random = SystemRandom::new();
    let document = signature::EcdsaKeyPair::generate_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &random,
    )
    .unwrap();
    signature::EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        document.as_ref(),
        &random,
    )
    .unwrap()
}

fn jwks(key: &signature::EcdsaKeyPair) -> Value {
    let public = key.public_key().as_ref();
    json!({"keys":[{
        "kid":"fixture-key", "kty":"EC", "alg":"ES256", "use":"sig",
        "crv":"P-256", "x":URL_SAFE_NO_PAD.encode(&public[1..33]),
        "y":URL_SAFE_NO_PAD.encode(&public[33..65])
    }]})
}

fn sign(key: &signature::EcdsaKeyPair, claims: &Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","kid":"fixture-key","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    let input = format!("{header}.{payload}");
    let signature = key.sign(&SystemRandom::new(), input.as_bytes()).unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
}

impl TestIssuer {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let pem = certificate.cert.pem();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(
                    certificate.signing_key.serialize_der(),
                )
                .into(),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = Url::parse(&format!(
            "https://localhost:{}/issuer",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        let signing_key = Arc::new(signer());
        let overrides = Arc::new(Mutex::new(json!({})));
        let nonce: Arc<Mutex<Option<String>>> = Arc::default();
        let challenge: Arc<Mutex<Option<String>>> = Arc::default();
        let token_requests = Arc::new(Mutex::new(vec![]));
        let redirect_discovery = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = {
            let issuer = issuer.clone();
            let signing_key = signing_key.clone();
            let overrides = overrides.clone();
            let nonce = nonce.clone();
            let challenge = challenge.clone();
            let token_requests = token_requests.clone();
            let redirect_discovery = redirect_discovery.clone();
            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let Ok(mut stream) = acceptor.accept(socket).await else {
                        continue;
                    };
                    let mut header = Vec::new();
                    let mut byte = [0u8];
                    while header.len() < 32 * 1024 && !header.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).await.is_err() {
                            break;
                        }
                        header.push(byte[0]);
                    }
                    let header = String::from_utf8_lossy(&header);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if length > 32 * 1024 {
                        continue;
                    }
                    let mut request_body = vec![0; length];
                    if stream.read_exact(&mut request_body).await.is_err() {
                        continue;
                    }
                    let path = header.split_whitespace().nth(1).unwrap_or("");
                    let body = match path {
                        "/issuer/.well-known/openid-configuration" => {
                            if redirect_discovery.load(std::sync::atomic::Ordering::SeqCst) {
                                let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/never\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                continue;
                            }
                            json!({
                                "issuer":issuer.as_str(),
                                "authorization_endpoint":format!("{issuer}/authorize"),
                                "token_endpoint":format!("{issuer}/token"),
                                "jwks_uri":format!("{issuer}/jwks"),
                                "code_challenge_methods_supported":["S256"],
                                "id_token_signing_alg_values_supported":["ES256"],
                                "token_endpoint_auth_methods_supported":["none"]
                            })
                        }
                        "/issuer/jwks" => jwks(&signing_key),
                        "/issuer/token" => {
                            let text = String::from_utf8(request_body).unwrap();
                            let form: std::collections::BTreeMap<_, _> =
                                url::form_urlencoded::parse(text.as_bytes())
                                    .into_owned()
                                    .collect();
                            token_requests.lock().unwrap().push(text);
                            let refresh =
                                form.get("grant_type").map(String::as_str) == Some("refresh_token");
                            if !refresh {
                                let verifier = form.get("code_verifier").unwrap();
                                assert_eq!(
                                    Some(
                                        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
                                    ),
                                    *challenge.lock().unwrap()
                                );
                                assert_eq!(
                                    form.get("code").map(String::as_str),
                                    Some("synthetic-code")
                                );
                            }
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            let mut claims = json!({
                                "iss":issuer.as_str(),"sub":"fixture-subject","aud":"consolecrypt-fixture",
                                "iat":now,"exp":now+3600,
                                "at_hash":URL_SAFE_NO_PAD.encode(&Sha256::digest(b"synthetic-access")[..16])
                            });
                            if !refresh {
                                claims["nonce"] = nonce.lock().unwrap().clone().unwrap().into();
                            }
                            for (key, value) in overrides.lock().unwrap().as_object().unwrap() {
                                claims[key] = value.clone();
                            }
                            json!({
                                "id_token":sign(&signing_key,&claims),
                                "access_token":"synthetic-access","refresh_token":"synthetic-refresh",
                                "token_type":"Bearer","expires_in":3600
                            })
                        }
                        _ => json!({"error":"unexpected"}),
                    };
                    let body = body.to_string();
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            })
        };
        Self {
            issuer,
            certificate: pem,
            signing_key,
            overrides,
            nonce,
            challenge,
            token_requests,
            redirect_discovery,
            task,
        }
    }

    fn config(&self) -> OidcConfig {
        OidcConfig {
            issuer: self.issuer.clone(),
            client_id: "consolecrypt-fixture".into(),
            client_secret: None,
            scopes: vec!["profile".into()],
            certificate_authority_pem: Some(self.certificate.clone()),
        }
    }

    fn callback(&self, flow: &LoginFlow) -> Url {
        let parameters: std::collections::BTreeMap<_, _> = flow
            .authorization_url()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(
            parameters.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        *self.nonce.lock().unwrap() = Some(parameters["nonce"].clone());
        *self.challenge.lock().unwrap() = Some(parameters["code_challenge"].clone());
        let mut callback = flow.redirect_uri().clone();
        callback
            .query_pairs_mut()
            .append_pair("code", "synthetic-code")
            .append_pair("state", &parameters["state"]);
        callback
    }
}

#[tokio::test]
async fn browser_pkce_with_custom_ca_and_refresh_verifies_signed_identity() {
    let issuer = TestIssuer::start().await;
    let client = OidcClient::discover(issuer.config(), CancellationToken::new())
        .await
        .unwrap();
    let flow = client
        .begin(Url::parse("http://127.0.0.1:32123/callback").unwrap())
        .unwrap();
    let callback = issuer.callback(&flow);
    let tokens = flow
        .finish(&callback, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(tokens.access_token.expose_secret(), "synthetic-access");
    let Authentication::Bearer(bearer) = tokens.credential().unwrap().authentication else {
        panic!("expected ID token");
    };
    assert_eq!(bearer.expose_secret(), tokens.id_token.expose_secret());
    assert!(!format!("{tokens:?} {client:?}").contains("synthetic"));
    let refreshed = client
        .refresh(
            tokens.refresh_token.clone().unwrap(),
            Some(&tokens),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(refreshed.subject.expose_secret(), "fixture-subject");
    assert_eq!(issuer.token_requests.lock().unwrap().len(), 2);
    *issuer.overrides.lock().unwrap() = json!({"sub":"different-subject"});
    assert_eq!(
        client
            .refresh(
                refreshed.refresh_token.clone().unwrap(),
                Some(&refreshed),
                CancellationToken::new()
            )
            .await
            .unwrap_err(),
        AuthError::InvalidToken
    );
}

#[tokio::test]
async fn callback_origin_path_state_and_duplicates_reject_before_token_exchange() {
    let issuer = TestIssuer::start().await;
    let client = OidcClient::discover(issuer.config(), CancellationToken::new())
        .await
        .unwrap();
    for change in 0..6 {
        let flow = client
            .begin(Url::parse("consolecrypt://kubernetes/oauth/callback").unwrap())
            .unwrap();
        let mut callback = issuer.callback(&flow);
        match change {
            0 => {
                callback.set_host(Some("wrong")).unwrap();
            }
            1 => callback.set_path("/other"),
            2 => callback.set_query(Some("code=synthetic-code&state=wrong")),
            3 => {
                callback.query_pairs_mut().append_pair("state", "second");
            }
            4 => {
                callback.query_pairs_mut().append_pair("code", "second");
            }
            _ => callback.set_fragment(Some("fragment")),
        }
        assert_eq!(
            flow.finish(&callback, CancellationToken::new())
                .await
                .unwrap_err(),
            AuthError::InvalidCallback
        );
    }
    assert!(issuer.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn signed_tokens_with_wrong_claims_are_rejected() {
    let issuer = TestIssuer::start().await;
    let client = OidcClient::discover(issuer.config(), CancellationToken::new())
        .await
        .unwrap();
    for overrides in [
        json!({"iss":"https://wrong.example.test"}),
        json!({"aud":"other-client"}),
        json!({"aud":["consolecrypt-fixture","second"]}),
        json!({"azp":"other-client"}),
        json!({"exp":1}),
        json!({"iat":u64::MAX}),
        json!({"nbf":u64::MAX}),
        json!({"nonce":"wrong"}),
        json!({"sub":""}),
        json!({"at_hash":"wrong"}),
    ] {
        *issuer.overrides.lock().unwrap() = overrides;
        let flow = client
            .begin(Url::parse("http://127.0.0.1:32123/callback").unwrap())
            .unwrap();
        let callback = issuer.callback(&flow);
        assert_eq!(
            flow.finish(&callback, CancellationToken::new())
                .await
                .unwrap_err(),
            AuthError::InvalidToken
        );
    }
}

#[tokio::test]
async fn untrusted_tls_redirects_and_cancelled_requests_are_rejected() {
    let issuer = TestIssuer::start().await;
    let mut config = issuer.config();
    config.certificate_authority_pem = None;
    assert_eq!(
        OidcClient::discover(config, CancellationToken::new())
            .await
            .unwrap_err(),
        AuthError::Transport
    );
    issuer
        .redirect_discovery
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        OidcClient::discover(issuer.config(), CancellationToken::new())
            .await
            .unwrap_err(),
        AuthError::Rejected
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        OidcClient::discover(issuer.config(), cancelled)
            .await
            .unwrap_err(),
        AuthError::Cancelled
    );
    for redirect in [
        "http://example.test:32123/callback",
        "http://localhost:32123/callback",
        "http://127.0.0.1/callback",
        "javascript:bad",
        "consolecrypt://other/oauth/callback",
    ] {
        assert!(validate_redirect(&Url::parse(redirect).unwrap()).is_err());
    }
}

#[tokio::test]
async fn token_signature_and_key_selection_cannot_be_bypassed() {
    let issuer = TestIssuer::start().await;
    let claims =
        json!({"iss":issuer.issuer.as_str(),"sub":"fixture","aud":"client","exp":100,"iat":1});
    let raw = sign(&issuer.signing_key, &claims);
    let keys: Jwks = serde_json::from_value(jwks(&issuer.signing_key)).unwrap();
    assert!(verify_jwt(&raw, &keys, &["ES256".into()]).is_ok());
    let mut pieces: Vec<_> = raw.split('.').map(str::to_string).collect();
    pieces[1] = URL_SAFE_NO_PAD.encode(br#"{"sub":"forged"}"#);
    assert!(verify_jwt(&pieces.join("."), &keys, &["ES256".into()]).is_err());
    pieces[0] = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    assert!(verify_jwt(&pieces.join("."), &keys, &["none".into()]).is_err());
    let wrong: Jwks = serde_json::from_value(jwks(&signer())).unwrap();
    assert!(verify_jwt(&raw, &wrong, &["ES256".into()]).is_err());
    let mut duplicate = jwks(&issuer.signing_key);
    let key = duplicate["keys"][0].clone();
    duplicate["keys"].as_array_mut().unwrap().push(key);
    let duplicate: Jwks = serde_json::from_value(duplicate).unwrap();
    assert!(verify_jwt(&raw, &duplicate, &["ES256".into()]).is_err());
}

#[test]
fn rsa_sha256_public_fixture_verifies_and_modified_signature_fails() {
    // Contains only public verification material; its synthetic signing key
    // was discarded when the fixture was generated.
    let fixture: Value = serde_json::from_str(include_str!("testdata/rs256.json")).unwrap();
    let keys: Jwks = serde_json::from_value(fixture["jwks"].clone()).unwrap();
    let raw = fixture["token"].as_str().unwrap();
    let claims = verify_jwt(raw, &keys, &["RS256".into()]).unwrap();
    assert_eq!(claims.sub.expose_secret(), "fixture-subject");
    let mut pieces: Vec<_> = raw.split('.').map(str::to_owned).collect();
    let mut signature = URL_SAFE_NO_PAD.decode(&pieces[2]).unwrap();
    signature[0] ^= 1;
    pieces[2] = URL_SAFE_NO_PAD.encode(signature);
    assert!(verify_jwt(&pieces.join("."), &keys, &["RS256".into()]).is_err());
}
