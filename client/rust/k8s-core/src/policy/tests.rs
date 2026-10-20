use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{service_fn, ServiceExt};

#[derive(Default)]
struct MemoryAudit(Mutex<Vec<AuditEntry>>);

impl Audit for MemoryAudit {
    fn record(&self, entry: AuditEntry) -> Result<(), BoxError> {
        self.0.lock().unwrap().push(entry);
        Ok(())
    }
}

fn policy() -> Policy {
    Policy {
        context_id: Uuid::new_v4(),
        context_name: "Production".into(),
        production: true,
        device: DeviceClass::Desktop,
        allow_exec_in_production: false,
        allow_port_forward_in_production: false,
    }
}

fn guard_with(policy: Policy) -> ProdGuard {
    ProdGuard::new(policy, false, Arc::new(MemoryAudit::default())).unwrap()
}

#[test]
fn network_forward_requires_non_production_desktop_and_explicit_approval() {
    for production in [false, true] {
        for device in [DeviceClass::Desktop, DeviceClass::Mobile] {
            let mut settings = policy();
            settings.production = production;
            settings.device = device;
            settings.allow_port_forward_in_production = true;
            let guard = guard_with(settings);
            assert!(guard.allow_network_forward(false).is_err());
            assert_eq!(
                guard.allow_network_forward(true).is_ok(),
                !production && device == DeviceClass::Desktop
            );
            if production && device == DeviceClass::Desktop {
                guard.open_write_window("Production").unwrap();
                assert!(guard.allow_network_forward(true).is_err());
            }
            guard.close();
            assert!(guard.allow_network_forward(true).is_err());
        }
    }
}

fn check(guard: &ProdGuard, method: &str, path: &str) -> Result<(), PolicyError> {
    guard.dispatch(
        Request::builder()
            .method(method)
            .uri(path)
            .body(())
            .unwrap(),
        |_, _| (),
    )
}

#[test]
fn reads_discovery_resources_and_known_safe_subresources() {
    let guard = guard_with(policy());
    for path in [
        "/version",
        "/healthz",
        "/livez",
        "/readyz",
        "/api",
        "/api/v1",
        "/apis",
        "/apis/apps",
        "/apis/apps/v1",
        "/openapi/v2",
        "/openapi/v3/apis/apps/v1?hash=abcd",
        "/api/v1/namespaces",
        "/api/v1/namespaces/default",
        "/api/v1/namespaces/default/status",
        "/api/v1/namespaces/default/pods",
        "/api/v1/namespaces/default/pods/app",
        "/api/v1/namespaces/default/pods/app/log?follow=true&container=app",
        "/api/v1/pods?watch=true&resourceVersion=123",
        "/apis/apps/v1/namespaces/default/deployments/app/scale",
        "/apis/metrics.k8s.io/v1beta1/nodes",
        "/apis/kustomize.toolkit.fluxcd.io/v1/namespaces/flux-system/kustomizations",
        "/apis/example.test/v1/widgets?limit=100&continue=opaque%2Fvalue",
    ] {
        for method in ["GET", "HEAD"] {
            assert_eq!(check(&guard, method, path), Ok(()), "{method} {path}");
        }
    }
}

