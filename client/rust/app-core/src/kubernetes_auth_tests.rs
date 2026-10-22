use super::*;
use crate::kubernetes_import::KubeImportChoice;
use cc_models::kubernetes::KubeRoute;
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn fixture() -> (tempfile::TempDir, AppCore, KubeContext, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("synthetic-auth-plugin");
    let marker = directory.path().join("executions");
    std::fs::write(&binary,format!(
        "#!/bin/sh\nprintf 'executed\\n' >> '{}'\nprintf '%s' '{{\"apiVersion\":\"client.authentication.k8s.io/v1\",\"kind\":\"ExecCredential\",\"status\":{{\"token\":\"synthetic-facade-token\"}}}}'\n",
        marker.display()
    )).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().join("app").to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("auth".into(), "auth-test-passphrase".into())
        .await
        .unwrap();
    let yaml = format!(
        "apiVersion: v1\nkind: Config\nclusters:\n- name: fixture\n  cluster:\n    server: https://api.example.test\nusers:\n- name: user\n  user:\n    exec:\n      command: '{}'\n      args: [synthetic-private-argument]\n      env:\n      - name: SYNTHETIC_VALUE\n        value: synthetic-private-environment\n      apiVersion: client.authentication.k8s.io/v1\n      interactiveMode: Never\ncontexts:\n- name: fixture\n  context:\n    cluster: fixture\n    user: user\n",
        binary.display()
    );
    let preview = app
        .kube_import_preview(Zeroizing::new(yaml.into_bytes()), None)
        .await
        .unwrap();
    let mut contexts = app
        .kube_import_commit(
            preview.ticket,
            vec![KubeImportChoice {
                index: 0,
                production: true,
                route: KubeRoute::Direct,
                approved_files: vec![],
            }],
        )
        .await
        .unwrap();
    (directory, app, contexts.remove(0), marker)
}

async fn resolve(app: &AppCore, id: ObjectId) -> AppResult<ResolvedAuth> {
    let (_, unlocked) = app.unlocked().await?;
    let (context, cluster, _) = connection(&unlocked, id)?;
    unlocked
        .kube
        .auth
        .resolve(&unlocked, &context, &cluster, CancellationToken::new())
        .await
}

