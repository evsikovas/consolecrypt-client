//! Saved Kubernetes connections use the same encrypted mutation/outbox path as
//! SSH. This module never writes ~/.kube/config or starts authentication hooks.
use crate::inventory::{ensure_unreferenced, validate_refs};
use crate::{AppCore, AppError, AppResult};
use cc_models::kubernetes::{KubeAuth, KubeCluster, KubeContext};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, ObjectKind, VaultObject};

#[derive(Debug, Clone, serde::Serialize)]
pub struct KubeInventory {
    pub clusters: Vec<KubeCluster>,
    pub contexts: Vec<KubeContext>,
}

impl AppCore {
    pub async fn kube_inventory(&self) -> AppResult<KubeInventory> {
        let (_, u) = self.unlocked().await?;
        let mut result = KubeInventory {
            clusters: Vec::new(),
            contexts: Vec::new(),
        };
        for object in u.working().objects() {
            match object {
                VaultObject::KubeCluster(cluster) => result.clusters.push(cluster),
                VaultObject::KubeContext(context) => result.contexts.push(context),
                _ => {}
            }
        }
        result
            .clusters
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        result
            .contexts
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(result)
    }
    pub async fn list_kube_clusters(&self) -> AppResult<Vec<KubeCluster>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working().kube_clusters())
    }

    pub async fn list_kube_contexts(&self) -> AppResult<Vec<KubeContext>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working().kube_contexts())
    }

    pub async fn save_kube_cluster(&self, mut cluster: KubeCluster) -> AppResult<KubeCluster> {
        let (_, u) = self.unlocked().await?;
        if let Some(kind) = u.working().kind_of(cluster.id) {
            if kind != ObjectKind::KubeCluster {
                return Err(AppError::invalid(
                    "id",
                    "object is not a Kubernetes cluster",
                ));
            }
        }
        if let Some(existing) = u.working().kube_cluster(cluster.id) {
            cluster.created_at = existing.created_at;
        }
        cluster.updated_at = chrono::Utc::now();
        let object = VaultObject::KubeCluster(cluster.clone());
        validate_refs(u.working(), &object)?;
        u.writer.put(object).await?;
        Ok(cluster)
    }

    pub async fn save_kube_context(&self, mut context: KubeContext) -> AppResult<KubeContext> {
        let (_, u) = self.unlocked().await?;
        if let Some(kind) = u.working().kind_of(context.id) {
            if kind != ObjectKind::KubeContext {
                return Err(AppError::invalid(
                    "id",
                    "object is not a Kubernetes context",
                ));
            }
        }
        if let Some(existing) = u.working().kube_context(context.id) {
            context.created_at = existing.created_at;
        }
        context.updated_at = chrono::Utc::now();
        let object = VaultObject::KubeContext(context.clone());
        validate_refs(u.working(), &object)?;
        u.writer.put(object).await?;
        Ok(context)
    }

    /// Native import boundary. All IDs are new; referenced credentials must be
    /// included exactly, preventing accidental access to unrelated vault secrets.
    pub async fn import_kube_connection(
        &self,
        cluster: KubeCluster,
        context: KubeContext,
        secrets: Vec<Secret>,
    ) -> AppResult<KubeContext> {
        let (_, u) = self.unlocked().await?;
        persist_import(&u, cluster, context, secrets).await
    }

    /// Replacement token and the referencing context are committed together.
    /// Old shared/referenced credentials are retained, never deleted implicitly.
    pub async fn set_kube_token(&self, id: ObjectId, token: SecretValue) -> AppResult<KubeContext> {
        if token.is_empty() || token.len() > 1024 * 1024 {
            return Err(AppError::invalid("token", "token is empty or too large"));
        }
        let (_, u) = self.unlocked().await?;
        let mut context = u
            .working()
            .kube_context(id)
            .ok_or_else(|| AppError::not_found("kube_context", id))?;
        let cluster = u
            .working()
            .kube_cluster(context.cluster_id)
            .ok_or_else(|| AppError::invalid("cluster_id", "cluster is unavailable"))?;
        let secret = Secret::new(SecretKind::Token, token);
        context.auth = KubeAuth::Token {
            secret_id: secret.id,
        };
        context.updated_at = chrono::Utc::now();
        context.validate_cluster(&cluster)?;
        u.writer
            .put_batch(vec![
                VaultObject::Secret(secret),
                VaultObject::KubeContext(context.clone()),
            ])
            .await?;
        Ok(context)
    }

    pub async fn delete_kube_context(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::KubeContext).await?;
        u.writer.delete(id).await
    }

    pub async fn delete_kube_cluster(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::KubeCluster).await?;
        ensure_unreferenced(u.working(), id, "Kubernetes cluster")?;
        u.writer.delete(id).await
    }
}

