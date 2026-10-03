use crate::{CertificateInfo, RdpError};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use x509_cert::{der::Decode, Certificate};

#[derive(Debug)]
struct CertificateVerifier {
    pin: Option<[u8; 32]>,
    observed: Arc<Mutex<Option<Vec<u8>>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
    mismatch: Arc<AtomicBool>,
}

impl ServerCertVerifier for CertificateVerifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // A probe authenticates no user. Actual connections require the complete DER fingerprint.
        // Handshake signatures below still prove possession of the matching certificate key.
        if cert.len() > 64 * 1024 {
            return Err(rustls::Error::General("certificate limit".into()));
        }
        let fingerprint: [u8; 32] = Sha256::digest(cert.as_ref()).into();
        if self.pin.is_some_and(|expected| expected != fingerprint) {
            self.mismatch.store(true, Ordering::Relaxed);
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        let parsed = Certificate::from_der(cert.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        if self.pin.is_some()
            && (now.as_secs()
                < parsed
                    .tbs_certificate
                    .validity
                    .not_before
                    .to_unix_duration()
                    .as_secs()
                || now.as_secs()
                    > parsed
                        .tbs_certificate
                        .validity
                        .not_after
                        .to_unix_duration()
                        .as_secs())
        {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired,
            ));
        }
        *self
            .observed
            .lock()
            .map_err(|_| rustls::Error::General("certificate unavailable".into()))? =
            Some(cert.as_ref().to_vec());
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub(crate) async fn upgrade(
    stream: TcpStream,
    name: &str,
    pin: Option<[u8; 32]>,
) -> Result<(TlsStream<TcpStream>, Vec<u8>, CertificateInfo), RdpError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let observed = Arc::new(Mutex::new(None));
    let mismatch = Arc::new(AtomicBool::new(false));
    let verifier = Arc::new(CertificateVerifier {
        pin,
        observed: observed.clone(),
        provider: provider.clone(),
        mismatch: mismatch.clone(),
    });
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| RdpError::Tls)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();
    config.enable_early_data = false;
    let name = ServerName::try_from(name.to_owned()).map_err(|_| RdpError::InvalidConfig)?;
    let stream = TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|_| {
            if mismatch.load(Ordering::Relaxed) {
                RdpError::CertificateMismatch
            } else {
                RdpError::Tls
            }
        })?;
    let der = observed
        .lock()
        .map_err(|_| RdpError::Tls)?
        .take()
        .ok_or(RdpError::Tls)?;
    let cert = Certificate::from_der(&der).map_err(|_| RdpError::Tls)?;
    let public_key = cert
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .ok_or(RdpError::Tls)?
        .to_vec();
    let sha256: [u8; 32] = Sha256::digest(&der).into();
    let fingerprint = sha256
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok((
        stream,
        public_key,
        CertificateInfo {
            sha256,
            fingerprint,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use tokio::{io::AsyncReadExt, net::TcpListener};
    use tokio_rustls::TlsAcceptor;

    async fn endpoint() -> (u16, [u8; 32], tokio::task::JoinHandle<bool>) {
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let pin = Sha256::digest(cert.as_ref()).into();
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert],
            PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der()).into(),
        )
        .unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            match TlsAcceptor::from(Arc::new(server)).accept(tcp).await {
                Ok(mut stream) => {
                    let mut buf = [0; 1];
                    let _ = stream.read(&mut buf).await;
                    true
                }
                Err(_) => false,
            }
        });
        (port, pin, task)
    }

    #[tokio::test]
    async fn explicit_pin_proves_tls_key_and_returns_matching_metadata() {
        let (port, pin, task) = endpoint().await;
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (stream, key, info) = upgrade(tcp, "localhost", Some(pin)).await.unwrap();
        assert_eq!(info.sha256, pin);
        assert!(!key.is_empty());
        drop(stream);
        assert!(task.await.unwrap());
    }
    #[tokio::test]
    async fn wrong_pin_fails_handshake_before_application_authentication() {
        let (port, mut pin, task) = endpoint().await;
        pin[0] ^= 1;
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert!(matches!(
            upgrade(tcp, "localhost", Some(pin)).await,
            Err(RdpError::CertificateMismatch)
        ));
        assert!(!task.await.unwrap());
    }
    #[tokio::test]
    async fn probe_returns_untrusted_certificate_without_authenticating() {
        let (port, pin, task) = endpoint().await;
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (stream, _, info) = upgrade(tcp, "localhost", None).await.unwrap();
        assert_eq!(info.sha256, pin);
        drop(stream);
        assert!(task.await.unwrap());
    }
}