#[tokio::test]
async fn all_mutation_routes_are_rejected_before_transport_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let service = service_fn(move |_request: Request<String>| {
        seen.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, BoxError>(()) }
    });
    let guard = guard_with(policy());
    let protected = GuardLayer(guard).layer(service);
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        for path in [
            "/api/v1/namespaces/default/pods",
            "/api/v1/namespaces/default/pods/app",
            "/api/v1/namespaces/default/pods/app/eviction",
            "/api/v1/namespaces/default/pods/app/ephemeralcontainers",
            "/api/v1/nodes/worker",
            "/api/v1/namespaces/default/secrets/sh.helm.release.v1.app.v1",
            "/api/v1/namespaces/default/configmaps/app",
            "/apis/apps/v1/namespaces/default/deployments/app/scale",
            "/apis/apps/v1/namespaces/default/deployments/app?dryRun=All",
            "/apis/apps/v1/namespaces/default/deployments/app?fieldManager=flux&force=true",
            "/apis/argoproj.io/v1alpha1/namespaces/default/applications/app",
            "/apis/kustomize.toolkit.fluxcd.io/v1/namespaces/default/kustomizations/app",
            "/apis/helm.toolkit.fluxcd.io/v2/namespaces/default/helmreleases/app",
            "/apis/source.toolkit.fluxcd.io/v1/namespaces/default/gitrepositories/app",
            "/apis/source.toolkit.fluxcd.io/v1/namespaces/default/ocirepositories/app",
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .body("secret body that must not reach transport".to_owned())
                .unwrap();
            let error = protected.clone().oneshot(request).await.unwrap_err();
            assert_eq!(
                error.downcast_ref::<PolicyError>(),
                Some(&PolicyError::ReadOnly),
                "{method} {path}"
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn encoded_proxy_and_unknown_stream_routes_cannot_masquerade_as_reads() {
    let guard = guard_with(policy());
    for path in [
        "/api/v1//pods",
        "/api/v1/./pods",
        "/api/v1/namespaces/default/../pods",
        "/api/v1/namespaces/default/pods/app/%65xec",
        "/api/v1/namespaces/default/pods/app%2fexec",
        "/api/v1/namespaces/default/pods/app%252fexec",
        "/api/v1/namespaces/default/pods/app\\exec",
        "/api/v1/proxy/namespaces/default/pods/app",
        "/api/v1/namespaces/default/pods/app/proxy",
        "/api/v1/namespaces/default/services/app:8080/proxy/mutate",
        "/api/v1/nodes/node/proxy",
        "/api/v1/watch/namespaces/default/pods",
        "/apis/example.test/v1/namespaces/default/widgets/app/exec",
        "/apis/apps/v1/namespaces/default/deployments/app/unknown",
        "https://other.example.test/api/v1/pods",
        "//other.example.test/api/v1/pods",
    ] {
        assert_eq!(
            check(&guard, "GET", path),
            Err(PolicyError::UnsupportedPath),
            "{path}"
        );
    }
    for method in ["CONNECT", "OPTIONS", "TRACE"] {
        assert!(check(&guard, method, "/api/v1/pods").is_err());
    }
}

#[test]
fn exec_and_attach_have_their_own_permission_even_with_a_write_window() {
    let guard = guard_with(policy());
    guard.open_write_window("Production").unwrap();
    for endpoint in ["exec", "attach"] {
        let path = format!("/api/v1/namespaces/default/pods/app/{endpoint}?command=rm");
        for method in ["GET", "POST"] {
            assert_eq!(check(&guard, method, &path), Err(PolicyError::ExecDisabled));
        }
    }
    let mut enabled = policy();
    enabled.allow_exec_in_production = true;
    let guard = guard_with(enabled);
    assert_eq!(
        check(&guard, "GET", "/api/v1/namespaces/default/pods/app/exec"),
        Ok(())
    );
    assert_eq!(
        check(&guard, "POST", "/api/v1/namespaces/default/pods/app/attach"),
        Ok(())
    );
}

#[test]
fn write_window_expires_and_closes_across_cloned_transports() {
    let guard = guard_with(policy());
    let other = guard.clone();
    let target = "/apis/apps/v1/namespaces/default/deployments/app";
    assert_eq!(
        guard.open_write_window("production"),
        Err(PolicyError::ContextNameMismatch)
    );
    assert_eq!(check(&guard, "PATCH", target), Err(PolicyError::ReadOnly));
    guard.open_write_window("Production").unwrap();
    assert!(guard.write_window_remaining().unwrap() <= WRITE_WINDOW);
    assert_eq!(check(&other, "PATCH", target), Ok(()));
    guard.0.state.lock().unwrap().write_until = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(guard.write_window_remaining(), None);
    assert_eq!(check(&other, "PATCH", target), Err(PolicyError::ReadOnly));
    guard.open_write_window("Production").unwrap();
    other.close_write_window();
    assert_eq!(check(&guard, "PATCH", target), Err(PolicyError::ReadOnly));
}

#[test]
fn mobile_cannot_open_or_use_a_write_window() {
    let mut mobile = policy();
    mobile.device = DeviceClass::Mobile;
    let guard = guard_with(mobile);
    assert_eq!(
        guard.open_write_window("Production"),
        Err(PolicyError::MobileWriteWindow)
    );
    // Even accidental state restoration from a desktop cannot grant writes.
    guard.0.state.lock().unwrap().write_until = Some(Instant::now() + WRITE_WINDOW);
    assert_eq!(
        check(&guard, "DELETE", "/api/v1/namespaces/default/pods/app"),
        Err(PolicyError::ReadOnly)
    );
}

#[test]
fn audit_records_metadata_before_dispatch_and_never_records_query_or_body() {
    let audit = Arc::new(MemoryAudit::default());
    let guard = ProdGuard::new(policy(), false, audit.clone()).unwrap();
    guard.open_write_window("Production").unwrap();
    let path = "/api/v1/namespaces/default/secrets/app";
    let request = Request::builder()
        .method("PATCH")
        .uri(format!("{path}?fieldManager=private-query"))
        .header("Authorization", "Bearer private-token")
        .body("private-secret-body")
        .unwrap();
    guard
        .dispatch(request, |_, _| {
            assert_eq!(audit.0.lock().unwrap().len(), 1);
        })
        .unwrap();
    let entries = audit.0.lock().unwrap();
    assert_eq!(entries[0].path, path);
    assert_eq!(entries[0].method, Method::PATCH);
    assert!(!format!("{entries:?}").contains("private-"));
}

#[test]
fn audit_failure_prevents_dispatch() {
    struct FailingAudit;
    impl Audit for FailingAudit {
        fn record(&self, _: AuditEntry) -> Result<(), BoxError> {
            Err("storage failure with confidential details".into())
        }
    }
    let guard = ProdGuard::new(policy(), false, Arc::new(FailingAudit)).unwrap();
    guard.open_write_window("Production").unwrap();
    assert_eq!(
        guard.dispatch(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/namespaces/default/secrets/app")
                .body(())
                .unwrap(),
            |_, _| panic!("request must not reach transport")
        ),
        Err(PolicyError::AuditUnavailable)
    );
}

fn forwarding_guard() -> ProdGuard {
    let mut p = policy();
    p.allow_port_forward_in_production = true;
    guard_with(p)
}

fn forward_uri() -> Uri {
    "/api/v1/namespaces/default/pods/app/portforward?ports=5432"
        .parse()
        .unwrap()
}

fn forward_request(confirmation: ForwardConfirmation) -> Request<()> {
    Request::builder()
        .method("GET")
        .uri(forward_uri())
        .extension(confirmation)
        .body(())
        .unwrap()
}

#[test]
fn forwarding_requires_flag_and_confirmation() {
    let disabled = guard_with(policy());
    assert!(matches!(
        disabled.confirm_port_forward(Method::GET, forward_uri()),
        Err(PolicyError::PortForwardDisabled)
    ));
    let enabled = forwarding_guard();
    assert_eq!(
        check(&enabled, "GET", &forward_uri().to_string()),
        Err(PolicyError::ConfirmationRequired)
    );
    let confirmation = enabled
        .confirm_port_forward(Method::GET, forward_uri())
        .unwrap();
    assert_eq!(
        format!("{confirmation:?}"),
        "ForwardConfirmation([redacted])"
    );
    assert_eq!(
        enabled.dispatch(forward_request(confirmation.clone()), |_, _| ()),
        Ok(())
    );
    assert_eq!(
        enabled.dispatch(forward_request(confirmation), |_, _| ()),
        Err(PolicyError::ConfirmationRequired)
    );
}

#[test]
fn forwarding_confirmation_cannot_move_between_contexts_methods_ports_or_time() {
    let guard = forwarding_guard();
    let confirmation = guard
        .confirm_port_forward(Method::GET, forward_uri())
        .unwrap();
    let other = forwarding_guard();
    assert_eq!(
        other.dispatch(forward_request(confirmation.clone()), |_, _| ()),
        Err(PolicyError::ConfirmationRequired)
    );
    let mut request = forward_request(confirmation.clone());
    *request.method_mut() = Method::POST;
    assert_eq!(
        guard.dispatch(request, |_, _| ()),
        Err(PolicyError::ConfirmationRequired)
    );
    let mut request = forward_request(confirmation.clone());
    *request.uri_mut() = "/api/v1/namespaces/default/pods/app/portforward?ports=6379"
        .parse()
        .unwrap();
    assert_eq!(
        guard.dispatch(request, |_, _| ()),
        Err(PolicyError::ConfirmationRequired)
    );
    let mut expired = confirmation;
    expired.expires = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        guard.dispatch(forward_request(expired), |_, _| ()),
        Err(PolicyError::ConfirmationRequired)
    );
}

#[test]
fn closed_session_revokes_all_clones_and_pending_confirmations() {
    let guard = forwarding_guard();
    let other = guard.clone();
    guard.open_write_window("Production").unwrap();
    let confirmation = guard
        .confirm_port_forward(Method::GET, forward_uri())
        .unwrap();
    guard.close();
    assert_eq!(
        check(&other, "GET", "/api/v1/pods"),
        Err(PolicyError::Closed)
    );
    assert_eq!(
        other.open_write_window("Production"),
        Err(PolicyError::Closed)
    );
    assert_eq!(
        other.dispatch(forward_request(confirmation), |_, _| ()),
        Err(PolicyError::Closed)
    );
    assert_eq!(other.write_window_remaining(), None);
}

#[tokio::test]
async fn closing_after_readiness_still_prevents_transport_call() {
    let guard = guard_with(policy());
    let service = service_fn(|_: Request<()>| async {
        panic!("closed request reached transport");
        #[allow(unreachable_code)]
        Ok::<_, BoxError>(())
    });
    let mut protected = GuardLayer(guard.clone()).layer(service);
    protected.ready().await.unwrap();
    guard.close();
    let error = protected
        .call(Request::builder().uri("/api/v1/pods").body(()).unwrap())
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<PolicyError>(),
        Some(&PolicyError::Closed)
    );
}

