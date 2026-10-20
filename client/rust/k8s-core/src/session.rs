//! Explicit session construction. No environment inference, kubeconfig loading,
//! credential files, auth-provider hooks or exec plugins run in this module.

use crate::policy::{Audit, GuardLayer, Policy, PolicyError, ProdGuard};
use base64::{engine::general_purpose::STANDARD, Engine};
use http::{header::HOST, HeaderValue};
use kube::{client::ClientBuilder, Client, Config};
use secrecy::{ExposeSecret, SecretString};
use std::{fmt, net::SocketAddr, sync::Arc, time::Duration};
use url::Url;
use zeroize::Zeroizing;

const MAX_CERT_BYTES: usize = 4 * 1024 * 1024;

/// Resolved in memory from the vault or from an explicitly approved auth flow.
/// This type cannot name a local credential file or an executable.
pub enum Authentication {
    Bearer(SecretString),
    ClientCertificate {
        certificate_pem: String,
        private_key_pem: SecretString,
    },
}

impl fmt::Debug for Authentication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bearer(_) => "Authentication::Bearer([redacted])",
            Self::ClientCertificate { .. } => "Authentication::ClientCertificate([redacted])",
        })
    }
}

#[derive(Clone)]
pub enum ConnectionRoute {
    Direct,
    /// A loopback listener owned by this session's SSH tunnel. TLS still
    /// verifies the original server name and HTTP uses the original Host.
    SshLoopback(SocketAddr),
    /// HTTP CONNECT or SOCKS5, with the original destination and TLS intact.
    /// Credentials are handled separately when resolving the saved proxy.
    Proxy(Url),
}

impl fmt::Debug for ConnectionRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => f.write_str("Direct"),
            Self::SshLoopback(address) => f.debug_tuple("SshLoopback").field(address).finish(),
            Self::Proxy(_) => f.write_str("Proxy([redacted])"),
        }
    }
}

#[derive(Clone)]
pub struct ConnectOptions {
    pub server: Url,
    pub namespace: String,
    /// DER-encoded trust anchors, already parsed from the imported PEM.
    pub root_certificates: Vec<Vec<u8>>,
    pub tls_server_name: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub route: ConnectionRoute,
}

impl fmt::Debug for ConnectOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Even invalid URLs may contain credentials before validation.
        f.debug_struct("ConnectOptions")
            .field("route", &self.route)
            .field("root_certificates", &self.root_certificates.len())
            .field("insecure_skip_tls_verify", &self.insecure_skip_tls_verify)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    #[error(
        "A Kubernetes API server must have an HTTPS URL without credentials or query parameters"
    )]
    InvalidServer,
    #[error("The Kubernetes namespace is invalid")]
    InvalidNamespace,
    #[error("The TLS server name is invalid")]
    InvalidTlsName,
    #[error("The SSH endpoint must be a loopback address with a nonzero port")]
    InvalidTunnel,
    #[error("The proxy URL is unsupported")]
    InvalidProxy,
    #[error("The certificate data is empty or too large")]
    InvalidCertificate,
    #[error("The authentication data is empty or invalid")]
    InvalidAuthentication,
    #[error("The TLS or authentication configuration could not be initialized")]
    Configuration,
    #[error(transparent)]
    Policy(#[from] PolicyError),
}

pub struct KubernetesSession {
    client: Client,
    guard: ProdGuard,
}

impl fmt::Debug for KubernetesSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KubernetesSession")
            .field("guard", &self.guard)
            .finish_non_exhaustive()
    }
}

impl KubernetesSession {
    /// Does not contact the cluster. Every resulting client clone carries the
    /// guard before kube's fixed base-URI, credentials and transport layers.
    pub fn new(
        options: ConnectOptions,
        auth: Authentication,
        policy: Policy,
        audit: Arc<dyn Audit>,
    ) -> Result<Self, ConnectError> {
        Self::new_scoped(
            options,
            auth,
            policy,
            audit,
            tokio_util::sync::CancellationToken::new(),
        )
    }

    pub fn new_scoped(
        options: ConnectOptions,
        auth: Authentication,
        policy: Policy,
        audit: Arc<dyn Audit>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<Self, ConnectError> {
        let guard = ProdGuard::new_scoped(
            policy,
            options.insecure_skip_tls_verify,
            audit,
            cancellation,
        )?;
        let config = explicit_config(options, auth)?;
        // Avoid ambiguous automatic provider selection when another native
        // module also enables aws-lc. Preserve an already selected provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = ClientBuilder::try_from(config)
            .map_err(|_| ConnectError::Configuration)?
            .with_layer(&crate::stream_scope::BodyScopeLayer(guard.cancellation()))
            .with_layer(&GuardLayer(guard.clone()))
            .build();
        Ok(Self { client, guard })
    }

    pub fn client(&self) -> Client {
        self.client.clone()
    }

    pub fn guard(&self) -> &ProdGuard {
        &self.guard
    }

    pub fn close(&self) {
        self.guard.close();
    }

    /// Own the complete lifetime of a consumer, including websocket upgrades.
    /// Lock/revocation drops the future and its open streams. Runtime workers
    /// must use this for exec/attach/forward, not just for their HTTP handshake.
    pub async fn scoped<F: std::future::Future>(
        &self,
        future: F,
    ) -> Result<F::Output, PolicyError> {
        let cancel = self.guard.cancellation();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(PolicyError::Closed),
            value = future => Ok(value),
        }
    }
}

