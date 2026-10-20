//! Staged kubeconfig import. Parsing does no I/O, environment expansion, login
//! or command execution. File references and exec plugins remain explicit
//! pending actions for the confirmation screen.

use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::kubernetes::ExecInteractiveMode;
use kube::config::{AuthInfo, Cluster, Kubeconfig};
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};
use url::Url;
use zeroize::Zeroizing;

pub const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONTEXTS: usize = 1024;
const MAX_EXPANDED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("The kubeconfig is empty or exceeds the size limit")]
    Size,
    #[error("The kubeconfig YAML is invalid")]
    Yaml,
    #[error("Only Kubernetes v1 Config documents are supported")]
    Format,
    #[error("The kubeconfig has duplicate names")]
    DuplicateName,
    #[error("The kubeconfig contains too many entries")]
    TooManyEntries,
    #[error("A context references a missing cluster or user")]
    MissingReference,
    #[error("An entry name is empty, too long or contains control characters")]
    Name,
    #[error("The server URL must be HTTPS without credentials, query or fragment")]
    Server,
    #[error("The namespace is invalid")]
    Namespace,
    #[error("The authentication method is missing or unsupported")]
    Authentication,
    #[error("Multiple authentication methods or data sources are ambiguous")]
    AmbiguousAuthentication,
    #[error("Embedded certificate or key data is invalid")]
    EmbeddedData,
    #[error("A referenced path is invalid")]
    FilePath,
    #[error("Relative file references need a source file")]
    RelativePathNeedsFile,
    #[error("The exec-plugin configuration is invalid or unsupported")]
    Exec,
    #[error("The OIDC configuration is invalid")]
    Oidc,
    #[error("User impersonation is not supported during import")]
    Impersonation,
}

/// No `Serialize`: raw imported authentication stays in native memory until
/// it is converted into separate encrypted Secret objects.
pub enum SecretSource {
    Inline(SecretString),
    File(PathBuf),
}

impl fmt::Debug for SecretSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inline(_) => "SecretSource::Inline([redacted])",
            Self::File(_) => "SecretSource::File([pending confirmation])",
        })
    }
}

pub enum CertificateSource {
    Pem(String),
    File(PathBuf),
}

impl fmt::Debug for CertificateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pem(_) => "CertificateSource::Pem([certificate])",
            Self::File(_) => "CertificateSource::File([pending confirmation])",
        })
    }
}

pub struct ExecSpecification {
    pub command: String,
    pub args: Zeroizing<Vec<String>>,
    pub env: BTreeMap<String, SecretString>,
    pub api_version: String,
    pub provide_cluster_info: bool,
    pub interactive_mode: ExecInteractiveMode,
}

impl fmt::Debug for ExecSpecification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExecSpecification([approval required])")
    }
}

pub enum ImportedAuth {
    Token(SecretSource),
    ClientCertificate {
        certificate: CertificateSource,
        key: SecretSource,
    },
    Oidc {
        issuer: Url,
        client_id: String,
        client_secret: Option<SecretString>,
        refresh_token: Option<SecretString>,
        scopes: Vec<String>,
        certificate_authority: Option<CertificateSource>,
    },
    Exec(ExecSpecification),
}

impl fmt::Debug for ImportedAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Token(_) => "ImportedAuth::Token([redacted])",
            Self::ClientCertificate { .. } => "ImportedAuth::ClientCertificate([redacted])",
            Self::Oidc { .. } => "ImportedAuth::Oidc([redacted])",
            Self::Exec(_) => "ImportedAuth::Exec([approval required])",
        })
    }
}

pub struct ImportContext {
    pub name: String,
    pub cluster_name: String,
    pub user_name: String,
    pub server: Url,
    pub namespace: String,
    pub certificate_authority: Option<CertificateSource>,
    pub tls_server_name: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub proxy: Option<Url>,
    pub auth: ImportedAuth,
}

impl fmt::Debug for ImportContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportContext")
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ImportEntry {
    pub name: String,
    pub context: Result<ImportContext, ImportError>,
}

#[derive(Debug)]
pub struct ImportBatch {
    pub entries: Vec<ImportEntry>,
    pub current_context: Option<String>,
}

