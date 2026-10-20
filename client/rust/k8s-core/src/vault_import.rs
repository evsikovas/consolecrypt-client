//! Conversion to the E2EE object graph. Unconfirmed external files and routing
//! are rejected; this step has no network, filesystem or process side effects.
use crate::import::{CertificateSource, ImportContext, ImportedAuth, SecretSource};
use cc_models::kubernetes::{KubeAuth, KubeCluster, KubeContext, KubeRoute};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::ObjectId;
use secrecy::{ExposeSecret, SecretString};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrepareError {
    #[error("Referenced files require confirmation before import")]
    PendingFile,
    #[error("The imported proxy must be mapped to a saved connection route")]
    PendingRoute,
    #[error("The connection metadata is invalid for the selected policy")]
    Invalid,
}

/// `Debug` of Secret values is redacted. Only metadata should cross the UI bridge.
#[derive(Debug)]
pub struct PreparedConnection {
    pub cluster: KubeCluster,
    pub context: KubeContext,
    pub secrets: Vec<Secret>,
}

fn certificate(source: CertificateSource) -> Result<String, PrepareError> {
    match source {
        CertificateSource::Pem(pem) => Ok(pem),
        CertificateSource::File(_) => Err(PrepareError::PendingFile),
    }
}

fn secret(source: SecretSource) -> Result<SecretString, PrepareError> {
    match source {
        SecretSource::Inline(value) => Ok(value),
        SecretSource::File(_) => Err(PrepareError::PendingFile),
    }
}

pub fn prepare(
    imported: ImportContext,
    production: bool,
    route: KubeRoute,
) -> Result<PreparedConnection, PrepareError> {
    if imported.proxy.is_some() && route == KubeRoute::Direct {
        return Err(PrepareError::PendingRoute);
    }
    let mut cluster = KubeCluster::new(imported.cluster_name, imported.server.to_string());
    cluster.certificate_authority_pem = imported
        .certificate_authority
        .map(certificate)
        .transpose()?;
    cluster.tls_server_name = imported.tls_server_name;
    cluster.insecure_skip_tls_verify = imported.insecure_skip_tls_verify;
    cluster.route = route;
    let mut secrets = Vec::new();
    let mut put = |kind, value: SecretString| -> ObjectId {
        let object = Secret::new(kind, SecretValue::new(value.expose_secret()));
        let id = object.id;
        secrets.push(object);
        id
    };
    let auth = match imported.auth {
        ImportedAuth::Token(source) => KubeAuth::Token {
            secret_id: put(SecretKind::Token, secret(source)?),
        },
        ImportedAuth::ClientCertificate {
            certificate: cert,
            key,
        } => KubeAuth::ClientCertificate {
            certificate_pem: certificate(cert)?,
            key_secret_id: put(SecretKind::Other, secret(key)?),
        },
        ImportedAuth::Oidc {
            issuer,
            client_id,
            client_secret,
            refresh_token,
            scopes,
            certificate_authority,
        } => KubeAuth::Oidc {
            issuer: issuer.to_string(),
            client_id,
            scopes,
            refresh_secret_id: refresh_token.map(|v| put(SecretKind::Token, v)),
            client_secret_id: client_secret.map(|v| put(SecretKind::Other, v)),
            certificate_authority_pem: certificate_authority.map(certificate).transpose()?,
        },
        ImportedAuth::Exec(exec) => {
            let args = serde_json::to_string(&*exec.args).map_err(|_| PrepareError::Invalid)?;
            let args_secret_id = put(SecretKind::Other, SecretString::from(args));
            let env: BTreeMap<_, _> = exec
                .env
                .into_iter()
                .map(|(name, value)| (name, put(SecretKind::Other, value)))
                .collect();
            KubeAuth::Exec {
                command: exec.command,
                args_secret_id,
                env,
                api_version: exec.api_version,
                provide_cluster_info: exec.provide_cluster_info,
                interactive_mode: exec.interactive_mode,
            }
        }
    };
    let mut context = KubeContext::new(imported.name, cluster.id);
    context.namespace = imported.namespace;
    context.auth = auth;
    context.production = production;
    context
        .validate_cluster(&cluster)
        .map_err(|_| PrepareError::Invalid)?;
    Ok(PreparedConnection {
        cluster,
        context,
        secrets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(auth: &str) -> ImportContext {
        let text = format!("apiVersion: v1\nkind: Config\nclusters:\n- name: cluster\n  cluster:\n    server: https://kube.example.test\ncontexts:\n- name: operator\n  context:\n    cluster: cluster\n    user: operator\nusers:\n- name: operator\n  user:\n{auth}");
        crate::import::parse(&text, None)
            .unwrap()
            .entries
            .remove(0)
            .context
            .unwrap()
    }

    #[test]
    fn embedded_credentials_leave_no_plaintext_in_inventory_objects() {
        let token = uuid::Uuid::new_v4().to_string();
        let prepared = prepare(
            source(&format!("    token: {token}")),
            true,
            KubeRoute::Direct,
        )
        .unwrap();
        assert_eq!(prepared.secrets.len(), 1);
        let metadata = serde_json::to_string(&(&prepared.cluster, &prepared.context)).unwrap();
        assert!(!metadata.contains(&token));
        assert!(!format!("{prepared:?}").contains(&token));
        assert_eq!(prepared.secrets[0].value.expose_secret(), token);
        assert_eq!(
            prepared.context.auth.secret_ids(),
            vec![prepared.secrets[0].id]
        );
    }

    #[test]
    fn exec_arguments_and_environment_are_separate_secrets_without_approval() {
        let prepared = prepare(source("    exec:\n      apiVersion: client.authentication.k8s.io/v1beta1\n      command: provider\n      args: ['--token', 'private-argument']\n      env:\n      - name: TOKEN\n        value: private-env\n"), true, KubeRoute::Direct).unwrap();
        let metadata = serde_json::to_string(&prepared.context).unwrap();
        assert!(!metadata.contains("private-argument"));
        assert!(!metadata.contains("private-env"));
        assert!(!metadata.contains("approved"));
        assert_eq!(prepared.secrets.len(), 2);
    }

    #[test]
    fn external_files_are_not_read_and_insecure_production_is_rejected() {
        assert_eq!(
            prepare(
                source("    tokenFile: /unapproved/token"),
                true,
                KubeRoute::Direct
            )
            .unwrap_err(),
            PrepareError::PendingFile
        );
        let mut imported = source("    token: synthetic");
        imported.insecure_skip_tls_verify = true;
        assert_eq!(
            prepare(imported, true, KubeRoute::Direct).unwrap_err(),
            PrepareError::Invalid
        );
    }
}