pub(crate) async fn persist_import(
    u: &crate::session::Unlocked,
    cluster: KubeCluster,
    context: KubeContext,
    secrets: Vec<Secret>,
) -> AppResult<KubeContext> {
    let contexts = persist_imports(u, vec![(cluster, context, secrets)]).await?;
    Ok(contexts
        .into_iter()
        .next()
        .expect("one validated connection"))
}

pub(crate) async fn persist_imports(
    u: &crate::session::Unlocked,
    connections: Vec<(KubeCluster, KubeContext, Vec<Secret>)>,
) -> AppResult<Vec<KubeContext>> {
    if connections.is_empty() || connections.len() > 64 {
        return Err(AppError::invalid(
            "contexts",
            "select between 1 and 64 contexts",
        ));
    }
    let mut objects = Vec::new();
    let mut contexts = Vec::new();
    for (cluster, context, secrets) in connections {
        context.validate_cluster(&cluster)?;
        validate_refs(u.working(), &VaultObject::KubeCluster(cluster.clone()))?;
        let expected: std::collections::BTreeSet<_> =
            context.auth.secret_ids().into_iter().collect();
        let actual: std::collections::BTreeSet<_> = secrets.iter().map(|s| s.id).collect();
        if actual != expected
            || actual.len() != secrets.len()
            || secrets
                .iter()
                .any(|s| s.value.is_empty() || s.id == ObjectId::NIL)
        {
            return Err(AppError::invalid(
                "auth",
                "credential bundle does not match the context",
            ));
        }
        objects.extend(secrets.into_iter().map(VaultObject::Secret));
        objects.push(VaultObject::KubeCluster(cluster));
        objects.push(VaultObject::KubeContext(context.clone()));
        contexts.push(context);
    }
    let ids: std::collections::BTreeSet<_> = objects.iter().map(VaultObject::id).collect();
    if objects.len() > 256 || ids.len() != objects.len() {
        return Err(AppError::invalid(
            "contexts",
            "too many or duplicate imported objects",
        ));
    }
    if objects
        .iter()
        .any(|o| u.working().kind_of(o.id()).is_some())
    {
        return Err(AppError::invalid(
            "id",
            "import requires new object identities",
        ));
    }
    u.writer.put_batch(objects).await?;
    Ok(contexts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountMode, AppConfig};
    use cc_models::KekClass;
    use cc_sync_core::mock::MockServer;

    fn app(path: &std::path::Path) -> AppCore {
        AppCore::new(AppConfig::for_tests(path.to_string_lossy().into_owned())).unwrap()
    }

    fn connection() -> (KubeCluster, KubeContext, Secret) {
        let cluster = KubeCluster::new("kube test", "https://kube.example.test:6443");
        let mut context = KubeContext::new("operator", cluster.id);
        let secret = Secret::new(
            SecretKind::Token,
            SecretValue::new(uuid::Uuid::new_v4().to_string()),
        );
        context.auth = KubeAuth::Token {
            secret_id: secret.id,
        };
        (cluster, context, secret)
    }

    #[tokio::test]
    async fn imports_are_atomic_and_dependencies_and_production_tls_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path());
        app.create_local_profile(
            "test".into(),
            format!("{} local test", uuid::Uuid::new_v4()),
        )
        .await
        .unwrap();
        let (cluster, context, secret) = connection();
        assert!(app
            .import_kube_connection(cluster.clone(), context.clone(), vec![])
            .await
            .is_err());
        assert!(app.list_kube_clusters().await.unwrap().is_empty());
        let (_, u) = app.unlocked().await.unwrap();
        assert!(u.writer.read_secret(secret.id).await.is_err());
        let large = Secret::new(
            SecretKind::Other,
            SecretValue::new("x".repeat(5 * 1024 * 1024)),
        );
        assert!(u
            .writer
            .put_batch(vec![
                VaultObject::Secret(secret.clone()),
                VaultObject::Secret(large)
            ])
            .await
            .is_err());
        assert!(
            u.writer.read_secret(secret.id).await.is_err(),
            "batch must roll back its first object"
        );
        app.import_kube_connection(cluster.clone(), context.clone(), vec![secret.clone()])
            .await
            .unwrap();
        assert_eq!(
            u.writer
                .store
                .stored(secret.id)
                .await
                .unwrap()
                .unwrap()
                .kek_class_hint,
            Some(KekClass::Secrets)
        );
        assert_eq!(
            u.writer
                .store
                .stored(context.id)
                .await
                .unwrap()
                .unwrap()
                .kek_class_hint,
            Some(KekClass::Inventory)
        );
        assert!(app
            .delete_kube_cluster(cluster.id.to_string())
            .await
            .is_err());
        let mut insecure = cluster.clone();
        insecure.insecure_skip_tls_verify = true;
        assert!(app.save_kube_cluster(insecure).await.is_err());
        app.delete_kube_context(context.id.to_string())
            .await
            .unwrap();
        app.delete_kube_cluster(cluster.id.to_string())
            .await
            .unwrap();
        app.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_devices_sync_encrypted_cluster_context_and_separate_credentials() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let a = app(&dir.path().join("a"));
        let b = app(&dir.path().join("b"));
        let email = format!("kube-{}@example.test", uuid::Uuid::new_v4());
        let password = uuid::Uuid::new_v4().to_string() + " account";
        let passphrase = uuid::Uuid::new_v4().to_string() + " vault";
        a.create_synced_profile(
            "a".into(),
            server.url().to_string(),
            email.clone(),
            password.clone(),
            AccountMode::Register,
        )
        .await
        .unwrap();
        let kit = a.create_vault(passphrase.clone()).await.unwrap();
        let (cluster, context, secret) = connection();
        a.import_kube_connection(cluster.clone(), context.clone(), vec![secret.clone()])
            .await
            .unwrap();
        a.sync_now().await.unwrap();
        let vault_id = kit.vault_id.parse().unwrap();
        for id in [cluster.id, context.id, secret.id] {
            let encrypted = server.object(vault_id, id).unwrap();
            let body = encrypted.body.as_ref().unwrap();
            let ciphertext = body.ciphertext.as_slice();
            for plaintext in [
                cluster.server.as_bytes(),
                secret.value.expose_secret().as_bytes(),
            ] {
                assert!(!ciphertext.windows(plaintext.len()).any(|v| v == plaintext));
            }
        }
        b.create_synced_profile(
            "b".into(),
            server.url().to_string(),
            email,
            password,
            AccountMode::Login,
        )
        .await
        .unwrap();
        b.join_vault_with_passphrase(kit.vault_id.clone(), passphrase)
            .await
            .unwrap();
        b.sync_now().await.unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let (_, u) = b.unlocked().await.unwrap();
            if u.working().kube_context(context.id).is_some()
                && u.working().secret_kind(secret.id).is_some()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "context did not synchronize"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(b.list_kube_clusters().await.unwrap(), vec![cluster]);
        assert_eq!(b.list_kube_contexts().await.unwrap(), vec![context]);
        let (_, u) = b.unlocked().await.unwrap();
        assert_eq!(
            u.writer.read_secret(secret.id).await.unwrap().value,
            secret.value
        );
        assert!(!format!("{:?}", u.working().objects()).contains(secret.value.expose_secret()));
        a.shutdown().await.unwrap();
        b.shutdown().await.unwrap();
    }
}