/// `source_file` only resolves relative references; it is never opened.
/// The caller must keep pasted YAML in a zeroizing buffer as well.
pub fn parse(text: &str, source_file: Option<&Path>) -> Result<ImportBatch, ImportError> {
    if text.trim().is_empty() || text.len() > MAX_CONFIG_BYTES {
        return Err(ImportError::Size);
    }
    let options = serde_saphyr::options! {
        with_snippet: false,
    };
    let documents: Vec<Kubeconfig> =
        serde_saphyr::from_multiple_with_options(text, options).map_err(|_| ImportError::Yaml)?;
    let mut clusters = BTreeMap::new();
    let mut users = BTreeMap::new();
    let mut contexts = BTreeMap::new();
    let mut current_context = None;
    for document in documents {
        if document.kind.as_deref().is_some_and(|s| s != "Config")
            || document.api_version.as_deref().is_some_and(|s| s != "v1")
        {
            return Err(ImportError::Format);
        }
        if document.clusters.len() > MAX_CONTEXTS
            || document.auth_infos.len() > MAX_CONTEXTS
            || document.contexts.len() > MAX_CONTEXTS
        {
            return Err(ImportError::TooManyEntries);
        }
        for cluster in document.clusters {
            valid_name(&cluster.name)?;
            if clusters.insert(cluster.name, cluster.cluster).is_some() {
                return Err(ImportError::DuplicateName);
            }
        }
        for user in document.auth_infos {
            valid_name(&user.name)?;
            if users.insert(user.name, user.auth_info).is_some() {
                return Err(ImportError::DuplicateName);
            }
        }
        for context in document.contexts {
            valid_name(&context.name)?;
            if contexts.insert(context.name, context.context).is_some() {
                return Err(ImportError::DuplicateName);
            }
        }
        if current_context.is_none() {
            current_context = document.current_context;
        }
    }
    if clusters.len() > MAX_CONTEXTS || users.len() > MAX_CONTEXTS || contexts.len() > MAX_CONTEXTS
    {
        return Err(ImportError::TooManyEntries);
    }
    if contexts.is_empty() {
        return Err(ImportError::MissingReference);
    }
    let mut entries = Vec::with_capacity(contexts.len());
    let mut expanded_bytes = 0;
    for (name, context) in contexts {
        let imported = (|| {
            let context = context.ok_or(ImportError::MissingReference)?;
            let cluster = clusters
                .get(&context.cluster)
                .and_then(Option::as_ref)
                .ok_or(ImportError::MissingReference)?;
            let user_name = context.user.ok_or(ImportError::Authentication)?;
            let user = users
                .get(&user_name)
                .and_then(Option::as_ref)
                .ok_or(ImportError::MissingReference)?;
            let namespace = context.namespace.unwrap_or_else(|| "default".into());
            if namespace.is_empty()
                || namespace.len() > 63
                || namespace.starts_with('-')
                || namespace.ends_with('-')
                || !namespace
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            {
                return Err(ImportError::Namespace);
            }
            let server = server_url(cluster.server.as_deref().ok_or(ImportError::Server)?)?;
            let proxy = cluster
                .proxy_url
                .as_deref()
                .map(|value| {
                    let proxy = Url::parse(value).map_err(|_| ImportError::Server)?;
                    if !matches!(proxy.scheme(), "http" | "https" | "socks5")
                        || proxy.host_str().is_none()
                        || !proxy.username().is_empty()
                        || proxy.password().is_some()
                        || proxy.query().is_some()
                        || proxy.fragment().is_some()
                        || !matches!(proxy.path(), "" | "/")
                    {
                        return Err(ImportError::Server);
                    }
                    Ok(proxy)
                })
                .transpose()?;
            if let Some(name) = &cluster.tls_server_name {
                rustls::pki_types::ServerName::try_from(name.clone())
                    .map_err(|_| ImportError::Server)?;
            }
            Ok(ImportContext {
                name: name.clone(),
                cluster_name: context.cluster,
                user_name,
                server,
                namespace,
                certificate_authority: ca_source(cluster, source_file)?,
                tls_server_name: cluster.tls_server_name.clone(),
                insecure_skip_tls_verify: cluster.insecure_skip_tls_verify.unwrap_or(false),
                proxy,
                auth: import_auth(user, source_file)?,
            })
        })();
        if let Ok(context) = &imported {
            expanded_bytes += imported_size(context);
            // Several contexts may reference the same large credential. Bound
            // the expanded native preview as well as the original YAML input.
            if expanded_bytes > MAX_EXPANDED_BYTES {
                return Err(ImportError::Size);
            }
        }
        entries.push(ImportEntry {
            name,
            context: imported,
        });
    }
    if current_context
        .as_ref()
        .is_some_and(|name| !entries.iter().any(|e| e.name == *name))
    {
        current_context = None;
    }
    Ok(ImportBatch {
        entries,
        current_context,
    })
}

