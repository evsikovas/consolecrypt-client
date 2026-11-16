//! Maintained TLS1.3 mTLS, layered over the opaque Relay byte carrier.
use cc_agent_node_wire::{Error, Hash, Result, TunnelRoutePolicy};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{client::TlsStream, TlsConnector};

#[derive(Debug)]
struct Pinned {
    pki: Arc<dyn ServerCertVerifier>,
    leaf: [u8; 32],
}
impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        self.pki
            .verify_server_cert(end, intermediates, name, ocsp, now)?;
        let actual: [u8; 32] = Sha256::digest(end.as_ref()).into();
        if actual != self.leaf {
            return Err(rustls::Error::General("tunnel peer denied".into()));
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.pki.verify_tls12_signature(msg, cert, signed)
    }
    fn verify_tls13_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.pki.verify_tls13_signature(msg, cert, signed)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.pki.supported_verify_schemes()
    }
}
/// Service TLS key is supplied from explicit Native provisioning, never Node
/// OS custody, account bearer, Reader issuer or profile DeviceIdentity export.
pub(super) struct TunnelTls {
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
    client: Hash,
    agent: Hash,
}
impl std::fmt::Debug for TunnelTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelTls([redacted])")
    }
}
impl TunnelTls {
    pub fn provisioned(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
        roots: RootCertStore,
        name: ServerName<'static>,
        policy: &TunnelRoutePolicy,
    ) -> Result<Self> {
        policy.validate()?;
        if key.secret_der().len() > 65536
            || chain.is_empty()
            || chain.len() > 4
            || chain.iter().any(|c| c.len() > 65536)
            || roots.is_empty()
        {
            return Err(Error::InvalidKey);
        }
        let client = Hash::from_bytes(Sha256::digest(chain[0].as_ref()).into());
        if client != policy.client_tls_certificate_sha256 {
            return Err(Error::WrongClaim);
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots.clone()),
            provider.clone(),
        )
        .build()
        .map_err(|_| Error::InvalidKey)?;
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Error::InvalidWire)?
            .with_root_certificates(roots)
            .with_client_auth_cert(chain, key)
            .map_err(|_| Error::InvalidKey)?;
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(Pinned {
                pki: verifier,
                leaf: *policy.agent_tls_certificate_sha256.as_bytes(),
            }));
        config.resumption = rustls::client::Resumption::disabled();
        config.enable_early_data = false;
        config.max_fragment_size = Some(16384);
        config.alpn_protocols.clear();
        Ok(Self {
            config: Arc::new(config),
            name,
            client,
            agent: policy.agent_tls_certificate_sha256,
        })
    }
    pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
        policy: &TunnelRoutePolicy,
    ) -> Result<TlsStream<S>> {
        if self.client != policy.client_tls_certificate_sha256
            || self.agent != policy.agent_tls_certificate_sha256
        {
            return Err(Error::WrongClaim);
        }
        let stream = TlsConnector::from(self.config.clone())
            .connect(self.name.clone(), stream)
            .await
            .map_err(|_| Error::InvalidSignature)?;
        if stream.get_ref().1.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3) {
            return Err(Error::InvalidWire);
        }
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    fn policy() -> TunnelRoutePolicy {
        serde_json::from_str(include_str!(
            "../../../agent-node-wire/src/tunnel_policy_vector.json"
        ))
        .unwrap()
    }
    #[tokio::test]
    async fn actual_maintained_tls13_mtls_matches_both_policy_leaves() {
        let agent = rcgen::generate_simple_self_signed(vec!["agent-tunnel.test".into()]).unwrap();
        let client = rcgen::generate_simple_self_signed(vec!["client-tunnel.test".into()]).unwrap();
        let mut p = policy();
        p.agent_tls_certificate_sha256 = Hash::from_bytes(Sha256::digest(agent.cert.der()).into());
        p.client_tls_certificate_sha256 =
            Hash::from_bytes(Sha256::digest(client.cert.der()).into());
        let mut agent_roots = RootCertStore::empty();
        agent_roots.add(agent.cert.der().clone()).unwrap();
        let mut client_roots = RootCertStore::empty();
        client_roots.add(client.cert.der().clone()).unwrap();
        let native = TunnelTls::provisioned(
            vec![client.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(client.signing_key.serialize_der()).into(),
            agent_roots,
            ServerName::try_from("agent-tunnel.test")
                .unwrap()
                .to_owned(),
            &p,
        )
        .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(client_roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let server = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![agent.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(agent.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let (a, b) = tokio::io::duplex(65536);
        let run = async {
            let serve = async {
                let mut stream = tokio_rustls::TlsAcceptor::from(Arc::new(server))
                    .accept(a)
                    .await
                    .unwrap();
                assert_eq!(
                    stream.get_ref().1.protocol_version(),
                    Some(rustls::ProtocolVersion::TLSv1_3)
                );
                let peer = stream.get_ref().1.peer_certificates().unwrap();
                assert_eq!(
                    Hash::from_bytes(Sha256::digest(peer[0].as_ref()).into()),
                    p.client_tls_certificate_sha256
                );
                let mut bytes = [0; 6];
                stream.read_exact(&mut bytes).await.unwrap();
                assert_eq!(&bytes, b"opaque");
                stream.write_all(&bytes).await.unwrap();
                stream.flush().await.unwrap();
            };
            let connect = async {
                let mut stream = native.connect(b, &p).await.unwrap();
                stream.write_all(b"opaque").await.unwrap();
                stream.flush().await.unwrap();
                let mut echo = [0; 6];
                stream.read_exact(&mut echo).await.unwrap();
                assert_eq!(&echo, b"opaque");
            };
            tokio::join!(serve, connect);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), run)
            .await
            .unwrap();
    }
    #[test]
    fn foreign_client_tls_leaf_is_rejected_before_transport() {
        let agent = rcgen::generate_simple_self_signed(vec!["agent-tunnel.test".into()]).unwrap();
        let client = rcgen::generate_simple_self_signed(vec!["client-tunnel.test".into()]).unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(agent.cert.der().clone()).unwrap();
        let p = policy();
        assert!(TunnelTls::provisioned(
            vec![client.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(client.signing_key.serialize_der()).into(),
            roots,
            ServerName::try_from("agent-tunnel.test")
                .unwrap()
                .to_owned(),
            &p
        )
        .is_err());
    }
}
