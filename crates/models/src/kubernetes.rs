//! Kubernetes connection metadata. Authentication material is referenced by
//! Secret IDs and never embedded in inventory objects. Exec consent, caches
//! and active sessions are device-local and deliberately absent here.

use crate::{ObjectId, Timestamp, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KubeRoute {
    #[default]
    Direct,
    Ssh {
        host_id: ObjectId,
    },
    Jump {
        jump_profile_id: ObjectId,
    },
    Proxy {
        proxy_id: ObjectId,
    },
}

impl KubeRoute {
    pub fn reference(&self) -> Option<ObjectId> {
        match self {
            Self::Direct => None,
            Self::Ssh { host_id } => Some(*host_id),
            Self::Jump { jump_profile_id } => Some(*jump_profile_id),
            Self::Proxy { proxy_id } => Some(*proxy_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KubeCluster {
    pub id: ObjectId,
    pub name: String,
    pub server: String,
    #[serde(default)]
    pub certificate_authority_pem: Option<String>,
    #[serde(default)]
    pub tls_server_name: Option<String>,
    #[serde(default)]
    pub insecure_skip_tls_verify: bool,
    #[serde(default)]
    pub route: KubeRoute,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Even exec arguments can contain passwords. The entire argument vector is a
/// JSON array in a separate Secret; environment values are also Secret objects.
/// The executable and public protocol settings convey no execution permission.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KubeAuth {
    #[default]
    None,
    Token {
        secret_id: ObjectId,
    },
    ClientCertificate {
        certificate_pem: String,
        key_secret_id: ObjectId,
    },
    Oidc {
        issuer: String,
        client_id: String,
        #[serde(default)]
        refresh_secret_id: Option<ObjectId>,
        #[serde(default)]
        client_secret_id: Option<ObjectId>,
        #[serde(default)]
        certificate_authority_pem: Option<String>,
        #[serde(default)]
        scopes: Vec<String>,
    },
    Exec {
        command: String,
        args_secret_id: ObjectId,
        #[serde(default)]
        env: BTreeMap<String, ObjectId>,
        api_version: String,
        #[serde(default)]
        provide_cluster_info: bool,
        #[serde(default)]
        interactive_mode: ExecInteractiveMode,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecInteractiveMode {
    #[default]
    Never,
    IfAvailable,
    Always,
}

impl KubeAuth {
    pub fn secret_ids(&self) -> Vec<ObjectId> {
        match self {
            Self::None => Vec::new(),
            Self::Token { secret_id } => vec![*secret_id],
            Self::ClientCertificate { key_secret_id, .. } => vec![*key_secret_id],
            Self::Oidc {
                refresh_secret_id,
                client_secret_id,
                ..
            } => refresh_secret_id
                .iter()
                .chain(client_secret_id.iter())
                .copied()
                .collect(),
            Self::Exec {
                args_secret_id,
                env,
                ..
            } => std::iter::once(*args_secret_id)
                .chain(env.values().copied())
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.secret_ids().contains(&ObjectId::NIL) {
            return Err(invalid("auth", "invalid secret reference"));
        }
        match self {
            Self::None | Self::Token { .. } => {}
            Self::ClientCertificate {
                certificate_pem, ..
            } => {
                validate_certificates(certificate_pem)?;
            }
            Self::Oidc {
                issuer,
                client_id,
                certificate_authority_pem,
                scopes,
                ..
            } => {
                validate_https_url(issuer)?;
                text(client_id, 1024, "client_id")?;
                if let Some(pem) = certificate_authority_pem {
                    validate_certificates(pem)?;
                }
                if scopes.len() > 64 {
                    return Err(invalid("scopes", "too many scopes"));
                }
                for scope in scopes {
                    text(scope, 256, "scope")?;
                }
            }
            Self::Exec {
                command,
                env,
                api_version,
                ..
            } => {
                text(command, 4096, "command")?;
                if !matches!(
                    api_version.as_str(),
                    "client.authentication.k8s.io/v1" | "client.authentication.k8s.io/v1beta1"
                ) || env.len() > 128
                {
                    return Err(invalid("exec", "invalid exec protocol or environment"));
                }
                for name in env.keys() {
                    if name.is_empty()
                        || name.len() > 256
                        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        || name.as_bytes()[0].is_ascii_digit()
                    {
                        return Err(invalid("exec", "invalid environment name"));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KubeContext {
    pub id: ObjectId,
    pub name: String,
    pub cluster_id: ObjectId,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub auth: KubeAuth,
    #[serde(default = "production_default")]
    pub production: bool,
    #[serde(default)]
    pub allow_exec_in_prod: bool,
    #[serde(default)]
    pub allow_port_forward_in_prod: bool,
    #[serde(default)]
    pub favourite_namespaces: Vec<String>,
    #[serde(default)]
    pub favourite_kinds: Vec<String>,
    #[serde(default)]
    pub event_history_enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_namespace() -> String {
    "default".into()
}
fn production_default() -> bool {
    true
}
fn invalid(field: &'static str, reason: &'static str) -> ValidationError {
    ValidationError::new(field, reason)
}
fn text(value: &str, limit: usize, field: &'static str) -> Result<(), ValidationError> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(invalid(
            field,
            "empty, too long or contains control characters",
        ));
    }
    Ok(())
}

pub fn validate_https_url(value: &str) -> Result<(), ValidationError> {
    let url = Url::parse(value).map_err(|_| invalid("server", "invalid HTTPS URL"))?;
    if value.len() > 8192
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            "server",
            "HTTPS URL must not include credentials, query or fragment",
        ));
    }
    Ok(())
}

pub fn validate_namespace(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 63
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(invalid("namespace", "invalid namespace"));
    }
    Ok(())
}

/// Reject key blocks, comments and trailing text rather than retaining arbitrary
/// kubeconfig data in the inventory KEK. DER validation happens at connection.
pub fn validate_certificates(value: &str) -> Result<(), ValidationError> {
    if value.len() > 1024 * 1024 {
        return Err(invalid("certificate", "certificate bundle too large"));
    }
    let blocks = pem::parse_many(value).map_err(|_| invalid("certificate", "invalid PEM"))?;
    if blocks.is_empty()
        || blocks.len() > 64
        || blocks
            .iter()
            .any(|p| p.tag() != "CERTIFICATE" || p.contents().is_empty())
    {
        return Err(invalid("certificate", "only certificates are allowed"));
    }
    let canonical = blocks.iter().map(pem::encode).collect::<String>();
    let normalized = |s: &str| s.split_whitespace().collect::<String>();
    if normalized(&canonical) != normalized(value) {
        return Err(invalid(
            "certificate",
            "unexpected content outside certificates",
        ));
    }
    Ok(())
}

impl KubeCluster {
    pub fn new(name: impl Into<String>, server: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            server: server.into(),
            certificate_authority_pem: None,
            tls_server_name: None,
            insecure_skip_tls_verify: false,
            route: KubeRoute::Direct,
            tags: Vec::new(),
            color: None,
            created_at: now,
            updated_at: now,
        }
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id == ObjectId::NIL || self.route.reference() == Some(ObjectId::NIL) {
            return Err(invalid("id", "invalid reference"));
        }
        text(&self.name, 1024, "name")?;
        validate_https_url(&self.server)?;
        if let Some(pem) = &self.certificate_authority_pem {
            validate_certificates(pem)?;
        }
        if let Some(name) = &self.tls_server_name {
            text(name, 253, "tls_server_name")?;
            if name
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && !b".-:".contains(&b))
            {
                return Err(invalid("tls_server_name", "invalid TLS server name"));
            }
        }
        if self.tags.len() > 128 {
            return Err(invalid("tags", "too many tags"));
        }
        for tag in &self.tags {
            text(tag, 256, "tag")?;
        }
        if let Some(color) = &self.color {
            text(color, 64, "color")?;
        }
        Ok(())
    }
}

impl KubeContext {
    pub fn new(name: impl Into<String>, cluster_id: ObjectId) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            cluster_id,
            namespace: default_namespace(),
            auth: KubeAuth::None,
            production: true,
            allow_exec_in_prod: false,
            allow_port_forward_in_prod: false,
            favourite_namespaces: Vec::new(),
            favourite_kinds: Vec::new(),
            event_history_enabled: false,
            created_at: now,
            updated_at: now,
        }
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id == ObjectId::NIL || self.cluster_id == ObjectId::NIL {
            return Err(invalid("id", "invalid reference"));
        }
        text(&self.name, 1024, "name")?;
        validate_namespace(&self.namespace)?;
        self.auth.validate()?;
        if self.favourite_namespaces.len() > 256 || self.favourite_kinds.len() > 256 {
            return Err(invalid("favourites", "too many favourites"));
        }
        for namespace in &self.favourite_namespaces {
            validate_namespace(namespace)?;
        }
        for kind in &self.favourite_kinds {
            text(kind, 256, "kind")?;
        }
        Ok(())
    }
    pub fn validate_cluster(&self, cluster: &KubeCluster) -> Result<(), ValidationError> {
        self.validate()?;
        cluster.validate()?;
        if cluster.id != self.cluster_id {
            return Err(invalid("cluster_id", "different cluster"));
        }
        if self.production && cluster.insecure_skip_tls_verify {
            return Err(invalid(
                "tls",
                "production contexts require TLS verification",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KekClass, ObjectPayload, VaultObject};

    #[test]
    fn connection_metadata_is_inventory_and_auth_is_only_a_reference() {
        let cluster = KubeCluster::new("test", "https://kube.example.test:6443");
        let mut context = KubeContext::new("test", cluster.id);
        context.auth = KubeAuth::Token {
            secret_id: ObjectId::new(),
        };
        context.validate_cluster(&cluster).unwrap();
        for object in [
            VaultObject::KubeCluster(cluster),
            VaultObject::KubeContext(context),
        ] {
            let id = object.id();
            assert_eq!(object.kind().kek_class(), KekClass::Inventory);
            let encoded = serde_json::to_string(&ObjectPayload::new(object)).unwrap();
            let decoded: ObjectPayload = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.object.id(), id);
            assert!(!encoded.contains("approved_hash"));
        }
    }

    #[test]
    fn remote_configuration_cannot_transfer_exec_approval_or_inline_credentials() {
        let id = ObjectId::new();
        for extra in [
            serde_json::json!({"token":"private"}),
            serde_json::json!({"approved_hash":"trusted-elsewhere"}),
        ] {
            let mut auth = serde_json::json!({"kind":"token","secret_id":id});
            auth.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(serde_json::from_value::<KubeAuth>(auth).is_err());
        }
        let exec = KubeAuth::Exec {
            command: "/usr/bin/auth-plugin".into(),
            args_secret_id: id,
            env: BTreeMap::from([("TOKEN".into(), ObjectId::new())]),
            api_version: "client.authentication.k8s.io/v1".into(),
            provide_cluster_info: false,
            interactive_mode: ExecInteractiveMode::Never,
        };
        exec.validate().unwrap();
        assert_eq!(exec.secret_ids().len(), 2);
    }

    #[test]
    fn production_defaults_to_verified_tls_and_rejects_credential_urls() {
        let mut cluster = KubeCluster::new("test", "https://kube.example.test");
        let context = KubeContext::new("test", cluster.id);
        cluster.insecure_skip_tls_verify = true;
        assert!(context.validate_cluster(&cluster).is_err());
        for server in [
            "http://kube.test",
            "https://user:password@kube.test",
            "https://kube.test?token=x",
        ] {
            cluster.server = server.into();
            assert!(cluster.validate().is_err());
        }
        assert!(validate_certificates(
            "-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----"
        )
        .is_err());
        assert!(validate_certificates(
            "secret\n-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----"
        )
        .is_err());
    }
}
