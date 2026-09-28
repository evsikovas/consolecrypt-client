//! First vertical milestone against a LIVE ConsoleCrypt server
//! (PARALLEL_AGENTS_PROMPT "Первый вертикальный end-to-end milestone"):
//!
//! ```text
//! Register → Create Vault → Create encrypted Host objects → Push →
//! second client login → Pull ciphertext → Decrypt locally → Hosts appear →
//! SSH connection works (here: through a 2-hop jump chain)
//! ```
//!
//! Uses the real client crypto (vault-core) and raw `cc-protocol` DTOs over
//! HTTP, so it checks wire compatibility with the server independently of
//! sync-core. Run with:
//!
//! ```sh
//! CC_E2E_SERVER=http://localhost:8080 cargo test -p cc-app-core --test e2e_live_server -- --nocapture
//! # add CC_SSH_IT=1 to also connect over SSH to the Docker testbed
//! ```

use cc_models::credential::{Credential, CredentialKind};
use cc_models::host::{Host, HostKeyPolicy};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{KekClass, ObjectPayload, VaultObject};
use cc_protocol::auth::{AuthResponse, LoginRequest, RegisterRequest};
use cc_protocol::devices::DeviceStatus;
use cc_protocol::envelopes::{KeyEnvelope, RecipientType};
use cc_protocol::recovery::VaultRecoveryMaterial;
use cc_protocol::sync::{
    ChangesResponse, Mutation, MutationOp, MutationResult, PushRequest, PushResponse,
};
use cc_protocol::vaults::{ListEnvelopesResponse, ListVaultsResponse, VaultInfo};
use cc_protocol::version::{
    Platform, HEADER_CLIENT_VERSION, HEADER_PLATFORM, HEADER_PROTOCOL_VERSION,
};
use cc_protocol::{
    paths, ApiError, DeviceId, ErrorCode, MutationId, ObjectId, VaultId, PROTOCOL_VERSION,
};
use cc_ssh_core::{
    ConnectionPlanner, HostKeyDecision, MemoryCredentialResolver, MemoryInventory,
    MemoryKnownHosts, PlannerDefaults, ResolvedCredential,
};
use cc_vault_core::{
    create_vault, unlock_with_device, unlock_with_passphrase, unlock_with_recovery_input,
    Argon2Params, DeviceIdentity, SecretString, UnlockedVault,
};
use reqwest::StatusCode;
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;

const ACCOUNT_PASSWORD: &str = "e2e-correct-horse-battery-staple";
const VAULT_PASSPHRASE: &str = "e2e vault passphrase 42";

struct Api {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    /// Protocol 1.5: authenticated requests carry `x-cc-device-proof`.
    signer: Option<Arc<DeviceIdentity>>,
}