fn imported_size(context: &ImportContext) -> usize {
    fn cert(source: &CertificateSource) -> usize {
        match source {
            CertificateSource::Pem(value) => value.len(),
            CertificateSource::File(path) => path.as_os_str().len(),
        }
    }
    fn secret(source: &SecretSource) -> usize {
        match source {
            SecretSource::Inline(value) => value.expose_secret().len(),
            SecretSource::File(path) => path.as_os_str().len(),
        }
    }
    context.name.len()
        + context.cluster_name.len()
        + context.user_name.len()
        + context.server.as_str().len()
        + context.namespace.len()
        + context.certificate_authority.as_ref().map_or(0, cert)
        + context.tls_server_name.as_ref().map_or(0, String::len)
        + context.proxy.as_ref().map_or(0, |p| p.as_str().len())
        + match &context.auth {
            ImportedAuth::Token(value) => secret(value),
            ImportedAuth::ClientCertificate { certificate, key } => cert(certificate) + secret(key),
            ImportedAuth::Oidc {
                issuer,
                client_id,
                client_secret,
                refresh_token,
                scopes,
                certificate_authority,
            } => {
                issuer.as_str().len()
                    + client_id.len()
                    + client_secret
                        .as_ref()
                        .map_or(0, |s| s.expose_secret().len())
                    + refresh_token
                        .as_ref()
                        .map_or(0, |s| s.expose_secret().len())
                    + scopes.iter().map(String::len).sum::<usize>()
                    + certificate_authority.as_ref().map_or(0, cert)
            }
            ImportedAuth::Exec(exec) => {
                exec.command.len()
                    + exec.args.iter().map(String::len).sum::<usize>()
                    + exec
                        .env
                        .iter()
                        .map(|(k, v)| k.len() + v.expose_secret().len())
                        .sum::<usize>()
                    + exec.api_version.len()
            }
        }
}

fn valid_name(value: &str) -> Result<(), ImportError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(ImportError::Name)
    } else {
        Ok(())
    }
}

fn server_url(value: &str) -> Result<Url, ImportError> {
    let server = Url::parse(value).map_err(|_| ImportError::Server)?;
    if server.scheme() != "https"
        || server.host_str().is_none()
        || !server.username().is_empty()
        || server.password().is_some()
        || server.query().is_some()
        || server.fragment().is_some()
        || server.path().contains('%')
        || server.path().contains("//")
    {
        return Err(ImportError::Server);
    }
    Ok(server)
}

fn file_reference(value: &str, source_file: Option<&Path>) -> Result<PathBuf, ImportError> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(ImportError::FilePath);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    // No HOME or environment expansion. File confirmation receives this exact
    // reference; platform-specific resolution happens after that confirmation.
    let parent = source_file
        .and_then(Path::parent)
        .filter(|p| p.is_absolute())
        .ok_or(ImportError::RelativePathNeedsFile)?;
    Ok(parent.join(path))
}

fn decode_secret(value: &str) -> Result<SecretString, ImportError> {
    if value.len() > MAX_CONFIG_BYTES {
        return Err(ImportError::EmbeddedData);
    }
    let compact = Zeroizing::new(
        value
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect::<String>(),
    );
    let bytes = Zeroizing::new(
        STANDARD
            .decode(compact.as_bytes())
            .map_err(|_| ImportError::EmbeddedData)?,
    );
    let text = std::str::from_utf8(&bytes).map_err(|_| ImportError::EmbeddedData)?;
    if text.is_empty() {
        return Err(ImportError::EmbeddedData);
    }
    Ok(SecretString::from(text))
}

