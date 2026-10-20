//! Explicit Kubernetes sharing projection: no personal object IDs, device
//! approvals, local file references, caches or session state are transferable.
use crate::{AppCore, AppError, AppResult};
use cc_models::kubernetes::{ExecInteractiveMode, KubeAuth, KubeCluster, KubeContext, KubeRoute};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::ObjectId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SharedKubeAuthentication {
    Token {
        value: SecretValue,
    },
    ClientCertificate {
        certificate_pem: String,
        key: SecretValue,
    },
    Oidc {
        issuer: String,
        client_id: String,
        certificate_authority_pem: Option<String>,
        scopes: Vec<String>,
        refresh_token: Option<SecretValue>,
        client_secret: Option<SecretValue>,
    },
    Exec {
        command: String,
        args: SecretValue,
        env: BTreeMap<String, SecretValue>,
        api_version: String,
        provide_cluster_info: bool,
        interactive_mode: ExecInteractiveMode,
    },
}

impl Zeroize for SharedKubeAuthentication {
    fn zeroize(&mut self) {
        match self {
            Self::Token { value } => value.zeroize(),
            Self::ClientCertificate {
                certificate_pem,
                key,
            } => {
                certificate_pem.zeroize();
                key.zeroize();
            }
            Self::Oidc {
                issuer,
                client_id,
                certificate_authority_pem,
                scopes,
                refresh_token,
                client_secret,
            } => {
                issuer.zeroize();
                client_id.zeroize();
                certificate_authority_pem.zeroize();
                scopes.zeroize();
                refresh_token.zeroize();
                client_secret.zeroize();
            }
            Self::Exec {
                command,
                args,
                env,
                api_version,
                provide_cluster_info,
                ..
            } => {
                command.zeroize();
                args.zeroize();
                for value in env.values_mut() {
                    value.zeroize();
                }
                env.clear();
                api_version.zeroize();
                provide_cluster_info.zeroize();
            }
        }
    }
}

impl Drop for SharedKubeAuthentication {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for SharedKubeAuthentication {}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedKubeProjection {
    pub name: String,
    pub cluster_name: String,
    pub server: String,
    pub certificate_authority_pem: Option<String>,
    pub tls_server_name: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub namespace: String,
    pub production: bool,
    /// The receiver must choose their own SSH/proxy route before connecting.
    pub requires_route: bool,
    /// Present only after explicit credential-sharing consent.
    pub authentication: Option<SharedKubeAuthentication>,
}

impl fmt::Debug for SharedKubeAuthentication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedKubeAuthentication(<redacted>)")
    }
}
impl fmt::Debug for SharedKubeProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedKubeProjection(<redacted>)")
    }
}