impl Api {
    fn new(base: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_owned(),
            token: None,
            signer: None,
        }
    }

    /// `path` is the exact request target (path + query) and `body` the exact
    /// bytes sent, as bound into the per-request device proof.
    fn req(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> reqwest::RequestBuilder {
        let proof = match (&self.token, &self.signer) {
            (Some(_), Some(id)) => Some(
                id.request_proof(
                    method.as_str(),
                    path,
                    body.as_deref().unwrap_or_default(),
                    chrono::Utc::now(),
                )
                .expect("request proof")
                .encode(),
            ),
            _ => None,
        };
        let mut r = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .header(HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION.to_string())
            .header(HEADER_CLIENT_VERSION, env!("CARGO_PKG_VERSION"))
            .header(HEADER_PLATFORM, Platform::Cli.as_str());
        if let Some(t) = &self.token {
            r = r.bearer_auth(t);
        }
        if let Some(p) = proof {
            r = r.header(cc_protocol::version::HEADER_DEVICE_PROOF, p);
        }
        if let Some(b) = body {
            r = r
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(b);
        }
        r
    }

    async fn send<R: DeserializeOwned>(
        &self,
        r: reqwest::RequestBuilder,
    ) -> Result<(StatusCode, R), (StatusCode, ApiError)> {
        let resp = r.send().await.expect("server reachable");
        let status = resp.status();
        let bytes = resp.bytes().await.expect("body");
        if status.is_success() || status == StatusCode::CONFLICT {
            if let Ok(v) = serde_json::from_slice::<R>(&bytes) {
                return Ok((status, v));
            }
        }
        let err: ApiError = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            panic!(
                "status {status}: unexpected body {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        Err((status, err))
    }

    async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(StatusCode, R), (StatusCode, ApiError)> {
        let bytes = serde_json::to_vec(body).expect("serialize body");
        self.send(self.req(reqwest::Method::POST, path, Some(bytes)))
            .await
    }

    async fn get<R: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<(StatusCode, R), (StatusCode, ApiError)> {
        self.send(self.req(reqwest::Method::GET, path, None)).await
    }
}

fn ok<T>(r: Result<(StatusCode, T), (StatusCode, ApiError)>, what: &str) -> T {
    match r {
        Ok((_, v)) => v,
        Err((s, e)) => panic!(
            "{what} failed: {s} {:?} {} (request_id {:?})",
            e.code, e.message, e.request_id
        ),
    }
}

fn put(vault: &UnlockedVault, object: VaultObject, base_revision: i64) -> Mutation {
    let object_id = object.id();
    let body = vault
        .encrypt_object(object_id, base_revision + 1, &ObjectPayload::new(object))
        .expect("encrypt");
    Mutation {
        mutation_id: MutationId::new(),
        object_id,
        base_revision,
        op: MutationOp::Put { body },
    }
}

async fn login(
    base: &str,
    email: &str,
    identity: &Arc<DeviceIdentity>,
    name: &str,
) -> (Api, AuthResponse) {
    let mut api = Api::new(base);
    let auth: AuthResponse = ok(
        api.post(
            paths::AUTH_LOGIN,
            &LoginRequest {
                email: email.into(),
                password: ACCOUNT_PASSWORD.into(),
                device: identity.registration(name, Platform::Cli, None).unwrap(),
                device_proof: Some(identity.login_proof(chrono::Utc::now()).unwrap()),
            },
        )
        .await,
        "login",
    );
    api.token = Some(auth.tokens.access_token.expose_secret().to_owned());
    api.signer = Some(identity.clone());
    (api, auth)
}

async fn pull_all(api: &Api, vault_id: VaultId) -> ChangesResponse {
    ok(
        api.get(&format!(
            "{}?vault_id={vault_id}&after=0",
            paths::SYNC_CHANGES
        ))
        .await,
        "changes",
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn vertical_slice_against_live_server() {
    let Ok(base) = std::env::var("CC_E2E_SERVER") else {
        eprintln!("skipped: set CC_E2E_SERVER=http://localhost:8080 to run against a live server");
        return;
    };
    let with_ssh = cc_ssh_core::testing::enabled();
    let testbed = if with_ssh {
        Some(Arc::new(
            tokio::task::spawn_blocking(|| cc_ssh_core::testing::SshTestbed::start(true))
                .await
                .unwrap()
                .expect("ssh testbed"),
        ))
    } else {
        eprintln!("note: CC_SSH_IT not set — SSH step skipped");
        None
    };

    let email = format!("e2e-{}@example.test", uuid::Uuid::new_v4().simple());
    let passphrase = SecretString::from(VAULT_PASSPHRASE.to_owned());

    // ── Device A: register, create vault ────────────────────────────────
    let id_a = Arc::new(DeviceIdentity::generate().unwrap());
    let mut api_a = Api::new(&base);
    let auth_a: AuthResponse = ok(
        api_a
            .post(
                paths::AUTH_REGISTER,
                &RegisterRequest {
                    email: email.clone(),
                    password: ACCOUNT_PASSWORD.into(),
                    device: id_a
                        .registration("E2E device A", Platform::Cli, None)
                        .unwrap(),
                    device_proof: Some(id_a.login_proof(chrono::Utc::now()).unwrap()),
                },
            )
            .await,
        "register",
    );
    assert_eq!(auth_a.device_id, id_a.device_id());
    api_a.token = Some(auth_a.tokens.access_token.expose_secret().to_owned());
    api_a.signer = Some(id_a.clone());

    let created = tokio::task::spawn_blocking({
        let pass = passphrase.clone();
        let base = base.clone();
        let id = id_a.clone();
        move || {
            create_vault(
                &id,
                &pass,
                Argon2Params::for_tests(),
                Some(&base),
                chrono::Utc::now(),
            )
            .unwrap()
        }
    })
    .await
    .unwrap();
    let vault_id = created.create_request.vault_id;
    let vault_a = created.unlocked;
    let info: VaultInfo = ok(
        api_a.post(paths::VAULTS, &created.create_request).await,
        "create vault",
    );
    assert_eq!(info.vault_id, vault_id);
    assert!(info.caller_trusted);

    // ── Device A: encrypted objects (secret, credential, 3 hosts) ───────
    let (ssh_user, ssh_key, b1_port, b2_addr, t_addr) = match &testbed {
        Some(tb) => (
            tb.secrets.user.clone(),
            tb.secrets.key.private_openssh.expose_secret().to_owned(),
            tb.bastion1_port,
            tb.bastion2.clone().unwrap(),
            tb.target.clone().unwrap(),
        ),
        None => (
            "alex".to_owned(),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nplaceholder-not-a-real-key\n-----END OPENSSH PRIVATE KEY-----\n".to_owned(),
            2222,
            "bastion-b.internal".to_owned(),
            "10.10.10.20".to_owned(),
        ),
    };
    let secret = Secret::new(SecretKind::SshPrivateKey, SecretValue::new(ssh_key.clone()));
    let mut cred = Credential::new("e2e key", CredentialKind::SshPrivateKey);
    cred.secret_id = Some(secret.id);
    cred.username = Some(ssh_user.clone());
    let mk_host = |name: &str, addr: &str, port: u16| {
        let mut h = Host::new(name, addr);
        h.port = Some(port);
        h.credential_id = Some(cred.id);
        h.host_key_policy = HostKeyPolicy::AcceptNew;
        h
    };
    let b1 = mk_host("bastion1", "127.0.0.1", b1_port);
    let b2 = mk_host("bastion2", &b2_addr, 22);
    let mut target = mk_host("target", &t_addr, 22);
    target.jump_chain = vec![b1.id, b2.id];
    let target_id = target.id;

    let objects = vec![
        VaultObject::Secret(secret.clone()),
        VaultObject::Credential(cred.clone()),
        VaultObject::Host(b1.clone()),
        VaultObject::Host(b2.clone()),
        VaultObject::Host(target.clone()),
    ];
    let push = PushRequest {
        vault_id,
        device_id: id_a.device_id(),
        mutations: objects.into_iter().map(|o| put(&vault_a, o, 0)).collect(),
    };
    let resp: PushResponse = ok(api_a.post(paths::SYNC_PUSH, &push).await, "push");
    assert_eq!(resp.results.len(), 5);
    assert!(resp
        .results
        .iter()
        .all(|r| matches!(r, MutationResult::Accepted { revision: 1, .. })));
    assert_eq!(resp.latest_sequence, 5);

    // Idempotent retry of the same batch returns the stored results.
    let replay: PushResponse = ok(api_a.post(paths::SYNC_PUSH, &push).await, "push replay");
    assert!(replay
        .results
        .iter()
        .all(|r| matches!(r, MutationResult::Accepted { replayed: true, .. })));

    // Nothing the server stores contains the plaintext.
    let raw = serde_json::to_string(&pull_all(&api_a, vault_id).await.changes).unwrap();
    for needle in [t_addr.as_str(), "bastion", ssh_user.as_str(), "OPENSSH"] {
        assert!(!raw.contains(needle), "server copy leaks {needle:?}");
    }

    // ── Device B: login with the passphrase path ────────────────────────
    let id_b = Arc::new(DeviceIdentity::generate().unwrap());
    let (api_b, auth_b) = login(&base, &email, &id_b, "E2E device B").await;
    assert_eq!(auth_b.device_status, DeviceStatus::Active);

    let vaults: ListVaultsResponse = ok(api_b.get(paths::VAULTS).await, "list vaults");
    let v = vaults
        .vaults
        .iter()
        .find(|v| v.vault_id == vault_id)
        .expect("vault listed");
    assert!(!v.caller_trusted);

    // Untrusted device cannot pull objects …
    match api_b
        .get::<ChangesResponse>(&format!(
            "{}?vault_id={vault_id}&after=0",
            paths::SYNC_CHANGES
        ))
        .await
    {
        Err((StatusCode::FORBIDDEN, e)) => assert_eq!(e.code, ErrorCode::DeviceNotTrusted),
        other => panic!(
            "untrusted pull must be 403, got {:?}",
            other.map(|(s, _)| s)
        ),
    }
    // … but gets exactly what it needs to unlock.
    let material: VaultRecoveryMaterial = ok(
        api_b
            .get(&format!(
                "{}?vault_id={vault_id}",
                paths::RECOVERY_VAULT_ENVELOPE
            ))
            .await,
        "recovery material",
    );
    let pw_env = material.password_envelope.expect("password envelope");
    let vault_b = tokio::task::spawn_blocking({
        let pass = passphrase.clone();
        move || unlock_with_passphrase(vault_id, &pw_env, &pass).unwrap()
    })
    .await
    .unwrap();

    let attest = vault_b.attest_device_request(&id_b).unwrap();
    let _stored: KeyEnvelope = ok(
        api_b
            .post(
                &paths::fill(
                    paths::DEVICE_ATTEST,
                    &[("device_id", &id_b.device_id().to_string())],
                ),
                &attest,
            )
            .await,
        "attest",
    );

    // ── Device B: pull ciphertext, decrypt locally ──────────────────────
    let changes = pull_all(&api_b, vault_id).await;
    assert_eq!(changes.changes.len(), 5);
    assert_eq!(changes.latest_sequence, 5);
    let inventory = Arc::new(MemoryInventory::new());
    let resolver = Arc::new(MemoryCredentialResolver::new());
    let mut secrets = std::collections::HashMap::new();
    let mut creds = Vec::new();
    for ch in &changes.changes {
        let body = ch.body.as_ref().expect("live object");
        let (payload, class) = vault_b
            .decrypt_object(ch.object_id, ch.revision, body, None)
            .unwrap();
        assert_eq!(payload.object.kind().kek_class(), class);
        match payload.object {
            VaultObject::Host(h) => {
                inventory.add_host(h);
            }
            VaultObject::Credential(c) => creds.push(c),
            VaultObject::Secret(s) => {
                assert_eq!(class, KekClass::Secrets);
                secrets.insert(s.id, s);
            }
            other => panic!("unexpected {:?}", other.kind()),
        }
    }
    for c in creds {
        let s = secrets.get(&c.secret_id.unwrap()).expect("secret synced");
        resolver.insert(
            c.id,
            ResolvedCredential::PrivateKey {
                openssh: s.value.expose_secret().to_owned().into(),
                passphrase: None,
                certificate: None,
            },
        );
        inventory.add_credential(c);
    }
    let plan = ConnectionPlanner::new(inventory.clone())
        .with_defaults(PlannerDefaults::default())
        .plan(target_id)
        .await
        .expect("plan");
    assert_eq!(
        plan.route.len(),
        2,
        "target is reached through bastion1 → bastion2"
    );
    println!(
        "E2E: host synced and decrypted on device B — {}",
        plan.describe()
    );

    // Device B can now also unlock with its own device envelope.
    let envs: ListEnvelopesResponse = ok(
        api_b
            .get(&paths::fill(
                paths::VAULT_ENVELOPES,
                &[("vault_id", &vault_id.to_string())],
            ))
            .await,
        "envelopes",
    );
    let own = envs
        .envelopes
        .iter()
        .find(|e| {
            e.recipient_type == RecipientType::Device
                && e.recipient_id == Some(*id_b.device_id().as_uuid())
        })
        .expect("own device envelope");
    unlock_with_device(vault_id, own, &id_b).expect("device unlock");

    // ── Conflict: B updates target, A pushes a stale revision → 409 ─────
    let mut t2 = target.clone();
    t2.notes = "edited on B".into();
    let r: PushResponse = ok(
        api_b
            .post(
                paths::SYNC_PUSH,
                &PushRequest {
                    vault_id,
                    device_id: id_b.device_id(),
                    mutations: vec![put(&vault_b, VaultObject::Host(t2), 1)],
                },
            )
            .await,
        "push B",
    );
    assert!(matches!(
        r.results[0],
        MutationResult::Accepted {
            revision: 2,
            sequence: 6,
            ..
        }
    ));
    let mut t3 = target.clone();
    t3.notes = "edited on A".into();
    match api_a
        .post::<_, PushResponse>(
            paths::SYNC_PUSH,
            &PushRequest {
                vault_id,
                device_id: id_a.device_id(),
                mutations: vec![put(&vault_a, VaultObject::Host(t3), 1)],
            },
        )
        .await
    {
        Ok((StatusCode::CONFLICT, r)) => assert!(matches!(
            r.results[0],
            MutationResult::Conflict {
                current_revision: 2,
                current_sequence: 6,
                ..
            }
        )),
        other => panic!("expected 409, got {:?}", other.map(|(s, _)| s)),
    }

    // ── Device C: recovery-key path ─────────────────────────────────────
    let id_c = Arc::new(DeviceIdentity::generate().unwrap());
    let (api_c, _) = login(&base, &email, &id_c, "E2E device C").await;
    let material: VaultRecoveryMaterial = ok(
        api_c
            .get(&format!(
                "{}?vault_id={vault_id}",
                paths::RECOVERY_VAULT_ENVELOPE
            ))
            .await,
        "recovery material C",
    );
    let words = SecretString::from(created.recovery_kit.phrase().expose_phrase().to_owned());
    let vault_c =
        unlock_with_recovery_input(vault_id, &material.recovery_envelope.unwrap(), &words)
            .expect("recovery key unlock");
    assert_eq!(vault_c.vault_access_key(), vault_a.vault_access_key());
    let _: KeyEnvelope = ok(
        api_c
            .post(
                &paths::fill(
                    paths::DEVICE_ATTEST,
                    &[("device_id", &id_c.device_id().to_string())],
                ),
                &vault_c.attest_device_request(&id_c).unwrap(),
            )
            .await,
        "attest C",
    );
    assert_eq!(pull_all(&api_c, vault_id).await.changes.len(), 5);

    // A wrong vault access key must not attest.
    let id_d = Arc::new(DeviceIdentity::generate().unwrap());
    let (api_d, _) = login(&base, &email, &id_d, "E2E device D").await;
    let mut bad = vault_c.attest_device_request(&id_d).unwrap();
    bad.vault_access_key = vec![0u8; 32].into();
    match api_d
        .post::<_, KeyEnvelope>(
            &paths::fill(
                paths::DEVICE_ATTEST,
                &[("device_id", &id_d.device_id().to_string())],
            ),
            &bad,
        )
        .await
    {
        Err((_, e)) => assert_eq!(e.code, ErrorCode::InvalidProof),
        Ok(_) => panic!("attest with a wrong VAK must fail"),
    }

    // ── SSH: device B connects to the target through the jump chain ─────
    if let Some(_tb) = &testbed {
        let (connector, _prompt) = cc_ssh_core::testing::connector(
            resolver.clone(),
            Arc::new(MemoryKnownHosts::new()),
            HostKeyDecision::Reject,
        );
        let session = connector
            .connect(&plan)
            .await
            .expect("ssh via 2 jump hosts");
        let out = session
            .exec("echo consolecrypt-e2e-ok; hostname")
            .await
            .expect("exec");
        let stdout = out.stdout_lossy();
        assert!(stdout.contains("consolecrypt-e2e-ok"), "{stdout}");
        println!(
            "E2E: SSH through bastion1 → bastion2 → target OK: {}",
            stdout.trim()
        );
        session.disconnect().await.ok();
    }

    let _ = (DeviceId::NIL, ObjectId::NIL); // keep imports honest across cfgs
    println!("E2E vertical slice against {base}: OK (ssh step: {with_ssh})");
}