fn cert_source(
    data: Option<&str>,
    file: Option<&str>,
    source: Option<&Path>,
) -> Result<CertificateSource, ImportError> {
    match (data, file) {
        (Some(_), Some(_)) => Err(ImportError::AmbiguousAuthentication),
        (Some(data), None) => {
            let decoded = decode_secret(data)?;
            Ok(CertificateSource::Pem(canonical_certificates(
                decoded.expose_secret(),
            )?))
        }
        (None, Some(file)) => Ok(CertificateSource::File(file_reference(file, source)?)),
        (None, None) => Err(ImportError::Authentication),
    }
}

pub(crate) fn canonical_certificates(value: &str) -> Result<String, ImportError> {
    if value.len() > MAX_CONFIG_BYTES {
        return Err(ImportError::EmbeddedData);
    }
    let certificates = pem::parse_many(value).map_err(|_| ImportError::EmbeddedData)?;
    if certificates.is_empty()
        || certificates.len() > 64
        || certificates.iter().any(|p| p.tag() != "CERTIFICATE")
    {
        return Err(ImportError::EmbeddedData);
    }
    let mut roots = rustls::RootCertStore::empty();
    for certificate in &certificates {
        roots
            .add(rustls::pki_types::CertificateDer::from(
                certificate.contents(),
            ))
            .map_err(|_| ImportError::EmbeddedData)?;
    }
    // Unrelated plaintext surrounding PEM blocks never enters inventory.
    Ok(pem::encode_many(&certificates))
}

fn ca_source(
    cluster: &Cluster,
    source: Option<&Path>,
) -> Result<Option<CertificateSource>, ImportError> {
    if cluster.certificate_authority_data.is_none() && cluster.certificate_authority.is_none() {
        return Ok(None);
    }
    cert_source(
        cluster.certificate_authority_data.as_deref(),
        cluster.certificate_authority.as_deref(),
        source,
    )
    .map(Some)
}