fn invalid() -> AppError {
    AppError::invalid("shared_kube_context", "invalid connection or credentials")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeSharingPreview {
    pub preview_json: String,
    pub snapshot_stamp: String,
}

pub(crate) async fn prepare(
    u: &crate::session::Unlocked,
    id: ObjectId,
    include_credentials: bool,
) -> AppResult<(SharedKubeProjection, String)> {
    let context = u.working().kube_context(id).ok_or_else(invalid)?;
    let cluster = u
        .working()
        .kube_cluster(context.cluster_id)
        .ok_or_else(invalid)?;
    let mut ids = vec![context.id, cluster.id];
    if include_credentials {
        ids.extend(context.auth.secret_ids());
    }
    let revisions: Vec<_> = ids
        .iter()
        .map(|id| (*id, u.working().revision_of(*id)))
        .collect();
    let serialized = serde_json::to_vec(&(
        u.vault_id,
        &context,
        &cluster,
        &revisions,
        include_credentials,
    ))
    .map_err(|_| invalid())?;
    let stamp: String = cc_crypto_core::request_body_sha256(&serialized)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let credentials = if include_credentials {
        let mut values = BTreeMap::new();
        for id in context.auth.secret_ids() {
            values.insert(id, u.writer.read_secret(id).await?);
        }
        Some(values)
    } else {
        None
    };
    if u.working().kube_context(context.id).as_ref() != Some(&context)
        || u.working().kube_cluster(cluster.id).as_ref() != Some(&cluster)
        || revisions
            .iter()
            .any(|(id, revision)| u.working().revision_of(*id) != *revision)
    {
        return Err(AppError::invalid(
            "context",
            "connection changed; review it again",
        ));
    }
    Ok((
        SharedKubeProjection::from_connection(&cluster, &context, credentials.as_ref())?,
        stamp,
    ))
}

impl AppCore {
    pub async fn sharing_preview_kube_context(
        &self,
        id: String,
        include_credentials: bool,
    ) -> AppResult<KubeSharingPreview> {
        let (_, u) = self.unlocked().await?;
        let id = id.parse().map_err(|_| invalid())?;
        let (projection, snapshot_stamp) = prepare(&u, id, include_credentials).await?;
        Ok(KubeSharingPreview {
            preview_json: projection.preview_json()?,
            snapshot_stamp,
        })
    }

    pub async fn sharing_copy_kube_context(
        &self,
        share_id: String,
        route: KubeRoute,
        include_credentials: bool,
    ) -> AppResult<KubeContext> {
        self.sharing_with_verified_projection(
            share_id,
            cc_protocol::sharing::SharedItemKind::KubeContext,
            move |u, _, projection| {
                Box::pin(async move {
                    let crate::sharing_projection::SharedProjection::KubeContext(projection) =
                        projection
                    else {
                        return Err(invalid());
                    };
                    let (cluster, context, secrets) =
                        projection.instantiate(route, include_credentials)?;
                    crate::kubernetes::persist_import(u, cluster, context, secrets).await
                })
            },
        )
        .await
    }
}

impl SharedKubeProjection {
    /// `credentials: None` excludes all authentication, including exec configuration.
    pub fn from_connection(
        cluster: &KubeCluster,
        context: &KubeContext,
        credentials: Option<&BTreeMap<ObjectId, Secret>>,
    ) -> AppResult<Self> {
        context.validate_cluster(cluster)?;
        let get = |id: &ObjectId| -> AppResult<SecretValue> {
            credentials
                .and_then(|values| values.get(id))
                .filter(|v| !v.value.is_empty())
                .map(|v| v.value.clone())
                .ok_or_else(invalid)
        };
        let authentication = if credentials.is_none() {
            None
        } else {
            match &context.auth {
                KubeAuth::None => None,
                KubeAuth::Token { secret_id } => Some(SharedKubeAuthentication::Token {
                    value: get(secret_id)?,
                }),
                KubeAuth::ClientCertificate {
                    certificate_pem,
                    key_secret_id,
                } => Some(SharedKubeAuthentication::ClientCertificate {
                    certificate_pem: certificate_pem.clone(),
                    key: get(key_secret_id)?,
                }),
                KubeAuth::Oidc {
                    issuer,
                    client_id,
                    refresh_secret_id,
                    client_secret_id,
                    certificate_authority_pem,
                    scopes,
                } => Some(SharedKubeAuthentication::Oidc {
                    issuer: issuer.clone(),
                    client_id: client_id.clone(),
                    scopes: scopes.clone(),
                    certificate_authority_pem: certificate_authority_pem.clone(),
                    refresh_token: refresh_secret_id.as_ref().map(get).transpose()?,
                    client_secret: client_secret_id.as_ref().map(get).transpose()?,
                }),
                KubeAuth::Exec {
                    command,
                    args_secret_id,
                    env,
                    api_version,
                    provide_cluster_info,
                    interactive_mode,
                } => Some(SharedKubeAuthentication::Exec {
                    command: command.clone(),
                    args: get(args_secret_id)?,
                    env: env
                        .iter()
                        .map(|(name, id)| Ok((name.clone(), get(id)?)))
                        .collect::<AppResult<_>>()?,
                    api_version: api_version.clone(),
                    provide_cluster_info: *provide_cluster_info,
                    interactive_mode: *interactive_mode,
                }),
            }
        };
        let projection = Self {
            name: context.name.clone(),
            cluster_name: cluster.name.clone(),
            server: cluster.server.clone(),
            certificate_authority_pem: cluster.certificate_authority_pem.clone(),
            tls_server_name: cluster.tls_server_name.clone(),
            insecure_skip_tls_verify: cluster.insecure_skip_tls_verify,
            namespace: context.namespace.clone(),
            production: context.production,
            requires_route: cluster.route != KubeRoute::Direct,
            authentication,
        };
        projection.validate()?;
        Ok(projection)
    }

    fn metadata(&self, route: KubeRoute) -> (KubeCluster, KubeContext) {
        let mut cluster = KubeCluster::new(&self.cluster_name, &self.server);
        cluster.certificate_authority_pem = self.certificate_authority_pem.clone();
        cluster.tls_server_name = self.tls_server_name.clone();
        cluster.insecure_skip_tls_verify = self.insecure_skip_tls_verify;
        cluster.route = route;
        let mut context = KubeContext::new(&self.name, cluster.id);
        context.namespace = self.namespace.clone();
        context.production = self.production;
        // Session permissions and exec approval always start at local defaults.
        (cluster, context)
    }

    pub fn validate(&self) -> AppResult<()> {
        let (cluster, mut context) = self.metadata(KubeRoute::Direct);
        let (auth, _) = self.instantiate_auth()?;
        context.auth = auth;
        context.validate_cluster(&cluster)?;
        Ok(())
    }

    fn instantiate_auth(&self) -> AppResult<(KubeAuth, Vec<Secret>)> {
        let mut secrets = Vec::new();
        let mut put = |kind, value: &SecretValue| -> AppResult<ObjectId> {
            if value.is_empty() || value.len() > 1024 * 1024 {
                return Err(invalid());
            }
            let secret = Secret::new(kind, value.clone());
            let id = secret.id;
            secrets.push(secret);
            Ok(id)
        };
        let auth = match &self.authentication {
            None => KubeAuth::None,
            Some(SharedKubeAuthentication::Token { value }) => {
                if value
                    .expose_secret()
                    .bytes()
                    .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
                {
                    return Err(invalid());
                }
                KubeAuth::Token {
                    secret_id: put(SecretKind::Token, value)?,
                }
            }
            Some(SharedKubeAuthentication::ClientCertificate {
                certificate_pem,
                key,
            }) => KubeAuth::ClientCertificate {
                certificate_pem: certificate_pem.clone(),
                key_secret_id: put(SecretKind::Other, key)?,
            },
            Some(SharedKubeAuthentication::Oidc {
                issuer,
                client_id,
                certificate_authority_pem,
                scopes,
                refresh_token,
                client_secret,
            }) => KubeAuth::Oidc {
                issuer: issuer.clone(),
                client_id: client_id.clone(),
                scopes: scopes.clone(),
                certificate_authority_pem: certificate_authority_pem.clone(),
                refresh_secret_id: refresh_token
                    .as_ref()
                    .map(|v| put(SecretKind::Token, v))
                    .transpose()?,
                client_secret_id: client_secret
                    .as_ref()
                    .map(|v| put(SecretKind::Other, v))
                    .transpose()?,
            },
            Some(SharedKubeAuthentication::Exec {
                command,
                args,
                env,
                api_version,
                provide_cluster_info,
                interactive_mode,
            }) => {
                let arguments: zeroize::Zeroizing<Vec<String>> = zeroize::Zeroizing::new(
                    serde_json::from_str(args.expose_secret()).map_err(|_| invalid())?,
                );
                if arguments.len() > 256
                    || arguments.iter().any(|v| v.len() > 8192 || v.contains('\0'))
                    || env
                        .values()
                        .any(|v| v.len() > 65536 || v.expose_secret().contains('\0'))
                {
                    return Err(invalid());
                }
                KubeAuth::Exec {
                    command: command.clone(),
                    args_secret_id: put(SecretKind::Other, args)?,
                    env: env
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), put(SecretKind::Other, value)?)))
                        .collect::<AppResult<_>>()?,
                    api_version: api_version.clone(),
                    provide_cluster_info: *provide_cluster_info,
                    interactive_mode: *interactive_mode,
                }
            }
        };
        auth.validate()?;
        Ok((auth, secrets))
    }

    pub fn instantiate(
        &self,
        route: KubeRoute,
        include_credentials: bool,
    ) -> AppResult<(KubeCluster, KubeContext, Vec<Secret>)> {
        self.validate()?;
        if self.requires_route && route == KubeRoute::Direct {
            return Err(AppError::invalid(
                "route",
                "select a local route for this connection",
            ));
        }
        let (cluster, mut context) = self.metadata(route);
        let secrets = if include_credentials {
            let (auth, secrets) = self.instantiate_auth()?;
            context.auth = auth;
            secrets
        } else {
            Vec::new()
        };
        context.validate_cluster(&cluster)?;
        Ok((cluster, context, secrets))
    }

    /// UI previews deliberately omit secret values, exec args and environment.
    pub fn preview_json(&self) -> AppResult<String> {
        serde_json::to_string(&serde_json::json!({
            "kind": "kube_context",
            "data": {
                "name": self.name, "cluster_name": self.cluster_name, "server": self.server,
                "namespace": self.namespace, "production": self.production,
                "insecure_skip_tls_verify": self.insecure_skip_tls_verify,
                "requires_route": self.requires_route,
                "includes_credentials": self.authentication.is_some(),
                "authentication_kind": match self.authentication {
                    None => "none",
                    Some(SharedKubeAuthentication::Token { .. }) => "token",
                    Some(SharedKubeAuthentication::ClientCertificate { .. }) => "client_certificate",
                    Some(SharedKubeAuthentication::Oidc { .. }) => "oidc",
                    Some(SharedKubeAuthentication::Exec { .. }) => "exec",
                },
            },
        })).map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_excludes_credentials_by_default_and_rehomes_explicit_secrets() {
        let cluster = KubeCluster::new("cluster", "https://kube.example.test");
        let mut context = KubeContext::new("operator", cluster.id);
        context.allow_exec_in_prod = true;
        let secret = Secret::new(
            SecretKind::Token,
            SecretValue::new(uuid::Uuid::new_v4().to_string()),
        );
        context.auth = KubeAuth::Token {
            secret_id: secret.id,
        };
        let default = SharedKubeProjection::from_connection(&cluster, &context, None).unwrap();
        assert!(default.authentication.is_none());
        let explicit = SharedKubeProjection::from_connection(
            &cluster,
            &context,
            Some(&BTreeMap::from([(secret.id, secret.clone())])),
        )
        .unwrap();
        assert!(!explicit
            .preview_json()
            .unwrap()
            .contains(secret.value.expose_secret()));
        assert!(!format!("{explicit:?}").contains(secret.value.expose_secret()));
        let encoded = serde_json::to_string(&explicit).unwrap();
        for id in [cluster.id, context.id, secret.id] {
            assert!(!encoded.contains(&id.to_string()));
        }
        let (received_cluster, received_context, received_secrets) =
            explicit.instantiate(KubeRoute::Direct, true).unwrap();
        assert_ne!(received_cluster.id, cluster.id);
        assert_ne!(received_context.id, context.id);
        assert_eq!(received_context.cluster_id, received_cluster.id);
        assert!(!received_context.allow_exec_in_prod);
        assert_eq!(received_secrets[0].value, secret.value);
        assert_ne!(received_secrets[0].id, secret.id);
        assert_eq!(
            received_context.auth.secret_ids(),
            vec![received_secrets[0].id]
        );
        let (_, without_auth, secrets) = explicit.instantiate(KubeRoute::Direct, false).unwrap();
        assert_eq!(without_auth.auth, KubeAuth::None);
        assert!(secrets.is_empty());
    }

    #[test]
    fn receiving_a_routed_context_never_silently_connects_directly() {
        let mut cluster = KubeCluster::new("cluster", "https://kube.internal");
        cluster.route = KubeRoute::Ssh {
            host_id: ObjectId::new(),
        };
        let context = KubeContext::new("operator", cluster.id);
        let projection = SharedKubeProjection::from_connection(&cluster, &context, None).unwrap();
        assert!(projection.instantiate(KubeRoute::Direct, false).is_err());
        assert!(projection
            .instantiate(
                KubeRoute::Ssh {
                    host_id: ObjectId::new()
                },
                false
            )
            .is_ok());
        assert!(!serde_json::to_string(&projection)
            .unwrap()
            .contains(&cluster.route.reference().unwrap().to_string()));
    }
}
