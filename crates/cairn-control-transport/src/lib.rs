//! Outbound mutually authenticated WebSocket transport for the worker-control protocol.
//!
//! This crate owns TLS, WebSocket, handshake envelopes, and bounded wire I/O. It does not decide
//! assignment, lease, execution, acknowledgement, or receipt truth.

mod enrollment;
mod tls;
mod wire;

pub use enrollment::{
    EnrollmentBundle, EnrollmentEndpoint, EnrollmentPurpose, EnrollmentRejectCode,
    EnrollmentRequest, EnrollmentResponse, EnrollmentSecret, IssuedWorkerCredential,
    WorkerControlEndpoint,
};

pub use tls::{
    CertificateFingerprint, ClientTlsFiles, ClientWebSocket, ServerTlsFiles, ServerWebSocket,
    accept_enrollment_socket, accept_worker_socket, connect_enrollment_socket,
    connect_worker_socket,
};
pub use wire::{
    ControllerRejectCode, ControllerWireMessage, TransportMessageByteLimit, TransportPolicy,
    WorkerWireMessage, read_wire_message, write_wire_message,
};

use thiserror::Error;

/// Network, authentication, framing, or canonical wire failure.
#[derive(Debug, Error)]
pub enum TransportError {
    /// An enabled transport bound cannot be zero.
    #[error("control transport message byte limit must be positive or disabled")]
    ZeroMessageLimit,
    /// Canonical wire bytes exceed the configured bound.
    #[error("control transport message is {observed} bytes, exceeding configured limit {limit}")]
    MessageTooLarge { observed: u64, limit: u64 },
    /// Canonical JSON encoding/decoding failed.
    #[error("control transport canonical JSON failed: {0}")]
    Codec(String),
    /// A text/continuation/unsupported WebSocket message crossed the binary JSON boundary.
    #[error("unsupported worker-control WebSocket message: {0}")]
    UnsupportedMessage(&'static str),
    /// The peer closed the WebSocket.
    #[error("worker-control WebSocket closed")]
    Closed,
    /// WebSocket protocol or I/O failed.
    #[error("worker-control WebSocket failed: {0}")]
    WebSocket(String),
    /// TCP connect/listen I/O failed.
    #[error("worker-control TCP failed: {0}")]
    Io(#[from] std::io::Error),
    /// Certificate or key material is invalid.
    #[error("worker-control TLS material is invalid: {0}")]
    TlsMaterial(String),
    /// TLS handshake or peer verification failed.
    #[error("worker-control TLS handshake failed: {0}")]
    TlsHandshake(String),
    /// The authenticated TLS peer did not present a leaf certificate.
    #[error("mutual TLS peer certificate is missing")]
    MissingPeerCertificate,
    /// Configured DNS server name is invalid.
    #[error("worker-control TLS server name is invalid")]
    InvalidServerName,
    /// Certificate fingerprint wire form is invalid.
    #[error("certificate fingerprint must be sha256 plus 64 lowercase hexadecimal characters")]
    InvalidCertificateFingerprint,
    /// Enrollment bearer secrets are exact lowercase 256-bit hexadecimal values.
    #[error("enrollment secret must be exactly 64 lowercase hexadecimal characters")]
    InvalidEnrollmentSecret,
}