#[tokio::test]
async fn approval_is_explicit_local_one_use_and_token_cache_stays_native() {
    let (_directory, app, context, marker) = fixture().await;
    assert!(!marker.exists());
    assert!(resolve(&app, context.id)
        .await
        .unwrap_err()
        .to_string()
        .contains("exec_approval_required"));
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    assert!(review
        .arguments
        .contains(&"synthetic-private-argument".into()));
    assert_eq!(
        review.environment["SYNTHETIC_VALUE"],
        "synthetic-private-environment"
    );
    assert!(!format!("{review:?}").contains("synthetic"));
    assert!(!marker.exists(), "review must never run a plugin");
    app.kube_exec_approve(review.ticket.clone()).await.unwrap();
    assert!(!marker.exists(), "consent alone must not run a plugin");
    assert!(app.kube_exec_approve(review.ticket.clone()).await.is_err());
    let resolved = resolve(&app, context.id).await.unwrap();
    let Authentication::Bearer(token) = resolved.authentication else {
        panic!("bearer required");
    };
    assert_eq!(token.expose_secret(), "synthetic-facade-token");
    assert!(resolved.expires_at.is_some());
    resolve(&app, context.id).await.unwrap();
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);
    let (_, old) = app.unlocked().await.unwrap();
    app.lock().await.unwrap();
    assert!(old.kube.auth.state.lock().unwrap().cache.is_empty());
    assert!(old.kube.auth.state.lock().unwrap().approved.is_empty());
    app.unlock_with_passphrase("auth-test-passphrase".into())
        .await
        .unwrap();
    assert!(resolve(&app, context.id)
        .await
        .unwrap_err()
        .to_string()
        .contains("exec_approval_required"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changes_cancel_review_and_revoke_cached_credentials_before_reuse() {
    let (_directory, app, mut context, _) = fixture().await;
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    let (_, unlocked) = app.unlocked().await.unwrap();
    context.name = "renamed-before-consent".into();
    unlocked
        .writer
        .put(VaultObject::KubeContext(context.clone()))
        .await
        .unwrap();
    assert!(app.kube_exec_approve(review.ticket.clone()).await.is_err());
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    app.kube_exec_approve(review.ticket.clone()).await.unwrap();
    resolve(&app, context.id).await.unwrap();
    context.name = "changed-after-consent".into();
    unlocked
        .writer
        .put(VaultObject::KubeContext(context.clone()))
        .await
        .unwrap();
    assert!(resolve(&app, context.id)
        .await
        .unwrap_err()
        .to_string()
        .contains("exec_approval_required"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancel_and_profile_switch_discard_secret_reviews() {
    let (_directory, app, context, _) = fixture().await;
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    app.kube_auth_cancel(review.ticket.clone()).await.unwrap();
    assert!(app.kube_exec_approve(review.ticket.clone()).await.is_err());
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    app.create_local_profile("other".into(), "other-auth-passphrase".into())
        .await
        .unwrap();
    assert!(app.kube_exec_approve(review.ticket.clone()).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_also_reaches_an_already_consumed_operation() {
    let (_directory, app, context, _) = fixture().await;
    let review = app.kube_exec_review(context.id.to_string()).await.unwrap();
    let (_, unlocked) = app.unlocked().await.unwrap();
    let (pending, _operation) = unlocked
        .kube
        .auth
        .take(review.ticket.parse().unwrap())
        .unwrap();
    assert!(!pending.cancel.is_cancelled());
    app.kube_auth_cancel(review.ticket.clone()).await.unwrap();
    assert!(pending.cancel.is_cancelled());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn loopback_listener_ignores_wrong_host_and_state_and_accepts_bound_callback() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let issuer = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer_url = format!(
        "https://localhost:{}/issuer",
        issuer.local_addr().unwrap().port()
    );
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let issuer_pem = certificate.cert.pem();
    let tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )
        .unwrap();
    let metadata = serde_json::json!({
        "issuer":issuer_url,
        "authorization_endpoint":format!("{issuer_url}/authorize"),
        "token_endpoint":format!("{issuer_url}/token"),
        "jwks_uri":format!("{issuer_url}/jwks"),
        "code_challenge_methods_supported":["S256"],
        "id_token_signing_alg_values_supported":["RS256"]
    })
    .to_string();
    let server = tokio::spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let (socket, _) = issuer.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{metadata}",
            metadata.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let client = OidcClient::discover(
        OidcConfig {
            issuer: Url::parse(&issuer_url).unwrap(),
            client_id: "fixture".into(),
            client_secret: None,
            scopes: vec![],
            certificate_authority_pem: Some(issuer_pem),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    server.await.unwrap();
    let callback = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = callback.local_addr().unwrap();
    let redirect = Url::parse(&format!("http://{address}/oauth/native-fixture")).unwrap();
    let flow = client.begin(redirect).unwrap();
    let state = flow
        .authorization_url()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let worker = tokio::spawn(async move {
        loopback_callback(
            callback,
            &flow,
            CancellationToken::new(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
    });
    for (host, provided_state, expected) in [
        ("attacker.example.test".to_string(), state.clone(), "400"),
        (address.to_string(), "wrong-state".into(), "400"),
        (address.to_string(), state, "200"),
    ] {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let request=format!("GET /oauth/native-fixture?code=synthetic-code&state={provided_state} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {expected}")));
        assert!(!response.contains("synthetic-code"));
    }
    let result = worker.await.unwrap().unwrap();
    assert!(result
        .query_pairs()
        .any(|(key, value)| key == "code" && value == "synthetic-code"));
}
