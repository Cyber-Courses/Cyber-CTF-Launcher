//! Trust on first use for a server's self-signed TLS certificate (Proxmox ships one). The learner
//! confirms the certificate's SHA-256 fingerprint once, as the server's own page shows it; it is
//! kept with the server, and from then on only that certificate is accepted: a different one is
//! refused before anything (a token, a password, a console ticket) is sent, until the learner
//! confirms the new one. A server with a publicly trusted certificate needs no pin: the system's
//! trust store checks it.
//!
//! The pinned certificate is still made to prove it holds its key: the handshake's signatures are
//! checked against it as for any certificate. Only the chain to a trusted authority, and the name
//! in it, are replaced by the fingerprint.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Code, Error, Result};

/// A certificate's SHA-256 fingerprint, written as the Proxmox web interface shows it: upper-case
/// hex pairs joined by colons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of the certificate `der` (its DER bytes).
    pub fn of(der: &[u8]) -> Fingerprint {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&Sha256::digest(der));
        Fingerprint(bytes)
    }

    /// A fingerprint as typed or stored: 64 hex digits, with or without colons, in any case.
    pub fn parse(text: &str) -> Option<Fingerprint> {
        let hex: String = text.chars().filter(|c| *c != ':' && !c.is_whitespace()).collect();
        if hex.len() != 64 || !hex.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Fingerprint(bytes))
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pairs: Vec<String> = self.0.iter().map(|b| format!("{b:02X}")).collect();
        f.write_str(&pairs.join(":"))
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Fingerprint::parse(&text).ok_or_else(|| serde::de::Error::custom("not a SHA-256 certificate fingerprint"))
    }
}

/// How a server's certificate is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// By the system's trust store, as any HTTPS site.
    System,
    /// By its fingerprint alone, which the learner confirmed; None before they have.
    Pinned(Option<Fingerprint>),
}

/// The certificate a connection was offered, kept by the verifier for the error that follows a
/// refusal.
type Seen = Arc<Mutex<Option<Fingerprint>>>;

/// Accepts the one certificate whose fingerprint is `pin` (none before the learner confirmed
/// one), and records every certificate it is offered in `seen`.
#[derive(Debug)]
struct PinVerifier {
    pin: Option<Fingerprint>,
    seen: Seen,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let presented = Fingerprint::of(end_entity.as_ref());
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(presented);
        if self.pin == Some(presented) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("the server's certificate isn't the one confirmed for it".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// An HTTP client for one server, and the certificate it was last offered.
pub struct Client {
    /// The client.
    pub http: reqwest::Client,
    /// What a pinned client's verifier saw; None for one the system's store checks.
    seen: Option<Seen>,
    /// The pin it checks against.
    pin: Option<Fingerprint>,
}

impl Client {
    /// A client trusting the server as `trust` says, giving up after `timeout` (a connect after 15 s
    /// at most).
    pub fn new(trust: Trust, timeout: Duration) -> Result<Client> {
        Client::built_with(trust, reqwest::Client::builder().connect_timeout(Duration::from_secs(15).min(timeout)).timeout(timeout))
    }

    /// A client for a websocket upgrade: HTTP/1.1, a connect given up after 15 s, and no overall
    /// limit (the upgraded stream lives as long as its window).
    pub fn upgrade(trust: Trust) -> Result<Client> {
        Client::built_with(trust, reqwest::Client::builder().http1_only().connect_timeout(Duration::from_secs(15)))
    }

    fn built_with(trust: Trust, builder: reqwest::ClientBuilder) -> Result<Client> {
        let Trust::Pinned(pin) = trust else {
            return Ok(Client { http: built(builder)?, seen: None, pin: None });
        };
        let seen = Seen::default();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = PinVerifier { pin, seen: Arc::clone(&seen), algorithms: provider.signature_verification_algorithms };
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::Invalid(format!("couldn't set up TLS: {e}")))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Client { http: built(builder.tls_backend_preconfigured(config))?, seen: Some(seen), pin })
    }

    /// Why a request of this client failed on the server's certificate: none confirmed yet, or
    /// not the confirmed one. None when the certificate isn't why (or none was offered).
    pub fn refused(&self) -> Option<Refusal> {
        let presented = (*self.seen.as_ref()?.lock().unwrap_or_else(PoisonError::into_inner))?;
        match self.pin {
            None => Some(Refusal::Unconfirmed { presented }),
            Some(pinned) if pinned != presented => Some(Refusal::Changed { pinned, presented }),
            Some(_) => None,
        }
    }
}

fn built(b: reqwest::ClientBuilder) -> Result<reqwest::Client> {
    b.build().map_err(|e| Error::Invalid(format!("couldn't set up HTTP: {e}")))
}

/// Why a pinned client refused a server's certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No certificate was confirmed for the server yet: this is the one it offers.
    Unconfirmed {
        /// The certificate the server offered.
        presented: Fingerprint,
    },
    /// The server offered another certificate than the one confirmed.
    Changed {
        /// The certificate confirmed for it.
        pinned: Fingerprint,
        /// The one it offered.
        presented: Fingerprint,
    },
}

impl Refusal {
    /// The error the learner sees for server `server` (its name).
    pub fn error(self, server: &str) -> Error {
        Error::coded(match self {
            Refusal::Unconfirmed { presented } => Code::TlsUnconfirmed { server: server.to_owned(), presented: presented.to_string() },
            Refusal::Changed { pinned, presented } => {
                Code::TlsChanged { server: server.to_owned(), pinned: pinned.to_string(), presented: presented.to_string() }
            }
        })
    }
}

/// The fingerprint of the certificate `host` offers on `port`, for the learner to confirm: read
/// from the handshake alone, which is ended before any request (nothing is sent to the server).
pub async fn fetch(host: &str, port: u16, timeout: Duration) -> Result<Fingerprint> {
    let client = Client::new(Trust::Pinned(None), timeout)?;
    let url = format!("https://{}:{port}/", if host.contains(':') { format!("[{host}]") } else { host.to_owned() });
    let answer = client.http.get(url).send().await;
    match (client.refused(), answer) {
        (Some(Refusal::Unconfirmed { presented } | Refusal::Changed { presented, .. }), _) => Ok(presented),
        (None, Err(e)) => Err(Error::Invalid(format!("couldn't read the certificate of {host}:{port}: {}", chain(&e)))),
        (None, Ok(_)) => Err(Error::Invalid(format!("{host}:{port} offered no certificate"))),
    }
}

/// An error with its causes, on one line.
fn chain(e: &dyn std::error::Error) -> String {
    std::iter::successors(Some(e), |e| e.source()).map(ToString::to_string).collect::<Vec<_>>().join(": ")
}

#[cfg(test)]
pub(crate) mod tests;
