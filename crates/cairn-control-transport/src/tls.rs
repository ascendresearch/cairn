use std::{
    fmt, fs,
    io::BufReader,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    server::WebPkiClientVerifier,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::{WebSocketStream, accept_async_with_config, client_async_with_config};

use crate::{TransportError, TransportPolicy};

/// SHA-256 fingerprint of one verified DER leaf certificate.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CertificateFingerprint([u8; 32]);

impl CertificateFingerprint {
    /// Hashes exact DER certificate bytes.
    #[must_use]
    pub fn from_der(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Loads the first certificate in a PEM file and returns its DER fingerprint.
    ///
    /// # Errors
    ///
    /// Returns an error for file I/O, invalid PEM, or an empty certificate chain.
    pub fn from_pem_file(path: impl AsRef<Path>) -> Result<Self, TransportError> {
        let certificates = read_certificates(path.as_ref())?;
        certificates
            .first()
            .map(|certificate| Self::from_der(certificate.as_ref()))
            .ok_or_else(|| TransportError::TlsMaterial("certificate chain is empty".into()))
    }

    fn hexadecimal(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

impl fmt::Display for CertificateFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "sha256:{}", self.hexadecimal())
    }
}

impl fmt::Debug for CertificateFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CertificateFingerprint")
            .field(&self.to_string())
            .finish()
    }
}

impl FromStr for CertificateFingerprint {
    type Err = TransportError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digest = value
            .strip_prefix("sha256:")
            .ok_or(TransportError::InvalidCertificateFingerprint)?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(TransportError::InvalidCertificateFingerprint);
        }
        let mut bytes = [0_u8; 32];
        for (index, pair) in digest.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for CertificateFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CertificateFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Files used for a mutually authenticated worker client.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientTlsFiles {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub server_ca: PathBuf,
    pub server_name: String,
}

/// Files used for a mutually authenticated controller listener.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerTlsFiles {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub client_ca: PathBuf,
}

/// Worker-side mTLS WebSocket type.
pub type ClientWebSocket = WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

/// Controller-side mTLS WebSocket type.
pub type ServerWebSocket = WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>;

/// Connects TCP, verifies the controller, presents the worker certificate, and upgrades to a
/// WebSocket. The endpoint is an HTTP WebSocket URI while TLS is configured independently.
///
/// # Errors
///
/// Returns an error for TLS material, DNS/TCP, mTLS, URI, or WebSocket handshake failure.
pub async fn connect_worker_socket<A: ToSocketAddrs>(
    address: A,
    websocket_uri: &str,
    tls: &ClientTlsFiles,
    policy: TransportPolicy,
) -> Result<ClientWebSocket, TransportError> {
    let tcp = TcpStream::connect(address).await?;
    let config = client_config(tls)?;
    let name = ServerName::try_from(tls.server_name.clone())
        .map_err(|_| TransportError::InvalidServerName)?;
    let tls_stream = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|error| TransportError::TlsHandshake(error.to_string()))?;
    let (socket, _) =
        client_async_with_config(websocket_uri, tls_stream, Some(policy.websocket_config()))
            .await
            .map_err(|error| TransportError::WebSocket(error.to_string()))?;
    Ok(socket)
}

/// Accepts and verifies one mTLS client, returns its leaf fingerprint, and upgrades to WebSocket.
///
/// # Errors
///
/// Returns an error for TLS material/handshake, a missing peer certificate, or WebSocket failure.
pub async fn accept_worker_socket(
    tcp: TcpStream,
    tls: Arc<ServerConfig>,
    policy: TransportPolicy,
) -> Result<(ServerWebSocket, CertificateFingerprint, SocketAddr), TransportError> {
    let peer = tcp.peer_addr()?;
    let tls_stream = TlsAcceptor::from(tls)
        .accept(tcp)
        .await
        .map_err(|error| TransportError::TlsHandshake(error.to_string()))?;
    let certificate = tls_stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or(TransportError::MissingPeerCertificate)?;
    let fingerprint = CertificateFingerprint::from_der(certificate.as_ref());
    let socket = accept_async_with_config(tls_stream, Some(policy.websocket_config()))
        .await
        .map_err(|error| TransportError::WebSocket(error.to_string()))?;
    Ok((socket, fingerprint, peer))
}

impl ServerTlsFiles {
    /// Loads a rustls configuration that requires a client certificate chaining to `client_ca`.
    ///
    /// # Errors
    ///
    /// Returns an error for file I/O, invalid certificates/keys, or verifier construction.
    pub fn load(&self) -> Result<Arc<ServerConfig>, TransportError> {
        install_crypto_provider();
        let certificates = read_certificates(&self.certificate)?;
        let key = read_private_key(&self.private_key)?;
        let roots = read_roots(&self.client_ca)?;
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|error| TransportError::TlsMaterial(error.to_string()))?;
        let config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certificates, key)
            .map_err(|error| TransportError::TlsMaterial(error.to_string()))?;
        Ok(Arc::new(config))
    }
}

fn client_config(files: &ClientTlsFiles) -> Result<ClientConfig, TransportError> {
    install_crypto_provider();
    let roots = read_roots(&files.server_ca)?;
    let certificates = read_certificates(&files.certificate)?;
    let key = read_private_key(&files.private_key)?;
    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(certificates, key)
        .map_err(|error| TransportError::TlsMaterial(error.to_string()))
}

fn install_crypto_provider() {
    // A full workspace can enable more than one rustls provider through unrelated dependencies.
    // Select one deliberately at this transport boundary; an already-installed provider is valid.
    let _installed = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn read_roots(path: &Path) -> Result<RootCertStore, TransportError> {
    let certificates = read_certificates(path)?;
    let mut roots = RootCertStore::empty();
    let (added, ignored) = roots.add_parsable_certificates(certificates);
    if added == 0 || ignored != 0 {
        return Err(TransportError::TlsMaterial(format!(
            "CA store added {added} certificates and ignored {ignored}"
        )));
    }
    Ok(roots)
}

fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TransportError> {
    let bytes = fs::read(path)?;
    let mut reader = BufReader::new(bytes.as_slice());
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| TransportError::TlsMaterial(error.to_string()))?;
    if certificates.is_empty() {
        return Err(TransportError::TlsMaterial(format!(
            "certificate file {} is empty",
            path.display()
        )));
    }
    Ok(certificates)
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TransportError> {
    let bytes = fs::read(path)?;
    let mut reader = BufReader::new(bytes.as_slice());
    rustls_pemfile::private_key(&mut reader)
        .map_err(|error| TransportError::TlsMaterial(error.to_string()))?
        .ok_or_else(|| {
            TransportError::TlsMaterial(format!(
                "private key file {} contains no supported key",
                path.display()
            ))
        })
}

fn nibble(byte: u8) -> Result<u8, TransportError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(TransportError::InvalidCertificateFingerprint),
    }
}