impl Drop for KubernetesSession {
    fn drop(&mut self) {
        // Escaped client clones cannot start requests after their owner closes.
        self.close();
    }
}

fn explicit_config(options: ConnectOptions, auth: Authentication) -> Result<Config, ConnectError> {
    let server = &options.server;
    if server.scheme() != "https"
        || server.host_str().is_none()
        || !server.username().is_empty()
        || server.password().is_some()
        || server.query().is_some()
        || server.fragment().is_some()
        || server.path().contains('%')
        || server.path().contains("//")
    {
        return Err(ConnectError::InvalidServer);
    }
    let namespace = &options.namespace;
    if namespace.is_empty()
        || namespace.len() > 63
        || namespace.starts_with('-')
        || namespace.ends_with('-')
        || !namespace
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(ConnectError::InvalidNamespace);
    }
    let mut endpoint = server.clone();
    let original_name = match server.host().ok_or(ConnectError::InvalidServer)? {
        url::Host::Ipv6(address) => address.to_string(),
        host => host.to_string(),
    };
    let tls_name = options.tls_server_name.unwrap_or(original_name);
    rustls::pki_types::ServerName::try_from(tls_name.clone())
        .map_err(|_| ConnectError::InvalidTlsName)?;
    if let ConnectionRoute::SshLoopback(address) = &options.route {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(ConnectError::InvalidTunnel);
        }
        endpoint
            .set_ip_host(address.ip())
            .map_err(|_| ConnectError::InvalidTunnel)?;
        endpoint
            .set_port(Some(address.port()))
            .map_err(|_| ConnectError::InvalidTunnel)?;
    }
    let mut config = Config::new(
        endpoint
            .as_str()
            .parse()
            .map_err(|_| ConnectError::InvalidServer)?,
    );
    config.default_namespace = options.namespace;
    config.accept_invalid_certs = options.insecure_skip_tls_verify;
    config.tls_server_name = Some(tls_name);
    config.connect_timeout = Some(Duration::from_secs(15));
    config.write_timeout = Some(Duration::from_secs(30));
    // The native session owns bounded retries and scope revalidation. A
    // transport retry must not repeat a previously authorized mutation.
    config.default_retry = false;
    if !options.root_certificates.is_empty() {
        if options.root_certificates.len() > 64
            || options.root_certificates.iter().any(Vec::is_empty)
            || options
                .root_certificates
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                > MAX_CERT_BYTES
        {
            return Err(ConnectError::InvalidCertificate);
        }
        config.root_cert = Some(options.root_certificates);
    }
    match options.route {
        ConnectionRoute::Direct => {}
        ConnectionRoute::SshLoopback(_) => {
            let original: http::Uri = server
                .as_str()
                .parse()
                .map_err(|_| ConnectError::InvalidServer)?;
            config.headers.push((
                HOST,
                HeaderValue::from_str(
                    original
                        .authority()
                        .ok_or(ConnectError::InvalidServer)?
                        .as_str(),
                )
                .map_err(|_| ConnectError::InvalidServer)?,
            ));
        }
        ConnectionRoute::Proxy(proxy) => {
            // kube's HTTPS-proxy connector shares the cluster TLS settings.
            // A proxy needs its own trust/name configuration; until that is
            // available, accept only the saved HTTP CONNECT/SOCKS5 routes.
            if !matches!(proxy.scheme(), "http" | "socks5")
                || proxy.host_str().is_none()
                || !proxy.username().is_empty()
                || proxy.password().is_some()
                || !matches!(proxy.path(), "" | "/")
                || proxy.query().is_some()
                || proxy.fragment().is_some()
            {
                return Err(ConnectError::InvalidProxy);
            }
            config.proxy_url = Some(
                proxy
                    .as_str()
                    .parse()
                    .map_err(|_| ConnectError::InvalidProxy)?,
            );
        }
    }
    match auth {
        Authentication::Bearer(token) => {
            if token.expose_secret().is_empty()
                || token.expose_secret().len() > 1024 * 1024
                || token
                    .expose_secret()
                    .bytes()
                    .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
            {
                return Err(ConnectError::InvalidAuthentication);
            }
            config.auth_info.token = Some(token);
        }
        Authentication::ClientCertificate {
            certificate_pem,
            private_key_pem,
        } => {
            if certificate_pem.is_empty()
                || certificate_pem.len() > MAX_CERT_BYTES
                || private_key_pem.expose_secret().is_empty()
                || private_key_pem.expose_secret().len() > MAX_CERT_BYTES
            {
                return Err(ConnectError::InvalidCertificate);
            }
            config.auth_info.client_certificate_data = Some(STANDARD.encode(certificate_pem));
            let encoded = Zeroizing::new(STANDARD.encode(private_key_pem.expose_secret()));
            config.auth_info.client_key_data = Some(SecretString::from(encoded.as_str()));
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ConnectOptions {
        ConnectOptions {
            server: Url::parse("https://api.example.test:6443").unwrap(),
            namespace: "default".into(),
            root_certificates: Vec::new(),
            tls_server_name: None,
            insecure_skip_tls_verify: false,
            route: ConnectionRoute::Direct,
        }
    }

    fn token() -> Authentication {
        Authentication::Bearer(SecretString::from("private-test-token"))
    }

    #[test]
    fn direct_configuration_does_not_load_local_files_or_run_auth_plugins() {
        let config = explicit_config(options(), token()).unwrap();
        assert_eq!(
            config.cluster_url.to_string(),
            "https://api.example.test:6443/"
        );
        assert_eq!(config.tls_server_name.as_deref(), Some("api.example.test"));
        assert!(!config.default_retry);
        assert!(config.auth_info.exec.is_none());
        assert!(config.auth_info.auth_provider.is_none());
        assert!(config.auth_info.token_file.is_none());
        assert!(config.auth_info.client_key.is_none());
        assert!(config.auth_info.client_certificate.is_none());
        assert!(config.root_cert_file.is_none());
        assert!(!format!("{:?}", token()).contains("private-test-token"));
    }

    #[test]
    fn ssh_tunnel_preserves_original_tls_name_and_http_host() {
        let mut settings = options();
        settings.route = ConnectionRoute::SshLoopback("127.0.0.1:41234".parse().unwrap());
        let config = explicit_config(settings, token()).unwrap();
        assert_eq!(config.cluster_url.to_string(), "https://127.0.0.1:41234/");
        assert_eq!(config.tls_server_name.as_deref(), Some("api.example.test"));
        assert_eq!(
            config.headers,
            vec![(HOST, HeaderValue::from_static("api.example.test:6443"))]
        );
    }

    #[test]
    fn rejects_unsafe_server_tunnel_and_namespace() {
        for url in [
            "http://api.example.test",
            "https://user:password@api.example.test",
            "https://api.example.test/?token=secret",
            "https://api.example.test/#token",
            "https://api.example.test/prefix%2fescape",
        ] {
            let mut settings = options();
            settings.server = Url::parse(url).unwrap();
            assert_eq!(
                explicit_config(settings, token()).unwrap_err(),
                ConnectError::InvalidServer
            );
        }
        let mut settings = options();
        settings.route = ConnectionRoute::SshLoopback("192.0.2.1:6443".parse().unwrap());
        assert_eq!(
            explicit_config(settings, token()).unwrap_err(),
            ConnectError::InvalidTunnel
        );
        for namespace in ["", "bad/name", "-default", "default-", "Default", "a.b"] {
            let mut settings = options();
            settings.namespace = namespace.into();
            assert_eq!(
                explicit_config(settings, token()).unwrap_err(),
                ConnectError::InvalidNamespace
            );
        }
    }

    #[test]
    fn rejects_token_header_injection() {
        for token in [
            "",
            "value\r\nImpersonate-User: admin",
            "value with spaces",
            "value\0",
        ] {
            assert_eq!(
                explicit_config(options(), Authentication::Bearer(SecretString::from(token)))
                    .unwrap_err(),
                ConnectError::InvalidAuthentication
            );
        }
    }

    #[test]
    fn ipv6_server_name_is_an_ip_without_url_brackets() {
        let mut settings = options();
        settings.server = Url::parse("https://[2001:db8::1]:6443").unwrap();
        let config = explicit_config(settings, token()).unwrap();
        assert_eq!(config.tls_server_name.as_deref(), Some("2001:db8::1"));
    }

    #[test]
    fn proxy_preserves_cluster_identity_and_rejects_ambiguous_tls_proxy() {
        for scheme in ["http", "socks5"] {
            let mut settings = options();
            settings.route =
                ConnectionRoute::Proxy(Url::parse(&format!("{scheme}://proxy.test:8080")).unwrap());
            let expected_server = settings.server.to_string();
            let config = explicit_config(settings, token()).unwrap();
            assert_eq!(config.cluster_url.to_string(), expected_server);
            assert_eq!(config.tls_server_name.as_deref(), Some("api.example.test"));
            assert_eq!(
                config.proxy_url.unwrap().to_string(),
                format!("{scheme}://proxy.test:8080/")
            );
        }
        let mut settings = options();
        settings.route = ConnectionRoute::Proxy(Url::parse("https://proxy.test:8443").unwrap());
        assert_eq!(
            explicit_config(settings, token()).unwrap_err(),
            ConnectError::InvalidProxy
        );
    }

    #[test]
    fn even_rejected_configuration_is_redacted_in_debug_output() {
        let mut settings = options();
        settings.server = Url::parse("https://user:private-password@api.example.test").unwrap();
        settings.route = ConnectionRoute::Proxy(
            Url::parse("http://user:private-proxy-password@proxy.example.test").unwrap(),
        );
        assert!(!format!("{settings:?}").contains("private"));
        assert!(!format!("{settings:?}").contains("password"));
    }
}
