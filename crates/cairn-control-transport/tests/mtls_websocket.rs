use std::{error::Error, fs};

use cairn_control_transport::{
    CertificateFingerprint, ClientTlsFiles, ServerTlsFiles, TransportPolicy, accept_worker_socket,
    connect_worker_socket, read_wire_message, write_wire_message,
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    ordinal: u64,
    value: String,
}

#[tokio::test]
async fn mutually_authenticated_binary_json_round_trip() -> Result<(), Box<dyn Error + Send + Sync>>
{
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let ca = directory.path().join("ca.pem");
    let server_certificate = directory.path().join("server.pem");
    let server_key = directory.path().join("server-key.pem");
    let worker_certificate = directory.path().join("worker.pem");
    let worker_key = directory.path().join("worker-key.pem");
    fs::write(&ca, &pki.ca_certificate)?;
    fs::write(&server_certificate, &pki.server.certificate)?;
    fs::write(&server_key, &pki.server.private_key)?;
    fs::write(&worker_certificate, &pki.worker.certificate)?;
    fs::write(&worker_key, &pki.worker.private_key)?;

    let expected_fingerprint = CertificateFingerprint::from_pem_file(&worker_certificate)?;
    let server_tls = ServerTlsFiles {
        certificate: server_certificate,
        private_key: server_key,
        client_ca: ca.clone(),
    }
    .load()?;
    let client_tls = ClientTlsFiles {
        certificate: worker_certificate,
        private_key: worker_key,
        server_ca: ca,
        server_name: "localhost".into(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await?;
        let (mut socket, fingerprint, _) =
            accept_worker_socket(tcp, server_tls, TransportPolicy::default()).await?;
        let received: Probe = read_wire_message(&mut socket, TransportPolicy::default()).await?;
        write_wire_message(&mut socket, &received, TransportPolicy::default()).await?;
        Ok::<_, Box<dyn Error + Send + Sync>>((fingerprint, received))
    });

    let mut client = connect_worker_socket(
        address,
        &format!("wss://localhost:{}/control", address.port()),
        &client_tls,
        TransportPolicy::default(),
    )
    .await?;
    let sent = Probe {
        ordinal: 7,
        value: "canonical-binary-json".into(),
    };
    write_wire_message(&mut client, &sent, TransportPolicy::default()).await?;
    let echoed: Probe = read_wire_message(&mut client, TransportPolicy::default()).await?;
    let (fingerprint, received) = server.await??;
    assert_eq!(fingerprint, expected_fingerprint);
    assert_eq!(received, sent);
    assert_eq!(echoed, sent);
    Ok(())
}

struct TestPki {
    ca_certificate: String,
    server: PemIdentity,
    worker: PemIdentity,
}

struct PemIdentity {
    certificate: String,
    private_key: String,
}

fn test_pki() -> Result<TestPki, rcgen::Error> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Cairn control test CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let ca = ca_params.self_signed(&ca_key)?;
    Ok(TestPki {
        ca_certificate: ca.pem(),
        server: signed_identity(
            "localhost",
            vec!["localhost".into()],
            ExtendedKeyUsagePurpose::ServerAuth,
            &ca,
            &ca_key,
        )?,
        worker: signed_identity(
            "worker-a",
            Vec::new(),
            ExtendedKeyUsagePurpose::ClientAuth,
            &ca,
            &ca_key,
        )?,
    })
}

fn signed_identity(
    common_name: &str,
    subject_alt_names: Vec<String>,
    purpose: ExtendedKeyUsagePurpose,
    ca: &rcgen::Certificate,
    ca_key: &KeyPair,
) -> Result<PemIdentity, rcgen::Error> {
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::new(subject_alt_names)?;
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![purpose];
    let certificate = params.signed_by(&key, ca, ca_key)?;
    Ok(PemIdentity {
        certificate: certificate.pem(),
        private_key: key.serialize_pem(),
    })
}