#[tokio::test]
async fn a_future_created_before_lock_cannot_later_send_the_request() {
    let guard = guard_with(policy());
    let polls = Arc::new(AtomicUsize::new(0));
    let seen = polls.clone();
    let service = service_fn(move |_: Request<()>| {
        let seen = seen.clone();
        async move {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok::<_, BoxError>(())
        }
    });
    let mut protected = GuardLayer(guard.clone()).layer(service);
    let pending = protected.call(Request::builder().uri("/api/v1/pods").body(()).unwrap());
    guard.close();
    let error = pending.await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<PolicyError>(),
        Some(&PolicyError::Closed)
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn closing_write_window_cancels_pending_writes_but_keeps_reads() {
    let guard = guard_with(policy());
    guard.open_write_window("Production").unwrap();
    let polls = Arc::new(AtomicUsize::new(0));
    let seen = polls.clone();
    let service = service_fn(move |_: Request<()>| {
        let seen = seen.clone();
        async move {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok::<_, BoxError>(())
        }
    });
    let mut protected = GuardLayer(guard.clone()).layer(service);
    let pending = protected.call(
        Request::builder()
            .method("DELETE")
            .uri("/api/v1/namespaces/default/pods/app")
            .body(())
            .unwrap(),
    );
    guard.close_write_window();
    let error = pending.await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<PolicyError>(),
        Some(&PolicyError::ReadOnly)
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    protected
        .oneshot(Request::builder().uri("/api/v1/pods").body(()).unwrap())
        .await
        .unwrap();
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[test]
fn production_never_accepts_insecure_tls() {
    assert!(matches!(
        ProdGuard::new(policy(), true, Arc::new(MemoryAudit::default())),
        Err(PolicyError::InsecureProductionTls)
    ));
}

#[test]
fn only_self_reviews_are_exempt_from_production_write_restriction() {
    let guard = guard_with(policy());
    for kind in ["selfsubjectaccessreviews", "selfsubjectrulesreviews"] {
        assert_eq!(
            check(
                &guard,
                "POST",
                &format!("/apis/authorization.k8s.io/v1/{kind}")
            ),
            Ok(())
        );
    }
    for path in [
        "/apis/authorization.k8s.io/v1/subjectaccessreviews",
        "/apis/authorization.k8s.io/v1/localsubjectaccessreviews",
        "/apis/authentication.k8s.io/v1/tokenreviews",
        "/apis/example.test/v1/selfsubjectaccessreviews",
        "/apis/authorization.k8s.io/v1/selfsubjectaccessreviews/name",
    ] {
        assert_eq!(
            check(&guard, "POST", path),
            Err(PolicyError::ReadOnly),
            "{path}"
        );
    }
}

#[test]
fn nonproduction_mutations_do_not_require_a_window() {
    let mut p = policy();
    p.production = false;
    p.device = DeviceClass::Mobile;
    let guard = guard_with(p);
    assert_eq!(
        check(
            &guard,
            "PATCH",
            "/apis/apps/v1/namespaces/default/deployments/app/scale"
        ),
        Ok(())
    );
    assert_eq!(
        check(&guard, "POST", "/api/v1/namespaces/default/pods/app/exec"),
        Ok(())
    );
    // Proxy endpoints are never an escape hatch, even outside production.
    assert_eq!(
        check(&guard, "GET", "/api/v1/nodes/node/proxy"),
        Err(PolicyError::UnsupportedPath)
    );
}
