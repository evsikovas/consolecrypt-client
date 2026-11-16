//! Explicit immutable public CA trust bound to ONE configured server base URL.
//! Never imports a private key, file, env override, user bearer or host root.
use super::*;
use rustls::pki_types::CertificateDer;
use std::sync::OnceLock;
const MAX_ROOTS: usize = 4;
const MAX_DER: usize = 16 * 1024;
const MAX_TOTAL: usize = 64 * 1024;
fn invalid() -> ApiError {
    ApiError::InvalidConfig("invalid extra public server CA trust".into())
}
#[derive(Clone)]
pub struct ApiServerTrust(Arc<Trust>);
struct Trust {
    server: Url,
    roots: Vec<CertificateDer<'static>>,
    standard: OnceLock<Result<Arc<rustls::ClientConfig>, ApiError>>,
    relay: OnceLock<Result<Arc<rustls::ClientConfig>, ApiError>>,
}
impl std::fmt::Debug for ApiServerTrust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiServerTrust")
            .field("public_ca_count", &self.0.roots.len())
            .finish_non_exhaustive()
    }
}
impl ApiServerTrust {
    pub fn from_public_ca_der(
        server_url: &str,
        certificates: Vec<Vec<u8>>,
    ) -> Result<Self, ApiError> {
        let server = ApiConfig::new(server_url, "server-trust-config", Platform::Cli)?.server_url;
        if server.scheme() != "https" || certificates.is_empty() || certificates.len() > MAX_ROOTS {
            return Err(invalid());
        }
        if !cfg!(any(all(unix, not(target_os = "android")), windows)) {
            return Err(invalid());
        }
        let mut roots = Vec::new();
        let mut total = 0usize;
        for der in certificates {
            total = total.checked_add(der.len()).ok_or_else(invalid)?;
            if der.is_empty()
                || der.len() > MAX_DER
                || total > MAX_TOTAL
                || roots
                    .iter()
                    .any(|v: &CertificateDer<'static>| v.as_ref() == der.as_slice())
            {
                return Err(invalid());
            }
            let (remaining, certificate) =
                x509_parser::parse_x509_certificate(&der).map_err(|_| invalid())?;
            if !remaining.is_empty()
                || !certificate
                    .basic_constraints()
                    .map_err(|_| invalid())?
                    .is_some_and(|v| v.value.ca)
            {
                return Err(invalid());
            }
            if certificate
                .key_usage()
                .map_err(|_| invalid())?
                .is_some_and(|v| !v.value.key_cert_sign())
            {
                return Err(invalid());
            }
            // Maintained PKI parser independently rejects unusable DER anchors.
            let cert = CertificateDer::from(der);
            let mut check = rustls::RootCertStore::empty();
            check.add(cert.clone()).map_err(|_| invalid())?;
            roots.push(cert);
        }
        Ok(Self(Arc::new(Trust {
            server,
            roots,
            standard: OnceLock::new(),
            relay: OnceLock::new(),
        })))
    }
    pub fn applies_to(&self, url: &Url) -> bool {
        &self.0.server == url
    }
    fn verifier(&self) -> Result<Arc<rustls_platform_verifier::Verifier>, ApiError> {
        #[cfg(any(all(unix, not(target_os = "android")), windows))]
        {
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
                self.0.roots.clone(),
                provider,
            )
            .map_err(|_| invalid())?;
            Ok(Arc::new(verifier))
        }
        #[cfg(not(any(all(unix, not(target_os = "android")), windows)))]
        {
            Err(invalid())
        }
    }
    fn tls(&self, relay: bool) -> Result<Arc<rustls::ClientConfig>, ApiError> {
        let cell = if relay {
            &self.0.relay
        } else {
            &self.0.standard
        };
        cell.get_or_init(|| {
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            let builder = rustls::ClientConfig::builder_with_provider(provider);
            let builder = if relay {
                builder.with_protocol_versions(&[&rustls::version::TLS13])
            } else {
                builder.with_safe_default_protocol_versions()
            }
            .map_err(|_| invalid())?;
            // Full maintained platform PKI + hostname verification WITH explicit
            // extra roots. This is not an invalid-cert/custom assertion bypass.
            let mut config = builder
                .dangerous()
                .with_custom_certificate_verifier(self.verifier()?)
                .with_no_client_auth();
            config.enable_early_data = false;
            config.resumption = rustls::client::Resumption::disabled();
            Ok(Arc::new(config))
        })
        .clone()
    }
}
impl ApiConfig {
    pub fn with_server_trust(mut self, trust: ApiServerTrust) -> Result<Self, ApiError> {
        if self.server_url.scheme() != "https" || !trust.applies_to(&self.server_url) {
            return Err(invalid());
        }
        self.extra_server_trust = Some(trust);
        Ok(self)
    }
    /// None preserves the original default paths. The shared cached relay
    /// config is cloned into reqwest and passed as-is to maintained WSS.
    pub(crate) fn extra_server_tls(
        &self,
        relay: bool,
    ) -> Result<Option<Arc<rustls::ClientConfig>>, ApiError> {
        let Some(trust) = &self.extra_server_trust else {
            return Ok(None);
        };
        if !trust.applies_to(&self.server_url) || self.server_url.scheme() != "https" {
            return Err(invalid());
        }
        trust.tls(relay).map(Some)
    }
}
#[cfg(test)]
#[path = "relay_trust_tests.rs"]
mod tests;