fn import_auth(user: &AuthInfo, source: Option<&Path>) -> Result<ImportedAuth, ImportError> {
    if user.impersonate.is_some()
        || user.impersonate_uid.is_some()
        || user.impersonate_groups.is_some()
        || user.impersonate_user_extra.is_some()
    {
        return Err(ImportError::Impersonation);
    }
    if user.username.is_some() || user.password.is_some() {
        return Err(ImportError::Authentication);
    }
    let has_token = user.token.is_some() || user.token_file.is_some();
    let has_cert = user.client_key.is_some()
        || user.client_key_data.is_some()
        || user.client_certificate.is_some()
        || user.client_certificate_data.is_some();
    let count = [
        has_token,
        has_cert,
        user.exec.is_some(),
        user.auth_provider.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    if count > 1 {
        return Err(ImportError::AmbiguousAuthentication);
    }
    if count == 0 {
        return Err(ImportError::Authentication);
    }
    if has_token {
        let token = match (&user.token, &user.token_file) {
            (Some(token), None)
                if !token.expose_secret().is_empty()
                    && !token
                        .expose_secret()
                        .bytes()
                        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) =>
            {
                SecretSource::Inline(token.clone())
            }
            (None, Some(file)) => SecretSource::File(file_reference(file, source)?),
            (Some(_), Some(_)) => return Err(ImportError::AmbiguousAuthentication),
            _ => return Err(ImportError::Authentication),
        };
        return Ok(ImportedAuth::Token(token));
    }
    if has_cert {
        let certificate = cert_source(
            user.client_certificate_data.as_deref(),
            user.client_certificate.as_deref(),
            source,
        )?;
        let key = match (&user.client_key_data, &user.client_key) {
            (Some(data), None) => SecretSource::Inline(decode_secret(data.expose_secret())?),
            (None, Some(file)) => SecretSource::File(file_reference(file, source)?),
            (Some(_), Some(_)) => return Err(ImportError::AmbiguousAuthentication),
            _ => return Err(ImportError::Authentication),
        };
        return Ok(ImportedAuth::ClientCertificate { certificate, key });
    }
    if let Some(exec) = &user.exec {
        let command = exec
            .command
            .clone()
            .filter(|v| !v.trim().is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
            .ok_or(ImportError::Exec)?;
        let version = exec.api_version.as_deref().ok_or(ImportError::Exec)?;
        if !matches!(
            version,
            "client.authentication.k8s.io/v1" | "client.authentication.k8s.io/v1beta1"
        ) || exec.drop_env.as_ref().is_some_and(|v| !v.is_empty())
        {
            return Err(ImportError::Exec);
        }
        let args = exec.args.clone().unwrap_or_default();
        if args.len() > 256
            || args
                .iter()
                .any(|arg| arg.len() > 8192 || arg.contains('\0'))
        {
            return Err(ImportError::Exec);
        }
        let mut env = BTreeMap::new();
        for variable in exec.env.as_deref().unwrap_or_default() {
            let name = variable.get("name").ok_or(ImportError::Exec)?;
            let value = variable.get("value").ok_or(ImportError::Exec)?;
            if variable.len() != 2
                || name.is_empty()
                || name.len() > 128
                || name.as_bytes()[0].is_ascii_digit()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || value.len() > 65536
                || value.contains('\0')
                || env
                    .insert(name.clone(), SecretString::from(value.as_str()))
                    .is_some()
            {
                return Err(ImportError::Exec);
            }
        }
        if env.len() > 256 {
            return Err(ImportError::Exec);
        }
        // A relative command containing separators must stay relative to the
        // source kubeconfig rather than the application's working directory.
        let command = if command.contains('/') || command.contains('\\') {
            file_reference(&command, source)?
                .to_string_lossy()
                .into_owned()
        } else {
            command
        };
        return Ok(ImportedAuth::Exec(ExecSpecification {
            command,
            args: Zeroizing::new(args),
            env,
            api_version: version.into(),
            provide_cluster_info: exec.provide_cluster_info,
            interactive_mode: match &exec.interactive_mode {
                Some(kube::config::ExecInteractiveMode::Never) => ExecInteractiveMode::Never,
                Some(kube::config::ExecInteractiveMode::IfAvailable) => {
                    ExecInteractiveMode::IfAvailable
                }
                Some(kube::config::ExecInteractiveMode::Always) => ExecInteractiveMode::Always,
                None if version.ends_with("/v1beta1") => ExecInteractiveMode::IfAvailable,
                None => return Err(ImportError::Exec),
            },
        }));
    }
    let provider = user
        .auth_provider
        .as_ref()
        .ok_or(ImportError::Authentication)?;
    if provider.name != "oidc" {
        return Err(ImportError::Authentication);
    }
    let get = |key: &str| {
        provider
            .config
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    };
    let issuer = server_url(get("idp-issuer-url").ok_or(ImportError::Oidc)?)
        .map_err(|_| ImportError::Oidc)?;
    let client_id = get("client-id").ok_or(ImportError::Oidc)?.to_owned();
    let scopes = get("extra-scopes")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(ImportedAuth::Oidc {
        issuer,
        client_id,
        client_secret: get("client-secret").map(SecretString::from),
        refresh_token: get("refresh-token").map(SecretString::from),
        certificate_authority: if get("idp-certificate-authority-data").is_some()
            || get("idp-certificate-authority").is_some()
        {
            Some(cert_source(
                get("idp-certificate-authority-data"),
                get("idp-certificate-authority"),
                source,
            )?)
        } else {
            None
        },
        scopes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_credentials_cannot_expand_a_small_file_without_bound() {
        let mut yaml = format!(
            "apiVersion: v1\nkind: Config\nclusters:\n- name: cluster\n  cluster:\n    server: https://cluster.test\nusers:\n- name: user\n  user:\n    token: {}\ncontexts:\n",
            "x".repeat(100_000),
        );
        for index in 0..200 {
            yaml.push_str(&format!(
                "- name: context-{index}\n  context: {{cluster: cluster, user: user}}\n"
            ));
        }
        assert!(yaml.len() < MAX_CONFIG_BYTES);
        assert_eq!(parse(&yaml, None).unwrap_err(), ImportError::Size);
    }

    fn config(auth: &str) -> String {
        format!(
            "apiVersion: v1\nkind: Config\nclusters:\n- name: cluster\n  cluster:\n    server: https://api.example.test:6443\nusers:\n- name: user\n  user:\n{auth}\ncontexts:\n- name: work\n  context:\n    cluster: cluster\n    user: user\n    namespace: team\ncurrent-context: work\n"
        )
    }

    fn selected(text: &str) -> Result<ImportContext, ImportError> {
        parse(text, None)?.entries.remove(0).context
    }

    #[test]
    fn token_import_keeps_the_secret_native_and_redacted() {
        let batch = parse(&config("    token: private-token-value"), None).unwrap();
        assert!(!format!("{batch:?}").contains("private-token-value"));
        assert_eq!(batch.current_context.as_deref(), Some("work"));
        let context = batch.entries.into_iter().next().unwrap().context.unwrap();
        assert_eq!(context.server.as_str(), "https://api.example.test:6443/");
        assert_eq!(context.namespace, "team");
        match context.auth {
            ImportedAuth::Token(SecretSource::Inline(token)) => {
                assert_eq!(token.expose_secret(), "private-token-value")
            }
            _ => panic!("expected token"),
        }
    }

    #[test]
    fn duplicate_yaml_keys_and_named_entries_are_not_silently_merged() {
        let duplicate_key = config("    token: first\n    token: second");
        assert!(matches!(
            parse(&duplicate_key, None),
            Err(ImportError::Yaml)
        ));
        let duplicate_context = config("    token: fixture").replace(
            "current-context: work",
            "  \n- name: work\n  context:\n    cluster: cluster\n    user: user\ncurrent-context: work"
        );
        assert!(matches!(
            parse(&duplicate_context, None),
            Err(ImportError::DuplicateName)
        ));
        let multi_document = format!(
            "{}\n---\n{}",
            config("    token: one"),
            config("    token: two")
        );
        assert!(matches!(
            parse(&multi_document, None),
            Err(ImportError::DuplicateName)
        ));
    }

    #[test]
    fn rejects_basic_impersonation_and_ambiguous_credentials() {
        for auth in [
            "    username: admin\n    password: secret",
            "    token: secret\n    as: admin",
            "    token: secret\n    tokenFile: /tmp/credential",
            "    token: secret\n    exec:\n      command: evil\n      apiVersion: client.authentication.k8s.io/v1",
        ] {
            assert!(selected(&config(auth)).is_err(), "{auth}");
        }
    }

    #[test]
    fn references_are_staged_without_opening_files_or_resolving_the_environment() {
        let text = config("    tokenFile: missing/never-read");
        assert_eq!(
            selected(&text).unwrap_err(),
            ImportError::RelativePathNeedsFile
        );
        let batch = parse(&text, Some(Path::new("/private/fixture/config"))).unwrap();
        let context = batch.entries.into_iter().next().unwrap().context.unwrap();
        match context.auth {
            ImportedAuth::Token(SecretSource::File(path)) => {
                assert_eq!(path, PathBuf::from("/private/fixture/missing/never-read"))
            }
            _ => panic!("expected staged file reference"),
        }
        let context = selected(&config("    tokenFile: /never-existing/config-token")).unwrap();
        assert!(matches!(
            context.auth,
            ImportedAuth::Token(SecretSource::File(_))
        ));
    }

    #[test]
    fn exec_is_only_a_staged_descriptor_and_environment_values_are_secret() {
        let text = config(
            "    exec:\n      apiVersion: client.authentication.k8s.io/v1\n      interactiveMode: Never\n      command: /never-existing/do-not-run\n      args: [get-token, '--secret=private-argument']\n      env:\n      - name: CLOUD_SECRET\n        value: private-environment\n      provideClusterInfo: true"
        );
        let batch = parse(&text, None).unwrap();
        let debug = format!("{batch:?}");
        for value in ["private-argument", "private-environment", "do-not-run"] {
            assert!(!debug.contains(value));
        }
        let context = batch.entries.into_iter().next().unwrap().context.unwrap();
        match context.auth {
            ImportedAuth::Exec(exec) => {
                assert_eq!(exec.command, "/never-existing/do-not-run");
                assert_eq!(
                    exec.env["CLOUD_SECRET"].expose_secret(),
                    "private-environment"
                );
                assert!(exec.provide_cluster_info);
            }
            _ => panic!("expected pending exec approval"),
        }
    }

    #[test]
    fn exec_rejects_duplicate_env_and_unsupported_api_versions() {
        for auth in [
            "    exec:\n      apiVersion: client.authentication.k8s.io/v1alpha1\n      command: cloud",
            "    exec:\n      apiVersion: client.authentication.k8s.io/v1\n      interactiveMode: Never\n      command: cloud\n      env:\n      - {name: TOKEN, value: one}\n      - {name: TOKEN, value: two}",
            "    exec:\n      apiVersion: client.authentication.k8s.io/v1\n      interactiveMode: Never\n      command: cloud\n      env:\n      - {name: 'BAD=NAME', value: one}",
        ] {
            assert_eq!(selected(&config(auth)).unwrap_err(), ImportError::Exec);
        }
    }

    #[test]
    fn embedded_private_key_and_certificate_use_separate_types() {
        let generated =
            rcgen::generate_simple_self_signed(vec!["api.example.test".into()]).unwrap();
        let certificate = generated.cert.pem();
        let key = generated.signing_key.serialize_pem();
        let auth = format!(
            "    client-certificate-data: {}\n    client-key-data: {}",
            STANDARD.encode(&certificate),
            STANDARD.encode(&key)
        );
        let context = selected(&config(&auth)).unwrap();
        assert!(!format!("{context:?}").contains("private-key-material"));
        match context.auth {
            ImportedAuth::ClientCertificate {
                certificate: CertificateSource::Pem(public),
                key: SecretSource::Inline(secret),
            } => {
                assert_eq!(
                    pem::parse(public).unwrap().contents(),
                    generated.cert.der().as_ref()
                );
                assert_eq!(secret.expose_secret(), key);
            }
            _ => panic!("expected split certificate and key"),
        }
        let auth = format!(
            "    client-certificate-data: {}\n    client-key-data: {}",
            STANDARD.encode(&key),
            STANDARD.encode(&key)
        );
        assert_eq!(
            selected(&config(&auth)).unwrap_err(),
            ImportError::EmbeddedData
        );
    }

    #[test]
    fn oidc_preserves_refresh_material_without_reusing_the_old_id_token() {
        let text = config(
            "    auth-provider:\n      name: oidc\n      config:\n        idp-issuer-url: https://issuer.example.test/realm\n        client-id: client\n        client-secret: private-client-secret\n        refresh-token: private-refresh-token\n        id-token: expired-id-token\n        extra-scopes: 'groups,email'"
        );
        let context = selected(&text).unwrap();
        assert!(!format!("{context:?}").contains("private"));
        match context.auth {
            ImportedAuth::Oidc {
                issuer,
                client_id,
                client_secret,
                refresh_token,
                scopes,
                certificate_authority,
            } => {
                assert!(certificate_authority.is_none());
                assert_eq!(issuer.as_str(), "https://issuer.example.test/realm");
                assert_eq!(client_id, "client");
                assert_eq!(
                    client_secret.unwrap().expose_secret(),
                    "private-client-secret"
                );
                assert_eq!(
                    refresh_token.unwrap().expose_secret(),
                    "private-refresh-token"
                );
                assert_eq!(scopes, vec!["groups", "email"]);
            }
            _ => panic!("expected OIDC"),
        }
    }

    #[test]
    fn malformed_yaml_error_never_contains_source_snippets() {
        let text = config("    token: 'private-broken-yaml");
        let error = parse(&text, None).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("private-broken-yaml"));
        assert!(matches!(
            parse(&"x".repeat(MAX_CONFIG_BYTES + 1), None),
            Err(ImportError::Size)
        ));
    }
}
